//! OSIRIS plugin arayüzü (R5) — `Collector` trait'i ve plugin'lere sunulan host yetenekleri.
//!
//! **Plugin'ler ağa doğrudan erişmez.** HTTP yalnızca [`http::fetch`] (her adımda SSRF
//! denetimi, yönlendirme sınırı, zaman aşımı, gövde sınırı), DNS [`net::Dns`], ham TCP
//! [`Host::safe_connect`] (çöz → denetle → doğrulanmış IP'ye bağlan) üzerinden gider.
//! Gerçek ağ uygulaması [`real`], fixture oynatıcısı [`replay`] modülündedir.

pub mod http;
pub mod net;
pub mod proxy;
pub mod real;
pub mod replay;

use async_trait::async_trait;
use osiris_security::{CheckError, UrlPolicy};
use osiris_types::{CollectionResult, Config, Manifest};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub use http::{FetchOptions, HttpError, Proxy, Response, Transport, fetch};
pub use net::{Connection, Dns, DnsError, Tcp};

/// SSRF denetiminin kullandığı DNS (host → IP dizeleri; çözülemezse boş).
pub type SsrfResolveFn = Arc<dyn Fn(&str) -> Vec<String> + Send + Sync>;

struct FnResolver(SsrfResolveFn);

impl osiris_security::Resolver for FnResolver {
    type Error = std::convert::Infallible;

    fn resolve(&self, host: &str) -> Result<Vec<String>, Self::Error> {
        Ok((self.0)(host))
    }
}

/// Operatör ortamı (proxy adresleri, izinli dizinler) — env'den bir kez okunur.
#[derive(Debug, Clone, Default)]
pub struct HostEnv {
    /// `OSIRIS_TOR_SOCKS` (`host:port`)
    pub tor_socks: Option<String>,
    /// `OSIRIS_I2P_HTTP_PROXY` (`host:port`)
    pub i2p_http_proxy: Option<String>,
    /// `OSIRIS_USERNAME_DB_DIR`
    pub username_db_dir: Option<PathBuf>,
}

impl HostEnv {
    pub fn from_env() -> Self {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        Self {
            tor_socks: var("OSIRIS_TOR_SOCKS"),
            i2p_http_proxy: var("OSIRIS_I2P_HTTP_PROXY"),
            username_db_dir: var("OSIRIS_USERNAME_DB_DIR").map(PathBuf::from),
        }
    }
}

/// Plugin'e verilen yetenekler.
#[derive(Clone)]
pub struct Host {
    pub http: Arc<dyn Transport>,
    pub dns: Arc<dyn Dns>,
    pub tcp: Arc<dyn Tcp>,
    pub ssrf_resolve: SsrfResolveFn,
    pub env: HostEnv,
}

impl Host {
    /// `assert_safe_url` (DNS engelleyici olabileceği için ayrı iş parçacığında).
    pub async fn assert_safe_url(&self, url: &str, policy: UrlPolicy) -> Result<String, HttpError> {
        let resolver = FnResolver(self.ssrf_resolve.clone());
        let url = url.to_owned();
        let res = tokio::task::spawn_blocking(move || {
            osiris_security::assert_safe_url(&url, policy, &resolver)
        })
        .await
        .map_err(|e| HttpError::Value(format!("SSRF denetimi çalışmadı: {e}")))?;
        res.map_err(|e| match e {
            CheckError::Denied(d) => HttpError::Value(d.0),
            CheckError::Resolver(never) => match never {},
        })
    }

    /// `resolve_safe_ips`
    pub async fn resolve_safe_ips(&self, host: &str) -> Result<Vec<String>, String> {
        let resolver = FnResolver(self.ssrf_resolve.clone());
        let host = host.to_owned();
        let res = tokio::task::spawn_blocking(move || {
            osiris_security::resolve_safe_ips(&host, &resolver)
        })
        .await
        .map_err(|e| e.to_string())?;
        res.map_err(|e| match e {
            CheckError::Denied(d) => d.0,
            CheckError::Resolver(never) => match never {},
        })
    }

    /// `safe_create_connection`: host bir kez çözülür, tüm kayıtlar denetlenir, bağlantı
    /// doğrulanmış IP'lere sırayla denenir. `Err(Ok(msg))` = ValueError (güvensiz/geçersiz),
    /// `Err(Err(msg))` = OSError (bağlanılamadı).
    pub async fn safe_connect(
        &self,
        host: &str,
        port: i64,
        timeout: Duration,
    ) -> Result<Box<dyn Connection>, Result<String, String>> {
        if !(1..=65535).contains(&port) {
            return Err(Ok("Geçersiz port".into()));
        }
        let ips = self.resolve_safe_ips(host).await.map_err(Ok)?;
        let mut last = format!("Bağlanılamadı: {host}");
        for ip in ips {
            match self.tcp.connect(&ip, port as u16, timeout).await {
                Ok(c) => return Ok(c),
                Err(e) => last = e,
            }
        }
        Err(Err(last))
    }
}

/// Bir veri kaynağı plugin'i (Python `BaseCollector`).
#[async_trait]
pub trait Collector: Send + Sync {
    fn manifest(&self) -> &Manifest;

    fn id(&self) -> &str {
        &self.manifest().id
    }

    /// Veri toplama. Ağ hataları `CollectionResult::fail` ile döner; `Err` yalnızca
    /// Python'da yakalanmayan istisnalara (yönetici "plugin hatası" olarak raporlar) karşılık gelir.
    async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash>;

    async fn health_check(&self, host: &Host, cfg: &Config) -> bool;

    /// Ek yapılandırma doğrulaması (hatalı alan adları).
    fn validate_config(&self, _cfg: &Config) -> Vec<String> {
        Vec::new()
    }
}

/// Python'da plugin'in yakalamadığı istisna (yönetici: `plugin hatası: …`).
#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct PluginCrash(pub String);
