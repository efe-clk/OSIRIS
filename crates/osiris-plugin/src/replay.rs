//! `fixtures/plugins/*.json` oynatıcısı — Python `tests/golden/harness.py` ile aynı kurallar.
//!
//! HTTP: istek `{method, url, params?, json?}` henüz tüketilmemiş ilk eşdeğer kayıtla eşleşir
//! (sıra bağımsız); beklenmeyen istek hatadır. DNS: türe göre liste / `NoAnswer` / `NXDOMAIN`.
//! SSRF çözücüsü her host'u `93.184.216.34`'e çözer. Soketler sırayla verilir; açılan
//! adresler raporlanır.

use crate::http::{Transport, TransportError, WireRequest, WireResponse};
use crate::net::{Connection, Dns, DnsError, RecvError, Tcp};
use crate::{Host, HostEnv};
use async_trait::async_trait;
use base64::Engine;
use serde_json::{Map, Value, json};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Fixture'larda tüm DNS sorguları bu genel IP'ye çözülür.
pub const PUBLIC_IP: &str = "93.184.216.34";

pub struct Replay {
    exchanges: Mutex<Vec<(Value, bool)>>,
    dns: Map<String, Value>,
    sockets: Mutex<VecDeque<Vec<Vec<u8>>>>,
    connects: Mutex<Vec<(String, u16)>>,
    unexpected: Mutex<Vec<Value>>,
}

impl Replay {
    pub fn from_case(case: &Value) -> Arc<Self> {
        let exchanges = case["http"].as_array().cloned().unwrap_or_default();
        let sockets = case["sockets"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|s| {
                        s["recv"]
                            .as_array()
                            .map(|c| {
                                c.iter()
                                    .map(|x| x.as_str().unwrap_or("").as_bytes().to_vec())
                                    .collect()
                            })
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .unwrap_or_default();
        Arc::new(Self {
            exchanges: Mutex::new(exchanges.into_iter().map(|e| (e, false)).collect()),
            dns: case["dns"].as_object().cloned().unwrap_or_default(),
            sockets: Mutex::new(sockets),
            connects: Mutex::new(Vec::new()),
            unexpected: Mutex::new(Vec::new()),
        })
    }

    /// Bu oynatıcıya bağlı host (ortam boş: proxy env'i yok).
    pub fn host(self: &Arc<Self>) -> Host {
        Host {
            http: self.clone(),
            dns: self.clone(),
            tcp: self.clone(),
            ssrf_resolve: Arc::new(|_h: &str| vec![PUBLIC_IP.to_owned()]),
            env: HostEnv::default(),
        }
    }

    /// `{"http_unconsumed": n, "connects": [[host, port], …]}`
    pub fn report(&self) -> Value {
        let unconsumed = self.exchanges.lock().unwrap().iter().filter(|(_, used)| !used).count();
        let connects: Vec<Value> =
            self.connects.lock().unwrap().iter().map(|(h, p)| json!([h, p])).collect();
        json!({ "http_unconsumed": unconsumed, "connects": connects })
    }

    /// Beklenmeyen (eşleşmeyen) istekler — boş olmalı.
    pub fn unexpected(&self) -> Vec<Value> {
        self.unexpected.lock().unwrap().clone()
    }
}

#[async_trait]
impl Transport for Replay {
    async fn send(&self, req: WireRequest) -> Result<WireResponse, TransportError> {
        let mut got = Map::new();
        got.insert("method".into(), json!(req.method.to_uppercase()));
        got.insert("url".into(), json!(req.url));
        if !req.params.is_empty() {
            let p: Map<String, Value> =
                req.params.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
            got.insert("params".into(), Value::Object(p));
        }
        if let Some(j) = &req.json {
            got.insert("json".into(), j.clone());
        }
        let got = Value::Object(got);
        let mut ex = self.exchanges.lock().unwrap();
        let found = ex.iter_mut().find(|(e, used)| {
            if *used {
                return false;
            }
            let r = &e["request"];
            let want: Map<String, Value> = ["method", "url", "params", "json"]
                .iter()
                .filter_map(|k| r.get(*k).map(|v| (k.to_string(), v.clone())))
                .collect();
            Value::Object(want) == got
        });
        let Some((e, used)) = found else {
            self.unexpected.lock().unwrap().push(got.clone());
            return Err(TransportError {
                kind: "AssertionError".into(),
                message: format!("beklenmeyen istek: {got}"),
            });
        };
        *used = true;
        let res = &e["response"];
        if let Some(err) = res.get("error").and_then(Value::as_str) {
            return Err(TransportError {
                kind: err.to_owned(),
                message: "fixture ağ hatası".into(),
            });
        }
        let body = match res.get("body_b64").and_then(Value::as_str) {
            Some(b) => base64::engine::general_purpose::STANDARD.decode(b).unwrap_or_default(),
            None => res.get("body").and_then(Value::as_str).unwrap_or("").as_bytes().to_vec(),
        };
        let headers = res["headers"]
            .as_object()
            .map(|h| {
                h.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_owned())).collect()
            })
            .unwrap_or_default();
        Ok(WireResponse {
            status: res.get("status").and_then(Value::as_u64).unwrap_or(200) as u16,
            reason: res.get("reason").and_then(Value::as_str).unwrap_or("").to_owned(),
            headers,
            body,
            encoding: Some(
                res.get("encoding").and_then(Value::as_str).unwrap_or("utf-8").to_owned(),
            ),
        })
    }
}

#[async_trait]
impl Dns for Replay {
    async fn resolve(
        &self,
        _domain: &str,
        rtype: &str,
        _nameservers: &[String],
        _lifetime: Duration,
    ) -> Result<Vec<String>, DnsError> {
        match self.dns.get(rtype) {
            Some(Value::Array(a)) => {
                Ok(a.iter().map(|v| v.as_str().unwrap_or("").to_owned()).collect())
            }
            Some(Value::String(s)) if s == "NXDOMAIN" => Err(DnsError::NxDomain),
            _ => Err(DnsError::NoAnswer),
        }
    }
}

struct ReplayConn(VecDeque<Vec<u8>>);

#[async_trait]
impl Connection for ReplayConn {
    async fn send_all(&mut self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }

    async fn recv(&mut self, _n: usize, _timeout: Duration) -> Result<Vec<u8>, RecvError> {
        Ok(self.0.pop_front().unwrap_or_default())
    }
}

#[async_trait]
impl Tcp for Replay {
    async fn connect(
        &self,
        host: &str,
        port: u16,
        _timeout: Duration,
    ) -> Result<Box<dyn Connection>, String> {
        self.connects.lock().unwrap().push((host.to_owned(), port));
        match self.sockets.lock().unwrap().pop_front() {
            Some(chunks) => Ok(Box::new(ReplayConn(chunks.into()))),
            None => Err("fixture: soket yok".into()),
        }
    }
}
