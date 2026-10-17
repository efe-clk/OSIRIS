//! `fixtures/extract/cases.json` altın verileri.

use osiris_extract::extract_entities;
use serde_json::Value;
use std::path::PathBuf;

#[test]
fn extract_fixtures() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/extract/cases.json");
    let data: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let cases = data["cases"].as_array().unwrap();
    assert!(!cases.is_empty());
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let got: Vec<(String, String)> = extract_entities(case["input"].as_str().unwrap())
            .into_iter()
            .map(|e| (e.etype.to_owned(), e.value))
            .collect();
        let want: Vec<(String, String)> = case["expected"]["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (e["type"].as_str().unwrap().to_owned(), e["value"].as_str().unwrap().to_owned())
            })
            .collect();
        assert_eq!(got, want, "{name}");
    }
}
