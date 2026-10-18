//! OSIRIS REST API (R6) — Python `osiris_api.main` (FastAPI) uçlarının axum karşılığı.
//!
//! Bağlayıcı tanım `tests/contract/` paketidir (değiştirilmeden yeşil). Uçlar, durum kodları,
//! hata iletileri, sayfalama sınırları, kimlik sırası ve RBAC Python ile aynıdır; farklar
//! `docs/RUST_GECIS_DURUMU.md` R6 bölümünde.
//!
//! Ek olarak (R6): API anahtarı/kimlik başına kota (`governor`, `OSIRIS_API_RATE_PER_MIN`),
//! Prometheus ölçümleri ayrı dinleyicide (`OSIRIS_METRICS_ADDR`), yapılandırılmış JSON log
//! (`OSIRIS_LOG_FORMAT=json`), OpenAPI (`/openapi.json`) + Swagger UI (`/docs`).

pub mod auth;
pub mod error;
pub mod input;
pub mod metrics;
pub mod openapi;
pub mod routes;

use auth::{Ident, constant_eq, hash_api_key, role_satisfies, verify_token};
use axum::Router;
use axum::extract::{DefaultBodyLimit, MatchedPath, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use error::{ApiError, ApiResult};
use governor::{DefaultKeyedRateLimiter, Quota};
use osiris_graph::Graph;
use sqlx::PgPool;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tower_http::cors::{AllowOrigin, CorsLayer};

/// Gövde tavanı — Nginx `client_max_body_size 5m` ile aynı.
pub const MAX_BODY_BYTES: usize = 5 * 1024 * 1024;
pub const DEFAULT_DATABASE_URL: &str = "postgresql://osiris:osiris@localhost:5432/osiris";

/// Ortamdan okunan API yapılandırması (Python `main.py` ile aynı değişkenler).
#[derive(Clone, Debug)]
pub struct Config {
    pub api_key: String,
    pub jwt_secret: String,
    pub insecure_open: bool,
    pub cors_origins: Vec<String>,
    /// Kimlik başına dakikalık istek kotası (0 = kapalı).
    pub rate_per_min: u32,
}

impl Config {
    pub fn from_env() -> Self {
        let var = |k: &str| std::env::var(k).unwrap_or_default();
        let origins =
            std::env::var("OSIRIS_CORS_ORIGINS").unwrap_or_else(|_| "http://localhost:3000".into());
        Self {
            api_key: var("OSIRIS_API_KEY"),
            // API anahtarına düşme yedeği YOK: aynı sır iki işte kullanılmamalı
            jwt_secret: var("OSIRIS_JWT_SECRET"),
            insecure_open: var("OSIRIS_INSECURE_OPEN") == "1",
            cors_origins: origins
                .split(',')
                .map(|o| o.trim().to_owned())
                .filter(|o| !o.is_empty())
                .collect(),
            rate_per_min: var("OSIRIS_API_RATE_PER_MIN").trim().parse().unwrap_or(600),
        }
    }

    /// `check_startup_config` (fail-closed): anahtar yoksa ve açık mod istenmediyse başlatma.
    pub fn check_startup(&self) -> anyhow::Result<()> {
        if self.api_key.is_empty() && !self.insecure_open {
            anyhow::bail!(
                "OSIRIS_API_KEY tanımlı değil — API başlatılmadı. Güçlü bir anahtar atayın \
                 (openssl rand -hex 32) veya yalnızca güvenilir ağda OSIRIS_INSECURE_OPEN=1 kullanın."
            );
        }
        if self.api_key.is_empty() {
            tracing::warn!(
                "OSIRIS_INSECURE_OPEN=1 — API kimlik doğrulamasız çalışıyor (yalnızca güvenilir ağda kullanın)"
            );
        }
        if self.jwt_secret.is_empty() {
            tracing::warn!("OSIRIS_JWT_SECRET tanımlı değil — JWT kapalı (/auth/token 503 döner)");
        }
        Ok(())
    }
}

/// Bellek içi graf (ilk kullanımda DB'den beslenir).
pub struct GraphState {
    pub graph: Graph,
    pub loaded: bool,
}

pub struct AppState {
    pub cfg: Config,
    pub pool: PgPool,
    pub collector: osiris_collector::Manager,
    pub graph: tokio::sync::Mutex<GraphState>,
    pub metrics: metrics::Metrics,
    limiter: Option<DefaultKeyedRateLimiter<String>>,
    /// Semantik arama sorgu vektörü için embedding modeli (ilk kullanımda yüklenir).
    embedder: tokio::sync::OnceCell<Option<Arc<osiris_pipeline::embed::Embedder>>>,
}

pub type Shared = Arc<AppState>;

impl AppState {
    pub fn new(cfg: Config, pool: PgPool, collector: osiris_collector::Manager) -> Self {
        let limiter = NonZeroU32::new(cfg.rate_per_min)
            .map(|n| DefaultKeyedRateLimiter::keyed(Quota::per_minute(n)));
        Self {
            cfg,
            pool,
            collector,
            graph: tokio::sync::Mutex::new(GraphState { graph: Graph::default(), loaded: false }),
            metrics: metrics::Metrics::default(),
            limiter,
            embedder: tokio::sync::OnceCell::new(),
        }
    }

    /// `OSIRIS_EMBEDDING_MODEL` + `$OSIRIS_MODEL_DIR` dosyalarıyla modeli bir kez yükler;
    /// tanımsız ya da yüklenemezse None (uç 503 döner, API çalışmaya devam eder).
    pub async fn embedder(&self) -> Option<Arc<osiris_pipeline::embed::Embedder>> {
        self.embedder
            .get_or_init(|| async {
                let name = std::env::var("OSIRIS_EMBEDDING_MODEL")
                    .ok()
                    .filter(|m| !m.trim().is_empty())?;
                let root = osiris_pipeline::model_dir();
                match tokio::task::spawn_blocking(move || {
                    osiris_pipeline::embed::Embedder::load(&root, name.trim())
                })
                .await
                {
                    Ok(Ok(e)) => Some(Arc::new(e)),
                    Ok(Err(e)) => {
                        tracing::warn!(
                            "Semantik arama kapalı: embedding modeli yüklenemedi: {e:#}"
                        );
                        None
                    }
                    Err(e) => {
                        tracing::warn!("Semantik arama kapalı: {e}");
                        None
                    }
                }
            })
            .await
            .clone()
    }

    /// `_lookup_db_key`: özet eşleşmesi + `last_used_at`. DB hatası → kimlik yok.
    async fn lookup_db_key(&self, raw: &str) -> Option<Ident> {
        let hash = hash_api_key(raw);
        let row: Result<Option<(String, String)>, _> = sqlx::query_as(
            "SELECT u.username, u.role::text FROM api_keys k JOIN users u ON u.id = k.user_id \
             WHERE k.key_hash = $1 AND NOT k.revoked",
        )
        .bind(&hash)
        .fetch_optional(&self.pool)
        .await;
        match row {
            Ok(Some((user, role))) => {
                if let Err(e) =
                    sqlx::query("UPDATE api_keys SET last_used_at = NOW() WHERE key_hash = $1")
                        .bind(&hash)
                        .execute(&self.pool)
                        .await
                {
                    tracing::debug!("last_used_at yazılamadı: {e}");
                }
                Some(Ident { user, role })
            }
            Ok(None) => None,
            Err(e) => {
                tracing::debug!("Anahtar araması atlandı: {e}");
                None
            }
        }
    }

    pub fn check_operator_key(&self, provided: &str) -> bool {
        !self.cfg.api_key.is_empty()
            && !provided.is_empty()
            && constant_eq(provided, &self.cfg.api_key)
    }

    /// `resolve_identity`: operatör anahtarı → Bearer JWT → DB anahtarı.
    pub async fn resolve(
        &self,
        api_key: Option<&str>,
        authorization: Option<&str>,
    ) -> Option<Ident> {
        if api_key.is_some_and(|k| self.check_operator_key(k)) {
            return Some(Ident { user: "operator".into(), role: "admin".into() });
        }
        if let Some(a) = authorization
            && a.len() > 7
            && a[..7].eq_ignore_ascii_case("bearer ")
            && let Ok((user, role)) = verify_token(a[7..].trim(), &self.cfg.jwt_secret, 60)
        {
            return Some(Ident { user, role });
        }
        match api_key {
            Some(k) if !k.is_empty() => self.lookup_db_key(k).await,
            _ => None,
        }
    }

    /// `_require_role` + kota.
    pub async fn require(&self, headers: &HeaderMap, minimum: &str) -> ApiResult<Ident> {
        if self.cfg.api_key.is_empty() {
            if self.cfg.insecure_open {
                return Ok(Ident { user: "anonymous".into(), role: "admin".into() });
            }
            // Başlatma denetimi atlatılmışsa bile kapalı kal
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "Kimlik doğrulama yapılandırılmamış",
            ));
        }
        let h = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
        let ident = self
            .resolve(h("x-api-key"), h("authorization"))
            .await
            .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "Geçersiz kimlik bilgisi"))?;
        if !role_satisfies(&ident.role, minimum) {
            return Err(ApiError::new(StatusCode::FORBIDDEN, "Yetki yetersiz"));
        }
        if let Some(l) = &self.limiter {
            let key = format!("{}:{}", ident.role, ident.user);
            if l.check_key(&key).is_err() {
                self.metrics.rate_limited.fetch_add(1, Ordering::Relaxed);
                return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "İstek kotası aşıldı"));
            }
            if l.len() > 10_000 {
                l.retain_recent();
            }
        }
        Ok(ident)
    }

    /// Best-effort denetim kaydı (doküman §10.4) — asla isteği bozmaz.
    pub async fn audit(
        &self,
        user: &str,
        action: &str,
        resource: Option<&str>,
        detail: serde_json::Value,
    ) {
        let detail = osiris_hash::Json::from(detail);
        if let Err(e) =
            osiris_db::append_audit(&self.pool, Some(user), action, resource, &detail).await
        {
            self.metrics.audit_failures.fetch_add(1, Ordering::Relaxed);
            tracing::debug!("Denetim kaydı atlandı: {e}");
        }
    }

    /// Grafı ilk kullanımda DB'den besler; yükleme başarısızsa sonraki istekte yeniden dener.
    pub async fn graph(&self) -> tokio::sync::MutexGuard<'_, GraphState> {
        let mut g = self.graph.lock().await;
        if !g.loaded {
            match osiris_db::graph_edges(&self.pool, 10_000).await {
                Ok(edges) => {
                    for (s, t, r, w) in edges {
                        let _ = g.graph.add_relation(&s, &t, &r, w);
                    }
                    g.loaded = true;
                }
                Err(e) => tracing::debug!("Graf yüklenemedi: {e}"),
            }
        }
        g
    }
}

/// İstek ölçümü + yapılandırılmış erişim logu (yol parametresi/sorgu dizgesi yazılmaz).
async fn track(State(st): State<Shared>, req: Request, next: Next) -> Response {
    let start = Instant::now();
    let method = req.method().as_str().to_owned();
    let route =
        req.extensions().get::<MatchedPath>().map_or("-".to_owned(), |m| m.as_str().to_owned());
    let resp = next.run(req).await;
    let status = resp.status().as_u16();
    let elapsed = start.elapsed();
    st.metrics.observe(&method, &route, status, elapsed);
    tracing::info!(target: "osiris_api::access", method, route, status, ms = elapsed.as_millis() as u64, "istek");
    resp
}

fn cors(cfg: &Config) -> CorsLayer {
    let origins: Vec<HeaderValue> =
        cfg.cors_origins.iter().filter_map(|o| o.parse().ok()).collect();
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
        .allow_headers([
            header::CONTENT_TYPE,
            HeaderName::from_static("x-api-key"),
            header::AUTHORIZATION,
        ])
        .max_age(Duration::from_secs(600))
}

pub fn router(state: Shared) -> Router {
    routes::routes()
        .merge(openapi::routes())
        .route_layer(axum::middleware::from_fn_with_state(state.clone(), track))
        .fallback(|| async { ApiError::not_found("Not Found") })
        .method_not_allowed_fallback(|| async {
            ApiError::new(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed").into_response()
        })
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(cors(&state.cfg))
        .with_state(state)
}

/// Prometheus dinleyicisi (`/metrics`).
pub fn metrics_router(state: Shared) -> Router {
    Router::new()
        .route(
            "/metrics",
            axum::routing::get(|State(st): State<Shared>| async move {
                (
                    [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
                    st.metrics.render(),
                )
            }),
        )
        .with_state(state)
}
