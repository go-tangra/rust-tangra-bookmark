-- Per-tenant row-level security (paperless/deployer 0003_rls.sql). The
-- application role bookmark_app is NOBYPASSRLS; every request runs in a
-- transaction with app.tenant_id set to the caller's tenant
-- (tangra_sdk::db::begin). The grants are applied when the role exists (the
-- bundle's init-db.sh creates it before the first start).
DO $$
DECLARE t text;
BEGIN
  FOREACH t IN ARRAY ARRAY['bookmark_bookmarks', 'bookmark_permissions', 'bookmark_audit_events']
  LOOP
    EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
    EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
    EXECUTE format($p$CREATE POLICY tenant_isolation ON %I USING (tenant_id = current_setting('app.tenant_id', true)::uuid OR current_setting('app.system', true) = 'on') WITH CHECK (tenant_id = current_setting('app.tenant_id', true)::uuid OR current_setting('app.system', true) = 'on')$p$, t);
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'bookmark_app') THEN
      EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON %I TO bookmark_app', t);
    END IF;
  END LOOP;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'bookmark_app') THEN
    GRANT USAGE ON SEQUENCE bookmark_audit_events_id_seq TO bookmark_app;
  END IF;
END $$;
