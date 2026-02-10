use chrono::Utc;

use crate::authz::relations::{Permission, Relation, ResourceType, SubjectType};
use crate::data::permission_repo::PermissionRepo;

/// Result of a permission check.
#[derive(Debug)]
pub struct CheckResult {
    pub allowed: bool,
    pub relation: Option<Relation>,
    pub reason: String,
}

/// Context for a permission check.
pub struct CheckContext {
    pub tenant_id: i32,
    pub user_id: String,
    pub resource_type: ResourceType,
    pub resource_id: String,
    pub permission: Permission,
}

/// Zanzibar-like permission engine (simplified for flat bookmarks — no hierarchy).
#[derive(Clone)]
pub struct Engine {
    store: PermissionRepo,
}

impl Engine {
    pub fn new(store: PermissionRepo) -> Self {
        Self { store }
    }

    /// Check performs a permission check following the Zanzibar algorithm:
    /// 1. Check direct user permission on resource
    /// 2. Check user's role permissions on resource
    /// 3. Check tenant-level permissions
    ///
    /// No hierarchy traversal needed (flat bookmarks).
    pub async fn check(&self, ctx: &CheckContext, role_ids: &[String]) -> CheckResult {
        tracing::debug!(
            user = %ctx.user_id,
            resource_type = ?ctx.resource_type,
            resource_id = %ctx.resource_id,
            permission = ?ctx.permission,
            "checking permission"
        );

        // Step 1: Check direct user permission
        if let Some(result) = self
            .check_direct(ctx, SubjectType::User, &ctx.user_id)
            .await
        {
            return result;
        }

        // Step 2: Check user's role permissions
        for role_id in role_ids {
            if let Some(result) = self.check_direct(ctx, SubjectType::Role, role_id).await {
                return result;
            }
        }

        // Step 3: Check tenant-level permissions
        if let Some(result) = self.check_direct(ctx, SubjectType::Tenant, "all").await {
            return result;
        }

        CheckResult {
            allowed: false,
            relation: None,
            reason: "no permission found".to_string(),
        }
    }

    async fn check_direct(
        &self,
        ctx: &CheckContext,
        subject_type: SubjectType,
        subject_id: &str,
    ) -> Option<CheckResult> {
        let row = match self
            .store
            .has_permission(
                ctx.tenant_id,
                ctx.resource_type,
                &ctx.resource_id,
                subject_type,
                subject_id,
            )
            .await
        {
            Ok(Some(row)) => row,
            Ok(None) => return None,
            Err(e) => {
                tracing::debug!(error = %e, "error checking permission");
                return None;
            }
        };

        // Check expiration
        if let Some(expires) = &row.expires_at {
            if *expires < Utc::now() {
                return Some(CheckResult {
                    allowed: false,
                    relation: None,
                    reason: "permission expired".to_string(),
                });
            }
        }

        // Check if relation grants the required permission
        let relation = Relation::from_str(&row.relation)?;
        if relation.grants(ctx.permission) {
            Some(CheckResult {
                allowed: true,
                relation: Some(relation),
                reason: "direct permission".to_string(),
            })
        } else {
            None
        }
    }

    pub async fn list_accessible_resources(
        &self,
        tenant_id: i32,
        user_id: &str,
        resource_type: ResourceType,
        role_ids: &[String],
    ) -> anyhow::Result<Vec<String>> {
        let mut accessible = std::collections::HashSet::new();

        // User's direct permissions
        let user_resources = self
            .store
            .list_resources_by_subject(tenant_id, SubjectType::User, user_id, resource_type)
            .await?;
        accessible.extend(user_resources);

        // Role permissions
        for role_id in role_ids {
            let role_resources = self
                .store
                .list_resources_by_subject(tenant_id, SubjectType::Role, role_id, resource_type)
                .await?;
            accessible.extend(role_resources);
        }

        // Tenant-level permissions
        let tenant_resources = self
            .store
            .list_resources_by_subject(tenant_id, SubjectType::Tenant, "all", resource_type)
            .await?;
        accessible.extend(tenant_resources);

        Ok(accessible.into_iter().collect())
    }

    pub async fn get_effective_permissions(
        &self,
        ctx: &CheckContext,
        role_ids: &[String],
    ) -> (Vec<Permission>, Option<Relation>) {
        let mut permissions = Vec::new();
        let mut highest_relation: Option<Relation> = None;

        for &perm in Permission::ALL {
            let check_ctx = CheckContext {
                tenant_id: ctx.tenant_id,
                user_id: ctx.user_id.clone(),
                resource_type: ctx.resource_type,
                resource_id: ctx.resource_id.clone(),
                permission: perm,
            };
            let result = self.check(&check_ctx, role_ids).await;
            if result.allowed {
                permissions.push(perm);
                if let Some(rel) = result.relation {
                    highest_relation = Some(match highest_relation {
                        Some(cur) if cur.is_at_least(rel) => cur,
                        _ => rel,
                    });
                }
            }
        }

        (permissions, highest_relation)
    }

    pub fn store(&self) -> &PermissionRepo {
        &self.store
    }
}
