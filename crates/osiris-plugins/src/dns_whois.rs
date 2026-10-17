//! `plugins/dns-whois` — DNS kayıtları + WHOIS (whois.iana.org:43).

use crate::common::meta;
use async_trait::async_trait;
use osiris_plugin::{Collector, DnsError, Host, PluginCrash, net::RecvError};
use osiris_types::pyval::{py_str, trunc};
use osiris_types::{CollectedItem, CollectionResult, Config, Manifest};
use serde_json::{Value, json};
use std::time::Duration;

const ALLOWED: [&str; 9] = ["A", "AAAA", "MX", "NS", "TXT", "CNAME", "SOA", "PTR", "SRV"];

pub struct DnsWhois(pub Manifest);

/// `_whois`: Python'da ASCII olmayan alan adı `encode("ascii")` ile çöker (yakalanmaz).
async fn whois(host: &Host, domain: &str) -> Result<Option<String>, PluginCrash> {
    let domain = osiris_security::sanitize_domain(domain).map_err(|e| PluginCrash(e.0))?;
    if !domain.is_ascii() {
        return Err(PluginCrash("UnicodeEncodeError: 'ascii' codec can't encode".into()));
    }
    let Ok(mut conn) = host.tcp.connect("whois.iana.org", 43, Duration::from_secs(10)).await else {
        return Ok(None);
    };
    if conn.send_all(format!("{domain}\r\n").as_bytes()).await.is_err() {
        return Ok(None);
    }
    let mut data = Vec::new();
    while data.len() < 50_000 {
        match conn.recv(4096, Duration::from_secs(10)).await {
            Ok(chunk) if chunk.is_empty() => break,
            Ok(chunk) => data.extend_from_slice(&chunk),
            Err(RecvError::Timeout) => break,
            Err(RecvError::Io(_)) => return Ok(None),
        }
    }
    Ok(Some(String::from_utf8_lossy(&data).into_owned()))
}

#[async_trait]
impl Collector for DnsWhois {
    fn manifest(&self) -> &Manifest {
        &self.0
    }

    async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash> {
        let Some(raw) = crate::common::cfg_str(cfg, "domain") else {
            return Ok(CollectionResult::fail("domain gerekli"));
        };
        let domain = match osiris_security::sanitize_domain(raw) {
            Ok(d) => d,
            Err(e) => return Ok(CollectionResult::fail(format!("Geçersiz domain: {e}"))),
        };
        let default = json!(["A", "AAAA", "MX", "NS", "TXT"]);
        let Value::Array(rt) = cfg.get("record_types").unwrap_or(&default) else {
            return Ok(CollectionResult::fail("record_types liste olmalı"));
        };
        let mut types: Vec<String> = rt
            .iter()
            .take(10)
            .map(|r| py_str(r).to_uppercase())
            .filter(|r| ALLOWED.contains(&r.as_str()))
            .collect();
        if types.is_empty() {
            types = vec!["A".into()];
        }
        let nameservers: Vec<String> = match cfg.get("nameservers") {
            Some(Value::Array(ns)) if !ns.is_empty() => {
                ns.iter().take(5).map(|n| trunc(&py_str(n), 100)).collect()
            }
            _ => Vec::new(),
        };
        let mut items = Vec::new();
        for rtype in &types {
            match host.dns.resolve(&domain, rtype, &nameservers, Duration::from_secs(10)).await {
                Ok(answers) => {
                    for a in answers.iter().take(50) {
                        let mut it = CollectedItem::new(trunc(a, 5000));
                        it.metadata = meta(json!({ "domain": domain, "record_type": rtype }));
                        items.push(it);
                    }
                }
                Err(DnsError::NoAnswer) => continue,
                Err(DnsError::NxDomain) => return Ok(CollectionResult::fail("Domain bulunamadı")),
                Err(DnsError::Other(e)) => return Ok(CollectionResult::fail(trunc(&e, 500))),
            }
        }
        if let Some(text) = whois(host, &domain).await?
            && !text.is_empty()
        {
            let mut it = CollectedItem::new(trunc(&text, 20_000));
            it.metadata = meta(json!({ "domain": domain, "record_type": "WHOIS" }));
            items.push(it);
        }
        items.truncate(100);
        Ok(CollectionResult::ok(items, meta(json!({ "domain": domain }))))
    }

    async fn health_check(&self, _host: &Host, cfg: &Config) -> bool {
        let d = cfg.get("domain").map(py_str).unwrap_or_default();
        osiris_security::sanitize_hostname(&d, 253).is_ok()
            && cfg.get("domain").is_some_and(osiris_types::pyval::truthy)
    }
}
