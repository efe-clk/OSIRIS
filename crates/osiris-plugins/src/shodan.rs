//! `plugins/shodan` — host arama API'si; API anahtarı hataya/metadata'ya yazılmaz.

use crate::common::{UA, jget, json_str, meta, to_value};
use async_trait::async_trait;
use osiris_hash::Json;
use osiris_plugin::{Collector, FetchOptions, Host, HttpError, PluginCrash, fetch};
use osiris_types::pyval::{py_str, trunc, truthy};
use osiris_types::{CollectedItem, CollectionResult, Config, Manifest};
use serde_json::{Value, json};

pub struct Shodan(pub Manifest);

fn shodan_err(e: &HttpError) -> CollectionResult {
    if !e.is_request_exception() {
        return CollectionResult::fail(trunc(&format!("Shodan hatası: {e}"), 300));
    }
    match e.status() {
        Some(401) => CollectionResult::fail("Shodan: yetkisiz (API anahtarı geçersiz)"),
        Some(429) => CollectionResult::fail("Shodan: hız limiti aşıldı"),
        _ => CollectionResult::fail(format!("Shodan hatası: {}", e.type_name())),
    }
}

#[async_trait]
impl Collector for Shodan {
    fn manifest(&self) -> &Manifest {
        &self.0
    }

    async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash> {
        let api_key = cfg.get("api_key").filter(|v| truthy(v));
        let query = match cfg.get("query") {
            Some(Value::String(q)) if !q.is_empty() => q,
            _ => return Ok(CollectionResult::fail("api_key ve query gerekli")),
        };
        let Some(api_key) = api_key else {
            return Ok(CollectionResult::fail("api_key ve query gerekli"));
        };
        if query.chars().count() > 500 {
            return Ok(CollectionResult::fail("query çok uzun"));
        }
        let mut opts = FetchOptions::new("GET", "https://api.shodan.io/shodan/host/search")
            .timeout_secs(30)
            .header("User-Agent", UA);
        opts.params = vec![("key".into(), py_str(api_key)), ("query".into(), query.clone())];
        let resp = match fetch(host, opts).await {
            Ok(r) => r,
            Err(e) => return Ok(shodan_err(&e)),
        };
        if let Err(e) = resp.raise_for_status() {
            return Ok(shodan_err(&e));
        }
        let Ok(payload) = resp.json() else {
            return Ok(CollectionResult::fail("Shodan: geçersiz yanıt"));
        };
        if !matches!(payload, Json::Object(_)) {
            return Err(PluginCrash("AttributeError: 'get'".into()));
        }
        let matches: Vec<Json> = match jget(&payload, "matches") {
            None => Vec::new(),
            Some(Json::Array(a)) => a.iter().take(100).cloned().collect(),
            Some(Json::Str(_)) => Vec::new(), // dize dilimi: karakterler dict değil → atlanır
            Some(_) => return Err(PluginCrash("TypeError: unhashable type: 'slice'".into())),
        };
        let mut items = Vec::new();
        for m in &matches {
            if !matches!(m, Json::Object(_)) {
                continue;
            }
            let ip = jget(m, "ip_str").cloned().unwrap_or(Json::Null);
            let port = jget(m, "port").cloned().unwrap_or(Json::Null);
            let product = jget(m, "product").map(json_str).unwrap_or_default();
            items.push(CollectedItem {
                url: Some(trunc(&format!("http://{}:{}", json_str(&ip), json_str(&port)), 2048)),
                title: Some(trunc(&product, 500)).filter(|s| !s.is_empty()),
                raw_content: trunc(&osiris_hash::canonical::record_json(m), 50_000),
                published_at: None,
                metadata: meta(json!({ "ip": to_value(&ip), "port": to_value(&port) })),
                tags: Vec::new(),
            });
        }
        let count = items.len();
        Ok(CollectionResult::ok(items, meta(json!({ "query": query, "count": count }))))
    }

    async fn health_check(&self, _host: &Host, cfg: &Config) -> bool {
        cfg.get("api_key").is_some_and(truthy)
    }
}
