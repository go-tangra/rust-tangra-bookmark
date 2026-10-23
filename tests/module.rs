//! The bookmark module end to end, in process: the fake platform
//! (`tangra_sdk::testutil::platform`: `gateway` and `auth` over mTLS), a
//! throwaway PostgreSQL (`tangra_sdk::testutil::pg`; the application role is
//! `NOBYPASSRLS`), the module built by `module::build` with an SVID from the
//! platform's CA, and requests made over mTLS as the gateway makes them.
//!
//! Without PostgreSQL binaries (`PG_DIR`, the cache, a download, or
//! `TANGRA_TEST_PG_DSN`) the test is skipped with a notice. The event check
//! needs a Valkey (`TANGRA_TEST_VALKEY_ADDR`, or a `valkey-server` /
//! `redis-server` binary via `VALKEY_SERVER` or `PATH`), else it is skipped.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};
use sqlx::{Connection, PgConnection, Row};
use tangra_bookmark::config::BookmarkFile;
use tangra_bookmark::module::{self, Options};
use tangra_bookmark::{VERSION, docs, manifest};
use tangra_sdk::events::{self, ValkeyConfig};
use tangra_sdk::log::{Capture, Level, Logger};
use tangra_sdk::testutil::MemoryProvider;
use tangra_sdk::testutil::pg::Postgres;
use tangra_sdk::testutil::platform::{Platform, TestResponse};

const TD: &str = "example.org";
const ISSUER: &str = "https://portal.example.org:8443";
const T1: &str = "0192f6b0-0000-7000-8000-0000000000f1";
const T2: &str = "0192f6b0-0000-7000-8000-0000000000f2";
const ANA: &str = "0192f6b0-0000-7000-8000-00000000a0a0";
const BORIS: &str = "0192f6b0-0000-7000-8000-00000000b0b0";
const DAVE: &str = "0192f6b0-0000-7000-8000-00000000d0d0";
const ERIN: &str = "0192f6b0-0000-7000-8000-00000000e0e0";
const FRANK: &str = "0192f6b0-0000-7000-8000-00000000f0f0";
const GINA: &str = "0192f6b0-0000-7000-8000-000000009090";
const CAROL: &str = "0192f6b0-0000-7000-8000-00000000c0c0";
const HELEN: &str = "0192f6b0-0000-7000-8000-000000001010";
const API: &str = "/api/bookmark/v1";

const POLICY: &str = r#"version: bookmark-test-1
rules:
  - {id: gateway-forwards, from: ["spiffe://example.org/svc/gateway"], to: ["bookmark"], operations: ["GET *", "HEAD *", "POST *", "PUT *"], effect: allow}
"#;

struct Valkey {
    addr: String,
    _child: Option<ChildGuard>,
    _dir: Option<tempfile::TempDir>,
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn valkey() -> Option<Valkey> {
    if let Ok(addr) = std::env::var("TANGRA_TEST_VALKEY_ADDR") {
        return Some(Valkey {
            addr,
            _child: None,
            _dir: None,
        });
    }
    let bin = std::env::var("VALKEY_SERVER")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            let path = std::env::var_os("PATH")?;
            std::env::split_paths(&path)
                .flat_map(|d| [d.join("valkey-server"), d.join("redis-server")])
                .find(|p| p.is_file())
        })?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .ok()?
        .local_addr()
        .ok()?
        .port();
    let dir = tempfile::tempdir().ok()?;
    let child = Command::new(bin)
        .args([
            "--bind",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--save",
            "",
            "--appendonly",
            "no",
        ])
        .arg("--dir")
        .arg(dir.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let guard = ChildGuard(child);
    for _ in 0..100 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Some(Valkey {
                addr: format!("127.0.0.1:{port}"),
                _child: Some(guard),
                _dir: Some(dir),
            });
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

fn config(p: &Platform, dir: &std::path::Path, owner_dsn: &str, valkey: Option<&Valkey>) -> BookmarkFile {
    let policy = dir.join("policy.yaml");
    std::fs::write(&policy, POLICY).unwrap();
    let app_dsn = Postgres::dsn_as(owner_dsn, "bookmark_app", "bookmark-test");
    let mut yaml = format!(
        "service_name: bookmark\ntrust_domain: {TD}\nidentity: {{provider: provided}}\n\
authz: {{source: file, path: {}}}\n\
server: {{grpc_addr: \"127.0.0.1:0\", http_addr: \"127.0.0.1:0\"}}\nadmin: {{addr: \"127.0.0.1:0\"}}\n\
gateway: {{service: gateway, issuer: \"{ISSUER}\"}}\n\
db: {{dsn: \"{app_dsn}\", migrate_dsn: \"{owner_dsn}\", max_conns: 4}}\n",
        policy.display()
    );
    if let Some(v) = valkey {
        yaml.push_str(&format!(
            "events: {{enabled: true}}\nvalkey: {{addresses: [\"{}\"], allow_plaintext: true}}\n",
            v.addr
        ));
    }
    let mut cfg = BookmarkFile::from_yaml(yaml.as_bytes()).unwrap();
    cfg.framework.discovery.r#static = p.discovery();
    tangra_bookmark::config::validate(&cfg).unwrap();
    cfg
}

async fn eventually(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..400 {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

struct Client<'a> {
    p: &'a Platform,
    addr: SocketAddr,
}

impl Client<'_> {
    async fn call(&self, method: &str, path: &str, token: Option<&str>, body: &str) -> TestResponse {
        let auth = token.map(|t| format!("Bearer {t}"));
        let mut h: Vec<(&str, &str)> = vec![("x-csrf-token", "csrf-1"), ("x-request-id", "req-42")];
        if let Some(a) = &auth {
            h.push(("authorization", a.as_str()));
        }
        if !body.is_empty() {
            h.push(("content-type", "application/json"));
        }
        self.p
            .call(self.addr, "bookmark", method, path, &h, body)
            .await
            .unwrap()
    }

    async fn get(&self, path: &str, token: &str) -> TestResponse {
        self.call("GET", &format!("{API}{path}"), Some(token), "").await
    }

    async fn post(&self, path: &str, token: &str, body: Value) -> TestResponse {
        self.call("POST", &format!("{API}{path}"), Some(token), &body.to_string())
            .await
    }

    async fn put(&self, path: &str, token: &str, body: Value) -> TestResponse {
        self.call("PUT", &format!("{API}{path}"), Some(token), &body.to_string())
            .await
    }

    async fn create(&self, token: &str, url: &str, title: &str, tags: &[&str]) -> String {
        let r = self
            .post(
                "/bookmarks",
                token,
                json!({"url": url, "title": title, "tags": tags}),
            )
            .await;
        assert_eq!(r.status, 201, "{}", r.body);
        r.json()["id"].as_str().unwrap().to_owned()
    }

    async fn grant(&self, token: &str, body: Value) -> TestResponse {
        self.post("/permissions/grant", token, body).await
    }
}

fn titles(r: &TestResponse) -> Vec<String> {
    r.json()["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["title"].as_str().unwrap().to_owned())
        .collect()
}

async fn sql_count(dsn: &str, sql: &str) -> i64 {
    let mut c = PgConnection::connect(dsn).await.unwrap();
    let n: i64 = sqlx::query(sql).fetch_one(&mut c).await.unwrap().get(0);
    let _ = c.close().await;
    n
}

#[tokio::test]
async fn bookmark_module_end_to_end() {
    let Some(pg) = Postgres::start().await else {
        return;
    };
    let owner = pg.database("bookmark").await.unwrap();
    pg.exec(
        "DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'bookmark_app') THEN \
         CREATE ROLE bookmark_app LOGIN NOBYPASSRLS PASSWORD 'bookmark-test'; END IF; END $$",
    )
    .await
    .unwrap();
    let mut p = Platform::start(TD, ISSUER).await.unwrap();
    let vk = valkey();
    if vk.is_none() {
        eprintln!("no Valkey (set TANGRA_TEST_VALKEY_ADDR or VALKEY_SERVER): event checks skipped");
    }
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(&p, dir.path(), &owner, vk.as_ref());

    // bootstrap: the migrations with the owner role, idempotent.
    module::migrate(&cfg).await.unwrap();
    module::migrate(&cfg).await.unwrap();

    let log = Capture::new();
    tangra_sdk::gateway::set_build_version(VERSION);
    let b = module::build(
        cfg,
        Options {
            logger: Some(Logger::new(log.clone(), Level::Debug)),
            identity: Some(MemoryProvider::new(&p.ca, &p.ca.issue("bookmark").unwrap()).unwrap()),
            registration_every: Some((Duration::from_millis(200), Duration::from_secs(300))),
        },
    )
    .await
    .unwrap();
    let app = b.app.clone();
    let verifier = std::sync::Arc::clone(&b.verifier);
    let digest = b.docs_digest.clone();
    let serves_ui = b.serves_ui;
    let addr = app.http_local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let done = tokio::spawn(b.run_until(async {
        let _ = rx.await;
    }));

    // Platform registration: gateway (manifest, docs digest, release) and
    // auth (permissions, roles, grants).
    assert!(
        eventually(|| p.gateway.counts().0 >= 1).await,
        "registered with the gateway"
    );
    let reg = p.gateway.last().unwrap();
    assert_eq!(
        reg.manifest.clone().unwrap(),
        manifest::manifest().unwrap().to_proto().unwrap()
    );
    assert_eq!(reg.docs_digest, digest);
    assert_eq!(reg.docs_digest, docs::bundle().unwrap().digest());
    assert_eq!(reg.build_version, VERSION);
    assert!(
        eventually(|| !p.auth.registrations().is_empty()).await,
        "registered with auth"
    );
    let (peer, req) = p.auth.registrations().remove(0);
    assert_eq!(peer, "spiffe://example.org/svc/bookmark");
    assert_eq!(req, manifest::registration().request());
    let admin = app.admin_url().trim_start_matches("http://").to_owned();
    tangra_bookmark::healthcheck::probe(&admin).await.unwrap();
    assert!(
        eventually(|| verifier.keys_fetched_at().is_some()).await,
        "keys fetched"
    );

    let c = Client { p: &p, addr };
    let ana = p.token(ANA, T1, &["member"]);
    let boris = p.token(BORIS, T1, &["member"]);
    let dave = p.token(DAVE, T1, &["member", "dev"]);
    let erin = p.token(ERIN, T1, &["member"]);
    let frank = p.token(FRANK, T1, &["admin"]);
    let gina = p.token(GINA, T1, &["member"]);
    let carol = p.token(CAROL, T2, &["member"]);
    let helen = p.token(HELEN, T2, &["owner"]);

    // ---- public route, 401 ----
    let r = c.call("GET", &format!("{API}/info"), None, "").await;
    assert_eq!(r.json(), json!({"module": "bookmark", "version": VERSION}));
    assert_eq!(r.headers["x-request-id"], "req-42");
    let r = c.call("GET", &format!("{API}/bookmarks"), None, "").await;
    assert_eq!((r.status, r.json()), (401, json!({"reason": "unauthenticated"})));
    assert_eq!(r.headers["www-authenticate"], "Bearer realm=\"freya\"");

    // ---- US1: create and validate ----
    let r = c.call("POST", &format!("{API}/bookmarks"), Some(&ana), "{").await;
    assert_eq!((r.status, r.json()), (400, json!({"reason": "malformed_body"})));
    let r = c
        .post(
            "/bookmarks",
            &ana,
            json!({"url": "javascript:alert(1)", "colour": 1}),
        )
        .await;
    assert_eq!(r.status, 422);
    assert_eq!(
        r.json()["detail"]["fields"],
        json!({"url": "scheme", "colour": "unknown"})
    );
    let r = p
        .call(
            addr,
            "bookmark",
            "POST",
            &format!("{API}/bookmarks"),
            &[
                ("authorization", &format!("Bearer {ana}")),
                ("content-type", "application/json"),
            ],
            r#"{"url":"https://go.dev"}"#,
        )
        .await
        .unwrap();
    assert_eq!(r.status, 422, "missing X-CSRF-Token");
    let r = c
        .post(
            "/bookmarks",
            &ana,
            json!({"url": " https://go.dev/doc ", "title": "Go docs", "description": "the\nmanual", "tags": ["go", " docs", "go"]}),
        )
        .await;
    assert_eq!(r.status, 201, "{}", r.body);
    assert_eq!(r.headers["cache-control"], "no-store");
    let go = r.json();
    let go_id = go["id"].as_str().unwrap().to_owned();
    assert_eq!(go["url"], "https://go.dev/doc");
    assert_eq!(go["tags"], json!(["go", "docs"]));
    assert_eq!(go["created_by"], ANA);
    assert_eq!(
        go["permissions"],
        json!({"read": true, "write": true, "delete": true, "share": true})
    );
    let rust_id = c
        .create(
            &ana,
            "https://doc.rust-lang.org/book/",
            "Rust book",
            &["rust", "docs"],
        )
        .await;
    let news_id = c
        .create(&ana, "https://news.example.org/a_b%c", "News", &[])
        .await;

    // ---- US1: list, sort, page, filters ----
    let r = c.get("/bookmarks", &ana).await;
    assert_eq!(r.status, 200);
    assert_eq!(titles(&r), ["News", "Rust book", "Go docs"], "newest first");
    assert_eq!(
        (
            r.json()["total"].clone(),
            r.json()["sort"].clone(),
            r.json()["order"].clone()
        ),
        (json!(3), json!("created_at"), json!("desc"))
    );
    let r = c.get("/bookmarks?sort=title&order=asc", &ana).await;
    assert_eq!(titles(&r), ["Go docs", "News", "Rust book"]);
    let r = c.get("/bookmarks?sort=title&page_size=2&page=2", &ana).await;
    assert_eq!(
        (titles(&r), r.json()["page"].clone()),
        (vec!["Rust book".to_owned()], json!(2))
    );
    let r = c.get("/bookmarks?sort=title&page_size=2&page=9", &ana).await;
    assert_eq!(r.json()["page"], 2, "a page beyond the end answers the last page");
    let r = c.get("/bookmarks?tag=docs&sort=title", &ana).await;
    assert_eq!(titles(&r), ["Go docs", "Rust book"]);
    let r = c.get("/bookmarks?tag=docs&tag=go", &ana).await;
    assert_eq!(titles(&r), ["Go docs"]);
    let r = c.get("/bookmarks?q=RUST", &ana).await;
    assert_eq!(titles(&r), ["Rust book"]);
    let r = c.get("/bookmarks?q=a_b%25", &ana).await;
    assert_eq!(titles(&r), ["News"], "LIKE metacharacters are literal");
    let r = c.get("/bookmarks?q=a%25b", &ana).await;
    assert_eq!(r.json()["total"], 0);
    let many = (0..11).map(|i| format!("tag=t{i}")).collect::<Vec<_>>().join("&");
    for (q, param) in [
        ("sort=created_by".to_owned(), "sort"),
        ("page_size=500".to_owned(), "page_size"),
        ("cursor=x".to_owned(), "cursor"),
        (many, "tag"),
        (format!("q={}", "x".repeat(201)), "q"),
    ] {
        let r = c.get(&format!("/bookmarks?{q}"), &ana).await;
        assert_eq!(
            r.json(),
            json!({"reason": "validation_failed", "detail": {"param": param}}),
            "{q}"
        );
    }

    // ---- isolation and masking ----
    let not_found = json!({"reason": "not_found"});
    assert_eq!(
        c.get("/bookmarks", &boris).await.json()["total"],
        0,
        "private until shared"
    );
    let hidden = c.get(&format!("/bookmarks/{go_id}"), &boris).await;
    let absent = c
        .get("/bookmarks/0192f6b0-0000-7000-8000-000000000000", &boris)
        .await;
    assert_eq!(
        (hidden.status, hidden.body.clone()),
        (absent.status, absent.body.clone())
    );
    assert_eq!(hidden.json(), not_found);
    assert_eq!(c.get("/bookmarks/not-a-uuid", &ana).await.status, 404);
    assert_eq!(
        c.get("/bookmarks", &carol).await.json()["total"],
        0,
        "another tenant"
    );
    assert_eq!(
        c.get(&format!("/bookmarks/{go_id}"), &carol).await.json(),
        not_found
    );
    assert_eq!(
        c.get(&format!("/bookmarks/{go_id}"), &helen).await.status,
        404,
        "admins stay in their tenant"
    );
    let r = c
        .put(&format!("/bookmarks/{go_id}"), &carol, json!({"title": "x"}))
        .await;
    assert_eq!(r.status, 404);

    // ---- US1: update and remove ----
    let r = c
        .put(&format!("/bookmarks/{rust_id}"), &ana, json!({"tags": ["rust"]}))
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        (r.json()["tags"].clone(), r.json()["title"].clone()),
        (json!(["rust"]), json!("Rust book"))
    );
    assert_eq!(
        c.put(&format!("/bookmarks/{rust_id}"), &ana, json!({}))
            .await
            .status,
        422
    );

    // ---- US2: sharing ----
    let r = c
        .grant(
            &ana,
            json!({"resource_id": go_id, "subject_type": "user", "subject_id": BORIS, "relation": "viewer"}),
        )
        .await;
    assert_eq!(r.status, 201, "{}", r.body);
    let boris_viewer = r.json();
    assert_eq!(
        (
            boris_viewer["granted_by"].clone(),
            boris_viewer["relation"].clone()
        ),
        (json!(ANA), json!("viewer"))
    );
    let r = c
        .grant(
            &ana,
            json!({"resource_id": go_id, "subject_type": "user", "subject_id": BORIS, "relation": "viewer"}),
        )
        .await;
    assert_eq!((r.status, r.json()), (409, json!({"reason": "conflict"})));
    let r = c.get("/bookmarks", &boris).await;
    assert_eq!(titles(&r), ["Go docs"]);
    assert_eq!(
        r.json()["items"][0]["permissions"],
        json!({"read": true, "write": false, "delete": false, "share": false})
    );
    let r = c
        .put(&format!("/bookmarks/{go_id}"), &boris, json!({"title": "mine"}))
        .await;
    assert_eq!((r.status, r.json()), (403, json!({"reason": "forbidden"})));
    assert_eq!(
        c.post(&format!("/bookmarks/{go_id}/remove"), &boris, json!({}))
            .await
            .status,
        403
    );
    let refused = sql_count(
        &owner,
        &format!("SELECT count(*) FROM bookmark_audit_events WHERE event_type = 'access_refused' AND actor_id = '{BORIS}'"),
    )
    .await;
    assert_eq!(refused, 2, "403s are audited");

    // A role grant: dave holds `dev`.
    let r = c
        .grant(&ana, json!({"resource_id": rust_id, "subject_type": "role", "subject_id": "dev", "relation": "editor"}))
        .await;
    assert_eq!(r.status, 201);
    let r = c
        .put(
            &format!("/bookmarks/{rust_id}"),
            &dave,
            json!({"description": "edited"}),
        )
        .await;
    assert_eq!(
        (r.status, r.json()["description"].clone()),
        (200, json!("edited"))
    );
    assert_eq!(
        c.post(&format!("/bookmarks/{rust_id}/remove"), &dave, json!({}))
            .await
            .status,
        403
    );
    let r = c
        .grant(
            &dave,
            json!({"resource_id": rust_id, "subject_type": "user", "subject_id": ERIN, "relation": "viewer"}),
        )
        .await;
    assert_eq!(r.status, 403, "editors cannot share");
    let r = c.get(&format!("/permissions?resource_id={rust_id}"), &dave).await;
    assert_eq!(r.status, 403, "listing a bookmark's grants needs share");

    // Sharers pass on viewer/sharer only.
    let r = c
        .grant(
            &ana,
            json!({"resource_id": go_id, "subject_type": "user", "subject_id": BORIS, "relation": "sharer"}),
        )
        .await;
    assert_eq!(r.status, 201);
    let r = c
        .grant(
            &boris,
            json!({"resource_id": go_id, "subject_type": "user", "subject_id": ERIN, "relation": "viewer"}),
        )
        .await;
    assert_eq!(r.status, 201, "{}", r.body);
    let erin_viewer_id = r.json()["id"].as_str().unwrap().to_owned();
    for rel in ["editor", "owner"] {
        let r = c
            .grant(
                &boris,
                json!({"resource_id": go_id, "subject_type": "user", "subject_id": ERIN, "relation": rel}),
            )
            .await;
        assert_eq!(r.status, 403, "a sharer cannot grant {rel}");
    }
    let r = c
        .get(
            &format!("/permissions?resource_id={go_id}&sort=granted_at&order=asc"),
            &boris,
        )
        .await;
    assert_eq!(r.status, 200);
    let grants = r.json()["items"].as_array().unwrap().clone();
    let rels: Vec<&str> = grants.iter().map(|g| g["relation"].as_str().unwrap()).collect();
    assert_eq!(rels, ["owner", "viewer", "sharer", "viewer"]);
    let ana_owner_id = grants[0]["id"].as_str().unwrap().to_owned();
    let r = c
        .post(
            "/permissions/revoke",
            &boris,
            json!({"resource_id": go_id, "id": ana_owner_id}),
        )
        .await;
    assert_eq!(r.status, 403, "a sharer cannot remove an owner");
    // A grant ID of another bookmark is not found through this one.
    let rust_grants = c
        .get(&format!("/permissions?resource_id={rust_id}"), &ana)
        .await
        .json();
    let rust_grant = rust_grants["items"][0]["id"].as_str().unwrap().to_owned();
    let r = c
        .post(
            "/permissions/revoke",
            &ana,
            json!({"resource_id": go_id, "id": rust_grant}),
        )
        .await;
    assert_eq!((r.status, r.json()), (404, not_found.clone()));
    let r = c
        .post(
            "/permissions/revoke",
            &ana,
            json!({"resource_id": go_id, "id": "nope"}),
        )
        .await;
    assert_eq!(r.status, 404);
    // Grants boris manages (no resource): only the Go docs grants.
    let r = c.get("/permissions", &boris).await;
    assert_eq!(r.json()["total"], 4);
    let r = c
        .get(
            &format!("/permissions?subject_type=user&subject_id={ERIN}"),
            &boris,
        )
        .await;
    assert_eq!(r.json()["total"], 1);
    assert_eq!(c.get("/permissions?subject_type=group", &boris).await.status, 422);
    assert_eq!(
        c.get("/permissions", &ana).await.json()["total"],
        7,
        "ana can share every grant's bookmark"
    );

    // check and effective.
    let check = |token: String, rid: String, perm: &'static str| {
        let c = &c;
        async move {
            c.post(
                "/permissions/check",
                &token,
                json!({"resource_id": rid, "permission": perm}),
            )
            .await
            .json()["allowed"]
                .clone()
        }
    };
    assert_eq!(check(boris.clone(), go_id.clone(), "share").await, json!(true));
    assert_eq!(check(boris.clone(), go_id.clone(), "write").await, json!(false));
    assert_eq!(
        check(boris.clone(), rust_id.clone(), "read").await,
        json!(false),
        "unreadable: false"
    );
    assert_eq!(check(boris.clone(), "x".into(), "read").await, json!(false));
    let r = c
        .get(&format!("/permissions/effective?resource_id={go_id}"), &boris)
        .await;
    let e = r.json();
    assert_eq!(
        e["permissions"],
        json!({"read": true, "write": false, "delete": false, "share": true})
    );
    assert_eq!(
        (e["relation"].clone(), e["admin"].clone()),
        (json!("sharer"), json!(false))
    );
    assert_eq!(
        e["grants"].as_array().unwrap().len(),
        2,
        "only the grants that apply to boris"
    );
    assert_eq!(
        c.get(&format!("/permissions/effective?resource_id={rust_id}"), &boris)
            .await
            .status,
        404
    );
    let r = c.get("/permissions/effective", &boris).await;
    assert_eq!(r.json()["permissions"]["read"], false, "no collection access");

    // Revoke erin's viewer (boris may: viewer).
    let r = c
        .post(
            "/permissions/revoke",
            &boris,
            json!({"resource_id": go_id, "id": erin_viewer_id}),
        )
        .await;
    assert_eq!((r.status, r.json()), (200, json!({"revoked": true})));
    assert_eq!(c.get(&format!("/bookmarks/{go_id}"), &erin).await.status, 404);

    // A tenant grant on one bookmark stays on that bookmark.
    let r = c
        .grant(
            &ana,
            json!({"resource_id": news_id, "subject_type": "tenant", "relation": "viewer"}),
        )
        .await;
    assert_eq!(r.status, 201, "{}", r.body);
    assert_eq!(titles(&c.get("/bookmarks", &erin).await), ["News"]);
    assert_eq!(c.get(&format!("/bookmarks/{go_id}"), &erin).await.status, 404);

    // Expiry: past expiries are refused; an expired grant confers nothing.
    let r = c
        .grant(
            &ana,
            json!({"resource_id": go_id, "subject_type": "user", "subject_id": GINA, "relation": "viewer", "expires_at": "2020-01-01T00:00:00Z"}),
        )
        .await;
    assert_eq!(r.json()["detail"]["fields"], json!({"expires_at": "past"}));
    let r = c
        .grant(
            &ana,
            json!({"resource_id": rust_id, "subject_type": "user", "subject_id": GINA, "relation": "viewer", "expires_at": "2999-01-01T00:00:00Z"}),
        )
        .await;
    assert_eq!(r.status, 201);
    assert_eq!(c.get(&format!("/bookmarks/{rust_id}"), &gina).await.status, 200);
    let mut sql = PgConnection::connect(&owner).await.unwrap();
    sqlx::query(&format!(
        "INSERT INTO bookmark_permissions (tenant_id, id, resource_id, relation, subject_type, subject_id, granted_by, expires_at)
         VALUES ('{T1}', gen_random_uuid(), '{go_id}', 'viewer', 'user', '{GINA}', '{ANA}', now() - interval '1 minute')"
    ))
    .execute(&mut sql)
    .await
    .unwrap();
    sqlx::query(&format!(
        "UPDATE bookmark_permissions SET expires_at = now() - interval '1 second' WHERE tenant_id = '{T1}' AND resource_id = '{rust_id}' AND subject_id = '{GINA}'"
    ))
    .execute(&mut sql)
    .await
    .unwrap();
    let _ = sql.close().await;
    assert_eq!(
        c.get(&format!("/bookmarks/{go_id}"), &gina).await.status,
        404,
        "expired grant"
    );
    assert_eq!(
        c.get(&format!("/bookmarks/{rust_id}"), &gina).await.status,
        404,
        "expired grant"
    );
    assert_eq!(titles(&c.get("/bookmarks", &gina).await), ["News"]);

    // ---- US3: administrators and collection grants ----
    let r = c
        .grant(&boris, json!({"subject_type": "tenant", "relation": "viewer"}))
        .await;
    assert_eq!(r.status, 403, "only collection sharers grant on every bookmark");
    let r = c
        .grant(
            &frank,
            json!({"subject_type": "role", "subject_id": "dev", "relation": "viewer"}),
        )
        .await;
    assert_eq!(r.json()["detail"]["fields"], json!({"resource_id": "required"}));
    let r = c
        .grant(&frank, json!({"subject_type": "tenant", "relation": "viewer"}))
        .await;
    assert_eq!(r.status, 201, "{}", r.body);
    let everyone_id = r.json()["id"].as_str().unwrap().to_owned();
    assert_eq!(r.json()["resource_id"], "");
    assert_eq!(
        c.get("/bookmarks", &gina).await.json()["total"],
        3,
        "every bookmark of the tenant"
    );
    assert_eq!(
        c.get("/bookmarks", &carol).await.json()["total"],
        0,
        "not another tenant"
    );
    let r = c.get("/permissions/effective", &gina).await;
    assert_eq!(r.json()["permissions"]["read"], true);
    let r = c
        .post("/permissions/revoke", &frank, json!({"id": everyone_id}))
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(titles(&c.get("/bookmarks", &gina).await), ["News"]);
    let r = c.get("/bookmarks", &frank).await;
    assert_eq!(r.json()["total"], 3, "admins see every bookmark of their tenant");
    assert_eq!(r.json()["items"][0]["permissions"]["delete"], true);
    let r = c
        .get(&format!("/permissions/effective?resource_id={go_id}"), &frank)
        .await;
    assert_eq!(
        (r.json()["admin"].clone(), r.json()["relation"].clone()),
        (json!(true), Value::Null)
    );
    assert_eq!(c.get("/permissions", &frank).await.json()["total"], 9);
    let r = c
        .post(&format!("/bookmarks/{news_id}/remove"), &frank, json!({}))
        .await;
    assert_eq!(r.status, 204);
    assert_eq!(c.get(&format!("/bookmarks/{news_id}"), &ana).await.status, 404);
    let left = sql_count(
        &owner,
        &format!("SELECT count(*) FROM bookmark_permissions WHERE resource_id = '{news_id}'"),
    )
    .await;
    assert_eq!(left, 0, "grants removed with the bookmark");

    // ---- events ----
    if let Some(v) = &vk {
        let ec = events::Client::connect(&ValkeyConfig {
            addresses: vec![v.addr.clone()],
            allow_plaintext: true,
            ..ValkeyConfig::default()
        })
        .await
        .expect("valkey reachable");
        let es = ec
            .xread(&events::key(T1), "0", Duration::from_secs(2), 100)
            .await
            .unwrap();
        let shared: Vec<&events::Entry> = es
            .iter()
            .filter(|e| e.fields["type"] == "bookmark.shared")
            .collect();
        assert!(!shared.is_empty(), "bookmark.shared published");
        let to_boris = shared
            .iter()
            .find(|e| e.fields["to"] == BORIS)
            .expect("to the grantee");
        let data: Value = serde_json::from_str(&to_boris.fields["data"]).unwrap();
        assert_eq!(data, json!({"bookmark_id": go_id, "relation": "viewer"}));
        assert!(
            shared.iter().all(|e| !e.fields["data"].contains("go.dev")),
            "no content"
        );
    }

    // ---- US4: backup ----
    let r = c.post("/backup/export", &frank, json!({})).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let mut backup = r.json();
    assert_eq!(
        (backup["schema_version"].clone(), backup["module"].clone()),
        (json!(1), json!("bookmark"))
    );
    assert_eq!(backup["bookmarks"].as_array().unwrap().len(), 2);
    let n_grants = backup["permissions"].as_array().unwrap().len();
    assert!(n_grants >= 5, "{n_grants}");
    assert_eq!(
        c.post("/backup/export", &helen, json!({})).await.json()["bookmarks"],
        json!([]),
        "per tenant"
    );
    // An orphan grant (its bookmark is in neither the file nor the tenant).
    backup["permissions"].as_array_mut().unwrap().push(json!({
        "id": "0192f6b0-0000-7000-8000-0000000000aa", "resource_type": "bookmark",
        "resource_id": "0192f6b0-0000-7000-8000-0000000000bb", "subject_type": "tenant", "relation": "viewer"
    }));
    let r = c.post("/backup/import", &helen, json!({"backup": backup})).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        r.json(),
        json!({"bookmarks_imported": 2, "bookmarks_skipped": 0, "permissions_imported": n_grants, "permissions_skipped": 1})
    );
    let r = c.get(&format!("/bookmarks/{go_id}"), &helen).await;
    assert_eq!(
        (r.status, r.json()["title"].clone()),
        (200, json!("Go docs")),
        "IDs kept in the other tenant"
    );
    let r = c
        .post(
            "/backup/import",
            &helen,
            json!({"mode": "skip", "backup": backup}),
        )
        .await;
    assert_eq!(
        r.json(),
        json!({"bookmarks_imported": 0, "bookmarks_skipped": 2, "permissions_imported": 0, "permissions_skipped": n_grants + 1})
    );
    backup["bookmarks"][0]["title"] = json!("Changed in the file");
    let changed_id = backup["bookmarks"][0]["id"].as_str().unwrap().to_owned();
    let r = c
        .post(
            "/backup/import",
            &helen,
            json!({"mode": "overwrite", "backup": backup}),
        )
        .await;
    assert_eq!(r.json()["bookmarks_imported"], 2);
    assert_eq!(r.json()["permissions_imported"], n_grants);
    assert_eq!(
        c.get(&format!("/bookmarks/{changed_id}"), &helen).await.json()["title"],
        "Changed in the file"
    );
    assert_ne!(
        c.get(&format!("/bookmarks/{changed_id}"), &ana).await.json()["title"],
        "Changed in the file",
        "the source tenant is untouched"
    );
    let t2_rows = sql_count(
        &owner,
        &format!("SELECT count(*) FROM bookmark_permissions WHERE tenant_id = '{T2}'"),
    )
    .await;
    assert_eq!(t2_rows as usize, n_grants, "overwrite replaced, not duplicated");
    // Refused imports write nothing.
    let mut bad = backup.clone();
    bad["schema_version"] = json!(2);
    let r = c.post("/backup/import", &carol, json!({"backup": bad})).await;
    assert_eq!(r.json()["detail"]["fields"], json!({"schema_version": "invalid"}));
    let mut bad = backup.clone();
    bad["bookmarks"][1]["url"] = json!("data:text/html,x");
    let r = c.post("/backup/import", &carol, json!({"backup": bad})).await;
    assert_eq!(
        r.json()["detail"]["fields"],
        json!({"bookmarks[1].url": "scheme"})
    );
    let r = c
        .post(
            "/backup/import",
            &carol,
            json!({"mode": "merge", "backup": backup}),
        )
        .await;
    assert_eq!(r.json()["detail"]["fields"], json!({"mode": "invalid"}));
    assert_eq!(c.post("/backup/import", &carol, json!({})).await.status, 422);
    let t2_bookmarks = sql_count(
        &owner,
        &format!("SELECT count(*) FROM bookmark_bookmarks WHERE tenant_id = '{T2}'"),
    )
    .await;
    assert_eq!(t2_bookmarks, 2);
    let audits = sql_count(&owner, "SELECT count(*) FROM bookmark_audit_events WHERE event_type IN ('backup_exported', 'backup_imported')").await;
    assert_eq!(audits, 5);
    let leaked = sql_count(&owner, "SELECT count(*) FROM bookmark_audit_events WHERE details::text LIKE '%go.dev%' OR details::text LIKE '%Go docs%'").await;
    assert_eq!(leaked, 0, "audit details carry no content");

    // ---- unknown paths and methods, help bundle ----
    let r = c.get("/unknown", &ana).await;
    assert_eq!((r.status, r.body.as_str()), (404, "{\"reason\":\"not_found\"}\n"));
    let r = c.call("PUT", &format!("{API}/info"), None, "").await;
    assert_eq!(
        (r.status, r.json()),
        (405, json!({"reason": "method_not_allowed"}))
    );
    let r = p
        .call(addr, "bookmark", "GET", "/docs/v1/bundle", &[], "")
        .await
        .unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.headers["etag"], format!("\"{digest}\"").as_str());
    let r = p
        .call_as("lcm", addr, "bookmark", "GET", "/docs/v1/bundle", &[], "")
        .await
        .unwrap();
    assert_eq!((r.status, r.body.as_str()), (403, "{\"reason\":\"denied\"}\n"));

    // The UI remote, when this build embeds one (feature `ui` and a built
    // ui/dist): the module-federation manifest of the remote `bookmark` with
    // the manifest's exposes, and its entry.
    let r = p
        .call(addr, "bookmark", "GET", "/ui/mf-manifest.json", &[], "")
        .await
        .unwrap();
    if serves_ui {
        assert_eq!(r.status, 200, "{}", r.body);
        assert_eq!(r.headers["cache-control"], "no-cache");
        let m = r.json();
        assert!(m["name"] == "bookmark" || m["id"] == "bookmark", "{}", r.body);
        assert_eq!(m["metaData"]["publicPath"], "/m/bookmark/", "{}", r.body);
        let mut got: Vec<&str> = m["exposes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["path"].as_str().unwrap())
            .collect();
        let mut want = manifest::EXPOSES.to_vec();
        got.sort_unstable();
        want.sort_unstable();
        assert_eq!(got, want, "the remote exposes the manifest's exposes");
        let r = p
            .call(addr, "bookmark", "GET", "/ui/remoteEntry.js", &[], "")
            .await
            .unwrap();
        assert_eq!(r.status, 200);
    } else {
        assert_eq!(r.status, 404);
        eprintln!("this build serves no UI remote (feature `ui` and a built ui/dist): UI check skipped");
    }

    // Nothing internal leaked into the log.
    assert!(!log.contents().contains("storage failure"), "{}", log.contents());

    // Shutdown withdraws the lease.
    let _ = tx.send(());
    tokio::time::timeout(Duration::from_secs(20), done)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(p.gateway.counts().2, 1, "deregistered on shutdown");
    tangra_sdk::gateway::set_build_version("");
    p.stop().await;
}
