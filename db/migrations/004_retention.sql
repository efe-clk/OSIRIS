-- OSIRIS — Migration 004: veri saklama süresi (KVKK, Rust geçişi R2 / K7)
-- Yalnızca ekleme. Anlam (docs/SOZLESME.md §6):
--   sources.retention_days NULL → OSIRIS_RETENTION_DAYS (varsayılan 180)
--                          0    → bu kaynağın öğeleri süreyle silinmez
--                          N>0  → N günden eski öğeler silinir
-- Kaynaksız (source_id NULL) öğelere varsayılan süre uygulanır.
-- Tekrar çalıştırılabilir.

ALTER TABLE sources
    ADD COLUMN IF NOT EXISTS retention_days INTEGER;

DO $$
BEGIN
    ALTER TABLE sources
        ADD CONSTRAINT sources_retention_days_chk CHECK (retention_days IS NULL OR retention_days BETWEEN 0 AND 36500);
EXCEPTION WHEN duplicate_object THEN
    RAISE NOTICE 'constraint exists: %', 'sources_retention_days_chk';
END
$$;

-- Temizlik sorgusu: kaynak + toplama zamanına göre tarar
CREATE INDEX IF NOT EXISTS idx_items_source_collected ON items (source_id, collected_at);
-- Varlık bazlı silme (KVKK erişim/silme talepleri): tip + küçük harfli değer
CREATE INDEX IF NOT EXISTS idx_entities_type_normalized ON entities (type, normalized_value);
