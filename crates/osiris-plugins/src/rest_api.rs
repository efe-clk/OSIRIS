//! `plugins/rest-api` — JSON API; kayıtlar kanonik JSON (SOZLESME §4).

use crate::common::{UA, cfg_str, jget, json_str, meta};
use async_trait::async_trait;
use osiris_hash::Json;
use osiris_plugin::{Collector, FetchOptions, Host, PluginCrash, fetch};
use osiris_types::pyval::{py_str, trunc};
use osiris_types::{CollectedItem, CollectionResult, Config, Manifest};
use serde_json::{Value, json};

const METHODS: [&str; 7] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

pub struct RestApi(pub Manifest);

/// `_dig`: nokta yolu; liste dizini aralık dışıysa Python IndexError (yakalanmaz).
fn dig(mut obj: Json, path: &str) -> Result<Json, PluginCrash> {
    for part in path.split('.') {
        obj = match obj {
            Json::Object(_) => jget(&obj, part).cloned().unwrap_or(Json::Null),
            Json::Array(a) if !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) => {
                let idx: usize = part.parse().unwrap_or(usize::MAX);
                a.get(idx)
                    .cloned()
                    .ok_or_else(|| PluginCrash("IndexError: list index out of range".into()))?
            }
            _ => return Ok(Json::Null),
        };
    }
    Ok(obj)
}

fn str_pairs(v: &Value) -> Vec<(String, String)> {
    v.as_object()
        .map(|m| m.iter().take(20).map(|(k, v)| (trunc(k, 200), trunc(&py_str(v), 2000))).collect())
        .unwrap_or_default()
}

#[async_trait]
impl Collector for RestApi {
    fn manifest(&self) -> &Manifest {
        &self.0
    }

    async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash> {
        let Some(endpoint) = cfg_str(cfg, "endpoint") else {
            return Ok(CollectionResult::fail("endpoint gerekli"));
        };
        let method = cfg.get("method").map_or("GET".into(), py_str).to_uppercase();
        if !METHODS.contains(&method.as_str()) {
            return Ok(CollectionResult::fail(format!("İzin verilmeyen metod: {method}")));
        }
        let empty = json!({});
        let headers = cfg.get("headers").unwrap_or(&empty);
        let params = cfg.get("params").unwrap_or(&empty);
        if !headers.is_object() || !params.is_object() {
            return Ok(CollectionResult::fail("headers/params dict olmalı"));
        }
        let mut opts = FetchOptions::new(&method, endpoint).timeout_secs(30);
        let mut hdrs: Vec<(String, String)> = vec![("User-Agent".into(), UA.into())];
        for (k, v) in str_pairs(headers) {
            match hdrs.iter_mut().find(|(k2, _)| *k2 == k) {
                Some(slot) => slot.1 = v,
                None => hdrs.push((k, v)),
            }
        }
        opts.headers = hdrs;
        opts.params = str_pairs(params);
        let resp = match fetch(host, opts).await {
            Ok(r) => r,
            Err(e) => return Ok(CollectionResult::fail(trunc(&e.to_string(), 500))),
        };
        if let Err(e) = resp.raise_for_status() {
            return Ok(CollectionResult::fail(trunc(&e.to_string(), 500)));
        }
        let Ok(mut data) = resp.json() else {
            return Ok(CollectionResult::fail("Geçersiz JSON yanıtı"));
        };
        if let Some(jp) = cfg.get("json_path").filter(|v| osiris_types::pyval::truthy(v)) {
            match jp {
                Value::String(p) if p.chars().count() <= 500 => data = dig(data, p)?,
                _ => return Ok(CollectionResult::fail("json_path geçersiz")),
            }
        }
        let records = match data {
            Json::Array(a) => a,
            other => vec![other],
        };
        let items = records
            .iter()
            .take(200)
            .map(|rec| {
                let title =
                    jget(rec, "title").filter(|t| !t.is_falsy()).map(|t| trunc(&json_str(t), 500));
                CollectedItem {
                    url: Some(trunc(endpoint, 2048)),
                    title,
                    raw_content: trunc(&osiris_hash::canonical::record_json(rec), 50_000),
                    published_at: None,
                    metadata: meta(json!({ "endpoint": trunc(endpoint, 1000) })),
                    tags: Vec::new(),
                }
            })
            .collect();
        Ok(CollectionResult::ok(items, meta(json!({ "endpoint": endpoint }))))
    }

    async fn health_check(&self, host: &Host, cfg: &Config) -> bool {
        let Some(url) = cfg_str(cfg, "endpoint") else { return false };
        let opts =
            FetchOptions::new("HEAD", url).timeout_secs(10).max_bytes(0).header("User-Agent", UA);
        fetch(host, opts).await.is_ok_and(|r| r.status < 500)
    }
}
