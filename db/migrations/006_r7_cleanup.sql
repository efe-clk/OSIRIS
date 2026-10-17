-- OSIRIS — Migration 006 (Rust geçişi R7, kullanıcı kararı 2026-10-17)
-- 1) Eski `items.embedding VECTOR(1536)` sütunu ve HNSW indeksi kaldırılır. VERİ SİLER:
--    vektörler `embedding_v2`'dedir (384, migration 005); öncesinde
--    `osiris-server pipeline --reembed` ile eksikler doldurulabilir.
-- 2) Denetim zinciri yalnızca v2 (RFC 8785): yeni kayıtların varsayılanı 2. Tablodaki eski v1
--    kayıtlar değiştirilmez (append-only) ancak uygulama artık onları doğrulamaz.
-- Tekrar çalıştırılabilir.

DROP INDEX IF EXISTS idx_items_embedding;
ALTER TABLE items DROP COLUMN IF EXISTS embedding;

ALTER TABLE audit_logs ALTER COLUMN chain_version SET DEFAULT 2;
