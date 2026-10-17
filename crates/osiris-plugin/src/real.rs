//! Gerçek ağ: reqwest (HTTP), hickory (DNS), tokio (TCP), getaddrinfo (SSRF çözücüsü).
//!
//! Proxy'siz HTTP istemcisi DNS'i SSRF denetimli bir çözücüyle yapar: bağlantı yalnızca
//! denetlenmiş IP'lere açılır (Python'daki DNS rebinding penceresi kapanır). Proxy'li
//! istemcilerde (Tor `socks5h`, I2P HTTP) host adını proxy çözer.

use crate::http::{
    Proxy, Transport, TransportError, WireRequest, WireResponse, encoding_from_headers,
    url_with_params,
};
use crate::net::{Connection, Dns, DnsError, RecvError, Tcp, dnspython_ipv6, dnspython_txt};
use crate::{Host, HostEnv};
use async_trait::async_trait;
use hickory_resolver::config::{NameServerConfig, ResolverConfig};
use hickory_resolver::proto::rr::{RData, RecordType};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Gerçek ağla çalışan host.
pub fn host() -> Host {
    let transport = Arc::new(ReqwestTransport::default());
    Host {
        http: transport,
        dns: Arc::new(HickoryDns),
        tcp: Arc::new(TokioTcp),
        ssrf_resolve: Arc::new(system_resolve),
        env: HostEnv::from_env(),
    }
}

/// Python `security._host_ips`: getaddrinfo (v4+v6), tekrarlar atılır; hata → boş.
pub fn system_resolve(host: &str) -> Vec<String> {
    use osiris_security::Resolver;
    osiris_security::SystemResolver.resolve(host).unwrap_or_default()
}

/// reqwest için SSRF denetimli çözücü.
struct SafeDns;

impl reqwest::dns::Resolve for SafeDns {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let ips = tokio::task::spawn_blocking(move || {
                osiris_security::resolve_safe_ips(&host, &osiris_security::SystemResolver)
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?
            .map_err(|e| match e {
                osiris_security::CheckError::Denied(d) => {
                    Box::new(d) as Box<dyn std::error::Error + Send + Sync>
                }
                osiris_security::CheckError::Resolver(never) => match never {},
            })?;
            let addrs: Vec<SocketAddr> = ips
                .iter()
                .filter_map(|ip| ip.split('%').next()?.parse::<IpAddr>().ok())
                .map(|ip| SocketAddr::new(ip, 0))
                .collect();
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// reqwest istemcileri (proxy + zaman aşımı başına önbellekli).
#[derive(Default)]
pub struct ReqwestTransport {
    clients: Mutex<HashMap<(Option<Proxy>, u64), reqwest::Client>>,
}

fn terr(kind: &str, e: impl std::fmt::Display) -> TransportError {
    TransportError { kind: kind.to_owned(), message: e.to_string() }
}

fn classify(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "Timeout"
    } else if e.is_connect() {
        "ConnectionError"
    } else if e.is_redirect() {
        "TooManyRedirects"
    } else {
        "RequestException"
    }
}

impl ReqwestTransport {
    fn client(
        &self,
        proxy: &Option<Proxy>,
        timeout: Duration,
    ) -> Result<reqwest::Client, TransportError> {
        let key = (proxy.clone(), timeout.as_secs());
        let mut cache = self.clients.lock().map_err(|e| terr("RequestException", e))?;
        if let Some(c) = cache.get(&key) {
            return Ok(c.clone());
        }
        let mut b = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(timeout)
            .read_timeout(timeout)
            .no_proxy();
        b = match proxy {
            Some(p) => {
                b.proxy(reqwest::Proxy::all(p.url()).map_err(|e| terr("InvalidProxyURL", e))?)
            }
            None => b.dns_resolver(Arc::new(SafeDns)),
        };
        let client = b.build().map_err(|e| terr("RequestException", e))?;
        cache.insert(key, client.clone());
        Ok(client)
    }
}

#[async_trait]
impl Transport for ReqwestTransport {
    async fn send(&self, req: WireRequest) -> Result<WireResponse, TransportError> {
        let client = self.client(&req.proxy, req.timeout)?;
        let method = reqwest::Method::from_bytes(req.method.to_uppercase().as_bytes())
            .map_err(|e| terr("RequestException", e))?;
        let url = url_with_params(&req.url, &req.params);
        let mut rb = client.request(method, &url).header("Accept", "*/*");
        for (k, v) in &req.headers {
            rb = rb.header(k.as_str(), v.as_str());
        }
        if let Some(j) = &req.json {
            rb = rb
                .header("Content-Type", "application/json")
                .body(serde_json::to_vec(j).map_err(|e| terr("InvalidJSONError", e))?);
        }
        let mut resp = rb.send().await.map_err(|e| terr(classify(&e), e))?;
        let status = resp.status();
        let headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .map(|(k, v)| {
                (k.as_str().to_owned(), String::from_utf8_lossy(v.as_bytes()).into_owned())
            })
            .collect();
        let mut body = Vec::new();
        while body.len() < req.max_bytes {
            match resp.chunk().await {
                Ok(Some(c)) => body.extend_from_slice(&c),
                Ok(None) => break,
                Err(e) => return Err(terr(classify(&e), e)),
            }
        }
        body.truncate(req.max_bytes);
        let encoding = encoding_from_headers(&headers);
        Ok(WireResponse {
            status: status.as_u16(),
            reason: status.canonical_reason().unwrap_or("").to_owned(),
            headers,
            body,
            encoding,
        })
    }
}

/// hickory ile DNS; kayıtlar dnspython `to_text()` biçiminde.
pub struct HickoryDns;

fn name_text(n: &impl std::fmt::Display) -> String {
    let s = n.to_string();
    if s.ends_with('.') { s } else { format!("{s}.") }
}

fn rdata_text(r: &RData) -> Option<String> {
    Some(match r {
        RData::A(a) => a.0.to_string(),
        RData::AAAA(a) => dnspython_ipv6(a.0),
        RData::MX(mx) => format!("{} {}", mx.preference, name_text(&mx.exchange)),
        RData::NS(ns) => name_text(&ns.0),
        RData::CNAME(c) => name_text(&c.0),
        RData::PTR(p) => name_text(&p.0),
        RData::TXT(t) => dnspython_txt(&t.txt_data.iter().map(|b| b.to_vec()).collect::<Vec<_>>()),
        RData::SOA(s) => format!(
            "{} {} {} {} {} {} {}",
            name_text(&s.mname),
            name_text(&s.rname),
            s.serial,
            s.refresh,
            s.retry,
            s.expire,
            s.minimum
        ),
        RData::SRV(s) => format!("{} {} {} {}", s.priority, s.weight, s.port, name_text(&s.target)),
        _ => return None,
    })
}

#[async_trait]
impl Dns for HickoryDns {
    async fn resolve(
        &self,
        domain: &str,
        rtype: &str,
        nameservers: &[String],
        lifetime: Duration,
    ) -> Result<Vec<String>, DnsError> {
        let rt: RecordType =
            rtype.parse().map_err(|_| DnsError::Other(format!("bilinmeyen tür {rtype}")))?;
        let provider = hickory_resolver::net::runtime::TokioRuntimeProvider::default();
        let builder = if nameservers.is_empty() {
            hickory_resolver::Resolver::builder(provider)
                .map_err(|e| DnsError::Other(e.to_string()))?
        } else {
            let ns: Vec<NameServerConfig> = nameservers
                .iter()
                .filter_map(|s| s.parse::<IpAddr>().ok())
                .map(NameServerConfig::udp_and_tcp)
                .collect();
            hickory_resolver::Resolver::builder_with_config(
                ResolverConfig::from_parts(None, vec![], ns),
                provider,
            )
        };
        let mut builder = builder;
        builder.options_mut().timeout = lifetime;
        builder.options_mut().attempts = 1;
        let resolver = builder.build().map_err(|e| DnsError::Other(e.to_string()))?;
        let fqdn = if domain.ends_with('.') { domain.to_owned() } else { format!("{domain}.") };
        let lookup = tokio::time::timeout(lifetime, resolver.lookup(fqdn.as_str(), rt))
            .await
            .map_err(|_| {
                DnsError::Other(format!(
                    "The resolution lifetime expired after {:.3} seconds",
                    lifetime.as_secs_f64()
                ))
            })?;
        match lookup {
            Ok(l) => {
                let out: Vec<String> = l
                    .answers()
                    .iter()
                    .filter(|r| r.record_type() == rt)
                    .filter_map(|r| rdata_text(&r.data))
                    .collect();
                if out.is_empty() { Err(DnsError::NoAnswer) } else { Ok(out) }
            }
            Err(e) if e.is_nx_domain() => Err(DnsError::NxDomain),
            Err(e) if e.is_no_records_found() => Err(DnsError::NoAnswer),
            Err(e) => Err(DnsError::Other(e.to_string())),
        }
    }
}

/// tokio TCP.
pub struct TokioTcp;

struct TokioConn(tokio::net::TcpStream);

#[async_trait]
impl Connection for TokioConn {
    async fn send_all(&mut self, data: &[u8]) -> Result<(), String> {
        self.0.write_all(data).await.map_err(|e| e.to_string())
    }

    async fn recv(&mut self, n: usize, timeout: Duration) -> Result<Vec<u8>, RecvError> {
        let mut buf = vec![0u8; n];
        match tokio::time::timeout(timeout, self.0.read(&mut buf)).await {
            Err(_) => Err(RecvError::Timeout),
            Ok(Err(e)) => Err(RecvError::Io(e.to_string())),
            Ok(Ok(k)) => {
                buf.truncate(k);
                Ok(buf)
            }
        }
    }
}

#[async_trait]
impl Tcp for TokioTcp {
    async fn connect(
        &self,
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> Result<Box<dyn Connection>, String> {
        let addr = (host, port);
        match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr)).await {
            Err(_) => Err("timed out".into()),
            Ok(Err(e)) => Err(e.to_string()),
            Ok(Ok(s)) => Ok(Box::new(TokioConn(s))),
        }
    }
}
