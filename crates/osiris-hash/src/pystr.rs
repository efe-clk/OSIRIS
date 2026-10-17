//! Python `str()` / `repr()` / `float()` karşılıkları (JSON'dan gelen değerler için):
//! rapor hücreleri, uyarı alanları ve STIX değerleri Python'daki gibi dizeye çevrilir.

use crate::Json;
use crate::canonical::py_float_repr;
use std::fmt::Write as _;

/// Python `str.isprintable` yaklaşığı: Cc, Zl, Zp, boşluk dışı Zs ve yaygın Cf yazdırılamaz.
fn py_printable(c: char) -> bool {
    !(c.is_control()
        || matches!(c as u32,
            0x00A0 | 0x00AD | 0x1680 | 0x2000..=0x200F | 0x2028..=0x202F | 0x205F..=0x2064
            | 0x3000 | 0xFEFF))
}

/// Python `repr(str)`
pub fn py_str_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if !py_printable(c) => {
                let n = c as u32;
                let _ = if n < 0x100 {
                    write!(out, "\\x{n:02x}")
                } else if n < 0x10000 {
                    write!(out, "\\u{n:04x}")
                } else {
                    write!(out, "\\U{n:08x}")
                };
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python `repr()` (JSON'dan gelen değerler için)
pub fn py_repr(v: &Json) -> String {
    match v {
        Json::Str(s) => py_str_repr(s),
        Json::Array(items) => {
            let parts: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        Json::Object(entries) => {
            let parts: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("{}: {}", py_str_repr(k), py_repr(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        other => py_str(other),
    }
}

/// Python `str()`
pub fn py_str(v: &Json) -> String {
    match v {
        Json::Null => "None".into(),
        Json::Bool(true) => "True".into(),
        Json::Bool(false) => "False".into(),
        Json::Int(i) => i.to_string(),
        Json::BigInt(s) => s.clone(),
        Json::Float(f) => py_float_repr(*f),
        Json::Str(s) => s.clone(),
        other => py_repr(other),
    }
}

/// Python `float(x)` (sayı, bool ya da sayısal dize; aksi halde None).
pub fn py_float(v: &Json) -> Option<f64> {
    match v {
        Json::Int(i) => Some(*i as f64),
        Json::BigInt(s) => s.parse().ok(),
        Json::Float(f) => Some(*f),
        Json::Bool(b) => Some(f64::from(u8::from(*b))),
        Json::Str(s) => crate::py_strip(s).parse().ok(),
        _ => None,
    }
}

/// Python `int(x)` (bool, tamsayı, kesirli sayı → sıfıra doğru, tamsayı dizesi; aksi None).
pub fn py_int(v: &Json) -> Option<i64> {
    match v {
        Json::Int(i) => Some(*i),
        Json::Bool(b) => Some(i64::from(*b)),
        Json::Float(f) if f.is_finite() => Some(f.trunc() as i64),
        Json::Str(s) => crate::py_strip(s).parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repr_ve_float() {
        assert_eq!(py_str_repr("v'q"), "\"v'q\"");
        assert_eq!(py_str_repr("a'b\"c\n"), "'a\\'b\"c\\n'");
        assert_eq!(py_str_repr("\u{7}é\u{2028}"), "'\\x07é\\u2028'");
        assert_eq!(py_float(&Json::Str(" 2.5 ".into())), Some(2.5));
        assert_eq!(py_float(&Json::Str("abc".into())), None);
        assert_eq!(py_int(&Json::Float(-2.9)), Some(-2));
        assert_eq!(py_int(&Json::Str("x".into())), None);
    }
}
