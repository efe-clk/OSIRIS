-- OSIRIS — Migration 005: fastembed embedding sütunu (Rust geçişi R3 / K5)
-- Yalnızca ekleme. Eski `embedding VECTOR(1536)` sütunu R7'ye kadar korunur.
-- Model: sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2 (384 boyut, L2-normalize)
-- `embedding_model`: vektörü üreten model adı (farklı modellerin vektörleri karıştırılmasın).
-- Tekrar çalıştırılabilir.

ALTER TABLE items ADD COLUMN IF NOT EXISTS embedding_v2 VECTOR(384);
ALTER TABLE items ADD COLUMN IF NOT EXISTS embedding_model TEXT;

CREATE INDEX IF NOT EXISTS idx_items_embedding_v2
    ON items USING hnsw (embedding_v2 vector_cosine_ops);
-- Yeniden embed etme taraması: henüz vektörü olmayan öğeler
CREATE INDEX IF NOT EXISTS idx_items_embedding_v2_missing
    ON items (collected_at) WHERE embedding_v2 IS NULL;
