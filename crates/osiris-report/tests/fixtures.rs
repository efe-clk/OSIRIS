//! `fixtures/report` (Markdown/HTML/CSV) ve `fixtures/stix` (bayt bayt) altın verileri.

use osiris_hash::Json;
use osiris_report::{ReportInput, html_at, markdown_at, stix};
use std::path::PathBuf;

fn load(rel: &str) -> Json {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(rel);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    Json::parse_py(&text).unwrap() // anahtar sırası korunur (CSV başlığı buna bağlı)
}

fn get<'a>(v: &'a Json, k: &str) -> Option<&'a Json> {
    match v {
        Json::Object(e) => e.iter().find(|(key, _)| key == k).map(|(_, v)| v),
        _ => None,
    }
}

fn text<'a>(v: &'a Json, k: &str) -> &'a str {
    match get(v, k) {
        Some(Json::Str(s)) => s,
        _ => "",
    }
}

fn list<'a>(v: &'a Json, k: &str) -> &'a [Json] {
    match get(v, k) {
        Some(Json::Array(a)) => a,
        _ => &[],
    }
}

fn cases(rel: &str) -> Vec<Json> {
    let data = load(rel);
    list(&data, "cases").to_vec()
}

#[test]
fn report_fixtures() {
    let cases = cases("report/cases.json");
    assert!(cases.len() >= 4);
    for case in &cases {
        let name = text(case, "name");
        let exp = get(case, "expected").unwrap();
        let input = ReportInput {
            title: text(case, "title"),
            scope: text(case, "scope"),
            summary: text(case, "summary"),
            findings: list(case, "findings"),
            sources: list(case, "sources"),
        };
        // generated_at: Python datetime.isoformat() — fixture zamanı birebir yazılır
        let now = text(case, "now");
        assert_eq!(markdown_at(&input, now), text(exp, "markdown"), "{name}: markdown");
        assert_eq!(html_at(&input, now), text(exp, "html"), "{name}: html");
        if get(case, "csv_rows").is_some() {
            let dir = tempfile::tempdir().unwrap();
            let g = osiris_report::ReportGenerator::new(dir.path()).unwrap();
            let path = g.generate_csv(list(case, "csv_rows"), "rapor").unwrap().unwrap();
            assert_eq!(path.file_name().unwrap(), "rapor.csv");
            assert_eq!(std::fs::read_to_string(path).unwrap(), text(exp, "csv"), "{name}: csv");
        }
    }
}

#[test]
fn stix_fixtures() {
    let cases = cases("stix/cases.json");
    assert!(!cases.is_empty());
    for case in &cases {
        let name = text(case, "name");
        let now: chrono::DateTime<chrono::Utc> = text(case, "now").parse().unwrap();
        let out = stix(
            text(case, "identity_name"),
            text(case, "title"),
            list(case, "entities"),
            now,
            Some(text(case, "bundle_id")),
        )
        .unwrap();
        assert_eq!(out, text(get(case, "expected").unwrap(), "bundle"), "{name}");
    }
    let now = chrono::Utc::now();
    assert!(stix("x", "t", &[], now, Some("bundle--zz")).is_err());
    assert!(stix("x", "t", &[], now, Some("00000000-0000-4000-8000-000000000001")).is_err());
    assert!(stix("x", "t", &[], now, None).unwrap().contains("\"id\": \"bundle--"));
}
