//! `bookmark-migrate` against a throwaway PostgreSQL
//! (`tangra_sdk::testutil::pg`): a v3 database built from the v3 migrations
//! (`testdata/v3/`) and seeded in the shape of production (4 bookmarks, 7
//! grants with user, role and tenant subjects, an expired grant and an
//! orphan), imported into a v4 database (`migrations/`, row-level security
//! forced) as the NOBYPASSRLS application role (library) and as the owner
//! (the binary, `db.migrate_dsn`).
//!
//! Skipped with a notice without PostgreSQL binaries (see tests/module.rs).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;
use sqlx::{Connection, PgConnection, Row};
use tangra_bookmark::migrate3::{self, Mapping, Mode, Options, PLATFORM_TENANT, Status};
use tangra_bookmark::store;
use tangra_sdk::db::{self, DbConfig};
use tangra_sdk::testutil::pg::Postgres;

const ANA: &str = "0192f6b0-0000-7000-8000-00000000a0a0";
const BORIS: &str = "0192f6b0-0000-7000-8000-00000000b0b0";
const CAROL: &str = "0192f6b0-0000-7000-8000-00000000c0c0";
const B1: &str = "0190a000-0000-7000-8000-000000000001";
const B2: &str = "0190a000-0000-7000-8000-000000000002";
const B3: &str = "0190a000-0000-7000-8000-000000000003";

const DIR: &str = env!("CARGO_MANIFEST_DIR");

async fn exec(dsn: &str, sql: &str) {
    let mut c = PgConnection::connect(dsn).await.unwrap();
    sqlx::raw_sql(sql).execute(&mut c).await.unwrap();
    let _ = c.close().await;
}

fn testdata(name: &str) -> String {
    std::fs::read_to_string(format!("{DIR}/testdata/v3/{name}")).unwrap()
}

fn mapping() -> Mapping {
    Mapping {
        tenants: BTreeMap::from([(0, PLATFORM_TENANT.to_owned())]),
        users_by_email: HashMap::from([
            ("ana@example.org".to_owned(), ANA.to_owned()),
            ("boris@example.org".to_owned(), BORIS.to_owned()),
        ]),
        users_by_v3_id: HashMap::from([(3, CAROL.to_owned())]),
        roles: BTreeMap::from([("platform:admin".to_owned(), "admin".to_owned())]),
        platform_actor: String::new(),
    }
}

const EXCLUDE: Options = Options {
    exclude_orphans: true,
    exclude_invalid: false,
};

async fn run(src: &str, dst: &sqlx::PgPool, mode: Mode, o: Options) -> migrate3::Report {
    let mut c = migrate3::connect_source(src).await.unwrap();
    let r = migrate3::import(&mut c, dst, &mapping(), mode, o).await.unwrap();
    let _ = c.close().await;
    r
}

#[tokio::test]
async fn imports_the_v3_tables() {
    let Some(pg) = Postgres::start().await else {
        return;
    };
    let v3 = pg.database("gwa").await.unwrap();
    exec(&v3, &testdata("001_create_bookmarks.sql")).await;
    exec(&v3, &testdata("002_create_permissions.sql")).await;
    exec(&v3, &testdata("seed.sql")).await;
    let v4 = pg.database("bookmark").await.unwrap();
    pg.exec(
        "DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'bookmark_app') THEN \
         CREATE ROLE bookmark_app LOGIN NOBYPASSRLS PASSWORD 'bookmark-test'; END IF; END $$",
    )
    .await
    .unwrap();
    db::migrate_dsn(&v4, &store::MIGRATOR).await.unwrap();
    // The library runs are made as the NOBYPASSRLS application role: the
    // importer's system scope is what admits its cross-tenant writes (the
    // bundle's owner role is a superuser and would bypass row-level
    // security).
    let dst = db::connect(&DbConfig {
        dsn: Postgres::dsn_as(&v4, "bookmark_app", "bookmark-test"),
        migrate_dsn: String::new(),
        max_conns: 2,
    })
    .await
    .unwrap();

    // The source session is read-only.
    let mut c = migrate3::connect_source(&v3).await.unwrap();
    let e = sqlx::query("CREATE TABLE x (id int)")
        .execute(&mut c)
        .await
        .unwrap_err();
    assert!(e.to_string().contains("read-only"), "{e}");
    let _ = c.close().await;

    // Without -exclude-orphans the grant to v3 user 9 (no v4 account) fails
    // the run; nothing is written.
    let r = run(&v3, &dst, Mode::Apply, Options::default()).await;
    assert_eq!(r.status, Status::Failed);
    assert_eq!(r.findings.errors.len(), 1, "{:?}", r.findings.errors);
    assert_eq!(r.findings.errors[0].id, "7");
    assert!(r.findings.errors[0].reason.contains("gone@example.org"));
    assert_eq!(r.destination["bookmarks"], 0);

    // Dry-run: the plan, nothing written.
    let r = run(&v3, &dst, Mode::DryRun, EXCLUDE).await;
    assert_eq!(r.status, Status::Succeeded);
    assert_eq!(r.source["bookmarks"], 4);
    assert_eq!(r.source["permissions"], 7);
    assert!(r.source_users_table);
    assert_eq!(r.plan["bookmarks"].insert, 4);
    assert_eq!(r.plan["permissions"].insert, 6);
    assert!(!r.reconciled);
    assert_eq!(r.findings.excluded.len(), 1);
    assert_eq!(r.findings.expired_grants, 1);
    assert_eq!(r.destination["bookmarks"], 0);

    // Apply: one transaction, reconciled.
    let r = run(&v3, &dst, Mode::Apply, EXCLUDE).await;
    assert_eq!(r.status, Status::Succeeded, "{r:?}");
    assert!(r.reconciled);
    assert_eq!(r.destination["bookmarks"], 4);
    assert_eq!(r.destination["permissions"], 6);

    // The v4 rows: IDs, authors, subjects, expiry and times kept.
    let mut c = PgConnection::connect(&v4).await.unwrap();
    let mut tx = c.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(PLATFORM_TENANT)
        .execute(&mut *tx)
        .await
        .unwrap();
    let b = store::get_bookmark(&mut tx, PLATFORM_TENANT, B2.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(b.created_by, ANA);
    assert_eq!(b.created_at.to_rfc3339(), "2025-03-03T10:00:00.123456+00:00");
    let b3 = store::get_bookmark(&mut tx, PLATFORM_TENANT, B3.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (b3.created_by.as_str(), b3.description.as_str()),
        (BORIS, "two\nlines")
    );
    let rows = sqlx::query(
        "SELECT resource_id, relation, subject_type, subject_id, granted_by, expires_at IS NOT NULL
         FROM bookmark_permissions ORDER BY granted_at",
    )
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    let got: Vec<(String, String, String, String, String, bool)> = rows
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5)))
        .collect();
    let row = |res: &str, rel: &str, st: &str, sid: &str, by: &str, exp: bool| {
        (
            res.to_owned(),
            rel.to_owned(),
            st.to_owned(),
            sid.to_owned(),
            by.to_owned(),
            exp,
        )
    };
    assert_eq!(
        got,
        vec![
            row(B1, "owner", "user", ANA, ANA, false),
            row(B1, "viewer", "tenant", "", ANA, false),
            row(B2, "owner", "user", ANA, ANA, false),
            row(B2, "editor", "role", "admin", ANA, true),
            row(B3, "owner", "user", BORIS, BORIS, false),
            row(
                "0190a000-0000-7000-8000-000000000004",
                "owner",
                "user",
                CAROL,
                CAROL,
                false
            ),
        ]
    );
    tx.rollback().await.unwrap();
    let _ = c.close().await;

    // Re-runs are no-ops.
    let r = run(&v3, &dst, Mode::Apply, EXCLUDE).await;
    assert_eq!(r.status, Status::AlreadyApplied);
    assert_eq!(r.plan["bookmarks"].unchanged, 4);
    assert_eq!(r.plan["permissions"].unchanged, 6);
    let r = run(&v3, &dst, Mode::DryRun, EXCLUDE).await;
    assert!(r.status == Status::Succeeded && r.reconciled);

    // A row changed in v4 since is a conflict, never overwritten.
    exec(
        &v4,
        &format!(
            "SET app.system = 'on'; UPDATE bookmark_bookmarks SET title = 'Renamed' WHERE id = '{B1}';\
             DELETE FROM bookmark_permissions WHERE resource_id = '{B3}';"
        ),
    )
    .await;
    let r = run(&v3, &dst, Mode::Apply, EXCLUDE).await;
    assert_eq!(r.status, Status::Failed);
    assert_eq!(r.plan["bookmarks"].conflict, 1);
    assert_eq!(r.plan["permissions"].insert, 1);
    assert_eq!(r.destination["permissions"], 5, "nothing written");

    // The binary: source configuration, users files, JSON report, exit
    // status. The conflicting title is restored first.
    exec(
        &v4,
        &format!("SET app.system = 'on'; UPDATE bookmark_bookmarks SET title = 'Runbook' WHERE id = '{B1}';"),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let p = |n: &str| dir.path().join(n).to_str().unwrap().to_owned();
    std::fs::write(
        p("users.csv"),
        format!("email,id\nana@example.org,{ANA}\nboris@example.org,{BORIS}\n"),
    )
    .unwrap();
    std::fs::write(p("v3-users.csv"), format!("3,{CAROL}\n")).unwrap();
    std::fs::write(
        p("import.yaml"),
        format!(
            "source: {{dsn: {{env: BOOKMARK_TEST_LEGACY_DSN}}}}\nusers_csv: {}\nv3_users_csv: {}\nroles: {{\"platform:admin\": admin}}\n",
            p("users.csv"),
            p("v3-users.csv")
        ),
    )
    .unwrap();
    std::fs::write(
        p("config.yaml"),
        format!("service_name: bookmark\ndb: {{dsn: \"{v4}\"}}\n"),
    )
    .unwrap();
    let bin = |extra: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_bookmark-migrate"))
            .args(["-source-config", &p("import.yaml"), "-config", &p("config.yaml")])
            .args(extra)
            .env("BOOKMARK_TEST_LEGACY_DSN", &v3)
            .output()
            .unwrap();
        let rep: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
        (
            out.status.code(),
            rep,
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let (code, rep, _) = bin(&["-apply"]);
    assert_eq!(code, Some(1), "the orphan fails the run: {rep}");
    assert_eq!(rep["errors"][0]["id"], "7");
    let (code, rep, err) = bin(&["-apply", "-exclude-orphans"]);
    assert_eq!(code, Some(0), "{err}");
    assert_eq!(rep["status"], "succeeded");
    assert_eq!(rep["mode"], "apply");
    assert_eq!(rep["plan"]["permissions"]["insert"], 1);
    assert_eq!(rep["destination"]["permissions"], 6);
    assert_eq!(rep["tenants"]["0"], PLATFORM_TENANT);
    assert!(
        rep["excluded"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("gone@example.org")
    );
    let (code, rep, _) = bin(&[
        "-dry-run",
        "-exclude-orphans",
        "-tenant",
        "0=0192f6b0-0000-7000-8000-0000000000f1",
    ]);
    assert_eq!(code, Some(0));
    assert_eq!(
        rep["plan"]["bookmarks"]["insert"], 4,
        "another tenant: the IDs are per tenant"
    );
    let (code, _, err) = bin(&["-dry-run", "-apply"]);
    assert_eq!(code, Some(2), "{err}");
}
