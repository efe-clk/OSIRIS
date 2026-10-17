//! Hata gövdesi: her zaman `{"detail": …}`, iç ayrıntı (SQL, yığın izi, sürücü iletisi) yok.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub detail: Value,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, detail: impl Into<String>) -> Self {
        Self { status, detail: Value::String(detail.into()) }
    }

    /// 422 — pydantic biçimine benzer hata listesi.
    pub fn validation(errors: Vec<Value>) -> Self {
        Self { status: StatusCode::UNPROCESSABLE_ENTITY, detail: Value::Array(errors) }
    }

    pub fn unprocessable(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, detail)
    }

    pub fn bad_request(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, detail)
    }

    pub fn not_found(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, detail)
    }

    /// 500: iç hata loglanır, gövdeye yalnızca genel ileti yazılır.
    pub fn internal(detail: &str, cause: impl std::fmt::Display) -> Self {
        tracing::error!(error = %cause, "{detail}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, detail)
    }
}

/// `{"type", "loc", "msg"}` doğrulama kaydı.
pub fn violation(loc: &[&str], kind: &str, msg: impl Into<String>) -> Value {
    json!({"type": kind, "loc": loc, "msg": msg.into()})
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, axum::Json(json!({"detail": self.detail}))).into_response()
    }
}
