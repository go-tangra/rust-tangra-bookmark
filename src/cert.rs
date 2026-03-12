use std::path::Path;
use tonic::transport::{Certificate, Identity, ServerTlsConfig};

/// Attempt to load mTLS configuration from certificate files.
/// Returns a tonic ServerTlsConfig if cert files exist, None otherwise.
pub fn load_tls_config() -> Option<ServerTlsConfig> {
    let certs_dir = std::env::var("CERTS_DIR").unwrap_or_else(|_| "/app/certs".to_string());

    // Convention-based paths: {certs_dir}/ca/ca.crt, {certs_dir}/bookmark-server/server.crt
    let ca_path = format!("{certs_dir}/ca/ca.crt");
    let cert_path = format!("{certs_dir}/bookmark-server/server.crt");
    let key_path = format!("{certs_dir}/bookmark-server/server.key");

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
