//! Uçlar — Python `osiris_api/main.py` ile aynı sıra: JSON ayrıştırma → kimlik/rol →
//! alan doğrulaması (422) → iş kuralları (400/404/409/413/502/503) → yanıt; iç hatalar 500
//! (genel ileti). Denetim kaydı best-effort, yanıttan önce yazılır.
//!
//! Liste uçlarının satırları PostgreSQL'de `json_agg` ile JSON'a çevrilir (zaman damgaları
//! UTC ISO 8601 — FastAPI çıktısıyla aynı biçim).

use super::Shared;
use super::auth::{self, ROLES};
use super::error::{ApiError, ApiResult};
use super::input::{Body, LaxBool, LaxInt, PLUGIN_ID, Params, Validate, Violations, parse};
use axum::Router;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use osiris_hash::Json;
use osiris_hash::pystr::py_repr;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;
use utoipa::ToSchema;

use osiris_db::sql_ts as ts;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/health", get(health))
        .route("/auth/token", post(issue_token))
        .route("/auth/keys", post(create_api_key).get(list_api_keys))
        .route("/auth/keys/{key_id}", delete(revoke_api_key))
        .route("/plugins", get(list_plugins))
        .route("/stats", get(stats))
        .route("/items/recent", get(recent_items))
        .route("/items/{item_id}", get(item_detail).delete(delete_item))
        .route("/entities/top", get(top_entities))
        .route("/entities/erase", post(erase_entity))
        .route("/collect", post(collect))
        .route("/collect/batch", post(collect_batch))
        .route("/search", post(search))
        .route("/search/entity", post(entity_search))
        .route("/search/semantic", post(semantic_search))
        .route("/graph", get(graph))
        .route("/graph/relation", post(graph_add))
        .route("/graph/analysis", get(graph_analysis))
        .route("/graph/export", get(graph_export))
        .route("/sources", get(list_sources).post(create_source))
        .route("/sources/{source_id}", get(get_source).patch(update_source).delete(delete_source))
        .route("/sources/{source_id}/collect", post(collect_source))
        .route("/saved-queries", get(list_saved_queries).post(create_saved_query))
        .route("/saved-queries/{query_id}", delete(delete_saved_query))
        .route("/alerts/test", post(test_alerts))
        .route("/reports/markdown", post(report_markdown))
        .route("/reports/generate", post(report_generate))
        .route("/admin/retention/run", post(run_retention))
}

// ------------------------------------------------------------------------- yardımcılar

fn ok(v: Value) -> Response {
    axum::Json(v).into_response()
}

/// PostgreSQL'in ürettiği JSON metni (yeniden serileştirmeden).
fn raw_json(text: String) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], text).into_response()
}

static UUID_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
    )
    .expect("uuid")
});

/// UUID biçimi; bozuksa 404 (500 yerine temiz hata).
fn uuid_or_404<'a>(value: &'a str, label: &str) -> ApiResult<&'a str> {
    if UUID_RE.is_match(value) {
        Ok(value)
    } else {
        Err(ApiError::not_found(format!("{label} bulunamadı")))
    }
}

/// Liste uçları için ortak sayfalama (limit 1-200, offset ≥ 0).
fn page(p: &Params) -> ApiResult<(i64, i64)> {
    let (limit, offset) = (p.int("limit", 100)?, p.int("offset", 0)?);
    if offset < 0 {
        return Err(ApiError::unprocessable("Sayfalama geçersiz"));
    }
    Ok((limit.clamp(1, 200), offset))
}

fn chars(s: &str, n: usize) -> String {
    osiris_hash::truncate_chars(s, n).to_owned()
}

/// Python `len(str(config))` — yapılandırma boyut sınırı (20 000).
fn config_too_big(config: &Map<String, Value>) -> bool {
    py_repr(&Json::from(Value::Object(config.clone()))).chars().count() > 20_000
}

fn d_true() -> LaxBool {
    LaxBool(true)
}

// ------------------------------------------------------------------------------- uçlar

/// Servis durumu (kimliksiz).
#[utoipa::path(get, path = "/health", tag = "Health", responses((status = 200, description = "Çalışıyor")))]
pub async fn health() -> Response {
    ok(json!({"status": "ok", "service": "osiris-api"}))
}

fn viewer() -> String {
    "viewer".into()
}

#[derive(Deserialize, ToSchema)]
pub struct TokenRequest {
    api_key: String,
    #[serde(default = "viewer")]
    #[schema(default = "viewer")]
    role: String,
}

impl Validate for TokenRequest {
    fn validate(&self, v: &mut Violations) {
        v.len("api_key", &self.api_key, 1, 500);
        v.len("role", &self.role, 0, 32);
    }
}

/// Operatör anahtarı karşılığında kısa ömürlü JWT (1 saat).
#[utoipa::path(post, path = "/auth/token", tag = "Auth", request_body = TokenRequest,
    responses((status = 200, description = "Jeton"), (status = 401), (status = 503, description = "JWT kapalı")))]
pub async fn issue_token(State(st): State<Shared>, Body(body): Body) -> ApiResult<Response> {
    let req: TokenRequest = parse(body)?;
    if st.cfg.api_key.is_empty() || st.cfg.jwt_secret.is_empty() {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Jeton üretimi kapalı (OSIRIS_JWT_SECRET yok)",
        ));
    }
    if !st.check_operator_key(&req.api_key) {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "Geçersiz kimlik bilgisi"));
    }
    if !ROLES.contains(&req.role.as_str()) {
        return Err(ApiError::bad_request("Geçersiz rol"));
    }
    let token = auth::create_token("api-client", &st.cfg.jwt_secret, 3600, &req.role)
        .map_err(|e| ApiError::internal("Jeton üretilemedi", e))?;
    st.audit("operator", "auth.token", None, json!({"role": req.role})).await;
    Ok(ok(token))
}

#[derive(Deserialize, ToSchema)]
pub struct ApiKeyRequest {
    username: String,
    #[serde(default = "viewer")]
    role: String,
    #[serde(default)]
    name: String,
}

impl Validate for ApiKeyRequest {
    fn validate(&self, v: &mut Violations) {
        v.len("username", &self.username, 1, 200);
        v.len("role", &self.role, 0, 32);
        v.len("name", &self.name, 0, 200);
    }
}

/// Kullanıcıya API anahtarı üretir (ham değer BİR KEZ döner). Yalnızca admin.
#[utoipa::path(post, path = "/auth/keys", tag = "Auth", request_body = ApiKeyRequest,
    responses((status = 200, description = "Yeni anahtar"), (status = 400), (status = 403)))]
pub async fn create_api_key(
    State(st): State<Shared>,
    headers: HeaderMap,
    Body(body): Body,
) -> ApiResult<Response> {
    let ident = st.require(&headers, "admin").await?;
    let req: ApiKeyRequest = parse(body)?;
    if !ROLES.contains(&req.role.as_str()) {
        return Err(ApiError::bad_request("Geçersiz rol"));
    }
    let fail = |e: &dyn std::fmt::Display| ApiError::internal("Anahtar üretilemedi", e);
    let raw = auth::new_api_key().map_err(|e| fail(&e))?;
    let res: Result<String, sqlx::Error> = async {
        let mut tx = st.pool.begin().await?;
        let user_id: String = sqlx::query_scalar(
            "INSERT INTO users (username, role) VALUES ($1, $2::user_role) \
             ON CONFLICT (username) DO UPDATE SET role = EXCLUDED.role RETURNING id::text",
        )
        .bind(&req.username)
        .bind(&req.role)
        .fetch_one(&mut *tx)
        .await?;
        let key_id: String = sqlx::query_scalar(
            "INSERT INTO api_keys (user_id, key_hash, key_prefix, name) \
             VALUES ($1::uuid, $2, $3, $4) RETURNING id::text",
        )
        .bind(&user_id)
        .bind(auth::hash_api_key(&raw))
        .bind(&raw[..8])
        .bind(chars(&req.name, 200))
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(key_id)
    }
    .await;
    let key_id = res.map_err(|e| fail(&e))?;
    let detail = json!({"role": req.role, "key_id": key_id});
    st.audit(&ident.user, "auth.key.create", Some(&req.username), detail).await;
    Ok(ok(json!({"id": key_id, "api_key": raw, "role": req.role})))
}

/// Anahtarları listeler (ham değer ya da özet ASLA dönmez).
#[utoipa::path(get, path = "/auth/keys", tag = "Auth", responses((status = 200, description = "Anahtarlar")))]
pub async fn list_api_keys(State(st): State<Shared>, headers: HeaderMap) -> ApiResult<Response> {
    st.require(&headers, "admin").await?;
    const SQL: &str = concat!(
        "SELECT COALESCE(json_agg(t), '[]')::text FROM (SELECT k.id::text AS id, u.username, ",
        "u.role::text AS role, k.key_prefix, k.name, k.revoked, ",
        ts!("k.created_at"),
        " AS created_at, ",
        ts!("k.last_used_at"),
        " AS last_used_at FROM api_keys k JOIN users u ON u.id = k.user_id ",
        "ORDER BY k.created_at DESC LIMIT 200) t"
    );
    let text: String = sqlx::query_scalar(SQL)
        .fetch_one(&st.pool)
        .await
        .map_err(|e| ApiError::internal("Liste okunamadı", e))?;
    Ok(raw_json(text))
}

/// Anahtarı iptal eder (silmez — denetim izi kalır).
#[utoipa::path(delete, path = "/auth/keys/{key_id}", tag = "Auth",
    params(("key_id" = String, Path)), responses((status = 200), (status = 404)))]
pub async fn revoke_api_key(
    State(st): State<Shared>,
    headers: HeaderMap,
    Path(key_id): Path<String>,
) -> ApiResult<Response> {
    let ident = st.require(&headers, "admin").await?;
    let key_id = uuid_or_404(&key_id, "Anahtar")?;
    let n = sqlx::query("UPDATE api_keys SET revoked = TRUE WHERE id::text = $1")
        .bind(key_id)
        .execute(&st.pool)
        .await
        .map_err(|e| ApiError::internal("İptal başarısız", e))?
        .rows_affected();
    if n == 0 {
        return Err(ApiError::not_found("Anahtar bulunamadı"));
    }
    st.audit(&ident.user, "auth.key.revoke", None, json!({"key_id": key_id})).await;
    Ok(ok(json!({"status": "revoked"})))
}

/// Plugin envanteri (+ config şemaları).
#[utoipa::path(get, path = "/plugins", tag = "Collect", responses((status = 200, description = "Plugin'ler")))]
pub async fn list_plugins(State(st): State<Shared>, headers: HeaderMap) -> ApiResult<Response> {
    st.require(&headers, "viewer").await?;
    let mut ids: Vec<&String> = st.collector.plugins.keys().collect();
    ids.sort();
    // sıralı yapı: config_schema alanları manifest sırasıyla kalır (arayüz form sırası)
    #[derive(serde::Serialize)]
    struct PluginOut<'a> {
        id: &'a str,
        name: &'a str,
        network_type: osiris_types::NetworkType,
        description: &'a str,
        config_schema: &'a indexmap::IndexMap<String, osiris_types::FieldSpec>,
        schedule_default: &'a str,
    }
    let out: Vec<PluginOut<'_>> = ids
        .into_iter()
        .map(|id| {
            let m = st.collector.plugins[id].manifest();
            PluginOut {
                id,
                name: &m.name,
                network_type: m.network_type,
                description: &m.description,
                config_schema: &m.config_schema,
                schedule_default: m.schedule_default.as_deref().unwrap_or_default(),
            }
        })
        .collect();
    Ok(axum::Json(out).into_response())
}

/// Panel sayaçları: kaynak/öğe/varlık/kenar/sorgu.
#[utoipa::path(get, path = "/stats", tag = "Stats", responses((status = 200, description = "Sayaçlar")))]
pub async fn stats(State(st): State<Shared>, headers: HeaderMap) -> ApiResult<Response> {
    st.require(&headers, "viewer").await?;
    let text: String = sqlx::query_scalar(concat!(
        "SELECT json_build_object(\
         'sources', (SELECT count(*) FROM sources), \
         'sources_enabled', (SELECT count(*) FROM sources WHERE enabled), \
         'items', (SELECT count(*) FROM items), \
         'entities', (SELECT count(*) FROM entities), \
         'edges', (SELECT count(*) FROM graph_edges), \
         'saved_queries', (SELECT count(*) FROM saved_queries), \
         'alerts', (SELECT count(*) FROM saved_queries WHERE alert_enabled), \
         'latest_item_at', ",
        ts!("(SELECT collected_at FROM items ORDER BY collected_at DESC LIMIT 1)"),
        ")::text"
    ))
    .fetch_one(&st.pool)
    .await
    .map_err(|e| ApiError::internal("İstatistik okunamadı", e))?;
    Ok(raw_json(text))
}

/// Son öğeler. Sayfalama: `limit` (1-50), `offset`.
#[utoipa::path(get, path = "/items/recent", tag = "Items",
    params(("limit" = Option<i64>, Query), ("offset" = Option<i64>, Query)), responses((status = 200)))]
pub async fn recent_items(
    State(st): State<Shared>,
    headers: HeaderMap,
    RawQuery(q): RawQuery,
) -> ApiResult<Response> {
    st.require(&headers, "viewer").await?;
    let p = Params::from_raw(q.as_deref());
    let limit = p.int("limit", 20)?.clamp(1, 50);
    let offset = p.int("offset", 0)?.max(0);
    let text: String = sqlx::query_scalar(concat!(
        "SELECT COALESCE(json_agg(t), '[]')::text FROM (SELECT id::text AS id, title, url, language, ",
        "LEFT(cleaned_content, 500) AS snippet, ",
        ts!("collected_at"),
        " AS collected_at, tags FROM items ORDER BY items.collected_at DESC LIMIT $1 OFFSET $2) t"
    ))
    .bind(limit)
    .bind(offset)
    .fetch_one(&st.pool)
    .await
    .map_err(|e| ApiError::internal("Öğeler okunamadı", e))?;
    Ok(raw_json(text))
}

/// Öğeyi bağlı varlıklarıyla döndürür (detay görünümü).
#[utoipa::path(get, path = "/items/{item_id}", tag = "Items", params(("item_id" = String, Path)),
    responses((status = 200), (status = 404)))]
pub async fn item_detail(
    State(st): State<Shared>,
    headers: HeaderMap,
    Path(item_id): Path<String>,
) -> ApiResult<Response> {
    st.require(&headers, "viewer").await?;
    let item_id = uuid_or_404(&item_id, "Öğe")?;
    let fail = |e: sqlx::Error| ApiError::internal("Öğe okunamadı", e);
    let row: Option<String> = sqlx::query_scalar(concat!(
        "SELECT json_build_object('id', id::text, 'source_id', source_id::text, 'title', title, ",
        "'url', url, 'language', language, 'cleaned_content', cleaned_content, 'collected_at', ",
        ts!("collected_at"),
        ", 'published_at', ",
        ts!("published_at"),
        ", 'content_hash', content_hash, 'metadata', metadata, 'tags', tags, 'entities', (\
           SELECT COALESCE(json_agg(t), '[]') FROM (SELECT e.type::text AS type, e.value, ie.mention_count \
           FROM item_entities ie JOIN entities e ON e.id = ie.entity_id WHERE ie.item_id = items.id \
           ORDER BY ie.mention_count DESC LIMIT 200) t))::text FROM items WHERE id::text = $1"
    ))
    .bind(item_id)
    .fetch_optional(&st.pool)
    .await
    .map_err(fail)?;
    row.map(raw_json).ok_or_else(|| ApiError::not_found("Öğe bulunamadı"))
}

/// Anma sayısına göre varlıklar. Filtre: `entity_type`. Limit 1-100.
#[utoipa::path(get, path = "/entities/top", tag = "Items",
    params(("entity_type" = Option<String>, Query), ("limit" = Option<i64>, Query)),
    responses((status = 200), (status = 400)))]
pub async fn top_entities(
    State(st): State<Shared>,
    headers: HeaderMap,
    RawQuery(q): RawQuery,
) -> ApiResult<Response> {
    st.require(&headers, "viewer").await?;
    let p = Params::from_raw(q.as_deref());
    let etype = p.text("entity_type");
    let limit = p.int("limit", 20)?.clamp(1, 100);
    if etype.is_some_and(|t| !osiris_query::VALID_ENTITY_TYPES.contains(&t)) {
        return Err(ApiError::bad_request("Geçersiz entity_type"));
    }
    let text: String = sqlx::query_scalar(concat!(
        "SELECT COALESCE(json_agg(t), '[]')::text FROM (SELECT e.type::text AS type, e.value, ",
        "COUNT(*) AS mentions, ",
        ts!("MAX(e.last_seen_at)"),
        " AS last_seen FROM entities e JOIN item_entities ie ON ie.entity_id = e.id ",
        "WHERE ($1::text IS NULL OR e.type::text = $1) GROUP BY e.type, e.value ",
        "ORDER BY mentions DESC LIMIT $2) t"
    ))
    .bind(etype)
    .bind(limit)
    .fetch_one(&st.pool)
    .await
    .map_err(|e| ApiError::internal("Varlıklar okunamadı", e))?;
    Ok(raw_json(text))
}

// ----------------------------------------------------------------------------- toplama

#[derive(Deserialize, ToSchema)]
pub struct CollectRequest {
    plugin_id: String,
    #[serde(default)]
    #[schema(value_type = Object)]
    config: Map<String, Value>,
    /// true → toplanan öğelerin önizlemesi de döner (`results`, en çok 1000 öğe)
    #[serde(default = "d_false")]
    #[schema(value_type = bool, default = false)]
    include_items: LaxBool,
}

/// `include_items` yanıt sınırları (DoS / yanıt boyutu korumaları).
const PREVIEW_MAX_ITEMS: usize = 1000;
const PREVIEW_MAX_CONTENT: usize = 500;

/// Toplanan öğelerin kısaltılmış önizlemesi (ham içerik en çok 500 kod noktası).
fn item_previews(items: &[osiris_types::CollectedItem]) -> Value {
    Value::Array(
        items
            .iter()
            .take(PREVIEW_MAX_ITEMS)
            .map(|it| {
                json!({
                    "url": it.url,
                    "title": it.title,
                    "snippet": chars(&it.raw_content, PREVIEW_MAX_CONTENT),
                    "published_at": it.published_at,
                    "metadata": it.metadata,
                    "tags": it.tags,
                })
            })
            .collect(),
    )
}

fn collect_out(result: &osiris_types::CollectionResult, include_items: bool) -> Value {
    let mut out = json!({"items": result.items.len(), "metadata": result.metadata});
    if include_items {
        out["results"] = item_previews(&result.items);
    }
    out
}

impl Validate for CollectRequest {
    fn validate(&self, v: &mut Violations) {
        v.len("plugin_id", &self.plugin_id, 1, 64);
        v.pattern("plugin_id", &self.plugin_id, &PLUGIN_ID);
        v.items("config", self.config.len(), 0, 50);
    }
}

#[derive(Deserialize, ToSchema)]
pub struct BatchRequest {
    jobs: Vec<CollectRequest>,
}

impl Validate for BatchRequest {
    fn validate(&self, v: &mut Violations) {
        v.items("jobs", self.jobs.len(), 1, 10);
        for j in &self.jobs {
            j.validate(v);
        }
    }
}

/// İşleri sırayla çalıştırır; iş başına sonuç (kısmi başarı mümkün).
#[utoipa::path(post, path = "/collect/batch", tag = "Collect", request_body = BatchRequest,
    responses((status = 200)))]
pub async fn collect_batch(
    State(st): State<Shared>,
    headers: HeaderMap,
    Body(body): Body,
) -> ApiResult<Response> {
    let ident = st.require(&headers, "analyst").await?;
    let req: BatchRequest = parse(body)?;
    let mut results = Vec::new();
    for job in req.jobs {
        let pid = job.plugin_id;
        if !st.collector.plugins.contains_key(&pid) {
            results.push(json!({"plugin_id": pid, "ok": false, "error": "Plugin bulunamadı"}));
            continue;
        }
        if config_too_big(&job.config) {
            results.push(json!({"plugin_id": pid, "ok": false, "error": "config çok büyük"}));
            continue;
        }
        match st.collector.run_collection(&pid, job.config, true).await {
            Ok(r) => results.push(json!({
                "plugin_id": pid,
                "ok": r.success,
                "items": if r.success { r.items.len() } else { 0 },
                "error": r.error,
            })),
            Err(e) => results.push(json!({"plugin_id": pid, "ok": false, "error": e.to_string()})),
        }
    }
    let ok_count = results.iter().filter(|r| r["ok"] == true).count();
    let detail = json!({"jobs": results.len(), "ok": ok_count});
    st.audit(&ident.user, "collect.batch", None, detail).await;
    Ok(ok(json!({"jobs": results, "ok": ok_count, "total": results.len()})))
}

/// Tek toplama görevi (öğeler kuyruğa yazılır).
#[utoipa::path(post, path = "/collect", tag = "Collect", request_body = CollectRequest,
    responses((status = 200), (status = 404), (status = 413), (status = 502, description = "Plugin başarısız")))]
pub async fn collect(
    State(st): State<Shared>,
    headers: HeaderMap,
    Body(body): Body,
) -> ApiResult<Response> {
    let ident = st.require(&headers, "analyst").await?;
    let req: CollectRequest = parse(body)?;
    if !st.collector.plugins.contains_key(&req.plugin_id) {
        return Err(ApiError::not_found("Plugin bulunamadı"));
    }
    if config_too_big(&req.config) {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "config çok büyük"));
    }
    let result = st
        .collector
        .run_collection(&req.plugin_id, req.config, true)
        .await
        .map_err(|e| ApiError::not_found(e.to_string()))?;
    if !result.success {
        let error = result.error.unwrap_or_default();
        let detail = json!({"error": error});
        st.audit(&ident.user, "collect.failed", Some(&req.plugin_id), detail).await;
        return Err(ApiError::new(StatusCode::BAD_GATEWAY, error));
    }
    let detail = json!({"items": result.items.len()});
    st.audit(&ident.user, "collect", Some(&req.plugin_id), detail).await;
    Ok(ok(collect_out(&result, req.include_items.0)))
}

// ------------------------------------------------------------------------------- arama

fn d20() -> LaxInt {
    LaxInt(20)
}

#[derive(Deserialize, ToSchema)]
pub struct SearchRequest {
    query: String,
    #[serde(default = "d20")]
    #[schema(value_type = i64, default = 20)]
    limit: LaxInt,
}

impl Validate for SearchRequest {
    fn validate(&self, v: &mut Violations) {
        v.len("query", &self.query, 1, 500);
        v.range("limit", self.limit.0, 1, 100);
    }
}

fn query_error(detail: &'static str) -> impl Fn(osiris_query::QueryError) -> ApiError {
    move |e| match e {
        osiris_query::QueryError::Invalid(m) => ApiError::bad_request(m),
        osiris_query::QueryError::Db(e) => ApiError::internal(detail, e),
    }
}

/// Tam metin arama.
#[utoipa::path(post, path = "/search", tag = "Search", request_body = SearchRequest,
    responses((status = 200), (status = 400)))]
pub async fn search(
    State(st): State<Shared>,
    headers: HeaderMap,
    Body(body): Body,
) -> ApiResult<Response> {
    let ident = st.require(&headers, "viewer").await?;
    let req: SearchRequest = parse(body)?;
    let rows = osiris_query::fulltext_search(&st.pool, &req.query, req.limit.0)
        .await
        .map_err(query_error("Arama başarısız"))?;
    let hits = osiris_query::count_rows(&rows);
    let detail = json!({"query": chars(&req.query, 200), "hits": hits});
    st.audit(&ident.user, "search", None, detail).await;
    Ok(raw_json(rows))
}

#[derive(Deserialize, ToSchema)]
pub struct EntitySearchRequest {
    entity_type: String,
    value: String,
    #[serde(default = "d20")]
    #[schema(value_type = i64, default = 20)]
    limit: LaxInt,
}

impl Validate for EntitySearchRequest {
    fn validate(&self, v: &mut Violations) {
        v.len("entity_type", &self.entity_type, 1, 32);
        v.len("value", &self.value, 1, 500);
        v.range("limit", self.limit.0, 1, 100);
    }
}

/// Varlık bazlı arama.
#[utoipa::path(post, path = "/search/entity", tag = "Search", request_body = EntitySearchRequest,
    responses((status = 200), (status = 400)))]
pub async fn entity_search(
    State(st): State<Shared>,
    headers: HeaderMap,
    Body(body): Body,
) -> ApiResult<Response> {
    st.require(&headers, "viewer").await?;
    let req: EntitySearchRequest = parse(body)?;
    let rows = osiris_query::entity_search(&st.pool, &req.entity_type, &req.value, req.limit.0)
        .await
        .map_err(query_error("Arama başarısız"))?;
    Ok(raw_json(rows))
}

// -------------------------------------------------------------------------------- graf

/// Graf düğüm/kenarları (en çok 5000 düğüm, 20 000 kenar).
#[utoipa::path(get, path = "/graph", tag = "Graph", responses((status = 200)))]
pub async fn graph(State(st): State<Shared>, headers: HeaderMap) -> ApiResult<Response> {
    st.require(&headers, "viewer").await?;
    let g = st.graph().await;
    Ok(ok(g.graph.to_json(5000, 20_000)))
}

fn related() -> String {
    "related".into()
}

#[derive(Deserialize, ToSchema)]
pub struct GraphAddRequest {
    source: String,
    target: String,
    #[serde(default = "related")]
    relation_type: String,
}

impl Validate for GraphAddRequest {
    fn validate(&self, v: &mut Violations) {
        v.len("source", &self.source, 1, 500);
        v.len("target", &self.target, 1, 500);
        v.len("relation_type", &self.relation_type, 0, 100);
    }
}

/// Graf ilişkisi ekle (kalıcı).
#[utoipa::path(post, path = "/graph/relation", tag = "Graph", request_body = GraphAddRequest,
    responses((status = 200), (status = 400)))]
pub async fn graph_add(
    State(st): State<Shared>,
    headers: HeaderMap,
    Body(body): Body,
) -> ApiResult<Response> {
    st.require(&headers, "analyst").await?;
    let req: GraphAddRequest = parse(body)?;
    let graph_err = |e: osiris_graph::GraphError| match e {
        osiris_graph::GraphError::Invalid(m) => ApiError::bad_request(m),
        overflow => ApiError::new(StatusCode::SERVICE_UNAVAILABLE, overflow.to_string()),
    };
    {
        let mut g = st.graph().await;
        g.graph.add_entity(&req.source, &[]).map_err(graph_err)?;
        g.graph.add_entity(&req.target, &[]).map_err(graph_err)?;
        g.graph
            .add_relation(&req.source, &req.target, &req.relation_type, 1.0)
            .map_err(graph_err)?;
    }
    let (s, t) = (osiris_hash::py_strip(&req.source), osiris_hash::py_strip(&req.target));
    if s != t
        && let Err(e) = osiris_db::upsert_graph_edge(&st.pool, s, t, &req.relation_type, 1.0).await
    {
        tracing::debug!("Graf persist atlandı: {e}");
    }
    Ok(ok(json!({"status": "ok"})))
}

// ---------------------------------------------------------------------------- kaynaklar

const VALID_NETWORKS: [&str; 13] = [
    "www",
    "tor",
    "i2p",
    "p2p",
    "freenet",
    "zeronet",
    "irc",
    "matrix",
    "rss",
    "api",
    "blockchain",
    "sdr",
    "custom",
];

macro_rules! sources_sql {
    ($retention:literal) => {
        concat!(
            "SELECT COALESCE(json_agg(t), '[]')::text FROM (SELECT id::text AS id, name, url, ",
            "network_type::text AS network_type, plugin_id, schedule, priority, enabled, tags, ",
            ts!("last_crawled_at"),
            " AS last_crawled_at, ",
            ts!("last_success_at"),
            " AS last_success_at, failure_count, avg_response_ms, ",
            $retention,
            " FROM sources WHERE ($1::bool IS NULL OR enabled = $1) ",
            "AND ($2::text IS NULL OR plugin_id = $2) ",
            "AND ($3::text IS NULL OR name ILIKE $3 ESCAPE '\\' OR url ILIKE $3 ESCAPE '\\') ",
            "ORDER BY name LIMIT $4 OFFSET $5) t"
        )
    };
}
const SOURCES: &str = sources_sql!("retention_days");

/// Filtreler: `enabled`, `plugin_id`, `q` (ad/URL alt-metin). Sayfalama: `limit` (1-200), `offset`.
#[utoipa::path(get, path = "/sources", tag = "Sources",
    params(("enabled" = Option<bool>, Query), ("plugin_id" = Option<String>, Query),
           ("q" = Option<String>, Query), ("limit" = Option<i64>, Query), ("offset" = Option<i64>, Query)),
    responses((status = 200), (status = 422)))]
pub async fn list_sources(
    State(st): State<Shared>,
    headers: HeaderMap,
    RawQuery(q): RawQuery,
) -> ApiResult<Response> {
    st.require(&headers, "viewer").await?;
    let p = Params::from_raw(q.as_deref());
    let (limit, offset) = page(&p)?;
    let enabled = p.opt_bool("enabled")?;
    let plugin_id = p.text("plugin_id").map(|s| chars(s, 64));
    // LIKE jokerleri etkisizleştirilir
    let needle = p.text("q").map(|s| format!("%{}%", osiris_query::escape_like(&chars(s, 200))));
    let fail = |e: sqlx::Error| ApiError::internal("Kaynaklar okunamadı", e);
    let text: String = sqlx::query_scalar(SOURCES)
        .bind(enabled)
        .bind(plugin_id)
        .bind(needle)
        .bind(limit)
        .bind(offset)
        .fetch_one(&st.pool)
        .await
        .map_err(fail)?;
    Ok(raw_json(text))
}

fn d_www() -> String {
    "www".into()
}

fn d5() -> LaxInt {
    LaxInt(5)
}

#[derive(Deserialize, ToSchema)]
pub struct SourceRequest {
    name: String,
    plugin_id: String,
    #[serde(default)]
    url: String,
    #[serde(default = "d_www")]
    network_type: String,
    #[serde(default)]
    schedule: String,
    #[serde(default = "d5")]
    #[schema(value_type = i64, default = 5)]
    priority: LaxInt,
    #[serde(default = "d_true")]
    #[schema(value_type = bool, default = true)]
    enabled: LaxBool,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    #[schema(value_type = Object)]
    config: Map<String, Value>,
    /// KVKK saklama süresi: null → OSIRIS_RETENTION_DAYS, 0 → süreyle silinmez
    #[serde(default)]
    #[schema(value_type = Option<i64>)]
    retention_days: Option<LaxInt>,
}

impl Validate for SourceRequest {
    fn validate(&self, v: &mut Violations) {
        v.len("name", &self.name, 1, 500);
        v.len("plugin_id", &self.plugin_id, 1, 64);
        v.pattern("plugin_id", &self.plugin_id, &PLUGIN_ID);
        v.len("url", &self.url, 0, 2048);
        v.len("network_type", &self.network_type, 0, 32);
        v.len("schedule", &self.schedule, 0, 100);
        v.range("priority", self.priority.0, 1, 10);
        v.items("tags", self.tags.len(), 0, 20);
        v.items("config", self.config.len(), 0, 50);
        if let Some(d) = self.retention_days {
            v.range("retention_days", d.0, 0, 36_500);
        }
    }
}

/// Kaynak oluştur.
#[utoipa::path(post, path = "/sources", tag = "Sources", request_body = SourceRequest,
    responses((status = 200), (status = 400), (status = 404)))]
pub async fn create_source(
    State(st): State<Shared>,
    headers: HeaderMap,
    Body(body): Body,
) -> ApiResult<Response> {
    let ident = st.require(&headers, "analyst").await?;
    let req: SourceRequest = parse(body)?;
    if !VALID_NETWORKS.contains(&req.network_type.as_str()) {
        return Err(ApiError::bad_request("Geçersiz network_type"));
    }
    if !st.collector.plugins.contains_key(&req.plugin_id) {
        return Err(ApiError::not_found("Plugin bulunamadı"));
    }
    let fail = |e: sqlx::Error| ApiError::internal("Kaynak oluşturulamadı", e);
    let tags: Vec<String> = req.tags.iter().take(20).map(|t| chars(t, 100)).collect();
    let metadata = json!({"config": req.config}).to_string();
    let sql = "INSERT INTO sources (name, url, network_type, plugin_id, schedule, priority, enabled, \
               tags, metadata, retention_days) VALUES ($1, NULLIF($2,''), $3::network_type, $4, \
               NULLIF($5,''), $6, $7, $8, $9::jsonb, $10) RETURNING id::text";
    let q = sqlx::query_scalar::<_, String>(sql)
        .bind(&req.name)
        .bind(&req.url)
        .bind(&req.network_type)
        .bind(&req.plugin_id)
        .bind(&req.schedule)
        .bind(req.priority.0 as i32)
        .bind(req.enabled.0)
        .bind(&tags)
        .bind(metadata)
        .bind(req.retention_days.map(|d| d.0 as i32));
    let id = q.fetch_one(&st.pool).await.map_err(fail)?;
    let detail = json!({"plugin": req.plugin_id});
    st.audit(&ident.user, "source.create", Some(&req.name), detail).await;
    Ok(ok(json!({"id": id})))
}

/// Kaynak sil.
#[utoipa::path(delete, path = "/sources/{source_id}", tag = "Sources",
    params(("source_id" = String, Path)), responses((status = 200), (status = 404)))]
pub async fn delete_source(
    State(st): State<Shared>,
    headers: HeaderMap,
    Path(source_id): Path<String>,
) -> ApiResult<Response> {
    let ident = st.require(&headers, "analyst").await?;
    let source_id = uuid_or_404(&source_id, "Kaynak")?;
    let n = sqlx::query("DELETE FROM sources WHERE id::text = $1")
        .bind(source_id)
        .execute(&st.pool)
        .await
        .map_err(|e| ApiError::internal("Kaynak silinemedi", e))?
        .rows_affected();
    if n == 0 {
        return Err(ApiError::not_found("Kaynak bulunamadı"));
    }
    st.audit(&ident.user, "source.delete", None, json!({"id": source_id})).await;
    Ok(ok(json!({"status": "deleted"})))
}

/// Kayıtlı kaynağı tek seferlik çalıştırır (sağlık takibi dahil).
/// `?include_items=true` → toplanan öğelerin önizlemesi de döner.
#[utoipa::path(post, path = "/sources/{source_id}/collect", tag = "Sources",
    params(("source_id" = String, Path), ("include_items" = Option<bool>, Query)),
    responses((status = 200), (status = 404), (status = 409, description = "Kaynak devre dışı"), (status = 502)))]
pub async fn collect_source(
    State(st): State<Shared>,
    headers: HeaderMap,
    Path(source_id): Path<String>,
    RawQuery(q): RawQuery,
) -> ApiResult<Response> {
    let ident = st.require(&headers, "analyst").await?;
    let include_items = Params::from_raw(q.as_deref()).opt_bool("include_items")?.unwrap_or(false);
    let source_id = uuid_or_404(&source_id, "Kaynak")?;
    let row: Option<(String, Option<String>, bool, String)> = sqlx::query_as(
        "SELECT plugin_id, url, enabled, COALESCE(metadata, '{}'::jsonb)::text FROM sources \
         WHERE id::text = $1",
    )
    .bind(source_id)
    .fetch_optional(&st.pool)
    .await
    .map_err(|e| ApiError::internal("Kaynak okunamadı", e))?;
    let Some((plugin_id, url, enabled, metadata)) = row else {
        return Err(ApiError::not_found("Kaynak bulunamadı"));
    };
    if !enabled {
        return Err(ApiError::new(StatusCode::CONFLICT, "Kaynak devre dışı"));
    }
    let src = osiris_db::ScheduledSource {
        id: source_id.to_owned(),
        plugin_id: plugin_id.clone(),
        url,
        schedule: String::new(),
        metadata: serde_json::from_str(&metadata).unwrap_or(Value::Null),
    };
    if !st.collector.plugins.contains_key(&plugin_id) {
        return Err(ApiError::not_found("Plugin bulunamadı"));
    }
    let result = st
        .collector
        .run_collection(&plugin_id, osiris_collector::task_config(&src), true)
        .await
        .map_err(|e| ApiError::not_found(e.to_string()))?;
    if !result.success {
        return Err(ApiError::new(StatusCode::BAD_GATEWAY, result.error.unwrap_or_default()));
    }
    let detail = json!({"id": source_id, "items": result.items.len()});
    st.audit(&ident.user, "source.collect", Some(&plugin_id), detail).await;
    Ok(ok(collect_out(&result, include_items)))
}

// ---------------------------------------------------------------- kayıtlı sorgu / uyarı

/// Filtre: `alert_enabled`. Sayfalama: `limit` (1-200), `offset`.
#[utoipa::path(get, path = "/saved-queries", tag = "Alerts",
    params(("alert_enabled" = Option<bool>, Query), ("limit" = Option<i64>, Query),
           ("offset" = Option<i64>, Query)),
    responses((status = 200), (status = 422)))]
pub async fn list_saved_queries(
    State(st): State<Shared>,
    headers: HeaderMap,
    RawQuery(q): RawQuery,
) -> ApiResult<Response> {
    st.require(&headers, "viewer").await?;
    let p = Params::from_raw(q.as_deref());
    let (limit, offset) = page(&p)?;
    let alert = p.opt_bool("alert_enabled")?;
    let text: String = sqlx::query_scalar(concat!(
        "SELECT COALESCE(json_agg(t), '[]')::text FROM (SELECT id::text AS id, name, query_text, ",
        "query_type::text AS query_type, alert_enabled, ",
        ts!("last_triggered_at"),
        " AS last_triggered_at FROM saved_queries WHERE ($1::bool IS NULL OR alert_enabled = $1) ",
        "ORDER BY name LIMIT $2 OFFSET $3) t"
    ))
    .bind(alert)
    .bind(limit)
    .bind(offset)
    .fetch_one(&st.pool)
    .await
    .map_err(|e| ApiError::internal("Sorgular okunamadı", e))?;
    Ok(raw_json(text))
}

fn d_fts() -> String {
    "fts".into()
}

fn d_false() -> LaxBool {
    LaxBool(false)
}

#[derive(Deserialize, ToSchema)]
pub struct SavedQueryRequest {
    name: String,
    query_text: String,
    #[serde(default = "d_fts")]
    query_type: String,
    #[serde(default = "d_false")]
    #[schema(value_type = bool, default = false)]
    alert_enabled: LaxBool,
}

impl Validate for SavedQueryRequest {
    fn validate(&self, v: &mut Violations) {
        v.len("name", &self.name, 1, 200);
        v.len("query_text", &self.query_text, 1, 500);
        v.len("query_type", &self.query_type, 0, 32);
    }
}

const VALID_QUERY_TYPES: [&str; 5] = ["fts", "semantic", "regex", "entity", "graph"];

/// Kayıtlı sorgu oluştur.
#[utoipa::path(post, path = "/saved-queries", tag = "Alerts", request_body = SavedQueryRequest,
    responses((status = 200), (status = 400)))]
pub async fn create_saved_query(
    State(st): State<Shared>,
    headers: HeaderMap,
    Body(body): Body,
) -> ApiResult<Response> {
    st.require(&headers, "analyst").await?;
    let req: SavedQueryRequest = parse(body)?;
    if !VALID_QUERY_TYPES.contains(&req.query_type.as_str()) {
        return Err(ApiError::bad_request("Geçersiz query_type"));
    }
    let id: String = sqlx::query_scalar(
        "INSERT INTO saved_queries (name, query_text, query_type, alert_enabled) \
         VALUES ($1, $2, $3::query_type, $4) RETURNING id::text",
    )
    .bind(&req.name)
    .bind(&req.query_text)
    .bind(&req.query_type)
    .bind(req.alert_enabled.0)
    .fetch_one(&st.pool)
    .await
    .map_err(|e| ApiError::internal("Sorgu oluşturulamadı", e))?;
    Ok(ok(json!({"id": id})))
}

/// Kayıtlı sorgu sil.
#[utoipa::path(delete, path = "/saved-queries/{query_id}", tag = "Alerts",
    params(("query_id" = String, Path)), responses((status = 200), (status = 404)))]
pub async fn delete_saved_query(
    State(st): State<Shared>,
    headers: HeaderMap,
    Path(query_id): Path<String>,
) -> ApiResult<Response> {
    st.require(&headers, "analyst").await?;
    let query_id = uuid_or_404(&query_id, "Sorgu")?;
    let n = sqlx::query("DELETE FROM saved_queries WHERE id::text = $1")
        .bind(query_id)
        .execute(&st.pool)
        .await
        .map_err(|e| ApiError::internal("Sorgu silinemedi", e))?
        .rows_affected();
    if n == 0 {
        return Err(ApiError::not_found("Sorgu bulunamadı"));
    }
    Ok(ok(json!({"status": "deleted"})))
}

#[derive(Deserialize, ToSchema)]
pub struct AlertTestRequest {
    text: String,
    #[serde(default)]
    title: String,
}

impl Validate for AlertTestRequest {
    fn validate(&self, v: &mut Violations) {
        v.len("text", &self.text, 1, 50_000);
        v.len("title", &self.title, 0, 2000);
    }
}

/// Ham metni kayıtlı uyarı sorgularıyla eşleştirir (kayıt oluşturmaz, yayın yapmaz).
#[utoipa::path(post, path = "/alerts/test", tag = "Alerts", request_body = AlertTestRequest,
    responses((status = 200)))]
pub async fn test_alerts(
    State(st): State<Shared>,
    headers: HeaderMap,
    Body(body): Body,
) -> ApiResult<Response> {
    st.require(&headers, "analyst").await?;
    let req: AlertTestRequest = parse(body)?;
    let fail = |e: &dyn std::fmt::Display| ApiError::internal("Eşleşme başarısız", e);
    let text: String = sqlx::query_scalar(
        "SELECT COALESCE(json_agg(t), '[]')::text FROM (SELECT id::text AS id, name, query_text, \
         alert_enabled FROM saved_queries WHERE alert_enabled LIMIT 100) t",
    )
    .fetch_one(&st.pool)
    .await
    .map_err(|e| fail(&e))?;
    let Json::Array(queries) = Json::parse_py(&text).map_err(|e| fail(&e))? else {
        return Err(fail(&"sorgu listesi dizi değil"));
    };
    let item = Json::Object(vec![
        ("cleaned_content".into(), Json::Str(req.text)),
        ("title".into(), Json::Str(req.title)),
    ]);
    let manager = osiris_alert::AlertManager::new(None, osiris_alert::AnomalyOptions::default());
    let triggered = manager.match_item(&item, &queries);
    let ids: Vec<String> = triggered
        .iter()
        .map(osiris_alert::to_value)
        .filter_map(|t| t.get("query_id").and_then(Value::as_str).map(str::to_owned))
        .filter(|id| !id.is_empty())
        .collect();
    if !ids.is_empty()
        && let Err(e) = sqlx::query(
            "UPDATE saved_queries SET last_triggered_at = NOW() WHERE id::text = ANY($1)",
        )
        .bind(&ids)
        .execute(&st.pool)
        .await
    {
        tracing::debug!("last_triggered yazılamadı: {e}");
    }
    Ok(ok(Value::Array(triggered.iter().map(osiris_alert::to_value).collect())))
}

// ------------------------------------------------------------------------------- rapor

#[derive(Deserialize, ToSchema)]
pub struct MarkdownReportRequest {
    title: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    #[schema(value_type = Vec<Object>)]
    findings: Vec<Map<String, Value>>,
    #[serde(default)]
    sources: Vec<String>,
}

impl Validate for MarkdownReportRequest {
    fn validate(&self, v: &mut Violations) {
        v.len("title", &self.title, 1, 500);
        v.len("scope", &self.scope, 0, 1000);
        v.len("summary", &self.summary, 0, 20_000);
        v.items("findings", self.findings.len(), 0, 500);
        v.items("sources", self.sources.len(), 0, 500);
    }
}

/// Markdown rapor üretir (gövdeyle döner, diske yazmaz; HTML kaçışlı).
#[utoipa::path(post, path = "/reports/markdown", tag = "Reports", request_body = MarkdownReportRequest,
    responses((status = 200)))]
pub async fn report_markdown(
    State(st): State<Shared>,
    headers: HeaderMap,
    Body(body): Body,
) -> ApiResult<Response> {
    st.require(&headers, "analyst").await?;
    let req: MarkdownReportRequest = parse(body)?;
    let findings: Vec<Json> =
        req.findings.into_iter().map(|f| Json::from(Value::Object(f))).collect();
    let sources: Vec<Json> = req.sources.into_iter().map(Json::Str).collect();
    let md = osiris_report::markdown(&osiris_report::ReportInput {
        title: &req.title,
        scope: &req.scope,
        summary: &req.summary,
        findings: &findings,
        sources: &sources,
    });
    Ok(ok(json!({"markdown": md})))
}

// --------------------------------------------------------------- KVKK: saklama ve silme

/// Öğeyi ve yalnızca ona bağlı kalan varlıkları kalıcı olarak siler.
#[utoipa::path(delete, path = "/items/{item_id}", tag = "Privacy", params(("item_id" = String, Path)),
    responses((status = 200), (status = 404)))]
pub async fn delete_item(
    State(st): State<Shared>,
    headers: HeaderMap,
    Path(item_id): Path<String>,
) -> ApiResult<Response> {
    let ident = st.require(&headers, "admin").await?;
    let item_id = uuid_or_404(&item_id, "Öğe")?;
    let res: Result<Option<u64>, sqlx::Error> = async {
        let mut tx = st.pool.begin().await?;
        let n = sqlx::query("DELETE FROM items WHERE id::text = $1")
            .bind(item_id)
            .execute(&mut *tx)
            .await?;
        if n.rows_affected() == 0 {
            return Ok(None);
        }
        let orphans = osiris_db::purge_orphan_entities(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some(orphans))
    }
    .await;
    let orphans = res
        .map_err(|e| ApiError::internal("Öğe silinemedi", e))?
        .ok_or_else(|| ApiError::not_found("Öğe bulunamadı"))?;
    let detail = json!({"id": item_id, "entities_deleted": orphans});
    st.audit(&ident.user, "item.delete", None, detail).await;
    Ok(ok(json!({"status": "deleted", "items_deleted": 1, "entities_deleted": orphans})))
}

#[derive(Deserialize, ToSchema)]
pub struct EraseRequest {
    entity_type: String,
    value: String,
    #[serde(default = "d_true")]
    #[schema(value_type = bool, default = true)]
    delete_items: LaxBool,
}

impl Validate for EraseRequest {
    fn validate(&self, v: &mut Violations) {
        v.len("entity_type", &self.entity_type, 1, 32);
        v.len("value", &self.value, 1, 500);
    }
}

/// Bir varlığı (örn. e-posta) ve — `delete_items` ise — onu anan öğeleri siler. Eşleşme tip +
/// büyük/küçük harf duyarsız değer; graf kenarlarındaki aynı ad da silinir. Denetim kaydına
/// değerin yalnızca SHA-256 özeti yazılır.
#[utoipa::path(post, path = "/entities/erase", tag = "Privacy", request_body = EraseRequest,
    responses((status = 200), (status = 400), (status = 422)))]
pub async fn erase_entity(
    State(st): State<Shared>,
    headers: HeaderMap,
    Body(body): Body,
) -> ApiResult<Response> {
    let ident = st.require(&headers, "admin").await?;
    let req: EraseRequest = parse(body)?;
    if !osiris_query::VALID_ENTITY_TYPES.contains(&req.entity_type.as_str()) {
        return Err(ApiError::bad_request("Geçersiz entity_type"));
    }
    let needle = osiris_hash::py_strip(&req.value).to_lowercase();
    if needle.is_empty() {
        return Err(ApiError::unprocessable("value boş olamaz"));
    }
    let res: Result<(u64, u64, u64), sqlx::Error> = async {
        let mut tx = st.pool.begin().await?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id::text FROM entities WHERE type = $1::entity_type AND lower(value) = $2",
        )
        .bind(&req.entity_type)
        .bind(&needle)
        .fetch_all(&mut *tx)
        .await?;
        let mut items_deleted = 0;
        if !ids.is_empty() && req.delete_items.0 {
            items_deleted = sqlx::query(
                "DELETE FROM items WHERE id IN (SELECT item_id FROM item_entities \
                 WHERE entity_id = ANY($1::uuid[]))",
            )
            .bind(&ids)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        }
        let mut entities_deleted = 0;
        if !ids.is_empty() {
            entities_deleted = sqlx::query("DELETE FROM entities WHERE id = ANY($1::uuid[])")
                .bind(&ids)
                .execute(&mut *tx)
                .await?
                .rows_affected();
        }
        let edges_deleted = sqlx::query(
            "DELETE FROM graph_edges WHERE lower(source_name) = $1 OR lower(target_name) = $1",
        )
        .bind(&needle)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if items_deleted > 0 {
            entities_deleted += osiris_db::purge_orphan_entities(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok((items_deleted, entities_deleted, edges_deleted))
    }
    .await;
    let (items_deleted, entities_deleted, edges_deleted) =
        res.map_err(|e| ApiError::internal("Silme başarısız", e))?;
    if edges_deleted > 0 {
        // bellekteki grafı DB'den yeniden kur
        let mut g = st.graph.lock().await;
        g.graph.clear();
        g.loaded = false;
    }
    let out = json!({
        "items_deleted": items_deleted,
        "entities_deleted": entities_deleted,
        "edges_deleted": edges_deleted,
    });
    let mut detail =
        json!({"entity_type": req.entity_type, "value_sha256": auth::hash_api_key(&needle)});
    if let (Value::Object(d), Value::Object(o)) = (&mut detail, &out) {
        d.extend(o.clone());
    }
    st.audit(&ident.user, "entity.erase", None, detail).await;
    Ok(ok(out))
}

/// Süresi dolan öğeleri hemen siler (pipeline işçisi yoksa n8n/cron ile tetiklenebilir).
#[utoipa::path(post, path = "/admin/retention/run", tag = "Privacy", responses((status = 200)))]
pub async fn run_retention(State(st): State<Shared>, headers: HeaderMap) -> ApiResult<Response> {
    let ident = st.require(&headers, "admin").await?;
    let days = osiris_db::default_retention_days();
    let (items, entities) = osiris_db::purge_expired(&st.pool, days, 1000, 1000)
        .await
        .map_err(|e| ApiError::internal("Saklama temizliği başarısız", e))?;
    let res = json!({"items_deleted": items, "entities_deleted": entities, "default_days": days});
    st.audit(&ident.user, "retention.run", None, res.clone()).await;
    Ok(ok(res))
}

// ============================================================================
// Web arayüzü uçları (E11 güncellemesi): kütüphanede hazır olan semantik arama, graf analizi
// ve dışa aktarımı, çok biçimli rapor; kaynak detay/güncelleme.
// ============================================================================

/// `Option<Option<T>>` için: alan yok → None, `null` → Some(None), değer → Some(Some(v)).
fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// Kaynak config'inde sır taşıyan anahtarlar: GET'te maskelenir, PATCH'te maske eski değeri korur.
static SECRET_KEY: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)(api_key|access_token|token|password|secret)").expect("secret")
});
pub const SECRET_MASK: &str = "********";

fn mask_secrets(config: &mut Map<String, Value>) {
    for (k, v) in config.iter_mut() {
        let empty = v.is_null() || v.as_str() == Some("");
        if SECRET_KEY.is_match(k) && !empty {
            *v = Value::String(SECRET_MASK.into());
        }
    }
}

/// Kaynak detayı: plugin config'i dahil; `api_key`, `token`, `password`, `secret` içeren
/// alanlar `********` olarak döner.
#[utoipa::path(get, path = "/sources/{source_id}", tag = "Sources",
    params(("source_id" = String, Path)), responses((status = 200), (status = 404)))]
pub async fn get_source(
    State(st): State<Shared>,
    headers: HeaderMap,
    Path(source_id): Path<String>,
) -> ApiResult<Response> {
    st.require(&headers, "viewer").await?;
    let source_id = uuid_or_404(&source_id, "Kaynak")?;
    let fail = |e: &dyn std::fmt::Display| ApiError::internal("Kaynak okunamadı", e);
    let row: Option<String> = sqlx::query_scalar(concat!(
        "SELECT json_build_object('id', id::text, 'name', name, 'url', url, ",
        "'network_type', network_type::text, 'plugin_id', plugin_id, 'schedule', schedule, ",
        "'priority', priority, 'enabled', enabled, 'tags', tags, 'last_crawled_at', ",
        ts!("last_crawled_at"),
        ", 'last_success_at', ",
        ts!("last_success_at"),
        ", 'failure_count', failure_count, 'avg_response_ms', avg_response_ms, ",
        "'retention_days', retention_days, 'created_at', ",
        ts!("created_at"),
        ", 'updated_at', ",
        ts!("updated_at"),
        ", 'config', COALESCE(metadata->'config', '{}'::jsonb))::text ",
        "FROM sources WHERE id::text = $1"
    ))
    .bind(source_id)
    .fetch_optional(&st.pool)
    .await
    .map_err(|e| fail(&e))?;
    let text = row.ok_or_else(|| ApiError::not_found("Kaynak bulunamadı"))?;
    let mut out: Value = serde_json::from_str(&text).map_err(|e| fail(&e))?;
    if let Some(Value::Object(cfg)) = out.get_mut("config") {
        mask_secrets(cfg);
    } else {
        out["config"] = json!({});
    }
    Ok(ok(out))
}

#[derive(Deserialize, ToSchema)]
pub struct SourceUpdateRequest {
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    name: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    url: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    schedule: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<i64>)]
    priority: Option<Option<LaxInt>>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<bool>)]
    enabled: Option<Option<LaxBool>>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<Vec<String>>)]
    tags: Option<Option<Vec<String>>>,
    /// Sığ birleştirme: verilen anahtarlar yazılır, `null` anahtarı siler, `********` eski
    /// sırrı korur.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<Object>)]
    config: Option<Option<Map<String, Value>>>,
    /// `null` → sunucu varsayılanı, 0 → süreyle silinmez
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<i64>)]
    retention_days: Option<Option<LaxInt>>,
}

impl Validate for SourceUpdateRequest {
    fn validate(&self, v: &mut Violations) {
        if let Some(Some(n)) = &self.name {
            v.len("name", n, 1, 500);
        }
        if let Some(Some(u)) = &self.url {
            v.len("url", u, 0, 2048);
        }
        if let Some(Some(s)) = &self.schedule {
            v.len("schedule", s, 0, 100);
        }
        if let Some(Some(p)) = self.priority {
            v.range("priority", p.0, 1, 10);
        }
        if let Some(Some(t)) = &self.tags {
            v.items("tags", t.len(), 0, 20);
        }
        if let Some(Some(c)) = &self.config {
            v.items("config", c.len(), 0, 50);
        }
        if let Some(Some(d)) = self.retention_days {
            v.range("retention_days", d.0, 0, 36_500);
        }
    }
}

impl SourceUpdateRequest {
    fn fields(&self) -> Vec<&'static str> {
        let mut f = Vec::new();
        let mut add = |present: bool, name: &'static str| {
            if present {
                f.push(name);
            }
        };
        add(self.config.is_some(), "config");
        add(self.enabled.is_some(), "enabled");
        add(self.name.is_some(), "name");
        add(self.priority.is_some(), "priority");
        add(self.retention_days.is_some(), "retention_days");
        add(self.schedule.is_some(), "schedule");
        add(self.tags.is_some(), "tags");
        add(self.url.is_some(), "url");
        f
    }
}

/// Kaynağı kısmen günceller: yalnızca gövdede verilen alanlar değişir (etkinleştir/kapat,
/// zamanlama, öncelik, etiketler, config, saklama süresi).
#[utoipa::path(patch, path = "/sources/{source_id}", tag = "Sources", request_body = SourceUpdateRequest,
    params(("source_id" = String, Path)),
    responses((status = 200), (status = 404), (status = 413), (status = 422)))]
pub async fn update_source(
    State(st): State<Shared>,
    headers: HeaderMap,
    Path(source_id): Path<String>,
    Body(body): Body,
) -> ApiResult<Response> {
    let ident = st.require(&headers, "analyst").await?;
    let req: SourceUpdateRequest = parse(body)?;
    let source_id = uuid_or_404(&source_id, "Kaynak")?;
    let fields = req.fields();
    if fields.is_empty() {
        return Err(ApiError::unprocessable("Güncellenecek alan yok"));
    }
    for (name, null) in [
        ("name", matches!(req.name, Some(None))),
        ("priority", matches!(req.priority, Some(None))),
        ("enabled", matches!(req.enabled, Some(None))),
    ] {
        if null {
            return Err(ApiError::unprocessable(format!("{name} boş olamaz")));
        }
    }
    if let Some(Some(c)) = &req.config
        && config_too_big(c)
    {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "config çok büyük"));
    }
    let res: Result<bool, sqlx::Error> = async {
        let mut tx = st.pool.begin().await?;
        let meta: Option<String> = sqlx::query_scalar(
            "SELECT COALESCE(metadata, '{}'::jsonb)::text FROM sources WHERE id::text = $1 FOR UPDATE",
        )
        .bind(source_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(meta) = meta else {
            return Ok(false);
        };
        // Sütun adları yalnızca bu sabit parçalardan gelir; değerler bağlı parametredir.
        let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new("UPDATE sources SET updated_at = NOW()");
        if let Some(Some(n)) = &req.name {
            qb.push(", name = ").push_bind(n.clone());
        }
        if let Some(u) = &req.url {
            qb.push(", url = NULLIF(").push_bind(u.clone().unwrap_or_default()).push(", '')");
        }
        if let Some(s) = &req.schedule {
            qb.push(", schedule = NULLIF(").push_bind(s.clone().unwrap_or_default()).push(", '')");
        }
        if let Some(Some(p)) = req.priority {
            qb.push(", priority = ").push_bind(p.0 as i32);
        }
        if let Some(Some(e)) = req.enabled {
            qb.push(", enabled = ").push_bind(e.0);
        }
        if let Some(t) = &req.tags {
            let tags: Vec<String> =
                t.iter().flatten().take(20).map(|x| chars(x, 100)).collect();
            qb.push(", tags = ").push_bind(tags);
        }
        if let Some(d) = req.retention_days {
            qb.push(", retention_days = ").push_bind(d.map(|x| x.0 as i32));
        }
        if let Some(c) = &req.config {
            let mut meta: Value = serde_json::from_str(&meta).unwrap_or_else(|_| json!({}));
            if !meta.is_object() {
                meta = json!({});
            }
            let mut cfg = meta.get("config").and_then(Value::as_object).cloned().unwrap_or_default();
            for (k, v) in c.iter().flatten() {
                if v.is_null() {
                    cfg.remove(k);
                } else if v.as_str() != Some(SECRET_MASK) {
                    cfg.insert(chars(k, 100), v.clone());
                }
            }
            meta["config"] = Value::Object(cfg);
            qb.push(", metadata = ").push_bind(meta.to_string()).push("::jsonb");
        }
        qb.push(" WHERE id::text = ").push_bind(source_id);
        qb.build().execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(true)
    }
    .await;
    let found = res.map_err(|e| ApiError::internal("Kaynak güncellenemedi", e))?;
    if !found {
        return Err(ApiError::not_found("Kaynak bulunamadı"));
    }
    let detail = json!({"id": source_id, "fields": fields});
    st.audit(&ident.user, "source.update", None, detail).await;
    Ok(ok(json!({"status": "updated"})))
}

// ------------------------------------------------------------------------ semantik arama

/// Anlamca yakın öğeler (`similarity` 0–1). Sorgu vektörünü API üretir: `OSIRIS_EMBEDDING_MODEL`
/// ve modeller (`osiris-server models`) yoksa 503.
#[utoipa::path(post, path = "/search/semantic", tag = "Search", request_body = SearchRequest,
    responses((status = 200), (status = 400), (status = 503, description = "Model yapılandırılmamış")))]
pub async fn semantic_search(
    State(st): State<Shared>,
    headers: HeaderMap,
    Body(body): Body,
) -> ApiResult<Response> {
    let ident = st.require(&headers, "viewer").await?;
    let req: SearchRequest = parse(body)?;
    let Some(model) = st.embedder().await else {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Semantik arama yapılandırılmamış: OSIRIS_EMBEDDING_MODEL ve modeller (osiris-server models) gerekli",
        ));
    };
    let text = req.query.clone();
    let vec = tokio::task::spawn_blocking(move || model.embed(&text))
        .await
        .map_err(|e| ApiError::internal("Sorgu vektörü üretilemedi", e))?
        .ok_or_else(|| ApiError::internal("Sorgu vektörü üretilemedi", "boş vektör"))?;
    let vec: Vec<f64> = vec.into_iter().map(f64::from).collect();
    let rows = osiris_query::semantic_search(&st.pool, &vec, req.limit.0)
        .await
        .map_err(query_error("Arama başarısız"))?;
    let hits = osiris_query::count_rows(&rows);
    let detail = json!({"query": chars(&req.query, 200), "hits": hits});
    st.audit(&ident.user, "search.semantic", None, detail).await;
    Ok(raw_json(rows))
}

// ------------------------------------------------------------------- graf analizi / dışa aktarım

/// Topluluk tespiti (CNM) büyük graflarda pahalıdır; üstünde yalnızca merkezilik döner.
const COMMUNITY_MAX_NODES: usize = 5000;

/// Derece merkeziliği (ilk `top`, 1-200), CNM toplulukları (en büyük 50, üye ≤ 200; ≤ 5000
/// düğümde) ve `node` verilirse komşular. Bkz. SOZLESME §9.
#[utoipa::path(get, path = "/graph/analysis", tag = "Graph",
    params(("top" = Option<i64>, Query), ("node" = Option<String>, Query)), responses((status = 200)))]
pub async fn graph_analysis(
    State(st): State<Shared>,
    headers: HeaderMap,
    RawQuery(q): RawQuery,
) -> ApiResult<Response> {
    st.require(&headers, "viewer").await?;
    let p = Params::from_raw(q.as_deref());
    let top = p.int("top", 25)?.clamp(1, 200) as usize;
    let node = p.get("node").map(|n| chars(n, 500));
    // Kopya üzerinde hesapla: kilit kısa tutulur, CPU işi async çalışanı bloklamaz
    let graph = st.graph().await.graph.clone();
    let out = tokio::task::spawn_blocking(move || {
        let n = graph.number_of_nodes();
        let mut cent: Vec<(String, f64)> = graph.centrality().into_iter().collect();
        cent.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let centrality: Vec<Value> = cent
            .into_iter()
            .take(top)
            .map(|(id, score)| {
                let degree = graph.neighbors(&id).len();
                json!({"id": id, "score": (score * 1e6).round() / 1e6, "degree": degree})
            })
            .collect();
        let communities = (n <= COMMUNITY_MAX_NODES).then(|| {
            let mut cs: Vec<Vec<String>> = graph.communities();
            for c in &mut cs {
                c.sort();
            }
            cs.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.first().cmp(&b.first())));
            cs.into_iter()
                .take(50)
                .map(|c| json!({"size": c.len(), "members": c.into_iter().take(200).collect::<Vec<_>>()}))
                .collect::<Vec<_>>()
        });
        let neighbors = node.map(|id| {
            let mut nb = graph.neighbors(osiris_hash::py_strip(&id));
            nb.sort();
            nb.truncate(500);
            nb
        });
        json!({
            "nodes": n,
            "edges": graph.number_of_edges(),
            "centrality": centrality,
            "communities": communities,
            "neighbors": neighbors,
        })
    })
    .await
    .map_err(|e| ApiError::internal("Graf analizi başarısız", e))?;
    Ok(ok(out))
}

/// Grafı GraphML ya da GEXF olarak dışa aktarır (`format` ∈ graphml|gexf; Gephi vb.).
#[utoipa::path(get, path = "/graph/export", tag = "Graph",
    params(("format" = Option<String>, Query)), responses((status = 200), (status = 400)))]
pub async fn graph_export(
    State(st): State<Shared>,
    headers: HeaderMap,
    RawQuery(q): RawQuery,
) -> ApiResult<Response> {
    st.require(&headers, "viewer").await?;
    let p = Params::from_raw(q.as_deref());
    let fmt = p.get("format").unwrap_or("graphml").to_ascii_lowercase();
    if fmt != "graphml" && fmt != "gexf" {
        return Err(ApiError::bad_request("format graphml veya gexf olmalı"));
    }
    let graph = st.graph().await.graph.clone();
    let body = tokio::task::spawn_blocking(move || {
        if fmt == "graphml" { (graph.graphml(), "graphml") } else { (graph.gexf(), "gexf") }
    })
    .await
    .map_err(|e| ApiError::internal("Graf dışa aktarılamadı", e))?;
    let disposition = format!("attachment; filename=\"osiris-graf.{}\"", body.1);
    Ok((
        [
            (header::CONTENT_TYPE, "application/xml".to_owned()),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        body.0,
    )
        .into_response())
}

// ------------------------------------------------------------------ çok biçimli rapor

fn d_markdown() -> String {
    "markdown".into()
}

fn d_osiris() -> String {
    "OSIRIS".into()
}

#[derive(Deserialize, ToSchema)]
pub struct ReportRequest {
    title: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    #[schema(value_type = Vec<Object>)]
    findings: Vec<Map<String, Value>>,
    #[serde(default)]
    sources: Vec<String>,
    /// markdown | html | json | stix
    #[serde(default = "d_markdown")]
    format: String,
    /// STIX/JSON göstergeleri: `[{"type": "ip", "value": "203.0.113.7"}]`
    #[serde(default)]
    #[schema(value_type = Vec<Object>)]
    entities: Vec<Map<String, Value>>,
    #[serde(default = "d_osiris")]
    identity_name: String,
}

impl Validate for ReportRequest {
    fn validate(&self, v: &mut Violations) {
        v.len("title", &self.title, 1, 500);
        v.len("scope", &self.scope, 0, 1000);
        v.len("summary", &self.summary, 0, 20_000);
        v.items("findings", self.findings.len(), 0, 500);
        v.items("sources", self.sources.len(), 0, 500);
        v.len("format", &self.format, 0, 16);
        v.items("entities", self.entities.len(), 0, 500);
        v.len("identity_name", &self.identity_name, 0, 200);
    }
}

fn value_str(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// İndirme dosya adı: ASCII (Türkçe harfler sadeleşir: "Dış yüzey" → "Dis-yuzey").
fn report_stem(title: &str) -> String {
    let ascii: String = title
        .chars()
        .map(|c| match c {
            'ı' | 'î' | 'í' | 'ì' | 'ï' => 'i',
            'İ' | 'Î' | 'Í' => 'I',
            'ş' | 'ș' => 's',
            'Ş' | 'Ș' => 'S',
            'ğ' => 'g',
            'Ğ' => 'G',
            'ü' | 'û' | 'ú' | 'ù' => 'u',
            'Ü' | 'Û' | 'Ú' => 'U',
            'ö' | 'ô' | 'ó' | 'ò' => 'o',
            'Ö' | 'Ô' | 'Ó' => 'O',
            'ç' => 'c',
            'Ç' => 'C',
            'â' | 'á' | 'à' | 'ä' => 'a',
            'Â' | 'Á' | 'Ä' => 'A',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'É' | 'È' => 'E',
            other => other,
        })
        .collect();
    static NON_FILE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"[^A-Za-z0-9_-]+").expect("dosya"));
    let stem = NON_FILE.replace_all(&ascii, "-");
    let stem: String = stem.trim_matches('-').chars().take(60).collect();
    if stem.is_empty() { "osiris-rapor".into() } else { stem }
}

/// Rapor üretir: `format` ∈ markdown|html|json|stix (gövdeyle döner, diske yazmaz).
/// STIX yalnızca ip/domain/email/crypto_address/hash/cve göstergelerini içerir.
#[utoipa::path(post, path = "/reports/generate", tag = "Reports", request_body = ReportRequest,
    responses((status = 200), (status = 400)))]
pub async fn report_generate(
    State(st): State<Shared>,
    headers: HeaderMap,
    Body(body): Body,
) -> ApiResult<Response> {
    st.require(&headers, "analyst").await?;
    let req: ReportRequest = parse(body)?;
    let fmt = req.format.to_ascii_lowercase();
    let (media, ext) = match fmt.as_str() {
        "markdown" => ("text/markdown", "md"),
        "html" => ("text/html", "html"),
        "json" => ("application/json", "json"),
        "stix" => ("application/stix+json", "stix.json"),
        _ => return Err(ApiError::bad_request("format markdown, html, json veya stix olmalı")),
    };
    let entities: Vec<Value> = req
        .entities
        .iter()
        .map(|e| {
            json!({
                "type": chars(&value_str(e.get("type")), 32),
                "value": chars(&value_str(e.get("value")), 500),
            })
        })
        .collect();
    let content = match fmt.as_str() {
        "markdown" | "html" => {
            let findings: Vec<Json> =
                req.findings.into_iter().map(|f| Json::from(Value::Object(f))).collect();
            let sources: Vec<Json> = req.sources.into_iter().map(Json::Str).collect();
            let input = osiris_report::ReportInput {
                title: &req.title,
                scope: &req.scope,
                summary: &req.summary,
                findings: &findings,
                sources: &sources,
            };
            if fmt == "html" {
                osiris_report::html(&input)
            } else {
                osiris_report::markdown(&input)
            }
        }
        "json" => {
            let findings: Vec<Value> = req
                .findings
                .iter()
                .take(500)
                .map(|f| {
                    json!({
                        "title": chars(&value_str(f.get("title")), 500),
                        "description": chars(&value_str(f.get("description")), 5000),
                    })
                })
                .collect();
            let sources: Vec<String> =
                req.sources.iter().take(500).map(|s| chars(s, 1000)).collect();
            let data = json!({
                "title": chars(&req.title, 500),
                "generated_at": osiris_report::py_isoformat(chrono::Utc::now()),
                "scope": req.scope,
                "summary": req.summary,
                "findings": findings,
                "sources": sources,
                "entities": entities,
            });
            osiris_report::json(&Json::from(data))
        }
        _ => {
            let ents: Vec<Json> = entities.into_iter().map(Json::from).collect();
            osiris_report::stix(&req.identity_name, &req.title, &ents, chrono::Utc::now(), None)
                .map_err(|e| match e {
                    osiris_report::ReportError::Invalid(m) => ApiError::bad_request(m),
                    other => ApiError::internal("Rapor üretilemedi", other),
                })?
        }
    };
    let filename = format!("{}.{ext}", report_stem(&req.title));
    Ok(ok(json!({"format": fmt, "media_type": media, "filename": filename, "content": content})))
}
