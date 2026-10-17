//! Uyarı yöneticisi (R6) — Python `osiris_alert.AlertManager` ile aynı kurallar.
//!
//! - Eşleşme: içerik (ilk 50 000 kod noktası) ve başlık (2000) küçük harfe çevrilir; sorgu
//!   metni kırpılıp küçültülür (500); alt-dize eşleşmesi. En fazla 100 sorgu; `alert_enabled`
//!   Python doğruluk değeriyle; sessize alınanlar atlanır.
//! - Anomali: kayan pencerede (bellek içi) ortalama ve popülasyon standart sapması; yeni
//!   değer tabana **katılmaz** (önce [`AlertManager::anomaly`], sonra
//!   [`AlertManager::record_metric`]). Isınma (`min_samples`) dolmadan uyarı yok.
//! - Yayın: `PUBLISH osiris:alerts <json>` (Redis isteğe bağlı) + kayıtlı işleyiciler
//!   (webhook, Telegram, e-posta). İşleyici hatası yayını durdurmaz, yalnızca loglanır.
//!
//! Fixture: `fixtures/alert/cases.json`. Python'daki gibi API yalnızca eşleşmeyi kullanır
//! (`/alerts/test`, yayın yok); işleyiciler kütüphane olarak sunulur (E11 kararı).

use async_trait::async_trait;
use osiris_hash::Json;
use osiris_hash::pystr::{py_float, py_int, py_str};
use osiris_hash::{py_strip, truncate_chars};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;
use std::time::Duration;

pub mod handlers;

pub const DEFAULT_CHANNEL: &str = "osiris:alerts";
const MAX_QUERY_TEXT: usize = 500;
const MAX_QUERIES_PER_ITEM: usize = 100;

/// Uyarı işleyici (e-posta, webhook, Telegram…).
#[async_trait]
pub trait Handler: Send + Sync {
    fn name(&self) -> &str;
    async fn handle(&self, alert: &Json) -> Result<(), String>;
}

/// Anomali ayarları (Python kurucusundaki sınırlarla kırpılır).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnomalyOptions {
    pub window: usize,
    pub threshold: f64,
    pub min_samples: usize,
}

impl Default for AnomalyOptions {
    fn default() -> Self {
        Self { window: 100, threshold: 3.0, min_samples: 10 }
    }
}

impl AnomalyOptions {
    /// `_clamp_int` / `_clamp_float`: Python `int()`/`float()` başarısızsa varsayılan.
    pub fn from_py(
        window: Option<&Json>,
        threshold: Option<&Json>,
        min_samples: Option<&Json>,
    ) -> Self {
        let int = |v: Option<&Json>, lo: i64, hi: i64, d: i64| {
            v.and_then(py_int).map_or(d, |x| x.clamp(lo, hi)) as usize
        };
        let threshold = threshold
            .and_then(py_float)
            .filter(|f| f.is_finite())
            .map_or(3.0, |f| f.clamp(0.5, 10.0));
        Self {
            window: int(window, 10, 10_000, 100),
            threshold,
            min_samples: int(min_samples, 5, 1000, 10),
        }
    }
}

pub struct AlertManager {
    pub channel: String,
    opts: AnomalyOptions,
    redis: Option<redis::aio::ConnectionManager>,
    handlers: Vec<Box<dyn Handler>>,
    muted: Mutex<HashSet<String>>,
    baselines: Mutex<HashMap<String, VecDeque<f64>>>,
}

fn obj(entries: Vec<(&str, Json)>) -> Json {
    Json::Object(entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn get<'a>(v: &'a Json, key: &str) -> Option<&'a Json> {
    match v {
        Json::Object(e) => e.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// Python `str(x or "")`
fn str_or_empty(v: Option<&Json>) -> String {
    match v {
        Some(x) if !x.is_falsy() => py_str(x),
        _ => String::new(),
    }
}

/// `json.dumps(alert, ensure_ascii=False)` (yayın yükü; NaN/Infinity oluşmaz)
pub fn alert_payload(alert: &Json) -> String {
    to_value(alert).to_string()
}

/// Sıralı `Json` → `serde_json::Value` (anahtar sırası yükte önemsizdir).
pub fn to_value(v: &Json) -> serde_json::Value {
    match v {
        Json::Null => serde_json::Value::Null,
        Json::Bool(b) => (*b).into(),
        Json::Int(i) => (*i).into(),
        Json::BigInt(s) => s.parse::<f64>().map_or(serde_json::Value::Null, Into::into),
        Json::Float(f) => {
            serde_json::Number::from_f64(*f).map_or(serde_json::Value::Null, Into::into)
        }
        Json::Str(s) => s.clone().into(),
        Json::Array(a) => a.iter().map(to_value).collect(),
        Json::Object(e) => e.iter().map(|(k, v)| (k.clone(), to_value(v))).collect(),
    }
}

impl AlertManager {
    /// `redis = None` → yayın yok (test/çevrimdışı); işleyiciler yine çalışır.
    pub fn new(redis: Option<redis::aio::ConnectionManager>, opts: AnomalyOptions) -> Self {
        Self::with_channel(redis, DEFAULT_CHANNEL, opts)
    }

    pub fn with_channel(
        redis: Option<redis::aio::ConnectionManager>,
        channel: &str,
        opts: AnomalyOptions,
    ) -> Self {
        let opts = AnomalyOptions {
            window: opts.window.clamp(10, 10_000),
            threshold: if opts.threshold.is_finite() {
                opts.threshold.clamp(0.5, 10.0)
            } else {
                3.0
            },
            min_samples: opts.min_samples.clamp(5, 1000),
        };
        Self {
            channel: truncate_chars(channel, 200).to_owned(),
            opts,
            redis,
            handlers: Vec::new(),
            muted: Mutex::default(),
            baselines: Mutex::default(),
        }
    }

    pub fn options(&self) -> AnomalyOptions {
        self.opts
    }

    pub fn register_handler(&mut self, handler: Box<dyn Handler>) {
        self.handlers.push(handler);
    }

    pub fn mute(&self, query_id: &str) {
        if let Ok(mut m) = self.muted.lock() {
            m.insert(query_id.to_owned());
        }
    }

    pub fn unmute(&self, query_id: &str) {
        if let Ok(mut m) = self.muted.lock() {
            m.remove(query_id);
        }
    }

    fn is_muted(&self, qid: &str) -> bool {
        self.muted.lock().map(|m| m.contains(qid)).unwrap_or(false)
    }

    /// `check_item` eşleşme kısmı (yayın yapmaz). `item`/`queries` JSON nesneleri.
    pub fn match_item(&self, item: &Json, queries: &[Json]) -> Vec<Json> {
        let mut triggered = Vec::new();
        if !matches!(item, Json::Object(_)) {
            return triggered;
        }
        // Python: önce kesilir, sonra küçültülür (`str(...)[:n].lower()`)
        let text =
            |k: &str, n: usize| truncate_chars(&str_or_empty(get(item, k)), n).to_lowercase();
        let (content, title) = (text("cleaned_content", 50_000), text("title", 2000));
        for query in queries.iter().take(MAX_QUERIES_PER_ITEM) {
            if !matches!(query, Json::Object(_))
                || get(query, "alert_enabled").is_none_or(Json::is_falsy)
            {
                continue;
            }
            let qid = str_or_empty(get(query, "id"));
            if self.is_muted(&qid) {
                continue;
            }
            // sorgu metni: kırp → küçült → kes
            let needle = py_strip(&str_or_empty(get(query, "query_text"))).to_lowercase();
            let needle = truncate_chars(&needle, MAX_QUERY_TEXT);
            if !needle.is_empty() && (content.contains(needle) || title.contains(needle)) {
                let name = str_or_empty(get(query, "name"));
                triggered.push(obj(vec![
                    ("kind", Json::Str("match".into())),
                    ("query_id", Json::Str(truncate_chars(&qid, 200).to_owned())),
                    ("query_name", Json::Str(truncate_chars(&name, 200).to_owned())),
                    ("item_id", get(item, "id").cloned().unwrap_or(Json::Null)),
                    ("matched", Json::Str(truncate_chars(needle, 200).to_owned())),
                ]));
            }
        }
        triggered
    }

    /// `check_item`: eşleşir ve her uyarıyı yayınlar.
    pub async fn check_item(&self, item: &Json, queries: &[Json]) -> Vec<Json> {
        let triggered = self.match_item(item, queries);
        for alert in &triggered {
            self.emit(alert).await;
        }
        triggered
    }

    /// `record_metric` (sayı değilse ya da sonlu değilse yok sayılır).
    pub fn record_metric(&self, name: &str, value: &Json) {
        let Some(v) = py_float(value).filter(|v| v.is_finite()) else { return };
        if let Ok(mut b) = self.baselines.lock() {
            let q = b.entry(truncate_chars(name, 200).to_owned()).or_default();
            q.push_back(v);
            while q.len() > self.opts.window {
                q.pop_front();
            }
        }
    }

    /// `check_anomaly` hesabı (yayın yapmaz): z-skoru eşiği aşarsa uyarı.
    pub fn anomaly(&self, name: &str, value: &Json) -> Option<Json> {
        let v = py_float(value).filter(|v| v.is_finite())?;
        let name = truncate_chars(name, 200);
        let samples: Vec<f64> = self
            .baselines
            .lock()
            .ok()?
            .get(name)
            .map(|q| q.iter().copied().collect())
            .unwrap_or_default();
        if samples.len() < self.opts.min_samples {
            return None;
        }
        let n = samples.len() as f64;
        let mean = samples.iter().sum::<f64>() / n;
        let var = samples.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / n;
        let stdev = var.sqrt();
        let (z, anomalous) = if stdev == 0.0 {
            let anomalous = v != mean;
            (if anomalous { f64::INFINITY } else { 0.0 }, anomalous)
        } else {
            let z = (v - mean) / stdev;
            (z, z.abs() > self.opts.threshold)
        };
        if !anomalous {
            return None;
        }
        Some(obj(vec![
            ("kind", Json::Str("anomaly".into())),
            ("metric", Json::Str(name.to_owned())),
            ("value", Json::Float(v)),
            ("mean", Json::Float(mean)),
            ("stdev", Json::Float(stdev)),
            // Infinity katı JSON üretmez; sonlu değilse null
            ("z_score", if z.is_finite() { Json::Float(z) } else { Json::Null }),
            ("samples", Json::Int(samples.len() as i64)),
        ]))
    }

    /// `check_anomaly`: hesaplar ve uyarıyı yayınlar.
    pub async fn check_anomaly(&self, name: &str, value: &Json) -> Option<Json> {
        let alert = self.anomaly(name, value)?;
        self.emit(&alert).await;
        Some(alert)
    }

    /// `_emit`: Redis yayını + işleyiciler (hatalar loglanır, yutulur).
    pub async fn emit(&self, alert: &Json) {
        let payload = alert_payload(alert);
        if let Some(con) = &self.redis {
            let mut con = con.clone();
            let mut cmd = redis::cmd("PUBLISH");
            cmd.arg(&self.channel).arg(&payload);
            match tokio::time::timeout(Duration::from_secs(5), cmd.query_async::<i64>(&mut con))
                .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => tracing::warn!("Redis yayını başarısız: {e}"),
                Err(_) => tracing::warn!("Redis yayını başarısız: zaman aşımı"),
            }
        }
        for h in &self.handlers {
            if let Err(e) = h.handle(alert).await {
                tracing::error!("Uyarı işleyici hatası ({}): {e}", h.name());
            }
        }
        let label = get(alert, "query_name").or_else(|| get(alert, "metric"));
        tracing::info!("Uyarı tetiklendi: {}", label.map(py_str).unwrap_or_default());
    }
}
