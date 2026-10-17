//! Dil tespiti (kullanıcı kararı, R5): lingua, sınırlı dil kümesi, deterministik.
//! Python langdetect ile zaman zaman farklı sonuç verir; `language` hash'e girmez.

use lingua::{Language, LanguageDetector, LanguageDetectorBuilder};
use std::sync::LazyLock;

static DETECTOR: LazyLock<LanguageDetector> = LazyLock::new(|| {
    use Language::*;
    LanguageDetectorBuilder::from_languages(&[
        Arabic, Chinese, Dutch, English, French, German, Italian, Japanese, Korean, Persian,
        Polish, Portuguese, Russian, Spanish, Turkish, Ukrainian,
    ])
    .build()
});

/// langdetect biçiminde ISO 639-1 kodu (Çince `zh-cn`); boş ya da belirsiz → `unknown`.
pub fn detect(text: &str) -> String {
    if text.is_empty() {
        return "unknown".into();
    }
    let sample: String = text.chars().take(500).collect();
    match DETECTOR.detect_language_of(sample) {
        Some(Language::Chinese) => "zh-cn".into(),
        Some(l) => l.iso_code_639_1().to_string(),
        None => "unknown".into(),
    }
}
