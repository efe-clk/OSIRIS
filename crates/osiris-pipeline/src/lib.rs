//! OSIRIS işleme hattı (R5) — Python `osiris_pipeline.pipeline.ProcessingPipeline` karşılığı.
//!
//! Stream'den oku → temizle (`osiris-hash`) → dil (lingua) → regex (`osiris-extract`) + ONNX
//! NER → konu sınıflandırma → embedding (fastembed) → depola (`osiris-db`) → `XACK`+`XDEL`.
//! Kuyruk kuralları SOZLESME §5 ile aynıdır (eski LIST boşaltma, `XAUTOCLAIM`, ölü-mektup).
//! [`shadow`] Python'un yazdığı öğeleri yeniden hesaplayıp karşılaştırır (geçiş doğrulaması).

pub mod embed;
pub mod lang;
pub mod models;
pub mod ner;
pub mod shadow;
pub mod stream;

use osiris_db::NewItem;
use osiris_types::pyval::{py_str, trunc, truthy};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock;

pub const MAX_CONTENT_CHARS: usize = 200_000;

/// Basit anahtar kelime sınıflandırması (Python `_TOPIC_KEYWORDS`, aynı sıra).
const TOPICS: [(&str, &[&str]); 4] = [
    ("threat", &["saldırı", "attack", "exploit", "malware", "cve", "fidye", "ransomware"]),
    ("breach", &["sızıntı", "leak", "breach", "ifşa", "dump"]),
    ("vulnerability", &["zafiyet", "vulnerability", "patch", "yama"]),
    ("phishing", &["oltalama", "phishing", "dolandırıcılık", "scam"]),
];

static UUID: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"\A[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\n?\z",
    )
    .expect("uuid")
});

/// `OSIRIS_MODEL_DIR` (varsayılan `./models`).
pub fn model_dir() -> PathBuf {
    PathBuf::from(std::env::var("OSIRIS_MODEL_DIR").unwrap_or_else(|_| "models".into()))
}

/// `classify`
pub fn classify(text: &str) -> Vec<String> {
    let lowered = text.to_lowercase();
    TOPICS
        .iter()
        .filter(|(_, kws)| kws.iter().any(|k| lowered.contains(k)))
        .map(|(t, _)| (*t).to_owned())
        .collect()
}

/// İşlenmiş kayıt (Python `process_one` sonucu).
#[derive(Debug, Clone)]
pub struct Processed {
    pub plugin_id: String,
    pub item: Map<String, Value>,
    pub source_id: Option<String>,
    pub cleaned: String,
    pub language: String,
    pub content_hash: String,
    pub entities: Vec<(String, String)>,
    pub topics: Vec<String>,
    pub embedding: Option<Vec<f32>>,
}

/// Kalıcı (bozuk) kayıt — ölü-mektup akışına gider.
#[derive(Debug, Clone)]
pub struct Invalid(pub String);

pub struct Pipeline {
    pub ner: Option<ner::OnnxNer>,
    pub embedder: Option<embed::Embedder>,
}

impl Pipeline {
    /// Modeller ortamdan: `OSIRIS_NER=off` kapatır; `OSIRIS_EMBEDDING_MODEL` boşsa vektör yok.
    pub fn from_env() -> Self {
        let root = model_dir();
        let ner_off = matches!(
            std::env::var("OSIRIS_NER").unwrap_or_default().to_lowercase().as_str(),
            "off" | "0" | "false"
        );
        let ner = if ner_off || !ner::OnnxNer::available(&root) {
            tracing::info!("NER kapalı ya da model yok; yalnızca regex");
            None
        } else {
            match ner::OnnxNer::load(&root.join(ner::MODEL_SUBDIR)) {
                Ok(n) => Some(n),
                Err(e) => {
                    tracing::warn!("NER modeli yüklenemedi: {e:#}");
                    None
                }
            }
        };
        let embedder =
            std::env::var("OSIRIS_EMBEDDING_MODEL").ok().filter(|m| !m.is_empty()).and_then(|m| {
                embed::Embedder::load(&root, &m)
                    .map_err(|e| tracing::warn!("Embedding modeli yüklenemedi: {e:#}"))
                    .ok()
            });
        Self { ner, embedder }
    }

    /// Regex + NER varlıkları; `(tip, değer.lower())` tekilleştirmesi, NER regex'ten sonra.
    pub fn extract_entities(&self, text: &str) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = osiris_extract::extract_entities(text)
            .into_iter()
            .map(|e| (e.etype.to_owned(), e.value))
            .collect();
        let mut seen: HashSet<(String, String)> =
            out.iter().map(|(t, v)| (t.clone(), v.to_lowercase())).collect();
        if let Some(n) = &self.ner {
            match n.extract(text) {
                Ok(ents) => {
                    for (t, v) in ents {
                        let v: String = osiris_hash::py_strip(&v).chars().take(500).collect();
                        if !v.is_empty() && seen.insert((t.clone(), v.to_lowercase())) {
                            out.push((t, v));
                        }
                    }
                }
                Err(e) => tracing::warn!("NER başarısız, regex sonuçları kullanılıyor: {e:#}"),
            }
        }
        out
    }

    /// `process_one(raw)`
    pub fn process_one(&self, raw: &str) -> Result<Processed, Invalid> {
        let payload: Value =
            serde_json::from_str(raw).map_err(|e| Invalid(format!("Geçersiz JSON: {e}")))?;
        let Value::Object(payload) = payload else {
            return Err(Invalid("Payload dict olmalı".into()));
        };
        let item = payload.get("item");
        let plugin_id = payload.get("plugin_id").filter(|v| truthy(v));
        let (Some(Value::Object(item)), Some(plugin_id)) = (item, plugin_id) else {
            return Err(Invalid("Payload 'item' ve 'plugin_id' içermeli".into()));
        };
        let raw_content = item.get("raw_content").map_or(String::new(), py_str);
        let cleaned = osiris_hash::clean(&raw_content);
        let sid = [payload.get("source_id"), item.get("source_id")]
            .into_iter()
            .flatten()
            .find(|v| truthy(v));
        let source_id = match sid {
            Some(Value::String(s)) if UUID.is_match(s) => Some(s.clone()),
            _ => None,
        };
        Ok(Processed {
            plugin_id: py_str(plugin_id),
            item: item.clone(),
            source_id,
            language: lang::detect(&cleaned),
            entities: self.extract_entities(&cleaned),
            topics: classify(&cleaned),
            content_hash: osiris_hash::content_hash(&cleaned),
            embedding: self.embedder.as_ref().and_then(|e| e.embed(&cleaned)),
            cleaned,
        })
    }

    /// `store()` girdisi (Python `_insert_item` sınırlarıyla).
    pub fn new_item(&self, p: &Processed) -> NewItem {
        let s = |k: &str| match p.item.get(k) {
            Some(v) if truthy(v) => py_str(v),
            _ => String::new(),
        };
        // tags: item.tags + topics; dize/sayı olanlar, ilk 100 kod noktası, tekil, en fazla 50
        let raw_tags = match p.item.get("tags") {
            Some(Value::Array(a)) => a.clone(),
            _ => Vec::new(),
        };
        let mut tags: Vec<String> = Vec::new();
        for t in raw_tags
            .iter()
            .chain(p.topics.iter().map(|t| Value::String(t.clone())).collect::<Vec<_>>().iter())
        {
            if matches!(t, Value::String(_) | Value::Number(_) | Value::Bool(_)) {
                let v = trunc(&py_str(t), 100);
                if !tags.contains(&v) {
                    tags.push(v);
                }
            }
        }
        tags.truncate(50);
        let metadata = match p.item.get("metadata") {
            Some(v) if truthy(v) => v.clone(),
            _ => Value::Object(Map::new()),
        };
        NewItem {
            source_id: p.source_id.clone(),
            raw_content: trunc(&s("raw_content"), MAX_CONTENT_CHARS),
            cleaned_content: trunc(&p.cleaned, MAX_CONTENT_CHARS),
            url: trunc(&s("url"), 2048),
            title: trunc(&s("title"), 500),
            language: trunc(if p.language.is_empty() { "unknown" } else { &p.language }, 5),
            published_at: trunc(&s("published_at"), 100),
            content_hash: p.content_hash.clone(),
            metadata,
            tags,
            embedding: match (&p.embedding, &self.embedder) {
                (Some(v), Some(e)) if v.len() == embed::DIM => {
                    Some((v.clone(), trunc(&e.name, 200)))
                }
                _ => None,
            },
            entities: p.entities.clone(),
        }
    }
}

/// `--reembed`: vektörü olmayan öğeleri partiler hâlinde vektörleştirir (boş metin
/// `<model>#skip` ile işaretlenir, bir daha seçilmez). Dönen: işlenen öğe sayısı.
pub async fn reembed(
    pool: &sqlx::PgPool,
    embedder: &embed::Embedder,
    batch: i64,
) -> Result<usize, sqlx::Error> {
    let mut done = 0;
    loop {
        let rows = osiris_db::items_without_embedding(pool, batch).await?;
        if rows.is_empty() {
            return Ok(done);
        }
        for (id, text) in rows {
            osiris_db::set_embedding(pool, &id, embedder.embed(&text).as_deref(), &embedder.name)
                .await?;
            done += 1;
        }
        tracing::info!("Yeniden vektörleştirme: {done} öğe");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isleme_python_gibi() {
        let p = Pipeline { ner: None, embedder: None };
        let raw = r#"{"plugin_id":"rss","source_id":"0D9C2A4E-5F6B-4C1D-8E7F-0123456789AB","item":{"raw_content":"<p>Fidye saldırısı: a@b.com 8.8.8.8</p>","url":"https://x","title":null,"tags":["a",1,true,null,"a"]}}"#;
        let out = p.process_one(raw).unwrap();
        assert_eq!(out.cleaned, "Fidye saldırısı: a@b.com 8.8.8.8");
        assert_eq!(out.topics, vec!["threat"]);
        assert_eq!(out.source_id.as_deref(), Some("0D9C2A4E-5F6B-4C1D-8E7F-0123456789AB"));
        assert_eq!(
            out.entities,
            vec![("email".into(), "a@b.com".into()), ("ip".into(), "8.8.8.8".into())]
        );
        let ni = p.new_item(&out);
        assert_eq!(ni.tags, vec!["a", "1", "True", "threat"]);
        assert_eq!(ni.title, "");
        assert!(p.process_one("[]").is_err());
        assert!(p.process_one(r#"{"item":{}}"#).is_err());
    }
}
