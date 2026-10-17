//! `plugins/matrix` — oda mesaj geçmişi (client-server API v3).

use crate::common::{UA, jget, json_str, meta};
use async_trait::async_trait;
use osiris_hash::Json;
use osiris_plugin::http::quote_all;
use osiris_plugin::{Collector, FetchOptions, Host, HttpError, PluginCrash, fetch};
use osiris_security::UrlPolicy;
use osiris_types::pyval::{py_str, trunc, truthy};
use osiris_types::{CollectedItem, CollectionResult, Config, Manifest};
use serde_json::json;

pub struct Matrix(pub Manifest);

fn matrix_err(e: &HttpError) -> CollectionResult {
    if !e.is_request_exception() {
        return CollectionResult::fail(trunc(&format!("Matrix hatası: {e}"), 300));
    }
    match e.status() {
        Some(401 | 403) => CollectionResult::fail("Matrix: yetkisiz"),
        _ => CollectionResult::fail(format!("Matrix hatası: {}", e.type_name())),
    }
}

#[async_trait]
impl Collector for Matrix {
    fn manifest(&self) -> &Manifest {
        &self.0
    }

    async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash> {
        let (Some(hs), Some(room)) =
            (crate::common::cfg_str(cfg, "homeserver"), crate::common::cfg_str(cfg, "room_id"))
        else {
            return Ok(CollectionResult::fail("homeserver ve room_id gerekli"));
        };
        let base = hs.trim_end_matches('/');
        if let Err(e) = host.assert_safe_url(&format!("{base}/"), UrlPolicy::default()).await {
            return Ok(CollectionResult::fail(format!("Güvensiz homeserver: {e}")));
        }
        if room.chars().count() > 500 || room.contains(['\r', '\n', '\0']) {
            return Ok(CollectionResult::fail("Geçersiz room_id"));
        }
        let url = format!("{base}/_matrix/client/v3/rooms/{}/messages", quote_all(room));
        let mut opts = FetchOptions::new("GET", &url).timeout_secs(30).header("User-Agent", UA);
        if let Some(tok) = cfg.get("access_token").filter(|v| truthy(v)) {
            opts.headers.push(("Authorization".into(), format!("Bearer {}", py_str(tok))));
        }
        opts.params = vec![("dir".into(), "b".into()), ("limit".into(), "50".into())];
        let resp = match fetch(host, opts).await {
            Ok(r) => r,
            Err(e) => return Ok(matrix_err(&e)),
        };
        if let Err(e) = resp.raise_for_status() {
            return Ok(matrix_err(&e));
        }
        let Ok(payload) = resp.json() else {
            return Ok(CollectionResult::fail("Matrix: geçersiz yanıt"));
        };
        if !matches!(payload, Json::Object(_)) {
            return Err(PluginCrash("AttributeError: 'get'".into()));
        }
        let chunk: Vec<Json> = match jget(&payload, "chunk") {
            None => Vec::new(),
            Some(Json::Array(a)) => a.iter().take(100).cloned().collect(),
            Some(Json::Str(_)) => Vec::new(),
            Some(_) => return Err(PluginCrash("TypeError: unhashable type: 'slice'".into())),
        };
        let mut items = Vec::new();
        for ev in &chunk {
            if !matches!(ev, Json::Object(_))
                || jget(ev, "type") != Some(&Json::Str("m.room.message".into()))
            {
                continue;
            }
            let body = match jget(ev, "content") {
                Some(c @ Json::Object(_)) => {
                    jget(c, "body").cloned().unwrap_or(Json::Str(String::new()))
                }
                _ => Json::Str(String::new()),
            };
            if body.is_falsy() {
                continue;
            }
            let sender = jget(ev, "sender").map(json_str).unwrap_or_default();
            let mut it = CollectedItem::new(trunc(&json_str(&body), 20_000));
            it.metadata = meta(json!({ "room": trunc(room, 500), "sender": trunc(&sender, 500) }));
            items.push(it);
        }
        Ok(CollectionResult::ok(items, meta(json!({ "room": room }))))
    }

    async fn health_check(&self, host: &Host, cfg: &Config) -> bool {
        let Some(hs) = crate::common::cfg_str(cfg, "homeserver") else { return false };
        let base = hs.trim_end_matches('/');
        let opts = FetchOptions::new("GET", &format!("{base}/_matrix/client/versions"))
            .timeout_secs(10)
            .max_bytes(65536)
            .header("User-Agent", UA);
        host.assert_safe_url(&format!("{base}/"), UrlPolicy::default()).await.is_ok()
            && fetch(host, opts).await.is_ok_and(|r| r.status < 500)
    }
}
