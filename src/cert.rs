use std::path::Path;
use tonic::transport::{Certificate, Identity, ServerTlsConfig};

/// Attempt to load mTLS configuration from certificate files.
/// Returns a tonic ServerTlsConfig if cert files exist, None otherwise.
pub fn load_tls_config() -> Option<ServerTlsConfig> {
    let prefix = std::env::var("BOOKMARK_ENV_PREFIX").unwrap_or_else(|_| "BOOKMARK".to_string());

    let ca_path = std::env::var(format!("{prefix}_CA_CERT_PATH"))
        .unwrap_or_else(|_| "/app/certs/ca/ca.crt".to_string());
    let cert_path = std::env::var(format!("{prefix}_SERVER_CERT_PATH"))
        .unwrap_or_else(|_| "/app/certs/server/server.crt".to_string());
    let key_path = std::env::var(format!("{prefix}_SERVER_KEY_PATH"))
        .unwrap_or_else(|_| "/app/certs/server/server.key".to_string());

    if !Path::new(&ca_path).exists()
        || !Path::new(&cert_path).exists()
        || !Path::new(&key_path).exists()
    {
        tracing::warn!(
            ca = %ca_path,
            cert = %cert_path,
            key = %key_path,
            "TLS certificate files not found, running without mTLS"
        );
        return None;
    }

    match build_tls_config(&ca_path, &cert_path, &key_path) {
        Ok(config) => {
            tracing::info!("mTLS configuration loaded successfully");
            Some(config)
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to build TLS config, running without mTLS");
            None
        }
    }
}

fn build_tls_config(
    ca_path: &str,
    cert_path: &str,
    key_path: &str,
) -> anyhow::Result<ServerTlsConfig> {
    let ca_pem = std::fs::read(ca_path)?;
    let cert_pem = std::fs::read(cert_path)?;
    let key_pem = std::fs::read(key_path)?;

    let ca = Certificate::from_pem(ca_pem);
    let identity = Identity::from_pem(cert_pem, key_pem);

    let tls = ServerTlsConfig::new()
        .identity(identity)
        .client_ca_root(ca);

    Ok(tls)
}
