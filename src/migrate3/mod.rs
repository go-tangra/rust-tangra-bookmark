//! `bookmark-migrate`: imports the v3 bookmark tables (`bookmark_bookmarks`,
//! `bookmark_permissions` in the v3 database `gwa`) into the v4 schema, in
//! the shape of sms-gw's `cmd/smsgw-migrate` (specs/038-bookmark-v4
//! FR-060–FR-066):
//!
//! 1. [`read_snapshot`] reads the source in one `REPEATABLE READ READ ONLY`
//!    transaction on a session with `default_transaction_read_only = on`;
//!    with the v3 admin table `sys_users` present it also reads each user's
//!    e-mail.
//! 2. [`transform`] maps tenants, users, roles, relations and subjects
//!    ([`Mapping`]) and checks every row against the v4 rules; nothing is
//!    changed silently: a row that cannot be carried over as it is fails the
//!    run, or with `exclude_orphans` / `exclude_invalid` is left out and
//!    listed.
//! 3. [`import`] compares the result with the destination (system scope:
//!    the importer crosses tenants) and, with [`Mode::Apply`], writes the
//!    missing rows in one transaction, compares again inside it and commits
//!    only when every row is present exactly. Rows already there unchanged
//!    are skipped, so a re-run is a no-op (`already_applied`); a row there
//!    with other content is a conflict and nothing is written.

pub mod mapping;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{ConnectOptions, Connection, PgConnection, PgPool, Row};
use tangra_sdk::db::{self, Scope};
use uuid::Uuid;

use crate::api::input;
use crate::authz::{Grant, RESOURCE, Relation, SubjectType};
use crate::store::{self, Bookmark};
pub use mapping::{Mapping, Overrides, PLATFORM_TENANT, SourceConfig};

/// A failure of the run itself (configuration, connection, SQL); a refused
/// import is a [`Report`] with status `failed`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct MigrateError(pub String);

impl From<sqlx::Error> for MigrateError {
    fn from(e: sqlx::Error) -> Self {
        Self(format!("database: {e}"))
    }
}

/// A v3 `bookmark_bookmarks` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V3Bookmark {
    /// Kept as the v4 ID.
    pub id: Uuid,
    /// v3 tenant.
    pub tenant_id: i64,
    /// URL.
    pub url: String,
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Tags.
    pub tags: Vec<String>,
    /// v3 user ID of the creator.
    pub created_by: Option<i64>,
    /// `create_time`.
    pub create_time: DateTime<Utc>,
    /// `update_time`.
    pub update_time: DateTime<Utc>,
}

/// A v3 `bookmark_permissions` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V3Grant {
    /// SERIAL ID (the v4 ID is derived from it, [`grant_id`]).
    pub id: i64,
    /// v3 tenant.
    pub tenant_id: i64,
    /// `RESOURCE_TYPE_BOOKMARK`.
    pub resource_type: String,
    /// Bookmark UUID text.
    pub resource_id: String,
    /// `RELATION_*`.
    pub relation: String,
    /// `SUBJECT_TYPE_*`.
    pub subject_type: String,
    /// v3 user ID, role code, or `all`.
    pub subject_id: String,
    /// v3 user ID of the granter.
    pub granted_by: Option<i64>,
    /// Expiry (kept).
    pub expires_at: Option<DateTime<Utc>>,
    /// `create_time` (the v4 `granted_at`).
    pub create_time: DateTime<Utc>,
}

/// One consistent read of the v3 database.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Bookmarks by ID.
    pub bookmarks: Vec<V3Bookmark>,
    /// Grants by ID.
    pub grants: Vec<V3Grant>,
    /// v3 user ID → e-mail (`sys_users`; `None` without that table).
    pub users: Option<HashMap<i64, String>>,
    /// Source columns the import does not carry over.
    pub unsupported: Vec<String>,
}

const BOOKMARK_COLS: &[&str] = &[
    "id",
    "tenant_id",
    "url",
    "title",
    "description",
    "tags",
    "created_by",
    "create_time",
    "update_time",
];
const GRANT_COLS: &[&str] = &[
    "id",
    "tenant_id",
    "resource_type",
    "resource_id",
    "relation",
    "subject_type",
    "subject_id",
    "granted_by",
    "expires_at",
    "create_time",
];

/// Opens the source read-only (`default_transaction_read_only = on`).
pub async fn connect_source(dsn: &str) -> Result<PgConnection, MigrateError> {
    let o: sqlx::postgres::PgConnectOptions = dsn
        .parse()
        .map_err(|_| MigrateError("source dsn is invalid".into()))?;
    let o = o
        .options([("default_transaction_read_only", "on")])
        .application_name("bookmark-migrate")
        .disable_statement_logging();
    PgConnection::connect_with(&o)
        .await
        .map_err(|e| MigrateError(format!("source: {e}")))
}

async fn columns(c: &mut PgConnection, table: &str) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT column_name::text FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = $1 ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(&mut *c)
    .await?;
    rows.iter().map(|r| r.try_get(0)).collect()
}

/// Reads the snapshot in one repeatable-read, read-only transaction.
/// Missing v3 tables or columns are an error; extra columns are reported.
pub async fn read_snapshot(c: &mut PgConnection) -> Result<Snapshot, MigrateError> {
    let mut tx = c.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let mut s = Snapshot::default();
    for (table, want) in [
        ("bookmark_bookmarks", BOOKMARK_COLS),
        ("bookmark_permissions", GRANT_COLS),
    ] {
        let have = columns(&mut tx, table).await?;
        if have.is_empty() {
            return Err(MigrateError(format!(
                "source: table {table} not found (is this the v3 database?)"
            )));
        }
        if let Some(m) = want.iter().find(|w| !have.iter().any(|h| h == *w)) {
            return Err(MigrateError(format!("source: {table} has no column {m}")));
        }
        s.unsupported.extend(
            have.iter()
                .filter(|h| !want.contains(&h.as_str()))
                .map(|h| format!("{table}.{h}")),
        );
    }
    let rows = sqlx::query(
        "SELECT id, tenant_id::bigint, url::text, title::text, description::text, tags::text[],
                created_by::bigint, create_time, update_time
         FROM public.bookmark_bookmarks ORDER BY tenant_id, id",
    )
    .fetch_all(&mut *tx)
    .await?;
    for r in &rows {
        s.bookmarks.push(V3Bookmark {
            id: r.try_get(0)?,
            tenant_id: r.try_get(1)?,
            url: r.try_get(2)?,
            title: r.try_get(3)?,
            description: r.try_get(4)?,
            tags: r.try_get(5)?,
            created_by: r.try_get(6)?,
            create_time: r.try_get(7)?,
            update_time: r.try_get(8)?,
        });
    }
    let rows = sqlx::query(
        "SELECT id::bigint, tenant_id::bigint, resource_type::text, resource_id::text, relation::text,
                subject_type::text, subject_id::text, granted_by::bigint, expires_at, create_time
         FROM public.bookmark_permissions ORDER BY id",
    )
    .fetch_all(&mut *tx)
    .await?;
    for r in &rows {
        s.grants.push(V3Grant {
            id: r.try_get(0)?,
            tenant_id: r.try_get(1)?,
            resource_type: r.try_get(2)?,
            resource_id: r.try_get(3)?,
            relation: r.try_get(4)?,
            subject_type: r.try_get(5)?,
            subject_id: r.try_get(6)?,
            granted_by: r.try_get(7)?,
            expires_at: r.try_get(8)?,
            create_time: r.try_get(9)?,
        });
    }
    let users_cols = columns(&mut tx, "sys_users").await?;
    if users_cols.iter().any(|c| c == "id") && users_cols.iter().any(|c| c == "email") {
        let rows = sqlx::query(
            "SELECT id::bigint, email::text FROM public.sys_users WHERE email IS NOT NULL AND email <> ''",
        )
        .fetch_all(&mut *tx)
        .await?;
        let mut m = HashMap::new();
        for r in &rows {
            let email: String = r.try_get(1)?;
            m.insert(r.try_get::<i64, _>(0)?, email.trim().to_lowercase());
        }
        s.users = Some(m);
    }
    tx.rollback().await?;
    Ok(s)
}

/// The v4 grant ID of a v3 grant: stable across runs (name-based UUID,
/// version 8, from the v3 tenant and SERIAL ID), so a re-run finds it.
pub fn grant_id(v3_tenant: i64, v3_id: i64) -> Uuid {
    let h = Sha256::digest(format!("tangra-bookmark-v3-grant:{v3_tenant}:{v3_id}"));
    let mut b = [0_u8; 16];
    b.copy_from_slice(&h[..16]);
    Uuid::new_v8(b)
}

/// Import options.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// Leave out rows with unmapped references instead of failing.
    pub exclude_orphans: bool,
    /// Leave out rows the v4 schema refuses instead of failing.
    pub exclude_invalid: bool,
}

/// A row that was not carried over (or failed the run).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    /// `bookmark` or `permission`.
    pub entity: &'static str,
    /// The v3 ID.
    pub id: String,
    /// v3 tenant.
    pub tenant: i64,
    /// Why.
    pub reason: String,
}

/// What the transform found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Findings {
    /// Rows that fail the run (unmapped references without
    /// `exclude_orphans`, invalid rows without `exclude_invalid`).
    pub errors: Vec<Issue>,
    /// Rows left out (with `exclude_orphans` / `exclude_invalid`).
    pub excluded: Vec<Issue>,
    /// Authors (`created_by`, `granted_by`) recorded as the platform actor
    /// because the v3 row had none, or (with `exclude_orphans`) their v3
    /// user has no v4 account.
    pub author_fallbacks: usize,
    /// v3 grants whose v4 tuple another v3 grant already gives (two role
    /// codes mapped to one slug); the first is kept.
    pub merged_duplicates: Vec<Issue>,
    /// Grants imported although expired (kept with their expiry, as v3
    /// kept them; they confer nothing).
    pub expired_grants: usize,
}

/// The transformed rows of one v4 tenant.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TenantRows {
    /// Bookmarks.
    pub bookmarks: Vec<Bookmark>,
    /// Grants.
    pub grants: Vec<Grant>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Orphan,
    Invalid,
}

struct Ctx<'a> {
    m: &'a Mapping,
    s: &'a Snapshot,
    o: Options,
    f: Findings,
}

impl Ctx<'_> {
    fn issue(&mut self, kind: Kind, entity: &'static str, id: String, tenant: i64, reason: String) {
        let excluded = match kind {
            Kind::Orphan => self.o.exclude_orphans,
            Kind::Invalid => self.o.exclude_invalid,
        };
        let i = Issue {
            entity,
            id,
            tenant,
            reason,
        };
        if excluded {
            self.f.excluded.push(i);
        } else {
            self.f.errors.push(i);
        }
    }

    /// A v3 user ID → v4 user UUID, or why not.
    fn user(&self, v3: i64) -> Result<String, String> {
        if let Some(u) = self.m.users_by_v3_id.get(&v3) {
            return Ok(u.clone());
        }
        let Some(users) = &self.s.users else {
            return Err(format!(
                "v3 user {v3} is not in the v3 users file and the source has no sys_users table"
            ));
        };
        let Some(email) = users.get(&v3) else {
            return Err(format!("v3 user {v3} has no e-mail in sys_users"));
        };
        self.m
            .users_by_email
            .get(email)
            .cloned()
            .ok_or_else(|| format!("v3 user {v3} ({email}) has no v4 user in the users file"))
    }

    /// An author (`created_by`, `granted_by`): none → the platform actor; an
    /// unmapped one → an orphan, or the platform actor with
    /// `exclude_orphans`.
    fn author(&mut self, v3: Option<i64>) -> Result<String, String> {
        match v3.filter(|n| *n != 0) {
            None => {
                self.f.author_fallbacks += 1;
                Ok(self.m.platform_actor.clone())
            }
            Some(n) => match self.user(n) {
                Ok(u) => Ok(u),
                Err(_) if self.o.exclude_orphans => {
                    self.f.author_fallbacks += 1;
                    Ok(self.m.platform_actor.clone())
                }
                Err(e) => Err(e),
            },
        }
    }
}

fn v3_relation(s: &str) -> Option<Relation> {
    Some(match s {
        "RELATION_OWNER" => Relation::Owner,
        "RELATION_EDITOR" => Relation::Editor,
        "RELATION_SHARER" => Relation::Sharer,
        "RELATION_VIEWER" => Relation::Viewer,
        _ => return None,
    })
}

/// Checks a bookmark against the v4 rules without changing it: the
/// normalized value must equal the stored one.
fn check_bookmark(b: &V3Bookmark) -> Result<(), String> {
    let same = |field: &str, r: Result<String, &str>, v: &str| match r {
        Ok(n) if n == v => Ok(()),
        Ok(_) => Err(format!("{field} would be changed (surrounding white space)")),
        Err(code) => Err(format!("{field}: {code}")),
    };
    same("url", input::url(&b.url), &b.url)?;
    same("title", input::text(&b.title, input::MAX_TITLE, false), &b.title)?;
    same(
        "description",
        input::text(&b.description, input::MAX_DESCRIPTION, true),
        &b.description,
    )?;
    match input::tags(&b.tags) {
        Ok(t) if t == b.tags => Ok(()),
        Ok(_) => Err("tags would be changed (white space or duplicates)".into()),
        Err(code) => Err(format!("tags: {code}")),
    }
}

/// Maps the snapshot to v4 rows per v4 tenant (no database access).
pub fn transform(s: &Snapshot, m: &Mapping, o: Options) -> (BTreeMap<String, TenantRows>, Findings) {
    let mut cx = Ctx {
        m,
        s,
        o,
        f: Findings::default(),
    };
    let mut out: BTreeMap<String, TenantRows> = BTreeMap::new();
    // (v3 tenant, bookmark ID) of the bookmarks carried over.
    let mut kept: BTreeSet<(i64, Uuid)> = BTreeSet::new();
    let mut left_out: BTreeSet<(i64, Uuid)> = BTreeSet::new();
    let now = Utc::now();
    for b in &s.bookmarks {
        let id = b.id.to_string();
        let Some(tenant) = m.tenants.get(&b.tenant_id).cloned() else {
            cx.issue(
                Kind::Orphan,
                "bookmark",
                id,
                b.tenant_id,
                format!("v3 tenant {} is not mapped", b.tenant_id),
            );
            left_out.insert((b.tenant_id, b.id));
            continue;
        };
        if let Err(e) = check_bookmark(b) {
            cx.issue(Kind::Invalid, "bookmark", id, b.tenant_id, e);
            left_out.insert((b.tenant_id, b.id));
            continue;
        }
        let created_by = match cx.author(b.created_by) {
            Ok(u) => u,
            Err(e) => {
                cx.issue(
                    Kind::Orphan,
                    "bookmark",
                    id,
                    b.tenant_id,
                    format!("created_by: {e}"),
                );
                left_out.insert((b.tenant_id, b.id));
                continue;
            }
        };
        kept.insert((b.tenant_id, b.id));
        out.entry(tenant).or_default().bookmarks.push(Bookmark {
            id: b.id,
            url: b.url.clone(),
            title: b.title.clone(),
            description: b.description.clone(),
            tags: b.tags.clone(),
            created_by,
            created_at: b.create_time,
            updated_at: b.update_time,
        });
    }
    let mut tuples: HashMap<(String, String, &'static str, String, &'static str), i64> = HashMap::new();
    for g in &s.grants {
        let id = g.id.to_string();
        let t = g.tenant_id;
        let Some(tenant) = m.tenants.get(&t).cloned() else {
            cx.issue(
                Kind::Orphan,
                "permission",
                id,
                t,
                format!("v3 tenant {t} is not mapped"),
            );
            continue;
        };
        if g.resource_type != "RESOURCE_TYPE_BOOKMARK" {
            cx.issue(
                Kind::Invalid,
                "permission",
                id,
                t,
                format!("resource type {:?}", g.resource_type),
            );
            continue;
        }
        let Some(relation) = v3_relation(&g.relation) else {
            cx.issue(
                Kind::Invalid,
                "permission",
                id,
                t,
                format!("relation {:?}", g.relation),
            );
            continue;
        };
        let bookmark = match Uuid::parse_str(&g.resource_id) {
            Ok(u) if kept.contains(&(t, u)) => u,
            Ok(u) if left_out.contains(&(t, u)) => {
                // Follows its bookmark: listed, never a second error.
                cx.f.excluded.push(Issue {
                    entity: "permission",
                    id,
                    tenant: t,
                    reason: format!("its bookmark {u} is not imported"),
                });
                continue;
            }
            _ => {
                cx.issue(
                    Kind::Orphan,
                    "permission",
                    id,
                    t,
                    format!("bookmark {:?} does not exist in v3 tenant {t}", g.resource_id),
                );
                continue;
            }
        };
        let subject = match g.subject_type.as_str() {
            "SUBJECT_TYPE_USER" => match g.subject_id.trim().parse::<i64>() {
                Ok(n) => cx
                    .user(n)
                    .map(|u| (SubjectType::User, u))
                    .map_err(|e| (Kind::Orphan, e)),
                Err(_) => Err((
                    Kind::Invalid,
                    format!("user subject {:?} is not a v3 user ID", g.subject_id),
                )),
            },
            "SUBJECT_TYPE_ROLE" => m
                .roles
                .get(&g.subject_id)
                .map(|r| (SubjectType::Role, r.clone()))
                .ok_or_else(|| {
                    (
                        Kind::Orphan,
                        format!("v3 role {:?} is not in the role map", g.subject_id),
                    )
                }),
            // v3 stored the tenant subject as "all" on one bookmark: the v4
            // tenant subject ('') on the same bookmark.
            "SUBJECT_TYPE_TENANT" if g.subject_id == "all" => Ok((SubjectType::Tenant, String::new())),
            other => Err((Kind::Invalid, format!("subject {other}/{:?}", g.subject_id))),
        };
        let (subject_type, subject_id) = match subject {
            Ok(s) => s,
            Err((k, e)) => {
                cx.issue(k, "permission", id, t, e);
                continue;
            }
        };
        let granted_by = match cx.author(g.granted_by) {
            Ok(u) => u,
            Err(e) => {
                cx.issue(Kind::Orphan, "permission", id, t, format!("granted_by: {e}"));
                continue;
            }
        };
        let key = (
            tenant.clone(),
            bookmark.to_string(),
            subject_type.as_str(),
            subject_id.clone(),
            relation.as_str(),
        );
        if let Some(first) = tuples.get(&key) {
            cx.f.merged_duplicates.push(Issue {
                entity: "permission",
                id,
                tenant: t,
                reason: format!("same v4 grant as v3 permission {first}"),
            });
            continue;
        }
        tuples.insert(key, g.id);
        if g.expires_at.is_some_and(|e| e <= now) {
            cx.f.expired_grants += 1;
        }
        out.entry(tenant).or_default().grants.push(Grant {
            id: grant_id(t, g.id),
            resource_type: RESOURCE,
            resource_id: bookmark.to_string(),
            subject_type,
            subject_id,
            relation,
            granted_by,
            granted_at: g.create_time,
            expires_at: g.expires_at,
        });
    }
    (out, cx.f)
}

/// Run mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Compare only.
    DryRun,
    /// Write in one transaction and reconcile.
    Apply,
}

/// Outcome of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The dry-run found nothing in the way / the apply committed.
    Succeeded,
    /// Errors or conflicts; nothing written.
    Failed,
    /// Apply with every row already present unchanged (a re-run).
    AlreadyApplied,
}

/// What an apply does (did) to one entity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct EntityPlan {
    /// Rows to write.
    pub insert: usize,
    /// Rows already present, identical.
    pub unchanged: usize,
    /// Rows present with other content (never overwritten).
    pub conflict: usize,
}

/// The JSON report (stdout).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Mode.
    pub mode: Mode,
    /// Status.
    pub status: Status,
    /// v3 tenant → v4 tenant used.
    pub tenants: BTreeMap<String, String>,
    /// Rows read from the source.
    pub source: BTreeMap<&'static str, usize>,
    /// Whether the v3 `sys_users` table supplied e-mails.
    pub source_users_table: bool,
    /// Source columns not carried over.
    pub unsupported_columns: Vec<String>,
    /// Per entity (`bookmarks`, `permissions`).
    pub plan: BTreeMap<&'static str, EntityPlan>,
    /// Rows of the imported tenants in the destination after the run.
    pub destination: BTreeMap<&'static str, usize>,
    /// Every transformed row is in the destination, identical.
    pub reconciled: bool,
    /// Rows present with other content.
    pub conflicts: Vec<String>,
    /// Transform findings.
    #[serde(flatten)]
    pub findings: Findings,
}

fn same_grant(a: &Grant, b: &Grant) -> bool {
    a.resource_id == b.resource_id
        && a.subject_type == b.subject_type
        && a.subject_id == b.subject_id
        && a.relation == b.relation
        && a.granted_by == b.granted_by
        && a.granted_at == b.granted_at
        && a.expires_at == b.expires_at
}

/// Compares the rows with the destination: the plan, the rows to write and
/// the conflicts.
async fn diff(
    c: &mut PgConnection,
    rows: &BTreeMap<String, TenantRows>,
) -> Result<
    (
        BTreeMap<&'static str, EntityPlan>,
        BTreeMap<String, TenantRows>,
        Vec<String>,
    ),
    sqlx::Error,
> {
    let mut bp = EntityPlan::default();
    let mut gp = EntityPlan::default();
    let mut todo: BTreeMap<String, TenantRows> = BTreeMap::new();
    let mut conflicts = Vec::new();
    for (tenant, tr) in rows {
        for b in &tr.bookmarks {
            match store::get_bookmark(c, tenant, b.id).await? {
                None => {
                    bp.insert += 1;
                    todo.entry(tenant.clone()).or_default().bookmarks.push(b.clone());
                }
                Some(have) if have == *b => bp.unchanged += 1,
                Some(_) => {
                    bp.conflict += 1;
                    conflicts.push(format!(
                        "bookmark {} in tenant {tenant} exists with other content",
                        b.id
                    ));
                }
            }
        }
        for g in &tr.grants {
            let by_id = store::get_grant(c, tenant, g.id).await?;
            let by_tuple = store::find_grant(
                c,
                tenant,
                &g.resource_id,
                g.subject_type,
                &g.subject_id,
                g.relation,
            )
            .await?;
            match (by_id, by_tuple) {
                (None, None) => {
                    gp.insert += 1;
                    todo.entry(tenant.clone()).or_default().grants.push(g.clone());
                }
                // The same grant, or the same tuple given since in v4 (e.g. a
                // backup import) with the same expiry: nothing to add.
                (Some(h), _) if same_grant(&h, g) => gp.unchanged += 1,
                (None, Some(h)) if h.expires_at == g.expires_at => gp.unchanged += 1,
                _ => {
                    gp.conflict += 1;
                    conflicts.push(format!(
                        "permission {} ({} {}:{} on {}) in tenant {tenant} exists with other content",
                        g.id,
                        g.relation.as_str(),
                        g.subject_type.as_str(),
                        g.subject_id,
                        g.resource_id
                    ));
                }
            }
        }
    }
    Ok((
        BTreeMap::from([("bookmarks", bp), ("permissions", gp)]),
        todo,
        conflicts,
    ))
}

async fn counts(pool: &PgPool, tenants: &[String]) -> Result<BTreeMap<&'static str, usize>, sqlx::Error> {
    let ids: Vec<Uuid> = tenants.iter().filter_map(|t| Uuid::parse_str(t).ok()).collect();
    let mut tx = db::begin(pool, &Scope::system()).await?;
    let mut out = BTreeMap::new();
    for (k, table) in [
        ("bookmarks", "bookmark_bookmarks"),
        ("permissions", "bookmark_permissions"),
    ] {
        let n: i64 = sqlx::query(&format!("SELECT count(*) FROM {table} WHERE tenant_id = ANY($1)"))
            .bind(&ids)
            .fetch_one(&mut *tx)
            .await?
            .try_get(0)?;
        out.insert(k, usize::try_from(n).unwrap_or(0));
    }
    tx.rollback().await?;
    Ok(out)
}

/// Runs the import: snapshot, transform, compare and (apply) write and
/// reconcile in one transaction. `dst` should use the migration role
/// (`db.migrate_dsn`); the transactions run in system scope.
pub async fn import(
    src: &mut PgConnection,
    dst: &PgPool,
    m: &Mapping,
    mode: Mode,
    o: Options,
) -> Result<Report, MigrateError> {
    let snap = read_snapshot(src).await?;
    let (rows, findings) = transform(&snap, m, o);
    let tenants: Vec<String> = rows.keys().cloned().collect();
    let mut rep = Report {
        mode,
        status: Status::Succeeded,
        tenants: m
            .tenants
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
        source: BTreeMap::from([
            ("bookmarks", snap.bookmarks.len()),
            ("permissions", snap.grants.len()),
        ]),
        source_users_table: snap.users.is_some(),
        unsupported_columns: snap.unsupported.clone(),
        plan: BTreeMap::new(),
        destination: BTreeMap::new(),
        reconciled: false,
        conflicts: Vec::new(),
        findings,
    };
    let mut tx = db::begin(dst, &Scope::system()).await?;
    let schema: bool = sqlx::query("SELECT to_regclass('public.bookmark_permissions') IS NOT NULL")
        .fetch_one(&mut *tx)
        .await?
        .try_get(0)?;
    if !schema {
        return Err(MigrateError(
            "destination has no bookmark schema: run `bookmarksvc bootstrap` first".into(),
        ));
    }
    let (plan, todo, conflicts) = diff(&mut tx, &rows).await?;
    tx.rollback().await?;
    rep.plan = plan;
    rep.conflicts = conflicts;
    let inserts: usize = rep.plan.values().map(|p| p.insert).sum();
    let blocked = !rep.findings.errors.is_empty() || !rep.conflicts.is_empty();
    if blocked {
        rep.status = Status::Failed;
    } else if mode == Mode::DryRun {
        rep.reconciled = inserts == 0;
    } else if inserts == 0 {
        rep.status = Status::AlreadyApplied;
        rep.reconciled = true;
    } else {
        let mut tx = db::begin(dst, &Scope::system()).await?;
        for (tenant, tr) in &todo {
            for b in &tr.bookmarks {
                store::insert_bookmark(&mut tx, tenant, b).await?;
            }
            for g in &tr.grants {
                store::insert_grant(&mut tx, tenant, g).await?;
            }
        }
        let (after, _, conflicts) = diff(&mut tx, &rows).await?;
        let expected = BTreeMap::from([
            (
                "bookmarks",
                rows.values().map(|r| r.bookmarks.len()).sum::<usize>(),
            ),
            (
                "permissions",
                rows.values().map(|r| r.grants.len()).sum::<usize>(),
            ),
        ]);
        let ok = conflicts.is_empty()
            && after
                .iter()
                .all(|(k, p)| p.insert == 0 && p.conflict == 0 && p.unchanged == expected[k]);
        if !ok {
            tx.rollback().await?;
            rep.status = Status::Failed;
            rep.conflicts
                .push("reconciliation failed after the writes; rolled back".into());
        } else {
            tx.commit().await?;
            rep.reconciled = true;
        }
    }
    rep.destination = counts(dst, &tenants).await?;
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn bm(id: u128, tenant: i64, url: &str, by: Option<i64>) -> V3Bookmark {
        V3Bookmark {
            id: Uuid::from_u128(id),
            tenant_id: tenant,
            url: url.into(),
            title: "t".into(),
            description: String::new(),
            tags: vec!["a".into()],
            created_by: by,
            create_time: ts("2025-01-01T00:00:00Z"),
            update_time: ts("2025-01-02T00:00:00Z"),
        }
    }

    fn gr(id: i64, tenant: i64, res: u128, rel: &str, st: &str, sid: &str) -> V3Grant {
        V3Grant {
            id,
            tenant_id: tenant,
            resource_type: "RESOURCE_TYPE_BOOKMARK".into(),
            resource_id: Uuid::from_u128(res).to_string(),
            relation: rel.into(),
            subject_type: st.into(),
            subject_id: sid.into(),
            granted_by: Some(1),
            expires_at: None,
            create_time: ts("2025-01-01T00:00:00Z"),
        }
    }

    const U1: &str = "0192f6b0-0000-7000-8000-00000000a0a0";

    fn mapping() -> Mapping {
        Mapping {
            tenants: BTreeMap::from([(0, PLATFORM_TENANT.to_owned())]),
            users_by_email: HashMap::from([("ana@example.org".to_owned(), U1.to_owned())]),
            users_by_v3_id: HashMap::new(),
            roles: BTreeMap::from([("platform:admin".to_owned(), "admin".to_owned())]),
            platform_actor: "legacy".into(),
        }
    }

    fn snap() -> Snapshot {
        Snapshot {
            bookmarks: vec![
                bm(1, 0, "https://a.example", Some(1)),
                bm(2, 0, "javascript:x", Some(1)),
            ],
            grants: vec![
                gr(1, 0, 1, "RELATION_OWNER", "SUBJECT_TYPE_USER", "1"),
                gr(2, 0, 1, "RELATION_VIEWER", "SUBJECT_TYPE_TENANT", "all"),
                gr(3, 0, 1, "RELATION_EDITOR", "SUBJECT_TYPE_ROLE", "platform:admin"),
                gr(4, 0, 2, "RELATION_OWNER", "SUBJECT_TYPE_USER", "1"),
                gr(5, 0, 9, "RELATION_OWNER", "SUBJECT_TYPE_USER", "1"),
                gr(6, 0, 1, "RELATION_VIEWER", "SUBJECT_TYPE_USER", "2"),
            ],
            users: Some(HashMap::from([
                (1, "ana@example.org".to_owned()),
                (2, "bob@example.org".to_owned()),
            ])),
            unsupported: vec![],
        }
    }

    #[test]
    fn transform_maps_and_refuses() {
        let (rows, f) = transform(&snap(), &mapping(), Options::default());
        let t = &rows[PLATFORM_TENANT];
        assert_eq!(t.bookmarks.len(), 1);
        assert_eq!(t.bookmarks[0].created_by, U1);
        assert_eq!(t.grants.len(), 3);
        let tenant = t
            .grants
            .iter()
            .find(|g| g.subject_type == SubjectType::Tenant)
            .unwrap();
        assert_eq!(
            (tenant.subject_id.as_str(), tenant.relation),
            ("", Relation::Viewer)
        );
        assert_eq!(tenant.resource_id, Uuid::from_u128(1).to_string());
        let role = t
            .grants
            .iter()
            .find(|g| g.subject_type == SubjectType::Role)
            .unwrap();
        assert_eq!(role.subject_id, "admin");
        // invalid URL, grant on a missing bookmark, unmapped bob → errors;
        // the grant of the invalid bookmark follows it (excluded).
        assert_eq!(f.errors.len(), 3, "{:?}", f.errors);
        assert_eq!(f.excluded.len(), 1);
        assert!(f.errors[0].reason.contains("url"));

        let (rows, f) = transform(
            &snap(),
            &mapping(),
            Options {
                exclude_orphans: true,
                exclude_invalid: true,
            },
        );
        assert!(f.errors.is_empty());
        assert_eq!(f.excluded.len(), 4);
        assert_eq!(rows[PLATFORM_TENANT].grants.len(), 3);
        assert_eq!(grant_id(0, 1), grant_id(0, 1));
        assert_ne!(grant_id(0, 1), grant_id(1, 1));
    }

    #[test]
    fn unmapped_authors_and_duplicates() {
        let mut s = snap();
        s.bookmarks.truncate(1);
        s.bookmarks[0].created_by = Some(2); // bob: no v4 user
        s.grants = vec![
            gr(1, 0, 1, "RELATION_EDITOR", "SUBJECT_TYPE_ROLE", "platform:admin"),
            gr(2, 0, 1, "RELATION_EDITOR", "SUBJECT_TYPE_ROLE", "tenant:manager"),
        ];
        s.grants[0].granted_by = None;
        let mut m = mapping();
        m.roles.insert("tenant:manager".into(), "admin".into());
        let (_, f) = transform(&s, &m, Options::default());
        assert_eq!(f.errors.len(), 1, "unmapped creator fails");
        let (rows, f) = transform(
            &s,
            &m,
            Options {
                exclude_orphans: true,
                ..Options::default()
            },
        );
        let t = &rows[PLATFORM_TENANT];
        assert_eq!(t.bookmarks[0].created_by, "legacy");
        assert_eq!(t.grants.len(), 1);
        assert_eq!(t.grants[0].granted_by, "legacy");
        assert_eq!(f.author_fallbacks, 2);
        assert_eq!(f.merged_duplicates.len(), 1);
        // No sys_users table and no v3 users file: users cannot be mapped.
        s.users = None;
        let (_, f) = transform(&s, &m, Options::default());
        assert!(f.errors[0].reason.contains("sys_users"));
        m.users_by_v3_id.insert(2, U1.into());
        let (rows, _) = transform(&s, &m, Options::default());
        assert_eq!(rows[PLATFORM_TENANT].bookmarks[0].created_by, U1);
    }

    #[test]
    fn bookmarks_are_never_changed_silently() {
        let mut b = bm(1, 0, " https://a.example", None);
        assert!(check_bookmark(&b).unwrap_err().contains("url"));
        b.url = "https://a.example".into();
        b.tags = vec!["a".into(), "a".into()];
        assert!(check_bookmark(&b).unwrap_err().contains("tags"));
        b.tags = (0..21).map(|i| i.to_string()).collect();
        assert_eq!(check_bookmark(&b).unwrap_err(), "tags: too_many");
        b.tags.clear();
        check_bookmark(&b).unwrap();
    }
}
