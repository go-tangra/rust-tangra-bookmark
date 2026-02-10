use tonic::Status;

use crate::authz::engine::{CheckContext, Engine};
use crate::authz::relations::{Permission, Relation, ResourceType};

/// High-level convenience API for permission checks.
#[derive(Clone)]
pub struct Checker {
    engine: Engine,
}

impl Checker {
    pub fn new(engine: Engine) -> Self {
        Self { engine }
    }

    pub async fn can_read(
        &self,
        tenant_id: i32,
        user_id: &str,
        resource_id: &str,
        role_ids: &[String],
    ) -> Result<(), Status> {
        self.require_permission(
            tenant_id, user_id, ResourceType::Bookmark, resource_id, Permission::Read, role_ids,
        )
        .await
    }

    pub async fn can_write(
        &self,
        tenant_id: i32,
        user_id: &str,
        resource_id: &str,
        role_ids: &[String],
    ) -> Result<(), Status> {
        self.require_permission(
            tenant_id, user_id, ResourceType::Bookmark, resource_id, Permission::Write, role_ids,
        )
        .await
    }

    pub async fn can_delete(
        &self,
        tenant_id: i32,
        user_id: &str,
        resource_id: &str,
        role_ids: &[String],
    ) -> Result<(), Status> {
        self.require_permission(
            tenant_id, user_id, ResourceType::Bookmark, resource_id, Permission::Delete, role_ids,
        )
        .await
    }

    pub async fn can_share(
        &self,
        tenant_id: i32,
        user_id: &str,
        resource_id: &str,
        role_ids: &[String],
    ) -> Result<(), Status> {
        self.require_permission(
            tenant_id, user_id, ResourceType::Bookmark, resource_id, Permission::Share, role_ids,
        )
        .await
    }

    pub async fn require_permission(
        &self,
        tenant_id: i32,
        user_id: &str,
        resource_type: ResourceType,
        resource_id: &str,
        permission: Permission,
        role_ids: &[String],
    ) -> Result<(), Status> {
        let ctx = CheckContext {
            tenant_id,
            user_id: user_id.to_string(),
            resource_type,
            resource_id: resource_id.to_string(),
            permission,
        };

        let result = self.engine.check(&ctx, role_ids).await;
        if !result.allowed {
            return Err(Status::permission_denied(format!(
                "access denied: {}",
                result.reason
            )));
        }
        Ok(())
    }

    pub async fn list_accessible_bookmarks(
        &self,
        tenant_id: i32,
        user_id: &str,
        role_ids: &[String],
    ) -> anyhow::Result<Vec<String>> {
        self.engine
            .list_accessible_resources(tenant_id, user_id, ResourceType::Bookmark, role_ids)
            .await
    }

    pub async fn get_effective_permissions(
        &self,
        tenant_id: i32,
        user_id: &str,
        resource_id: &str,
        role_ids: &[String],
    ) -> (Vec<Permission>, Option<Relation>) {
        let ctx = CheckContext {
            tenant_id,
            user_id: user_id.to_string(),
            resource_type: ResourceType::Bookmark,
            resource_id: resource_id.to_string(),
            permission: Permission::Read, // placeholder, overridden inside
        };
        self.engine.get_effective_permissions(&ctx, role_ids).await
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }
}
