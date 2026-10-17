//! OSIRIS `content_hash` normalizasyonu, kanonik JSON ve denetim zinciri.
//!
//! Sözleşme: docs/SOZLESME.md §1–§2. Altın veriler: `fixtures/hash`, `fixtures/audit`
//! (`tests/fixtures.rs` hepsini koşar).

pub mod audit;
pub mod canonical;
pub mod html;
pub mod json;

use sha2::{Digest, Sha256};

pub use html::{py_is_space, py_strip};
pub use json::Json;

/// `ProcessingPipeline._MAX_CONTENT_CHARS` — kod noktası.
pub const MAX_CONTENT_CHARS: usize = 200_000;

/// İlk `n` Unicode kod noktası (Python `s[:n]`).
pub fn truncate_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Python `re.sub(r"\s+", " ", s)` (Unicode `\s`).
pub fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if py_is_space(c) {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

/// `ProcessingPipeline.clean`: kes → HTML soy (bs4 html.parser) → boşluk daralt → kırp → kes.
/// Ayrıştırıcı reddederse (Python'da istisna) HTML soyulmadan ham metin kullanılır.
pub fn clean(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let text = truncate_chars(text, MAX_CONTENT_CHARS);
    let plain = html::get_text(text).unwrap_or_else(|_| text.to_owned());
    let collapsed = collapse_ws(&plain);
    truncate_chars(py_strip(&collapsed), MAX_CONTENT_CHARS).to_owned()
}

/// Küçük harfli onaltılık SHA-256.
pub fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// `content_hash = hex(sha256(utf8(cleaned)))` — temizlenmiş metin üzerinden.
pub fn content_hash(cleaned: &str) -> String {
    sha256_hex(cleaned.as_bytes())
}
