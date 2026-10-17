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

/// Zaman damgası sütununu FastAPI/pydantic JSON biçiminde metne çeviren SQL ifadesi:
/// UTC, `YYYY-MM-DDTHH:MM:SS[.ffffff]Z` — mikrosaniye sıfırsa kesir yok, değilse tam 6 hane
/// (PostgreSQL `to_json` sondaki sıfırları atar, pydantic atmaz). NULL korunur.
#[macro_export]
macro_rules! sql_ts {
    ($col:literal) => {
        concat!(
            "(to_char((",
            $col,
            ") AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS') || CASE WHEN ",
            "extract(microseconds FROM (",
            $col,
            "))::bigint % 1000000 = 0 THEN '' ELSE ",
            "to_char((",
            $col,
            ") AT TIME ZONE 'UTC', '.US') END || 'Z')"
        )
    };
}

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

/// Tembel havuz (API/CLI): DB kapalıyken de süreç başlar; her istek en çok `acquire` bekler
/// (Python'daki istek başına `connect_timeout` karşılığı).
pub fn connect_lazy(url: &str, max_connections: u32, acquire: Duration) -> Result<PgPool, Error> {
    PgPoolOptions::new().max_connections(max_connections).acquire_timeout(acquire).connect_lazy(url)
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
    let ents = purge_orphan_entities(pool).await?;
    if deleted > 0 || ents > 0 {
        tracing::info!(deleted, ents, "Saklama temizliği");
    }
    Ok((deleted, ents))
}

const ORPHAN_ENTITIES: &str = "DELETE FROM entities e WHERE NOT EXISTS \
     (SELECT 1 FROM item_entities ie WHERE ie.entity_id = e.id)";

/// `retention.purge_orphan_entities`: hiçbir öğeye bağlı olmayan varlıkları siler.
pub async fn purge_orphan_entities<'e, E>(executor: E) -> Result<u64, Error>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    Ok(sqlx::query(ORPHAN_ENTITIES).execute(executor).await?.rows_affected())
}

// ------------------------------------------------------------------------- denetim (R6)

/// Eklenen denetim kaydının zincir bilgisi.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditAppend {
    pub prev_hash: String,
    pub hash: String,
    pub chain_version: i64,
}

/// `osiris.audit.append_audit` — append-only, hash zincirli (SOZLESME §2). Danışma kilidi
/// zincirin eşzamanlı yazıcılarla çatallanmasını önler; migration 003 yoksa v1 yazılır.
pub async fn append_audit(
    pool: &PgPool,
    user_id: Option<&str>,
    action: &str,
    resource: Option<&str>,
    detail: &osiris_hash::Json,
) -> Result<AuditAppend, Error> {
    use osiris_hash::audit::{CURRENT_CHAIN_VERSION, GENESIS, canonical_payload, chain_hash};
    use osiris_hash::canonical::{jcs_safe, py_dumps};
    use osiris_hash::{Json, truncate_chars};

    let protocol = |m: &str| Error::Protocol(m.to_owned());
    let n = action.chars().count();
    if n == 0 || n > 200 {
        return Err(protocol("action 1-200 karakter olmalı"));
    }
    if resource.is_some_and(|r| r.chars().count() > 500) {
        return Err(protocol("resource çok uzun"));
    }
    // _normalize: kesme önce; nesne olmayan ayrıntı {"value": …} olur
    let user_id = truncate_chars(user_id.unwrap_or(""), 200);
    let resource = truncate_chars(resource.unwrap_or(""), 500);
    let detail = match detail {
        d if d.is_falsy() => Json::Object(Vec::new()), // `detail or {}`
        Json::Object(_) => jcs_safe(detail),
        other => Json::Object(vec![("value".to_owned(), jcs_safe(other))]),
    };
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('osiris_audit_chain'))")
        .execute(&mut *tx)
        .await?;
    let has_version: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
         WHERE table_name = 'audit_logs' AND column_name = 'chain_version')",
    )
    .fetch_one(&mut *tx)
    .await?;
    let version = if has_version { CURRENT_CHAIN_VERSION } else { 1 };
    let prev: Option<String> =
        sqlx::query_scalar("SELECT hash::text FROM audit_logs ORDER BY id DESC LIMIT 1")
            .fetch_optional(&mut *tx)
            .await?;
    let prev_hash = prev.filter(|p| !p.is_empty()).unwrap_or_else(|| GENESIS.to_owned());
    let payload = canonical_payload(Some(user_id), action, Some(resource), Some(&detail), version)
        .map_err(|e| protocol(&e.0))?;
    let digest = chain_hash(Some(&prev_hash), &payload);
    let stored_prev = (prev_hash != GENESIS).then_some(prev_hash.as_str());
    let q = if has_version {
        sqlx::query(
            "INSERT INTO audit_logs (user_id, action, resource, detail, prev_hash, hash, chain_version) \
             VALUES ($1, $2, $3, $4::jsonb, $5, $6, $7)",
        )
    } else {
        sqlx::query(
            "INSERT INTO audit_logs (user_id, action, resource, detail, prev_hash, hash) \
             VALUES ($1, $2, $3, $4::jsonb, $5, $6)",
        )
    };
    let q = q
        .bind(user_id)
        .bind(action)
        .bind(resource)
        .bind(py_dumps(&detail))
        .bind(stored_prev)
        .bind(&digest);
    let q = if has_version { q.bind(version as i32) } else { q };
    q.execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(AuditAppend { prev_hash, hash: digest, chain_version: version })
}

/// `verify_chain` sonucu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainReport {
    pub ok: bool,
    pub checked: usize,
    /// Pencere tablonun başını içermiyor (ilk kaydın öncülü doğrulanamadı).
    pub truncated: bool,
    pub error: Option<String>,
}

/// `osiris.audit.verify_chain`: son `limit` kaydın (1–100 000) zincir bütünlüğü.
pub async fn verify_chain(pool: &PgPool, limit: i64) -> Result<ChainReport, Error> {
    use osiris_hash::Json;
    use osiris_hash::audit::{AuditRow, verify_records};
    let rows = sqlx::query(
        "SELECT id, user_id, action, resource, detail::text, prev_hash::text, hash::text, \
         chain_version::bigint FROM audit_logs ORDER BY id DESC LIMIT $1",
    )
    .bind(limit.clamp(1, 100_000))
    .fetch_all(pool)
    .await?;
    let first_id: Option<i64> = rows.last().map(|r| r.get(0));
    let min_id: Option<i64> =
        sqlx::query_scalar("SELECT min(id) FROM audit_logs").fetch_one(pool).await?;
    let rows: Vec<AuditRow> = rows
        .iter()
        .rev()
        .map(|r| AuditRow {
            user_id: r.get(1),
            action: r.get(2),
            resource: r.get(3),
            detail: r
                .get::<Option<String>, _>(4)
                .map(|t| Json::parse_py(&t).unwrap_or(Json::Object(Vec::new()))),
            prev_hash: r.get(5),
            hash: r.get(6),
            chain_version: r.get(7),
        })
        .collect();
    let v = verify_records(&rows);
    Ok(ChainReport {
        ok: v.ok,
        checked: v.checked,
        truncated: v.ok && first_id.is_some() && first_id != min_id,
        error: v.error,
    })
}

// ---------------------------------------------------------------------- graf kenarları

/// `GraphEngine.load_from_db`: en son görülen kenarlar (`limit` 1–100 000).
pub async fn graph_edges(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<(String, String, String, f64)>, Error> {
    let rows = sqlx::query(
        "SELECT source_name, target_name, relation_type, weight FROM graph_edges \
         ORDER BY last_seen_at DESC LIMIT $1",
    )
    .bind(limit.clamp(1, 100_000))
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| (r.get(0), r.get(1), r.get(2), r.get(3))).collect())
}

/// `GraphEngine.save_to_db` tek kenar için: upsert, `evidence_count` artar.
pub async fn upsert_graph_edge(
    pool: &PgPool,
    source: &str,
    target: &str,
    relation_type: &str,
    weight: f64,
) -> Result<(), Error> {
    use osiris_hash::truncate_chars;
    sqlx::query(
        "INSERT INTO graph_edges (source_name, target_name, relation_type, weight, last_seen_at) \
         VALUES ($1, $2, $3, $4, NOW()) ON CONFLICT (source_name, target_name, relation_type) \
         DO UPDATE SET weight = EXCLUDED.weight, evidence_count = graph_edges.evidence_count + 1, \
         last_seen_at = NOW()",
    )
    .bind(truncate_chars(source, 500))
    .bind(truncate_chars(target, 500))
    .bind(truncate_chars(relation_type, 100))
    .bind(weight)
    .execute(pool)
    .await?;
    Ok(())
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
