# Contract: Mesh policy and platform wiring

Trust domain placeholder `example.org` (dev) / the install's `TRUST_DOMAIN`.

## bookmark `policy.yaml` (callee rules)

```yaml
version: bookmark-1
rules:
  - id: gateway-forwards
    from: ["spiffe://example.org/svc/gateway"]
    to: ["bookmark"]
    operations: ["GET *", "HEAD *", "POST *", "PUT *"]
    effect: allow
```

Only the gateway reaches the module: the API (routes in the manifest), the
help bundle (`GET /docs/v1/bundle`, additionally restricted to peer
`gateway`) and the UI remote (`GET /ui/*`). The API uses no `DELETE`
(removal is `POST .../remove`), matching the rule. Bookmark exposes no gRPC
API (decision 3).

## Outbound

| to | calls | why |
|---|---|---|
| auth | `auth.v1.Keys/List`, `auth.v1.Sessions/RevokedSince` | token verification |
| auth | `auth.v1.Authorization/RegisterPermissions` | permissions, module roles, built-in grants |
| gateway | `gateway.v1.Registry/Register|Renew|Deregister` | lease with manifest and docs digest |
| lcm | enrolment (HTTP join token), `lcm.v1.Enrollment/Enroll` | SVID (production) |
| Valkey | `XADD platform:events:<tenant>` | `bookmark.shared` (optional) |
| PostgreSQL | app role / owner role | storage, migrations |

`discovery.static` names `gateway`, `auth` (and `lcm` with enrolment).

## Gateway admission

Catalogue grant: prefix `/api/bookmark`, `/m/bookmark`, `/bookmark`; name
`bookmark`; SPIFFE ID `spiffe://<td>/svc/bookmark`.

## Ports (bundle, later phase)

gRPC 9975 (unused but bound by the runtime only when configured), HTTP
9976, admin 127.0.0.1:9790. Final numbers are fixed with the bundle
(tasks.md Phase 9).
