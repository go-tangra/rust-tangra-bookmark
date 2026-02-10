use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::authz::relations::{Relation, ResourceType, SubjectType};

#[derive(Debug, sqlx::FromRow)]
pub struct PermissionRow {
    pub id: i32,
    pub tenant_id: i32,
    pub resource_type: String,
    pub resource_id: String,
    pub relation: String,
    pub subject_type: String,
    pub subject_id: String,
    pub granted_by: Option<i32>,
    pub expires_at: Option<DateTime<Utc>>,
    pub create_time: DateTime<Utc>,
}

#[derive(Clone)]
pub struct PermissionRepo {
    pool: PgPool,
}

impl PermissionRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn has_permission(
        &self,
        tenant_id: i32,
        resource_type: ResourceType,
        resource_id: &str,
        subject_type: SubjectType,
        subject_id: &str,
    ) -> anyhow::Result<Option<PermissionRow>> {
        let row = sqlx::query_as::<_, PermissionRow>(
            r#"
            SELECT * FROM bookmark_permissions
            WHERE tenant_id = $1
              AND resource_type = $2
              AND resource_id = $3
              AND subject_type = $4
              AND subject_id = $5
            LIMIT 1
            "#,
        )
        .bind(tenant_id)
        .bind(resource_type.as_str())
        .bind(resource_id)
        .bind(subject_type.as_str())
        .bind(subject_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_permission(
        &self,
        tenant_id: i32,
        resource_type: ResourceType,
        resource_id: &str,
        relation: Relation,
        subject_type: SubjectType,
        subject_id: &str,
        granted_by: Option<i32>,
        expires_at: Option<DateTime<Utc>>,
    ) -> anyhow::Result<PermissionRow> {
        let row = sqlx::query_as::<_, PermissionRow>(
            r#"
            INSERT INTO bookmark_permissions
                (tenant_id, resource_type, resource_id, relation, subject_type, subject_id, granted_by, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (tenant_id, resource_type, resource_id, relation, subject_type, subject_id) DO UPDATE
                SET granted_by = EXCLUDED.granted_by, expires_at = EXCLUDED.expires_at
            RETURNING *
            "#,
        )
        .bind(tenant_id)
        .bind(resource_type.as_str())
        .bind(resource_id)
        .bind(relation.as_str())
        .bind(subject_type.as_str())
        .bind(subject_id)
        .bind(granted_by)
        .bind(expires_at)
        .fetch_one(&self.pool)
        .await?;

        Ok(row)
    }

    pub async fn delete_permission(
        &self,
        tenant_id: i32,
        resource_type: ResourceType,
        resource_id: &str,
        relation: Option<Relation>,
        subject_type: SubjectType,
        subject_id: &str,
    ) -> anyhow::Result<u64> {
        let result = if let Some(rel) = relation {
            sqlx::query(
                r#"
                DELETE FROM bookmark_permissions
                WHERE tenant_id = $1
                  AND resource_type = $2
                  AND resource_id = $3
                  AND relation = $4
                  AND subject_type = $5
                  AND subject_id = $6
                "#,
            )
            .bind(tenant_id)
            .bind(resource_type.as_str())
            .bind(resource_id)
            .bind(rel.as_str())
            .bind(subject_type.as_str())
            .bind(subject_id)
            .execute(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                DELETE FROM bookmark_permissions
                WHERE tenant_id = $1
                  AND resource_type = $2
                  AND resource_id = $3
                  AND subject_type = $4
                  AND subject_id = $5
                "#,
            )
            .bind(tenant_id)
            .bind(resource_type.as_str())
            .bind(resource_id)
            .bind(subject_type.as_str())
            .bind(subject_id)
            .execute(&self.pool)
            .await?
        };

        Ok(result.rows_affected())
    }

    pub async fn delete_all_for_resource(
        &self,
        tenant_id: i32,
        resource_type: ResourceType,
        resource_id: &str,
    ) -> anyhow::Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM bookmark_permissions
            WHERE tenant_id = $1 AND resource_type = $2 AND resource_id = $3
            "#,
        )
        .bind(tenant_id)
        .bind(resource_type.as_str())
        .bind(resource_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    pub async fn get_direct_permissions(
        &self,
        tenant_id: i32,
        resource_type: ResourceType,
        resource_id: &str,
    ) -> anyhow::Result<Vec<PermissionRow>> {
        let rows = sqlx::query_as::<_, PermissionRow>(
            r#"
            SELECT * FROM bookmark_permissions
            WHERE tenant_id = $1 AND resource_type = $2 AND resource_id = $3
            ORDER BY create_time DESC
            "#,
        )
        .bind(tenant_id)
        .bind(resource_type.as_str())
        .bind(resource_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    pub async fn list_resources_by_subject(
        &self,
        tenant_id: i32,
        subject_type: SubjectType,
        subject_id: &str,
        resource_type: ResourceType,
    ) -> anyhow::Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT DISTINCT resource_id FROM bookmark_permissions
            WHERE tenant_id = $1
              AND subject_type = $2
              AND subject_id = $3
              AND resource_type = $4
            "#,
        )
        .bind(tenant_id)
        .bind(subject_type.as_str())
        .bind(subject_id)
        .bind(resource_type.as_str())
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn list_permissions_filtered(
        &self,
        tenant_id: i32,
        resource_type: Option<ResourceType>,
        resource_id: Option<&str>,
        subject_type: Option<SubjectType>,
        subject_id: Option<&str>,
        page: u32,
        page_size: u32,
    ) -> anyhow::Result<(Vec<PermissionRow>, i64)> {
        let offset = (page.saturating_sub(1)) * page_size;

        // Build dynamic query with optional filters
        let mut conditions = vec!["tenant_id = $1".to_string()];
        let mut param_idx = 2u32;

        if resource_type.is_some() {
            conditions.push(format!("resource_type = ${param_idx}"));
            param_idx += 1;
        }
        if resource_id.is_some() {
            conditions.push(format!("resource_id = ${param_idx}"));
            param_idx += 1;
        }
        if subject_type.is_some() {
            conditions.push(format!("subject_type = ${param_idx}"));
            param_idx += 1;
        }
        if subject_id.is_some() {
            conditions.push(format!("subject_id = ${param_idx}"));
            param_idx += 1;
        }

        let where_clause = conditions.join(" AND ");
        let count_sql = format!("SELECT COUNT(*) FROM bookmark_permissions WHERE {where_clause}");
        let query_sql = format!(
            "SELECT * FROM bookmark_permissions WHERE {where_clause} ORDER BY create_time DESC LIMIT ${param_idx} OFFSET ${}",
            param_idx + 1
        );

        // Execute count query
        let mut count_query = sqlx::query_as::<_, (i64,)>(&count_sql).bind(tenant_id);
        if let Some(rt) = &resource_type {
            count_query = count_query.bind(rt.as_str());
        }
        if let Some(ri) = resource_id {
            count_query = count_query.bind(ri);
        }
        if let Some(st) = &subject_type {
            count_query = count_query.bind(st.as_str());
        }
        if let Some(si) = subject_id {
            count_query = count_query.bind(si);
        }
        let (total,) = count_query.fetch_one(&self.pool).await?;

        // Execute data query
        let mut data_query = sqlx::query_as::<_, PermissionRow>(&query_sql).bind(tenant_id);
        if let Some(rt) = &resource_type {
            data_query = data_query.bind(rt.as_str());
        }
        if let Some(ri) = resource_id {
            data_query = data_query.bind(ri);
        }
        if let Some(st) = &subject_type {
            data_query = data_query.bind(st.as_str());
        }
        if let Some(si) = subject_id {
            data_query = data_query.bind(si);
        }
        data_query = data_query.bind(page_size as i64).bind(offset as i64);
        let rows = data_query.fetch_all(&self.pool).await?;

        Ok((rows, total))
    }
}
