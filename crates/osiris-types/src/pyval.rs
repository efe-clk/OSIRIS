//! Plugin config değerlerinin Python semantiği (`int(x)`, `float(x)`, `str(x)`, `bool(x)`,
//! `repr(x)`, `s[:n]`). Plugin'ler Python'daki dönüşümlerle birebir aynı sonucu üretmelidir.

use osiris_hash::canonical::py_float_repr;
use serde_json::Value;
use std::sync::LazyLock;

/// `int()` / `float()` dönüşüm hatası.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PyConvError {
    /// Python `ValueError` / `TypeError` (plugin'ler genelde varsayılana düşer).
    Invalid,
    /// Python `OverflowError` (`int(float('inf'))`) — plugin'lerde yakalanmaz → plugin hatası.
    Overflow,
}

/// `s[:n]` — kod noktası bazında.
pub fn trunc(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((i, _)) => s[..i].to_owned(),
        None => s.to_owned(),
    }
}

/// `bool(x)`
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Python `int(str)`: kırpılmış, isteğe bağlı işaret, rakamlar (tek `_` ayırıcılar).
fn parse_int_str(s: &str) -> Result<i64, PyConvError> {
    let t = s.trim_matches(osiris_hash::py_is_space);
    let (neg, body) = match t.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if body.is_empty()
        || body.starts_with('_')
        || body.ends_with('_')
        || body.contains("__")
        || !body.chars().all(|c| c == '_' || c.is_ascii_digit())
    {
        // Python Unicode rakamları da kabul eder (ör. "١٢"); config'de pratikte yok
        return Err(PyConvError::Invalid);
    }
    let digits: String = body.chars().filter(|c| *c != '_').collect();
    let mag: i128 = digits.parse::<i128>().unwrap_or(i128::MAX);
    let v = if neg { -mag } else { mag };
    Ok(v.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
}

/// `int(x)` (sınırları aşan değerler doyurulur; çağıranlar zaten min/max ile kırpar).
pub fn py_int(v: &Value) -> Result<i64, PyConvError> {
    match v {
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i)
            } else if n.as_u64().is_some() {
                Ok(i64::MAX)
            } else {
                let f = n.as_f64().unwrap_or(f64::NAN);
                if f.is_nan() {
                    Err(PyConvError::Invalid)
                } else if f.is_infinite() {
                    Err(PyConvError::Overflow)
                } else {
                    Ok(f.trunc().clamp(i64::MIN as f64, i64::MAX as f64) as i64)
                }
            }
        }
        Value::String(s) => parse_int_str(s),
        _ => Err(PyConvError::Invalid),
    }
}

/// `float(x)`
pub fn py_float(v: &Value) -> Result<f64, PyConvError> {
    match v {
        Value::Bool(b) => Ok(f64::from(u8::from(*b))),
        Value::Number(n) => n.as_f64().ok_or(PyConvError::Invalid),
        Value::String(s) => {
            let t = s.trim_matches(osiris_hash::py_is_space);
            let lower = t.to_ascii_lowercase();
            let unsigned = lower.trim_start_matches(['+', '-']);
            let neg = lower.starts_with('-');
            if lower.len() - unsigned.len() > 1 {
                return Err(PyConvError::Invalid);
            }
            let special = match unsigned {
                "inf" | "infinity" => Some(f64::INFINITY),
                "nan" => Some(f64::NAN),
                _ => None,
            };
            if let Some(f) = special {
                return Ok(if neg { -f } else { f });
            }
            let ok_chars = unsigned.chars().all(|c| c.is_ascii_digit() || "._e+-".contains(c));
            let clean: String = t.chars().filter(|c| *c != '_').collect();
            if !ok_chars || t.contains("__") || unsigned.starts_with('_') || unsigned.is_empty() {
                return Err(PyConvError::Invalid);
            }
            clean.parse::<f64>().map_err(|_| PyConvError::Invalid)
        }
        _ => Err(PyConvError::Invalid),
    }
}

/// Python `float.__repr__`/`str` (sonlu olmayanlar dahil).
pub fn float_str(f: f64) -> String {
    if f.is_nan() {
        "nan".into()
    } else if f.is_infinite() {
        if f > 0.0 { "inf".into() } else { "-inf".into() }
    } else {
        py_float_repr(f)
    }
}

fn number_str(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        i.to_string()
    } else if let Some(u) = n.as_u64() {
        u.to_string()
    } else {
        float_str(n.as_f64().unwrap_or(f64::NAN))
    }
}

/// `str(x)` — dize olduğu gibi, diğerleri `repr`.
pub fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => py_repr(other),
    }
}

/// `repr(x)` (JSON'dan gelen değerler için: dict, list, str, int, float, bool, None).
pub fn py_repr(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) => number_str(n),
        Value::String(s) => repr_str(s),
        Value::Array(a) => format!("[{}]", a.iter().map(py_repr).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| format!("{}: {}", repr_str(k), py_repr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Python `str.isprintable()` (tek karakter): Cc, Cf, Cs, Co, Cn, Zl, Zp, Zs dışı + boşluk.
fn is_printable(c: char) -> bool {
    static NONPRINT: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"^[\p{Cc}\p{Cf}\p{Co}\p{Cn}\p{Zl}\p{Zp}\p{Zs}]$").expect("isprintable")
    });
    if c == ' ' {
        return true;
    }
    if c.is_ascii() {
        return !c.is_ascii_control();
    }
    let mut buf = [0u8; 4];
    !NONPRINT.is_match(c.encode_utf8(&mut buf))
}

/// `repr(str)`
pub fn repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if !is_printable(c) => {
                let cp = c as u32;
                if cp <= 0xff {
                    out.push_str(&format!("\\x{cp:02x}"));
                } else if cp <= 0xffff {
                    out.push_str(&format!("\\u{cp:04x}"));
                } else {
                    out.push_str(&format!("\\U{cp:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn python_donusumleri() {
        assert_eq!(py_int(&json!(" 12 ")), Ok(12));
        assert_eq!(py_int(&json!("1_000")), Ok(1000));
        assert_eq!(py_int(&json!("5.0")), Err(PyConvError::Invalid));
        assert_eq!(py_int(&json!(7.9)), Ok(7));
        assert_eq!(py_int(&json!(true)), Ok(1));
        assert_eq!(py_int(&json!(null)), Err(PyConvError::Invalid));
        assert_eq!(py_float(&json!(" 2.5 ")), Ok(2.5));
        assert_eq!(py_float(&json!("1e3")), Ok(1000.0));
        assert!(py_float(&json!("-inf")).unwrap().is_infinite());
        assert_eq!(py_float(&json!("abc")), Err(PyConvError::Invalid));
        assert_eq!(
            py_repr(&json!({"a": [1, 1.0, null, true, "x'y"]})),
            r#"{'a': [1, 1.0, None, True, "x'y"]}"#
        );
        assert_eq!(py_str(&json!(["\u{7}\u{200b}é"])), "['\\x07\\u200bé']");
        assert_eq!(trunc("şğü", 2), "şğ");
    }
}
