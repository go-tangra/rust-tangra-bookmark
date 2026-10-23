# Feature Specification: Bookmark Module for v4 (Rust SDK)

**Feature Branch**: `038-bookmark-v4` (implemented on branch `v4` of `rust-tangra-bookmark`)

**Created**: 2026-10-16

**Status**: Backend implemented (API, storage, authorization, help pages, tests); UI remote, v3 data import and packaging follow (tasks.md, Phases 7–9)

**Spans**: rust-tangra-bookmark (v4 module: API + later UI remote), go-tangra-sdk-rust (new `db` feature and `testutil::pg`), go-tangra-auth (bookmark permissions and module roles, via registration only), go-tangra-portal-v4 (gateway route and UI remote, via registration only)

**Input**: User description: "migrate bookmark to v4 using the Rust SDK"

**Decisions taken with the user (2026-10-16)**:

1. **Same repository**: `rust-tangra-bookmark` stays the home of the module.
   v4 is built on branch `v4` and later merged to `main`; v3 continues on
   branch `v3`.
2. **v3 data is imported with a tool**: `bookmark-migrate` reads a v3
   database read-only and writes into a v4 tenant (dry run, then apply in one
   transaction; users mapped by e-mail through a CSV, roles through a
   `v3=v4` map; v3 tenant 0 → platform tenant
   `00000000-0000-0000-0000-000000000001`; bookmark UUIDs kept; JSON report).
   The importer is a later phase of this feature (FR-060–FR-066); this
   specification fixes the v4 schema it writes into.
3. **HTTP through the gateway only**: no mesh gRPC API for other modules.
4. **Backup is kept**: export/import like paperless v4 (`backup:manage`,
   schema version, skip/overwrite, IDs preserved).

**Defaults chosen by the lead (not the user)**:

- Authorization follows paperless v4 (`internal/authz`, after its
  tenant-grant fix): relations owner/editor/sharer/viewer, subjects
  user/role/tenant, expiry, tenant `admin`/`owner` short-circuit, existence
  masking; no inheritance (bookmarks are flat).
- The coarse permissions are enforced (v3 registered them and never checked
  them).
- Strict v3 feature parity: no folders, metadata fetching or other new
  features.
- Spec number 038.

## Context

### What v3 provides

The v3 bookmark service (Rust, tonic gRPC behind the v3 admin gateway)
keeps a tenant's web bookmarks:

- **Bookmarks**: URL, title, description, tags; created by a user; listed
  with paging and a tag filter; read, updated (tags replaced only when asked)
  and deleted by ID.
- **Sharing** ("Zanzibar-like" tuples): a bookmark is shared with a user, a
  role or the whole tenant as owner (read, write, delete, share), editor
  (read, write), sharer (read, share) or viewer (read), optionally until an
  expiry time. The creator becomes owner. The list shows only bookmarks the
  caller can read. Grant, revoke, list grants, check one permission, list
  accessible bookmarks and the caller's effective permissions are exposed.
- **Users and roles**: the service proxies the v3 admin service's user and
  role lists so the UI can pick share targets and show names.
- **Backup**: JSON export (a platform admin with tenant 0 exports every
  tenant) and import with skip/overwrite.
- **Roles**: permission codes `bookmark.bookmark.{view,create,update,delete}`
  and `bookmark.permission.manage`, roles `bookmark.admin` and
  `bookmark.viewer`, two menu entries.

### The v4 gap

v4 has no bookmark module. Porting v3 is not a copy; v3 has defects v4 must
not inherit:

- **Coarse permissions are never enforced**: every user reaches every
  operation; only the tuples are checked.
- **Reads and writes by ID ignore the tenant**: `get`, `update` and `delete`
  select by bookmark ID only, after a tuple check scoped to the tenant.
  The ID space is global, so the tuple check is the only isolation.
- **Unreadable bookmarks are distinguishable**: a bookmark the caller cannot
  read answers "permission denied", one that does not exist "not found".
- **No admin override**: a tenant administrator cannot see or clean up a
  bookmark nobody shared with them.
- **Any URL is stored**: `javascript:` and `data:` URLs are accepted and
  rendered as links.
- **Unbounded input**: description and tags have no limits; page size is
  capped at 100 but nothing else is.
- **Errors leak internals**: database errors are returned to the client
  verbatim (`database error: ...`).
- **No business audit**: a log line per RPC only.
- **Users/roles proxy**: v4 modules do not proxy auth; the UI reads auth's
  directory through the gateway.
- **v3 plumbing**: admin-service registration, menus.yaml, proto
  descriptors, `x-md-global-*` metadata, integer tenant and user IDs.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Keep bookmarks (Priority: P1)

Ana saves `https://go.dev/doc` titled "Go docs" with tags `go` and `docs`.
She lists her bookmarks newest first, filters by tag `go`, searches "docs",
edits the description and later deletes one she no longer needs.

**Why this priority**: keeping bookmarks is the module.

**Independent Test**: through the API, create, list (paging, sort, tag
filter, search), read, update and remove bookmarks as one user; another
tenant sees none of them.

**Acceptance Scenarios**:

1. **Given** a user with `bookmarks:create`, **When** they create a bookmark
   with an http(s) URL, optional title, description and tags, **Then** it is
   stored in their tenant with them as creator and owner, and returned with
   their effective permissions.
2. **Given** bookmarks, **When** the user lists them, **Then** they get one
   page (default 25, at most 200) of the bookmarks they can read, sorted by
   `created_at` (newest first) or by `title`, `url` or `updated_at` in
   either order, with the exact total.
3. **Given** a tag filter (one or more tags), **Then** only bookmarks
   carrying every given tag are listed; **given** `q`, only bookmarks whose
   title or URL contains it (case-insensitive).
4. **Given** a bookmark the user can write, **When** they update some fields,
   **Then** only those change; `tags` replaces the whole tag set when given.
5. **Given** a bookmark the user owns, **When** they remove it, **Then** it
   and all its grants are deleted.
6. **Given** invalid input (missing or non-http(s) URL, too long fields, too
   many tags, unknown fields), **Then** the request is refused with
   `422 validation_failed` naming the fields; a body that is not JSON is
   `400 malformed_body`.

---

### User Story 2 - Share a bookmark (Priority: P1)

Ana shares "Go docs" with Boris as viewer until Friday, with the role
`developers` as editor, and with everyone in the tenant as viewer. Boris is
told on screen that a bookmark was shared with him. Ana later revokes the
role grant.

**Why this priority**: sharing is what distinguishes the module from a
browser's bookmarks; v3 users rely on it.

**Independent Test**: grant, list, check, effective and revoke through the
API with three users and one role; expired grants stop counting; a sharer
cannot promote anyone.

**Acceptance Scenarios**:

1. **Given** a bookmark the caller can share, **When** they grant viewer or
   sharer to a user (by user ID), a role (by slug) or the tenant, optionally
   with a future expiry, **Then** the grant is stored and the subject gains
   the relation's permissions on that bookmark only.
2. **Given** an editor or owner grant (or revoking one), **Then** the caller
   must hold every permission on the bookmark (owner, or a tenant
   administrator); a sharer is refused (`403`).
3. **Given** a grant whose expiry has passed, **Then** it no longer confers
   anything (it stays listed until revoked).
4. **Given** the same subject and relation granted twice on a bookmark,
   **Then** the second grant is refused with `409 conflict`.
5. **Given** a grant to a user, **Then** that user receives a
   `bookmark.shared` event carrying the bookmark ID and relation only.
6. **Given** a bookmark the caller can share, **When** they list its grants,
   **Then** they see every grant on it; **when** they list without a
   bookmark, they see the grants on every bookmark they can share
   (administrators: every grant of the tenant), with paging and filters by
   subject.
7. **Given** any readable bookmark, **When** the caller asks for their
   effective permissions or checks one permission, **Then** they get
   read/write/delete/share, their highest relation and the grants that
   apply to them.
8. **Given** a revoke naming a grant of another bookmark, **Then** it is
   `404 not_found`.

---

### User Story 3 - Tenant administrators and tenant-wide grants (Priority: P2)

A tenant administrator opens the bookmark list and sees every bookmark of
the tenant, removes a stale one shared by a former colleague, and gives
everyone viewer access to all bookmarks of the tenant.

**Why this priority**: v3 had no way to clean up or open the whole tenant's
bookmarks.

**Acceptance Scenarios**:

1. **Given** a caller with the tenant role `admin` or `owner`, **Then** they
   hold every permission on every bookmark of their tenant (not others).
2. **Given** an administrator, **When** they grant a relation to the tenant
   without naming a bookmark, **Then** every member holds that relation on
   every bookmark of the tenant. A tenant grant on one bookmark stays on
   that bookmark.
3. **Given** a non-administrator, **Then** a grant without a bookmark is
   refused (`403`); a grant without a bookmark to a user or role is invalid
   (`422`).

---

### User Story 4 - Backup and restore (Priority: P2)

An administrator exports the tenant's bookmarks and grants before an
upgrade and, after a mistake, imports the file with "skip" (keep what
exists) or "overwrite" (replace what the file holds).

**Acceptance Scenarios**:

1. **Given** `backup:manage`, **When** exporting, **Then** the file holds
   schema version 1, every bookmark and grant of the caller's tenant, with
   their IDs, and nothing of other tenants.
2. **Given** a backup, **When** importing with `skip`, **Then** bookmarks and
   grants that already exist (bookmark by ID, grant by bookmark, subject and
   relation) are kept and counted as skipped; the rest is created with the
   IDs from the file. With `overwrite`, existing ones are replaced.
3. **Given** an unsupported schema version, an invalid entry or an unknown
   mode, **Then** nothing is imported and the request is `422`.
4. **Given** a backup from another tenant, **Then** importing writes into the
   caller's tenant only.

---

### User Story 5 - The module on the platform (Priority: P1)

An operator installs the module; it enrols its identity, registers its
permissions and roles with auth, its routes, help pages and UI remote with
the gateway, and appears in the portal navigation for users with
`bookmarks:read`.

**Acceptance Scenarios**:

1. **Given** the module starts, **Then** it applies its migrations, opens the
   database pool, registers with auth (permissions, module roles, built-in
   grants) and with the gateway (manifest, docs digest, build version).
2. **Given** a request without a valid platform token, **Then** `401
   unauthenticated` with `WWW-Authenticate: Bearer realm="freya"`.
3. **Given** `bookmark bootstrap -config <path>`, **Then** the migrations
   are applied with the migration role and the process exits.

### Edge Cases

- A page beyond the end answers the last page.
- A bookmark of another tenant, a malformed ID and a bookmark the caller
  cannot read all answer `404 not_found` (one shape).
- A tag differing only in surrounding spaces is the same tag; duplicates in
  one request collapse.
- Removing a bookmark removes its grants in the same transaction.
- Revoking the last owner grant is allowed; tenant administrators keep
  access (v3 parity).
- An expired grant cannot be created (expiry must be in the future).
- Import of a grant whose bookmark is in neither the file nor the tenant is
  skipped.

## Requirements *(mandatory)*

### Functional Requirements

**Bookmarks**

- **FR-001**: Users with `bookmarks:create` MUST be able to create a
  bookmark: URL (required, absolute `http`/`https`, at most 2048
  characters), title (at most 500), description (at most 5000), tags (at
  most 20, each 1–64 characters after trimming, no control characters,
  duplicates collapsed).
- **FR-002**: The creator MUST become the bookmark's owner (an owner grant to
  their user ID, written in the same transaction).
- **FR-003**: Users with `bookmarks:read` MUST be able to list the bookmarks
  they can read with the platform list contract (`page`, `page_size` ≤ 200,
  `sort` ∈ {created_at (default, desc), updated_at, title, url}, `order`;
  `cursor`/`limit` refused) plus `tag` (repeatable, ≤ 10, all must match)
  and `q` (≤ 200 characters, title or URL contains, case-insensitive).
- **FR-004**: Users with `bookmarks:read` MUST be able to read a bookmark
  they can read.
- **FR-005**: Users with `bookmarks:update` MUST be able to change any of
  URL, title, description and tags of a bookmark they can write; `tags`
  replaces the set when present (v3 `update_tags`).
- **FR-006**: Users with `bookmarks:delete` MUST be able to remove a bookmark
  they can delete; its grants are removed with it.
- **FR-007**: Every bookmark returned MUST carry the caller's effective
  permissions on it (read, write, delete, share), so the UI needs no extra
  call per row.

**Sharing**

- **FR-010**: Relations confer: owner = read, write, delete, share; editor =
  read, write; sharer = read, share; viewer = read (v3).
- **FR-011**: Subjects: a user (platform user ID, a UUID), a role (a role
  slug from the caller's token vocabulary) or the tenant (everyone in it).
- **FR-012**: Users with `permissions:manage` MUST be able to grant a
  relation on a bookmark they can share, with an optional future expiry.
  Granting or revoking editor or owner MUST require every permission on the
  bookmark.
- **FR-013**: A grant without a bookmark MUST only be possible to the tenant
  subject ("every bookmark of the tenant") and only for callers who can
  share the whole collection (tenant administrators, or holders of such a
  tenant-wide sharer/owner grant).
- **FR-014**: Users with `permissions:manage` MUST be able to revoke a grant
  of a bookmark they can share; a grant ID of another bookmark is not found.
- **FR-015**: Users with `permissions:manage` MUST be able to list the grants
  of a bookmark they can share, or (without a bookmark) every grant on
  bookmarks they can share, paged, filterable by subject type and ID.
- **FR-016**: Users with `bookmarks:read` MUST be able to check one
  permission and read their effective permissions on a bookmark (or on the
  collection): permissions, highest relation, contributing grants.
- **FR-017**: Duplicate grants (same bookmark, subject and relation) MUST be
  refused with `409 conflict`.
- **FR-018**: Expired grants MUST confer nothing.
- **FR-019**: When a grant names a user, the module SHOULD publish
  `bookmark.shared` to that user (when the event bus is enabled).

**Authorization**

- **FR-020**: Every route MUST declare its coarse permission in the OpenAPI
  document; the gateway enforces it from the manifest, and the module only
  admits the gateway (mesh policy).
- **FR-021**: Fine-grained access MUST come from the grants: direct user,
  any of the caller's roles, the tenant on that bookmark, the tenant on the
  whole collection.
- **FR-022**: Callers holding the tenant role `admin` or `owner` MUST hold
  every permission on every bookmark of their tenant (paperless `IsAdmin`).
- **FR-023**: A bookmark the caller cannot read, of another tenant, or that
  does not exist MUST answer `404 not_found`; a readable bookmark lacking the
  needed permission `403 forbidden`.

**Backup**

- **FR-030**: Users with `backup:manage` MUST be able to export the caller's
  tenant: schema version 1, export time, bookmarks and grants with IDs.
- **FR-031**: Import MUST accept mode `skip` (default) or `overwrite`, keep
  IDs, write only into the caller's tenant, validate every entry with the
  create rules, refuse an unsupported schema version, and apply all or
  nothing; it reports created and skipped counts per kind.
- **FR-032**: Backup bodies are limited (16 MiB by default,
  `limits_bookmark.backup_max_bytes`).

**Platform**

- **FR-040**: Module `bookmark`, API prefix `/api/bookmark`, routes under
  `/api/bookmark/v1`, help pages, UI remote `/m/bookmark/` with `./routes`,
  `./nav`, `./boot`.
- **FR-041**: Permissions registered with auth: `bookmarks:read`,
  `bookmarks:create`, `bookmarks:update`, `bookmarks:delete`,
  `permissions:manage`, `backup:manage`; module roles `administrator`
  (all) and `viewer` (`bookmarks:read`); built-in grants owner/admin → all,
  member/operator → bookmarks:* + permissions:manage, auditor →
  bookmarks:read.
- **FR-042**: PostgreSQL storage with the tenant in every statement and
  row-level security backing it; migrations embedded and applied by
  `bootstrap` and at start with the migration role.
- **FR-043**: Business audit of every change (closed vocabulary,
  contracts/audit-events.md).
- **FR-044**: Help pages: start page, getting started, sharing, backup,
  reference, operator page.
- **FR-045**: Users and roles for share pickers come from auth's directory
  through the gateway (`/api/v1/users`, `/api/v1/users/lookup`,
  `/api/v1/roles`); the module stores IDs only.

**v3 data import (later phase)**

- **FR-060**: `bookmark-migrate` MUST read the v3 database read-only in one
  snapshot.
- **FR-061**: `-dry-run` MUST report what would be written; `-apply` MUST
  write in one transaction.
- **FR-062**: v3 user IDs MUST map to v4 user UUIDs through an `email,uuid`
  CSV; v3 role IDs through `v3=v4` pairs; unmapped subjects are reported
  and skipped.
- **FR-063**: v3 tenant 0 MUST map to `00000000-0000-0000-0000-000000000001`.
- **FR-064**: Bookmark UUIDs MUST be kept; v3 relations `RELATION_*` map to
  the v4 names; the v3 tenant subject `all` maps to the tenant subject on
  the same bookmark.
- **FR-065**: Invalid v3 rows (e.g. non-http(s) URLs) MUST be reported, not
  silently changed.
- **FR-066**: A JSON report lists counts and every skipped row with a reason.

### Security Requirements *(mandatory — Constitution: Development Workflow)*

- **Trust boundaries crossed**: browser → gateway → bookmark (user API);
  bookmark → auth (keys, revocations, registration); bookmark → gateway
  (registration); bookmark → Valkey (events); bookmark → PostgreSQL.
- **Data classification**: **Internal (personal)**: bookmark URLs, titles,
  descriptions, tags; who shares what with whom.
- **Threat scenarios**: reading another tenant's or another user's bookmarks
  by ID; learning that a private bookmark exists; a sharer promoting
  themselves to editor/owner; reaching other bookmarks' grants through a
  revoke; storing script URLs; leaking content through events, logs,
  errors or audit; a backup crossing tenants.
- **SR-001**: Every statement MUST carry the caller's tenant; row-level
  security (app role `NOBYPASSRLS`, `app.tenant_id` per transaction) backs
  it.
- **SR-002**: Unreadable and foreign bookmarks MUST be indistinguishable
  from absent ones (FR-023).
- **SR-003**: Grant management MUST be capped at the caller's own level
  (FR-012).
- **SR-004**: Only `http` and `https` URLs MUST be stored.
- **SR-005**: Events, logs and audit details MUST carry IDs and codes only
  — no URLs, titles, descriptions or tags.
- **SR-006**: Internal errors MUST answer `500 internal` without detail.
- **SR-007**: Inputs MUST be bounded (field lengths, tag counts, page size,
  query length, body size).
- **SR-008**: Production MUST refuse a database DSN that does not verify the
  server certificate (`sslmode=verify-full|verify-ca`).

### Key Entities

- **Bookmark**: tenant, ID, URL, title, description, tags, creator, created
  and updated times.
- **Grant (permission tuple)**: tenant, ID, resource (bookmark ID or "every
  bookmark"), relation, subject type and ID, granted by, granted at,
  expiry.
- **Audit event**: time, tenant, type, actor, subject, outcome, reason,
  correlation ID, details (codes and IDs).

## Success Criteria *(mandatory)*

- **SC-001**: A user saves, finds (tag or text) and opens a bookmark in under
  30 seconds in the portal.
- **SC-002**: 0 cross-tenant reads or writes and 0 reads of unshared
  bookmarks succeed in the authorization test suite; unreadable and absent
  bookmarks answer byte-identical responses.
- **SC-003**: 0 promotions by sharers succeed in the authorization test
  suite.
- **SC-004**: An export followed by an import into an empty tenant
  reproduces every bookmark and grant with the same IDs.
- **SC-005**: A list page of 25 bookmarks out of 10,000 is served in under
  100 ms on the reference database.
- **SC-006**: The 4 bookmarks and 7 grants of the v3 production tenant are in
  v4 after the import (later phase), or reported with a reason.

## Assumptions

- **People**: users are auth's tenant members; the module stores user IDs
  and role slugs. Names are resolved by the UI from auth's directory.
- **Roles in tokens**: role subjects match the `roles` claim of the
  platform token (built-in role slugs and module role slugs as auth issues
  them).
- **Administrators**: the tenant roles `admin` and `owner` are tenant
  super-users (as paperless); the module role "Bookmark administrator"
  grants every coarse permission but no bookmark access by itself.
- **Database**: one PostgreSQL database per module install (the bundle's own
  server, as deployer); no TimescaleDB features needed.
- **Localization**: English, like the other v4 modules.

## Dependencies

- go-tangra-sdk-rust with the `db` feature (added with this feature).
- Auth: token keys, revocations, permission registration with module roles.
- Portal gateway: registration, route enforcement, help bundle, UI remote,
  SSE fan-out of `bookmark.shared`.
- Valkey event bus (optional, for `bookmark.shared`).

## Out of Scope

- Folders, collections, favicon or metadata fetching, link checking,
  browser extensions (strict v3 parity).
- A mesh gRPC API for other modules (decision 3).
- The v3 platform-wide "full backup" for tenant 0: exports are per tenant,
  as paperless.
- Listing "accessible resource IDs" as a public endpoint: the bookmark list
  is that list.
