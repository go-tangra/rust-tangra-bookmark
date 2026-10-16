---
title: Operate bookmarks
summary: Running the service, its database, configuration keys, the policy it needs and fixes for common start-up errors.
kind: reference
order: 90
tags: [bookmarks, operations, configuration, database, troubleshooting]
audience: operator
---
The module runs as `bookmarksvc`.

```sh
bookmarksvc -config /app/config.yaml                 # migrate, then serve
bookmarksvc bootstrap -config /app/config.yaml       # apply the migrations and exit
bookmarksvc --healthcheck -config /app/config.yaml   # exit 0 when /readyz answers
bookmarksvc version
```

## Database

PostgreSQL 14 or newer, one database `bookmark` with two roles:

- the **owner** (`db.migrate_dsn`) applies the embedded migrations at every
  start and with `bootstrap`;
- **`bookmark_app`** (`db.dsn`), `LOGIN NOBYPASSRLS`, serves requests.
  Create it before the first start; the migrations grant it the tables.
  Row-level security keeps every request inside its tenant.

In production both DSNs must use `sslmode=verify-full` (or `verify-ca`) with
the server's CA as `sslrootcert=`; the service refuses to start otherwise.

## Configuration

Besides the framework keys (`service_name: bookmark`, `trust_domain`,
`identity`, `authz`, `server`, `admin`, `discovery`, `limits`):

| Key | Meaning | Default |
|---|---|---|
| `gateway.service` | the gateway's service name | `gateway` |
| `gateway.issuer` | platform token issuer (https origin) | required |
| `db.dsn` | application role DSN | required |
| `db.migrate_dsn` | owner DSN for migrations | `db.dsn` |
| `db.max_conns` | pool size | 16 |
| `events.enabled` | publish `bookmark.shared` | false |
| `valkey.*` | event bus (TLS, user `bookmark`) | needed with events |
| `enroll.*` | lcm network enrolment (production identity) | off |
| `limits_bookmark.backup_max_bytes` | largest backup import | 16 MiB |

Raise the framework's `limits.max_request_bytes` to at least
`backup_max_bytes` so backup imports reach the module.

## Policy

Only the gateway may call the module: allow
`spiffe://<trust domain>/svc/gateway` → `bookmark` for `GET *`, `HEAD *`,
`POST *`, `PUT *`. `discovery.static` names `gateway`, `auth` (and `lcm`
with enrolment).

## Troubleshooting

- `config: db.dsn must use sslmode=verify-full (or verify-ca) in production`:
  add `sslmode=verify-full&sslrootcert=/tls/ca.pem` to both DSNs.
- `bookmark: migrate: ... permission denied`: `db.migrate_dsn` must be the
  database owner, not `bookmark_app`.
- `bookmark: database: ...`: the server is unreachable, the certificate does
  not match the host name, or the password is wrong.
- Requests answer `500 internal` and the log shows `permission denied for
  table bookmark_bookmarks`: `bookmark_app` was created after the first
  start, so the migrations could not grant it the tables. Grant them as the
  owner: `GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA
  public TO bookmark_app; GRANT USAGE ON ALL SEQUENCES IN SCHEMA public TO
  bookmark_app;`.
- Imports answer `413 body_too_large`: raise `limits.max_request_bytes` and
  `limits_bookmark.backup_max_bytes`.
