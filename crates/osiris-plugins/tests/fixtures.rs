//! `fixtures/plugins/*.json` — Python ile aynı sonuç ve aynı ağ etkileşimi.
//! Hata metni sözleşme dışıdır: yalnızca varlığı karşılaştırılır.

use osiris_plugin::replay::Replay;
use serde_json::Value;
use std::path::PathBuf;

#[tokio::test]
async fn plugin_fixtures() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/plugins");
    let mut paths: Vec<PathBuf> =
        std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    paths.sort();
    let mut seen = std::collections::BTreeSet::new();
    for path in &paths {
        let case: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let plugin = osiris_plugins::by_id(case["plugin"].as_str().unwrap()).expect("plugin");
        seen.insert(plugin.id().to_owned());
        let replay = Replay::from_case(&case);
        let cfg = case["config"].as_object().cloned().unwrap_or_default();
        let res = plugin
            .collect(&replay.host(), &cfg)
            .await
            .unwrap_or_else(|e| panic!("{name}: çökme {e}"));
        let mut got = serde_json::to_value(&res).unwrap();
        let mut want = case["expected"].clone();
        for v in [&mut got, &mut want] {
            let has = !v["error"].is_null();
            v["error"] = Value::Bool(has);
        }
        assert_eq!(got, want, "{name}: sonuç");
        assert_eq!(replay.unexpected(), Vec::<Value>::new(), "{name}: beklenmeyen istek");
        assert_eq!(replay.report(), case["expected_network"], "{name}: ağ");
    }
    assert_eq!(seen.len(), 11, "her plugin en az bir fixture'la sınanmalı");
}
