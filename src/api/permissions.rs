//! `/api/bookmark/v1/permissions`: list, grant, revoke, check, effective
//! (FR-010–FR-019), exposed the way paperless v4 does.

use axum::body::Body;
use axum::extract::{RawQuery, State};
use axum::response::Response;
use chrono::Utc;
use http::{HeaderMap, StatusCode};
use serde_json::json;
use tangra_sdk::auth::User;
use tangra_sdk::error::{self, ApiError};
use tangra_sdk::events::Target as EventTarget;
use tangra_sdk::listquery::{self, Page, Query};
use tangra_sdk::log::attr;
use uuid::Uuid;

use super::input::{self, MAX_BODY_BYTES, MAX_SUBJECT_ID, Target};
use super::{
    ApiState, access, audit_ok, begin, collection_access, commit, correlation, db_error, internal, refuse,
    require_csrf, subjects,
};
use crate::authz::{self, Action, Effective, Grant, RESOURCE, SubjectType, Subjects};
use crate::store::{self, GrantFilter, GrantScope};

/// The event a user grantee receives (contracts/audit-events.md).
pub const EVENT_SHARED: &str = "bookmark.shared";

/// The caller's access to `target` (`404` for an unknown or unreadable
/// bookmark).
async fn target_access(
    s: &ApiState,
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    subj: &Subjects,
    target: Target,
) -> Result<(Effective, Vec<Grant>), ApiError> {
    let now = Utc::now();
    match target {
        Target::Collection => collection_access(s, tx, subj, now).await,
        Target::Bookmark(id) => {
            let (_, eff, grants) = access(s, tx, subj, id, now).await?;
            Ok((eff, grants))
        }
        Target::Unknown => Err(ApiError::not_found()),
    }
}

/// `GET /permissions`: the grants of one bookmark the caller can share, or
/// of every bookmark they can share (admins: every grant of the tenant).
pub async fn list(
    State(s): State<ApiState>,
    user: User,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Result<Response, ApiError> {
    let raw = raw.unwrap_or_default();
    let q = Query::parse(&raw);
    if listquery::legacy(&q) {
        return Err(ApiError::invalid_param(if q.has("cursor") {
            "cursor"
        } else {
            "limit"
        }));
    }
    let req = listquery::parse(&q, &s.grant_spec).map_err(|e| ApiError::invalid_param(e.param))?;
    let mut f = GrantFilter::default();
    if q.has("subject_type") {
        f.subject_type = Some(
            SubjectType::parse(q.get("subject_type"))
                .ok_or_else(|| ApiError::invalid_param("subject_type"))?,
        );
    }
    if q.has("subject_id") {
        let v = q.get("subject_id");
        if v.len() > MAX_SUBJECT_ID {
            return Err(ApiError::invalid_param("subject_id"));
        }
        f.subject_id = Some(v.to_owned());
    }
    let subj = subjects(&user);
    let corr = correlation(&headers);
    let mut tx = begin(&s, &subj).await?;
    let scope = if q.has("resource_id") {
        let target = input::target_param(Some(q.get("resource_id")));
        let (eff, _) = target_access(&s, &mut tx, &subj, target).await?;
        if !eff.has(Action::Share) {
            drop(tx);
            return Err(refuse(&s, &subj, target.resource_id(), Action::Share, &corr).await);
        }
        GrantScope::Resource(target.resource_id())
    } else {
        GrantScope::Shareable(subj.clone(), Utc::now())
    };
    let (items, total, served) =
        store::list_grants(&mut tx, &subj.tenant_id, &scope, &f, &req, &s.grant_spec)
            .await
            .map_err(|e| internal(&s.log, "list grants", &e))?;
    Ok(error::json(StatusCode::OK, &Page::new(items, total, &served)))
}

/// `POST /permissions/grant`.
pub async fn grant(
    State(s): State<ApiState>,
    user: User,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    let now = Utc::now();
    let ng = input::new_grant(&input::read_object(body, MAX_BODY_BYTES).await?, now)?;
    let subj = subjects(&user);
    let corr = correlation(&headers);
    let mut tx = begin(&s, &subj).await?;
    let (eff, _) = target_access(&s, &mut tx, &subj, ng.target).await?;
    // Viewer/sharer grants need share; editor/owner grants full control
    // (paperless canHandle).
    if !authz::may_handle(&eff, ng.relation) {
        drop(tx);
        return Err(refuse(&s, &subj, ng.target.resource_id(), Action::Share, &corr).await);
    }
    let g = Grant {
        id: Uuid::now_v7(),
        resource_type: RESOURCE,
        resource_id: ng.target.resource_id(),
        subject_type: ng.subject_type,
        subject_id: ng.subject_id,
        relation: ng.relation,
        granted_by: subj.user_id.clone(),
        granted_at: now,
        expires_at: ng.expires_at,
    };
    store::insert_grant(&mut tx, &subj.tenant_id, &g)
        .await
        .map_err(|e| db_error(&s.log, "insert grant", &e))?;
    audit_ok(
        &s,
        &mut tx,
        &subj,
        "permission_granted",
        ("permission", g.id.to_string()),
        &corr,
        json!({
            "resource_id": g.resource_id,
            "subject_type": g.subject_type.as_str(),
            "subject_id": g.subject_id,
            "relation": g.relation.as_str(),
            "expires": g.expires_at.is_some(),
        }),
    )
    .await?;
    commit(&s, tx).await?;
    publish_shared(&s, &subj, &g).await;
    Ok(error::json(StatusCode::CREATED, &g))
}

/// Tells a user grantee about the share (IDs and relation only). A failed
/// publish is logged; the grant stays.
async fn publish_shared(s: &ApiState, subj: &Subjects, g: &Grant) {
    let Some(p) = &s.events else { return };
    if g.subject_type != SubjectType::User || g.subject_id == subj.user_id || g.resource_id.is_empty() {
        return;
    }
    let data = json!({"bookmark_id": g.resource_id, "relation": g.relation.as_str()});
    if let Err(e) = p
        .publish(
            &subj.tenant_id,
            &EventTarget::Users(vec![g.subject_id.clone()]),
            EVENT_SHARED,
            &data,
        )
        .await
    {
        s.log.warn(
            "bookmark: event not sent",
            vec![
                attr("tenant_id", subj.tenant_id.as_str()),
                attr("event", EVENT_SHARED),
                attr("err", e.to_string()),
            ],
        );
    }
}

/// `POST /permissions/revoke`.
pub async fn revoke(
    State(s): State<ApiState>,
    user: User,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    let (target, id) = input::revoke(&input::read_object(body, MAX_BODY_BYTES).await?)?;
    let subj = subjects(&user);
    let corr = correlation(&headers);
    let mut tx = begin(&s, &subj).await?;
    let (eff, _) = target_access(&s, &mut tx, &subj, target).await?;
    if !eff.has(Action::Share) {
        drop(tx);
        return Err(refuse(&s, &subj, target.resource_id(), Action::Share, &corr).await);
    }
    // The grant must belong to the named resource: an ID of another
    // bookmark's grant is not found (paperless Revoke).
    let g = match id {
        Some(id) => store::get_grant(&mut tx, &subj.tenant_id, id)
            .await
            .map_err(|e| internal(&s.log, "get grant", &e))?,
        None => None,
    }
    .filter(|g| g.resource_id == target.resource_id())
    .ok_or_else(ApiError::not_found)?;
    if !authz::may_handle(&eff, g.relation) {
        drop(tx);
        return Err(refuse(&s, &subj, target.resource_id(), Action::Share, &corr).await);
    }
    if !store::delete_grant(&mut tx, &subj.tenant_id, g.id)
        .await
        .map_err(|e| internal(&s.log, "delete grant", &e))?
    {
        return Err(ApiError::not_found());
    }
    audit_ok(
        &s,
        &mut tx,
        &subj,
        "permission_revoked",
        ("permission", g.id.to_string()),
        &corr,
        json!({"resource_id": g.resource_id, "relation": g.relation.as_str()}),
    )
    .await?;
    commit(&s, tx).await?;
    Ok(error::json(StatusCode::OK, &json!({"revoked": true})))
}

/// `POST /permissions/check`: `allowed` (false also for an unknown or
/// unreadable bookmark).
pub async fn check(
    State(s): State<ApiState>,
    user: User,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    let (target, action) = input::check(&input::read_object(body, MAX_BODY_BYTES).await?)?;
    let subj = subjects(&user);
    let mut tx = begin(&s, &subj).await?;
    let allowed = match target_access(&s, &mut tx, &subj, target).await {
        Ok((eff, _)) => eff.has(action),
        Err(e) if e.status() == StatusCode::NOT_FOUND => false,
        Err(e) => return Err(e),
    };
    Ok(error::json(StatusCode::OK, &json!({"allowed": allowed})))
}

/// `GET /permissions/effective?resource_id=`: the caller's permissions,
/// highest relation and the grants that apply to them.
pub async fn effective(
    State(s): State<ApiState>,
    user: User,
    RawQuery(raw): RawQuery,
) -> Result<Response, ApiError> {
    let q = Query::parse(raw.as_deref().unwrap_or(""));
    let target = input::target_param(q.has("resource_id").then(|| q.get("resource_id")));
    let subj = subjects(&user);
    let mut tx = begin(&s, &subj).await?;
    let (eff, grants) = target_access(&s, &mut tx, &subj, target).await?;
    let rid = target.resource_id();
    let now = Utc::now();
    let applying: Vec<&Grant> = grants.iter().filter(|g| g.applies(&subj, &rid, now)).collect();
    Ok(error::json(
        StatusCode::OK,
        &json!({
            "permissions": eff.permissions,
            "relation": eff.relation,
            "admin": eff.admin,
            "grants": applying,
        }),
    ))
}
