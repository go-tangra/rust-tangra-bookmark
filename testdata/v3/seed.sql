-- The v3 data in the shape of production (4 bookmarks, 7 grants in `gwa`),
-- over the v3 migrations (001, 002 copied verbatim from v3 20ed5c4) and the
-- v3 admin-service users table (subset of go-tangra-portal
-- app/admin/service/internal/data/ent/schema/user.go: sys_users).
CREATE TABLE sys_users (
    id         BIGSERIAL PRIMARY KEY,
    tenant_id  BIGINT,
    username   VARCHAR(255),
    realname   VARCHAR(255),
    email      VARCHAR(320),
    status     VARCHAR(16) DEFAULT 'NORMAL',
    created_at TIMESTAMPTZ DEFAULT now()
);
INSERT INTO sys_users (id, tenant_id, username, email) VALUES
    (1, 0, 'ana', 'Ana@Example.org'),   -- e-mail case differs from the users file
    (2, 0, 'boris', 'boris@example.org'),
    (3, 0, 'carol', NULL),              -- mapped by the v3 users file only
    (9, 0, 'gone', 'gone@example.org'); -- no v4 account (orphan)

INSERT INTO bookmark_bookmarks (id, tenant_id, url, title, description, tags, created_by, create_time, update_time) VALUES
    ('0190a000-0000-7000-8000-000000000001', 0, 'https://docs.example.org/runbook', 'Runbook', 'On-call runbook', '{ops,oncall}', 1, '2025-03-01 10:00:00+00', '2025-03-02 11:00:00+00'),
    ('0190a000-0000-7000-8000-000000000002', 0, 'https://grafana.example.org/d/abc', 'Dashboards', '', '{ops}', 1, '2025-03-03 10:00:00.123456+00', '2025-03-03 10:00:00.123456+00'),
    ('0190a000-0000-7000-8000-000000000003', 0, 'http://wiki.example.org/page?id=7', 'Wiki', E'two\nlines', '{}', 2, '2025-04-01 09:00:00+00', '2025-04-05 09:00:00+00'),
    ('0190a000-0000-7000-8000-000000000004', 0, 'https://carol.example.org/', 'Carol''s', '', '{personal}', 3, '2025-05-01 08:00:00+00', '2025-05-01 08:00:00+00');

INSERT INTO bookmark_permissions (id, tenant_id, resource_type, resource_id, relation, subject_type, subject_id, granted_by, expires_at, create_time) VALUES
    (1, 0, 'RESOURCE_TYPE_BOOKMARK', '0190a000-0000-7000-8000-000000000001', 'RELATION_OWNER',  'SUBJECT_TYPE_USER',   '1', 1, NULL, '2025-03-01 10:00:00+00'),
    (2, 0, 'RESOURCE_TYPE_BOOKMARK', '0190a000-0000-7000-8000-000000000002', 'RELATION_OWNER',  'SUBJECT_TYPE_USER',   '1', 1, NULL, '2025-03-03 10:00:00.123456+00'),
    (3, 0, 'RESOURCE_TYPE_BOOKMARK', '0190a000-0000-7000-8000-000000000003', 'RELATION_OWNER',  'SUBJECT_TYPE_USER',   '2', 2, NULL, '2025-04-01 09:00:00+00'),
    (4, 0, 'RESOURCE_TYPE_BOOKMARK', '0190a000-0000-7000-8000-000000000004', 'RELATION_OWNER',  'SUBJECT_TYPE_USER',   '3', 3, NULL, '2025-05-01 08:00:00+00'),
    (5, 0, 'RESOURCE_TYPE_BOOKMARK', '0190a000-0000-7000-8000-000000000001', 'RELATION_VIEWER', 'SUBJECT_TYPE_TENANT', 'all', 1, NULL, '2025-03-01 10:05:00+00'),
    (6, 0, 'RESOURCE_TYPE_BOOKMARK', '0190a000-0000-7000-8000-000000000002', 'RELATION_EDITOR', 'SUBJECT_TYPE_ROLE',   'platform:admin', 1, '2025-06-01 00:00:00+00', '2025-03-04 10:00:00+00'),
    (7, 0, 'RESOURCE_TYPE_BOOKMARK', '0190a000-0000-7000-8000-000000000003', 'RELATION_SHARER', 'SUBJECT_TYPE_USER',   '9', 2, NULL, '2025-04-02 09:00:00+00');
SELECT setval('bookmark_permissions_id_seq', 7);
