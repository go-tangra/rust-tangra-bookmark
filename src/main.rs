#![allow(dead_code, clippy::result_large_err)]

mod authz;
mod cert;
mod config;
mod data;
mod frontend;
mod middleware;
mod registration;
mod service;

use std::net::SocketAddr;
use std::path::Path;

use tokio::signal;
use tokio::sync::watch;
use tonic::transport::Server;

use crate::authz::checker::Checker;
use crate::authz::engine::Engine;
use crate::config::{DataConfig, LoggerConfig, ServerConfig};
use crate::data::bookmark_repo::BookmarkRepo;
use crate::data::permission_repo::PermissionRepo;
use crate::service::bookmark_service::proto::backup_service_server::BackupServiceServer;
use crate::service::bookmark_service::proto::bookmark_permission_service_server::BookmarkPermissionServiceServer;
use crate::service::bookmark_service::proto::bookmark_service_server::BookmarkServiceServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Load config
    let config_dir = std::env::var("CONFIG_DIR").unwrap_or_else(|_| "configs".to_string());

    let logger_cfg: LoggerConfig =
        config::load_config(Path::new(&config_dir).join("logger.yaml").as_ref())?;
    let server_cfg: ServerConfig =
        config::load_config(Path::new(&config_dir).join("server.yaml").as_ref())?;
    let data_cfg: DataConfig =
        config::load_config(Path::new(&config_dir).join("data.yaml").as_ref())?;

    // 2. Init tracing/logging
    init_tracing(&logger_cfg.logger);
    tracing::info!("starting bookmark service v1.0.0");

    // 3. Load mTLS certs (optional)
    let tls_config = cert::load_tls_config();

    // 4. Create DB pool, run migrations
    let pool = data::db::create_pool(&data_cfg).await?;
    data::db::run_migrations(&pool).await?;

    // 5. Create repos, authz engine, services
    let bookmark_repo = BookmarkRepo::new(pool.clone());
    let permission_repo = PermissionRepo::new(pool.clone());
    let engine = Engine::new(permission_repo);
    let checker = Checker::new(engine);

    let bookmark_svc = service::bookmark_service::BookmarkServiceImpl::new(
        bookmark_repo,
        checker.clone(),
    );
    let permission_svc =
        service::permission_service::PermissionServiceImpl::new(checker.clone());
    let backup_svc = service::backup_service::BackupServiceImpl::new(pool.clone());

    // 6. Start frontend HTTP server (serves Module Federation assets)
    let frontend_dist = std::env::var("FRONTEND_DIST_PATH")
        .unwrap_or_else(|_| "/app/frontend-dist".to_string());
    if std::path::Path::new(&frontend_dist).exists() {
        let frontend_addr: SocketAddr = server_cfg
            .server
            .http
            .as_ref()
            .map(|h| h.addr.as_str())
            .unwrap_or("0.0.0.0:9701")
            .parse()?;
        let dist_path = frontend_dist.clone();
        tokio::spawn(async move {
            if let Err(e) = frontend::start_frontend_server(frontend_addr, &dist_path).await {
                tracing::error!(error = %e, "Frontend server failed");
            }
        });
        tracing::info!(path = %frontend_dist, "Frontend serving static files");
    } else {
        tracing::info!(path = %frontend_dist, "No frontend dist directory found, skipping frontend server");
    }

    // 7. Build tonic server
    let addr: SocketAddr = server_cfg.server.grpc.addr.parse()?;

    let mut server = Server::builder();

    // 8. Apply mTLS if available
    if let Some(tls) = tls_config {
        server = server.tls_config(tls)?;
        tracing::info!("gRPC server configured with mTLS");
    } else {
        tracing::warn!("running without mTLS");
    }

    let router = server
        .add_service(BookmarkServiceServer::with_interceptor(
            bookmark_svc,
            middleware::audit::audit_interceptor,
        ))
        .add_service(BookmarkPermissionServiceServer::with_interceptor(
            permission_svc,
            middleware::audit::audit_interceptor,
        ))
        .add_service(BackupServiceServer::new(backup_svc));

    // 9. Start registration background task
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let reg_handle = registration::start_registration(shutdown_rx);

    // 10. Serve
    tracing::info!(addr = %addr, "gRPC server listening");

    let graceful = router.serve_with_shutdown(addr, async {
        shutdown_signal().await;
        tracing::info!("shutdown signal received");
    });

    graceful.await?;

    // 11. Graceful shutdown: unregister, drain connections
    let _ = shutdown_tx.send(true);
    let _ = reg_handle.await;

    tracing::info!("bookmark service stopped");
    Ok(())
}

fn init_tracing(logger: &config::LoggerSection) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&logger.level));

    match logger.format.as_str() {
        "json" => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .json()
                .init();
        }
        _ => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .init();
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
