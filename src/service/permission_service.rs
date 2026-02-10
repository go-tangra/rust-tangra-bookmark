use tonic::{Request, Response, Status};

use crate::authz::checker::Checker;
use crate::authz::relations::{Permission, Relation, ResourceType, SubjectType};
use crate::data::permission_repo::PermissionRow;
use crate::service::context_helper::extract_context;

// Re-use the proto module from bookmark_service (same package)
use crate::service::bookmark_service::proto;

use proto::bookmark_permission_service_server::BookmarkPermissionService;
use proto::{
    CheckAccessRequest, CheckAccessResponse, GetEffectivePermissionsRequest,
    GetEffectivePermissionsResponse, GrantAccessRequest, GrantAccessResponse,
    ListAccessibleResourcesRequest, ListAccessibleResourcesResponse, ListPermissionsRequest,
    ListPermissionsResponse, PermissionTuple, RevokeAccessRequest,
};

pub struct PermissionServiceImpl {
    checker: Checker,
}

impl PermissionServiceImpl {
    pub fn new(checker: Checker) -> Self {
        Self { checker }
    }
}

#[tonic::async_trait]
impl BookmarkPermissionService for PermissionServiceImpl {
    async fn grant_access(
        &self,
        request: Request<GrantAccessRequest>,
    ) -> Result<Response<GrantAccessResponse>, Status> {
        let ctx = extract_context(&request)?;
        let req = request.into_inner();

        let resource_type = ResourceType::from_proto(req.resource_type)
            .ok_or_else(|| Status::invalid_argument("invalid resource_type"))?;
        let relation = Relation::from_proto(req.relation)
            .ok_or_else(|| Status::invalid_argument("invalid relation"))?;
        let subject_type = SubjectType::from_proto(req.subject_type)
            .ok_or_else(|| Status::invalid_argument("invalid subject_type"))?;

        if req.resource_id.is_empty() || req.subject_id.is_empty() {
            return Err(Status::invalid_argument(
                "resource_id and subject_id are required",
            ));
        }

        // Require SHARE permission to grant access
        self.checker
            .can_share(
                ctx.tenant_id,
                &ctx.user_id,
                &req.resource_id,
                &ctx.role_ids,
            )
            .await?;

        let expires_at = req.expires_at.map(|ts| {
            chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32)
                .unwrap_or_else(chrono::Utc::now)
        });

        let row = self
            .checker
            .engine()
            .store()
            .create_permission(
                ctx.tenant_id,
                resource_type,
                &req.resource_id,
                relation,
                subject_type,
                &req.subject_id,
                ctx.user_id.parse::<i32>().ok(),
                expires_at,
            )
            .await
            .map_err(|e| Status::internal(format!("database error: {e}")))?;

        Ok(Response::new(GrantAccessResponse {
            permission: Some(row_to_proto(row)),
        }))
    }

    async fn revoke_access(
        &self,
        request: Request<RevokeAccessRequest>,
    ) -> Result<Response<()>, Status> {
        let ctx = extract_context(&request)?;
        let req = request.into_inner();

        let resource_type = ResourceType::from_proto(req.resource_type)
            .ok_or_else(|| Status::invalid_argument("invalid resource_type"))?;
        let subject_type = SubjectType::from_proto(req.subject_type)
            .ok_or_else(|| Status::invalid_argument("invalid subject_type"))?;
        let relation = req.relation.and_then(Relation::from_proto);

        // Require SHARE permission to revoke access
        self.checker
            .can_share(
                ctx.tenant_id,
                &ctx.user_id,
                &req.resource_id,
                &ctx.role_ids,
            )
            .await?;

        self.checker
            .engine()
            .store()
            .delete_permission(
                ctx.tenant_id,
                resource_type,
                &req.resource_id,
                relation,
                subject_type,
                &req.subject_id,
            )
            .await
            .map_err(|e| Status::internal(format!("database error: {e}")))?;

        Ok(Response::new(()))
    }

    async fn list_permissions(
        &self,
        request: Request<ListPermissionsRequest>,
    ) -> Result<Response<ListPermissionsResponse>, Status> {
        let ctx = extract_context(&request)?;
        let req = request.into_inner();

        let resource_type = req.resource_type.and_then(ResourceType::from_proto);
        let subject_type = req.subject_type.and_then(SubjectType::from_proto);
        let page = req.page.unwrap_or(1).max(1);
        let page_size = req.page_size.unwrap_or(20).min(100);

        let (rows, total) = self
            .checker
            .engine()
            .store()
            .list_permissions_filtered(
                ctx.tenant_id,
                resource_type,
                req.resource_id.as_deref(),
                subject_type,
                req.subject_id.as_deref(),
                page,
                page_size,
            )
            .await
            .map_err(|e| Status::internal(format!("database error: {e}")))?;

        let permissions: Vec<PermissionTuple> = rows.into_iter().map(row_to_proto).collect();

        Ok(Response::new(ListPermissionsResponse {
            permissions,
            total: total as u32,
        }))
    }

    async fn check_access(
        &self,
        request: Request<CheckAccessRequest>,
    ) -> Result<Response<CheckAccessResponse>, Status> {
        let ctx = extract_context(&request)?;
        let req = request.into_inner();

        let resource_type = ResourceType::from_proto(req.resource_type)
            .ok_or_else(|| Status::invalid_argument("invalid resource_type"))?;
        let permission = Permission::from_proto(req.permission)
            .ok_or_else(|| Status::invalid_argument("invalid permission"))?;

        let check_ctx = crate::authz::engine::CheckContext {
            tenant_id: ctx.tenant_id,
            user_id: req.user_id.clone(),
            resource_type,
            resource_id: req.resource_id.clone(),
            permission,
        };

        let result = self.checker.engine().check(&check_ctx, &ctx.role_ids).await;

        Ok(Response::new(CheckAccessResponse {
            allowed: result.allowed,
            reason: Some(result.reason),
        }))
    }

    async fn list_accessible_resources(
        &self,
        request: Request<ListAccessibleResourcesRequest>,
    ) -> Result<Response<ListAccessibleResourcesResponse>, Status> {
        let ctx = extract_context(&request)?;
        let req = request.into_inner();

        let _resource_type = ResourceType::from_proto(req.resource_type)
            .ok_or_else(|| Status::invalid_argument("invalid resource_type"))?;

        let ids = self
            .checker
            .list_accessible_bookmarks(ctx.tenant_id, &req.user_id, &ctx.role_ids)
            .await
            .map_err(|e| Status::internal(format!("authz error: {e}")))?;

        Ok(Response::new(ListAccessibleResourcesResponse {
            total: ids.len() as u32,
            resource_ids: ids,
        }))
    }

    async fn get_effective_permissions(
        &self,
        request: Request<GetEffectivePermissionsRequest>,
    ) -> Result<Response<GetEffectivePermissionsResponse>, Status> {
        let ctx = extract_context(&request)?;
        let req = request.into_inner();

        let (permissions, highest_relation) = self
            .checker
            .get_effective_permissions(
                ctx.tenant_id,
                &req.user_id,
                &req.resource_id,
                &ctx.role_ids,
            )
            .await;

        Ok(Response::new(GetEffectivePermissionsResponse {
            permissions: permissions.iter().map(|p| p.to_proto()).collect(),
            highest_relation: highest_relation.map(|r| r.to_proto()).unwrap_or(0),
        }))
    }
}

fn row_to_proto(row: PermissionRow) -> PermissionTuple {
    PermissionTuple {
        id: row.id as u32,
        tenant_id: row.tenant_id as u32,
        resource_type: ResourceType::from_proto(1)
            .map(|_| {
                // Map string back to proto enum value
                match row.resource_type.as_str() {
                    "RESOURCE_TYPE_BOOKMARK" => 1,
                    _ => 0,
                }
            })
            .unwrap_or(0),
        resource_id: row.resource_id,
        relation: match row.relation.as_str() {
            "RELATION_OWNER" => 1,
            "RELATION_EDITOR" => 2,
            "RELATION_VIEWER" => 3,
            "RELATION_SHARER" => 4,
            _ => 0,
        },
        subject_type: match row.subject_type.as_str() {
            "SUBJECT_TYPE_USER" => 1,
            "SUBJECT_TYPE_ROLE" => 2,
            "SUBJECT_TYPE_TENANT" => 3,
            _ => 0,
        },
        subject_id: row.subject_id,
        granted_by: row.granted_by.map(|v| v as u32),
        expires_at: row.expires_at.map(|ts| prost_types::Timestamp {
            seconds: ts.timestamp(),
            nanos: ts.timestamp_subsec_nanos() as i32,
        }),
        create_time: Some(prost_types::Timestamp {
            seconds: row.create_time.timestamp(),
            nanos: row.create_time.timestamp_subsec_nanos() as i32,
        }),
    }
}
