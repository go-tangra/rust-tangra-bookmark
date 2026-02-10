use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
pub struct BookmarkRow {
    pub id: Uuid,
    pub tenant_id: i32,
    pub url: String,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub created_by: Option<i32>,
    pub create_time: DateTime<Utc>,
    pub update_time: DateTime<Utc>,
}

#[derive(Clone)]
pub struct BookmarkRepo {
    pool: PgPool,
}

impl BookmarkRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        tenant_id: i32,
        url: &str,
        title: &str,
        description: &str,
        tags: &[String],
        created_by: Option<i32>,
    ) -> anyhow::Result<BookmarkRow> {
        let row = sqlx::query_as::<_, BookmarkRow>(
            r#"
            INSERT INTO bookmark_bookmarks (tenant_id, url, title, description, tags, created_by)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
            "#,
        )
        .bind(tenant_id)
        .bind(url)
        .bind(title)
        .bind(description)
        .bind(tags)
        .bind(created_by)
        .fetch_one(&self.pool)
        .await?;

        Ok(row)
    }

    pub async fn get_by_id(&self, id: Uuid) -> anyhow::Result<Option<BookmarkRow>> {
        let row = sqlx::query_as::<_, BookmarkRow>(
            "SELECT * FROM bookmark_bookmarks WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    pub async fn list_by_tenant(
        &self,
        tenant_id: i32,
        page: u32,
        page_size: u32,
    ) -> anyhow::Result<(Vec<BookmarkRow>, i64)> {
        let offset = (page.saturating_sub(1)) * page_size;

        let total: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM bookmark_bookmarks WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_one(&self.pool)
        .await?;

        let rows = sqlx::query_as::<_, BookmarkRow>(
            r#"
            SELECT * FROM bookmark_bookmarks
            WHERE tenant_id = $1
            ORDER BY create_time DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(tenant_id)
        .bind(page_size as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok((rows, total.0))
    }

    pub async fn list_by_ids(
        &self,
        tenant_id: i32,
        ids: &[Uuid],
        page: u32,
        page_size: u32,
    ) -> anyhow::Result<(Vec<BookmarkRow>, i64)> {
        if ids.is_empty() {
            return Ok((vec![], 0));
        }

        let offset = (page.saturating_sub(1)) * page_size;

        let total: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM bookmark_bookmarks WHERE tenant_id = $1 AND id = ANY($2)",
        )
        .bind(tenant_id)
        .bind(ids)
        .fetch_one(&self.pool)
        .await?;

        let rows = sqlx::query_as::<_, BookmarkRow>(
            r#"
            SELECT * FROM bookmark_bookmarks
            WHERE tenant_id = $1 AND id = ANY($2)
            ORDER BY create_time DESC
            LIMIT $3 OFFSET $4
            "#,
        )
        .bind(tenant_id)
        .bind(ids)
        .bind(page_size as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok((rows, total.0))
    }

    pub async fn update(
        &self,
        id: Uuid,
        url: Option<&str>,
        title: Option<&str>,
        description: Option<&str>,
        tags: Option<&[String]>,
    ) -> anyhow::Result<Option<BookmarkRow>> {
        let row = sqlx::query_as::<_, BookmarkRow>(
            r#"
            UPDATE bookmark_bookmarks SET
                url = COALESCE($2, url),
                title = COALESCE($3, title),
                description = COALESCE($4, description),
                tags = COALESCE($5, tags),
                update_time = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(url)
        .bind(title)
        .bind(description)
        .bind(tags)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    pub async fn delete(&self, id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM bookmark_bookmarks WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }
}
