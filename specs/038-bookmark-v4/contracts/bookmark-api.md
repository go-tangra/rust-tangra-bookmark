# Contract: Bookmark browser API (through the gateway)

OpenAPI 3.1 document `api/openapi/bookmark.yaml` (the source of the gateway
routes via `x-freya-*`). Base path `/api/bookmark/v1`. JSON; errors in the
platform envelope `{"reason": "...", "detail"?: {...}}`:

| status | reason | when |
|---|---|---|
| 400 | malformed_body | body is not JSON |
| 401 | unauthenticated | no/invalid platform token (`WWW-Authenticate: Bearer realm="freya"`) |
| 403 | forbidden | readable bookmark, missing permission; sharer handling editor/owner |
| 404 | not_found | absent, foreign or unreadable bookmark; unknown grant; unknown path |
| 405 | method_not_allowed | declared path, other method |
| 409 | conflict | duplicate grant |
| 413 | body_too_large | body over the route limit |
| 422 | validation_failed | `detail.fields` (body) or `detail.param` (query) |
| 500 | internal | never with detail |

Every `POST`/`PUT` requires `X-CSRF-Token` (1–128 bytes; the gateway's
double-submit token), else `422`.

## Routes

| method | path | permission | body / query | answer |
|---|---|---|---|---|
| GET | /info | public | | `{module, version}` |
| GET | /bookmarks | bookmarks:read | `page, page_size (≤200), sort (created_at\|updated_at\|title\|url), order, q (≤200), tag (repeatable ≤10)` | `BookmarkPage` |
| POST | /bookmarks | bookmarks:create | `NewBookmark {url, title?, description?, tags?}` | 201 `Bookmark` |
| GET | /bookmarks/{id} | bookmarks:read | | `Bookmark` |
| PUT | /bookmarks/{id} | bookmarks:update | `BookmarkPatch {url?, title?, description?, tags?}` (≥1 field) | `Bookmark` |
| POST | /bookmarks/{id}/remove | bookmarks:delete | | 204 |
| GET | /permissions | permissions:manage | `resource_id?, subject_type?, subject_id?, page, page_size, sort (granted_at\|relation\|subject_type), order` | `GrantPage` |
| POST | /permissions/grant | permissions:manage | `NewGrant {resource_type?='bookmark', resource_id?, subject_type, subject_id?, relation, expires_at?}` | 201 `Grant` |
| POST | /permissions/revoke | permissions:manage | `{resource_type?, resource_id?, id}` | `{revoked: true}` |
| POST | /permissions/check | bookmarks:read | `{resource_type?, resource_id?, permission: read\|write\|delete\|share}` | `{allowed}` |
| GET | /permissions/effective | bookmarks:read | `resource_id?` | `Effective` |
| POST | /backup/export | backup:manage | (empty) | `Backup` |
| POST | /backup/import | backup:manage (16 MiB, 120 s) | `{mode?: skip\|overwrite, backup: Backup}` | `ImportResult` |

`resource_id` omitted or empty means the collection ("every bookmark of the
tenant"): allowed for grants only with `subject_type: tenant`.

## Shapes

```text
Bookmark     {id, url, title, description, tags[], created_by, created_at, updated_at,
              permissions: {read, write, delete, share}}
BookmarkPage {items: Bookmark[], total, page, page_size, sort, order}
Grant        {id, resource_type, resource_id, subject_type, subject_id, relation,
              granted_by, granted_at, expires_at?}
GrantPage    {items: Grant[], total, page, page_size, sort, order}
Effective    {permissions: {read, write, delete, share}, relation: owner|editor|sharer|viewer|null,
              admin: bool, grants: Grant[]}   (grants: only those applying to the caller)
Backup       data-model.md "Backup document"
ImportResult {bookmarks_imported, bookmarks_skipped, permissions_imported, permissions_skipped}
```

## Validation codes (`detail.fields.<field>`)

`required`, `invalid`, `too_long`, `too_many`, `unknown` (unknown body
field), `scheme` (URL not http/https), `past` (expiry not in the future).
Values are never echoed.

## Access matrix (fine-grained, after the gateway's coarse check)

| operation | needs on the bookmark | unreadable / absent |
|---|---|---|
| read, list row | read | 404 / not listed |
| update | write | 404 |
| remove | delete | 404 |
| grant/revoke viewer, sharer | share | 404 |
| grant/revoke editor, owner | read+write+delete+share | 404 |
| list grants of a bookmark | share | 404 |
| check, effective | read (collection: none) | 404 |

Tenant roles `admin`/`owner` pass every check within their tenant.

## Users and roles

The module stores user IDs and role slugs. The UI resolves names and offers
pickers from auth through the gateway: `GET /api/v1/users?q=`,
`POST /api/v1/users/lookup`, `GET /api/v1/roles` (as paperless
`ui/src/stores/directory.ts`).
