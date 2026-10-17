//! `plugins/irc` — kanal mesajları (ham TCP; SSRF denetimli bağlantı, doğrulanmış IP'ye).

use crate::common::{meta, splitlines};
use async_trait::async_trait;
use osiris_plugin::net::RecvError;
use osiris_plugin::{Collector, Host, PluginCrash};
use osiris_types::pyval::{PyConvError, py_int, py_str, trunc};
use osiris_types::{CollectedItem, CollectionResult, Config, Manifest};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

pub struct Irc(pub Manifest);

#[async_trait]
impl Collector for Irc {
    fn manifest(&self) -> &Manifest {
        &self.0
    }

    async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash> {
        let server = cfg.get("server");
        let channels_v = cfg.get("channels").filter(|v| osiris_types::pyval::truthy(v));
        let (Some(Value::String(server)), Some(channels_v)) =
            (server.filter(|v| osiris_types::pyval::truthy(v)), channels_v)
        else {
            return Ok(CollectionResult::fail("server ve channels gerekli"));
        };
        // channels[:10] — dize ise karakterler, dict ise Python TypeError (yakalanmaz)
        let raw_channels: Vec<String> = match channels_v {
            Value::Array(a) => a.iter().take(10).map(py_str).collect(),
            Value::String(s) => s.chars().take(10).map(String::from).collect(),
            _ => return Err(PluginCrash("TypeError: unhashable type: 'slice'".into())),
        };
        let cfg_err = |e: String| CollectionResult::fail(format!("Geçersiz config: {e}"));
        let server = match osiris_security::sanitize_hostname(server, 253) {
            Ok(s) => s,
            Err(e) => return Ok(cfg_err(e.0)),
        };
        let mut channels = Vec::new();
        for c in &raw_channels {
            match osiris_security::sanitize_channel(c) {
                Ok(c) => channels.push(c.to_owned()),
                Err(e) => return Ok(cfg_err(e.0)),
            }
        }
        let nick_raw = cfg.get("nickname").map_or("osiris-bot".into(), py_str);
        let nick = match osiris_security::sanitize_irc_token(&nick_raw, 64) {
            Ok(n) => n.to_owned(),
            Err(e) => return Ok(cfg_err(e.0)),
        };
        let port = match cfg.get("port").map_or(Ok(6667), py_int) {
            Ok(p) => p,
            Err(PyConvError::Invalid) => return Ok(CollectionResult::fail("Geçersiz port")),
            Err(PyConvError::Overflow) => return Err(PluginCrash("OverflowError".into())),
        };
        if !(1..=65535).contains(&port) {
            return Ok(CollectionResult::fail("Geçersiz port"));
        }
        let mut conn = match host.safe_connect(&server, port, Duration::from_secs(15)).await {
            Ok(c) => c,
            Err(Ok(value_err)) => return Ok(cfg_err(value_err)),
            Err(Err(os_err)) => return Ok(CollectionResult::fail(trunc(&os_err, 500))),
        };
        let mut send = vec![format!("NICK {nick}\r\n"), format!("USER {nick} 0 * :OSIRIS\r\n")];
        send.extend(channels.iter().map(|c| format!("JOIN {c}\r\n")));
        for line in send {
            if let Err(e) = conn.send_all(line.as_bytes()).await {
                return Ok(CollectionResult::fail(trunc(&e, 500)));
            }
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut items = Vec::new();
        while Instant::now() < deadline && items.len() < 200 {
            let data = match conn.recv(4096, Duration::from_secs(5)).await {
                Ok(d) => d,
                Err(RecvError::Timeout) => break,
                Err(RecvError::Io(e)) => return Ok(CollectionResult::fail(trunc(&e, 500))),
            };
            if data.is_empty() {
                break;
            }
            let text = String::from_utf8_lossy(&data).into_owned();
            for line in splitlines(&text).into_iter().take(100) {
                if line.starts_with("PING") {
                    let token = line.split_once(' ').map_or("", |(_, t)| t);
                    let _ = conn.send_all(format!("PONG {token}\r\n").as_bytes()).await;
                    continue;
                }
                if line.contains(" PRIVMSG ") {
                    let mut it = CollectedItem::new(trunc(&line, 2000));
                    it.metadata = meta(json!({ "server": server, "protocol": "irc" }));
                    items.push(it);
                }
            }
        }
        Ok(CollectionResult::ok(items, meta(json!({ "server": server }))))
    }

    async fn health_check(&self, host: &Host, cfg: &Config) -> bool {
        let Some(Value::String(server)) = cfg.get("server") else { return false };
        let Ok(server) = osiris_security::sanitize_hostname(server, 253) else { return false };
        let Ok(port) = cfg.get("port").map_or(Ok(6667), py_int) else { return false };
        host.safe_connect(&server, port, Duration::from_secs(10)).await.is_ok()
    }
}
