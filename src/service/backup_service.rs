use std::collections::HashMap;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::service::bookmark_service::proto::backup_service_server::BackupService;
use crate::service::bookmark_service::proto::{
    EntityImportResult, ExportBackupRequest, ExportBackupResponse, ImportBackupRequest,
    ImportBackupResponse, RestoreMode,
};
use crate::service::context_helper::extract_context;

const BACKUP_MODULE: &str = "bookmark";
const BACKUP_VERSION: &str = "1.0";

pub struct BackupServiceImpl {
    pool: PgPool,
}

impl BackupServiceImpl {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

// --- Serde models for JSON backup data ---

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupData {
    module: String,
    version: String,
    exported_at: String,
    tenant_id: u32,
    full_backup: bool,
    data: BackupEntities,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupEntities {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    bookmarks: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    permissions: Vec<serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BookmarkBackup {
    id: String,
    tenant_id: i32,
    url: String,
    title: String,
    description: String,
    tags: Vec<String>,
    created_by: Option<i32>,
    create_time: String,
    update_time: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PermissionBackup {
    tenant_id: i32,
    resource_type: String,
    resource_id: String,
    relation: String,
    subject_type: String,
    subject_id: String,
    granted_by: Option<i32>,
    expires_at: Option<String>,
    create_time: String,
}

#[tonic::async_trait]
impl BackupService for BackupServiceImpl {
    async fn export_backup(
        &self,
        request: Request<ExportBackupRequest>,
    ) -> Result<Response<ExportBackupResponse>, Status> {
        let ctx = extract_context(&request)?;
        let req = request.into_inner();

        let is_platform_admin = ctx
            .role_ids
            .iter()
            .any(|r| r == "platform:admin" || r == "super:admin");

        let (tenant_id, full_backup) = match req.tenant_id {
            Some(0) | None if is_platform_admin => (0_i32, true),
            Some(tid) => (tid as i32, false),
            _ => (ctx.tenant_id, false),
        };

        tracing::info!(
            tenant_id,
            full_backup,
            "exporting bookmark backup"
        );

        // Export bookmarks
        let bookmarks: Vec<serde_json::Value> = if full_backup {
            let rows = sqlx::query_as::<_, BookmarkRow>(
                "SELECT * FROM bookmark_bookmarks ORDER BY create_time",
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Status::internal(format!("query bookmarks: {e}")))?;
            rows.into_iter().map(|r| bookmark_to_json(&r)).collect()
        } else {
            let rows = sqlx::query_as::<_, BookmarkRow>(
                "SELECT * FROM bookmark_bookmarks WHERE tenant_id = $1 ORDER BY create_time",
            )
            .bind(tenant_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Status::internal(format!("query bookmarks: {e}")))?;
            rows.into_iter().map(|r| bookmark_to_json(&r)).collect()
        };

        // Export permissions
        let permissions: Vec<serde_json::Value> = if full_backup {
            let rows = sqlx::query_as::<_, PermissionRow>(
                "SELECT * FROM bookmark_permissions ORDER BY create_time",
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Status::internal(format!("query permissions: {e}")))?;
            rows.into_iter().map(|r| permission_to_json(&r)).collect()
        } else {
            let rows = sqlx::query_as::<_, PermissionRow>(
                "SELECT * FROM bookmark_permissions WHERE tenant_id = $1 ORDER BY create_time",
            )
            .bind(tenant_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Status::internal(format!("query permissions: {e}")))?;
            rows.into_iter().map(|r| permission_to_json(&r)).collect()
        };

        let backup = BackupData {
            module: BACKUP_MODULE.to_string(),
            version: BACKUP_VERSION.to_string(),
            exported_at: Utc::now().to_rfc3339(),
            tenant_id: tenant_id as u32,
            full_backup,
            data: BackupEntities {
                bookmarks,
                permissions,
            },
        };

        let data = serde_json::to_vec(&backup)
            .map_err(|e| Status::internal(format!("serialize backup: {e}")))?;

        let mut entity_counts = HashMap::new();
        entity_counts.insert("bookmarks".to_string(), backup.data.bookmarks.len() as i64);
        entity_counts.insert(
            "permissions".to_string(),
            backup.data.permissions.len() as i64,
        );

        let now = Utc::now();
        Ok(Response::new(ExportBackupResponse {
            data,
            module: BACKUP_MODULE.to_string(),
            version: BACKUP_VERSION.to_string(),
            exported_at: Some(prost_types::Timestamp {
                seconds: now.timestamp(),
                nanos: now.timestamp_subsec_nanos() as i32,
            }),
            tenant_id: tenant_id as u32,
            entity_counts,
        }))
    }

    async fn import_backup(
        &self,
        request: Request<ImportBackupRequest>,
    ) -> Result<Response<ImportBackupResponse>, Status> {
        let _ctx = extract_context(&request)?;
        let req = request.into_inner();

        let mode = RestoreMode::try_from(req.mode).unwrap_or(RestoreMode::Skip);

        let backup: BackupData = serde_json::from_slice(&req.data)
            .map_err(|e| Status::invalid_argument(format!("invalid backup data: {e}")))?;

        if backup.module != BACKUP_MODULE {
            return Err(Status::invalid_argument(format!(
                "backup module mismatch: expected {BACKUP_MODULE}, got {}",
                backup.module
            )));
        }

        tracing::info!(
            module = %backup.module,
            version = %backup.version,
            mode = ?mode,
            "importing bookmark backup"
        );

        let mut warnings = Vec::new();
        let mut results = Vec::new();

        // Import bookmarks
        let bookmark_result =
            self.import_bookmarks(&backup.data.bookmarks, mode, &mut warnings).await;
        results.push(bookmark_result);

        // Import permissions (after bookmarks so references exist)
        let permission_result = self
            .import_permissions(&backup.data.permissions, mode, &mut warnings)
            .await;
        results.push(permission_result);

        let success = results.iter().all(|r| r.failed == 0);

        Ok(Response::new(ImportBackupResponse {
            success,
            results,
            warnings,
        }))
    }
}

impl BackupServiceImpl {
    async fn import_bookmarks(
        &self,
        items: &[serde_json::Value],
        mode: RestoreMode,
        warnings: &mut Vec<String>,
    ) -> EntityImportResult {
        let mut created = 0i64;
        let mut updated = 0i64;
        let mut skipped = 0i64;
        let mut failed = 0i64;

        for item in items {
            let bk: BookmarkBackup = match serde_json::from_value(item.clone()) {
                Ok(b) => b,
                Err(e) => {
                    warnings.push(format!("skip invalid bookmark: {e}"));
                    failed += 1;
                    continue;
                }
            };

            let id = match Uuid::parse_str(&bk.id) {
                Ok(id) => id,
                Err(e) => {
                    warnings.push(format!("skip bookmark with bad UUID {}: {e}", bk.id));
                    failed += 1;
                    continue;
                }
            };

            // Check if exists
            let existing: Option<(Uuid,)> =
                sqlx::query_as("SELECT id FROM bookmark_bookmarks WHERE id = $1")
                    .bind(id)
                    .fetch_optional(&self.pool)
                    .await
                    .unwrap_or(None);

            if existing.is_some() {
                match mode {
                    RestoreMode::Skip => {
                        skipped += 1;
                        continue;
                    }
                    RestoreMode::Overwrite => {
                        let res = sqlx::query(
                            r#"UPDATE bookmark_bookmarks
                               SET url = $2, title = $3, description = $4, tags = $5,
                                   created_by = $6, tenant_id = $7, update_time = NOW()
                               WHERE id = $1"#,
                        )
                        .bind(id)
                        .bind(&bk.url)
                        .bind(&bk.title)
                        .bind(&bk.description)
                        .bind(&bk.tags)
                        .bind(bk.created_by)
                        .bind(bk.tenant_id)
                        .execute(&self.pool)
                        .await;

                        match res {
                            Ok(_) => updated += 1,
                            Err(e) => {
                                warnings.push(format!("update bookmark {}: {e}", bk.id));
                                failed += 1;
                            }
                        }
                    }
                }
            } else {
                let res = sqlx::query(
                    r#"INSERT INTO bookmark_bookmarks (id, tenant_id, url, title, description, tags, created_by)
                       VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                )
                .bind(id)
                .bind(bk.tenant_id)
                .bind(&bk.url)
                .bind(&bk.title)
                .bind(&bk.description)
                .bind(&bk.tags)
                .bind(bk.created_by)
                .execute(&self.pool)
                .await;

                match res {
                    Ok(_) => created += 1,
                    Err(e) => {
                        warnings.push(format!("create bookmark {}: {e}", bk.id));
                        failed += 1;
                    }
                }
            }
        }

        EntityImportResult {
            entity_type: "bookmarks".to_string(),
            total: items.len() as i64,
            created,
            updated,
            skipped,
            failed,
        }
    }

    async fn import_permissions(
        &self,
        items: &[serde_json::Value],
        mode: RestoreMode,
        warnings: &mut Vec<String>,
    ) -> EntityImportResult {
        let mut created = 0i64;
        let mut updated = 0i64;
        let mut skipped = 0i64;
        let mut failed = 0i64;

        for item in items {
            let perm: PermissionBackup = match serde_json::from_value(item.clone()) {
                Ok(p) => p,
                Err(e) => {
                    warnings.push(format!("skip invalid permission: {e}"));
                    failed += 1;
                    continue;
                }
            };

            // Check if exists (by unique constraint columns)
            let existing: Option<(i32,)> = sqlx::query_as(
                r#"SELECT id FROM bookmark_permissions
                   WHERE tenant_id = $1 AND resource_type = $2 AND resource_id = $3
                     AND relation = $4 AND subject_type = $5 AND subject_id = $6"#,
            )
            .bind(perm.tenant_id)
            .bind(&perm.resource_type)
            .bind(&perm.resource_id)
            .bind(&perm.relation)
            .bind(&perm.subject_type)
            .bind(&perm.subject_id)
            .fetch_optional(&self.pool)
            .await
            .unwrap_or(None);

            if existing.is_some() {
                match mode {
                    RestoreMode::Skip => {
                        skipped += 1;
                        continue;
                    }
                    RestoreMode::Overwrite => {
                        let expires_at = perm
                            .expires_at
                            .as_deref()
                            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            .map(|dt| dt.with_timezone(&Utc));

                        let res = sqlx::query(
                            r#"UPDATE bookmark_permissions
                               SET granted_by = $7, expires_at = $8
                               WHERE tenant_id = $1 AND resource_type = $2 AND resource_id = $3
                                 AND relation = $4 AND subject_type = $5 AND subject_id = $6"#,
                        )
                        .bind(perm.tenant_id)
                        .bind(&perm.resource_type)
                        .bind(&perm.resource_id)
                        .bind(&perm.relation)
                        .bind(&perm.subject_type)
                        .bind(&perm.subject_id)
                        .bind(perm.granted_by)
                        .bind(expires_at)
                        .execute(&self.pool)
                        .await;

                        match res {
                            Ok(_) => updated += 1,
                            Err(e) => {
                                warnings.push(format!("update permission: {e}"));
                                failed += 1;
                            }
                        }
                    }
                }
            } else {
                let expires_at = perm
                    .expires_at
                    .as_deref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc));

                let res = sqlx::query(
                    r#"INSERT INTO bookmark_permissions
                       (tenant_id, resource_type, resource_id, relation, subject_type, subject_id, granted_by, expires_at)
                       VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
                )
                .bind(perm.tenant_id)
                .bind(&perm.resource_type)
                .bind(&perm.resource_id)
                .bind(&perm.relation)
                .bind(&perm.subject_type)
                .bind(&perm.subject_id)
                .bind(perm.granted_by)
                .bind(expires_at)
                .execute(&self.pool)
                .await;

                match res {
                    Ok(_) => created += 1,
                    Err(e) => {
                        warnings.push(format!("create permission: {e}"));
                        failed += 1;
                    }
                }
            }
        }

        EntityImportResult {
            entity_type: "permissions".to_string(),
            total: items.len() as i64,
            created,
            updated,
            skipped,
            failed,
        }
    }
}

// --- SQLx row types for raw queries ---

#[derive(sqlx::FromRow)]
struct BookmarkRow {
    id: Uuid,
    tenant_id: i32,
    url: String,
    title: String,
    description: String,
    tags: Vec<String>,
    created_by: Option<i32>,
    create_time: chrono::DateTime<Utc>,
    update_time: chrono::DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct PermissionRow {
    #[allow(dead_code)]
    id: i32,
    tenant_id: i32,
    resource_type: String,
    resource_id: String,
    relation: String,
    subject_type: String,
    subject_id: String,
    granted_by: Option<i32>,
    expires_at: Option<chrono::DateTime<Utc>>,
    create_time: chrono::DateTime<Utc>,
}

fn bookmark_to_json(row: &BookmarkRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.id.to_string(),
        "tenantId": row.tenant_id,
        "url": row.url,
        "title": row.title,
        "description": row.description,
        "tags": row.tags,
        "createdBy": row.created_by,
        "createTime": row.create_time.to_rfc3339(),
        "updateTime": row.update_time.to_rfc3339(),
    })
}

fn permission_to_json(row: &PermissionRow) -> serde_json::Value {
    serde_json::json!({
        "tenantId": row.tenant_id,
        "resourceType": row.resource_type,
        "resourceId": row.resource_id,
        "relation": row.relation,
        "subjectType": row.subject_type,
        "subjectId": row.subject_id,
        "grantedBy": row.granted_by,
        "expiresAt": row.expires_at.map(|dt| dt.to_rfc3339()),
        "createTime": row.create_time.to_rfc3339(),
    })
}
