use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::config::DataConfig;

pub async fn create_pool(config: &DataConfig) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(config.data.database.max_connections)
        .connect(&config.data.database.source)
        .await?;

    tracing::info!("database connection pool created");
    Ok(pool)
}

pub async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    tracing::info!("database migrations applied");
    Ok(())
}
