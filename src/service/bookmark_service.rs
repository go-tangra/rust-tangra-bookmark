use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::authz::checker::Checker;
use crate::authz::relations::{Relation, ResourceType, SubjectType};
use crate::data::bookmark_repo::{BookmarkRepo, BookmarkRow};
use crate::service::context_helper::extract_context;

/// Generated proto types.
pub mod proto {
    tonic::include_proto!("bookmark.service.v1");
}

use proto::bookmark_service_server::BookmarkService;
use proto::{
    Bookmark, CreateBookmarkRequest, DeleteBookmarkRequest, GetBookmarkRequest,
    ListBookmarksRequest, ListBookmarksResponse, UpdateBookmarkRequest,
};

pub struct BookmarkServiceImpl {
    repo: BookmarkRepo,
    checker: Checker,
}

impl BookmarkServiceImpl {
    pub fn new(repo: BookmarkRepo, checker: Checker) -> Self {
        Self { repo, checker }
    }
}

#[tonic::async_trait]
impl BookmarkService for BookmarkServiceImpl {
    async fn create_bookmark(
        &self,
        request: Request<CreateBookmarkRequest>,
    ) -> Result<Response<Bookmark>, Status> {
        let ctx = extract_context(&request)?;
        let req = request.into_inner();

        if req.url.is_empty() {
            return Err(Status::invalid_argument("url is required"));
        }

        let row = self
            .repo
            .create(
                ctx.tenant_id,
                &req.url,
                &req.title,
                &req.description,
                &req.tags,
                ctx.user_id.parse::<i32>().ok(),
            )
            .await
            .map_err(|e| Status::internal(format!("database error: {e}")))?;

        // Grant OWNER permission to the creator
        let _ = self
            .checker
            .engine()
            .store()
            .create_permission(
                ctx.tenant_id,
                ResourceType::Bookmark,
                &row.id.to_string(),
                Relation::Owner,
                SubjectType::User,
                &ctx.user_id,
                ctx.user_id.parse::<i32>().ok(),
                None,
            )
            .await;

        Ok(Response::new(row_to_proto(row)))
    }

    async fn get_bookmark(
        &self,
        request: Request<GetBookmarkRequest>,
    ) -> Result<Response<Bookmark>, Status> {
        let ctx = extract_context(&request)?;
        let req = request.into_inner();

        let id = parse_uuid(&req.id)?;

        // Check read permission
        self.checker
            .can_read(ctx.tenant_id, &ctx.user_id, &req.id, &ctx.role_ids)
            .await?;

        let row = self
            .repo
            .get_by_id(id)
            .await
            .map_err(|e| Status::internal(format!("database error: {e}")))?
            .ok_or_else(|| Status::not_found("bookmark not found"))?;

        Ok(Response::new(row_to_proto(row)))
    }

    async fn list_bookmarks(
        &self,
        request: Request<ListBookmarksRequest>,
    ) -> Result<Response<ListBookmarksResponse>, Status> {
        let ctx = extract_context(&request)?;
        let req = request.into_inner();

        let page = req.page.unwrap_or(1).max(1);
        let page_size = req.page_size.unwrap_or(20).min(100);

        // Get accessible bookmark IDs from authz
        let accessible_ids = self
            .checker
            .list_accessible_bookmarks(ctx.tenant_id, &ctx.user_id, &ctx.role_ids)
            .await
            .map_err(|e| Status::internal(format!("authz error: {e}")))?;

        let uuids: Vec<Uuid> = accessible_ids
            .iter()
            .filter_map(|id| Uuid::parse_str(id).ok())
            .collect();

        let (rows, total) = self
            .repo
            .list_by_ids(ctx.tenant_id, &uuids, page, page_size)
            .await
            .map_err(|e| Status::internal(format!("database error: {e}")))?;

        let bookmarks: Vec<Bookmark> = rows.into_iter().map(row_to_proto).collect();

        Ok(Response::new(ListBookmarksResponse {
            bookmarks,
            total: total as u32,
        }))
    }

    async fn update_bookmark(
        &self,
        request: Request<UpdateBookmarkRequest>,
    ) -> Result<Response<Bookmark>, Status> {
        let ctx = extract_context(&request)?;
        let req = request.into_inner();

        let id = parse_uuid(&req.id)?;

        // Check write permission
        self.checker
            .can_write(ctx.tenant_id, &ctx.user_id, &req.id, &ctx.role_ids)
            .await?;

        let tags = if req.update_tags {
            Some(req.tags.as_slice())
        } else {
            None
        };

        let row = self
            .repo
            .update(
                id,
                req.url.as_deref(),
                req.title.as_deref(),
                req.description.as_deref(),
                tags,
            )
            .await
            .map_err(|e| Status::internal(format!("database error: {e}")))?
            .ok_or_else(|| Status::not_found("bookmark not found"))?;

        Ok(Response::new(row_to_proto(row)))
    }

    async fn delete_bookmark(
        &self,
        request: Request<DeleteBookmarkRequest>,
    ) -> Result<Response<()>, Status> {
        let ctx = extract_context(&request)?;
        let req = request.into_inner();

        let id = parse_uuid(&req.id)?;

        // Check delete permission
        self.checker
            .can_delete(ctx.tenant_id, &ctx.user_id, &req.id, &ctx.role_ids)
            .await?;

        let deleted = self
            .repo
            .delete(id)
            .await
            .map_err(|e| Status::internal(format!("database error: {e}")))?;

        if !deleted {
            return Err(Status::not_found("bookmark not found"));
        }

        // Clean up all permissions for this bookmark
        let _ = self
            .checker
            .engine()
            .store()
            .delete_all_for_resource(ctx.tenant_id, ResourceType::Bookmark, &req.id)
            .await;

        Ok(Response::new(()))
    }
}

fn row_to_proto(row: BookmarkRow) -> Bookmark {
    Bookmark {
        id: row.id.to_string(),
        tenant_id: row.tenant_id as u32,
        url: row.url,
        title: row.title,
        description: row.description,
        tags: row.tags,
        created_by: row.created_by.map(|v| v as u32),
        create_time: Some(prost_types::Timestamp {
            seconds: row.create_time.timestamp(),
            nanos: row.create_time.timestamp_subsec_nanos() as i32,
        }),
        update_time: Some(prost_types::Timestamp {
            seconds: row.update_time.timestamp(),
            nanos: row.update_time.timestamp_subsec_nanos() as i32,
        }),
    }
}

fn parse_uuid(s: &str) -> Result<Uuid, Status> {
    Uuid::parse_str(s).map_err(|_| Status::invalid_argument("invalid UUID"))
}
