-- Bookmark v4 schema (specs/038-bookmark-v4/data-model.md). Tenant and
-- bookmark IDs are UUIDs; user IDs and role slugs are text as auth issues
-- them. Primary keys are per tenant so a backup can move between tenants
-- with its IDs.

CREATE TABLE bookmark_bookmarks (
  tenant_id   uuid        NOT NULL,
  id          uuid        NOT NULL,
  url         text        NOT NULL CHECK (char_length(url) BETWEEN 1 AND 2048),
  title       text        NOT NULL DEFAULT '' CHECK (char_length(title) <= 500),
  description text        NOT NULL DEFAULT '' CHECK (char_length(description) <= 5000),
  tags        text[]      NOT NULL DEFAULT '{}' CHECK (cardinality(tags) <= 20),
  created_by  text        NOT NULL DEFAULT '',
  created_at  timestamptz NOT NULL DEFAULT now(),
  updated_at  timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant_id, id)
);
CREATE INDEX bookmarks_created ON bookmark_bookmarks (tenant_id, created_at DESC, id DESC);
CREATE INDEX bookmarks_updated ON bookmark_bookmarks (tenant_id, updated_at DESC, id DESC);
CREATE INDEX bookmarks_title ON bookmark_bookmarks (tenant_id, lower(title), id);
CREATE INDEX bookmarks_tags ON bookmark_bookmarks USING GIN (tags);

-- Grants (permission tuples). resource_id '' = every bookmark of the tenant,
-- only for the tenant subject. No foreign key: removing a bookmark deletes
-- its grants in the same transaction.
CREATE TABLE bookmark_permissions (
  tenant_id     uuid        NOT NULL,
  id            uuid        NOT NULL,
  resource_type text        NOT NULL DEFAULT 'bookmark' CHECK (resource_type = 'bookmark'),
  resource_id   text        NOT NULL,
  relation      text        NOT NULL CHECK (relation IN ('owner', 'editor', 'sharer', 'viewer')),
  subject_type  text        NOT NULL CHECK (subject_type IN ('user', 'role', 'tenant')),
  subject_id    text        NOT NULL DEFAULT '',
  granted_by    text        NOT NULL DEFAULT '',
  granted_at    timestamptz NOT NULL DEFAULT now(),
  expires_at    timestamptz,
  PRIMARY KEY (tenant_id, id),
  UNIQUE (tenant_id, resource_type, resource_id, relation, subject_type, subject_id),
  CHECK (resource_id <> '' OR subject_type = 'tenant'),
  CHECK ((subject_type = 'tenant') = (subject_id = ''))
);
CREATE INDEX permissions_resource ON bookmark_permissions (tenant_id, resource_type, resource_id);
CREATE INDEX permissions_subject ON bookmark_permissions (tenant_id, subject_type, subject_id);

-- Business audit (contracts/audit-events.md): IDs and codes only.
CREATE TABLE bookmark_audit_events (
  id             bigserial   PRIMARY KEY,
  ts             timestamptz NOT NULL DEFAULT now(),
  tenant_id      uuid        NOT NULL,
  event_type     text        NOT NULL,
  actor_id       text        NOT NULL DEFAULT '',
  subject_kind   text        NOT NULL DEFAULT '',
  subject_id     text        NOT NULL DEFAULT '',
  outcome        text        NOT NULL CHECK (outcome IN ('ok', 'refused')),
  reason         text        NOT NULL DEFAULT '',
  correlation_id text        NOT NULL DEFAULT '',
  details        jsonb
);
CREATE INDEX audit_tenant_ts ON bookmark_audit_events (tenant_id, ts DESC);
