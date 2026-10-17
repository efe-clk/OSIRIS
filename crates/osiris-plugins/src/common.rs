//! Plugin'lerin ortak Python semantiği yardımcıları.

use osiris_hash::Json;
use osiris_plugin::PluginCrash;
use osiris_types::Config;
use osiris_types::pyval::{PyConvError, float_str, py_int, repr_str};
use serde_json::{Map, Value, json};

pub const UA: &str = "OSIRIS-OSINT/0.1 (+self-hosted)";

/// `x = cfg.get(k); if not x or not isinstance(x, str): hata` → `Some(&str)` ya da None.
pub fn cfg_str<'a>(cfg: &'a Config, key: &str) -> Option<&'a str> {
    match cfg.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Some(s),
        _ => None,
    }
}

/// `max(lo, min(int(cfg.get(k, default)), hi))`; geçersiz → `default`; OverflowError → çökme.
pub fn int_cfg(
    cfg: &Config,
    key: &str,
    default: i64,
    lo: i64,
    hi: i64,
) -> Result<i64, PluginCrash> {
    let v = match cfg.get(key) {
        None => return Ok(default.clamp(lo, hi)),
        Some(v) => v,
    };
    match py_int(v) {
        Ok(i) => Ok(i.clamp(lo, hi)),
        Err(PyConvError::Invalid) => Ok(default),
        Err(PyConvError::Overflow) => {
            Err(PluginCrash("OverflowError: cannot convert float infinity to integer".into()))
        }
    }
}

/// `json!({...})` → metadata haritası.
pub fn meta(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

pub fn headers_ua() -> Vec<(String, String)> {
    vec![("User-Agent".into(), UA.into())]
}

/// `osiris_hash::Json` → serde değeri (metadata için; i64 dışı tamsayı yaklaşık float olur).
pub fn to_value(j: &Json) -> Value {
    match j {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Int(i) => json!(i),
        Json::BigInt(s) => s.parse::<u64>().map(Value::from).unwrap_or_else(|_| {
            serde_json::Number::from_f64(s.parse::<f64>().unwrap_or(0.0))
                .map_or(Value::Null, Value::Number)
        }),
        Json::Float(f) => serde_json::Number::from_f64(*f).map_or(Value::Null, Value::Number),
        Json::Str(s) => Value::String(s.clone()),
        Json::Array(a) => Value::Array(a.iter().map(to_value).collect()),
        Json::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), to_value(v))).collect()),
    }
}

/// `str(x)` (osiris_hash::Json için; kaplar repr).
pub fn json_str(j: &Json) -> String {
    match j {
        Json::Str(s) => s.clone(),
        other => json_repr(other),
    }
}

pub fn json_repr(j: &Json) -> String {
    match j {
        Json::Null => "None".into(),
        Json::Bool(true) => "True".into(),
        Json::Bool(false) => "False".into(),
        Json::Int(i) => i.to_string(),
        Json::BigInt(s) => s.clone(),
        Json::Float(f) => float_str(*f),
        Json::Str(s) => repr_str(s),
        Json::Array(a) => format!("[{}]", a.iter().map(json_repr).collect::<Vec<_>>().join(", ")),
        Json::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| format!("{}: {}", repr_str(k), json_repr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Python `dict.get(k)`
pub fn jget<'a>(j: &'a Json, key: &str) -> Option<&'a Json> {
    match j {
        Json::Object(o) => o.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// Python `bool(x)` (Json)
pub fn jtruthy(j: &Json) -> bool {
    !j.is_falsy()
}

/// Python `a - b` (sayısal JSON değerleri; diğer türler TypeError → çökme).
pub fn py_sub(a: &Json, b: &Json) -> Result<String, PluginCrash> {
    fn as_int(j: &Json) -> Option<i128> {
        match j {
            Json::Bool(b) => Some(i128::from(*b)),
            Json::Int(i) => Some(i128::from(*i)),
            Json::BigInt(s) => s.parse().ok(),
            _ => None,
        }
    }
    fn as_float(j: &Json) -> Option<f64> {
        match j {
            Json::Float(f) => Some(*f),
            other => as_int(other).map(|i| i as f64),
        }
    }
    if let (Some(x), Some(y)) = (as_int(a), as_int(b)) {
        return Ok((x - y).to_string());
    }
    match (as_float(a), as_float(b)) {
        (Some(x), Some(y)) => Ok(float_str(x - y)),
        _ => Err(PluginCrash("TypeError: unsupported operand type(s) for -".into())),
    }
}

/// Python `str.splitlines()`
pub fn splitlines(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\r' => {
                if it.peek() == Some(&'\n') {
                    it.next();
                }
                out.push(std::mem::take(&mut cur));
            }
            '\n' | '\u{b}' | '\u{c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}'
            | '\u{2029}' => {
                out.push(std::mem::take(&mut cur));
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Python `min(a, b)` / `max(a, b)` (NaN dahil karşılaştırma sırasıyla).
pub fn py_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

pub fn py_max(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yardimcilar() {
        assert_eq!(splitlines("a\r\nb\rc\nd\u{2028}e\n"), vec!["a", "b", "c", "d", "e"]);
        assert_eq!(splitlines(""), Vec::<String>::new());
        assert_eq!(py_sub(&Json::Int(7000), &Json::Int(2500)).unwrap(), "4500");
        assert_eq!(py_sub(&Json::Float(1.5), &Json::Int(1)).unwrap(), "0.5");
        assert!(py_sub(&Json::Str("x".into()), &Json::Int(1)).is_err());
        assert_eq!(py_max(0.1, py_min(f64::NAN, 50.0)), 0.1);
        assert_eq!(
            json_str(&Json::Array(vec![Json::Null, Json::Bool(true), Json::Str("a'".into())])),
            "[None, True, \"a'\"]"
        );
    }
}
