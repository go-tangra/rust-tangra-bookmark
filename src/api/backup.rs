//! `/api/bookmark/v1/backup/{export,import}` (FR-030–FR-032; `backup:manage`
//! enforced by the gateway). Both work on the caller's tenant only.

use axum::body::Body;
use axum::extract::State;
use axum::response::Response;
use chrono::Utc;
use http::{HeaderMap, StatusCode};
use serde_json::{Value, json};
use tangra_sdk::auth::User;
use tangra_sdk::error::{self, ApiError};

use super::input;
use super::{ApiState, audit_ok, begin, commit, correlation, db_error, internal, require_csrf, subjects};
use crate::backup::{self, Backup, Mode};

/// `POST /backup/export`.
pub async fn export(State(s): State<ApiState>, user: User, headers: HeaderMap) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    let subj = subjects(&user);
    let corr = correlation(&headers);
    let mut tx = begin(&s, &subj).await?;
    let b = backup::export(&mut tx, &subj.tenant_id, Utc::now())
        .await
        .map_err(|e| internal(&s.log, "export", &e))?;
    audit_ok(
        &s,
        &mut tx,
        &subj,
        "backup_exported",
        ("backup", String::new()),
        &corr,
        json!({"bookmarks": b.bookmarks.len(), "permissions": b.permissions.len()}),
    )
    .await?;
    commit(&s, tx).await?;
    Ok(error::json(StatusCode::OK, &b))
}

/// `POST /backup/import`: `{mode?, backup}`; all or nothing.
pub async fn import(
    State(s): State<ApiState>,
    user: User,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    let mut obj = input::read_object(body, s.backup_max_bytes).await?;
    let mut e = input::Errors::default();
    input::unknown_keys(&obj, &["mode", "backup"], &mut e);
    let mode = match obj.get("mode") {
        None | Some(Value::Null) => Some(Mode::Skip),
        Some(v) => v.as_str().and_then(Mode::parse),
    };
    if mode.is_none() {
        e.set("mode", "invalid");
    }
    let doc = match obj.remove("backup") {
        None => {
            e.set("backup", "required");
            None
        }
        Some(v) => match serde_json::from_value::<Backup>(v) {
            Ok(b) => Some(b),
            Err(_) => {
                e.set("backup", "invalid");
                None
            }
        },
    };
    let (mode, doc) = e.finish((mode, doc))?;
    let (Some(mode), Some(doc)) = (mode, doc) else {
        return Err(ApiError::validation_failed());
    };
    let subj = subjects(&user);
    let corr = correlation(&headers);
    let v = backup::validate(&doc, &subj.user_id, Utc::now())?;
    let mut tx = begin(&s, &subj).await?;
    let res = backup::import(&mut tx, &subj.tenant_id, v, mode)
        .await
        .map_err(|e| db_error(&s.log, "import", &e))?;
    audit_ok(
        &s,
        &mut tx,
        &subj,
        "backup_imported",
        ("backup", String::new()),
        &corr,
        json!({
            "mode": mode.as_str(),
            "bookmarks_imported": res.bookmarks_imported,
            "bookmarks_skipped": res.bookmarks_skipped,
            "permissions_imported": res.permissions_imported,
            "permissions_skipped": res.permissions_skipped,
        }),
    )
    .await?;
    commit(&s, tx).await?;
    Ok(error::json(StatusCode::OK, &res))
}
