//! Diller arası JSON değeri: Python `json`/psycopg nesnelerinin kayıpsız karşılığı.
//!
//! `serde_json::Value` yerine kendi türümüz: Python tamsayıları sınırsızdır (`BigInt`) ve
//! iki kanonik biçim (v1 = Python `json.dumps`, v2 = RFC 8785) sayıları farklı yazar.

/// JSON değeri. Nesne anahtarları benzersizdir (Python `dict`); sıra kanonikleştirmede
/// yeniden belirlenir.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    /// `i64` dışındaki Python tamsayısı, işaretli ondalık dize (`"-123…"`).
    BigInt(String),
    Float(f64),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Python doğruluk değeri (`detail or {}` için).
    pub fn is_falsy(&self) -> bool {
        match self {
            Json::Null => true,
            Json::Bool(b) => !b,
            Json::Int(i) => *i == 0,
            Json::BigInt(_) => false,
            Json::Float(f) => *f == 0.0,
            Json::Str(s) => s.is_empty(),
            Json::Array(a) => a.is_empty(),
            Json::Object(o) => o.is_empty(),
        }
    }

    /// JSON metnini ayrıştırır (Python `json.loads` gibi: tamsayı literali tamsayı,
    /// kesirli/üslü literal float olur).
    pub fn parse(text: &str) -> Result<Json, serde_json::Error> {
        let v: serde_json::Value = serde_json::from_str(text)?;
        Ok(Json::from(v))
    }
}

impl From<serde_json::Value> for Json {
    fn from(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Null => Json::Null,
            serde_json::Value::Bool(b) => Json::Bool(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Json::Int(i)
                } else if let Some(u) = n.as_u64() {
                    Json::BigInt(u.to_string())
                } else {
                    Json::Float(n.as_f64().unwrap_or(f64::NAN))
                }
            }
            serde_json::Value::String(s) => Json::Str(s),
            serde_json::Value::Array(a) => Json::Array(a.into_iter().map(Json::from).collect()),
            serde_json::Value::Object(o) => {
                Json::Object(o.into_iter().map(|(k, v)| (k, Json::from(v))).collect())
            }
        }
    }
}

/// JSON dize kaçışı — Python `json.dumps(ensure_ascii=False)` ve RFC 8785 aynıdır:
/// `"` `\\` `\b` `\f` `\n` `\r` `\t` kısa biçim, diğer U+0000–U+001F `\u00xx` (küçük harf).
pub(crate) fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
