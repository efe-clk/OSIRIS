//! `fixtures/alert` altın verileri (eşleşme, sessize alma, z-skor) + işleyiciler.

use osiris_alert::handlers::{EmailHandler, TelegramHandler, WebhookHandler};
use osiris_alert::{AlertManager, AnomalyOptions, Handler};
use osiris_hash::Json;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn get<'a>(v: &'a Json, k: &str) -> Option<&'a Json> {
    match v {
        Json::Object(e) => e.iter().find(|(key, _)| key == k).map(|(_, v)| v),
        _ => None,
    }
}

fn arr(v: Option<&Json>) -> Vec<Json> {
    match v {
        Some(Json::Array(a)) => a.clone(),
        _ => Vec::new(),
    }
}

fn s(v: Option<&Json>) -> String {
    match v {
        Some(Json::Str(x)) => x.clone(),
        _ => String::new(),
    }
}

#[test]
fn alert_fixtures() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/alert/cases.json");
    let data = Json::parse_py(&std::fs::read_to_string(path).unwrap()).unwrap();
    let cases = arr(get(&data, "cases"));
    assert!(cases.len() >= 7);
    for case in &cases {
        let name = s(get(case, "name"));
        let o = get(case, "options");
        let opts = AnomalyOptions::from_py(
            o.and_then(|o| get(o, "anomaly_window")),
            o.and_then(|o| get(o, "anomaly_threshold")),
            o.and_then(|o| get(o, "anomaly_min_samples")),
        );
        let m = AlertManager::new(None, opts);
        for q in arr(get(case, "muted")) {
            m.mute(&s(Some(&q)));
        }
        let exp = get(case, "expected").unwrap();
        if let Some(item) = get(case, "item") {
            let got = m.match_item(item, &arr(get(case, "queries")));
            assert_eq!(Json::Array(got), *get(exp, "triggered").unwrap(), "{name}: triggered");
        }
        if get(case, "metrics").is_some() {
            let metric = get(case, "metric").map_or("m".to_owned(), |v| s(Some(v)));
            let mut results = Vec::new();
            for step in arr(get(case, "metrics")) {
                let value = get(&step, "value").unwrap();
                if matches!(get(&step, "record"), Some(Json::Bool(false))) {
                    results.push(m.anomaly(&metric, value).unwrap_or(Json::Null));
                } else {
                    m.record_metric(&metric, value);
                }
            }
            assert_eq!(Json::Array(results), *get(exp, "anomalies").unwrap(), "{name}: anomalies");
        }
    }
    let m = AlertManager::new(None, AnomalyOptions::default());
    m.mute("q");
    m.unmute("q");
    let q = Json::parse_py(r#"[{"id":"q","query_text":"a","alert_enabled":true}]"#).unwrap();
    let Json::Array(q) = q else { unreachable!() };
    let item = Json::parse_py(r#"{"cleaned_content":"a"}"#).unwrap();
    assert_eq!(m.match_item(&item, &q).len(), 1);
}

/// Tek isteği kabul edip 200 dönen yerel HTTP sunucusu; gelen isteği döndürür.
async fn one_shot() -> (String, tokio::task::JoinHandle<String>) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut sock, _) = l.accept().await.unwrap();
        let mut buf = vec![0u8; 65536];
        let mut got = Vec::new();
        loop {
            let n = sock.read(&mut buf).await.unwrap();
            got.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&got).to_string();
            if let Some(head) = text.find("\r\n\r\n") {
                let len = text[..head]
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase().strip_prefix("content-length: ").map(str::to_owned)
                    })
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if got.len() >= head + 4 + len {
                    break;
                }
            }
            if n == 0 {
                break;
            }
        }
        sock.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok")
            .await
            .unwrap();
        String::from_utf8_lossy(&got).to_string()
    });
    (url, task)
}

fn alert() -> Json {
    Json::parse_py(
        r#"{"kind":"match","query_id":"1","query_name":"fidye","item_id":null,"matched":"fidye"}"#,
    )
    .unwrap()
}

#[tokio::test]
async fn webhook_ve_telegram() {
    let (url, task) = one_shot().await;
    WebhookHandler::new(&format!("{url}/hook")).handle(&alert()).await.unwrap();
    let req = task.await.unwrap();
    assert!(req.starts_with("POST /hook "), "{req}");
    assert!(req.contains("\"query_name\":\"fidye\""), "{req}");

    let (url, task) = one_shot().await;
    TelegramHandler::with_api_base("123:ABC", "42", &url).handle(&alert()).await.unwrap();
    let req = task.await.unwrap();
    assert!(req.starts_with("POST /bot123:ABC/sendMessage "), "{req}");
    assert!(req.contains("\"chat_id\":\"42\""), "{req}");
    assert!(req.contains("OSIRIS uyarısı: fidye — eşleşme: fidye"), "{req}");

    // ulaşılamayan hedef: hata iletisinde jeton/URL yok
    let err = TelegramHandler::with_api_base("gizli-jeton", "1", "http://127.0.0.1:9")
        .handle(&alert())
        .await
        .unwrap_err();
    assert!(!err.contains("gizli-jeton"), "{err}");
}

#[test]
fn eposta_iletisi() {
    let h = EmailHandler::new(
        "smtp://127.0.0.1:2525",
        "OSIRIS <osiris@example.com>",
        "a@example.com, b@example.com",
    )
    .unwrap();
    let raw = String::from_utf8(h.message(&alert()).unwrap().formatted()).unwrap();
    assert!(raw.contains("To: a@example.com, b@example.com"), "{raw}");
    assert!(raw.contains("\"query_name\": \"fidye\""), "{raw}");
    assert!(EmailHandler::new("smtp://127.0.0.1", "bozuk", "a@example.com").is_err());
    assert!(EmailHandler::new("smtp://127.0.0.1", "a@example.com", " ").is_err());
}
