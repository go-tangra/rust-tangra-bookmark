//! `/api/bookmark/v1/bookmarks`: list, create, read, update, remove
//! (FR-001–FR-007).

use axum::body::Body;
use axum::extract::{Path, RawQuery, State};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use http::{HeaderMap, StatusCode};
use serde::Serialize;
use serde_json::json;
use tangra_sdk::auth::User;
use tangra_sdk::error::{self, ApiError};
use tangra_sdk::listquery::{self, Page, Query};
use uuid::Uuid;

use super::input::{self, MAX_BODY_BYTES, MAX_QUERY, MAX_TAG, MAX_TAG_FILTER};
use super::{
    ApiState, access, audit_ok, begin, commit, correlation, internal, refuse, require_csrf, subjects,
};
use crate::authz::{self, Action, Grant, Permissions, RESOURCE, Relation, SubjectType};
use crate::store::{self, Bookmark, BookmarkFilter};

/// A bookmark as the API returns it: the stored fields and the caller's
/// permissions on it (FR-007).
#[derive(Debug, Clone, Serialize)]
pub struct BookmarkView {
    /// ID.
    pub id: Uuid,
    /// URL.
    pub url: String,
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Tags.
    pub tags: Vec<String>,
    /// Creator's user ID.
    pub created_by: String,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Updated.
    pub updated_at: DateTime<Utc>,
    /// The caller's permissions.
    pub permissions: Permissions,
}

impl BookmarkView {
    /// `b` with `permissions`.
    pub fn new(b: Bookmark, permissions: Permissions) -> Self {
        Self {
            id: b.id,
            url: b.url,
            title: b.title,
            description: b.description,
            tags: b.tags,
            created_by: b.created_by,
            created_at: b.created_at,
            updated_at: b.updated_at,
            permissions,
        }
    }
}

/// `GET /bookmarks`: one page of the bookmarks the caller can read.
pub async fn list(
    State(s): State<ApiState>,
    user: User,
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
    let req = listquery::parse(&q, &s.bookmark_spec).map_err(|e| ApiError::invalid_param(e.param))?;
    let text = q.get("q").trim().to_owned();
    if text.chars().count() > MAX_QUERY || text.chars().any(char::is_control) {
        return Err(ApiError::invalid_param("q"));
    }
    let mut tags = Vec::new();
    for t in input::query_all(&raw, "tag") {
        let t = t.trim().to_owned();
        if t.is_empty() || t.chars().count() > MAX_TAG {
            return Err(ApiError::invalid_param("tag"));
        }
        if !tags.contains(&t) {
            tags.push(t);
        }
    }
    if tags.len() > MAX_TAG_FILTER {
        return Err(ApiError::invalid_param("tag"));
    }
    let subj = subjects(&user);
    let mut tx = begin(&s, &subj).await?;
    let (rows, total, served) = store::list_bookmarks(
        &mut tx,
        &subj,
        &BookmarkFilter { tags, q: text },
        &req,
        &s.bookmark_spec,
        Utc::now(),
    )
    .await
    .map_err(|e| internal(&s.log, "list bookmarks", &e))?;
    let items = rows
        .into_iter()
        .map(|(b, rels)| BookmarkView::new(b, authz::from_relations(&subj, &rels)))
        .collect();
    Ok(error::json(StatusCode::OK, &Page::new(items, total, &served)))
}

/// `POST /bookmarks`: creates a bookmark owned by the caller.
pub async fn create(
    State(s): State<ApiState>,
    user: User,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    let nb = input::new_bookmark(&input::read_object(body, MAX_BODY_BYTES).await?)?;
    let subj = subjects(&user);
    let corr = correlation(&headers);
    let now = Utc::now();
    let b = Bookmark {
        id: Uuid::now_v7(),
        url: nb.url,
        title: nb.title,
        description: nb.description,
        tags: nb.tags,
        created_by: subj.user_id.clone(),
        created_at: now,
        updated_at: now,
    };
    let owner = Grant {
        id: Uuid::now_v7(),
        resource_type: RESOURCE,
        resource_id: b.id.to_string(),
        subject_type: SubjectType::User,
        subject_id: subj.user_id.clone(),
        relation: Relation::Owner,
        granted_by: subj.user_id.clone(),
        granted_at: now,
        expires_at: None,
    };
    let mut tx = begin(&s, &subj).await?;
    store::insert_bookmark(&mut tx, &subj.tenant_id, &b)
        .await
        .map_err(|e| super::db_error(&s.log, "insert bookmark", &e))?;
    // The creator becomes the owner (FR-002).
    store::insert_grant(&mut tx, &subj.tenant_id, &owner)
        .await
        .map_err(|e| internal(&s.log, "owner grant", &e))?;
    audit_ok(
        &s,
        &mut tx,
        &subj,
        "bookmark_created",
        ("bookmark", b.id.to_string()),
        &corr,
        json!({"tags": b.tags.len()}),
    )
    .await?;
    commit(&s, tx).await?;
    let perms = authz::effective(&subj, &[owner], &b.id.to_string(), now).permissions;
    Ok(error::json(StatusCode::CREATED, &BookmarkView::new(b, perms)))
}

/// `GET /bookmarks/{id}`.
pub async fn get_one(
    State(s): State<ApiState>,
    user: User,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = input::bookmark_id(&id).ok_or_else(ApiError::not_found)?;
    let subj = subjects(&user);
    let mut tx = begin(&s, &subj).await?;
    let (b, eff, _) = access(&s, &mut tx, &subj, id, Utc::now()).await?;
    Ok(error::json(
        StatusCode::OK,
        &BookmarkView::new(b, eff.permissions),
    ))
}

/// `PUT /bookmarks/{id}`: changes the given fields (write).
pub async fn update(
    State(s): State<ApiState>,
    user: User,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    let p = input::patch(&input::read_object(body, MAX_BODY_BYTES).await?)?;
    let id = input::bookmark_id(&id).ok_or_else(ApiError::not_found)?;
    let subj = subjects(&user);
    let corr = correlation(&headers);
    let now = Utc::now();
    let mut tx = begin(&s, &subj).await?;
    let (mut b, eff, _) = access(&s, &mut tx, &subj, id, now).await?;
    if !eff.has(Action::Write) {
        drop(tx);
        return Err(refuse(&s, &subj, id.to_string(), Action::Write, &corr).await);
    }
    let fields = p.names();
    if let Some(u) = p.url {
        b.url = u;
    }
    if let Some(t) = p.title {
        b.title = t;
    }
    if let Some(d) = p.description {
        b.description = d;
    }
    if let Some(t) = p.tags {
        b.tags = t;
    }
    b.updated_at = now;
    if !store::update_bookmark(&mut tx, &subj.tenant_id, &b)
        .await
        .map_err(|e| internal(&s.log, "update bookmark", &e))?
    {
        return Err(ApiError::not_found());
    }
    audit_ok(
        &s,
        &mut tx,
        &subj,
        "bookmark_updated",
        ("bookmark", id.to_string()),
        &corr,
        json!({"fields": fields}),
    )
    .await?;
    commit(&s, tx).await?;
    Ok(error::json(
        StatusCode::OK,
        &BookmarkView::new(b, eff.permissions),
    ))
}

/// `POST /bookmarks/{id}/remove`: removes the bookmark and its grants
/// (delete).
pub async fn remove(
    State(s): State<ApiState>,
    user: User,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    let id = input::bookmark_id(&id).ok_or_else(ApiError::not_found)?;
    let subj = subjects(&user);
    let corr = correlation(&headers);
    let mut tx = begin(&s, &subj).await?;
    let (_, eff, _) = access(&s, &mut tx, &subj, id, Utc::now()).await?;
    if !eff.has(Action::Delete) {
        drop(tx);
        return Err(refuse(&s, &subj, id.to_string(), Action::Delete, &corr).await);
    }
    let (removed, grants) = store::delete_bookmark(&mut tx, &subj.tenant_id, id)
        .await
        .map_err(|e| internal(&s.log, "delete bookmark", &e))?;
    if !removed {
        return Err(ApiError::not_found());
    }
    audit_ok(
        &s,
        &mut tx,
        &subj,
        "bookmark_deleted",
        ("bookmark", id.to_string()),
        &corr,
        json!({"grants": grants}),
    )
    .await?;
    commit(&s, tx).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
