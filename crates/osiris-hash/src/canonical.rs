//! İki kanonik JSON biçimi (docs/SOZLESME.md §2):
//! - [`jcs`] — RFC 8785 (audit `chain_version = 2`), Python `rfc8785` 0.1.4 ile aynı.
//! - [`py_dumps`] — Python `json.dumps(sort_keys=True, separators=(",", ":"),
//!   ensure_ascii=False)` (audit `chain_version = 1`, yalnızca eski kayıtları doğrulamak için).

use crate::json::{Json, write_str};

/// Kanonikleştirme hatası (Python'da `ValueError` alt sınıfı).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonError(pub String);

impl std::fmt::Display for CanonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CanonError {}

/// RFC 8785 / I-JSON güvenli tamsayı aralığı: ±(2^53 − 1).
pub const MAX_SAFE_INT: i64 = (1 << 53) - 1;

// ------------------------------------------------------------------------------- JCS

/// RFC 8785 kanonik JSON. Tamsayılar ±(2^53−1) dışında hata verir; çağıran önce
/// [`jsonb_numbers`] uygulamalıdır (audit v2 böyle yapar).
pub fn jcs(value: &Json) -> Result<String, CanonError> {
    let mut out = String::new();
    write_jcs(&mut out, value)?;
    Ok(out)
}

fn write_jcs(out: &mut String, value: &Json) -> Result<(), CanonError> {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Int(i) => {
            if !(-MAX_SAFE_INT..=MAX_SAFE_INT).contains(i) {
                return Err(CanonError(format!("{i} exceeds safe integer domain for JSON floats")));
            }
            out.push_str(&i.to_string());
        }
        Json::BigInt(s) => {
            return Err(CanonError(format!("{s} exceeds safe integer domain for JSON floats")));
        }
        Json::Float(f) => {
            if !f.is_finite() {
                return Err(CanonError(format!(
                    "{} is not representable in JCS",
                    py_float_str(*f)
                )));
            }
            if *f == 0.0 {
                out.push('0'); // Python +0 / -0 ayrımı yapmaz
            } else {
                out.push_str(ryu_js::Buffer::new().format_finite(*f));
            }
        }
        Json::Str(s) => write_str(out, s),
        Json::Array(items) => {
            out.push('[');
            for (idx, item) in items.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                write_jcs(out, item)?;
            }
            out.push(']');
        }
        Json::Object(entries) => {
            // RFC 8785 §3.2.3: anahtarlar UTF-16 kod birimi sırasıyla
            let mut sorted: Vec<&(String, Json)> = entries.iter().collect();
            sorted.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
            out.push('{');
            for (idx, (k, v)) in sorted.into_iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                write_str(out, k);
                out.push(':');
                write_jcs(out, v)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

/// Audit v2 `_jsonb_numbers`: güvenli aralık dışındaki tamsayılar float olur (jsonb büyük
/// bir float'ı tamsayı olarak geri verebilir; float'a dönmek aynı JCS çıktısını üretir).
pub fn jsonb_numbers(value: &Json) -> Result<Json, CanonError> {
    Ok(match value {
        Json::Int(i) if !(-MAX_SAFE_INT..=MAX_SAFE_INT).contains(i) => Json::Float(*i as f64),
        Json::BigInt(s) => {
            let f: f64 = s.parse().map_err(|_| CanonError(format!("geçersiz tamsayı: {s}")))?;
            if !f.is_finite() {
                return Err(CanonError("int too large to convert to float".to_owned()));
            }
            Json::Float(f)
        }
        Json::Array(items) => {
            Json::Array(items.iter().map(jsonb_numbers).collect::<Result<_, _>>()?)
        }
        Json::Object(entries) => Json::Object(
            entries
                .iter()
                .map(|(k, v)| Ok((k.clone(), jsonb_numbers(v)?)))
                .collect::<Result<_, CanonError>>()?,
        ),
        other => other.clone(),
    })
}

/// Audit yazımı `_jcs_safe`: güvenli aralık dışındaki tamsayılar dizeye çevrilir.
pub fn jcs_safe(value: &Json) -> Json {
    match value {
        Json::Int(i) if !(-MAX_SAFE_INT..=MAX_SAFE_INT).contains(i) => Json::Str(i.to_string()),
        Json::BigInt(s) => Json::Str(s.clone()),
        Json::Array(items) => Json::Array(items.iter().map(jcs_safe).collect()),
        Json::Object(entries) => {
            Json::Object(entries.iter().map(|(k, v)| (k.clone(), jcs_safe(v))).collect())
        }
        other => other.clone(),
    }
}

// --------------------------------------------------------------------- kayıt JSON'u (R5)

/// Plugin `raw_content` kanonik JSON'u (rest-api, shodan — SOZLESME §4): RFC 8785 sıralama
/// ve biçimi; ancak tamsayılar her büyüklükte **tam ondalık** yazılır, sonlu olmayan float
/// `null` olur. Dize kayıt olduğu gibi döner (tırnaksız).
pub fn record_json(value: &Json) -> String {
    match value {
        Json::Str(s) => s.clone(),
        other => {
            let mut out = String::new();
            write_record(&mut out, other);
            out
        }
    }
}

fn write_record(out: &mut String, value: &Json) {
    match value {
        Json::Int(i) => out.push_str(&i.to_string()),
        Json::BigInt(s) => out.push_str(s),
        Json::Float(f) if !f.is_finite() => out.push_str("null"),
        Json::Array(items) => {
            out.push('[');
            for (idx, item) in items.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                write_record(out, item);
            }
            out.push(']');
        }
        Json::Object(entries) => {
            let mut sorted: Vec<&(String, Json)> = entries.iter().collect();
            sorted.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
            out.push('{');
            for (idx, (k, v)) in sorted.into_iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                write_str(out, k);
                out.push(':');
                write_record(out, v);
            }
            out.push('}');
        }
        // null, bool, sonlu float, dize: JCS ile aynı (hata veremez)
        other => {
            let _ = write_jcs(out, other);
        }
    }
}

// --------------------------------------------------------------------------- Python v1

/// Python `json.dumps(v, ensure_ascii=False, sort_keys=True, separators=(",", ":"))`.
pub fn py_dumps(value: &Json) -> String {
    let mut out = String::new();
    write_py(&mut out, value);
    out
}

fn write_py(out: &mut String, value: &Json) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Int(i) => out.push_str(&i.to_string()),
        Json::BigInt(s) => out.push_str(s),
        Json::Float(f) => {
            // json.dumps: allow_nan=True → NaN / Infinity / -Infinity
            if f.is_nan() {
                out.push_str("NaN");
            } else if f.is_infinite() {
                out.push_str(if *f > 0.0 { "Infinity" } else { "-Infinity" });
            } else {
                out.push_str(&py_float_repr(*f));
            }
        }
        Json::Str(s) => write_str(out, s),
        Json::Array(items) => {
            out.push('[');
            for (idx, item) in items.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                write_py(out, item);
            }
            out.push(']');
        }
        Json::Object(entries) => {
            // Python str karşılaştırması = kod noktası sırası = UTF-8 bayt sırası
            let mut sorted: Vec<&(String, Json)> = entries.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            out.push('{');
            for (idx, (k, v)) in sorted.into_iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                write_str(out, k);
                out.push(':');
                write_py(out, v);
            }
            out.push('}');
        }
    }
}

/// Python `str(float)` (hata iletileri için; sonlu olmayanlar dahil).
fn py_float_str(f: f64) -> String {
    if f.is_nan() {
        "nan".to_owned()
    } else if f.is_infinite() {
        if f > 0.0 { "inf".to_owned() } else { "-inf".to_owned() }
    } else {
        py_float_repr(f)
    }
}

/// Python `float.__repr__` (CPython `format_float_short`, kip 'r'): en kısa gidiş-dönüş
/// basamakları; `decpt <= -4 || decpt > 16` ise üslü (`1e+16`, `1.5e-05`), değilse sabit
/// ve tam sayıya `.0` eklenir.
pub fn py_float_repr(f: f64) -> String {
    debug_assert!(f.is_finite());
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0".to_owned() } else { "0.0".to_owned() };
    }
    let (neg, digits, decpt) = shortest_digits(f);
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    let nd = digits.len() as i32;
    if decpt <= -4 || decpt > 16 {
        out.push_str(&digits[..1]);
        if nd > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let exp = decpt - 1;
        out.push('e');
        out.push(if exp < 0 { '-' } else { '+' });
        out.push_str(&format!("{:02}", exp.abs()));
    } else if decpt <= 0 {
        out.push_str("0.");
        out.push_str(&"0".repeat((-decpt) as usize));
        out.push_str(&digits);
    } else if decpt < nd {
        out.push_str(&digits[..decpt as usize]);
        out.push('.');
        out.push_str(&digits[decpt as usize..]);
    } else {
        out.push_str(&digits);
        out.push_str(&"0".repeat((decpt - nd) as usize));
        out.push_str(".0");
    }
    out
}

/// (negatif mi, baştaki/sondaki sıfırsız basamaklar, ondalık nokta konumu) —
/// değer = 0.d₁d₂… × 10^decpt. Basamaklar ryu'nun en kısa gösteriminden gelir.
fn shortest_digits(f: f64) -> (bool, String, i32) {
    let mut buf = ryu::Buffer::new();
    let text = buf.format_finite(f);
    let (neg, text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (mant, exp) = match text.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.parse::<i32>().unwrap_or(0)),
        None => (text, 0),
    };
    let (int_part, frac_part) = mant.split_once('.').unwrap_or((mant, ""));
    let mut digits: String = format!("{int_part}{frac_part}");
    let mut decpt = int_part.len() as i32 + exp;
    let lead = digits.len() - digits.trim_start_matches('0').len();
    digits.drain(..lead);
    decpt -= lead as i32;
    let trimmed = digits.trim_end_matches('0').len();
    digits.truncate(trimmed);
    (neg, digits, decpt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_float_repr() {
        let cases = [
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (0.1, "0.1"),
            (1.0, "1.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (123.456, "123.456"),
            (-2.5, "-2.5"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (5e-324, "5e-324"),
            (12345678901234567.0, "1.2345678901234568e+16"),
        ];
        for (f, want) in cases {
            assert_eq!(py_float_repr(f), want, "{f:?}");
        }
    }

    #[test]
    fn kayit_json_buyuk_tamsayi_ve_dize() {
        let v = Json::Object(vec![
            ("z".into(), Json::BigInt("123456789012345678901234567890".into())),
            (
                "a".into(),
                Json::Array(vec![Json::Float(f64::NAN), Json::Float(1.0), Json::Bool(true)]),
            ),
        ]);
        assert_eq!(record_json(&v), "{\"a\":[null,1,true],\"z\":123456789012345678901234567890}");
        assert_eq!(record_json(&Json::Str("düz \"metin\"".into())), "düz \"metin\"");
    }

    #[test]
    fn jcs_sayi_ve_anahtar_sirasi() {
        let v = Json::Object(vec![
            ("b".into(), Json::Float(1e16)),
            ("a".into(), Json::Float(1.0)),
            ("\u{1f600}".into(), Json::Int(1)),
            ("\u{ff21}".into(), Json::Float(1e-7)),
        ]);
        // UTF-16 sırası: U+1F600 (D83D…) < U+FF21
        assert_eq!(
            jcs(&v).unwrap(),
            "{\"a\":1,\"b\":10000000000000000,\"\u{1f600}\":1,\"\u{ff21}\":1e-7}"
        );
    }
}
