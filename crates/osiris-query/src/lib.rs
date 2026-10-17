//! Sorgu motoru (R6) — Python `osiris_query.QueryEngine` ile aynı SQL ve doğrulama.
//!
//! Satırlar PostgreSQL'de `json_agg` ile JSON'a çevrilir: zaman damgaları oturum saat
//! diliminde (sqlx: UTC) ISO 8601 yazılır — FastAPI'nin `datetime.isoformat()` çıktısıyla aynı
//! biçim (`2026-10-17T12:00:00.123456+00:00`, mikrosaniye sıfırsa kesirsiz).

use osiris_hash::canonical::py_float_repr;
use osiris_hash::py_strip;
use sqlx::PgPool;

pub use osiris_db::VALID_ENTITY_TYPES;

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    /// Python `ValueError` (API: 400, ileti gövdeye yazılır)
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

fn invalid(msg: &str) -> QueryError {
    QueryError::Invalid(msg.to_owned())
}

/// `_validate_limit`: en az 1, en çok 100.
pub fn validate_limit(limit: i64) -> Result<i64, QueryError> {
    if limit < 1 {
        return Err(invalid("limit en az 1 olmalı"));
    }
    Ok(limit.min(100))
}

/// `_validate_query`: kırpılmış, boş olmayan, ≤ 500 kod noktası.
pub fn validate_query(query: &str) -> Result<String, QueryError> {
    let q = py_strip(query);
    if q.is_empty() {
        return Err(invalid("query boş olamaz"));
    }
    if q.chars().count() > 500 {
        return Err(invalid("query çok uzun (max 500)"));
    }
    Ok(q.to_owned())
}

/// `[x,y,…]` — Python `str(float(x))` ile.
pub fn vector_literal(embedding: &[f64]) -> String {
    let parts: Vec<String> = embedding.iter().map(|x| py_float_repr(*x)).collect();
    format!("[{}]", parts.join(","))
}

type Scalar<'q> = sqlx::query::QueryScalar<'q, sqlx::Postgres, String, sqlx::postgres::PgArguments>;

/// Satırlar: JSON dizi metni (sütun sırası korunur; zaman damgaları `sql_ts!` biçiminde).
async fn rows(q: Scalar<'_>, pool: &PgPool) -> Result<String, QueryError> {
    Ok(q.fetch_one(pool).await?)
}

/// JSON dizi metnindeki satır sayısı.
pub fn count_rows(text: &str) -> usize {
    serde_json::from_str::<Vec<serde::de::IgnoredAny>>(text).map_or(0, |v| v.len())
}

const FULLTEXT: &str = concat!(
    "SELECT COALESCE(json_agg(t), '[]')::text FROM (\
    SELECT id::text AS id, title, url, LEFT(cleaned_content, 2000) AS cleaned_content, ",
    osiris_db::sql_ts!("collected_at"),
    " AS collected_at FROM items \
    WHERE to_tsvector('simple', COALESCE(cleaned_content,'')) @@ plainto_tsquery('simple', $1) \
    ORDER BY ts_rank(to_tsvector('simple', COALESCE(cleaned_content,'')), plainto_tsquery('simple', $1)) DESC \
    LIMIT $2) t"
);

/// `fulltext_search` (tsvector, `simple` sözlüğü, `ts_rank` sırası).
pub async fn fulltext_search(pool: &PgPool, query: &str, limit: i64) -> Result<String, QueryError> {
    let query = validate_query(query)?;
    let limit = validate_limit(limit)?;
    rows(sqlx::query_scalar(FULLTEXT).bind(query).bind(limit), pool).await
}

macro_rules! semantic_sql {
    ($col:literal) => {
        concat!(
            "SELECT COALESCE(json_agg(t), '[]')::text FROM (",
            "SELECT id::text AS id, title, url, LEFT(cleaned_content, 2000) AS cleaned_content, ",
            osiris_db::sql_ts!("collected_at"),
            " AS collected_at, ",
            "1 - (",
            $col,
            " <=> $1::vector) AS similarity FROM items WHERE ",
            $col,
            " IS NOT NULL ",
            "ORDER BY ",
            $col,
            " <=> $1::vector LIMIT $2) t"
        )
    };
}
const SEMANTIC: &str = semantic_sql!("embedding_v2");

/// `semantic_search`: pgvector kosinüs, `embedding_v2` (384 boyut, R3/K5). Eski 1536 boyutlu
/// `embedding` sütunu R7'de (migration 006) kaldırıldı.
pub async fn semantic_search(
    pool: &PgPool,
    embedding: &[f64],
    limit: i64,
) -> Result<String, QueryError> {
    let limit = validate_limit(limit)?;
    if embedding.is_empty() {
        return Err(invalid("embedding boş olamaz"));
    }
    if embedding.len() != 384 || !embedding.iter().all(|x| x.is_finite()) {
        return Err(invalid("embedding geçersiz (384 boyutlu sonlu vektör olmalı)"));
    }
    rows(sqlx::query_scalar(SEMANTIC).bind(vector_literal(embedding)).bind(limit), pool).await
}

/// ILIKE jokerlerini etkisizleştirir (`\` kaçış karakteriyle).
pub fn escape_like(value: &str) -> String {
    value.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

const ENTITY: &str = "SELECT COALESCE(json_agg(t), '[]')::text FROM (\
    SELECT i.id::text AS id, i.title, i.url, LEFT(i.cleaned_content, 2000) AS cleaned_content \
    FROM items i JOIN item_entities ie ON ie.item_id = i.id JOIN entities e ON e.id = ie.entity_id \
    WHERE e.type = $1::entity_type AND e.value ILIKE $2 ESCAPE '\\' LIMIT $3) t";

/// `entity_search` (tip + değer alt-dizesi, büyük/küçük harf duyarsız).
pub async fn entity_search(
    pool: &PgPool,
    entity_type: &str,
    value: &str,
    limit: i64,
) -> Result<String, QueryError> {
    if !VALID_ENTITY_TYPES.contains(&entity_type) {
        let shown = osiris_hash::pystr::py_str_repr(entity_type);
        return Err(QueryError::Invalid(format!("Geçersiz entity_type: {shown}")));
    }
    if py_strip(value).is_empty() || value.chars().count() > 500 {
        return Err(invalid("value geçersiz"));
    }
    let limit = validate_limit(limit)?;
    let pattern = format!("%{}%", escape_like(value));
    rows(sqlx::query_scalar(ENTITY).bind(entity_type).bind(pattern).bind(limit), pool).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dogrulama() {
        assert!(matches!(validate_query("   "), Err(QueryError::Invalid(_))));
        assert_eq!(validate_query(" a ").unwrap(), "a");
        assert!(validate_query(&"ş".repeat(501)).is_err());
        assert!(validate_query(&"ş".repeat(500)).is_ok());
        assert_eq!(validate_limit(1000).unwrap(), 100);
        assert!(validate_limit(0).is_err());
        assert_eq!(escape_like(r"a%b_c\"), r"a\%b\_c\\");
        assert_eq!(vector_literal(&[0.1, -1.0, 3e-8]), "[0.1,-1.0,3e-08]");
        assert_eq!(count_rows(r#"[{"a":1}, {"b":[2]}]"#), 2);
        assert_eq!(count_rows("bozuk"), 0);
    }
}
