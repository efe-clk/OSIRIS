//! OpenAPI 3 belgesi (`/openapi.json`, utoipa) ve Swagger UI (`/docs`).
//!
//! `/docs` belgeyi **göreli** yoldan yükler: Nginx `/api/` önekiyle de çalışır
//! (`/api/docs` → `/api/openapi.json`). Swagger UI dosyaları FastAPI'deki gibi jsDelivr'dan
//! gelir; imaja gömülmez.

use super::Shared;
use super::routes::*;
use axum::Router;
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

struct Security;

impl Modify for Security {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let c = openapi.components.get_or_insert_with(Default::default);
        c.add_security_scheme(
            "api_key",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("X-API-Key"))),
        );
        c.add_security_scheme(
            "bearer",
            SecurityScheme::Http(
                HttpBuilder::new().scheme(HttpAuthScheme::Bearer).bearer_format("JWT").build(),
            ),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "OSIRIS API",
        description = "OSIRIS OSINT platformu REST arayüzü (Rust). Kimlik: `X-API-Key` \
                       (operatör/kullanıcı) veya `Authorization: Bearer <JWT>`. Roller: viewer → \
                       analyst → admin. Ayrıntılı başvuru: docs/API.md"
    ),
    paths(
        health, issue_token, create_api_key, list_api_keys, revoke_api_key, list_plugins, stats,
        recent_items, item_detail, top_entities, collect_batch, collect, search, entity_search,
        graph, graph_add, list_sources, create_source, delete_source, collect_source,
        list_saved_queries, create_saved_query, delete_saved_query, test_alerts, report_markdown,
        semantic_search, graph_analysis, graph_export, get_source, update_source, report_generate,
        delete_item, erase_entity, run_retention
    ),
    modifiers(&Security),
    security(("api_key" = []), ("bearer" = []))
)]
pub struct ApiDoc;

const DOCS: &str = r##"<!DOCTYPE html>
<html lang="tr"><head><meta charset="utf-8"><title>OSIRIS API - Swagger UI</title>
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui.css">
</head><body><div id="swagger-ui"></div>
<script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
<script>SwaggerUIBundle({url: "openapi.json", dom_id: "#swagger-ui"});</script>
</body></html>"##;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/openapi.json", get(|| async { axum::Json(ApiDoc::openapi()).into_response() }))
        .route("/docs", get(|| async { Html(DOCS) }))
}
