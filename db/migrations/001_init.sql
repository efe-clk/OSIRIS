-- OSIRIS — Veritabanı şeması (Faz 1)
-- Tekrar çalıştırılabilir (idempotent): tüm nesneler IF NOT EXISTS korumalı.
-- PostgreSQL 16 + pgvector + TimescaleDB
-- Bkz. doküman §7.1 ve §9

-- pgvector eklentisi (semantik arama için)
CREATE EXTENSION IF NOT EXISTS vector;
-- TimescaleDB eklentisi (zaman serisi verileri için; kurulu değilse uyarıyla geçilir,
-- pgvector/pgvector:pg16 imajında varsayılan olarak YOKTUR — bkz. source_metrics)
DO $$
BEGIN
    CREATE EXTENSION IF NOT EXISTS timescaledb;
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'TimescaleDB kurulamadi (%), source_metrics normal tablo olarak kalacak', SQLSTATE;
END
$$;

-- UUID üretimi
CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- ============================================================
-- Kaynaklar (sources)
-- ============================================================
DO $$
BEGIN
    CREATE TYPE network_type AS ENUM (    'www', 'tor', 'i2p', 'p2p', 'freenet', 'zeronet', 'irc',
    'matrix', 'rss', 'api', 'blockchain', 'sdr', 'custom');
EXCEPTION WHEN duplicate_object THEN
    RAISE NOTICE 'type exists: %', 'network_type';
END
$$;

CREATE TABLE IF NOT EXISTS sources (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL,
    url             TEXT,
    network_type    network_type NOT NULL,
    plugin_id       TEXT NOT NULL,
    auth_config     JSONB,          -- Kimlik bilgileri (şifreli)
    proxy_config    JSONB,          -- Proxy/VPN ayarları
    schedule        TEXT,           -- Cron ifadesi
    priority        INTEGER NOT NULL DEFAULT 5,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    tags            TEXT[],
    last_crawled_at TIMESTAMPTZ,
    last_success_at TIMESTAMPTZ,
    failure_count   INTEGER NOT NULL DEFAULT 0,
    avg_response_ms INTEGER,
    metadata        JSONB,          -- Plugin'e özel ek ayarlar
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_sources_network_type ON sources (network_type);
CREATE INDEX IF NOT EXISTS idx_sources_plugin_id   ON sources (plugin_id);
CREATE INDEX IF NOT EXISTS idx_sources_enabled     ON sources (enabled);

-- ============================================================
-- Toplanan veri birimleri (items)
-- ============================================================
CREATE TABLE IF NOT EXISTS items (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id       UUID REFERENCES sources(id) ON DELETE CASCADE,
    raw_content     TEXT,
    cleaned_content TEXT,
    url             TEXT,
    title           TEXT,
    language        CHAR(5),
    collected_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    published_at    TIMESTAMPTZ,
    content_hash    CHAR(64) UNIQUE,   -- Yineleme önleme (SHA-256)
    metadata        JSONB,
    tags            TEXT[]
);

CREATE INDEX IF NOT EXISTS idx_items_source_id     ON items (source_id);
CREATE INDEX IF NOT EXISTS idx_items_collected_at  ON items (collected_at);
CREATE INDEX IF NOT EXISTS idx_items_language      ON items (language);
-- (R7) Eski `embedding VECTOR(1536)` sütunu ve HNSW indeksi kaldırıldı — vektörler
-- `embedding_v2` (migration 005); mevcut veritabanlarında sütunu 006 siler.
CREATE INDEX IF NOT EXISTS idx_items_content_fts   ON items USING gin (to_tsvector('simple', cleaned_content));

-- ============================================================
-- Çıkarılan varlıklar (entities)
-- ============================================================
DO $$
BEGIN
    CREATE TYPE entity_type AS ENUM (    'person', 'org', 'location', 'ip', 'domain', 'email',
    'phone', 'crypto_address', 'hash', 'username', 'cve', 'custom');
EXCEPTION WHEN duplicate_object THEN
    RAISE NOTICE 'type exists: %', 'entity_type';
END
$$;

CREATE TABLE IF NOT EXISTS entities (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    type             entity_type NOT NULL,
    value            TEXT NOT NULL,
    normalized_value TEXT,
    confidence       FLOAT,
    first_seen_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata         JSONB
);

CREATE INDEX IF NOT EXISTS idx_entities_type  ON entities (type);
CREATE INDEX IF NOT EXISTS idx_entities_value ON entities (value);
CREATE UNIQUE INDEX IF NOT EXISTS idx_entities_type_value ON entities (type, value);

-- ============================================================
-- Varlık-Veri ilişkisi (item_entities)
-- ============================================================
CREATE TABLE IF NOT EXISTS item_entities (
    item_id       UUID REFERENCES items(id) ON DELETE CASCADE,
    entity_id     UUID REFERENCES entities(id) ON DELETE CASCADE,
    mention_count INTEGER NOT NULL DEFAULT 1,
    context       TEXT,
    PRIMARY KEY (item_id, entity_id)
);

-- ============================================================
-- Varlıklar arası ilişkiler (entity_relations) — graf kenarları
-- ============================================================
CREATE TABLE IF NOT EXISTS entity_relations (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_entity_id UUID REFERENCES entities(id) ON DELETE CASCADE,
    target_entity_id UUID REFERENCES entities(id) ON DELETE CASCADE,
    relation_type    TEXT,
    weight           FLOAT NOT NULL DEFAULT 1.0,
    evidence_count   INTEGER NOT NULL DEFAULT 1,
    first_seen_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_entity_relations_source ON entity_relations (source_entity_id);
CREATE INDEX IF NOT EXISTS idx_entity_relations_target ON entity_relations (target_entity_id);

-- ============================================================
-- Kayıtlı sorgular & uyarılar (saved_queries)
-- ============================================================
DO $$
BEGIN
    CREATE TYPE query_type AS ENUM ('fts', 'semantic', 'regex', 'entity', 'graph');
EXCEPTION WHEN duplicate_object THEN
    RAISE NOTICE 'type exists: %', 'query_type';
END
$$;

CREATE TABLE IF NOT EXISTS saved_queries (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name             TEXT,
    query_text       TEXT,
    query_type       query_type NOT NULL DEFAULT 'fts',
    alert_enabled    BOOLEAN NOT NULL DEFAULT FALSE,
    alert_channels   JSONB,
    last_triggered_at TIMESTAMPTZ
);

-- ============================================================
-- Denetim logları (audit_logs) — append-only, hash zincirli
-- ============================================================
CREATE TABLE IF NOT EXISTS audit_logs (
    id          BIGSERIAL PRIMARY KEY,
    ts          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    user_id     TEXT,
    action      TEXT NOT NULL,
    resource    TEXT,
    detail      JSONB,
    prev_hash   CHAR(64),           -- Önceki kaydın SHA-256'sı (zincir)
    hash        CHAR(64) NOT NULL   -- Bu kaydın SHA-256'sı
);

-- ============================================================
-- Zaman serisi ölçümleri (TimescaleDB hypertable)
-- ============================================================
CREATE TABLE IF NOT EXISTS source_metrics (
    time        TIMESTAMPTZ NOT NULL,
    source_id   UUID REFERENCES sources(id) ON DELETE CASCADE,
    response_ms INTEGER,
    success     BOOLEAN,
    items_count INTEGER
);

-- TimescaleDB yoksa hypertable oluşturulamaz; tablo normal tablo olarak kalır.
DO $$
BEGIN
    PERFORM create_hypertable('source_metrics', 'time', if_not_exists => TRUE);
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'create_hypertable atlandi (TimescaleDB yok, %)', SQLSTATE;
END
$$;
