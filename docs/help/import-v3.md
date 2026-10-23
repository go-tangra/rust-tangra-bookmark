---
title: Import v3 bookmarks
summary: Carry the bookmarks and shares of the v3 bookmark service into v4 with bookmark-migrate, from a read-only snapshot, after a dry run.
kind: guide
order: 95
tags: [bookmarks, operations, migration, v3, import]
audience: operator
---
`bookmark-migrate` (in the module image) copies the two v3 tables,
`bookmark_bookmarks` and `bookmark_permissions`, into the v4 schema. It
reads the source in one read-only snapshot and writes the destination in
one transaction. A dry run comes first and writes nothing. Running it again
changes nothing.

```sh
bookmark-migrate -source-config /import/import.yaml -config /app/config.yaml -dry-run
bookmark-migrate -source-config /import/import.yaml -config /app/config.yaml -apply
```

The report (JSON) goes to standard output. The exit status is:

- `0` for `succeeded` or `already_applied`;
- `1` for `failed`;
- `2` for a usage error.

## 1. Snapshot the v3 tables

Do not point the tool at the live v3 database. Dump the bookmark tables and
the v3 users table, which gives each v3 user ID its e-mail, on the v3 host:

```sh
pg_dump -U postgres -d gwa --no-owner --no-privileges \
  -t bookmark_bookmarks -t bookmark_permissions -t sys_users > bookmark-v3.sql
```

Restore the dump into a database of its own that the bundle can reach, for
example in the bundle's own server:

```sh
docker compose exec -T bookmark-db createdb -U postgres bookmark_v3
docker compose exec -T bookmark-db psql -U postgres -d bookmark_v3 < bookmark-v3.sql
```

## 2. Mapping files

Put these files in one directory (`$W` below). It is mounted read-only as
`/import`.

- **`users.csv`**: `email,uuid` lines for the v4 users of the target
  tenant, taken from auth:

  ```sh
  psql -U postgres -d auth -At -c "\copy (SELECT lower(email), id FROM users WHERE tenant_id = '$T4') TO STDOUT CSV" > "$W/users.csv"
  ```

  A v3 user ID reaches its v4 account through its e-mail in `sys_users`.
- **`v3-users.csv`** (optional): `v3id,uuid` lines. Use it for v3 users
  without an e-mail, or when `sys_users` is not in the snapshot. A line here
  wins over the e-mail route.
- **`import.yaml`**:

  ```yaml
  source:
    dsn: { env: BOOKMARK_LEGACY_DSN }   # or { file: /import/dsn }
  tenants: { "0": "00000000-0000-0000-0000-000000000001" }
  users_csv: /import/users.csv
  v3_users_csv: /import/v3-users.csv
  roles: { "platform:admin": admin, "tenant:manager": admin }
  platform_actor: ""
  exclude_orphans: false
  exclude_invalid: false
  ```

The source DSN never appears on the command line.

## Mapping rules

| v3 | v4 |
|---|---|
| tenant `0` (platform) | `00000000-0000-0000-0000-000000000001`, unless `tenants` or `-tenant 0=<uuid>` says otherwise |
| other tenant IDs | only through `tenants` / `-tenant 5=<uuid>,...` |
| bookmark ID | kept |
| `create_time`, `update_time` | `created_at`, `updated_at` |
| `created_by`, `granted_by` (user ID) | the v4 user; NULL or 0 becomes `platform_actor` |
| `RELATION_OWNER`/`EDITOR`/`SHARER`/`VIEWER` | `owner`/`editor`/`sharer`/`viewer` |
| `SUBJECT_TYPE_USER` + user ID | `user` + v4 user UUID |
| `SUBJECT_TYPE_ROLE` + role code | `role` + the slug from `roles` / `-roles code=slug` |
| `SUBJECT_TYPE_TENANT` + `all` | `tenant` on the same bookmark |
| permission ID (serial) | a stable UUID derived from tenant and ID |
| `expires_at` | kept; an expired share is imported but confers nothing |

Command-line flags override the file:

- `-tenant`, `-roles` and `-platform-actor` replace the matching keys;
- `-users` and `-v3-users` replace the CSV paths;
- `-exclude-orphans` and `-exclude-invalid` switch on the matching
  exclusion.

## 3. Dry run

From the bundle directory:

```sh
docker compose run --rm --no-deps -v "$W":/import:ro \
  -e BOOKMARK_LEGACY_DSN='postgres://postgres:<password>@bookmark-db:5432/bookmark_v3?sslmode=verify-full&sslrootcert=/tls/ca.pem' \
  --entrypoint bookmark-migrate bookmark \
  -source-config /import/import.yaml -config /app/config.yaml -dry-run | tee dry-run.json
```

In the report:

- `plan`: per entity, `insert` (rows to write), `unchanged` (already
  there) and `conflict` (there with other content).
- `errors`: rows that stop the run. Two kinds:
  - **orphans**: an unmapped tenant, user or role, or a share of a bookmark
    that does not exist.
  - **invalid rows**: rows that v4 refuses, such as a non-http(s) URL,
    more than 20 tags, or white space that v4 would trim.
- `excluded`: rows left out because of `-exclude-orphans` or
  `-exclude-invalid`. Shares of a bookmark that was left out are left out
  too.
- `author_fallbacks`: authors recorded as `platform_actor`. These are rows
  without an author, plus unmapped authors under `-exclude-orphans`.
- `merged_duplicates`: v3 shares that became the same v4 share, for example
  when two role codes map to one slug.
- `expired_grants`, `unsupported_columns` and `destination` (the row
  counts of the imported tenants).

Rows are never changed silently. Fix the mapping files and run again, or
accept the listed losses with `-exclude-orphans` / `-exclude-invalid`.

## 4. Apply

Run the same command with `-apply` in place of `-dry-run`. In one
transaction, the tool:

1. writes the missing rows;
2. compares every row again;
3. commits only when all of them match (`reconciled: true`).

With any error or conflict it writes nothing. A second `-apply` reports
`already_applied`. Afterwards, drop the snapshot database
(`dropdb bookmark_v3`).

The destination is `db.migrate_dsn` of `config.yaml` (the database owner).
The tool works across tenants, so it runs in the system scope of row-level
security.
