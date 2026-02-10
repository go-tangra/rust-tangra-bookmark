use tonic::{Request, Status};

/// Audit logging interceptor that logs every RPC call.
/// Records operation, tenant, user, and timestamp.
pub fn audit_interceptor(req: Request<()>) -> Result<Request<()>, Status> {
    let tenant_id = extract_metadata(&req, "x-md-global-tenant-id");
    let user_id = extract_metadata(&req, "x-md-global-user-id");

    tracing::info!(
        service = "bookmark-service",
        tenant_id = %tenant_id,
        user_id = %user_id,
        timestamp = %chrono::Utc::now().to_rfc3339(),
        "audit: rpc call"
    );

    Ok(req)
}

fn extract_metadata(req: &Request<()>, key: &str) -> String {
    req.metadata()
        .get(key)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}
