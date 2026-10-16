-- OSIRIS — Migration 003: audit zinciri sürümü (Rust geçişi R2, docs/SOZLESME.md §2)
-- Yalnızca ekleme: mevcut kayıtlar chain_version=1 (eski json.dumps kanonik biçimi)
-- olarak işaretlenir; yeni kayıtlar uygulama tarafından 2 (RFC 8785 / JCS) yazılır.
-- Tekrar çalıştırılabilir.

ALTER TABLE audit_logs
    ADD COLUMN IF NOT EXISTS chain_version SMALLINT NOT NULL DEFAULT 1;

DO $$
BEGIN
    ALTER TABLE audit_logs
        ADD CONSTRAINT audit_logs_chain_version_chk CHECK (chain_version IN (1, 2));
EXCEPTION WHEN duplicate_object THEN
    RAISE NOTICE 'constraint exists: %', 'audit_logs_chain_version_chk';
END
$$;
