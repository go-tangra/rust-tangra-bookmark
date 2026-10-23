# Implementation Plan: Bookmark Module for v4

**Branch**: `v4` (rust-tangra-bookmark) | **Spec**: [spec.md](./spec.md) | **Date**: 2026-10-16

## Summary

Rebuild the v3 bookmark service as a v4 module in Rust on the go-tangra Rust
SDK (`go-tangra-sdk-rust`), shaped like the SDK's example module
(`examples/notes`) and the Go reference module (deployer): one binary
`bookmarksvc` serving an HTTP API behind the gateway, PostgreSQL through the
SDK's new `db` feature, the paperless v4 tuple authorization without
inheritance, the paperless backup format, embedded help pages and (later)
a Vue remote.

## Technical Context

- **Language**: Rust (edition 2024, stable 1.99), crate `tangra-bookmark`,
  binary `bookmarksvc`.
- **Platform SDK**: `tangra-sdk` by path (`../go-tangra-sdk-rust/crates/tangra-sdk`)
  during development, features `db` and `events`; switched to a git rev
  before release (tasks.md T090).
- **Storage**: PostgreSQL 16 via sqlx 0.8 (rustls), migrations embedded with
  `sqlx::migrate!`, row-level security, tenant in every statement.
- **HTTP**: axum through the SDK `App` (mTLS h2, policy, admin listener).
- **Testing**: unit tests (authz matrix, validation, config), manifest pin,
  help docs, and an end-to-end test over `testutil::platform` (fake gateway
  and auth over mTLS) with a real throwaway PostgreSQL (`testutil::pg`).
- **Constraints**: no `unwrap()` outside tests; `cargo fmt`, `clippy -D
  warnings`, tests clean.

## Constitution Check

- Security first: tenant isolation in SQL and RLS, existence masking, capped
  grant management, bounded inputs, no content in events/logs/audit. ✅
- Tests precede implementation per phase (tasks.md). ✅
- Platform contracts: manifest from OpenAPI with `x-freya-*`, error
  vocabulary, list contract, help docs, registration — all through the SDK. ✅

## Structure

```text
api/openapi/bookmark.yaml        routes + x-freya-* (gateway manifest source)
migrations/0001_schema.sql       tables, constraints, indexes
migrations/0002_rls.sql          row-level security + grants to bookmark_app
docs/help/                       help pages (index, getting-started, guides/, reference, operations)
src/main.rs                      -config | bootstrap | --healthcheck | version
src/config.rs                    framework keys + gateway, db, valkey, events, enroll, limits_bookmark
src/manifest.rs                  permissions, roles, grants, abilities, nav, manifest, registration
src/module.rs                    wiring: identity, App, migrate, pool, verifier, registrations, events, router
src/authz.rs                     relations, permissions, subjects, effective(), admin short-circuit (pure)
src/store.rs                     SQL: bookmarks, grants, accessible list, audit (tenant-scoped tx)
src/backup.rs                    export/import (schema 1, skip/overwrite, IDs kept)
src/api/{mod,bookmarks,permissions,backup,input}.rs   handlers, validation, error mapping
src/docs.rs, src/remote.rs, src/healthcheck.rs
tests/{manifest,docs,module}.rs  manifest pin, help docs, end-to-end
testdata/manifest.sha256
```

## Design decisions

1. **Permission names** follow paperless (`<plural resource>:<action>`):
   `bookmarks:read|create|update|delete`, `permissions:manage`,
   `backup:manage`. v3's create/update split is kept (v3 parity).
2. **Coarse enforcement** is the gateway's (route permission from the
   manifest); the module only admits the gateway. The manifest test pins the
   route → permission table.
3. **Relations** keep v3's meaning (editor has no delete; paperless editors
   also delete — bookmark keeps v3).
4. **Admin short-circuit**: paperless `IsAdmin` — token roles `admin` or
   `owner`. The module role `administrator` grants coarse permissions only.
5. **404 masking** is stricter than paperless: a readable-but-forbidden
   bookmark is `403`, an unreadable one `404` (paperless answers `403` for
   an existing unreadable document; bookmarks are personal, so existence is
   masked too).
6. **Tenant-wide grants**: a tenant subject with empty `resource_id` (the
   paperless PR #13 semantics); a tenant grant on one bookmark stays on it.
   v3's `subject_id = "all"` becomes `''`.
7. **List**: one SQL query with an `EXISTS` over matching grants (no
   in-memory ID list as in v3), returning the applicable relations so every
   row carries the caller's permissions.
8. **Grant listing** needs share on the bookmark (paperless: read); viewers
   use `effective`, which returns only the grants applying to them
   (paperless returns all).
9. **Removal** is `POST /bookmarks/{id}/remove` (bundle policies allow GET,
   HEAD, POST, PUT only), as notes and paperless.
10. **Backup**: per tenant only (paperless); bookmark dedupe by ID, grant
    dedupe by tuple; the whole import is validated and applied in one
    transaction.
11. **Audit** rows are written in the change's transaction (no async
    writer: the volume is small).
12. **Events**: only `bookmark.shared` to a user grantee, IDs only;
    `events.enabled` defaults to `false` (the module works without Valkey).
13. **Primary keys per tenant** (`(tenant_id, id)`) so a backup can move
    between tenants with its IDs.

## Phases

See [tasks.md](./tasks.md): SDK `db` feature → setup → foundational
(config, manifest, schema, authz) → US1 bookmarks → US2 sharing → US3
admin/tenant-wide → US4 backup → US5 platform wiring and help → UI remote
(later agent) → v3 importer (later) → packaging and release (later).
