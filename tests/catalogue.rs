//! The catalogue descriptor and the join bundle (SDK examples/notes
//! `tests/catalogue.rs`, deployer `cmd/deployersvc/catalogue_test.go`):
//! `tangra-module.yaml` is valid, its host inputs accept values of the shape
//! operators give, the bundle templates use only placeholders the gateway
//! fills and render to a valid production configuration and policy, the
//! database bootstrap creates the NOBYPASSRLS application role, and the
//! permissions command prints the manifest's permissions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::Path;

use tangra_bookmark::config::{self, BookmarkFile};
use tangra_bookmark::manifest;
use tangra_sdk::catalogue;
use tangra_sdk::policy::Policy;

const DIR: &str = env!("CARGO_MANIFEST_DIR");

/// Values of the shape the gateway fills in: core values, generated values
/// and the host inputs.
fn join_values() -> BTreeMap<&'static str, String> {
    BTreeMap::from([
        ("TRUST_DOMAIN", "infra.example.org".into()),
        ("GATEWAY_ISSUER", "https://portal.example.org:8443".into()),
        (
            "LCM_ENROLL_URL",
            "https://portal.example.org:8443/api/lcm/v1/enroll".into(),
        ),
        ("LCM_GRPC", "portal.example.org:9945".into()),
        ("GATEWAY_GRPC", "portal.example.org:9643".into()),
        ("AUTH_GRPC", "portal.example.org:9543".into()),
        ("MESH_TENANT_ID", "00000000-0000-0000-0000-000000000001".into()),
        ("MODULE_IMAGE", "ghcr.io/go-tangra/rust-tangra-bookmark".into()),
        ("MODULE_VERSION", "4.0.0".into()),
        ("GEN_PASSWORD_1", "a".repeat(32)),
        ("GEN_PASSWORD_2", "b".repeat(32)),
        ("MODULE_ADVERTISE_HOST", "bookmark.example.org".into()),
        ("MODULE_BIND_IP", "10.0.0.5".into()),
    ])
}

fn bundle_file(name: &str) -> String {
    std::fs::read_to_string(Path::new(DIR).join("deploy/bundle").join(name)).unwrap()
}

fn render(name: &str) -> String {
    let raw = bundle_file(name);
    let re = regex::Regex::new(r"\$\{([A-Z][A-Z0-9_]*)\}").unwrap();
    let values = join_values();
    let mut missing = Vec::new();
    let out = re.replace_all(&raw, |c: &regex::Captures<'_>| {
        let k = &c[1];
        values.get(k).cloned().unwrap_or_else(|| {
            missing.push(k.to_owned());
            String::new()
        })
    });
    assert!(
        missing.is_empty(),
        "{name} uses placeholders the gateway does not fill: {missing:?}"
    );
    out.into_owned()
}

#[test]
fn descriptor_is_valid() {
    let raw = std::fs::read(Path::new(DIR).join("tangra-module.yaml")).unwrap();
    let d = catalogue::parse_descriptor(&raw).unwrap();
    assert_eq!(d.module, manifest::MODULE);
    assert_eq!(d.display_name, manifest::DISPLAY_NAME);
    assert!(d.routes.prefixes[0].starts_with(manifest::PREFIX));
    assert_eq!(d.bundle.tls_hosts, ["bookmark-db"]);
    let values = join_values();
    for i in &d.host_inputs {
        let re = regex::Regex::new(&i.pattern).unwrap();
        assert!(
            re.is_match(&values[i.key.as_str()]),
            "host input {} refuses {:?}",
            i.key,
            values[i.key.as_str()]
        );
        assert!(!catalogue::reserved(&i.key), "{}", i.key);
    }
    for t in &d.bundle.templates {
        assert!(
            Path::new(DIR).join(&d.bundle.dir).join(t).is_file(),
            "template {t}"
        );
    }
}

#[test]
fn join_bundle_renders_to_a_valid_production_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, render("config.yaml")).unwrap();
    let compose = render("compose.yaml");
    let cfg: BookmarkFile = config::load(path.to_str().unwrap()).unwrap();
    assert!(cfg.framework.is_production());
    assert!(cfg.module.warnings().is_empty(), "{:?}", cfg.module.warnings());
    assert_eq!(cfg.module.gateway.issuer, "https://portal.example.org:8443");
    assert!(cfg.module.enroll.enabled && !cfg.module.events.enabled);
    assert!(cfg.module.db.dsn.starts_with("postgres://bookmark_app:"));
    assert!(cfg.module.db.migrate_dsn.starts_with("postgres://postgres:"));
    for dsn in [&cfg.module.db.dsn, &cfg.module.db.migrate_dsn] {
        assert!(dsn.contains("@bookmark-db:5432/bookmark?sslmode=verify-full&sslrootcert=/tls/ca.pem"));
    }
    assert!(
        cfg.framework.limits.max_request_bytes
            >= i64::try_from(cfg.module.limits_bookmark.backup_max_bytes).unwrap(),
        "backup imports must fit the request limit"
    );
    // The mesh ports in compose are the configured ones; the healthcheck
    // uses the binary (the image has no wget/curl).
    for port in ["9975", "9976"] {
        assert!(
            cfg.framework.server.grpc_addr.ends_with(port) || cfg.framework.server.http_addr.ends_with(port)
        );
        assert!(compose.contains(&format!("10.0.0.5:{port}:{port}")), "{port}");
    }
    assert!(compose.contains(r#"["CMD", "bookmarksvc", "--healthcheck""#));
    assert!(compose.contains("./init-db.sh:/docker-entrypoint-initdb.d/"));
    let pol = render("policy.yaml");
    Policy::load(pol.as_bytes()).unwrap();
    assert!(pol.contains("spiffe://infra.example.org/svc/gateway"));
}

/// init-db.sh creates the application role the config's `db.dsn` uses,
/// without BYPASSRLS, with the password compose passes it.
#[test]
fn database_bootstrap_creates_the_application_role() {
    let sh = bundle_file("init-db.sh");
    assert!(sh.contains("CREATE ROLE bookmark_app LOGIN PASSWORD '${BOOKMARK_APP_PASSWORD}' NOBYPASSRLS;"));
    assert!(sh.contains("--dbname bookmark"));
    assert!(bundle_file("compose.yaml").contains("BOOKMARK_APP_PASSWORD: ${GEN_PASSWORD_2}"));
    assert!(bundle_file("config.yaml").contains("bookmark_app:${GEN_PASSWORD_2}@"));
}

/// The release job's `permissions-command`.
#[test]
fn permissions_command_prints_the_manifest_permissions() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_bookmark-catalogue-permissions"))
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        catalogue::permissions_json(manifest::permission_refs())
    );
}
