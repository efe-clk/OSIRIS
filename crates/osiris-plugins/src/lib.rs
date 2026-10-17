//! OSIRIS'in 11 collector plugin'i (R5) — Python `plugins/*/collector.py` ile aynı davranış.
//!
//! `manifest.json` dosyaları derleme zamanında gömülür. Her plugin `fixtures/plugins/`
//! senaryolarını geçer (`tests/fixtures.rs`).

pub use osiris_feed as feed;

pub mod blockchain;
pub mod common;
pub mod darkweb;
pub mod dns_whois;
pub mod irc;
pub mod matrix;
pub mod rest_api;
pub mod rss;
pub mod shodan;
pub mod username_search;
pub mod web_scraper;

use osiris_plugin::Collector;
use osiris_types::Manifest;
use std::sync::Arc;

macro_rules! manifest {
    ($dir:literal) => {
        serde_json::from_str::<Manifest>(include_str!(concat!(
            "../../../plugins/",
            $dir,
            "/manifest.json"
        )))
        .expect(concat!($dir, " manifest"))
    };
}

/// Tüm plugin'ler (Python yöneticisinin yükleme sırası: dizin adına göre).
pub fn all() -> Vec<Arc<dyn Collector>> {
    vec![
        Arc::new(blockchain::Blockchain(manifest!("blockchain"))),
        Arc::new(dns_whois::DnsWhois(manifest!("dns-whois"))),
        Arc::new(darkweb::I2p(manifest!("i2p"))),
        Arc::new(irc::Irc(manifest!("irc"))),
        Arc::new(matrix::Matrix(manifest!("matrix"))),
        Arc::new(rest_api::RestApi(manifest!("rest-api"))),
        Arc::new(rss::Rss(manifest!("rss"))),
        Arc::new(shodan::Shodan(manifest!("shodan"))),
        Arc::new(darkweb::Tor(manifest!("tor"))),
        Arc::new(username_search::UsernameSearch(manifest!("username-search"))),
        Arc::new(web_scraper::WebScraper(manifest!("web-scraper"))),
    ]
}

/// Kimliğe göre plugin.
pub fn by_id(id: &str) -> Option<Arc<dyn Collector>> {
    all().into_iter().find(|p| p.id() == id)
}
