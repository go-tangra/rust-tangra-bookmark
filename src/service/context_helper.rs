use tonic::{Request, Status};

/// Metadata keys using Kratos x-md-global- prefix for cross-service propagation.
const MD_TENANT_ID: &str = "x-md-global-tenant-id";
const MD_USER_ID: &str = "x-md-global-user-id";
const MD_USERNAME: &str = "x-md-global-username";
const MD_ROLES: &str = "x-md-global-roles";

/// Extracted request context.
pub struct RequestContext {
    pub tenant_id: i32,
    pub user_id: String,
    pub username: String,
    pub role_ids: Vec<String>,
}

/// Extract tenant_id, user_id, username, and roles from gRPC metadata.
pub fn extract_context<T>(req: &Request<T>) -> Result<RequestContext, Status> {
    let tenant_id = get_metadata_value(req, MD_TENANT_ID)
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);

    let role_ids: Vec<String> = get_metadata_value(req, MD_ROLES)
        .map(|s| {
            s.split(',')
                .filter(|r| !r.is_empty())
                .map(|r| r.to_string())
                .collect()
        })
        .unwrap_or_default();

    let is_platform_admin = role_ids.iter().any(|r| r == "platform:admin" || r == "super:admin");

    if tenant_id == 0 && !is_platform_admin {
        return Err(Status::unauthenticated("missing or invalid tenant_id"));
    }

    let user_id = get_metadata_value(req, MD_USER_ID).unwrap_or_default();
    if user_id.is_empty() {
        return Err(Status::unauthenticated("missing user_id"));
    }

    let username = get_metadata_value(req, MD_USERNAME).unwrap_or_default();

    Ok(RequestContext {
        tenant_id,
        user_id,
        username,
        role_ids,
    })
}

fn get_metadata_value<T>(req: &Request<T>, key: &str) -> Option<String> {
    req.metadata()
        .get(key)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}
