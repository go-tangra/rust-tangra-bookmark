# Contract: Audit vocabulary and realtime events

## Business audit (`bookmark_audit_events`)

Closed vocabulary; written in the same transaction as the change (refusals in
their own transaction). Details carry IDs, codes and counts only — never
URLs, titles, descriptions or tags (SR-005).

| event_type | subject_kind / subject_id | outcome | details |
|---|---|---|---|
| bookmark_created | bookmark / id | ok | `{tags: <count>}` |
| bookmark_updated | bookmark / id | ok | `{fields: [url, title, description, tags]}` (names only) |
| bookmark_deleted | bookmark / id | ok | `{grants: <count removed>}` |
| permission_granted | permission / grant id | ok | `{resource_id, subject_type, subject_id, relation, expires: bool}` |
| permission_revoked | permission / grant id | ok | `{resource_id, relation}` |
| access_refused | bookmark / id | refused | reason `forbidden`; `{action}` |
| backup_exported | backup / '' | ok | `{bookmarks, permissions}` |
| backup_imported | backup / '' | ok | ImportResult counts, `{mode}` |

Actor: the platform user ID. Correlation: the request's `x-request-id`.
Masked refusals (`404` for a bookmark the caller cannot read) are not
recorded: they look exactly like an absent bookmark. `403` refusals are
(`access_refused`).

The framework's security audit (authn/authz refusals, identity, policy)
goes to the log as in every SDK module.

## Realtime events (Valkey `platform:events:<tenant>`, optional)

Published only with `events.enabled: true` and a `valkey` section.

| type | to | data |
|---|---|---|
| `bookmark.shared` | the grantee user (subject_type `user`) | `{"bookmark_id": "<uuid>", "relation": "viewer"}` |

No event for role or tenant grants (the audience is unbounded) and no
content. A failed publish is logged; the grant stays.
