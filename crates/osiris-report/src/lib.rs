//! Rapor üretici (R6) — Python `osiris_report.ReportGenerator` ile aynı çıktı.
//!
//! - Markdown/HTML: Jinja2 şablonları (`autoescape=True`, `trim_blocks` kapalı, sondaki satır
//!   sonu atılır) elle aynı metne açıldı; kaçış markupsafe ile aynı (`& < > " '`).
//! - JSON/STIX: `json.dumps(…, ensure_ascii=False, indent=2)` biçimi, anahtar sırası korunur.
//!   STIX bayt bayt sözleşmedir (docs/SOZLESME.md §3, `fixtures/stix`).
//! - CSV: Python `csv` (excel lehçesi, `\r\n`, QUOTE_MINIMAL); `= + - @ \t \r` ile başlayan
//!   dize hücreleri `'` ile etkisizleşir (formül enjeksiyonu). Dosyalar yalnızca `output_dir`
//!   altına yazılır (yol bileşenleri atılır, dışarı çıkan sembolik bağ reddedilir).
//!
//! Fixture: `fixtures/report/cases.json`, `fixtures/stix/cases.json`.

use chrono::{DateTime, Timelike, Utc};
use osiris_hash::Json;
use osiris_hash::canonical::py_float_repr;
use osiris_hash::json::write_str;
use osiris_hash::pystr::py_str;
use osiris_hash::truncate_chars;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    /// Python `ValueError`
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Python `datetime.now(UTC).isoformat()` (mikrosaniye; sıfırsa kesir yazılmaz).
pub fn py_isoformat(t: DateTime<Utc>) -> String {
    let micros = t.nanosecond() / 1000 % 1_000_000;
    if micros == 0 {
        t.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
    } else {
        format!("{}.{micros:06}+00:00", t.format("%Y-%m-%dT%H:%M:%S"))
    }
}

/// markupsafe `escape`
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&#34;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn get<'a>(obj: &'a Json, key: &str) -> Option<&'a Json> {
    match obj {
        Json::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// `json.dumps(v, ensure_ascii=False, indent=2)` (anahtar sırası korunur).
pub fn py_dumps_indent(v: &Json) -> String {
    let mut out = String::new();
    write_indent(&mut out, v, 0);
    out
}

fn write_indent(out: &mut String, v: &Json, level: usize) {
    let pad = |out: &mut String, n: usize| out.extend(std::iter::repeat_n(' ', n * 2));
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Int(i) => out.push_str(&i.to_string()),
        Json::BigInt(s) => out.push_str(s),
        Json::Float(f) if f.is_nan() => out.push_str("NaN"),
        Json::Float(f) if f.is_infinite() => {
            out.push_str(if *f > 0.0 { "Infinity" } else { "-Infinity" })
        }
        Json::Float(f) => out.push_str(&py_float_repr(*f)),
        Json::Str(s) => write_str(out, s),
        Json::Array(items) if items.is_empty() => out.push_str("[]"),
        Json::Object(entries) if entries.is_empty() => out.push_str("{}"),
        Json::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                pad(out, level + 1);
                write_indent(out, item, level + 1);
            }
            out.push('\n');
            pad(out, level);
            out.push(']');
        }
        Json::Object(entries) => {
            out.push('{');
            for (i, (k, item)) in entries.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                pad(out, level + 1);
                write_str(out, k);
                out.push_str(": ");
                write_indent(out, item, level + 1);
            }
            out.push('\n');
            pad(out, level);
            out.push('}');
        }
    }
}

// ----------------------------------------------------------------------------- rapor

/// `_safe_findings` sonrası bulgu.
struct Finding {
    title: String,
    description: String,
}

fn safe_findings(findings: &[Json]) -> Vec<Finding> {
    let field = |f: &Json, key: &str, n: usize| {
        let s = get(f, key).map(py_str).unwrap_or_default();
        truncate_chars(&s, n).to_owned()
    };
    findings
        .iter()
        .take(500)
        .filter(|f| matches!(f, Json::Object(_)))
        .map(|f| Finding {
            title: field(f, "title", 500),
            description: field(f, "description", 5000),
        })
        .collect()
}

fn safe_sources(sources: &[Json]) -> Vec<String> {
    sources.iter().take(500).map(|s| truncate_chars(&py_str(s), 1000).to_owned()).collect()
}

/// Rapor girdisi (`generate_markdown` / `generate_html` bağımsız değişkenleri).
pub struct ReportInput<'a> {
    pub title: &'a str,
    pub scope: &'a str,
    pub summary: &'a str,
    pub findings: &'a [Json],
    pub sources: &'a [Json],
}

/// `generate_markdown` — `generated_at` Python `datetime.now(UTC).isoformat()`.
pub fn markdown_at(r: &ReportInput<'_>, generated_at: &str) -> String {
    let e = |s: &str, n: usize| html_escape(truncate_chars(s, n));
    let mut out = format!(
        "# {}\n\n**Oluşturulma:** {}\n**Kapsam:** {}\n\n## Özet\n\n{}\n\n## Bulgular\n\n",
        e(r.title, 500),
        html_escape(generated_at),
        e(r.scope, 1000),
        e(r.summary, 20_000)
    );
    for f in safe_findings(r.findings) {
        let _ = write!(out, "\n### {}\n{}\n\n", html_escape(&f.title), html_escape(&f.description));
    }
    out.push_str("\n## Kaynaklar\n\n");
    for s in safe_sources(r.sources) {
        let _ = write!(out, "\n- {}\n", html_escape(&s));
    }
    out
}

pub fn markdown(r: &ReportInput<'_>) -> String {
    markdown_at(r, &py_isoformat(Utc::now()))
}

const HTML_HEAD: &str = "<!DOCTYPE html>\n<html lang=\"tr\"><head><meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n";
const HTML_STYLE: &str = "<style>body{font-family:system-ui,sans-serif;max-width:800px;margin:2rem auto;\n\
padding:0 1rem;color:#1a1a1a}h1{border-bottom:2px solid #333}h3{color:#444}\n\
.meta{color:#666;font-size:.9rem}li{margin:.4rem 0}</style>\n</head><body>\n";

/// `generate_html` (PDF'e giden ara adım; autoescape açık).
pub fn html_at(r: &ReportInput<'_>, generated_at: &str) -> String {
    let title = html_escape(truncate_chars(r.title, 500));
    let mut out = format!("{HTML_HEAD}<title>{title}</title>\n{HTML_STYLE}<h1>{title}</h1>\n");
    let _ = write!(
        out,
        "<p class=\"meta\">Oluşturulma: {} · Kapsam: {}</p>\n<h2>Özet</h2><p>{}</p>\n<h2>Bulgular</h2>\n<ul>",
        html_escape(generated_at),
        html_escape(truncate_chars(r.scope, 1000)),
        html_escape(truncate_chars(r.summary, 20_000))
    );
    for f in safe_findings(r.findings) {
        let _ = writeln!(
            out,
            "<li><strong>{}</strong><br>{}</li>",
            html_escape(&f.title),
            html_escape(&f.description)
        );
    }
    out.push_str("</ul>\n<h2>Kaynaklar</h2>\n<ul>");
    for s in safe_sources(r.sources) {
        let _ = write!(out, "<li>{}</li>", html_escape(&s));
    }
    out.push_str("</ul>\n</body></html>");
    out
}

pub fn html(r: &ReportInput<'_>) -> String {
    html_at(r, &py_isoformat(Utc::now()))
}

/// `generate_json`
pub fn json(data: &Json) -> String {
    py_dumps_indent(data)
}

// ------------------------------------------------------------------------------ STIX

/// STIX 2.1 varlık eşlemesi (doküman §2.6): tip → (SCO tipi, özellik)
const STIX_IOC: [(&str, &str, &str); 6] = [
    ("ip", "ipv4-addr", "value"),
    ("domain", "domain-name", "value"),
    ("email", "email-addr", "value"),
    ("crypto_address", "cryptocurrency-wallet", "value"),
    ("hash", "file", "hashes.MD5"),
    ("cve", "vulnerability", "name"),
];

fn s(v: &str) -> Json {
    Json::Str(v.to_owned())
}

fn obj(entries: Vec<(&str, Json)>) -> Json {
    Json::Object(entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// `generate_stix(identity_name, title, entities, now=…, bundle_id=…)` — bayt bayt sözleşme.
/// `bundle_id` yoksa uuid4 (sözleşme dışı).
pub fn stix(
    identity_name: &str,
    title: &str,
    entities: &[Json],
    now: DateTime<Utc>,
    bundle_id: Option<&str>,
) -> Result<String, ReportError> {
    let now = now.format("%Y-%m-%dT%H:%M:%S.000Z").to_string();
    let bundle_id = match bundle_id {
        None => format!("bundle--{}", uuid::Uuid::new_v4()),
        Some(b) => {
            let Some(rest) = b.strip_prefix("bundle--") else {
                return Err(ReportError::Invalid(
                    "bundle_id 'bundle--<uuid>' biçiminde olmalı".into(),
                ));
            };
            uuid::Uuid::parse_str(rest)
                .map_err(|_| ReportError::Invalid("badly formed hexadecimal UUID string".into()))?;
            b.to_owned()
        }
    };
    let v5 = |ns: &uuid::Uuid, name: &str| uuid::Uuid::new_v5(ns, name.as_bytes());
    let identity_id = format!("identity--{}", v5(&uuid::Uuid::NAMESPACE_DNS, "osiris.local"));
    let name = truncate_chars(identity_name, 200);
    let mut objects = vec![obj(vec![
        ("type", s("identity")),
        ("spec_version", s("2.1")),
        ("id", s(&identity_id)),
        ("created", s(&now)),
        ("modified", s(&now)),
        ("name", s(if name.is_empty() { "OSIRIS" } else { name })),
        ("identity_class", s("system")),
    ])];
    for ent in entities.iter().take(500) {
        if !matches!(ent, Json::Object(_)) {
            continue;
        }
        let etype = get(ent, "type").map_or_else(|| "custom".to_owned(), py_str);
        // STIX dize değişmezleri tek satır olmalı (satır sonu desen bozar)
        let raw = get(ent, "value").map(py_str).unwrap_or_default();
        let value = truncate_chars(&raw.replace(['\r', '\n'], " "), 500).to_owned();
        let Some(&(_, mut stix_type, prop)) = STIX_IOC.iter().find(|(t, _, _)| *t == etype) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        if etype == "ip" && value.contains(':') {
            stix_type = "ipv6-addr"; // v4 varsayımı IPv6'yı bozardı
        }
        let obj_id = format!(
            "{stix_type}--{}",
            v5(&uuid::Uuid::NAMESPACE_URL, &format!("{etype}:{}", value.to_lowercase()))
        );
        objects.push(match etype.as_str() {
            "cve" => obj(vec![
                ("type", s("vulnerability")),
                ("spec_version", s("2.1")),
                ("id", s(&obj_id)),
                ("created", s(&now)),
                ("modified", s(&now)),
                ("name", s(&value.to_uppercase())),
                ("created_by_ref", s(&identity_id)),
            ]),
            "hash" => obj(vec![
                ("type", s("file")),
                ("spec_version", s("2.1")),
                ("id", s(&obj_id)),
                ("hashes", obj(vec![("MD5", s(&value))])),
                ("created_by_ref", s(&identity_id)),
            ]),
            _ => {
                // STIX dize değişmezi: tersbölü ve tek tırnak kaçırılmalı
                let safe = value.replace('\\', "\\\\").replace('\'', "\\'");
                let pattern = format!("[{stix_type}:{prop} = '{safe}']");
                let description = format!("OSIRIS: {title}");
                obj(vec![
                    ("type", s("indicator")),
                    ("spec_version", s("2.1")),
                    ("id", s(&format!("indicator--{}", v5(&uuid::Uuid::NAMESPACE_URL, &pattern)))),
                    ("created", s(&now)),
                    ("modified", s(&now)),
                    ("pattern", s(&pattern)),
                    ("pattern_type", s("stix")),
                    ("valid_from", s(&now)),
                    ("labels", Json::Array(vec![s("malicious-activity")])),
                    ("created_by_ref", s(&identity_id)),
                    ("object_marking_refs", Json::Array(vec![])),
                    ("description", s(truncate_chars(&description, 500))),
                ])
            }
        });
    }
    let bundle = obj(vec![
        ("type", s("bundle")),
        ("id", s(&bundle_id)),
        ("spec_version", s("2.1")),
        ("objects", Json::Array(objects)),
    ]);
    Ok(py_dumps_indent(&bundle))
}

// ------------------------------------------------------------------------------- CSV

/// `_csv_safe`: formül enjeksiyonu koruması.
fn csv_safe(v: &Json) -> String {
    match v {
        Json::Null => String::new(),
        Json::Str(s) if s.starts_with(['=', '+', '-', '@', '\t', '\r']) => format!("'{s}"),
        other => py_str(other),
    }
}

/// Python `csv.writer` (excel): gerekiyorsa tırnakla, `"` → `""`.
fn csv_field(out: &mut String, f: &str, only_field: bool) {
    if f.contains([',', '"', '\r', '\n']) || (only_field && f.is_empty()) {
        out.push('"');
        out.push_str(&f.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(f);
    }
}

/// `generate_csv` gövdesi: başlık ilk satırın anahtarları (≤ 50), en fazla 10 000 satır;
/// fazla anahtarlar yok sayılır, eksikler boş.
pub fn csv_text(rows: &[Json]) -> String {
    let Some(Json::Object(first)) = rows.first() else { return String::new() };
    let fields: Vec<&str> = first.iter().take(50).map(|(k, _)| k.as_str()).collect();
    let mut out = String::new();
    let only = fields.len() == 1;
    let write_row = |out: &mut String, cells: Vec<String>| {
        for (i, c) in cells.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            csv_field(out, c, only);
        }
        out.push_str("\r\n");
    };
    write_row(&mut out, fields.iter().map(|f| (*f).to_owned()).collect());
    for row in rows.iter().take(10_000) {
        let cells = fields.iter().map(|f| get(row, f).map(csv_safe).unwrap_or_default()).collect();
        write_row(&mut out, cells);
    }
    out
}

/// Dosya yazan üretici (`output_dir` kilidi).
pub struct ReportGenerator {
    pub output_dir: PathBuf,
}

impl ReportGenerator {
    pub fn new(output_dir: impl AsRef<Path>) -> Result<Self, ReportError> {
        std::fs::create_dir_all(output_dir.as_ref())?;
        Ok(Self { output_dir: std::fs::canonicalize(output_dir)? })
    }

    /// `_resolve_output_path`: yalnızca dosya adı alınır; hedef `output_dir` dışına çıkamaz.
    pub fn resolve_output_path(&self, path: &str, suffix: &str) -> Result<PathBuf, ReportError> {
        let outside = || ReportError::Invalid("Çıktı dizini dışına yazma engellendi".into());
        let name = Path::new(path).file_name().ok_or_else(outside)?;
        let mut out = self.output_dir.join(name);
        let ext = out.extension().map(|e| format!(".{}", e.to_string_lossy()));
        if !suffix.is_empty() && ext.as_deref() != Some(suffix) {
            out.set_extension(suffix.trim_start_matches('.'));
        }
        if std::fs::symlink_metadata(&out).is_ok() {
            let real = std::fs::canonicalize(&out).map_err(|_| outside())?;
            if real.parent() != Some(self.output_dir.as_path()) {
                return Err(outside());
            }
        }
        Ok(out)
    }

    /// `generate_csv(rows, path)` — satır yoksa dosya yazılmaz.
    pub fn generate_csv(&self, rows: &[Json], path: &str) -> Result<Option<PathBuf>, ReportError> {
        if rows.is_empty() {
            return Ok(None);
        }
        let out = self.resolve_output_path(path, ".csv")?;
        std::fs::write(&out, csv_text(rows))?;
        Ok(Some(out))
    }

    /// `save(content, path)` — en fazla 5 000 000 kod noktası.
    pub fn save(&self, content: &str, path: &str) -> Result<PathBuf, ReportError> {
        if content.chars().count() > 5_000_000 {
            return Err(ReportError::Invalid("Rapor çok büyük".into()));
        }
        let out = self.resolve_output_path(path, "")?;
        std::fs::write(&out, content)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_bicimleri() {
        let t: DateTime<Utc> = "2026-01-02T03:04:05.000007Z".parse().unwrap();
        assert_eq!(py_isoformat(t), "2026-01-02T03:04:05.000007+00:00");
        let t: DateTime<Utc> = "2026-01-02T03:04:05Z".parse().unwrap();
        assert_eq!(py_isoformat(t), "2026-01-02T03:04:05+00:00");
        assert_eq!(
            py_dumps_indent(&Json::parse_py(r#"{"b":[1,2.5,{}],"a":[]}"#).unwrap()),
            "{\n  \"b\": [\n    1,\n    2.5,\n    {}\n  ],\n  \"a\": []\n}"
        );
    }

    #[test]
    fn dizin_kilidi() {
        let dir = tempfile::tempdir().unwrap();
        let g = ReportGenerator::new(dir.path().join("r")).unwrap();
        let p = g.save("x", "../../etc/passwd").unwrap();
        assert_eq!(p, g.output_dir.join("passwd"));
        assert!(g.save("x", "..").is_err());
        assert_eq!(g.resolve_output_path("a.txt", ".csv").unwrap(), g.output_dir.join("a.csv"));
        assert_eq!(g.resolve_output_path("rapor", ".csv").unwrap(), g.output_dir.join("rapor.csv"));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/hostname", g.output_dir.join("kacis")).unwrap();
            assert!(g.save("x", "kacis").is_err());
        }
        assert!(g.save(&"x".repeat(5_000_001), "buyuk").is_err());
        assert_eq!(g.generate_csv(&[], "bos").unwrap(), None);
    }
}
