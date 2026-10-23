# Data Model: Bookmark v4

PostgreSQL, one database per install (`bookmark`). Two roles: the owner
(`postgres` in the bundle) runs the migrations (`db.migrate_dsn`);
`bookmark_app` (`LOGIN NOBYPASSRLS`) serves (`db.dsn`). Migrations are
embedded (`migrations/*.sql`, sqlx) and applied by `bookmarksvc bootstrap`
and at start.

IDs are UUIDs (v7 for new rows; v3 bookmark IDs kept by the importer).
Tenant IDs are UUIDs; user IDs and role slugs are text as auth issues them.

## `bookmark_bookmarks`

| column | type | rules |
|---|---|---|
| tenant_id | uuid | not null |
| id | uuid | not null; PK (tenant_id, id) |
| url | text | 1–2048 chars; `http`/`https` (checked by the API) |
| title | text | ≤ 500, default '' |
| description | text | ≤ 5000, default '' |
| tags | text[] | ≤ 20 entries, each 1–64 chars; GIN index |
| created_by | text | user ID of the creator ('' for imported rows without one) |
| created_at | timestamptz | default now() |
| updated_at | timestamptz | default now(); set on every update |

Indexes: `(tenant_id, created_at DESC, id)`, `(tenant_id, updated_at DESC, id)`,
`(tenant_id, lower(title), id)`, GIN `(tags)`.

The primary key is per tenant, so a backup of one tenant can be imported
into another with the same IDs.

## `bookmark_permissions` (grants)

| column | type | rules |
|---|---|---|
| tenant_id | uuid | not null |
| id | uuid | PK (tenant_id, id) |
| resource_type | text | `bookmark` |
| resource_id | text | bookmark ID, or '' = every bookmark of the tenant |
| relation | text | `owner` \| `editor` \| `sharer` \| `viewer` |
| subject_type | text | `user` \| `role` \| `tenant` |
| subject_id | text | user ID, role slug, '' for the tenant |
| granted_by | text | user ID of the granter |
| granted_at | timestamptz | default now() |
| expires_at | timestamptz | null = no expiry |

Constraints:

- `UNIQUE (tenant_id, resource_type, resource_id, relation, subject_type, subject_id)` (v3).
- `CHECK (resource_id <> '' OR subject_type = 'tenant')`: only the tenant
  subject can hold a collection-wide grant.
- `CHECK ((subject_type = 'tenant') = (subject_id = ''))`.

Indexes: `(tenant_id, resource_type, resource_id)`,
`(tenant_id, subject_type, subject_id)`.

Removing a bookmark deletes its grants in the same transaction (there is no
foreign key: `resource_id` is text so that '' can mean "every bookmark").

## `bookmark_audit_events`

| column | type |
|---|---|
| id | bigserial PK |
| ts | timestamptz default now() |
| tenant_id | uuid |
| event_type | text (contracts/audit-events.md) |
| actor_id | text |
| subject_kind | text (`bookmark` \| `permission` \| `backup`) |
| subject_id | text |
| outcome | text (`ok` \| `refused`) |
| reason | text |
| correlation_id | text |
| details | jsonb (codes and IDs only) |

Index `(tenant_id, ts DESC)`.

## Row-level security

Every table: `ENABLE` and `FORCE ROW LEVEL SECURITY`, policy
`tenant_isolation` `USING/WITH CHECK (tenant_id =
current_setting('app.tenant_id', true)::uuid OR current_setting('app.system',
true) = 'on')`, and `SELECT, INSERT, UPDATE, DELETE` (plus the audit
sequence) granted to `bookmark_app` when that role exists. Every request runs
in a transaction opened with `tangra_sdk::db::begin(Scope::tenant(..))`, and
every statement also names `tenant_id = $1`.

## Effective permissions (resolution)

For caller (tenant T, user U, roles R) and bookmark B at time `now`:

1. Roles contain `admin` or `owner` → owner permissions on every bookmark of T.
2. Else union over grants G of T with `G.expires_at IS NULL OR > now` and
   subject matching (`user`=U, `role` ∈ R, `tenant`) where
   `G.resource_id = B` or (`G.resource_id = ''` and `G.subject_type = tenant`).
3. For the collection (no bookmark): only the tenant-wide grants of step 2.

The list query evaluates step 2 as an `EXISTS` sub-select per bookmark and
returns the matching relations so each row carries its permissions.

## Backup document (schema 1)

```json
{
  "schema_version": 1,
  "module": "bookmark",
  "exported_at": "2026-10-16T10:00:00Z",
  "bookmarks": [{"id", "url", "title", "description", "tags", "created_by", "created_at", "updated_at"}],
  "permissions": [{"id", "resource_type", "resource_id", "subject_type", "subject_id", "relation", "granted_by", "granted_at", "expires_at"}]
}
```

Import dedupe keys: bookmark by `id`; grant by (`resource_type`,
`resource_id`, `subject_type`, `subject_id`, `relation`). A grant whose ID is
taken by a different grant gets a new ID.
