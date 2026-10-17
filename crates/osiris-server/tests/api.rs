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
    assert_eq!(b["paths"].as_object().unwrap().len(), 24);
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
