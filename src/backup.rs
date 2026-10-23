//! Export and import of a tenant's bookmarks and grants (paperless v4
//! `internal/backup`): schema version 1, IDs preserved, per-tenant only,
//! duplicates skipped or overwritten. Bookmarks are deduplicated by ID,
//! grants by (resource, subject, relation). The whole import is validated
//! first and applied in one transaction.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::api::input::{self, Errors};
use crate::authz::{Grant, RESOURCE, Relation, SubjectType};
use crate::store::{self, Bookmark};

/// The export format version.
pub const SCHEMA_VERSION: i64 = 1;

/// A bookmark in a backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupBookmark {
    /// ID (kept on import).
    pub id: Uuid,
    /// URL.
    pub url: String,
    /// Title.
    #[serde(default)]
    pub title: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Creator.
    #[serde(default)]
    pub created_by: String,
    /// Created (import: now when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    /// Updated (import: created_at when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

/// A grant in a backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupGrant {
    /// ID (kept on import unless another grant holds it).
    pub id: Uuid,
    /// `bookmark`.
    pub resource_type: String,
    /// The bookmark ID, or "" for every bookmark.
    pub resource_id: String,
    /// `user`, `role` or `tenant`.
    pub subject_type: String,
    /// User ID, role slug, "" for the tenant.
    #[serde(default)]
    pub subject_id: String,
    /// `owner`, `editor`, `sharer` or `viewer`.
    pub relation: String,
    /// Granter.
    #[serde(default)]
    pub granted_by: String,
    /// When (import: now when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granted_at: Option<DateTime<Utc>>,
    /// Expiry (kept as is, also when past).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

/// The export document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Backup {
    /// [`SCHEMA_VERSION`].
    pub schema_version: i64,
    /// `bookmark`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub module: String,
    /// When it was taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exported_at: Option<DateTime<Utc>>,
    /// Bookmarks.
    pub bookmarks: Vec<BackupBookmark>,
    /// Grants.
    pub permissions: Vec<BackupGrant>,
}

/// What an import did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ImportResult {
    /// Bookmarks created or replaced.
    pub bookmarks_imported: u64,
    /// Bookmarks kept as they were (skip mode).
    pub bookmarks_skipped: u64,
    /// Grants created or replaced.
    pub permissions_imported: u64,
    /// Grants kept (skip mode) or whose bookmark is in neither the file nor
    /// the tenant.
    pub permissions_skipped: u64,
}

/// Import modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Keep what exists (default).
    Skip,
    /// Replace what exists.
    Overwrite,
}

impl Mode {
    /// Parses `skip` / `overwrite`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "skip" => Some(Self::Skip),
            "overwrite" => Some(Self::Overwrite),
            _ => None,
        }
    }

    /// The API name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Overwrite => "overwrite",
        }
    }
}

/// Builds the export of `tenant`.
pub async fn export(c: &mut PgConnection, tenant: &str, now: DateTime<Utc>) -> Result<Backup, sqlx::Error> {
    let bookmarks = store::all_bookmarks(&mut *c, tenant)
        .await?
        .into_iter()
        .map(|b| BackupBookmark {
            id: b.id,
            url: b.url,
            title: b.title,
            description: b.description,
            tags: b.tags,
            created_by: b.created_by,
            created_at: Some(b.created_at),
            updated_at: Some(b.updated_at),
        })
        .collect();
    let permissions = store::all_grants(&mut *c, tenant)
        .await?
        .into_iter()
        .map(|g| BackupGrant {
            id: g.id,
            resource_type: RESOURCE.into(),
            resource_id: g.resource_id,
            subject_type: g.subject_type.as_str().into(),
            subject_id: g.subject_id,
            relation: g.relation.as_str().into(),
            granted_by: g.granted_by,
            granted_at: Some(g.granted_at),
            expires_at: g.expires_at,
        })
        .collect();
    Ok(Backup {
        schema_version: SCHEMA_VERSION,
        module: crate::manifest::MODULE.into(),
        exported_at: Some(now),
        bookmarks,
        permissions,
    })
}

/// A validated backup, ready to write.
#[derive(Debug, Clone)]
pub struct Validated {
    bookmarks: Vec<Bookmark>,
    grants: Vec<Grant>,
}

impl Validated {
    /// (bookmarks, grants) in the file.
    pub fn counts(&self) -> (usize, usize) {
        (self.bookmarks.len(), self.grants.len())
    }
}

fn entry_error(e: &mut Errors, kind: &str, index: usize, field: &str, code: &str) {
    e.set(&format!("{kind}[{index}].{field}"), code);
}

/// Checks `b` with the create rules (schema, URLs, lengths, tags, subjects,
/// relations); the first problems are reported as
/// `detail.fields["bookmarks[3].url"] = "scheme"`.
pub fn validate(
    b: &Backup,
    actor: &str,
    now: DateTime<Utc>,
) -> Result<Validated, tangra_sdk::error::ApiError> {
    let mut e = Errors::default();
    if b.schema_version != SCHEMA_VERSION {
        e.set("schema_version", "invalid");
        return e.finish(Validated {
            bookmarks: Vec::new(),
            grants: Vec::new(),
        });
    }
    if !b.module.is_empty() && b.module != crate::manifest::MODULE {
        e.set("module", "invalid");
    }
    let mut bookmarks = Vec::with_capacity(b.bookmarks.len());
    for (i, x) in b.bookmarks.iter().enumerate() {
        let url = input::url(&x.url).unwrap_or_else(|c| {
            entry_error(&mut e, "bookmarks", i, "url", c);
            String::new()
        });
        let title = input::text(&x.title, input::MAX_TITLE, false).unwrap_or_else(|c| {
            entry_error(&mut e, "bookmarks", i, "title", c);
            String::new()
        });
        let description = input::text(&x.description, input::MAX_DESCRIPTION, true).unwrap_or_else(|c| {
            entry_error(&mut e, "bookmarks", i, "description", c);
            String::new()
        });
        let tags = input::tags(&x.tags).unwrap_or_else(|c| {
            entry_error(&mut e, "bookmarks", i, "tags", c);
            Vec::new()
        });
        let created_at = x.created_at.unwrap_or(now);
        bookmarks.push(Bookmark {
            id: x.id,
            url,
            title,
            description,
            tags,
            created_by: x.created_by.clone(),
            created_at,
            updated_at: x.updated_at.unwrap_or(created_at),
        });
    }
    let mut grants = Vec::with_capacity(b.permissions.len());
    for (i, x) in b.permissions.iter().enumerate() {
        if x.resource_type != RESOURCE {
            entry_error(&mut e, "permissions", i, "resource_type", "invalid");
        }
        let resource_id = if x.resource_id.is_empty() {
            String::new()
        } else {
            match input::bookmark_id(&x.resource_id) {
                Some(id) => id.to_string(),
                None => {
                    entry_error(&mut e, "permissions", i, "resource_id", "invalid");
                    String::new()
                }
            }
        };
        let relation = Relation::parse(&x.relation);
        if relation.is_none() {
            entry_error(&mut e, "permissions", i, "relation", "invalid");
        }
        let st = SubjectType::parse(&x.subject_type);
        let mut subject_id = String::new();
        match st {
            None => entry_error(&mut e, "permissions", i, "subject_type", "invalid"),
            Some(st) => {
                match input::subject_id(st, &x.subject_id) {
                    Ok(s) => subject_id = s,
                    Err(c) => entry_error(&mut e, "permissions", i, "subject_id", c),
                }
                if resource_id.is_empty() && st != SubjectType::Tenant {
                    entry_error(&mut e, "permissions", i, "resource_id", "required");
                }
            }
        }
        if let (Some(subject_type), Some(relation)) = (st, relation) {
            grants.push(Grant {
                id: x.id,
                resource_type: RESOURCE,
                resource_id,
                subject_type,
                subject_id,
                relation,
                granted_by: if x.granted_by.is_empty() {
                    actor.to_owned()
                } else {
                    x.granted_by.clone()
                },
                granted_at: x.granted_at.unwrap_or(now),
                expires_at: x.expires_at,
            });
        }
    }
    e.finish(Validated { bookmarks, grants })
}

/// Writes `v` into `tenant` (on a transaction's connection).
pub async fn import(
    c: &mut PgConnection,
    tenant: &str,
    v: Validated,
    mode: Mode,
) -> Result<ImportResult, sqlx::Error> {
    let mut res = ImportResult::default();
    for b in &v.bookmarks {
        if store::bookmark_exists(&mut *c, tenant, b.id).await? {
            if mode == Mode::Skip {
                res.bookmarks_skipped += 1;
                continue;
            }
            store::update_bookmark(&mut *c, tenant, b).await?;
        } else {
            store::insert_bookmark(&mut *c, tenant, b).await?;
        }
        res.bookmarks_imported += 1;
    }
    for g in v.grants {
        if !g.resource_id.is_empty() {
            let exists = match Uuid::parse_str(&g.resource_id) {
                Ok(id) => store::bookmark_exists(&mut *c, tenant, id).await?,
                Err(_) => false,
            };
            if !exists {
                res.permissions_skipped += 1;
                continue;
            }
        }
        if let Some(old) = store::find_grant(
            &mut *c,
            tenant,
            &g.resource_id,
            g.subject_type,
            &g.subject_id,
            g.relation,
        )
        .await?
        {
            if mode == Mode::Skip {
                res.permissions_skipped += 1;
                continue;
            }
            store::delete_grant(&mut *c, tenant, old.id).await?;
        }
        let mut g = g;
        if store::get_grant(&mut *c, tenant, g.id).await?.is_some() {
            // The ID belongs to a different grant: keep both.
            g.id = Uuid::now_v7();
        }
        store::insert_grant(&mut *c, tenant, &g).await?;
        res.permissions_imported += 1;
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn backup(v: Value) -> Backup {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn validation() {
        let now = Utc::now();
        let id = Uuid::now_v7();
        let ok = backup(json!({
            "schema_version": 1, "module": "bookmark",
            "bookmarks": [{"id": id, "url": "https://go.dev", "tags": ["go", "go"]}],
            "permissions": [
                {"id": Uuid::now_v7(), "resource_type": "bookmark", "resource_id": id.to_string(), "subject_type": "role", "subject_id": "dev", "relation": "viewer"},
                {"id": Uuid::now_v7(), "resource_type": "bookmark", "resource_id": "", "subject_type": "tenant", "relation": "viewer"}
            ]
        }));
        let v = validate(&ok, "u-actor", now).unwrap();
        assert_eq!(v.counts(), (1, 2));
        assert_eq!(v.bookmarks[0].tags, ["go"]);
        assert_eq!(v.bookmarks[0].created_at, now);
        assert_eq!(v.grants[0].granted_by, "u-actor");

        let mut bad = ok.clone();
        bad.schema_version = 2;
        let e = validate(&bad, "u", now).unwrap_err();
        assert!(e.body().contains("schema_version"));
        let bad = backup(json!({
            "schema_version": 1,
            "bookmarks": [{"id": id, "url": "javascript:x"}],
            "permissions": [{"id": Uuid::now_v7(), "resource_type": "bookmark", "resource_id": "", "subject_type": "user", "subject_id": "x", "relation": "boss"}]
        }));
        let e = validate(&bad, "u", now).unwrap_err();
        let body: Value = serde_json::from_str(&e.body()).unwrap();
        assert_eq!(
            body["detail"]["fields"],
            json!({
                "bookmarks[0].url": "scheme",
                "permissions[0].relation": "invalid",
                "permissions[0].subject_id": "invalid",
                "permissions[0].resource_id": "required"
            })
        );
        assert!(
            serde_json::from_value::<Backup>(
                json!({"schema_version": 1, "bookmarks": [], "permissions": [], "extra": 1})
            )
            .is_err()
        );
        assert_eq!(Mode::parse("merge"), None);
    }
}
