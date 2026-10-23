# Tasks: Bookmark Module for v4

**Feature**: 038-bookmark-v4 | **Spec**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md)

Tests are MANDATORY and precede implementation in every phase.
`[P]` = parallelizable; `[US#]` = user story. Paths are relative to the
repository root unless a repository is named. Release tasks (push, tags,
images, deploy) are marked **(release)** and are not done in this change.

> **Implementation note (2026-10-16):** HTTP, sharing, backup and isolation
> flows are covered end to end in `tests/module.rs` (fake platform over mTLS
> + a throwaway PostgreSQL from `tangra_sdk::testutil::pg`; skipped with a
> notice when no PostgreSQL binaries are available); the authorization
> matrix is unit-tested in `src/authz.rs`, input validation in
> `src/api/input.rs`.

## Phase 0: SDK (go-tangra-sdk-rust, branch `feat/db`)

- [x] T001 `tangra_sdk::db` (feature `db`): `DbConfig` (dsn, migrate_dsn, max_conns; strict; validate(production): verify-full|verify-ca; warnings), `connect`, `ping`, `migrate`/`migrate_dsn`, `Scope` + `begin` (app.tenant_id / app.system), `is_unique_violation`.
- [x] T002 `testutil::pg` (feature `testutil-pg`): throwaway PostgreSQL from `TANGRA_TEST_PG_DSN`, `PG_DIR`, the cache or the zonky jar; TLS option; skip with notice.
- [x] T003 `tests/db_pg.rs`: migrate with the owner role, RLS isolation, unique violation, production refusal, verify-full over TLS.
- [x] T004 README deviations + guide step 6a (storage).

## Phase 1: Setup

- [x] T010 Remove the v3 service (tonic, admin registration, menus.yaml, descriptor, x-md-global, cert.rs, users/roles proxy, v3 frontend, configs, Dockerfile, CI).
- [x] T011 `Cargo.toml` crate `tangra-bookmark`, binary `bookmarksvc`, `tangra-sdk` by **path** `../go-tangra-sdk-rust/crates/tangra-sdk` (features `db`, `events`; dev: `testutil-pg`); feature `ui`; `build.rs` (`BOOKMARK_VERSION`, `BOOKMARK_UI_DIST`).
- [x] T012 `specs/038-bookmark-v4/` (this directory).

## Phase 2: Foundational

### Tests first
- [x] T020 [P] `src/config.rs` tests: defaults, unknown keys refused, db required, production sslmode, valkey only with events, limits bounds.
- [x] T021 [P] `src/authz.rs` tests: relation matrix, union, expiry, role and tenant subjects, tenant grant on one bookmark stays on it, collection grants, admin/owner short-circuit, highest relation.
- [x] T022 [P] `tests/manifest.rs`: pin, gateway validation, route → permission table, registration ↔ manifest, nav icons in the kit set, every OpenAPI route served (401 without token).

### Implementation
- [x] T023 `api/openapi/bookmark.yaml` (contracts/bookmark-api.md).
- [x] T024 `src/manifest.rs` (permissions, roles administrator/viewer, built-in grants, abilities, nav `mdi-link-variant`, `mdi-account-key-outline`, `mdi-database-arrow-up-outline`), `testdata/manifest.sha256`.
- [x] T025 `migrations/0001_schema.sql`, `0002_rls.sql` (data-model.md).
- [x] T026 `src/authz.rs` (pure), `src/store.rs` (tenant-scoped SQL).

## Phase 3: US1 bookmarks (P1)

- [x] T030 [US1] tests: create/list/get/update/remove, paging, sort, tag and `q` filters, validation (400/422), tenant isolation, 404 shape — `tests/module.rs`, `src/api/input.rs`.
- [x] T031 [US1] `src/api/bookmarks.rs`, `src/api/input.rs`.

## Phase 4: US2 sharing (P1)

- [x] T040 [US2] tests: grant user/role/tenant, duplicate 409, expiry (past refused, expired stops counting), sharer cannot grant editor/owner, revoke of another bookmark's grant 404, list grants (bookmark / all I manage), check, effective, `bookmark.shared` event (with Valkey).
- [x] T041 [US2] `src/api/permissions.rs`, event publish.

## Phase 5: US3 administrators (P2)

- [x] T050 [US3] tests: admin and owner roles see and remove everything in their tenant only; tenant-wide grant by an admin; refused for others (403) and for user/role subjects (422).
- [x] T051 [US3] collection checks in `authz`/`store`.

## Phase 6: US4 backup (P2) and US5 platform (P1)

- [x] T060 [US4] tests: export only the caller's tenant; import into another tenant (IDs kept); skip vs overwrite counts; bad schema/mode/entry 422 with nothing written.
- [x] T061 [US4] `src/backup.rs`, `src/api/backup.rs`.
- [x] T062 [US5] tests: gateway and auth registration, docs bundle for the gateway only, healthcheck, 401 challenge.
- [x] T063 [US5] `src/module.rs`, `src/main.rs` (`bootstrap`, `--healthcheck`, `version`), `src/audit` rows in `store.rs`.
- [x] T064 [US5] `docs/help` (index, getting-started, guides/sharing, guides/backup, reference, operations) + `tests/docs.rs`.

## Phase 7: UI remote (next agent)

- [x] T070 `ui/` Vue 3 module-federation remote with `@go-tangra/ui` (name `bookmark`, base `/m/bookmark/`, exposes `./routes`, `./nav`, `./boot`), built from `api/openapi/bookmark.yaml` alone.
- [x] T071 Views: bookmark list (server-side table: sort, paging, tag chips, search), create/edit drawer, share drawer (auth directory pickers, relation, expiry), sharing page (`GET /permissions`), backup page; `boot` shows `bookmark.shared` toasts.
- [x] T072 Icon safelist (`ui/src/safelist.ts`) for page icons outside the kit set; styles spec test.
- [x] T073 Build with feature `ui` and check `/ui/mf-manifest.json` in `tests/module.rs`.

## Phase 8: v3 data import (next agent)

- [x] T080 `bookmark-migrate` (src/bin): source config, read-only snapshot, `-dry-run`/`-apply` (one transaction), `-tenant`, user CSV `email,uuid`, roles `v3=v4`, tenant 0 → `00000000-0000-0000-0000-000000000001`, relation/subject mapping, JSON report (FR-060–FR-066). Tests against a seeded v3 schema in `testutil::pg`.

## Phase 9: Packaging and release (next agent)

- [ ] T090 **BLOCKER before release: switch `tangra-sdk` from the path dependency to a git rev dependency.** The `db` and `testutil-pg` features exist only on the SDK's local, unpushed branch `feat/db` (993a737); the SDK's `main` (880d911) lacks them. Steps:
  1. merge `feat/db` into the SDK's `main` and push it (SDK repository is public, so CI needs no token);
  2. in `Cargo.toml`, replace both `tangra-sdk = { path = "../go-tangra-sdk-rust/crates/tangra-sdk", ... }` lines (dependencies and dev-dependencies) with `tangra-sdk = { git = "https://github.com/go-tangra/go-tangra-sdk-rust", rev = "<merged sha>", features = [...] }` and run `cargo update -p tangra-sdk` so `Cargo.lock` records the git source; for local SDK work afterwards use a `[patch."https://github.com/go-tangra/go-tangra-sdk-rust"]` entry in an untracked `.cargo/config.toml`, never a committed path;
  3. delete the stopgap: `.github/sdk-ref`, every "SDK checkout (path dependency, tasks.md T090)" step in `.github/workflows/ci.yaml` and `release.yaml`, and the `build-contexts: sdk=...` inputs (the Dockerfile's empty `FROM scratch AS sdk` stage then stands in; it may be removed with its `COPY --from=sdk`).
  Until then CI clones the public SDK next to the checkout at the commit in `.github/sdk-ref` (the path layout of local work), and the image build takes it as the named build context `sdk`. That commit must be pushed (or `.github/sdk-ref` updated to the merged sha) before any CI run can pass.
- [x] T091 `Dockerfile` (node UI stage with the `npm_token` secret → rust `--features ui` building `bookmarksvc` and `bookmark-migrate` → debian-slim, user 10001, `/state` 0700, OCI labels, `APP_VERSION`), `.dockerignore`; compose healthcheck `bookmarksvc --healthcheck`. Not built locally (no Docker access on the workstation); first built by CI.
- [x] T092 `deploy/bundle` (config.yaml: `db` verify-full + `sslrootcert` for both DSNs, enrolment, events off with the Valkey section documented; policy.yaml gateway → bookmark; compose.yaml with `bookmark-db` (TLS from `tls_hosts`), `bookmark-bootstrap`, ports 9975/9976, admin 127.0.0.1:9790; `init-db.sh` creating `bookmark_app NOBYPASSRLS`), `tangra-module.yaml`, `tests/catalogue.rs`. `tangra-catalogue build` (v4.10.0) passes against them.
- [x] T093 `.github/workflows/ci.yaml` (PRs, manual and called: fmt, clippy, tests with testutil-pg and a Valkey service, UI lint/test/build, docs-lint v4.10.0, image build without push) and `release.yaml` (main pushes and `v*` tags: the checks, GHCR push with semver + sha tags and no `latest`, catalogue entry on `v*` tags with `permissions-command: cargo run -q --locked --bin bookmark-catalogue-permissions`).
- [ ] T094 **(release)** merge `v4` into `main`, tag `v4.0.0`, publish the image and catalogue entry; install on prod; import the v3 rows; remove `bookmark-service` from the v3 stack.
