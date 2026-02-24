use std::net::SocketAddr;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

pub async fn start_frontend_server(
    addr: SocketAddr,
    dist_path: &str,
) -> Result<(), anyhow::Error> {
    let app = Router::new()
        .fallback_service(ServeDir::new(dist_path))
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Frontend server listening on {}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}
