//! API birim testleri (veritabanı gerekmez: havuz ulaşılamayan adrese tembel bağlanır, DB
//! anahtarı araması başarısız olur → 401). Uçtan uca davranış `tests/contract/` paketiyle
//! gerçek PostgreSQL'e karşı sınanır (CI: Rust API'ye karşı).

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use osiris_server::api::{AppState, Config, auth, router};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

const KEY: &str = "test-operator-key-0123456789";
const JWT: &str = "test-jwt-secret-0123456789";

fn config(rate: u32) -> Config {
    Config {
        api_key: KEY.into(),
        jwt_secret: JWT.into(),
        insecure_open: false,
        cors_origins: vec!["http://localhost:3000".into()],
        rate_per_min: rate,
    }
}

fn app_with(cfg: Config) -> axum::Router {
    let pool =
        osiris_db::connect_lazy("postgresql://x:y@127.0.0.1:1/z", 2, Duration::from_millis(300))
            .unwrap();
    let redis = osiris_collector::redis_manager("redis://127.0.0.1:1/0").unwrap();
    let mgr = osiris_collector::Manager::new(
        osiris_plugin::real::host(),
        Some(redis),
        Some(pool.clone()),
    );
    router(Arc::new(AppState::new(cfg, pool, mgr)))
}

fn app() -> axum::Router {
    app_with(config(0))
}

async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> (StatusCode, Value) {
    let mut b = Request::builder().method(method).uri(path);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let req = b.body(body.map_or_else(Body::empty, |s| Body::from(s.to_owned()))).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn bearer(role: &str) -> String {
    let t = auth::create_token("tester", JWT, 600, role).unwrap();
    format!("Bearer {}", t["access_token"].as_str().unwrap())
}

#[tokio::test]
async fn saglik_ve_bilinmeyen_rotalar() {
    let a = app();
    let (s, b) = call(&a, "GET", "/health", &[], None).await;
    assert_eq!((s, b), (StatusCode::OK, json!({"status": "ok", "service": "osiris-api"})));
    let (s, b) = call(&a, "GET", "/yok", &[], None).await;
    assert_eq!((s, b["detail"].as_str()), (StatusCode::NOT_FOUND, Some("Not Found")));
    let (s, _) = call(&a, "PUT", "/sources", &[], None).await;
    assert_eq!(s, StatusCode::METHOD_NOT_ALLOWED);
    let (s, b) = call(&a, "GET", "/openapi.json", &[], None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(b["paths"]["/sources/{source_id}/collect"]["post"].is_object(), "{b}");
    assert_eq!(b["paths"].as_object().unwrap().len(), 29);
    // Web arayüzü uçları belgede
    assert!(b["paths"]["/sources/{source_id}"]["patch"].is_object(), "{b}");
    assert!(b["paths"]["/sources/{source_id}"]["get"].is_object(), "{b}");
    for p in ["/search/semantic", "/graph/analysis", "/graph/export", "/reports/generate"] {
        assert!(b["paths"][p].is_object(), "{p} yok");
    }
}

#[tokio::test]
async fn kimlik_ve_rol_matrisi() {
    let a = app();
    let analyst = bearer("analyst");
    let viewer = bearer("viewer");
    // kimliksiz / bozuk anahtar (DB'ye ulaşılamaz → 401) / bozuk jeton
    for h in [vec![], vec![("x-api-key", "osiris_yanlis")], vec![("authorization", "Bearer a.b.c")]]
    {
        assert_eq!(call(&a, "GET", "/stats", &h, None).await.0, StatusCode::UNAUTHORIZED);
    }
    // viewer → analyst ucu 403, admin ucu 403
    let v = [("authorization", viewer.as_str())];
    assert_eq!(
        call(&a, "POST", "/alerts/test", &v, Some(r#"{"text":"x"}"#)).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(call(&a, "GET", "/auth/keys", &v, None).await.0, StatusCode::FORBIDDEN);
    // analyst → admin ucu 403; doğrulama hatası yetkiden SONRA (422)
    let an = [("authorization", analyst.as_str())];
    assert_eq!(call(&a, "POST", "/admin/retention/run", &an, None).await.0, StatusCode::FORBIDDEN);
    let (s, b) = call(&a, "POST", "/search", &an, Some(r#"{"query":""}"#)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(b["detail"].is_array());
    // operatör anahtarı = admin; büyük/küçük harf duyarsız "bearer"
    let op = [("x-api-key", KEY)];
    assert_eq!(
        call(&a, "POST", "/auth/keys", &op, Some(r#"{"username":"u","role":"root"}"#)).await.0,
        StatusCode::BAD_REQUEST
    );
    let lower = analyst.replacen("Bearer", "bearer", 1);
    assert_eq!(
        call(&a, "GET", "/plugins", &[("authorization", lower.as_str())], None).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn dogrulama_sirasi_ve_hatalar() {
    let a = app();
    // bozuk JSON kimlikten önce 422 (FastAPI gibi)
    let (s, b) = call(&a, "POST", "/search", &[], Some("{bozuk")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(b["detail"][0]["type"], "json_invalid");
    let op = [("x-api-key", KEY)];
    // gövdesiz POST → 422; lax int ("20"); plugin kalıbı; config ≤ 50 anahtar
    assert_eq!(call(&a, "POST", "/search", &op, None).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        call(&a, "POST", "/collect", &op, Some(r#"{"plugin_id":"YOK"}"#)).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(&a, "POST", "/collect", &op, Some(r#"{"plugin_id":"yok-boyle"}"#)).await.0,
        StatusCode::NOT_FOUND
    );
    let big = format!(r#"{{"plugin_id":"rss","config":{{"feed_url":"{}"}}}}"#, "x".repeat(21_000));
    assert_eq!(
        call(&a, "POST", "/collect", &op, Some(&big)).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    // SSRF reddi → 502, iç ayrıntı yok
    let (s, b) = call(
        &a,
        "POST",
        "/collect",
        &op,
        Some(r#"{"plugin_id":"rss","config":{"feed_url":"http://127.0.0.1/"}}"#),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_GATEWAY, "{b}");
    // DB'ye ulaşılamıyor → 500 genel ileti (sürücü iletisi sızmaz)
    let (s, b) = call(&a, "GET", "/stats", &op, None).await;
    assert_eq!(
        (s, b["detail"].as_str()),
        (StatusCode::INTERNAL_SERVER_ERROR, Some("İstatistik okunamadı"))
    );
    // UUID biçimi 404, sayfalama 422
    assert_eq!(call(&a, "GET", "/items/1;DROP", &op, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        call(&a, "GET", "/sources?limit=abc", &op, None).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(&a, "GET", "/sources?offset=-1", &op, None).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    // rapor (DB gerekmez): HTML kaçışlı
    let (s, b) = call(
        &a,
        "POST",
        "/reports/markdown",
        &op,
        Some(r#"{"title":"R","findings":[{"title":"<script>"}]}"#),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(b["markdown"].as_str().unwrap().contains("### &lt;script&gt;"));
    // 5 MiB üstü gövde → 413
    let huge = format!(r#"{{"text":"{}"}}"#, "x".repeat(6 * 1024 * 1024));
    assert_eq!(
        call(&a, "POST", "/alerts/test", &op, Some(&huge)).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn jeton_ucu() {
    let a = app();
    assert_eq!(
        call(&a, "POST", "/auth/token", &[], Some(r#"{"api_key":"yanlis"}"#)).await.0,
        StatusCode::UNAUTHORIZED
    );
    let ok = format!(r#"{{"api_key":"{KEY}","role":"analyst"}}"#);
    let (s, b) = call(&a, "POST", "/auth/token", &[], Some(&ok)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!((b["token_type"].as_str(), b["expires_in"].as_i64()), (Some("bearer"), Some(3600)));
    let bad = format!(r#"{{"api_key":"{KEY}","role":"root"}}"#);
    assert_eq!(call(&a, "POST", "/auth/token", &[], Some(&bad)).await.0, StatusCode::BAD_REQUEST);
    let mut cfg = config(0);
    cfg.jwt_secret.clear();
    assert_eq!(
        call(&app_with(cfg), "POST", "/auth/token", &[], Some(&ok)).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn kota_ve_baslatma_denetimi() {
    let a = app_with(config(2));
    let op = [("x-api-key", KEY)];
    assert_eq!(call(&a, "GET", "/plugins", &op, None).await.0, StatusCode::OK);
    assert_eq!(call(&a, "GET", "/plugins", &op, None).await.0, StatusCode::OK);
    let (s, b) = call(&a, "GET", "/plugins", &op, None).await;
    assert_eq!(
        (s, b["detail"].as_str()),
        (StatusCode::TOO_MANY_REQUESTS, Some("İstek kotası aşıldı"))
    );
    // başka kimliğin kotası ayrı
    let v = bearer("viewer");
    assert_eq!(
        call(&a, "GET", "/plugins", &[("authorization", v.as_str())], None).await.0,
        StatusCode::OK
    );

    let mut cfg = config(0);
    cfg.api_key.clear();
    assert!(cfg.check_startup().is_err());
    cfg.insecure_open = true;
    assert!(cfg.check_startup().is_ok());
    // açık mod: kimliksiz admin
    assert_eq!(call(&app_with(cfg), "GET", "/plugins", &[], None).await.0, StatusCode::OK);
}

#[tokio::test]
async fn cors_izin_listesi() {
    let a = app();
    let req = |origin: &str| {
        Request::builder()
            .method("OPTIONS")
            .uri("/plugins")
            .header(header::ORIGIN, origin)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "x-api-key")
            .body(Body::empty())
            .unwrap()
    };
    let ok = a.clone().oneshot(req("http://localhost:3000")).await.unwrap();
    assert_eq!(ok.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "http://localhost:3000");
    assert_eq!(ok.headers()[header::ACCESS_CONTROL_MAX_AGE], "600");
    let no = a.oneshot(req("https://kotu.example")).await.unwrap();
    assert!(no.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
}

#[tokio::test]
async fn web_arayuzu_uclari_dogrulama_ve_rbac() {
    let a = app();
    let viewer = bearer("viewer");
    let analyst = bearer("analyst");
    let v = [("authorization", viewer.as_str())];
    let an = [("authorization", analyst.as_str())];
    let sid = "12345678-1234-1234-1234-123456789abc";
    let path = format!("/sources/{sid}");
    // PATCH: rol → gövde (422) → kimlik (404) → boş alan (422) → boyut (413)
    let (s, _) = call(&a, "PATCH", &path, &v, Some(r#"{"enabled":true}"#)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = call(&a, "PATCH", &path, &an, Some("{}")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _) = call(&a, "PATCH", &path, &an, Some(r#"{"priority":11}"#)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, b) = call(&a, "PATCH", &path, &an, Some(r#"{"enabled":null}"#)).await;
    assert_eq!(
        (s, b["detail"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("enabled boş olamaz"))
    );
    let (s, _) = call(&a, "PATCH", "/sources/bozuk", &an, Some(r#"{"enabled":true}"#)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let big = format!(r#"{{"config":{{"k":"{}"}}}}"#, "x".repeat(21_000));
    let (s, _) = call(&a, "PATCH", &path, &an, Some(&big)).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    let (s, _) = call(&a, "GET", "/sources/bozuk", &v, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Semantik arama: model yoksa 503 (doğrulama önce)
    let (s, _) = call(&a, "POST", "/search/semantic", &v, Some(r#"{"query":""}"#)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, b) = call(&a, "POST", "/search/semantic", &v, Some(r#"{"query":"fidye"}"#)).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert!(b["detail"].as_str().unwrap().contains("OSIRIS_EMBEDDING_MODEL"));
}

#[tokio::test]
async fn graf_analizi_ve_disa_aktarim() {
    let a = app();
    let analyst = bearer("analyst");
    let h = [("authorization", analyst.as_str())];
    for (s, t) in [("ali", "a.com"), ("ali", "b.com"), ("ali", "c.com"), ("x", "y")] {
        let body = json!({"source": s, "target": t, "relation_type": "r"}).to_string();
        let (st, _) = call(&a, "POST", "/graph/relation", &h, Some(&body)).await;
        assert_eq!(st, StatusCode::OK);
    }
    let (s, g) = call(&a, "GET", "/graph/analysis?top=2&node=ali", &h, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!((g["nodes"].as_u64(), g["edges"].as_u64()), (Some(6), Some(4)));
    assert_eq!(g["centrality"][0]["id"], "ali");
    assert_eq!(g["centrality"][0]["degree"], 3);
    assert_eq!(g["centrality"].as_array().unwrap().len(), 2);
    let sizes: Vec<u64> =
        g["communities"].as_array().unwrap().iter().map(|c| c["size"].as_u64().unwrap()).collect();
    assert_eq!(sizes, [4, 2]);
    assert_eq!(g["neighbors"], json!(["a.com", "b.com", "c.com"]));
    let (_, g) = call(&a, "GET", "/graph/analysis", &h, None).await;
    assert!(g["neighbors"].is_null());
    for (fmt, marker) in [("graphml", "<graphml"), ("gexf", "<gexf")] {
        let req = Request::builder()
            .uri(format!("/graph/export?format={fmt}"))
            .header("authorization", analyst.as_str())
            .body(Body::empty())
            .unwrap();
        let resp = a.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let cd = resp.headers()[header::CONTENT_DISPOSITION].to_str().unwrap().to_owned();
        assert!(cd.contains(&format!("osiris-graf.{fmt}")));
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&bytes).contains(marker));
    }
    let (s, _) = call(&a, "GET", "/graph/export?format=pdf", &h, None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn cok_bicimli_rapor() {
    let a = app();
    let analyst = bearer("analyst");
    let h = [("authorization", analyst.as_str())];
    let base = json!({
        "title": "Vaka <1>", "scope": "S", "summary": "O",
        "findings": [{"title": "<script>x</script>", "description": "d"}],
        "sources": ["https://k"],
        "entities": [{"type": "ip", "value": "1.2.3.4"}, {"type": "person", "value": "Ali"}],
    });
    let body_for = |fmt: &str| {
        let mut b = base.clone();
        b["format"] = json!(fmt);
        b.to_string()
    };
    let (s, md) = call(&a, "POST", "/reports/generate", &h, Some(&body_for("markdown"))).await;
    assert_eq!(s, StatusCode::OK);
    assert!(md["content"].as_str().unwrap().starts_with("# Vaka"));
    assert_eq!(md["filename"], "Vaka-1.md");
    let (_, html) = call(&a, "POST", "/reports/generate", &h, Some(&body_for("html"))).await;
    let c = html["content"].as_str().unwrap();
    assert!(c.contains("&lt;script&gt;") && !c.contains("<script>x"));
    assert_eq!(html["media_type"], "text/html");
    let (_, js) = call(&a, "POST", "/reports/generate", &h, Some(&body_for("json"))).await;
    let js: Value = serde_json::from_str(js["content"].as_str().unwrap()).unwrap();
    assert_eq!(js["entities"][0], json!({"type": "ip", "value": "1.2.3.4"}));
    assert_eq!(js["sources"], json!(["https://k"]));
    let (_, stix) = call(&a, "POST", "/reports/generate", &h, Some(&body_for("stix"))).await;
    let stix: Value = serde_json::from_str(stix["content"].as_str().unwrap()).unwrap();
    let kinds: Vec<&str> =
        stix["objects"].as_array().unwrap().iter().map(|o| o["type"].as_str().unwrap()).collect();
    assert_eq!((stix["type"].as_str(), kinds), (Some("bundle"), vec!["identity", "indicator"]));
    let (s, _) = call(&a, "POST", "/reports/generate", &h, Some(&body_for("pdf"))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let tr = json!({"title": "Dış yüzey ŞİRKET"}).to_string();
    let (_, out) = call(&a, "POST", "/reports/generate", &h, Some(&tr)).await;
    assert_eq!(out["filename"], "Dis-yuzey-SIRKET.md");
    let viewer = bearer("viewer");
    let (s, _) =
        call(&a, "POST", "/reports/generate", &[("authorization", &viewer)], Some(&tr)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}
