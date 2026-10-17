//! `plugins/web-scraper` — genel web sayfası (bs4 html.parser + soupsieve seçici).

use crate::common::{cfg_str, meta};
use async_trait::async_trait;
use osiris_hash::html::{self, ROOT};
use osiris_plugin::{Collector, FetchOptions, Host, PluginCrash, fetch};
use osiris_types::pyval::trunc;
use osiris_types::{CollectedItem, CollectionResult, Config, Manifest};
use serde_json::{Value, json};

const UA: &str = "OSIRIS-OSINT/0.1 (+self-hosted; contact: admin@localhost)";
const MAX_BYTES: usize = 2_000_000;

pub struct WebScraper(pub Manifest);

#[async_trait]
impl Collector for WebScraper {
    fn manifest(&self) -> &Manifest {
        &self.0
    }

    async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash> {
        let Some(url) = cfg_str(cfg, "url") else {
            return Ok(CollectionResult::fail("url gerekli"));
        };
        let opts = FetchOptions::new("GET", url)
            .timeout_secs(30)
            .max_bytes(MAX_BYTES)
            .header("User-Agent", UA);
        let resp = match fetch(host, opts).await {
            Ok(r) => r,
            Err(e) => return Ok(CollectionResult::fail(trunc(&e.to_string(), 500))),
        };
        if let Err(e) = resp.raise_for_status() {
            return Ok(CollectionResult::fail(trunc(&e.to_string(), 500)));
        }
        let text = trunc(&resp.text(), MAX_BYTES);
        let doc =
            html::parse(&text).map_err(|e| PluginCrash(format!("ParserRejectedMarkup: {e}")))?;
        // `soup.select(selector)[:20] if selector else [soup]`; seçici hatası → [soup]
        let nodes: Vec<usize> = match cfg.get("css_selector") {
            Some(v) if osiris_types::pyval::truthy(v) => match v {
                Value::String(sel) => match doc.select(sel) {
                    Ok(n) => n.into_iter().take(20).collect(),
                    Err(_) => vec![ROOT],
                },
                _ => vec![ROOT],
            },
            _ => vec![ROOT],
        };
        let title = doc.safe_title();
        let depth = cfg.get("depth").cloned().unwrap_or(json!(1));
        let mut items = Vec::new();
        for n in nodes {
            let node_text = trunc(&doc.text(n), 50_000);
            if node_text.is_empty() {
                continue;
            }
            items.push(CollectedItem {
                url: Some(trunc(url, 2048)),
                title: title.clone(),
                raw_content: node_text,
                published_at: None,
                metadata: meta(json!({ "depth": depth })),
                tags: Vec::new(),
            });
            if items.len() >= 20 {
                break;
            }
        }
        Ok(CollectionResult::ok(items, meta(json!({ "url": url }))))
    }

    async fn health_check(&self, host: &Host, cfg: &Config) -> bool {
        let Some(url) = cfg_str(cfg, "url") else { return false };
        let opts =
            FetchOptions::new("HEAD", url).timeout_secs(10).max_bytes(0).header("User-Agent", UA);
        fetch(host, opts).await.is_ok_and(|r| r.status < 500)
    }
}
