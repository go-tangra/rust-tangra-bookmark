//! The manifest the module registers: valid for the gateway, pinned to its
//! version, consistent with the auth registration, every route carrying the
//! intended coarse permission, and every OpenAPI route served by the router
//! (deployer `pkg/deployermanifest` tests).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use axum::body::Body;
use tangra_bookmark::api::{self, ApiState};
use tangra_bookmark::manifest;
use tangra_sdk::auth::{self, StaticKeys, Verifier};
use tower::ServiceExt;

/// The gateway keeps one manifest per module and accepts a different one
/// only under a higher version. After bumping `manifest::VERSION`:
/// `TANGRA_UPDATE_MANIFEST_PIN=1 cargo test --test manifest pinned`.
#[test]
fn manifest_version_pinned() {
    let m = manifest::manifest().unwrap();
    let pin = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/manifest.sha256");
    tangra_sdk::manifest::pin::check(&m, pin).unwrap();
}

/// The coarse permission of every route (FR-020): what the gateway enforces.
#[test]
fn routes_carry_the_intended_permissions() {
    let m = manifest::manifest().unwrap();
    m.validate().unwrap();
    let p = m.to_proto().unwrap();
    assert_eq!(p.remote.unwrap().entry, "/m/bookmark/mf-manifest.json");
    assert_eq!(p.prefixes, ["/api/bookmark"]);
    let routes: Vec<String> = m
        .routes
        .iter()
        .map(|r| {
            format!(
                "{} {} {}",
                r.method,
                r.path,
                if r.public { "public" } else { &r.permission }
            )
        })
        .collect();
    assert_eq!(
        routes,
        [
            "GET /api/bookmark/v1/bookmarks bookmarks:read",
            "GET /api/bookmark/v1/bookmarks/{id} bookmarks:read",
            "GET /api/bookmark/v1/info public",
            "GET /api/bookmark/v1/permissions permissions:manage",
            "GET /api/bookmark/v1/permissions/effective bookmarks:read",
            "POST /api/bookmark/v1/backup/export backup:manage",
            "POST /api/bookmark/v1/backup/import backup:manage",
            "POST /api/bookmark/v1/bookmarks bookmarks:create",
            "POST /api/bookmark/v1/bookmarks/{id}/remove bookmarks:delete",
            "POST /api/bookmark/v1/permissions/check bookmarks:read",
            "POST /api/bookmark/v1/permissions/grant permissions:manage",
            "POST /api/bookmark/v1/permissions/revoke permissions:manage",
            "PUT /api/bookmark/v1/bookmarks/{id} bookmarks:update",
        ]
    );
    let import = m.routes.iter().find(|r| r.path.ends_with("/import")).unwrap();
    assert_eq!(import.max_body_bytes, 16 << 20);
    assert_eq!(import.timeout, std::time::Duration::from_secs(120));
    // Bundle policies allow GET, HEAD, POST and PUT only.
    assert!(
        m.routes
            .iter()
            .all(|r| ["GET", "POST", "PUT"].contains(&r.method.as_str()))
    );
}

/// Roles, grants, abilities and nav name declared permissions only, and
/// auth would accept the registration.
#[test]
fn registration_matches_the_manifest() {
    let perms = manifest::permission_refs();
    assert_eq!(
        perms,
        [
            "bookmarks:read",
            "bookmarks:create",
            "bookmarks:update",
            "bookmarks:delete",
            "permissions:manage",
            "backup:manage"
        ]
    );
    let reg = manifest::registration();
    reg.validate().unwrap();
    let slugs: Vec<&str> = reg.roles.iter().map(|r| r.slug.as_str()).collect();
    assert_eq!(slugs, ["administrator", "viewer"]);
    for r in &reg.roles {
        assert!(r.permissions.iter().all(|p| perms.contains(p)), "{}", r.slug);
    }
    for (role, ps) in &reg.builtin_grants {
        assert!(auth::BUILTIN_ROLES.contains(&role.as_str()));
        assert!(ps.iter().all(|p| perms.contains(p)), "{role}");
    }
    assert!(!reg.builtin_grants["member"].contains(&"backup:manage".to_owned()));
    assert_eq!(reg.builtin_grants["auditor"], ["bookmarks:read"]);
    let req = reg.request();
    assert!(req.declares_roles && req.declares_builtin_grants);
    assert_eq!(req.module, "bookmark");
    for a in manifest::abilities() {
        assert!(perms.contains(&a.requires), "{}", a.requires);
    }
}

/// Nav icons must be in the `@go-tangra/ui` kit set (the shell's stylesheet
/// only carries those): this is the subset of the kit's `ICONS`
/// (go-tangra `ui/kit/src/icons.ts`, v4.10.0) the module uses.
#[test]
fn nav_icons_in_kit_set() {
    const KIT: [&str; 3] = [
        "mdi-link-variant",
        "mdi-account-key-outline",
        "mdi-database-arrow-up-outline",
    ];
    for n in manifest::nav() {
        assert!(KIT.contains(&n.icon.as_str()), "{}", n.icon);
        assert!(manifest::permission_refs().contains(&n.requires));
    }
}

fn router() -> axum::Router {
    let verifier = Arc::new(Verifier::new(
        auth::Config {
            issuer: "https://portal.example.org".into(),
            ..auth::Config::default()
        },
        Arc::new(StaticKeys(Default::default())),
        None,
    ));
    // Never connected: protected routes refuse before touching storage.
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://bookmark_app@127.0.0.1:1/bookmark")
        .unwrap();
    let state = ApiState::new(
        pool,
        None,
        tangra_sdk::log::Logger::new(std::io::sink(), tangra_sdk::log::Level::Info),
        "test",
        16 << 20,
    );
    api::router(state, verifier)
}

/// Every declared operation is implemented: without a token a protected
/// route is 401 (it matched and asked for the user), the public route 200;
/// an undeclared path is the catch-all 404 and a wrong method 405.
#[tokio::test]
async fn every_openapi_route_is_served() {
    let m = manifest::manifest().unwrap();
    for r in &m.routes {
        let path = r.path.replace("{id}", "0190");
        let req = http::Request::builder()
            .method(r.method.as_str())
            .uri(&path)
            .header("x-csrf-token", "t")
            .body(Body::empty())
            .unwrap();
        let resp = router().oneshot(req).await.unwrap();
        let want = if r.public { 200 } else { 401 };
        assert_eq!(resp.status().as_u16(), want, "{} {}", r.method, r.path);
        assert_eq!(resp.headers()["x-frame-options"], "DENY");
    }
    let get = |uri: &str, method: &str| {
        http::Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap()
    };
    let resp = router()
        .oneshot(get("/api/bookmark/v1/nope", "GET"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    assert_eq!(&body[..], b"{\"reason\":\"not_found\"}\n");
    let resp = router()
        .oneshot(get("/api/bookmark/v1/info", "DELETE"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 405);
}
