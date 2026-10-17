//! Gölge karşılaştırma (plan R5 "paralel çalışma"): Python işçisinin yazdığı öğeleri Rust
//! hattıyla yeniden hesaplar, **yazmadan** karşılaştırır. Akışa dokunmaz (Python'un `XDEL`'i
//! ile yarışmaz); kaynak olarak DB'deki `raw_content` kullanılır.
//!
//! Karşılaştırılan: `cleaned_content`, `content_hash` (birebir), varlık kümesi (birebir),
//! embedding (kosinüs ≥ 0.999), `language` (bilgi amaçlı — langdetect ↔ lingua farklı).

use crate::Pipeline;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::collections::BTreeSet;

#[derive(Debug, Default, Clone)]
pub struct Report {
    pub checked: usize,
    pub cleaned_mismatch: usize,
    pub hash_mismatch: usize,
    pub entity_mismatch: usize,
    pub embedding_mismatch: usize,
    pub embedding_compared: usize,
    pub language_agree: usize,
    pub examples: Vec<Value>,
}

fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(x, y)| f64::from(*x) * f64::from(*y)).sum();
    let na: f64 = a.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    let nb: f64 = b.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}

impl Report {
    /// Uyuşmazlık yoksa true (dil hariç).
    pub fn clean(&self) -> bool {
        self.cleaned_mismatch + self.hash_mismatch + self.entity_mismatch + self.embedding_mismatch
            == 0
    }

    pub fn to_json(&self) -> Value {
        json!({
            "checked": self.checked,
            "cleaned_mismatch": self.cleaned_mismatch,
            "hash_mismatch": self.hash_mismatch,
            "entity_mismatch": self.entity_mismatch,
            "embedding_compared": self.embedding_compared,
            "embedding_mismatch": self.embedding_mismatch,
            "language_agree": self.language_agree,
            "ok": self.clean(),
            "examples": self.examples,
        })
    }
}

/// Son `minutes` dakikadaki en fazla `limit` öğeyi karşılaştırır.
pub async fn compare_recent(
    p: &Pipeline,
    pool: &PgPool,
    minutes: i64,
    limit: i64,
) -> Result<Report, sqlx::Error> {
    let mut r = Report::default();
    for it in osiris_db::recent_items(pool, minutes, limit).await? {
        r.checked += 1;
        let cleaned = osiris_hash::clean(&it.raw_content);
        let hash = osiris_hash::content_hash(&cleaned);
        let mut ex = serde_json::Map::new();
        if cleaned != it.cleaned_content {
            r.cleaned_mismatch += 1;
            ex.insert("cleaned".into(), json!({"python": it.cleaned_content, "rust": cleaned}));
        }
        if hash != it.content_hash {
            r.hash_mismatch += 1;
            ex.insert("content_hash".into(), json!({"python": it.content_hash, "rust": hash}));
        }
        let norm = |t: &str| {
            if osiris_db::VALID_ENTITY_TYPES.contains(&t) { t.to_owned() } else { "custom".into() }
        };
        let ours: BTreeSet<(String, String)> = p
            .extract_entities(&cleaned)
            .into_iter()
            .map(|(t, v)| (norm(&t), v.chars().take(500).collect()))
            .collect();
        let theirs: BTreeSet<(String, String)> = it.entities.iter().cloned().collect();
        if ours != theirs {
            r.entity_mismatch += 1;
            let only_rust: Vec<_> = ours.difference(&theirs).collect();
            let only_py: Vec<_> = theirs.difference(&ours).collect();
            ex.insert("entities".into(), json!({"only_python": only_py, "only_rust": only_rust}));
        }
        if let (Some(e), Some(stored)) = (&p.embedder, &it.embedding)
            && let Some(v) = e.embed(&cleaned)
        {
            r.embedding_compared += 1;
            let c = cosine(&v, stored);
            if c < 0.999 {
                r.embedding_mismatch += 1;
                ex.insert("embedding_cosine".into(), json!(c));
            }
        }
        if crate::lang::detect(&cleaned) == it.language {
            r.language_agree += 1;
        }
        if !ex.is_empty() && r.examples.len() < 20 {
            ex.insert("item_id".into(), json!(it.id));
            r.examples.push(Value::Object(ex));
        }
    }
    Ok(r)
}
