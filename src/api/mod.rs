//! The gateway-facing HTTP API (`api/openapi/bookmark.yaml`), shaped like
//! the deployer's `internal/httpapi` and the SDK's notes example:
//!
//! - the gateway has already enforced the route's coarse permission
//!   (`x-freya-permission`); the module verifies the platform token
//!   ([`UserAuthLayer`] -> [`User`]) and scopes every statement to the
//!   token's tenant;
//! - fine-grained access comes from the grants ([`crate::authz`]): a
//!   bookmark the caller cannot read is `404` (as an absent or foreign one),
//!   a readable one lacking the permission `403`;
//! - refusals use the `{"reason"[,"detail"]}` vocabulary; internal errors
//!   are logged without content and answered `500 internal`; every response
//!   carries the deployer's security headers.

pub mod backup;
pub mod bookmarks;
pub mod input;
pub mod permissions;

use std::sync::Arc;

use axum::Router;
use axum::extract::{DefaultBodyLimit, State};
use axum::response::Response;
use axum::routing::{MethodRouter, any, get, post};
use chrono::{DateTime, Utc};
use http::{HeaderMap, StatusCode};
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};
use tangra_sdk::auth::{User, UserAuthLayer, Verifier};
use tangra_sdk::db::{self, Scope};
use tangra_sdk::error::{self, ApiError};
use tangra_sdk::events::Publisher;
use tangra_sdk::listquery::Spec;
use tangra_sdk::log::{Logger, attr};
use uuid::Uuid;

use crate::authz::{self, Action, Effective, Grant, Subjects};
use crate::manifest::MODULE;
use crate::store::{self, AuditEvent, Bookmark};

/// What the handlers share.
#[derive(Clone)]
pub struct ApiState {
    /// The application pool.
    pub pool: PgPool,
    /// The event bus publisher (`events.enabled`).
    pub events: Option<Arc<Publisher>>,
    /// The app's logger.
    pub log: Logger,
    /// The release reported by `GET /api/bookmark/v1/info`.
    pub version: &'static str,
    /// Largest backup import body.
    pub backup_max_bytes: usize,
    /// The bookmark list's sort allow-list.
    pub bookmark_spec: Arc<Spec>,
    /// The grant list's sort allow-list.
    pub grant_spec: Arc<Spec>,
}

impl ApiState {
    /// State over `pool` with the module's list specs.
    pub fn new(
        pool: PgPool,
        events: Option<Arc<Publisher>>,
        log: Logger,
        version: &'static str,
        backup_max_bytes: usize,
    ) -> Self {
        Self {
            pool,
            events,
            log,
            version,
            backup_max_bytes,
            bookmark_spec: Arc::new(store::bookmark_spec()),
            grant_spec: Arc::new(store::grant_spec()),
        }
    }
}

/// The API router. Mount it next to the help bundle and the UI remote; the
/// catch-all answers `404 not_found`.
pub fn router(state: ApiState, verifier: Arc<Verifier>) -> Router {
    const P: &str = "/api/bookmark/v1";
    let protected = Router::new()
        .route(
            &format!("{P}/bookmarks"),
            methods(get(bookmarks::list).post(bookmarks::create)),
        )
        .route(
            &format!("{P}/bookmarks/{{id}}"),
            methods(get(bookmarks::get_one).put(bookmarks::update)),
        )
        .route(
            &format!("{P}/bookmarks/{{id}}/remove"),
            methods(post(bookmarks::remove)),
        )
        .route(&format!("{P}/permissions"), methods(get(permissions::list)))
        .route(
            &format!("{P}/permissions/grant"),
            methods(post(permissions::grant)),
        )
        .route(
            &format!("{P}/permissions/revoke"),
            methods(post(permissions::revoke)),
        )
        .route(
            &format!("{P}/permissions/check"),
            methods(post(permissions::check)),
        )
        .route(
            &format!("{P}/permissions/effective"),
            methods(get(permissions::effective)),
        )
        .route(&format!("{P}/backup/export"), methods(post(backup::export)))
        .route(&format!("{P}/backup/import"), methods(post(backup::import)))
        .route_layer(UserAuthLayer::new(verifier));
    Router::new()
        .route(&format!("{P}/info"), methods(get(info)))
        .merge(protected)
        // A real route, not a fallback: the app installs its own fallback.
        .route("/", any(not_found))
        .route("/{*rest}", any(not_found))
        .layer(axum::middleware::map_response(secure))
        .layer(DefaultBodyLimit::disable())
        .with_state(state)
}

/// A declared path answers other methods `405 method_not_allowed`.
fn methods(m: MethodRouter<ApiState>) -> MethodRouter<ApiState> {
    m.fallback(|| async { ApiError::new(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed") })
}

async fn not_found() -> ApiError {
    ApiError::not_found()
}

/// The headers every API response shares (deployer `secure`).
async fn secure(mut r: Response) -> Response {
    error::set_secure_headers(r.headers_mut());
    r
}

/// `GET /api/bookmark/v1/info` (public).
async fn info(State(s): State<ApiState>) -> Response {
    error::json(StatusCode::OK, &json!({"module": MODULE, "version": s.version}))
}

/// The caller as the access model sees them.
pub fn subjects(u: &User) -> Subjects {
    Subjects {
        tenant_id: u.tenant_id.clone(),
        user_id: u.user_id.clone(),
        roles: u.roles.clone(),
    }
}

/// The request's correlation ID (the app's chain has set `x-request-id`).
pub fn correlation(h: &HeaderMap) -> String {
    h.get(tangra_sdk::correlation::HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

/// The OpenAPI `X-CSRF-Token` header parameter (required, at most 128
/// bytes); a violation is `422 validation_failed` as kin-openapi answers.
pub fn require_csrf(h: &HeaderMap) -> Result<(), ApiError> {
    match h.get("x-csrf-token") {
        Some(v) if !v.is_empty() && v.len() <= 128 => Ok(()),
        _ => Err(ApiError::validation_failed()),
    }
}

/// Logs a storage failure (the error only, never request content) and
/// answers `500 internal`.
pub fn internal(log: &Logger, what: &str, e: &dyn std::fmt::Display) -> ApiError {
    log.error(
        "bookmark: storage failure",
        vec![attr("op", what), attr("err", e.to_string())],
    );
    ApiError::internal()
}

/// Maps a storage error (a unique violation is `409 conflict`).
pub fn db_error(log: &Logger, what: &str, e: &sqlx::Error) -> ApiError {
    if db::is_unique_violation(e) {
        ApiError::conflict()
    } else {
        internal(log, what, e)
    }
}

/// A tenant-scoped transaction for the caller. A token whose tenant is not
/// a UUID has no data here: `404`.
pub async fn begin(s: &ApiState, subj: &Subjects) -> Result<Transaction<'static, Postgres>, ApiError> {
    if Uuid::parse_str(&subj.tenant_id).is_err() {
        return Err(ApiError::not_found());
    }
    db::begin(&s.pool, &Scope::tenant(&subj.tenant_id))
        .await
        .map_err(|e| internal(&s.log, "begin", &e))
}

/// Commits `tx`.
pub async fn commit(s: &ApiState, tx: Transaction<'static, Postgres>) -> Result<(), ApiError> {
    tx.commit().await.map_err(|e| internal(&s.log, "commit", &e))
}

/// The bookmark `id` with the caller's access to it and the caller's grants,
/// or `404` when it is absent, foreign or unreadable (SR-002).
pub async fn access(
    s: &ApiState,
    tx: &mut Transaction<'static, Postgres>,
    subj: &Subjects,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<(Bookmark, Effective, Vec<Grant>), ApiError> {
    let b = store::get_bookmark(tx, &subj.tenant_id, id)
        .await
        .map_err(|e| internal(&s.log, "get bookmark", &e))?
        .ok_or_else(ApiError::not_found)?;
    let grants = store::grants_for_subjects(tx, subj, now)
        .await
        .map_err(|e| internal(&s.log, "grants", &e))?;
    let eff = authz::effective(subj, &grants, &id.to_string(), now);
    if !eff.has(Action::Read) {
        return Err(ApiError::not_found());
    }
    Ok((b, eff, grants))
}

/// The caller's access to the collection (tenant-wide grants; admins).
pub async fn collection_access(
    s: &ApiState,
    tx: &mut Transaction<'static, Postgres>,
    subj: &Subjects,
    now: DateTime<Utc>,
) -> Result<(Effective, Vec<Grant>), ApiError> {
    let grants = store::grants_for_subjects(tx, subj, now)
        .await
        .map_err(|e| internal(&s.log, "grants", &e))?;
    Ok((authz::effective(subj, &grants, "", now), grants))
}

/// Records a `403` refusal (`access_refused`, in its own transaction; the
/// request's transaction is rolled back) and returns it.
pub async fn refuse(
    s: &ApiState,
    subj: &Subjects,
    subject_id: String,
    action: Action,
    corr: &str,
) -> ApiError {
    let res = async {
        let mut tx = db::begin(&s.pool, &Scope::tenant(&subj.tenant_id)).await?;
        store::audit(
            &mut tx,
            &subj.tenant_id,
            &AuditEvent {
                event_type: "access_refused",
                actor_id: &subj.user_id,
                subject_kind: "bookmark",
                subject_id,
                outcome: "refused",
                reason: "forbidden",
                correlation_id: corr,
                details: json!({"action": action.as_str()}),
            },
        )
        .await?;
        tx.commit().await
    }
    .await;
    if let Err(e) = res {
        s.log
            .warn("bookmark: audit not written", vec![attr("err", e.to_string())]);
    }
    ApiError::forbidden()
}

/// Writes an `ok` audit row on the request's transaction.
pub async fn audit_ok(
    s: &ApiState,
    tx: &mut Transaction<'static, Postgres>,
    subj: &Subjects,
    event_type: &'static str,
    (subject_kind, subject_id): (&'static str, String),
    corr: &str,
    details: serde_json::Value,
) -> Result<(), ApiError> {
    store::audit(
        tx,
        &subj.tenant_id,
        &AuditEvent {
            event_type,
            actor_id: &subj.user_id,
            subject_kind,
            subject_id,
            outcome: "ok",
            reason: "",
            correlation_id: corr,
            details,
        },
    )
    .await
    .map_err(|e| internal(&s.log, "audit", &e))
}
