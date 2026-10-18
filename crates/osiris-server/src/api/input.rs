//! İstek girdisi — FastAPI/pydantic davranışının karşılığı.
//!
//! - Gövde önce yalnızca JSON olarak okunur ([`Body`]): bozuk JSON kimlik denetiminden
//!   **önce** 422 olur (FastAPI gibi); alan doğrulaması kimlikten **sonra** ([`parse`]).
//! - Tamsayı/bool alanları pydantic "lax" kuralıyla kabul edilir ([`LaxInt`], [`LaxBool`]):
//!   `"20"` → 20, `5.0` → 5, `"yes"` → true (n8n iş akışı sayıları dize gönderebilir).
//! - Uzunluklar Unicode kod noktası sayılır.

use super::error::{ApiError, ApiResult, violation};
use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use serde::Deserialize;
use serde::de::{self, DeserializeOwned, Deserializer};
use serde_json::Value;

/// Ham JSON gövdesi (boşsa `None`).
pub struct Body(pub Option<Value>);

impl<S: Send + Sync> FromRequest<S> for Body {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = Bytes::from_request(req, state).await.map_err(|e| {
            if e.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
                ApiError::new(e.status(), "İstek gövdesi çok büyük")
            } else {
                ApiError::bad_request("İstek gövdesi okunamadı")
            }
        })?;
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return Ok(Self(None));
        }
        serde_json::from_slice(&bytes).map(|v| Self(Some(v))).map_err(|e| {
            ApiError::validation(vec![violation(
                &["body"],
                "json_invalid",
                format!("JSON decode error: {e}"),
            )])
        })
    }
}

/// Alan kuralları (pydantic `Field(...)` karşılığı).
pub trait Validate {
    fn validate(&self, v: &mut Violations);
}

#[derive(Default)]
pub struct Violations(pub Vec<Value>);

impl Violations {
    fn push(&mut self, field: &str, kind: &str, msg: String) {
        self.0.push(violation(&["body", field], kind, msg));
    }

    /// `min_length` / `max_length` (kod noktası).
    pub fn len(&mut self, field: &str, value: &str, min: usize, max: usize) {
        let n = value.chars().count();
        if n < min {
            self.push(
                field,
                "string_too_short",
                format!("String should have at least {min} character"),
            );
        } else if n > max {
            self.push(
                field,
                "string_too_long",
                format!("String should have at most {max} characters"),
            );
        }
    }

    /// `ge` / `le`
    pub fn range(&mut self, field: &str, value: i64, min: i64, max: i64) {
        if value < min {
            self.push(
                field,
                "greater_than_equal",
                format!("Input should be greater than or equal to {min}"),
            );
        } else if value > max {
            self.push(
                field,
                "less_than_equal",
                format!("Input should be less than or equal to {max}"),
            );
        }
    }

    /// liste/sözlük `max_length` (öğe sayısı)
    pub fn items(&mut self, field: &str, n: usize, min: usize, max: usize) {
        if n < min {
            self.push(field, "too_short", format!("List should have at least {min} item"));
        } else if n > max {
            self.push(field, "too_long", format!("Value should have at most {max} items"));
        }
    }

    /// Boş değilse 5 alanlı cron (collector'ın kabul ettiği biçim; aksi hâlde kaynak hiç çalışmaz).
    pub fn cron(&mut self, field: &str, value: &str) {
        if !value.trim().is_empty()
            && let Err(e) = osiris_collector::parse_cron(value)
        {
            self.push(field, "cron", e);
        }
    }

    pub fn pattern(&mut self, field: &str, value: &str, re: &regex::Regex) {
        if !re.is_match(value) {
            self.push(
                field,
                "string_pattern_mismatch",
                format!("String should match pattern '{}'", re.as_str()),
            );
        }
    }
}

/// Gövdeyi modele çevirir ve doğrular (422).
pub fn parse<T: DeserializeOwned + Validate>(body: Option<Value>) -> ApiResult<T> {
    let Some(value) = body else {
        return Err(ApiError::validation(vec![violation(&["body"], "missing", "Field required")]));
    };
    let model: T = serde_json::from_value(value).map_err(|e| {
        ApiError::validation(vec![violation(&["body"], "value_error", e.to_string())])
    })?;
    let mut v = Violations::default();
    model.validate(&mut v);
    if v.0.is_empty() { Ok(model) } else { Err(ApiError::validation(v.0)) }
}

pub static PLUGIN_ID: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"^[a-z0-9][a-z0-9-]{0,63}$").expect("plugin_id")
});

/// pydantic lax `int`: tamsayı, kesirsiz float, tamsayı dizesi (bool ve null hata).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaxInt(pub i64);

impl<'de> Deserialize<'de> for LaxInt {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        lax_int(&v).map(LaxInt).ok_or_else(|| de::Error::custom("Input should be a valid integer"))
    }
}

pub fn lax_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| {
            n.as_f64().filter(|f| f.fract() == 0.0 && f.abs() < 9.0e18).map(|f| f as i64)
        }),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// pydantic lax `bool`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaxBool(pub bool);

impl<'de> Deserialize<'de> for LaxBool {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let b = match &v {
            Value::Bool(b) => Some(*b),
            Value::Number(n) => match n.as_f64() {
                Some(0.0) => Some(false),
                Some(1.0) => Some(true),
                _ => None,
            },
            Value::String(s) => lax_bool_str(s),
            _ => None,
        };
        b.map(LaxBool).ok_or_else(|| de::Error::custom("Input should be a valid boolean"))
    }
}

pub fn lax_bool_str(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" | "t" | "y" => Some(true),
        "false" | "0" | "no" | "off" | "f" | "n" => Some(false),
        _ => None,
    }
}

/// Sorgu dizgesi parametreleri (aynı ad tekrar ederse sonuncusu geçerli).
pub struct Params(pub Vec<(String, String)>);

impl Params {
    pub fn from_raw(raw: Option<&str>) -> Self {
        let pairs = raw
            .map(|q| {
                q.split('&')
                    .filter(|p| !p.is_empty())
                    .map(|p| {
                        let (k, v) = p.split_once('=').unwrap_or((p, ""));
                        (decode(k), decode(v))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self(pairs)
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    fn invalid(name: &str, msg: &str) -> ApiError {
        ApiError::validation(vec![violation(&["query", name], "parsing", msg)])
    }

    pub fn int(&self, name: &str, default: i64) -> ApiResult<i64> {
        match self.get(name) {
            None => Ok(default),
            Some(v) => v.trim().parse().map_err(|_| {
                Self::invalid(
                    name,
                    "Input should be a valid integer, unable to parse string as an integer",
                )
            }),
        }
    }

    pub fn opt_bool(&self, name: &str) -> ApiResult<Option<bool>> {
        match self.get(name) {
            None => Ok(None),
            Some(v) => lax_bool_str(v).map(Some).ok_or_else(|| {
                Self::invalid(name, "Input should be a valid boolean, unable to interpret input")
            }),
        }
    }

    /// Boş dize `None` (Python'da falsy → filtre yok).
    pub fn text(&self, name: &str) -> Option<&str> {
        self.get(name).filter(|v| !v.is_empty())
    }
}

/// `application/x-www-form-urlencoded` çözümü (`+` → boşluk, `%XX`).
fn decode(s: &str) -> String {
    let s = s.replace('+', " ");
    percent_encoding::percent_decode_str(&s).decode_utf8_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lax_ve_parametreler() {
        assert_eq!(lax_int(&json!("20")), Some(20));
        assert_eq!(lax_int(&json!(5.0)), Some(5));
        assert_eq!(lax_int(&json!(5.5)), None);
        assert_eq!(lax_int(&json!(true)), None);
        assert_eq!(lax_bool_str("Yes"), Some(true));
        let p = Params::from_raw(Some("limit=1&limit=abc&q=a%25b+c&x"));
        assert_eq!(p.get("q"), Some("a%b c"));
        assert!(p.int("limit", 5).is_err());
        assert_eq!(p.int("offset", 5).unwrap(), 5);
        assert_eq!(p.text("x"), None);
    }
}
