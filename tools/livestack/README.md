# livestack: bookmark against the real core

`livestack.sh` runs `bookmarksvc` against the real go-tangra v4 core: lcm,
auth and the gateway, with PostgreSQL, Valkey (TLS) and OpenFGA. Everything
runs as native processes on `127.0.0.1`, with no Docker and no root. It then
checks the module end to end through the gateway, with real users.

The core comes from the SDK's live stack
(`go-tangra-sdk-rust/tools/livestack`). This script sources it with the
stack's module hooks:

- `LS_NOTES=0`: the SDK example is not built;
- `LS_EXTRA_SERVICES=bookmark`: a file SVID from `lcmsvc bootstrap` and a
  Valkey user `bookmark`;
- `LS_EXTRA_ALLOW`: `spiffe://example.org/svc/bookmark=/api/bookmark;bookmark`,
  which is what a catalogue install grants;
- `LS_EXTRA_DBS=bookmark`: the database with the `bookmark_app`
  `NOBYPASSRLS` role, as `deploy/bundle/init-db.sh` creates it.

On top of the SDK stack, the script builds the UI and `--features ui`, runs
`bookmarksvc bootstrap` (the migrations, as the database owner), adds three
more users and runs the bookmark checks. Read the SDK's README for the
platform setup, its parameters (`LS_CACHE`, `PORTAL_REF`, ports and so on)
and the known platform behaviour.

## Run

```sh
export PATH=$HOME/.cargo/bin:$PATH
export NODE_AUTH_TOKEN=$(gh auth token)      # read:packages, for @go-tangra/ui
VALKEY_SERVER=/path/to/valkey-server tools/livestack/livestack.sh all
```

The commands are `up`, `check`, `down`, `all` and `clean`, as in the SDK
script. `all` runs `down` even after a failure. A run takes about 3 minutes
once the core binaries are cached in `$LS_CACHE`.

| Variable | Default |
|---|---|
| `SDK_DIR` | `../go-tangra-sdk-rust` next to this repository |
| `LS_WORK` | `$TMPDIR/tangra-livestack-bookmark` |
| `LS_BM_UI` | `1`: `npm ci && npm run build` in `ui/`. With `0`, an existing `ui/dist` is used |
| `P_BM_GRPC`, `P_BM_HTTP`, `P_BM_ADMIN` | `19975`, `19976`, `19790` |

To run one check by hand against a stack that is up, source the script and
call the function:

```sh
bash -c 'source tools/livestack/livestack.sh; T0=$(date -u +%FT%T.%NZ); bm_up file $W/logs/bm.log; check_crud; stop bookmark'
```

## Users

All users sign in through the gateway's real invitation flow.

| User | Tenant | Roles | Bookmark permissions |
|---|---|---|---|
| `op` | platform | `owner`, `operator` | every permission; the tenant owner passes every per-bookmark check |
| `ana`, `boris`, `carol` | platform | `operator` | read, create, update, delete and `permissions:manage` (the member set). They are not tenant administrators |
| `auditor` | platform | `auditor` | `bookmarks:read` |
| `viewer` | platform | `m.lcm.viewer` | none |
| `globex` | Globex | `owner` | every permission |

The platform tenant has no `member` role. Its `operator` role gets the member
set from the bookmark manifest's built-in grants.

## What `check` proves

Phase A runs with `identity.provider: file`.

- **Registration:**
  - a `registration_accepted` audit row;
  - the manifest stored in `known_modules`;
  - `ops/registrations` shows bookmark `active`, with its routes, build version
    and docs digest.
- **Help docs:**
  - the gateway logs `docs: indexed` with the page count;
  - a `module_docs_state` row exists;
  - docs search for `share` finds the bookmark pages.
- **Auth registration:** the six permissions, the module roles
  `administrator` and `viewer`, and the built-in grants in the platform
  tenant (owner, auditor and operator).
- **CRUD and list:**
  - create, read, update and remove;
  - paging, `sort`/`order`, `q`, and `tag` (one and two tags);
  - `422` for an oversized `page_size` and for a non-http URL;
  - a removed bookmark returns `404`.
- **Access:**
  - an anonymous call returns `401`, and `/info` is public;
  - a user without bookmark permissions gets `403`;
  - the auditor can list but sees nothing unshared, and gets `403` on create
    and on export.
- **Tenant isolation:** the other tenant lists only its own bookmarks, and
  gets `404` on a read or update by ID.
- **Sharing (ana shares her bookmarks):**
  - a user grant: boris reads the bookmark. His permissions are read-only;
    update, remove and re-share return `403`, and his effective relation is
    `viewer`;
  - a duplicate grant returns `409`;
  - a role grant (`auditor`) reaches only that role;
  - a tenant grant on one bookmark stays on that bookmark;
  - a tenant-wide grant by a non-administrator returns `403`;
  - a past expiry returns `422`; a 6-second grant works, then stops;
  - a revoke that names another bookmark returns `404`; a real revoke
    removes the access;
  - the tenant owner reads ana's bookmark.
- **Events:** `bookmark.shared` reaches the grantee's
  `/gateway/v1/stream`. It does not reach the sharer's stream or the other
  tenant's.
- **Backup:** the owner exports the platform tenant. Globex imports the
  backup three times: with `skip` (everything imported), `skip` again
  (everything skipped) and `overwrite`. Nothing leaks back.
- **UI:** `/m/bookmark/mf-manifest.json` and `remoteEntry.js` are served
  through the gateway.
- **v3 import:**
  1. A `bookmark_v3` database is built from `testdata/v3`.
  2. `users.csv` is built from the users of auth's platform tenant, and
     `v3-users.csv` maps carol.
  3. A strict dry run fails on the orphan user `gone` (exit 1).
     `-exclude-orphans -dry-run` succeeds and writes nothing. `-apply`
     reconciles, and a second `-apply` reports `already_applied`.
  4. Through the gateway: ana owns Runbook and Dashboards (`created_by` is
     her v4 UUID); boris owns Wiki; the imported tenant viewer share lets
     boris and the auditor read Runbook; carol reads hers. The expired role
     share is listed but confers nothing.
- **Deregistration:** shutdown writes `registration_withdrawn`, and bookmark
  leaves `ops/registrations`.

Phase B runs with lcm enrolment:

1. The token is minted with `POST /gateway/v1/ops/enrollment-tokens`.
2. The module enrols through the gateway (`/api/lcm/v1/enroll`) as
   `spiffe://example.org/svc/bookmark`, and the state file has mode 0600.
3. The registration checks run again, and so do API calls through the
   gateway: an imported read (`200`) and a user without permissions (`403`).
4. The deregistration check runs again.
