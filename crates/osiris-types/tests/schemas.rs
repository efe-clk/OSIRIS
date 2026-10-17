//! Rust modelleri ↔ `schemas/*.schema.json` (plan R5: schemars ile üretip karşılaştır) ve
//! gömülecek tüm `plugins/*/manifest.json` dosyalarının gidiş-dönüşü.

use jsonschema::{Registry, Resource};
use osiris_types::{CollectedItem, CollectionResult, Manifest, QueueMessage};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn schema(name: &str) -> Value {
    let p = root().join("schemas").join(format!("{name}.schema.json"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn validator(name: &str) -> jsonschema::Validator {
    let mut builder = Registry::new();
    for n in ["collected_item", "collection_result", "manifest", "queue_message"] {
        let s = schema(n);
        let id = s["$id"].as_str().unwrap().to_owned();
        builder = builder.add(&id, Resource::from_contents(s)).unwrap();
    }
    let registry = builder.prepare().unwrap();
    jsonschema::options().with_registry(&registry).build(&schema(name)).unwrap()
}

fn assert_valid(name: &str, value: &Value) {
    let v = validator(name);
    let errors: Vec<String> = v.iter_errors(value).map(|e| e.to_string()).collect();
    assert!(errors.is_empty(), "{name}: {errors:?}\n{value}");
}

fn keys(v: &Value, field: &str) -> BTreeSet<String> {
    match &v[field] {
        Value::Object(o) => o.keys().cloned().collect(),
        Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect(),
        _ => BTreeSet::new(),
    }
}

/// schemars'ın ürettiği şema, elle yazılmış şemayla aynı özellik ve zorunlu alan kümesini taşır.
fn same_shape(generated: &Value, name: &str) {
    let hand = schema(name);
    assert_eq!(keys(generated, "properties"), keys(&hand, "properties"), "{name}: properties");
    assert_eq!(keys(generated, "required"), keys(&hand, "required"), "{name}: required");
    assert_eq!(
        generated.get("additionalProperties"),
        Some(&json!(false)),
        "{name}: additionalProperties"
    );
}

#[test]
fn schemars_semalari_elle_yazilanlarla_ayni_bicimde() {
    same_shape(schemars::schema_for!(CollectedItem).as_value(), "collected_item");
    same_shape(schemars::schema_for!(CollectionResult).as_value(), "collection_result");
    same_shape(schemars::schema_for!(QueueMessage).as_value(), "queue_message");
    let m = schemars::schema_for!(Manifest);
    let hand = schema("manifest");
    assert_eq!(keys(m.as_value(), "properties"), keys(&hand, "properties"));
    assert_eq!(keys(m.as_value(), "required"), keys(&hand, "required"));
}

#[test]
fn ornek_degerler_semalara_uyar() {
    let mut item = CollectedItem::new("ham");
    item.url = Some("https://example.com".into());
    item.metadata.insert("k".into(), json!(1));
    assert_valid("collected_item", &serde_json::to_value(&item).unwrap());
    let ok = CollectionResult::ok(vec![item.clone()], Default::default());
    assert_valid("collection_result", &serde_json::to_value(&ok).unwrap());
    assert_valid("collection_result", &serde_json::to_value(CollectionResult::fail("x")).unwrap());
    let msg = QueueMessage {
        plugin_id: "rss".into(),
        source_id: Some("0d9c2a4e-5f6b-4c1d-8e7f-0123456789ab".into()),
        item,
        collected_at: 1_760_000_000.5,
    };
    assert_valid("queue_message", &serde_json::to_value(&msg).unwrap());
}

#[test]
fn tum_plugin_manifestleri_gidis_donus() {
    let mut n = 0;
    for entry in std::fs::read_dir(root().join("plugins")).unwrap() {
        let path = entry.unwrap().path().join("manifest.json");
        if !path.is_file() {
            continue;
        }
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_valid("manifest", &raw);
        let m: Manifest =
            serde_json::from_value(raw.clone()).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        assert_eq!(serde_json::to_value(&m).unwrap(), raw, "{path:?}");
        n += 1;
    }
    assert_eq!(n, 11);
}
