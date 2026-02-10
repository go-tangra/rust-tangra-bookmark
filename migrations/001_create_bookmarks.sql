CREATE TABLE bookmark_bookmarks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id INTEGER NOT NULL,
    url TEXT NOT NULL,
    title VARCHAR(500) NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    tags TEXT[] NOT NULL DEFAULT '{}',
    created_by INTEGER,
    create_time TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    update_time TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_bookmarks_tenant ON bookmark_bookmarks(tenant_id);
CREATE INDEX idx_bookmarks_tags ON bookmark_bookmarks USING GIN(tags);
