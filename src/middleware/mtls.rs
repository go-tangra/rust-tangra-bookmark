use tonic::{Request, Status};

/// Client identity extracted from mTLS certificate.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ClientInfo {
    pub common_name: String,
    pub organization: String,
}

/// mTLS interceptor that validates client certificates.
/// In production (with TLS), the transport layer (rustls ServerConfig) handles
/// certificate validation before requests reach the interceptor.
/// This interceptor logs requests for observability.
#[allow(dead_code)]
pub fn mtls_interceptor(req: Request<()>) -> Result<Request<()>, Status> {
    tracing::trace!("mTLS interceptor: request passed");
    Ok(req)
}
