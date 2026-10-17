//! `fixtures/hash` ve `fixtures/audit` altın verileri (Python ile aynı çıktı).

use osiris_hash::audit::{canonical_payload, chain_hash};
use osiris_hash::{Json, clean, content_hash};
use serde_json::Value;
use std::path::PathBuf;

fn load(rel: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(rel);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    serde_json::from_str(&text).unwrap()
}

fn opt_str(v: &Value) -> Option<String> {
    v.as_str().map(str::to_owned)
}

#[test]
fn hash_fixtures() {
    let data = load("hash/cases.json");
    let cases = data["cases"].as_array().unwrap();
    assert!(cases.len() >= 23);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let input = match case.get("input") {
            Some(v) => v.as_str().unwrap().to_owned(),
            None => {
                let rep = &case["input_repeat"];
                rep["text"].as_str().unwrap().repeat(rep["times"].as_u64().unwrap() as usize)
            }
        };
        let cleaned = clean(&input);
        assert_eq!(cleaned, case["expected"]["cleaned"].as_str().unwrap(), "{name}: cleaned");
        assert_eq!(
            content_hash(&cleaned),
            case["expected"]["content_hash"].as_str().unwrap(),
            "{name}: content_hash"
        );
    }
}

/// Python `_normalize`: user_id/resource None → "", ilk 200/500 kod noktası; detail JSON
/// gidiş-dönüşü + `_jcs_safe`.
fn normalize(rec: &Value) -> (String, String, Json) {
    let cut =
        |v: &Value, n: usize| -> String { v.as_str().unwrap_or("").chars().take(n).collect() };
    let detail = match rec.get("detail") {
        Some(Value::Null) | None => Json::Object(vec![]),
        Some(v) => Json::from(v.clone()),
    };
    (
        cut(&rec["user_id"], 200),
        cut(&rec["resource"], 500),
        osiris_hash::canonical::jcs_safe(&detail),
    )
}

#[test]
fn audit_fixtures() {
    let data = load("audit/cases.json");
    let cases = data["cases"].as_array().unwrap();
    assert!(!cases.is_empty());
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let mut prev: Option<String> = None;
        let records = case["records"].as_array().unwrap();
        let expected = case["expected"]["chain"].as_array().unwrap();
        assert_eq!(records.len(), expected.len(), "{name}");
        for (rec, exp) in records.iter().zip(expected) {
            let (user_id, resource, detail) = normalize(rec);
            let payload = canonical_payload(
                Some(&user_id),
                rec["action"].as_str().unwrap(),
                Some(&resource),
                Some(&detail),
                rec["chain_version"].as_i64().unwrap(),
            )
            .unwrap();
            assert_eq!(payload, exp["payload"].as_str().unwrap(), "{name}: payload");
            assert_eq!(prev, opt_str(&exp["prev_hash"]), "{name}: prev_hash");
            let digest = chain_hash(prev.as_deref(), &payload);
            assert_eq!(digest, exp["hash"].as_str().unwrap(), "{name}: hash");
            prev = Some(digest);
        }
    }
}
