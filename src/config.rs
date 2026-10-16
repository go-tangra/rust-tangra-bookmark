//! The bookmark configuration: the framework keys (`service_name`,
//! `identity`, `authz`, `server`, `admin`, `discovery`, `limits`, ...) plus
//! the module's sections, shaped like the deployer's
//! (`internal/config/config.go`):
//!
//! ```yaml
//! gateway: { service: gateway, issuer: https://portal.example.org:8443 }
//! db:
//!   dsn: postgres://bookmark_app:...@bookmark-db:5432/bookmark?sslmode=verify-full&sslrootcert=/tls/ca.pem
//!   migrate_dsn: postgres://postgres:...@bookmark-db:5432/bookmark?sslmode=verify-full&sslrootcert=/tls/ca.pem
//!   max_conns: 16
//! events: { enabled: false }       # publish bookmark.shared on the event bus
//! valkey: { addresses: [...], username: bookmark, password: ..., ca_file: ... }
//! enroll: { enabled: true, enroll_url: ..., lcm_grpc: ..., tenant_id: ..., token_file: ..., state_file: ... }
//! limits_bookmark: { backup_max_bytes: 16777216 }
//! ```
//!
//! One YAML file, decoded strictly over these defaults; there are no
//! environment overrides (the install renders `${...}` placeholders).

use serde::{Deserialize, Serialize};
use tangra_sdk::config::{Config, ConfigError, EnrollConfig, FrameworkConfig};
use tangra_sdk::db::DbConfig;
use tangra_sdk::events::ValkeyConfig;

/// The whole configuration file.
pub type BookmarkFile = Config<BookmarkConfig>;

/// The module sections. Defaults live on each section (yaml.v3 decodes over
/// the whole default).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct BookmarkConfig {
    /// The gateway service and the platform token issuer.
    pub gateway: Gateway,
    /// PostgreSQL (application and migration roles).
    pub db: DbConfig,
    /// The platform event bus (required when `events.enabled`).
    pub valkey: Option<ValkeyConfig>,
    /// Realtime events.
    pub events: Events,
    /// Network enrolment with lcm (production identity).
    pub enroll: EnrollConfig,
    /// Module limits.
    pub limits_bookmark: Limits,
}

/// `gateway:`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gateway {
    /// The gateway's service name (registration target and the only peer
    /// served the help bundle).
    pub service: String,
    /// The platform token issuer (`https://` origin).
    pub issuer: String,
}

/// `events:`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Events {
    /// Publish `bookmark.shared` to `platform:events:<tenant>` (off by
    /// default: the module needs no event bus).
    pub enabled: bool,
}

/// `limits_bookmark:`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Largest backup import body (64 KiB - 256 MiB; the framework's
    /// `limits.max_request_bytes` must allow it too).
    pub backup_max_bytes: usize,
}

impl Default for Gateway {
    fn default() -> Self {
        Self {
            service: tangra_sdk::app::GATEWAY_SERVICE.to_owned(),
            issuer: String::new(),
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            backup_max_bytes: 16 << 20,
        }
    }
}

fn invalid(msg: &str) -> ConfigError {
    ConfigError(format!("config: {msg}"))
}

/// Whether `s` is an `https://host[:port]` origin.
fn https_origin(s: &str) -> bool {
    match s.parse::<http::Uri>() {
        Ok(u) => {
            u.scheme_str() == Some("https") && u.host().is_some_and(|h| !h.is_empty()) && u.query().is_none()
        }
        Err(_) => false,
    }
}

impl BookmarkConfig {
    /// Checks the module sections (the framework keys are checked by
    /// [`validate`]).
    pub fn validate(&self, fw: &FrameworkConfig) -> Result<(), ConfigError> {
        let prod = fw.is_production();
        if self.gateway.service.is_empty() {
            return Err(invalid("gateway.service is required"));
        }
        if !https_origin(&self.gateway.issuer) {
            return Err(invalid("gateway.issuer must be an https origin"));
        }
        self.db.validate(prod)?;
        if self.events.enabled {
            let v = self
                .valkey
                .as_ref()
                .ok_or_else(|| invalid("valkey is required when events.enabled"))?;
            v.validate(prod).map_err(|e| ConfigError(e.to_string()))?;
        }
        if self.enroll.enabled {
            for (k, v) in [
                ("enroll.enroll_url", &self.enroll.enroll_url),
                ("enroll.lcm_grpc", &self.enroll.lcm_grpc),
                ("enroll.token_file", &self.enroll.token_file),
            ] {
                if v.is_empty() {
                    return Err(invalid(&format!("{k} is required when enroll.enabled")));
                }
            }
            self.enroll.validate(&fw.trust_domain, prod)?;
        }
        if !((64 << 10)..=(256 << 20)).contains(&self.limits_bookmark.backup_max_bytes) {
            return Err(invalid(
                "limits_bookmark.backup_max_bytes must be within [65536, 268435456]",
            ));
        }
        Ok(())
    }

    /// Accepted insecure opt-outs of the module sections (logged at start;
    /// the framework's own are logged by the app).
    pub fn warnings(&self) -> Vec<String> {
        let mut w = self.db.warnings();
        w.extend(self.enroll.warnings());
        if self.events.enabled
            && let Some(v) = &self.valkey
        {
            w.extend(v.warnings());
        }
        w
    }
}

/// Checks the framework keys and every module section.
pub fn validate(cfg: &BookmarkFile) -> Result<(), ConfigError> {
    cfg.framework.validate()?;
    cfg.module.validate(&cfg.framework)
}

/// Reads and validates `path`.
pub fn load(path: &str) -> Result<BookmarkFile, ConfigError> {
    let cfg = BookmarkFile::load(path)?;
    validate(&cfg)?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "service_name: bookmark\ntrust_domain: example.org\n\
identity: {provider: file, file: {cert: c.pem, key: k.pem, bundle: b.pem}}\n\
authz: {source: file, path: policy.yaml}\n\
gateway: {issuer: \"https://portal.example.org:8443\"}\n";

    const DB: &str = "db: {dsn: \"postgres://bookmark_app@db/bookmark?sslmode=disable\"}\n";

    fn parse(extra: &str) -> Result<BookmarkFile, ConfigError> {
        let cfg = BookmarkFile::from_yaml(format!("{BASE}{extra}").as_bytes())?;
        validate(&cfg)?;
        Ok(cfg)
    }

    #[test]
    fn defaults_and_rules() {
        assert_eq!(parse("").unwrap_err().0, "config: db.dsn is required");
        let cfg = parse(DB).unwrap();
        assert_eq!(cfg.module.gateway.service, "gateway");
        assert!(!cfg.module.events.enabled, "events are opt-in");
        assert_eq!(cfg.module.db.max_conns, 16);
        assert_eq!(cfg.module.limits_bookmark.backup_max_bytes, 16 << 20);
        assert_eq!(
            cfg.module.warnings().len(),
            1,
            "unverified db TLS is warned about"
        );
        assert!(
            parse(&format!("{DB}bookmark_typo: 1\n")).is_err(),
            "unknown keys refused"
        );
        assert!(parse(&format!("{DB}db2: {{}}\n")).is_err());
        let e = parse(&format!("{DB}events: {{enabled: true}}\n")).unwrap_err();
        assert_eq!(e.0, "config: valkey is required when events.enabled");
        let cfg = parse(&format!(
            "{DB}events: {{enabled: true}}\nvalkey: {{addresses: [\"v:6379\"], allow_plaintext: true}}\n"
        ))
        .unwrap();
        assert_eq!(cfg.module.warnings().len(), 2);
        let e = parse(&format!("{DB}enroll: {{enabled: true}}\n")).unwrap_err();
        assert_eq!(e.0, "config: enroll.enroll_url is required when enroll.enabled");
        let e = parse(&format!("{DB}limits_bookmark: {{backup_max_bytes: 10}}\n")).unwrap_err();
        assert!(e.0.contains("backup_max_bytes"));
        for bad in ["http://x", "https://", "portal.example.org", "https://x?y=1", ""] {
            assert!(!https_origin(bad), "{bad}");
        }
    }

    #[test]
    fn production_requires_verified_database_tls() {
        let e = parse(&format!("env: production\n{DB}")).unwrap_err();
        assert_eq!(
            e.0,
            "config: db.dsn must use sslmode=verify-full (or verify-ca) in production"
        );
        parse(
            "env: production\ndb: {dsn: \"postgres://bookmark_app@db/bookmark?sslmode=verify-full&sslrootcert=/tls/ca.pem\"}\n",
        )
        .unwrap();
    }
}
