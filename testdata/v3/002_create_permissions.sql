CREATE TABLE bookmark_permissions (
    id SERIAL PRIMARY KEY,
    tenant_id INTEGER NOT NULL,
    resource_type VARCHAR(50) NOT NULL,
    resource_id VARCHAR(36) NOT NULL,
    relation VARCHAR(50) NOT NULL,
    subject_type VARCHAR(50) NOT NULL,
    subject_id VARCHAR(36) NOT NULL,
    granted_by INTEGER,
    expires_at TIMESTAMPTZ,
    create_time TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(tenant_id, resource_type, resource_id, relation, subject_type, subject_id)
);

CREATE INDEX idx_perms_resource ON bookmark_permissions(tenant_id, resource_type, resource_id);
CREATE INDEX idx_perms_subject ON bookmark_permissions(subject_type, subject_id);
CREATE INDEX idx_perms_tenant ON bookmark_permissions(tenant_id);
CREATE INDEX idx_perms_expires ON bookmark_permissions(expires_at);
