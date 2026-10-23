#!/usr/bin/env bash
# livestack (bookmark): run bookmarksvc against the REAL go-tangra v4 core -
# lcm, auth, gateway, with PostgreSQL, Valkey (TLS) and OpenFGA - as native
# processes on 127.0.0.1, and check the module end to end through the
# gateway with real users. The core is the SDK's live stack
# (go-tangra-sdk-rust tools/livestack/livestack.sh), sourced with its module
# hooks; this script adds the bookmark database, build, users and checks.
#
#   tools/livestack/livestack.sh up      build + start the core and seed users
#   tools/livestack/livestack.sh check   run bookmark (file identity, then lcm
#                                        enrolment) and the checks
#   tools/livestack/livestack.sh down    stop every process this run started
#   tools/livestack/livestack.sh all     up + check + down (exit 1 on a FAIL)
#   tools/livestack/livestack.sh clean   down, then delete the work dir
#
# See README.md next to this script.
set -euo pipefail

BM_HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
BM=$(cd "$BM_HERE/../.." && pwd)
: "${SDK_DIR:=$(cd "$BM/.." && pwd)/go-tangra-sdk-rust}"
[ -f "$SDK_DIR/tools/livestack/livestack.sh" ] || { echo "set SDK_DIR to a go-tangra-sdk-rust checkout" >&2; exit 2; }

: "${LS_WORK:=${TMPDIR:-/tmp}/tangra-livestack-bookmark}"
: "${LS_BM_UI:=1}"                 # 1: npm ci + npm run build in ui/
: "${P_BM_GRPC:=19975}" "${P_BM_HTTP:=19976}" "${P_BM_ADMIN:=19790}"
export LS_WORK
# The SDK stack's module hooks: no notes; bookmark gets a file SVID, a
# Valkey user, its route allow-list entry (what a catalogue install grants)
# and its database with the bookmark_app NOBYPASSRLS role (init-db.sh).
LS_NOTES=0
LS_EXTRA_SERVICES=bookmark
LS_EXTRA_ALLOW="spiffe://example.org/svc/bookmark=/api/bookmark;bookmark"
LS_EXTRA_DBS=bookmark
# shellcheck source=/dev/null
source "$SDK_DIR/tools/livestack/livestack.sh"

BMBIN=$BM/target/debug
API=/api/bookmark/v1

# ============================================================================
build_bookmark() {
  log "building bookmark (features ui)"
  if [ "$LS_BM_UI" = 1 ]; then
    [ -n "${NODE_AUTH_TOKEN:-}" ] || die "LS_BM_UI=1 needs NODE_AUTH_TOKEN (read:packages) for @go-tangra/ui"
    (cd "$BM/ui" && npm ci --no-audit --no-fund && npm run build) >"$W/logs/bookmark-ui-build.log" 2>&1 \
      || die "UI build failed (see $W/logs/bookmark-ui-build.log)"
  fi
  [ -f "$BM/ui/dist/mf-manifest.json" ] || die "ui/dist is missing (run with LS_BM_UI=1)"
  (cd "$BM" && "$CARGO" build -q --features ui --bins)
  echo "bookmark $(basename "$BM") HEAD $(git -C "$BM" rev-parse --short HEAD)$(git -C "$BM" diff --quiet HEAD -- src migrations ui/src docs || echo '+dirty')" >>"$W/VERSIONS"
}

bookmark_configs() {
  sed "s/\${TRUST_DOMAIN}/$TD/g" "$BM/deploy/bundle/policy.yaml" >"$W/cfg/bookmark-policy.yaml"
  mkdir -p "$W/bookmark-state" && chmod 700 "$W/bookmark-state"
  for mode in file lcm; do
    {
      echo "service_name: bookmark"
      echo "trust_domain: $TD"
      echo "env: dev"
      if [ $mode = file ]; then
        echo "identity: { provider: file, file: { cert: $W/certs/bookmark.pem, key: $W/certs/bookmark.key, bundle: $W/certs/ca.pem } }"
      else
        echo "identity: { provider: provided }"
        echo "enroll:"
        echo "  enabled: true"
        echo "  enroll_url: $G/api/lcm/v1/enroll"
        echo "  lcm_grpc: 127.0.0.1:$P_LCM_GRPC"
        echo "  tenant_id: \"$MESH_TENANT\""
        echo "  token_file: $W/tokens/bookmark.token"
        echo "  state_file: $W/bookmark-state/svid.json"
      fi
      echo "authz: { source: file, path: $W/cfg/bookmark-policy.yaml }"
      echo "server: { grpc_addr: 127.0.0.1:$P_BM_GRPC, http_addr: 127.0.0.1:$P_BM_HTTP }"
      echo "admin: { addr: 127.0.0.1:$P_BM_ADMIN }"
      echo "discovery: { static: { gateway: [\"127.0.0.1:$P_GW_GRPC\"], auth: [\"127.0.0.1:$P_AUTH_GRPC\"], lcm: [\"127.0.0.1:$P_LCM_GRPC\"] } }"
      echo "limits: { max_request_bytes: 16842752 }"
      echo "limits_bookmark: { backup_max_bytes: 16777216 }"
      echo "db:"
      echo "  dsn: postgres://bookmark_app:dev@127.0.0.1:$P_PG/bookmark?sslmode=disable"
      echo "  migrate_dsn: $PGURL/bookmark?sslmode=disable"
      echo "valkey: { addresses: [\"127.0.0.1:$P_VALKEY\"], username: bookmark, password: dev, ca_file: $W/valkey-tls/ca.crt }"
      echo "events: { enabled: true }"
      echo "gateway: { service: gateway, issuer: $G }"
    } >"$W/cfg/bookmark-$mode.yaml"
  done
}

# Platform-tenant users with the module's member permissions but no tenant
# administration: the built-in role "operator" (the platform tenant has no
# "member"; the bookmark manifest grants operator the member set).
bookmark_users() {
  local roles id r; roles=$(req op GET /api/v1/admin/roles | body)
  id=$(pyget "[r['id'] for r in d if r['slug']=='operator'][0]" <<<"$roles")
  for u in ana boris carol; do
    r=$(req op POST /api/v1/admin/invitations "{\"email\":\"$u@example.org\",\"role_ids\":[\"$id\"]}")
    [ "$(code <<<"$r")" = 202 ] || die "invite $u: $r"
    accept "$u" "$u@example.org"
  done
}

up() {
  [ -e "$W/pgdata" ] && die "$W holds a previous run; run '$0 clean' (or set LS_WORK)"
  build
  build_bookmark
  certs
  configs
  bookmark_configs
  infra
  core
  users
  bookmark_users
  # The schema, as the bundle's bookmark-bootstrap (owner role).
  "$BMBIN/bookmarksvc" bootstrap -config "$W/cfg/bookmark-file.yaml" >"$W/logs/bookmark-bootstrap.log" 2>&1 \
    || die "bookmarksvc bootstrap failed (see $W/logs/bookmark-bootstrap.log)"
  log "core up; work dir $W"
  cat "$W/VERSIONS" >&2
}

# ============================================================================
uid() { sql auth "select id from users where lower(email)=lower('$1')" | pyget "d['id']"; }
jget() { body <<<"$2" | pyget "$1"; }
ids() { body <<<"$1" | pyget "sorted(i['title'] for i in d['items'])"; }

bm_up() { # mode logfile
  start bookmark "$2" env SSL_CERT_FILE="$W/edge/ca.crt" "$BMBIN/bookmarksvc" -config "$W/cfg/bookmark-$1.yaml"
  wait_http "http://127.0.0.1:$P_BM_ADMIN/readyz" "bookmark ($1)" 90
  wait_log "$2" '"msg":"registered with gateway"' "bookmark registration" 60
}

check_registration() {
  local acc=0
  for _ in $(seq 1 20); do
    acc=$(sql gateway "select count(*) n from gateway_audit_events where module='bookmark' and event_type='registration_accepted' and ts >= '$T0'" | pyget "d['n']")
    [ "$acc" -ge 1 ] && break
    sleep 0.5
  done
  [ "$acc" -ge 1 ] && ok "gateway audit: registration_accepted for bookmark ($acc)" || ko "no registration_accepted row"
  local km; km=$(sql gateway "select last_version, manifest_hash, identity from known_modules where module='bookmark'")
  [ -n "$km" ] && ok "known_modules (manifest stored): $km" || ko "bookmark missing from known_modules"
  local st; st=$(req op GET /gateway/v1/ops/registrations | body | pyget "[(i['state'], i['manifest']['routes'], i['manifest']['permissions'], i['build_versions'], i['docs_digests']) for i in d['items'] if i['module']=='bookmark']")
  [[ $st == *active* ]] && ok "ops/registrations: bookmark (state, routes, permissions, build, docs) $st" || ko "ops/registrations: $st"
  local dl; dl=$(grep -h '"msg":"docs: indexed","module":"bookmark"' "$W/logs/gateway.log" | tail -1)
  [ -n "$dl" ] && ok "gateway log: $(cut -c1-200 <<<"$dl")" || ko "no 'docs: indexed' for bookmark"
  local ds; ds=$(sql gateway "select module, page_count, error from module_docs_state where module='bookmark'")
  [[ $ds == *'"page_count":'[1-9]* ]] && ok "module_docs_state: $ds" || ko "module_docs_state: $ds"
  local r hits; r=$(req op GET '/gateway/v1/docs/search?q=share&module=bookmark')
  hits=$(body <<<"$r" | pyget "[(i['module'], i['slug']) for i in d['items']]" 2>/dev/null || echo "?")
  [[ $(code <<<"$r") = 200 && $hits == *bookmark* ]] && ok "docs search 'share' finds bookmark pages: $hits" || ko "docs search: $r"
  local perms; perms=$(sql auth "select string_agg(resource||':'||action, ',' order by resource, action) p from module_permissions where module='bookmark' and retired_at is null" | pyget "d['p']")
  [ "$perms" = "backup:manage,bookmarks:create,bookmarks:delete,bookmarks:read,bookmarks:update,permissions:manage" ] \
    && ok "auth module_permissions: $perms" || ko "auth module_permissions: $perms"
  local roles; roles=$(sql auth "select string_agg(slug, ',' order by slug) s from module_role_defs where module='bookmark'" | pyget "d['s']")
  [ "$roles" = "administrator,viewer" ] && ok "auth module roles: $roles" || ko "auth module roles: $roles"
  local grant; grant=$(sql auth "select string_agg(distinct r.slug||'='||rp.resource||':'||rp.action, ',') g from role_permissions rp join roles r on r.id=rp.role_id where rp.module='bookmark' and rp.tenant_id='$MESH_TENANT' and r.slug in ('owner','auditor','operator')" | pyget "d['g']")
  [[ $grant == *owner=backup:manage* && $grant == *auditor=bookmarks:read* && $grant != *auditor=bookmarks:create* \
     && $grant == *operator=permissions:manage* && $grant != *operator=backup:manage* ]] \
    && ok "built-in grants (platform tenant): $grant" || ko "built-in grants: $grant"
}

check_crud() {
  local r id
  r=$(req op POST $API/bookmarks '{"url":"https://alpha.example.org/","title":"Alpha","description":"first","tags":["ls","docs"]}'); expect "owner creates a bookmark" 201 "$r"
  id=$(jget "d['id']" "$r"); BM_OP=$id
  [ "$(jget "d['permissions']" "$r")" = "{'read': True, 'write': True, 'delete': True, 'share': True}" ] && ok "creator holds every permission" || ko "creator permissions: $r"
  req op POST $API/bookmarks '{"url":"https://bravo.example.org/","title":"Bravo","tags":["ls"]}' >/dev/null
  req op POST $API/bookmarks '{"url":"https://charlie.example.org/","title":"Charlie","tags":["other"]}' >/dev/null
  r=$(req op GET "$API/bookmarks/$id"); expect "owner reads it" 200 "$r"
  r=$(req op PUT "$API/bookmarks/$id" '{"title":"Alpha 2"}'); expect "owner updates it" 200 "$r"
  [ "$(jget "d['title']" "$r")" = "Alpha 2" ] && ok "update stored" || ko "update: $r"
  r=$(req op GET "$API/bookmarks?page=1&page_size=2&sort=title&order=asc")
  [ "$(code <<<"$r")" = 200 ] && [ "$(jget "(len(d['items']), d['total'], d['page_size'], [i['title'] for i in d['items']])" "$r")" = "(2, 3, 2, ['Alpha 2', 'Bravo'])" ] \
    && ok "list paging + sort: $(jget "(d['total'], [i['title'] for i in d['items']])" "$r")" || ko "list paging: $r"
  r=$(req op GET "$API/bookmarks?page=2&page_size=2&sort=title&order=asc")
  [ "$(jget "[i['title'] for i in d['items']]" "$r")" = "['Charlie']" ] && ok "page 2: Charlie" || ko "page 2: $r"
  r=$(req op GET "$API/bookmarks?sort=title&order=desc")
  [ "$(jget "[i['title'] for i in d['items']]" "$r")" = "['Charlie', 'Bravo', 'Alpha 2']" ] && ok "sort title desc" || ko "sort desc: $r"
  r=$(req op GET "$API/bookmarks?q=charlie")
  [ "$(ids "$r")" = "['Charlie']" ] && ok "q=charlie" || ko "q: $r"
  r=$(req op GET "$API/bookmarks?tag=ls")
  [ "$(ids "$r")" = "['Alpha 2', 'Bravo']" ] && ok "tag=ls" || ko "tag: $r"
  r=$(req op GET "$API/bookmarks?tag=ls&tag=docs")
  [ "$(ids "$r")" = "['Alpha 2']" ] && ok "tag=ls&tag=docs (all tags)" || ko "two tags: $r"
  r=$(req op GET "$API/bookmarks?page_size=500"); expect "page_size over 200" 422 "$r"
  r=$(req op POST $API/bookmarks '{"url":"ftp://x.example.org/"}'); expect "non-http URL" 422 "$r"
  r=$(req op GET "$API/bookmarks?q=charlie")
  local cid; cid=$(jget "d['items'][0]['id']" "$r")
  r=$(req op POST "$API/bookmarks/$cid/remove"); expect "owner removes Charlie" 204 "$r"
  r=$(req op GET "$API/bookmarks/$cid"); expect "removed bookmark reads" 404 "$r"
  r=$(curl -s --cacert "$W/edge/ca.crt" -w '\n%{http_code}' "$G$API/bookmarks"); expect "anonymous list" 401 "$r"
  r=$(curl -s --cacert "$W/edge/ca.crt" -w '\n%{http_code}' "$G$API/info"); expect "anonymous /info (public)" 200 "$r"
  r=$(req viewer GET $API/bookmarks); expect "user without bookmark permissions lists" 403 "$r"
  r=$(req auditor GET $API/bookmarks); expect "auditor (bookmarks:read) lists" 200 "$r"
  [ "$(jget "d['total']" "$r")" = 0 ] && ok "auditor sees no unshared bookmark" || ko "auditor list: $r"
  r=$(req auditor POST $API/bookmarks '{"url":"https://no.example.org/"}'); expect "auditor creates" 403 "$r"
  r=$(req auditor POST $API/backup/export ''); expect "auditor exports (no backup:manage)" 403 "$r"
  r=$(req globex POST $API/bookmarks '{"url":"https://globex.example.org/","title":"Globex only"}'); expect "other tenant's owner creates" 201 "$r"
  r=$(req globex GET $API/bookmarks)
  [ "$(ids "$r")" = "['Globex only']" ] && ok "other tenant lists only its own" || ko "tenant isolation (list): $r"
  r=$(req globex GET "$API/bookmarks/$id"); expect "other tenant reads this tenant's bookmark" 404 "$r"
  r=$(req globex PUT "$API/bookmarks/$id" '{"title":"x"}'); expect "other tenant updates it" 404 "$r"
  r=$(req op GET "$API/bookmarks?q=globex")
  [ "$(jget "d['total']" "$r")" = 0 ] && ok "this tenant does not see the other tenant's bookmark" || ko "leak: $r"
  r=$(curl -s --cacert "$W/edge/ca.crt" -w '\n%{http_code}' "$G/m/bookmark/mf-manifest.json"); expect "GET /m/bookmark/mf-manifest.json" 200 "$r"
  local entry; entry=$(body <<<"$r" | pyget "d['metaData']['remoteEntry']['name']")
  r=$(curl -s --cacert "$W/edge/ca.crt" -o /dev/null -w 'x\n%{http_code}' "$G/m/bookmark/$entry"); expect "GET /m/bookmark/$entry" 200 "$r"
}

grant() { # user bookmark-id subject_type subject_id relation [expires_at]
  local e=""; [ -n "${6:-}" ] && e=",\"expires_at\":\"$6\""
  req "$1" POST $API/permissions/grant "{\"resource_id\":\"$2\",\"subject_type\":\"$3\",\"subject_id\":\"$4\",\"relation\":\"$5\"$e}"
}

check_sharing() {
  local r b1 b2 b3 b4 g ana boris
  ana=$(uid ana@example.org); boris=$(uid boris@example.org)
  mk() { req ana POST $API/bookmarks "{\"url\":\"https://$1.example.org/\",\"title\":\"$1\"}" | body | pyget "d['id']"; }
  b1=$(mk share-user); b2=$(mk share-role); b3=$(mk share-tenant); b4=$(mk share-expiry)
  r=$(req boris GET "$API/bookmarks/$b1"); expect "boris reads ana's unshared bookmark" 404 "$r"
  # SSE: boris (the grantee) gets bookmark.shared; ana and globex do not.
  local sb=$W/sse-boris.txt sa=$W/sse-ana.txt sg=$W/sse-globex.txt
  for p in "boris:$sb" "ana:$sa" "globex:$sg"; do
    timeout 8 curl -sN --cacert "$W/edge/ca.crt" -b "$(jar "${p%%:*}")" -H 'Accept: text/event-stream' "$G/gateway/v1/stream" >"${p#*:}" 2>/dev/null &
    echo $! >>"$W/run/sse.pids"
  done
  sleep 2
  r=$(grant ana "$b1" user "$boris" viewer); expect "ana shares with boris (user, viewer)" 201 "$r"
  g=$(jget "d['id']" "$r")
  r=$(grant ana "$b1" user "$boris" viewer); expect "same grant twice" 409 "$r"
  for p in $(cat "$W/run/sse.pids"); do wait "$p" 2>/dev/null || true; done; rm -f "$W/run/sse.pids"
  grep -q '^event: bookmark.shared' "$sb" && grep -q "$b1" "$sb" \
    && ok "SSE: boris received $(grep -A1 '^event: bookmark.shared' "$sb" | tr '\n' ' ' | cut -c1-200)" \
    || ko "SSE: bookmark.shared not seen by boris ($(tr '\n' ' ' <"$sb" | head -c 300))"
  grep -q "$b1" "$sa" && ko "SSE: ana (the sharer) received it" || ok "SSE: the sharer's stream did not receive it"
  grep -q "$b1" "$sg" && ko "SSE: other tenant received it" || ok "SSE: the other tenant's stream did not receive it"
  r=$(req boris GET "$API/bookmarks/$b1"); expect "boris reads the shared bookmark" 200 "$r"
  [ "$(jget "d['permissions']" "$r")" = "{'read': True, 'write': False, 'delete': False, 'share': False}" ] && ok "boris: read only" || ko "boris permissions: $r"
  r=$(req boris PUT "$API/bookmarks/$b1" '{"title":"hijack"}'); expect "boris (viewer) updates it" 403 "$r"
  r=$(req boris POST "$API/bookmarks/$b1/remove"); expect "boris (viewer) removes it" 403 "$r"
  r=$(req boris GET $API/bookmarks)
  [ "$(ids "$r")" = "['share-user']" ] && ok "boris lists only the shared bookmark" || ko "boris list: $r"
  r=$(req boris GET "$API/permissions/effective?resource_id=$b1")
  [ "$(jget "d['relation']" "$r")" = viewer ] && ok "boris effective relation: viewer" || ko "effective: $r"
  r=$(req auditor GET "$API/bookmarks/$b1"); expect "auditor reads the user-shared bookmark" 404 "$r"
  # Role subject: role slug auditor.
  r=$(grant ana "$b2" role auditor viewer); expect "ana shares with role auditor" 201 "$r"
  r=$(req auditor GET "$API/bookmarks/$b2"); expect "auditor (role grant) reads it" 200 "$r"
  r=$(req boris GET "$API/bookmarks/$b2"); expect "boris (not in role) reads it" 404 "$r"
  # Tenant subject on one bookmark: stays on that bookmark.
  r=$(grant ana "$b3" tenant "" viewer); expect "ana shares one bookmark with the tenant" 201 "$r"
  r=$(req boris GET "$API/bookmarks/$b3"); expect "boris reads the tenant-shared bookmark" 200 "$r"
  r=$(req carol GET "$API/bookmarks/$b3"); expect "carol reads the tenant-shared bookmark" 200 "$r"
  r=$(req boris GET "$API/bookmarks/$b4"); expect "tenant grant does not reach another bookmark" 404 "$r"
  r=$(req ana POST $API/permissions/grant '{"resource_id":"","subject_type":"tenant","relation":"viewer"}'); expect "non-admin tenant-wide grant (no bookmark)" 403 "$r"
  # Expiry.
  r=$(grant ana "$b4" user "$boris" viewer "2020-01-01T00:00:00Z"); expect "grant with a past expiry" 422 "$r"
  local exp; exp=$(date -u -d '+6 sec' +%FT%TZ)
  r=$(grant ana "$b4" user "$boris" viewer "$exp"); expect "grant expiring at $exp" 201 "$r"
  r=$(req boris GET "$API/bookmarks/$b4"); expect "boris reads before expiry" 200 "$r"
  sleep 8
  r=$(req boris GET "$API/bookmarks/$b4"); expect "boris reads after expiry" 404 "$r"
  # Readable without share: 403 (contract: readable, missing permission).
  r=$(grant boris "$b1" user "$(uid carol@example.org)" viewer); expect "boris (viewer) re-shares" 403 "$r"
  # Revoke.
  r=$(req ana POST $API/permissions/revoke "{\"resource_id\":\"$b2\",\"id\":\"$g\"}"); expect "revoke naming another bookmark" 404 "$r"
  r=$(req ana POST $API/permissions/revoke "{\"resource_id\":\"$b1\",\"id\":\"$g\"}"); expect "ana revokes boris" 200 "$r"
  r=$(req boris GET "$API/bookmarks/$b1"); expect "boris reads after revoke" 404 "$r"
  r=$(req ana GET "$API/permissions?resource_id=$b1")
  [ "$(jget "[(i['subject_type'], i['relation']) for i in d['items']]" "$r")" = "[('user', 'owner')]" ] && ok "grants of b1 after revoke: owner only" || ko "grants: $r"
  # The tenant owner (admin short-circuit) reads everything in the tenant.
  r=$(req op GET "$API/bookmarks/$b1"); expect "tenant owner reads ana's bookmark" 200 "$r"
}

check_backup() {
  local r doc n
  r=$(req op POST $API/backup/export ''); expect "owner exports" 200 "$r"
  doc=$W/backup.json; body <<<"$r" >"$doc"
  n=$(pyget "(len(d['bookmarks']), len(d['permissions']))" <"$doc")
  ok "backup: (bookmarks, permissions) = $n"
  local imp=$W/import.json
  for mode in skip skip overwrite; do
    python3 -I -c 'import json,sys; print(json.dumps({"mode": sys.argv[1], "backup": json.load(open(sys.argv[2]))}))' "$mode" "$doc" >"$imp"
    r=$(curl -s --cacert "$W/edge/ca.crt" -b "$(jar globex)" -c "$(jar globex)" -X POST -w '\n%{http_code}' \
      -H "Origin: $G" -H "X-CSRF-Token: $(csrf globex)" -H 'Content-Type: application/json' --data-binary @"$imp" "$G$API/backup/import")
    expect "import into the other tenant (mode $mode)" 200 "$r"
  done
  r=$(req globex GET "$API/bookmarks?q=share-user")
  [ "$(jget "d['total']" "$r")" = 1 ] && ok "imported bookmark visible to the other tenant's owner" || ko "after import: $r"
  r=$(req op GET "$API/bookmarks?q=globex")
  [ "$(jget "d['total']" "$r")" = 0 ] && ok "import did not leak into the source tenant" || ko "source after import: $r"
}

check_import() {
  local w=$W/import-v3 r
  mkdir -p "$w" && chmod 700 "$w"
  if [ -z "$(sql postgres "select 1 from pg_database where datname='bookmark_v3'")" ]; then
    sql postgres "CREATE DATABASE bookmark_v3" >/dev/null
    cat "$BM/testdata/v3/001_create_bookmarks.sql" "$BM/testdata/v3/002_create_permissions.sql" "$BM/testdata/v3/seed.sql" \
      | "$BIN/lshelper" "$PGURL/bookmark_v3" >/dev/null
  fi
  ok "v3 database: $(sql bookmark_v3 "select (select count(*) from bookmark_bookmarks) b, (select count(*) from bookmark_permissions) p")"
  "$BIN/lshelper" "$PGURL/auth" "select lower(email) e, id::text i from users where tenant_id='$MESH_TENANT'" \
    | python3 -I -c 'import json,sys; [print(r["e"]+","+r["i"]) for r in map(json.loads, sys.stdin)]' >"$w/users.csv"
  echo "3,$(uid carol@example.org)" >"$w/v3-users.csv"
  cat >"$w/import.yaml" <<EOF
source:
  dsn: { env: BOOKMARK_LEGACY_DSN }
tenants: { "0": "$MESH_TENANT" }
users_csv: $w/users.csv
v3_users_csv: $w/v3-users.csv
roles: { "platform:admin": admin }
platform_actor: ""
exclude_orphans: false
exclude_invalid: false
EOF
  local mig=("$BMBIN/bookmark-migrate" -source-config "$w/import.yaml" -config "$W/cfg/bookmark-file.yaml")
  export BOOKMARK_LEGACY_DSN="$PGURL/bookmark_v3?sslmode=disable"
  local rc=0
  "${mig[@]}" -dry-run >"$w/dry-run-strict.json" 2>"$w/err.log" || rc=$?
  [ $rc = 1 ] && [ "$(pyget "d['status']" <"$w/dry-run-strict.json")" = failed ] \
    && ok "dry run without exclusions fails on the orphan v3 user 9 (exit $rc): $(pyget "d['errors']" <"$w/dry-run-strict.json" | cut -c1-160)" \
    || ko "strict dry run: rc=$rc $(head -c 300 "$w/dry-run-strict.json")"
  rc=0; "${mig[@]}" -exclude-orphans -dry-run >"$w/dry-run.json" 2>>"$w/err.log" || rc=$?
  [ $rc = 0 ] && ok "dry run -exclude-orphans: $(pyget "(d['status'], d['plan'], len(d['excluded']))" <"$w/dry-run.json")" || ko "dry run: rc=$rc $(head -c 400 "$w/dry-run.json")"
  [ "$(sql bookmark "select count(*) n from bookmark_bookmarks where id='0190a000-0000-7000-8000-000000000001'" | pyget "d['n']")" = 0 ] && ok "dry run wrote nothing" || ko "dry run wrote rows"
  rc=0; "${mig[@]}" -exclude-orphans -apply >"$w/apply.json" 2>>"$w/err.log" || rc=$?
  [ $rc = 0 ] && [ "$(pyget "(d['status'], d['reconciled'])" <"$w/apply.json")" = "('succeeded', True)" ] \
    && ok "apply: $(pyget "(d['status'], d['reconciled'], d['destination'])" <"$w/apply.json")" || ko "apply: rc=$rc $(head -c 400 "$w/apply.json")"
  rc=0; "${mig[@]}" -exclude-orphans -apply >"$w/apply2.json" 2>>"$w/err.log" || rc=$?
  [ $rc = 0 ] && [ "$(pyget "d['status']" <"$w/apply2.json")" = already_applied ] && ok "re-run: already_applied" || ko "re-run: rc=$rc $(head -c 300 "$w/apply2.json")"
  unset BOOKMARK_LEGACY_DSN
  # Through the gateway, as the mapped v4 users.
  r=$(req ana GET "$API/bookmarks/0190a000-0000-7000-8000-000000000001"); expect "ana reads imported Runbook (owner)" 200 "$r"
  [ "$(jget "(d['created_by'], d['tags'], d['permissions']['share'])" "$r")" = "('$(uid ana@example.org)', ['ops', 'oncall'], True)" ] \
    && ok "Runbook: created_by = ana's v4 UUID, tags kept, ana may share" || ko "Runbook: $r"
  r=$(req ana GET "$API/bookmarks/0190a000-0000-7000-8000-000000000002"); expect "ana reads imported Dashboards (owner)" 200 "$r"
  r=$(req boris GET "$API/bookmarks/0190a000-0000-7000-8000-000000000003"); expect "boris reads imported Wiki (owner)" 200 "$r"
  [ "$(jget "d['permissions']['write']" "$r")" = True ] && ok "boris owns Wiki" || ko "Wiki: $r"
  r=$(req boris GET "$API/bookmarks/0190a000-0000-7000-8000-000000000001"); expect "boris reads Runbook (imported tenant viewer)" 200 "$r"
  [ "$(jget "d['permissions']['write']" "$r")" = False ] && ok "boris: Runbook read only" || ko "Runbook for boris: $r"
  r=$(req auditor GET "$API/bookmarks/0190a000-0000-7000-8000-000000000001"); expect "auditor reads Runbook (tenant viewer)" 200 "$r"
  r=$(req boris GET "$API/bookmarks/0190a000-0000-7000-8000-000000000002"); expect "boris reads Dashboards (not shared)" 404 "$r"
  r=$(req carol GET "$API/bookmarks/0190a000-0000-7000-8000-000000000004"); expect "carol (v3-users.csv) reads Carol's" 200 "$r"
  r=$(req ana GET "$API/permissions?resource_id=0190a000-0000-7000-8000-000000000002")
  [ "$(jget "sorted((i['subject_type'], i['subject_id'], i['relation'], bool(i.get('expires_at'))) for i in d['items'])" "$r")" = \
    "[('role', 'admin', 'editor', True), ('user', '$(uid ana@example.org)', 'owner', False)]" ] \
    && ok "Dashboards grants: owner ana + expired role admin editor" || ko "Dashboards grants: $r"
  r=$(req ana GET "$API/bookmarks/0190a000-0000-7000-8000-000000000003"); expect "ana reads Wiki (not shared with her)" 404 "$r"
}

check_deregistration() {
  local t1 w=""; t1=$(date -u +%FT%T.%NZ)
  stop bookmark
  for _ in $(seq 1 20); do
    w=$(sql gateway "select ts, reason from gateway_audit_events where module='bookmark' and event_type='registration_withdrawn' and ts >= '$t1'")
    [ -n "$w" ] && break
    sleep 0.5
  done
  [ -n "$w" ] && ok "gateway audit: registration_withdrawn $w" || ko "no registration_withdrawn after shutdown"
  local regs; regs=$(req op GET /gateway/v1/ops/registrations | body | pyget "[i['module'] for i in d['items']]")
  [[ $regs != *bookmark* ]] && ok "ops/registrations after shutdown: $regs" || ko "bookmark still registered: $regs"
}

check() {
  [ -e "$W/run/gateway.pid" ] || die "not up; run up first"
  echo "== A. bookmark with identity.provider: file"
  T0=$(date -u +%FT%T.%NZ)
  bm_up file "$W/logs/bookmark-file.log"
  check_registration
  check_crud
  check_sharing
  check_backup
  check_import
  check_deregistration

  echo "== B. bookmark with lcm enrolment (token minted through the gateway)"
  rm -f "$W/bookmark-state/svid.json"
  T0=$(date -u +%FT%T.%NZ)
  local r; r=$(req op POST /gateway/v1/ops/enrollment-tokens '{"services":["bookmark"],"ttl_seconds":600}')
  if [ "$(code <<<"$r")" = 201 ]; then
    (umask 077; body <<<"$r" | pyget "d['token']" | tr -d '\n' >"$W/tokens/bookmark.token")
    ok "enrolment token minted for $(body <<<"$r" | pyget "d['spiffe_ids']") (not printed)"
  else
    ko "mint enrolment token: $r"
  fi
  bm_up lcm "$W/logs/bookmark-lcm.log"
  local idl; idl=$(grep -h '"msg":"identity ready"' "$W/logs/bookmark-lcm.log" | tail -1)
  [[ $idl == *spiffe://$TD/svc/bookmark* ]] && ok "enrolled over HTTPS: $(cut -c1-230 <<<"$idl")" || ko "identity: $idl"
  local perm; perm=$(stat -c %a "$W/bookmark-state/svid.json")
  [ "$perm" = 600 ] && ok "state file written, mode $perm" || ko "state file mode $perm"
  check_registration
  r=$(req ana GET "$API/bookmarks/0190a000-0000-7000-8000-000000000001"); expect "API through the gateway (enrolled identity)" 200 "$r"
  r=$(req viewer GET $API/bookmarks); expect "user without bookmark permissions (enrolled identity)" 403 "$r"
  check_deregistration
  echo
  [ "$FAILS" = 0 ] && echo "ALL CHECKS PASSED" || echo "$FAILS CHECK(S) FAILED"
  [ "$FAILS" = 0 ]
}

# Sourced (for example to run one check by hand): define, run nothing.
[ "${BASH_SOURCE[0]}" = "$0" ] || return 0

case "${1:-}" in
  up) up ;;
  check) check ;;
  down) down ;;
  clean) clean ;;
  all)
    trap down EXIT
    up
    check
    ;;
  *) echo "usage: $0 up|check|down|all|clean" >&2; exit 2 ;;
esac
