//! `plugins/tor` (.onion, SOCKS5h) ve `plugins/i2p` (eepsite, HTTP proxy).

use crate::common::{cfg_str, meta};
use async_trait::async_trait;
use osiris_hash::html::{self, ROOT};
use osiris_plugin::proxy::resolve_proxy;
use osiris_plugin::{Collector, FetchOptions, Host, PluginCrash, Proxy, fetch};
use osiris_security::UrlPolicy;
use osiris_types::pyval::trunc;
use osiris_types::{CollectedItem, CollectionResult, Config, Manifest};
use serde_json::json;

const MAX_BYTES: usize = 2_000_000;

#[derive(Clone, Copy)]
enum Net {
    Tor,
    I2p,
}

impl Net {
    fn proxy(self, host: &Host, cfg: &Config) -> Result<Proxy, String> {
        match self {
            Net::Tor => resolve_proxy(
                cfg.get("socks5_host"),
                cfg.get("socks5_port"),
                host.env.tor_socks.as_deref(),
                "OSIRIS_TOR_SOCKS",
                9050,
                "SOCKS5",
            )
            .map(|(host, port)| Proxy::Socks5h { host, port }),
            Net::I2p => resolve_proxy(
                cfg.get("http_proxy_host"),
                cfg.get("http_proxy_port"),
                host.env.i2p_http_proxy.as_deref(),
                "OSIRIS_I2P_HTTP_PROXY",
                4444,
                "I2P",
            )
            .map(|(host, port)| Proxy::Http { host, port }),
        }
    }

    fn policy(self) -> UrlPolicy {
        match self {
            Net::Tor => UrlPolicy { allow_onion: true, allow_i2p: false },
            Net::I2p => UrlPolicy { allow_onion: false, allow_i2p: true },
        }
    }

    fn name(self) -> &'static str {
        match self {
            Net::Tor => "tor",
            Net::I2p => "i2p",
        }
    }
}

const UA: &str = "OSIRIS-OSINT/0.1 (+self-hosted)";

async fn collect(net: Net, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash> {
    let Some(url) = cfg_str(cfg, "url") else { return Ok(CollectionResult::fail("url gerekli")) };
    if matches!(net, Net::Tor) && !url.contains(".onion") {
        return Ok(CollectionResult::fail(".onion adresi gerekli"));
    }
    let proxy = match net.proxy(host, cfg) {
        Ok(p) => p,
        Err(e) => return Ok(CollectionResult::fail(e)),
    };
    let mut opts = FetchOptions::new("GET", url)
        .timeout_secs(60)
        .max_bytes(MAX_BYTES)
        .header("User-Agent", UA);
    opts.policy = net.policy();
    opts.proxy = Some(proxy);
    let resp = match fetch(host, opts).await {
        Ok(r) => r,
        Err(e) => return Ok(CollectionResult::fail(trunc(&e.to_string(), 500))),
    };
    if let Err(e) = resp.raise_for_status() {
        return Ok(CollectionResult::fail(trunc(&e.to_string(), 500)));
    }
    let doc = html::parse(&trunc(&resp.text(), MAX_BYTES))
        .map_err(|e| PluginCrash(format!("ParserRejectedMarkup: {e}")))?;
    let item = CollectedItem {
        url: Some(trunc(url, 2048)),
        title: doc.safe_title(),
        raw_content: trunc(&doc.text(ROOT), 50_000),
        published_at: None,
        metadata: meta(json!({ "network": net.name() })),
        tags: Vec::new(),
    };
    Ok(CollectionResult::ok(vec![item], meta(json!({ "url": url, "network": net.name() }))))
}

async fn health(net: Net, host: &Host, cfg: &Config) -> bool {
    let Some(url) = cfg_str(cfg, "url") else { return false };
    let Ok(proxy) = net.proxy(host, cfg) else { return false };
    let mut opts =
        FetchOptions::new("HEAD", url).timeout_secs(30).max_bytes(0).header("User-Agent", UA);
    opts.policy = net.policy();
    opts.proxy = Some(proxy);
    fetch(host, opts).await.is_ok_and(|r| r.status < 500)
}

pub struct Tor(pub Manifest);
pub struct I2p(pub Manifest);

#[async_trait]
impl Collector for Tor {
    fn manifest(&self) -> &Manifest {
        &self.0
    }

    async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash> {
        collect(Net::Tor, host, cfg).await
    }

    async fn health_check(&self, host: &Host, cfg: &Config) -> bool {
        health(Net::Tor, host, cfg).await
    }
}

#[async_trait]
impl Collector for I2p {
    fn manifest(&self) -> &Manifest {
        &self.0
    }

    async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash> {
        collect(Net::I2p, host, cfg).await
    }

    async fn health_check(&self, host: &Host, cfg: &Config) -> bool {
        health(Net::I2p, host, cfg).await
    }
}
