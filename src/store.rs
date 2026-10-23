//! PostgreSQL storage (`migrations/`, specs/038-bookmark-v4/data-model.md).
//!
//! Every function runs on a connection of a transaction opened with
//! [`tangra_sdk::db::begin`] for the caller's tenant (row-level security),
//! and every statement still names `tenant_id = $1`. ORDER BY clauses come
//! only from the list specs' constants ([`bookmark_spec`], [`grant_spec`]).

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::migrate::Migrator;
use sqlx::postgres::PgRow;
use sqlx::{PgConnection, Row};
use tangra_sdk::listquery::{Dir, Field, Request, Spec};
use uuid::Uuid;

use crate::authz::{Grant, RESOURCE, Relation, SubjectType, Subjects};

/// The embedded migrations (`bookmarksvc bootstrap` and start-up).
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// A stored bookmark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    /// ID.
    pub id: Uuid,
    /// Absolute http(s) URL.
    pub url: String,
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Tags (normalized: trimmed, unique, in the given order).
    pub tags: Vec<String>,
    /// The creator's user ID.
    pub created_by: String,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

/// The bookmark list's sort allow-list (`GET /api/bookmark/v1/bookmarks`).
pub fn bookmark_spec() -> Spec {
    let f = |expr, text, dir| Field {
        expr,
        text,
        default_dir: dir,
        not_null: true,
    };
    Spec {
        fields: [
            ("created_at", f("b.created_at", false, Some(Dir::Desc))),
            ("updated_at", f("b.updated_at", false, Some(Dir::Desc))),
            ("title", f("b.title", true, None)),
            ("url", f("b.url", true, None)),
        ]
        .into_iter()
        .collect(),
        default: "created_at",
        tie_break: "b.id",
        default_size: 0,
        max_size: 0,
    }
}

/// The grant list's sort allow-list (`GET /api/bookmark/v1/permissions`).
pub fn grant_spec() -> Spec {
    let f = |expr, dir| Field {
        expr,
        text: false,
        default_dir: dir,
        not_null: true,
    };
    Spec {
        fields: [
            ("granted_at", f("p.granted_at", Some(Dir::Desc))),
            ("relation", f("p.relation", None)),
            ("subject_type", f("p.subject_type", None)),
        ]
        .into_iter()
        .collect(),
        default: "granted_at",
        tie_break: "p.id",
        default_size: 0,
        max_size: 0,
    }
}

fn tenant_uuid(tenant: &str) -> Result<Uuid, sqlx::Error> {
    Uuid::parse_str(tenant).map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

const BOOKMARK_COLS: &str =
    "b.id, b.url, b.title, b.description, b.tags, b.created_by, b.created_at, b.updated_at";

fn bookmark_row(r: &PgRow) -> Result<Bookmark, sqlx::Error> {
    Ok(Bookmark {
        id: r.try_get("id")?,
        url: r.try_get("url")?,
        title: r.try_get("title")?,
        description: r.try_get("description")?,
        tags: r.try_get("tags")?,
        created_by: r.try_get("created_by")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

/// Inserts `b` (with its own ID and times).
pub async fn insert_bookmark(c: &mut PgConnection, tenant: &str, b: &Bookmark) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO bookmark_bookmarks (tenant_id, id, url, title, description, tags, created_by, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(tenant_uuid(tenant)?)
    .bind(b.id)
    .bind(&b.url)
    .bind(&b.title)
    .bind(&b.description)
    .bind(&b.tags)
    .bind(&b.created_by)
    .bind(b.created_at)
    .bind(b.updated_at)
    .execute(c)
    .await?;
    Ok(())
}

/// The bookmark `id` of `tenant`.
pub async fn get_bookmark(
    c: &mut PgConnection,
    tenant: &str,
    id: Uuid,
) -> Result<Option<Bookmark>, sqlx::Error> {
    let row = sqlx::query(&format!(
        "SELECT {BOOKMARK_COLS} FROM bookmark_bookmarks b WHERE b.tenant_id = $1 AND b.id = $2"
    ))
    .bind(tenant_uuid(tenant)?)
    .bind(id)
    .fetch_optional(c)
    .await?;
    row.as_ref().map(bookmark_row).transpose()
}

/// Replaces every field of `b` (by tenant and ID); false when absent.
pub async fn update_bookmark(c: &mut PgConnection, tenant: &str, b: &Bookmark) -> Result<bool, sqlx::Error> {
    let r = sqlx::query(
        "UPDATE bookmark_bookmarks SET url = $3, title = $4, description = $5, tags = $6,
                created_by = $7, created_at = $8, updated_at = $9
         WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_uuid(tenant)?)
    .bind(b.id)
    .bind(&b.url)
    .bind(&b.title)
    .bind(&b.description)
    .bind(&b.tags)
    .bind(&b.created_by)
    .bind(b.created_at)
    .bind(b.updated_at)
    .execute(c)
    .await?;
    Ok(r.rows_affected() == 1)
}

/// Removes the bookmark and its grants; returns (removed, grants removed).
pub async fn delete_bookmark(
    c: &mut PgConnection,
    tenant: &str,
    id: Uuid,
) -> Result<(bool, u64), sqlx::Error> {
    let t = tenant_uuid(tenant)?;
    let grants = sqlx::query(
        "DELETE FROM bookmark_permissions WHERE tenant_id = $1 AND resource_type = 'bookmark' AND resource_id = $2",
    )
    .bind(t)
    .bind(id.to_string())
    .execute(&mut *c)
    .await?
    .rows_affected();
    let r = sqlx::query("DELETE FROM bookmark_bookmarks WHERE tenant_id = $1 AND id = $2")
        .bind(t)
        .bind(id)
        .execute(&mut *c)
        .await?;
    Ok((r.rows_affected() == 1, grants))
}

/// Whether the bookmark exists in the tenant.
pub async fn bookmark_exists(c: &mut PgConnection, tenant: &str, id: Uuid) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("SELECT 1 FROM bookmark_bookmarks WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_uuid(tenant)?)
        .bind(id)
        .fetch_optional(c)
        .await?;
    Ok(r.is_some())
}

/// The list filter.
#[derive(Debug, Clone, Default)]
pub struct BookmarkFilter {
    /// Every tag must be present.
    pub tags: Vec<String>,
    /// Case-insensitive substring of the title or URL ("" = none).
    pub q: String,
}

/// `%q%` with LIKE's metacharacters escaped (`ESCAPE '\'`).
fn like_pattern(q: &str) -> String {
    let mut s = String::with_capacity(q.len() + 2);
    s.push('%');
    for ch in q.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            s.push('\\');
        }
        s.push(ch);
    }
    s.push('%');
    s
}

/// The grants matching the caller (`$2` now, `$3` user, `$4` roles) on
/// bookmark `b`, or on the whole collection.
const APPLYING: &str = "p.tenant_id = b.tenant_id AND p.resource_type = 'bookmark'
       AND (p.resource_id = b.id::text OR (p.resource_id = '' AND p.subject_type = 'tenant'))
       AND (p.expires_at IS NULL OR p.expires_at > $2)
       AND ((p.subject_type = 'user' AND p.subject_id = $3 AND $3 <> '')
            OR (p.subject_type = 'role' AND p.subject_id = ANY($4))
            OR p.subject_type = 'tenant')";

/// One page of the bookmarks `s` can read (all of the tenant for admins),
/// each with the relations that apply to the caller, and the exact total.
/// A page beyond the end answers the last page (`req` is clamped and the
/// served request returned).
pub async fn list_bookmarks(
    c: &mut PgConnection,
    s: &Subjects,
    f: &BookmarkFilter,
    req: &Request,
    spec: &Spec,
    now: DateTime<Utc>,
) -> Result<(Vec<(Bookmark, Vec<Relation>)>, i64, Request), sqlx::Error> {
    let t = tenant_uuid(&s.tenant_id)?;
    let pattern = if f.q.is_empty() {
        String::new()
    } else {
        like_pattern(&f.q)
    };
    let from = format!(
        "FROM bookmark_bookmarks b
         LEFT JOIN LATERAL (
           SELECT array_agg(DISTINCT p.relation) AS relations FROM bookmark_permissions p WHERE {APPLYING}
         ) g ON true
         WHERE b.tenant_id = $1
           AND ($5 OR g.relations IS NOT NULL)
           AND (cardinality($6::text[]) = 0 OR b.tags @> $6::text[])
           AND ($7 = '' OR b.title ILIKE $7 ESCAPE '\\' OR b.url ILIKE $7 ESCAPE '\\')"
    );
    let total: i64 = sqlx::query(&format!("SELECT count(*) AS n {from}"))
        .bind(t)
        .bind(now)
        .bind(&s.user_id)
        .bind(&s.roles)
        .bind(s.is_admin())
        .bind(&f.tags)
        .bind(&pattern)
        .fetch_one(&mut *c)
        .await?
        .try_get("n")?;
    let served = req.clamp(total);
    let rows = sqlx::query(&format!(
        "SELECT {BOOKMARK_COLS}, COALESCE(g.relations, '{{}}') AS relations {from}
         ORDER BY {} LIMIT $8 OFFSET $9",
        served.order_by(spec)
    ))
    .bind(t)
    .bind(now)
    .bind(&s.user_id)
    .bind(&s.roles)
    .bind(s.is_admin())
    .bind(&f.tags)
    .bind(&pattern)
    .bind(served.limit())
    .bind(served.offset())
    .fetch_all(&mut *c)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let rels: Vec<String> = r.try_get("relations")?;
        let rels = rels.iter().filter_map(|x| Relation::parse(x)).collect();
        out.push((bookmark_row(r)?, rels));
    }
    Ok((out, total, served))
}

/// Every bookmark of the tenant (backup export), oldest first.
pub async fn all_bookmarks(c: &mut PgConnection, tenant: &str) -> Result<Vec<Bookmark>, sqlx::Error> {
    let rows = sqlx::query(&format!(
        "SELECT {BOOKMARK_COLS} FROM bookmark_bookmarks b WHERE b.tenant_id = $1 ORDER BY b.created_at, b.id"
    ))
    .bind(tenant_uuid(tenant)?)
    .fetch_all(c)
    .await?;
    rows.iter().map(bookmark_row).collect()
}

// ---- grants ----

const GRANT_COLS: &str =
    "p.id, p.resource_id, p.subject_type, p.subject_id, p.relation, p.granted_by, p.granted_at, p.expires_at";

fn grant_row(r: &PgRow) -> Result<Grant, sqlx::Error> {
    let bad = |what: &str| sqlx::Error::Protocol(format!("bookmark_permissions: bad {what}"));
    let st: String = r.try_get("subject_type")?;
    let rel: String = r.try_get("relation")?;
    Ok(Grant {
        id: r.try_get("id")?,
        resource_type: RESOURCE,
        resource_id: r.try_get("resource_id")?,
        subject_type: SubjectType::parse(&st).ok_or_else(|| bad("subject_type"))?,
        subject_id: r.try_get("subject_id")?,
        relation: Relation::parse(&rel).ok_or_else(|| bad("relation"))?,
        granted_by: r.try_get("granted_by")?,
        granted_at: r.try_get("granted_at")?,
        expires_at: r.try_get("expires_at")?,
    })
}

/// The caller's grants in force at `now`: their user, any of their roles,
/// the tenant (paperless `GrantsForSubjects`).
pub async fn grants_for_subjects(
    c: &mut PgConnection,
    s: &Subjects,
    now: DateTime<Utc>,
) -> Result<Vec<Grant>, sqlx::Error> {
    let rows = sqlx::query(&format!(
        "SELECT {GRANT_COLS} FROM bookmark_permissions p
         WHERE p.tenant_id = $1 AND (p.expires_at IS NULL OR p.expires_at > $2)
           AND ((p.subject_type = 'user' AND p.subject_id = $3 AND $3 <> '')
                OR (p.subject_type = 'role' AND p.subject_id = ANY($4))
                OR p.subject_type = 'tenant')
         ORDER BY p.granted_at, p.id"
    ))
    .bind(tenant_uuid(&s.tenant_id)?)
    .bind(now)
    .bind(&s.user_id)
    .bind(&s.roles)
    .fetch_all(c)
    .await?;
    rows.iter().map(grant_row).collect()
}

/// Inserts `g`; a duplicate tuple is a unique violation
/// ([`tangra_sdk::db::is_unique_violation`]).
pub async fn insert_grant(c: &mut PgConnection, tenant: &str, g: &Grant) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO bookmark_permissions
           (tenant_id, id, resource_type, resource_id, relation, subject_type, subject_id, granted_by, granted_at, expires_at)
         VALUES ($1, $2, 'bookmark', $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(tenant_uuid(tenant)?)
    .bind(g.id)
    .bind(&g.resource_id)
    .bind(g.relation.as_str())
    .bind(g.subject_type.as_str())
    .bind(&g.subject_id)
    .bind(&g.granted_by)
    .bind(g.granted_at)
    .bind(g.expires_at)
    .execute(c)
    .await?;
    Ok(())
}

/// The grant `id` of `tenant`.
pub async fn get_grant(c: &mut PgConnection, tenant: &str, id: Uuid) -> Result<Option<Grant>, sqlx::Error> {
    let r = sqlx::query(&format!(
        "SELECT {GRANT_COLS} FROM bookmark_permissions p WHERE p.tenant_id = $1 AND p.id = $2"
    ))
    .bind(tenant_uuid(tenant)?)
    .bind(id)
    .fetch_optional(c)
    .await?;
    r.as_ref().map(grant_row).transpose()
}

/// The grant holding the tuple (resource, subject, relation).
pub async fn find_grant(
    c: &mut PgConnection,
    tenant: &str,
    resource_id: &str,
    subject_type: SubjectType,
    subject_id: &str,
    relation: Relation,
) -> Result<Option<Grant>, sqlx::Error> {
    let r = sqlx::query(&format!(
        "SELECT {GRANT_COLS} FROM bookmark_permissions p
         WHERE p.tenant_id = $1 AND p.resource_type = 'bookmark' AND p.resource_id = $2
           AND p.subject_type = $3 AND p.subject_id = $4 AND p.relation = $5"
    ))
    .bind(tenant_uuid(tenant)?)
    .bind(resource_id)
    .bind(subject_type.as_str())
    .bind(subject_id)
    .bind(relation.as_str())
    .fetch_optional(c)
    .await?;
    r.as_ref().map(grant_row).transpose()
}

/// Removes the grant `id`; false when absent.
pub async fn delete_grant(c: &mut PgConnection, tenant: &str, id: Uuid) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("DELETE FROM bookmark_permissions WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_uuid(tenant)?)
        .bind(id)
        .execute(c)
        .await?;
    Ok(r.rows_affected() == 1)
}

/// Which grants a listing may show.
#[derive(Debug, Clone)]
pub enum GrantScope {
    /// The grants of one resource ("" = the collection grants).
    Resource(String),
    /// The grants of every resource `s` can share at `now` (a collection
    /// share covers every resource); admins: every grant.
    Shareable(Subjects, DateTime<Utc>),
}

/// The grant list filter.
#[derive(Debug, Clone, Default)]
pub struct GrantFilter {
    /// Only this subject kind.
    pub subject_type: Option<SubjectType>,
    /// Only this subject ID.
    pub subject_id: Option<String>,
}

/// One page of grants (clamped like [`list_bookmarks`]).
pub async fn list_grants(
    c: &mut PgConnection,
    tenant: &str,
    scope: &GrantScope,
    f: &GrantFilter,
    req: &Request,
    spec: &Spec,
) -> Result<(Vec<Grant>, i64, Request), sqlx::Error> {
    let t = tenant_uuid(tenant)?;
    // $2 resource (or NULL), $3 shareable-by flag, $4 admin, $5 now,
    // $6 user, $7 roles, $8 subject_type, $9 subject_id.
    let from = "FROM bookmark_permissions p
         WHERE p.tenant_id = $1
           AND ($2::text IS NULL OR p.resource_id = $2)
           AND (NOT $3 OR $4 OR EXISTS (
                 SELECT 1 FROM bookmark_permissions m
                 WHERE m.tenant_id = p.tenant_id AND m.relation IN ('owner', 'sharer')
                   AND (m.expires_at IS NULL OR m.expires_at > $5)
                   AND ((m.subject_type = 'user' AND m.subject_id = $6 AND $6 <> '')
                        OR (m.subject_type = 'role' AND m.subject_id = ANY($7))
                        OR m.subject_type = 'tenant')
                   AND ((m.resource_id = '' AND m.subject_type = 'tenant') OR (m.resource_id = p.resource_id AND p.resource_id <> ''))))
           AND ($8::text IS NULL OR p.subject_type = $8)
           AND ($9::text IS NULL OR p.subject_id = $9)";
    let (resource, shareable, admin, now, user, roles) = match scope {
        GrantScope::Resource(r) => (
            Some(r.clone()),
            false,
            false,
            Utc::now(),
            String::new(),
            Vec::new(),
        ),
        GrantScope::Shareable(s, now) => (None, true, s.is_admin(), *now, s.user_id.clone(), s.roles.clone()),
    };
    let st = f.subject_type.map(SubjectType::as_str);
    macro_rules! binds {
        ($q:expr) => {
            $q.bind(t)
                .bind(&resource)
                .bind(shareable)
                .bind(admin)
                .bind(now)
                .bind(&user)
                .bind(&roles)
                .bind(st)
                .bind(&f.subject_id)
        };
    }
    let total: i64 = binds!(sqlx::query(&format!("SELECT count(*) AS n {from}")))
        .fetch_one(&mut *c)
        .await?
        .try_get("n")?;
    let served = req.clamp(total);
    let rows = binds!(sqlx::query(&format!(
        "SELECT {GRANT_COLS} {from} ORDER BY {} LIMIT $10 OFFSET $11",
        served.order_by(spec)
    )))
    .bind(served.limit())
    .bind(served.offset())
    .fetch_all(&mut *c)
    .await?;
    let items = rows.iter().map(grant_row).collect::<Result<_, _>>()?;
    Ok((items, total, served))
}

/// Every grant of the tenant (backup export), oldest first.
pub async fn all_grants(c: &mut PgConnection, tenant: &str) -> Result<Vec<Grant>, sqlx::Error> {
    let rows = sqlx::query(&format!(
        "SELECT {GRANT_COLS} FROM bookmark_permissions p WHERE p.tenant_id = $1 ORDER BY p.granted_at, p.id"
    ))
    .bind(tenant_uuid(tenant)?)
    .fetch_all(c)
    .await?;
    rows.iter().map(grant_row).collect()
}

// ---- audit ----

/// One business audit row (contracts/audit-events.md).
#[derive(Debug, Clone)]
pub struct AuditEvent<'a> {
    /// The closed vocabulary.
    pub event_type: &'static str,
    /// The acting user.
    pub actor_id: &'a str,
    /// `bookmark`, `permission` or `backup`.
    pub subject_kind: &'static str,
    /// The subject's ID.
    pub subject_id: String,
    /// `ok` or `refused`.
    pub outcome: &'static str,
    /// A reason code ("" for ok).
    pub reason: &'static str,
    /// The request's correlation ID.
    pub correlation_id: &'a str,
    /// IDs, codes and counts only.
    pub details: Value,
}

/// Writes `e` for `tenant`.
pub async fn audit(c: &mut PgConnection, tenant: &str, e: &AuditEvent<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO bookmark_audit_events
           (tenant_id, event_type, actor_id, subject_kind, subject_id, outcome, reason, correlation_id, details)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(tenant_uuid(tenant)?)
    .bind(e.event_type)
    .bind(e.actor_id)
    .bind(e.subject_kind)
    .bind(&e.subject_id)
    .bind(e.outcome)
    .bind(e.reason)
    .bind(e.correlation_id)
    .bind(&e.details)
    .execute(c)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_are_valid() {
        bookmark_spec().validate().unwrap();
        grant_spec().validate().unwrap();
    }

    #[test]
    fn like_escapes_metacharacters() {
        assert_eq!(like_pattern("a%b_c\\d"), "%a\\%b\\_c\\\\d%");
    }
}
