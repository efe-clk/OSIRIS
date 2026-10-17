//! PostgreSQL erişimi (R5) — Python `ProcessingPipeline.store`, `retention.purge_expired`,
//! `CollectorManager._record_health` ile aynı SQL. Şema migration'larla yönetilir; bu crate
//! yalnızca okur/yazar (sütun eklemez, migration çalıştırmaz).
//!
//! Sorgular çalışma zamanında doğrulanır (`sqlx::query`): derleme için veritabanı gerekmez.

use serde_json::Value;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Postgres, Row, Transaction};
use std::time::Duration;

pub use sqlx::Error;

/// Geçerli varlık türleri (`entity_type` enum'u); diğerleri `custom` olur.
pub const VALID_ENTITY_TYPES: [&str; 12] = [
    "person",
    "org",
    "location",
    "ip",
    "domain",
    "email",
    "phone",
    "crypto_address",
    "hash",
    "username",
    "cve",
    "custom",
];

pub async fn connect(url: &str) -> Result<PgPool, Error> {
    PgPoolOptions::new()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(10))
        .connect(url)
        .await
}

/// Depolanacak öğe (Python `store()` girdisi).
#[derive(Debug, Clone, Default)]
pub struct NewItem {
    /// Geçerli UUID ya da boş (kaynak yoksa NULL).
    pub source_id: Option<String>,
    pub raw_content: String,
    pub cleaned_content: String,
    pub url: String,
    pub title: String,
    pub language: String,
    /// Ham tarih dizgesi (`NULLIF(...,'')::timestamptz` ile PostgreSQL ayrıştırır).
    pub published_at: String,
    pub content_hash: String,
    pub metadata: Value,
    pub tags: Vec<String>,
    /// 384 boyutlu vektör + üreten model (migration 005).
    pub embedding: Option<(Vec<f32>, String)>,
    pub entities: Vec<(String, String)>,
}

/// `[x,y,…]` pgvector metni (f32 en kısa gösterim — float4'e geri aynı değer).
pub fn vector_literal(v: &[f32]) -> String {
    let parts: Vec<String> = v.iter().map(|x| format!("{x:?}")).collect();
    format!("[{}]", parts.join(","))
}

/// Migration 005 uygulanmış mı?
pub async fn has_column(pool: &PgPool, table: &str, column: &str) -> Result<bool, Error> {
    Ok(sqlx::query(
        "SELECT 1 FROM information_schema.columns WHERE table_name = $1 AND column_name = $2",
    )
    .bind(table)
    .bind(column)
    .fetch_optional(pool)
    .await?
    .is_some())
}

macro_rules! insert_item {
    ($cols:literal, $vals:literal) => {
        concat!(
            "INSERT INTO items (source_id, raw_content, cleaned_content, url, title, language, ",
            "collected_at, published_at, content_hash, metadata, tags",
            $cols,
            ") VALUES ",
            "((SELECT id FROM sources WHERE id = NULLIF($1,'')::uuid), $2, $3, $4, $5, $6, NOW(), ",
            "NULLIF($7,'')::timestamptz, $8, $9::jsonb, $10",
            $vals,
            ") ",
            "ON CONFLICT (content_hash) DO NOTHING RETURNING id::text"
        )
    };
}

const INSERT_ITEM: &str = insert_item!("", "");
const INSERT_ITEM_VEC: &str = insert_item!(", embedding_v2, embedding_model", ", $11::vector, $12");

/// `ProcessingPipeline.store`: öğe (ON CONFLICT content_hash → mevcut id), varlıklar ve
/// bağlantılar tek işlemde. Dönen: öğe kimliği (tekrarsa mevcut olanınki).
pub async fn store(
    pool: &PgPool,
    item: &NewItem,
    embedding_v2: bool,
) -> Result<Option<String>, Error> {
    let mut tx: Transaction<'_, Postgres> = pool.begin().await?;
    let with_vec = embedding_v2 && item.embedding.is_some();
    let sql = if with_vec { INSERT_ITEM_VEC } else { INSERT_ITEM };
    let mut q = sqlx::query(sql)
        .bind(item.source_id.clone().unwrap_or_default())
        .bind(&item.raw_content)
        .bind(&item.cleaned_content)
        .bind(&item.url)
        .bind(&item.title)
        .bind(&item.language)
        .bind(&item.published_at)
        .bind(&item.content_hash)
        .bind(item.metadata.to_string())
        .bind(&item.tags);
    if with_vec && let Some((v, model)) = &item.embedding {
        q = q.bind(vector_literal(v)).bind(model);
    }
    let row = q.fetch_optional(&mut *tx).await?;
    let Some(row) = row else {
        let existing: Option<String> =
            sqlx::query_scalar("SELECT id::text FROM items WHERE content_hash = $1")
                .bind(&item.content_hash)
                .fetch_optional(&mut *tx)
                .await?;
        tx.commit().await?;
        return Ok(existing);
    };
    let item_id: String = row.get(0);
    for (etype, value) in item.entities.iter().take(200) {
        let etype =
            if VALID_ENTITY_TYPES.contains(&etype.as_str()) { etype.as_str() } else { "custom" };
        let value: String = value.chars().take(500).collect();
        if value.is_empty() {
            continue;
        }
        let entity_id: String = sqlx::query_scalar(
            "INSERT INTO entities (type, value, normalized_value, last_seen_at) VALUES ($1::entity_type, $2, $3, NOW()) \
             ON CONFLICT (type, value) DO UPDATE SET last_seen_at = NOW() RETURNING id::text",
        )
        .bind(etype)
        .bind(&value)
        .bind(value.to_lowercase())
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO item_entities (item_id, entity_id, mention_count) VALUES ($1::uuid, $2::uuid, 1) \
             ON CONFLICT (item_id, entity_id) DO UPDATE SET mention_count = item_entities.mention_count + 1",
        )
        .bind(&item_id)
        .bind(&entity_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Some(item_id))
}

/// `CollectorManager._record_health`
pub async fn record_health(
    pool: &PgPool,
    source_id: &str,
    success: bool,
    elapsed_ms: i64,
    items: i64,
) -> Result<(), Error> {
    let mut tx = pool.begin().await?;
    let sql = if success {
        "UPDATE sources SET last_crawled_at = NOW(), last_success_at = NOW(), failure_count = 0, \
         avg_response_ms = $1, updated_at = NOW() WHERE id::text = $2"
    } else {
        "UPDATE sources SET last_crawled_at = NOW(), failure_count = failure_count + 1, \
         avg_response_ms = $1, updated_at = NOW() WHERE id::text = $2"
    };
    sqlx::query(sql).bind(elapsed_ms as i32).bind(source_id).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO source_metrics (time, source_id, response_ms, success, items_count) \
         VALUES (NOW(), NULLIF($1,'')::uuid, $2, $3, $4)",
    )
    .bind(source_id)
    .bind(elapsed_ms as i32)
    .bind(success)
    .bind(if success { items as i32 } else { 0 })
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// Zamanlanacak kaynak.
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledSource {
    pub id: String,
    pub plugin_id: String,
    pub url: Option<String>,
    pub schedule: String,
    pub metadata: Value,
}

/// Etkin ve `schedule` alanı dolu kaynaklar.
pub async fn scheduled_sources(pool: &PgPool) -> Result<Vec<ScheduledSource>, Error> {
    let rows = sqlx::query(
        "SELECT id::text, plugin_id, url, schedule, COALESCE(metadata, '{}'::jsonb)::text FROM sources \
         WHERE enabled AND schedule IS NOT NULL AND btrim(schedule) <> '' ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ScheduledSource {
            id: r.get(0),
            plugin_id: r.get(1),
            url: r.get(2),
            schedule: r.get(3),
            metadata: serde_json::from_str(&r.get::<String, _>(4)).unwrap_or(Value::Null),
        })
        .collect())
}

/// `retention.default_retention_days`: `OSIRIS_RETENTION_DAYS` (0–36500; geçersiz → 180).
pub fn default_retention_days() -> i64 {
    match std::env::var("OSIRIS_RETENTION_DAYS") {
        Err(_) => 180,
        Ok(v) if v.is_empty() => 180,
        Ok(v) => v.trim().parse::<i64>().map(|d| d.clamp(0, 36_500)).unwrap_or(180),
    }
}

/// `retention.purge_expired` → (silinen öğe, silinen yetim varlık).
pub async fn purge_expired(
    pool: &PgPool,
    default_days: i64,
    batch: i64,
    max_batches: usize,
) -> Result<(u64, u64), Error> {
    let per_source = has_column(pool, "sources", "retention_days").await?;
    let sql = if per_source {
        "DELETE FROM items WHERE id IN (SELECT i.id FROM items i LEFT JOIN sources s ON s.id = i.source_id \
         WHERE COALESCE(s.retention_days, $1) > 0 \
         AND i.collected_at < NOW() - make_interval(days => COALESCE(s.retention_days, $1)) LIMIT $2)"
    } else {
        "DELETE FROM items WHERE id IN (SELECT id FROM items WHERE $1 > 0 \
         AND collected_at < NOW() - make_interval(days => $1) LIMIT $2)"
    };
    let batch = batch.clamp(1, 10_000);
    let mut deleted = 0;
    for _ in 0..max_batches {
        let n = sqlx::query(sql)
            .bind(default_days as i32)
            .bind(batch)
            .execute(pool)
            .await?
            .rows_affected();
        deleted += n;
        if (n as i64) < batch {
            break;
        }
    }
    let ents = sqlx::query(
        "DELETE FROM entities e WHERE NOT EXISTS (SELECT 1 FROM item_entities ie WHERE ie.entity_id = e.id)",
    )
    .execute(pool)
    .await?
    .rows_affected();
    if deleted > 0 || ents > 0 {
        tracing::info!(deleted, ents, "Saklama temizliği");
    }
    Ok((deleted, ents))
}

/// Vektörü olmayan öğeler (`--reembed`).
pub async fn items_without_embedding(
    pool: &PgPool,
    batch: i64,
) -> Result<Vec<(String, String)>, Error> {
    let rows = sqlx::query(
        "SELECT id::text, COALESCE(cleaned_content, '') FROM items \
         WHERE embedding_v2 IS NULL AND embedding_model IS NULL ORDER BY collected_at LIMIT $1",
    )
    .bind(batch)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| (r.get(0), r.get(1))).collect())
}

pub async fn set_embedding(
    pool: &PgPool,
    item_id: &str,
    vec: Option<&[f32]>,
    model: &str,
) -> Result<(), Error> {
    match vec {
        Some(v) => sqlx::query(
            "UPDATE items SET embedding_v2 = $1::vector, embedding_model = $2 WHERE id::text = $3",
        )
        .bind(vector_literal(v))
        .bind(model)
        .bind(item_id)
        .execute(pool)
        .await
        .map(|_| ()),
        None => sqlx::query("UPDATE items SET embedding_model = $1 WHERE id::text = $2")
            .bind(format!("{model}#skip").chars().take(200).collect::<String>())
            .bind(item_id)
            .execute(pool)
            .await
            .map(|_| ()),
    }
}

/// Gölge karşılaştırma için Python'un yazdığı öğe.
#[derive(Debug, Clone)]
pub struct StoredItem {
    pub id: String,
    pub raw_content: String,
    pub cleaned_content: String,
    pub content_hash: String,
    pub language: String,
    pub embedding: Option<Vec<f32>>,
    pub entities: Vec<(String, String)>,
}

/// Son `minutes` dakikada toplanan öğeler (en fazla `limit`).
pub async fn recent_items(
    pool: &PgPool,
    minutes: i64,
    limit: i64,
) -> Result<Vec<StoredItem>, Error> {
    let rows = sqlx::query(
        "SELECT id::text, COALESCE(raw_content,''), COALESCE(cleaned_content,''), content_hash, \
         COALESCE(language,''), embedding_v2::text FROM items \
         WHERE collected_at > NOW() - make_interval(mins => $1) ORDER BY collected_at DESC LIMIT $2",
    )
    .bind(minutes as i32)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for r in rows {
        let id: String = r.get(0);
        let ents = sqlx::query(
            "SELECT e.type::text, e.value FROM item_entities ie JOIN entities e ON e.id = ie.entity_id \
             WHERE ie.item_id = $1::uuid",
        )
        .bind(&id)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|e| (e.get(0), e.get(1)))
        .collect();
        let emb: Option<String> = r.get(5);
        out.push(StoredItem {
            id,
            raw_content: r.get(1),
            cleaned_content: r.get(2),
            content_hash: r.get(3),
            language: r.get(4),
            embedding: emb.map(|s| {
                s.trim_matches(['[', ']'])
                    .split(',')
                    .filter_map(|x| x.trim().parse().ok())
                    .collect()
            }),
            entities: ents,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn vektor_metni() {
        assert_eq!(super::vector_literal(&[0.1, -1.0, 3e-8]), "[0.1,-1.0,3e-8]");
    }
}
