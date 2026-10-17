//! Regex varlık çıkarma — `ProcessingPipeline.extract_entities` (NER hariç) ile birebir.
//!
//! Sözleşme: docs/SOZLESME.md §8, altın veri `fixtures/extract/cases.json`.
//!
//! Python `re` (Unicode kip) ile Rust `regex` arasındaki farklar burada açıkça kapatılır:
//! - `\b`: Python'da kelime karakteri `str.isalnum() or "_"` = `[\p{L}\p{N}_]`; Rust'ın
//!   `\w`'si işaretleri (M), Pc ve ZWJ/ZWNJ'yi de içerir. Desenlerin uçları hep kelime
//!   karakteri olduğundan `\b` = `(?<!W)` / `(?!W)` (fancy-regex ile).
//! - `\s`: Python kümesi (U+001C–U+001F dahil) elle yazılır.
//! - `re.IGNORECASE` ile `[a-z]` Python'da `İ ı ſ K` (U+0130, U+0131, U+017F, U+212A)
//!   karakterlerini de eşler; sınıf elle genişletilir.
//! - `\d` = `\p{Nd}` (ikisi de Unicode ondalık rakam).
//!
//! Konumlar bayt ofsetidir; çakışma karşılaştırmaları sıra koruduğu için Python'un kod
//! noktası ofsetleriyle aynı sonucu verir.

use std::collections::HashSet;
use std::sync::LazyLock;

/// Python `re` `\w` (Unicode, str deseni).
const W: &str = r"[\p{L}\p{N}_]";
/// `[a-z]` + `re.IGNORECASE` (Python 3.11 / Unicode 14 ile hesaplandı).
const AZ_I: &str = r"a-zA-Z\x{130}\x{131}\x{17F}\x{212A}";
/// Python `re` `\s` (Unicode).
const PY_S: &str = r"\t\n\x0B\x0C\r\x1C-\x20\x85\xA0\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}";

static EMAIL: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").expect("email deseni")
});
static IP: LazyLock<fancy_regex::Regex> = LazyLock::new(|| {
    fancy_regex::Regex::new(&format!(r"(?<!{W})(?:\p{{Nd}}{{1,3}}\.){{3}}\p{{Nd}}{{1,3}}(?!{W})"))
        .expect("ip deseni")
});
static DOMAIN: LazyLock<fancy_regex::Regex> = LazyLock::new(|| {
    let a1 = format!("{AZ_I}0-9");
    fancy_regex::Regex::new(&format!(
        r"(?<!{W})(?:[{a1}](?:[{a1}-]{{0,61}}[{a1}])?\.)+[{AZ_I}]{{2,}}(?!{W})"
    ))
    .expect("domain deseni")
});
static PHONE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(&format!(r"\+?\p{{Nd}}[\p{{Nd}}{PY_S}\-().]{{7,}}\p{{Nd}}"))
        .expect("telefon deseni")
});
static CRYPTO: LazyLock<fancy_regex::Regex> = LazyLock::new(|| {
    fancy_regex::Regex::new(&format!(
        r"(?<!{W})(?:bc1|[13])[a-zA-HJ-NP-Z0-9]{{25,39}}(?!{W})|(?<!{W})0x[a-fA-F0-9]{{40}}(?!{W})"
    ))
    .expect("kripto deseni")
});
static CVE: LazyLock<fancy_regex::Regex> = LazyLock::new(|| {
    fancy_regex::Regex::new(&format!(r"(?<!{W})[Cc][Vv][Ee]-\p{{Nd}}{{4}}-\p{{Nd}}{{4,7}}(?!{W})"))
        .expect("cve deseni")
});

/// Değer üst sınırı (kod noktası).
pub const MAX_VALUE_CHARS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    pub etype: &'static str,
    pub value: String,
}

/// Python `str.isspace()`.
fn py_is_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}'
            | '\u{1c}'..='\u{20}'
            | '\u{85}'
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

/// Python `ipaddress.ip_address` yalnızca `a.b.c.d` biçimi için: ASCII rakam, 1–3 hane,
/// baştaki sıfır yasak ("0" hariç), ≤ 255.
fn py_ipv4_valid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 3
                && p.bytes().all(|b| b.is_ascii_digit())
                && (*p == "0" || !p.starts_with('0'))
                && p.parse::<u16>().is_ok_and(|v| v <= 255)
        })
}

type Span = (usize, usize);

fn overlaps(span: Span, spans: &[Span]) -> bool {
    spans.iter().any(|&(s, e)| (s <= span.0 && span.0 < e) || (s < span.1 && span.1 <= e))
}

fn fancy_spans(re: &fancy_regex::Regex, text: &str) -> Vec<Span> {
    // Desenler sabit ve basit; geri izleme sınırı aşılırsa (Python'da olmayan bir hata)
    // o eşleşme atlanır — sonuç Python'dan eksik olabilir ama asla fazla değildir.
    re.find_iter(text).filter_map(Result::ok).map(|m| (m.start(), m.end())).collect()
}

struct Collector {
    entities: Vec<Entity>,
    seen: HashSet<(&'static str, String)>,
}

impl Collector {
    fn add(&mut self, etype: &'static str, value: &str) {
        let v: String = value.trim_matches(py_is_space).chars().take(MAX_VALUE_CHARS).collect();
        if v.is_empty() {
            return;
        }
        if self.seen.insert((etype, v.to_lowercase())) {
            self.entities.push(Entity { etype, value: v });
        }
    }
}

/// Regex varlıkları: tip sırası email, ip, domain, phone, crypto_address, cve; tip içinde
/// metin sırası. Tekilleştirme `(tip, değer.lower())`, ilk görülen biçim kalır.
pub fn extract_entities(text: &str) -> Vec<Entity> {
    let mut out = Collector { entities: Vec::new(), seen: HashSet::new() };

    let email_spans: Vec<Span> = EMAIL.find_iter(text).map(|m| (m.start(), m.end())).collect();
    for &(s, e) in &email_spans {
        out.add("email", &text[s..e]);
    }
    let ip_spans = fancy_spans(&IP, text);
    let cve_spans = fancy_spans(&CVE, text);
    let crypto_spans = fancy_spans(&CRYPTO, text);

    for &(s, e) in &ip_spans {
        if py_ipv4_valid(&text[s..e]) {
            out.add("ip", &text[s..e]);
        }
    }
    for (s, e) in fancy_spans(&DOMAIN, text) {
        if email_spans.iter().any(|&(es, ee)| es <= s && s < ee) {
            continue;
        }
        out.add("domain", &text[s..e].to_lowercase());
    }
    for m in PHONE.find_iter(text) {
        let span = (m.start(), m.end());
        if overlaps(span, &ip_spans) || overlaps(span, &cve_spans) || overlaps(span, &crypto_spans)
        {
            continue;
        }
        let digits = m.as_str().chars().filter(|c| is_decimal(*c)).count();
        if (7..=15).contains(&digits) {
            out.add("phone", m.as_str());
        }
    }
    for &(s, e) in &crypto_spans {
        out.add("crypto_address", &text[s..e]);
    }
    for &(s, e) in &cve_spans {
        out.add("cve", &text[s..e].to_uppercase());
    }
    out.entities
}

/// `\d` / `\p{Nd}` tek karakter için.
fn is_decimal(c: char) -> bool {
    static ND: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^\p{Nd}$").expect("Nd"));
    if c.is_ascii() {
        return c.is_ascii_digit();
    }
    let mut buf = [0u8; 4];
    ND.is_match(c.encode_utf8(&mut buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(text: &str) -> Vec<(&'static str, String)> {
        extract_entities(text).into_iter().map(|e| (e.etype, e.value)).collect()
    }

    #[test]
    fn turkce_buyuk_i_alan_adi_python_ignorecase_gibi() {
        // Python: [a-z] + IGNORECASE 'İ'yi eşler; değer lower() ile "i̇" olur
        assert_eq!(pairs("BİLGİ.COM"), vec![("domain", "bi\u{307}lgi\u{307}.com".to_owned())]);
    }

    #[test]
    fn zwj_kelime_siniri_degildir() {
        // Python'da U+200D kelime karakteri değil → \b var → IP eşleşir
        assert_eq!(pairs("8.8.8.8\u{200d}"), vec![("ip", "8.8.8.8".to_owned())]);
    }

    #[test]
    fn bastaki_sifirli_ip_gecersiz() {
        assert!(pairs("01.2.3.4").iter().all(|(t, _)| *t != "ip"));
    }
}
