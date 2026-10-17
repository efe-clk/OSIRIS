//! Collector servisi (R5) — Python `CollectorManager.run_collection` + gerçek cron zamanlaması.
//!
//! - Görev yapılandırması API ile aynı kurulur: `metadata.config` + `source_id`; kaynağın
//!   `url`'si `url`/`feed_url`/`endpoint` alanlarına (yoksa) yazılır.
//! - Zorunlu alan denetimi (manifest `required`) ve plugin doğrulaması.
//! - Çökme izolasyonu: her toplama ayrı `tokio::spawn`; panik ya da `PluginCrash` →
//!   `plugin hatası: …` (yönetici düşmez).
//! - Öğeler kuyruk mesajı olarak Stream'e (`MAXLEN ~`), 512 000 bayttan büyükler atlanır.
//! - Kaynak sağlığı: `sources` + `source_metrics`.
//! - Zamanlama (E2): `sources.schedule` (5 alanlı standart cron, UTC; haftanın günü 0/7 =
//!   Pazar). **Varsayılan kapalı**: `OSIRIS_SCHEDULER=on` ile açılır (kullanıcı kararı).

use chrono::{DateTime, Utc};
use osiris_db::ScheduledSource;
use osiris_pipeline::stream;
use osiris_plugin::{Collector, Host};
use osiris_types::{CollectionResult, Config, QueueMessage};
use redis::aio::MultiplexedConnection;
use serde_json::Value;
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

pub const MAX_MESSAGE_BYTES: usize = 512_000;
static UUID: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"\A[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\n?\z",
    )
    .expect("uuid")
});
static CRON_PART: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\A[\d,/*-]+\z").expect("cron"));

pub struct Manager {
    pub plugins: HashMap<String, Arc<dyn Collector>>,
    pub host: Host,
    pub redis: Option<MultiplexedConnection>,
    pub pool: Option<PgPool>,
}

/// API'nin görev yapılandırması (`main.py` collect_source).
pub fn task_config(src: &ScheduledSource) -> Config {
    let meta = match &src.metadata {
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        other => other.clone(),
    };
    let mut cfg = match meta.get("config") {
        Some(Value::Object(c)) => c.clone(),
        _ => Config::new(),
    };
    cfg.insert("source_id".into(), Value::String(src.id.clone()));
    if let Some(url) = src.url.as_ref().filter(|u| !u.is_empty()) {
        for key in ["url", "feed_url", "endpoint"] {
            cfg.entry(key.to_owned()).or_insert_with(|| Value::String(url.clone()));
        }
    }
    cfg
}

/// 5 alanlı cron (Python `_cron_trigger` doğrulaması + standart anlam).
pub fn parse_cron(expr: &str) -> Result<croner::Cron, String> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 || parts.iter().any(|p| !CRON_PART.is_match(p)) {
        return Err(format!("Geçersiz cron (5 alan gerekli): {expr:?}"));
    }
    croner::Cron::from_str(&parts.join(" ")).map_err(|e| e.to_string())
}

impl Manager {
    pub fn new(host: Host, redis: Option<MultiplexedConnection>, pool: Option<PgPool>) -> Self {
        let plugins = osiris_plugins::all().into_iter().map(|p| (p.id().to_owned(), p)).collect();
        Self { plugins, host, redis, pool }
    }

    /// `run_collection(plugin_id, config)`; `enqueue = false` → kuru çalıştırma.
    pub async fn run_collection(
        &self,
        plugin_id: &str,
        cfg: Config,
        enqueue: bool,
    ) -> anyhow::Result<CollectionResult> {
        let plugin = self
            .plugins
            .get(plugin_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Plugin bulunamadı: {plugin_id}"))?;
        let mut missing = plugin.manifest().missing_required(&cfg);
        for m in plugin.validate_config(&cfg) {
            if !missing.contains(&m) {
                missing.push(m);
            }
        }
        if !missing.is_empty() {
            return Ok(CollectionResult::fail(format!(
                "Zorunlu alanlar eksik: {}",
                missing.join(", ")
            )));
        }
        let start = Instant::now();
        let host = self.host.clone();
        let task_cfg = cfg.clone();
        let p = plugin.clone();
        let joined = tokio::spawn(async move { p.collect(&host, &task_cfg).await }).await;
        let elapsed_ms = start.elapsed().as_millis() as i64;
        let mut result = match joined {
            Ok(Ok(r)) => r,
            Ok(Err(crash)) => {
                tracing::error!("{plugin_id}: plugin çöktü: {crash}");
                return Ok(CollectionResult::fail(format!("plugin hatası: {crash}")));
            }
            Err(join) => {
                tracing::error!("{plugin_id}: plugin paniği: {join}");
                return Ok(CollectionResult::fail(format!("plugin hatası: {join}")));
            }
        };
        if result.success && enqueue {
            let source_id = match cfg.get("source_id") {
                Some(Value::String(s)) if UUID.is_match(s) => Some(s.to_lowercase()),
                _ => None,
            };
            let mut enqueued = 0;
            if let Some(con) = &self.redis {
                let mut con = con.clone();
                for item in &result.items {
                    let collected_at = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs_f64())
                        .unwrap_or(0.0);
                    let msg = QueueMessage {
                        plugin_id: plugin_id.to_owned(),
                        source_id: source_id.clone(),
                        item: item.clone(),
                        collected_at,
                    };
                    let Ok(payload) = serde_json::to_string(&msg) else { continue };
                    if payload.len() > MAX_MESSAGE_BYTES {
                        tracing::warn!("{plugin_id}: öğe çok büyük, atlandı");
                        continue;
                    }
                    if let Err(e) = stream::enqueue(&mut con, stream::STREAM, &payload).await {
                        tracing::error!("{plugin_id}: kuyruğa yazılamadı: {e}");
                        result = CollectionResult {
                            success: false,
                            error: Some(format!("kuyruk hatası: {e}")),
                            ..result
                        };
                        break;
                    }
                    enqueued += 1;
                }
            }
            tracing::info!(
                "{plugin_id}: {} öğe toplandı ({elapsed_ms} ms, {enqueued} kuyrukta)",
                result.items.len()
            );
        } else if !result.success {
            tracing::warn!("{plugin_id}: başarısız — {}", result.error.as_deref().unwrap_or(""));
        }
        if let (Some(pool), Some(Value::String(sid))) = (&self.pool, cfg.get("source_id"))
            && let Err(e) = osiris_db::record_health(
                pool,
                sid,
                result.success,
                elapsed_ms,
                result.items.len() as i64,
            )
            .await
        {
            tracing::debug!("Sağlık kaydı yazılamadı: {e}");
        }
        Ok(result)
    }

    /// Zamanlayıcı döngüsü: kaynaklar `refresh` aralığıyla yeniden okunur; zamanı gelen
    /// kaynak (aynı anda tek çalıştırma, kaçırılanlar birleştirilir) toplanır.
    pub async fn schedule_loop(
        self: Arc<Self>,
        refresh: Duration,
        tick: Duration,
    ) -> anyhow::Result<()> {
        let pool = self
            .pool
            .clone()
            .ok_or_else(|| anyhow::anyhow!("zamanlayıcı için OSIRIS_DATABASE_URL gerekli"))?;
        let running: Arc<Mutex<HashSet<String>>> = Arc::default();
        let mut next: HashMap<String, (String, DateTime<Utc>)> = HashMap::new();
        let mut sources: Vec<ScheduledSource> = Vec::new();
        let mut last_refresh: Option<Instant> = None;
        loop {
            if last_refresh.is_none_or(|t| t.elapsed() >= refresh) {
                match osiris_db::scheduled_sources(&pool).await {
                    Ok(s) => sources = s,
                    Err(e) => tracing::error!("Kaynaklar okunamadı: {e}"),
                }
                last_refresh = Some(Instant::now());
            }
            let now = Utc::now();
            for src in &sources {
                if !self.plugins.contains_key(&src.plugin_id) {
                    continue;
                }
                let cron = match parse_cron(&src.schedule) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!("{}: {e}", src.id);
                        continue;
                    }
                };
                let due = match next.get(&src.id) {
                    // ifade değiştiyse yeniden hesapla
                    Some((expr, at)) if *expr == src.schedule => *at,
                    _ => {
                        let at = cron.find_next_occurrence(&now, false).unwrap_or(now);
                        next.insert(src.id.clone(), (src.schedule.clone(), at));
                        at
                    }
                };
                if due > now {
                    continue;
                }
                let at = cron
                    .find_next_occurrence(&now, false)
                    .unwrap_or(now + chrono::Duration::hours(1));
                next.insert(src.id.clone(), (src.schedule.clone(), at));
                if !running.lock().map(|mut r| r.insert(src.id.clone())).unwrap_or(false) {
                    continue; // max_instances = 1
                }
                let (me, src, running) = (self.clone(), src.clone(), running.clone());
                tokio::spawn(async move {
                    let res = me.run_collection(&src.plugin_id, task_config(&src), true).await;
                    if let Err(e) = res {
                        tracing::error!("{}: {e:#}", src.id);
                    }
                    if let Ok(mut r) = running.lock() {
                        r.remove(&src.id);
                    }
                });
            }
            tokio::time::sleep(tick).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn gorev_yapilandirmasi_ve_cron() {
        let src = ScheduledSource {
            id: "s1".into(),
            plugin_id: "rss".into(),
            url: Some("https://f/x.xml".into()),
            schedule: "*/15 * * * *".into(),
            metadata: json!({"config": {"max_items": 5, "url": "https://keep"}}),
        };
        let c = task_config(&src);
        assert_eq!(c["feed_url"], json!("https://f/x.xml"));
        assert_eq!(c["url"], json!("https://keep"));
        assert_eq!(c["source_id"], json!("s1"));
        assert!(parse_cron("*/15 * * * *").is_ok());
        assert!(parse_cron("0 0 * * MON").is_err());
        assert!(parse_cron("* * * *").is_err());
        let sunday = parse_cron("0 12 * * 0").unwrap();
        let next = sunday
            .find_next_occurrence(&"2026-10-17T00:00:00Z".parse::<DateTime<Utc>>().unwrap(), false)
            .unwrap();
        assert_eq!(next.to_rfc3339(), "2026-10-18T12:00:00+00:00"); // 18 Ekim 2026 Pazar
    }
}
