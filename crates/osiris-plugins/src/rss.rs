//! `plugins/rss` — RSS/Atom feed okuyucu.

use crate::common::{cfg_str, headers_ua, int_cfg, meta};
use crate::feed;
use async_trait::async_trait;
use osiris_plugin::{Collector, FetchOptions, Host, PluginCrash, fetch};
use osiris_types::pyval::trunc;
use osiris_types::{CollectedItem, CollectionResult, Config, Manifest};
use serde_json::json;

const MAX_BYTES: usize = 2_000_000;

pub struct Rss(pub Manifest);

#[async_trait]
impl Collector for Rss {
    fn manifest(&self) -> &Manifest {
        &self.0
    }

    async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash> {
        let Some(feed_url) = cfg_str(cfg, "feed_url") else {
            return Ok(CollectionResult::fail("feed_url gerekli"));
        };
        let mut opts = FetchOptions::new("GET", feed_url).timeout_secs(30).max_bytes(MAX_BYTES);
        opts.headers = headers_ua();
        let resp = match fetch(host, opts).await {
            Ok(r) => r,
            Err(e) => return Ok(CollectionResult::fail(trunc(&e.to_string(), 500))),
        };
        if let Err(e) = resp.raise_for_status() {
            return Ok(CollectionResult::fail(trunc(&e.to_string(), 500)));
        }
        let parsed = feed::parse(&resp.body[..resp.body.len().min(MAX_BYTES)]);
        if parsed.bozo && parsed.entries.is_empty() {
            return Ok(CollectionResult::fail("Geçersiz feed"));
        }
        let max_items = int_cfg(cfg, "max_items", 50, 1, 200)?.clamp(1, 200) as usize;
        let items = parsed
            .entries
            .iter()
            .take(max_items)
            .map(|e| {
                let opt = |v: &Option<String>, n: usize| {
                    Some(trunc(v.as_deref().unwrap_or(""), n)).filter(|s| !s.is_empty())
                };
                CollectedItem {
                    url: opt(&e.link, 2048),
                    title: opt(&e.title, 500),
                    raw_content: trunc(e.summary.as_deref().unwrap_or(""), 50_000),
                    published_at: opt(&e.published, 100),
                    metadata: meta(json!({ "feed": trunc(feed_url, 1000) })),
                    tags: Vec::new(),
                }
            })
            .collect();
        Ok(CollectionResult::ok(items, meta(json!({ "feed": feed_url }))))
    }

    async fn health_check(&self, host: &Host, cfg: &Config) -> bool {
        let Some(url) = cfg_str(cfg, "feed_url") else { return false };
        let mut opts = FetchOptions::new("HEAD", url).timeout_secs(10).max_bytes(0);
        opts.headers = headers_ua();
        fetch(host, opts).await.is_ok_and(|r| r.status < 500)
    }
}
