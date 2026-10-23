//! The bookmark access model (paperless v4 `internal/authz`, after its
//! tenant-grant fix, without category inheritance; relations keep v3's
//! meaning):
//!
//! - a **grant** binds a subject (a user, a role, or the whole tenant) to a
//!   bookmark with a relation: owner (read, write, delete, share), editor
//!   (read, write), sharer (read, share) or viewer (read), optionally until
//!   an expiry;
//! - a tenant grant with no bookmark (`resource_id == ""`) covers every
//!   bookmark of the tenant; a tenant grant on one bookmark stays on that
//!   bookmark;
//! - callers holding the tenant role `admin` or `owner` hold every
//!   permission on every bookmark of their tenant (paperless `IsAdmin`);
//! - grant management is capped at the caller's level: viewer and sharer
//!   grants need share, editor and owner grants need every permission
//!   (paperless `permissions.canHandle`).
//!
//! This module is pure: the store fetches the caller's grants (by subject,
//! not expired) and the handlers combine them here.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The only resource type (kept in the API and backups for paperless
/// parity).
pub const RESOURCE: &str = "bookmark";

/// A grant's relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Relation {
    /// Read, write, delete, share.
    Owner,
    /// Read, write.
    Editor,
    /// Read, share.
    Sharer,
    /// Read.
    Viewer,
}

impl Relation {
    /// Every relation.
    pub const ALL: [Self; 4] = [Self::Owner, Self::Editor, Self::Sharer, Self::Viewer];

    /// The stored name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Editor => "editor",
            Self::Sharer => "sharer",
            Self::Viewer => "viewer",
        }
    }

    /// Parses a stored name.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str() == s)
    }

    /// What the relation confers (v3 `Relation::grants`).
    pub fn permissions(self) -> Permissions {
        match self {
            Self::Owner => Permissions::ALL,
            Self::Editor => Permissions {
                read: true,
                write: true,
                ..Permissions::NONE
            },
            Self::Sharer => Permissions {
                read: true,
                share: true,
                ..Permissions::NONE
            },
            Self::Viewer => Permissions {
                read: true,
                ..Permissions::NONE
            },
        }
    }

    /// v3's hierarchy level (owner 4 > editor 3 > sharer 2 > viewer 1).
    pub fn rank(self) -> u8 {
        match self {
            Self::Owner => 4,
            Self::Editor => 3,
            Self::Sharer => 2,
            Self::Viewer => 1,
        }
    }

    /// Whether granting or revoking this relation needs every permission
    /// (paperless `canHandle`).
    pub fn needs_full_control(self) -> bool {
        matches!(self, Self::Owner | Self::Editor)
    }
}

/// A checkable action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Read the bookmark (and see it listed).
    Read,
    /// Change it.
    Write,
    /// Remove it.
    Delete,
    /// Grant and revoke access.
    Share,
}

impl Action {
    /// The API name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Delete => "delete",
            Self::Share => "share",
        }
    }

    /// Parses an API name.
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Read, Self::Write, Self::Delete, Self::Share]
            .into_iter()
            .find(|a| a.as_str() == s)
    }
}

/// The set of actions a caller holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Permissions {
    /// Read.
    pub read: bool,
    /// Write.
    pub write: bool,
    /// Delete.
    pub delete: bool,
    /// Share.
    pub share: bool,
}

impl Permissions {
    /// Nothing.
    pub const NONE: Self = Self {
        read: false,
        write: false,
        delete: false,
        share: false,
    };
    /// Everything (owner, tenant admin).
    pub const ALL: Self = Self {
        read: true,
        write: true,
        delete: true,
        share: true,
    };

    /// Whether `a` is held.
    pub fn has(self, a: Action) -> bool {
        match a {
            Action::Read => self.read,
            Action::Write => self.write,
            Action::Delete => self.delete,
            Action::Share => self.share,
        }
    }

    /// The union.
    #[must_use]
    pub fn union(self, o: Self) -> Self {
        Self {
            read: self.read || o.read,
            write: self.write || o.write,
            delete: self.delete || o.delete,
            share: self.share || o.share,
        }
    }
}

/// A grant's subject kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubjectType {
    /// One user (`subject_id` = user ID).
    User,
    /// Holders of a role (`subject_id` = role slug).
    Role,
    /// Everyone in the tenant (`subject_id` = "").
    Tenant,
}

impl SubjectType {
    /// The stored name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Role => "role",
            Self::Tenant => "tenant",
        }
    }

    /// Parses a stored name.
    pub fn parse(s: &str) -> Option<Self> {
        [Self::User, Self::Role, Self::Tenant]
            .into_iter()
            .find(|t| t.as_str() == s)
    }
}

/// The caller (paperless `Subjects`): from the verified platform token.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Subjects {
    /// The caller's tenant.
    pub tenant_id: String,
    /// The caller's user ID.
    pub user_id: String,
    /// The caller's tenant roles.
    pub roles: Vec<String>,
}

impl Subjects {
    /// Whether the caller is a tenant super-user: the platform roles `admin`
    /// and `owner` both confer full control of the tenant's bookmarks
    /// (paperless `Subjects.IsAdmin`).
    pub fn is_admin(&self) -> bool {
        self.roles.iter().any(|r| r == "admin" || r == "owner")
    }
}

/// A stored grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Grant {
    /// Grant ID.
    pub id: uuid::Uuid,
    /// Always [`RESOURCE`].
    pub resource_type: &'static str,
    /// The bookmark ID, or "" for every bookmark of the tenant.
    pub resource_id: String,
    /// Subject kind.
    pub subject_type: SubjectType,
    /// User ID, role slug, or "" for the tenant.
    pub subject_id: String,
    /// Relation.
    pub relation: Relation,
    /// Who granted it.
    pub granted_by: String,
    /// When.
    pub granted_at: DateTime<Utc>,
    /// Until when (none = no expiry).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

impl Grant {
    /// Whether the grant covers the whole tenant: a tenant subject with no
    /// bookmark (paperless `tenantWide`).
    pub fn tenant_wide(&self) -> bool {
        self.subject_type == SubjectType::Tenant && self.resource_id.is_empty()
    }

    /// Whether the grant is still in force at `now`.
    pub fn active(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_none_or(|e| e > now)
    }

    /// Whether the grant's subject is the caller.
    pub fn names(&self, s: &Subjects) -> bool {
        match self.subject_type {
            SubjectType::User => !s.user_id.is_empty() && self.subject_id == s.user_id,
            SubjectType::Role => s.roles.contains(&self.subject_id),
            SubjectType::Tenant => true,
        }
    }

    /// Whether the grant gives the caller access to `resource_id` ("" = the
    /// collection) at `now`.
    pub fn applies(&self, s: &Subjects, resource_id: &str, now: DateTime<Utc>) -> bool {
        self.active(now)
            && self.names(s)
            && (self.tenant_wide() || (!resource_id.is_empty() && self.resource_id == resource_id))
    }
}

/// The caller's access to one bookmark (or the collection).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    /// What the caller holds.
    pub permissions: Permissions,
    /// The highest relation among the applying grants.
    pub relation: Option<Relation>,
    /// The caller is a tenant admin/owner.
    pub admin: bool,
}

impl Effective {
    /// Whether `a` is held.
    pub fn has(&self, a: Action) -> bool {
        self.permissions.has(a)
    }

    /// Whether every permission is held (owner level).
    pub fn full_control(&self) -> bool {
        self.permissions == Permissions::ALL
    }
}

/// Combines `grants` (the caller's, as fetched) for `resource_id` ("" = the
/// collection: tenant-wide grants only) at `now` (paperless `Effective`).
pub fn effective(s: &Subjects, grants: &[Grant], resource_id: &str, now: DateTime<Utc>) -> Effective {
    let mut perms = Permissions::NONE;
    let mut relation: Option<Relation> = None;
    for g in grants.iter().filter(|g| g.applies(s, resource_id, now)) {
        perms = perms.union(g.relation.permissions());
        if relation.is_none_or(|r| g.relation.rank() > r.rank()) {
            relation = Some(g.relation);
        }
    }
    let admin = s.is_admin();
    if admin {
        perms = Permissions::ALL;
    }
    Effective {
        permissions: perms,
        relation,
        admin,
    }
}

/// Permissions from the relations a list row carries (the store already
/// matched subject, expiry and resource).
pub fn from_relations(s: &Subjects, relations: &[Relation]) -> Permissions {
    if s.is_admin() {
        return Permissions::ALL;
    }
    relations
        .iter()
        .fold(Permissions::NONE, |p, r| p.union(r.permissions()))
}

/// Whether a caller with `held` may grant or revoke `relation` (paperless
/// `canHandle`, after the share check).
pub fn may_handle(held: &Effective, relation: Relation) -> bool {
    held.has(Action::Share) && (!relation.needs_full_control() || held.full_control())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    const B1: &str = "0192f6b0-0000-7000-8000-000000000001";
    const B2: &str = "0192f6b0-0000-7000-8000-000000000002";

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-16T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn grant(resource: &str, st: SubjectType, sid: &str, rel: Relation) -> Grant {
        Grant {
            id: uuid::Uuid::now_v7(),
            resource_type: RESOURCE,
            resource_id: resource.into(),
            subject_type: st,
            subject_id: sid.into(),
            relation: rel,
            granted_by: "u-owner".into(),
            granted_at: now(),
            expires_at: None,
        }
    }

    fn user(id: &str, roles: &[&str]) -> Subjects {
        Subjects {
            tenant_id: "t1".into(),
            user_id: id.into(),
            roles: roles.iter().map(|r| (*r).to_owned()).collect(),
        }
    }

    #[test]
    fn relation_matrix() {
        let rows = [
            (Relation::Owner, [true, true, true, true]),
            (Relation::Editor, [true, true, false, false]),
            (Relation::Sharer, [true, false, false, true]),
            (Relation::Viewer, [true, false, false, false]),
        ];
        for (rel, want) in rows {
            let p = rel.permissions();
            let got = [
                p.has(Action::Read),
                p.has(Action::Write),
                p.has(Action::Delete),
                p.has(Action::Share),
            ];
            assert_eq!(got, want, "{rel:?}");
            assert_eq!(Relation::parse(rel.as_str()), Some(rel));
        }
        assert_eq!(Relation::parse("RELATION_OWNER"), None);
        assert_eq!(Action::parse("download"), None);
        assert_eq!(SubjectType::parse("group"), None);
    }

    #[test]
    fn direct_role_and_union() {
        let gs = vec![
            grant(B1, SubjectType::User, "u-ana", Relation::Viewer),
            grant(B1, SubjectType::Role, "dev", Relation::Editor),
            grant(B2, SubjectType::User, "u-ana", Relation::Sharer),
        ];
        let ana = user("u-ana", &["member"]);
        let e = effective(&ana, &gs, B1, now());
        assert_eq!(e.permissions, Relation::Viewer.permissions());
        assert_eq!(e.relation, Some(Relation::Viewer));
        let ana_dev = user("u-ana", &["member", "dev"]);
        let e = effective(&ana_dev, &gs, B1, now());
        assert_eq!(
            e.permissions,
            Permissions {
                read: true,
                write: true,
                ..Permissions::NONE
            }
        );
        assert_eq!(e.relation, Some(Relation::Editor), "highest relation");
        // Grants on another bookmark never leak over.
        let e = effective(&ana, &gs, B2, now());
        assert_eq!(e.permissions, Relation::Sharer.permissions());
        let bob = user("u-bob", &["member"]);
        assert_eq!(effective(&bob, &gs, B1, now()).permissions, Permissions::NONE);
        // An empty user ID never matches a user grant.
        let anon = user("", &[]);
        let g = [grant(B1, SubjectType::User, "", Relation::Owner)];
        assert!(!g[0].names(&anon));
    }

    #[test]
    fn expiry() {
        let mut g = grant(B1, SubjectType::User, "u-ana", Relation::Owner);
        g.expires_at = Some(now() - Duration::seconds(1));
        let ana = user("u-ana", &[]);
        assert_eq!(
            effective(&ana, &[g.clone()], B1, now()).permissions,
            Permissions::NONE
        );
        g.expires_at = Some(now());
        assert!(!g.active(now()), "expiry is exclusive");
        g.expires_at = Some(now() + Duration::seconds(1));
        assert!(effective(&ana, &[g], B1, now()).full_control());
    }

    #[test]
    fn tenant_grants() {
        // A tenant grant on one bookmark stays on that bookmark (paperless PR #13).
        let on_b1 = grant(B1, SubjectType::Tenant, "", Relation::Viewer);
        let carol = user("u-carol", &[]);
        assert!(effective(&carol, std::slice::from_ref(&on_b1), B1, now()).has(Action::Read));
        assert!(!effective(&carol, std::slice::from_ref(&on_b1), B2, now()).has(Action::Read));
        assert!(
            !effective(&carol, std::slice::from_ref(&on_b1), "", now()).has(Action::Read),
            "not a collection grant"
        );
        assert!(!on_b1.tenant_wide());
        // A tenant grant without a bookmark covers the whole collection.
        let all = grant("", SubjectType::Tenant, "", Relation::Sharer);
        assert!(all.tenant_wide());
        for r in [B1, B2, ""] {
            let e = effective(&carol, std::slice::from_ref(&all), r, now());
            assert_eq!(e.permissions, Relation::Sharer.permissions(), "{r}");
        }
    }

    #[test]
    fn admin_short_circuit() {
        for role in ["admin", "owner"] {
            let a = user("u-admin", &[role]);
            assert!(a.is_admin());
            let e = effective(&a, &[], B1, now());
            assert!(e.full_control() && e.admin && e.relation.is_none());
            assert!(effective(&a, &[], "", now()).full_control());
            assert_eq!(from_relations(&a, &[]), Permissions::ALL);
        }
        for role in [
            "member",
            "auditor",
            "operator",
            "administrator",
            "bookmark.administrator",
        ] {
            assert!(!user("u", &[role]).is_admin(), "{role}");
        }
    }

    #[test]
    fn handling_is_capped_at_the_callers_level() {
        let sharer = effective(
            &user("u-s", &[]),
            &[grant(B1, SubjectType::User, "u-s", Relation::Sharer)],
            B1,
            now(),
        );
        assert!(may_handle(&sharer, Relation::Viewer));
        assert!(may_handle(&sharer, Relation::Sharer));
        assert!(!may_handle(&sharer, Relation::Editor));
        assert!(!may_handle(&sharer, Relation::Owner));
        let editor = effective(
            &user("u-e", &[]),
            &[grant(B1, SubjectType::User, "u-e", Relation::Editor)],
            B1,
            now(),
        );
        assert!(!may_handle(&editor, Relation::Viewer), "editors cannot share");
        let owner = effective(
            &user("u-o", &[]),
            &[grant(B1, SubjectType::User, "u-o", Relation::Owner)],
            B1,
            now(),
        );
        for r in Relation::ALL {
            assert!(may_handle(&owner, r));
        }
        let admin = effective(&user("u-a", &["admin"]), &[], B1, now());
        assert!(may_handle(&admin, Relation::Owner));
    }

    #[test]
    fn list_rows() {
        let s = user("u", &[]);
        assert_eq!(
            from_relations(&s, &[Relation::Viewer, Relation::Sharer]),
            Permissions {
                read: true,
                share: true,
                ..Permissions::NONE
            }
        );
        assert_eq!(from_relations(&s, &[]), Permissions::NONE);
    }
}
