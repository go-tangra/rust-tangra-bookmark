use std::net::SocketAddr;

use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

const OPENAPI_SPEC: &[u8] = include_bytes!("../assets/openapi.yaml");
const PROTO_DESCRIPTOR: &[u8] = include_bytes!("../assets/descriptor.bin");
const MENUS_YAML: &[u8] = include_bytes!("../assets/menus.yaml");

pub async fn start_frontend_server(
    addr: SocketAddr,
    dist_path: &str,
) -> Result<(), anyhow::Error> {
    let app = Router::new()
        .route("/openapi.yaml", get(serve_openapi))
        .route("/proto-descriptor", get(serve_proto_descriptor))
        .route("/menus.yaml", get(serve_menus))
        .fallback_service(ServeDir::new(dist_path))
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Frontend server listening on {}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn serve_openapi() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/yaml")],
        OPENAPI_SPEC,
    )
}

async fn serve_menus() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/yaml")],
        MENUS_YAML,
    )
}

async fn serve_proto_descriptor() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=descriptor.bin",
            ),
        ],
        PROTO_DESCRIPTOR,
    )
}
