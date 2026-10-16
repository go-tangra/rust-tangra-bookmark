#!/bin/sh
# First start of the bundle's database: the application role. NOBYPASSRLS:
# row-level security backs every tenant predicate. Its password is generated
# by the gateway (hex, so it needs no quoting). The migrations (bookmarksvc
# bootstrap, as the owner) grant it the tables.
set -eu
psql -v ON_ERROR_STOP=1 --username postgres --dbname bookmark <<SQL
CREATE ROLE bookmark_app LOGIN PASSWORD '${BOOKMARK_APP_PASSWORD}' NOBYPASSRLS;
GRANT CONNECT ON DATABASE bookmark TO bookmark_app;
SQL
