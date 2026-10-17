//! `fixtures/ner` ve `fixtures/embedding` (model gerekir: `python models/download.py`).
//! Model yoksa atlanır; `OSIRIS_REQUIRE_MODELS=1` ise atlamak hatadır (CI entegrasyon işi).

use osiris_pipeline::{embed, model_dir, ner};
use serde_json::Value;
use std::path::PathBuf;

fn fixture(name: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
        .join("cases.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn root() -> Option<PathBuf> {
    // cargo test crate dizininde çalışır: göreli OSIRIS_MODEL_DIR repo köküne göre çözülür
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let r = std::env::var("OSIRIS_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("models"));
    let r = if r.is_absolute() { r } else { repo.join(r) };
    let _ = model_dir();
    if ner::OnnxNer::available(&r)
        && r.join(embed::LOCAL_DIR).join("model_optimized.onnx").is_file()
    {
        return Some(r);
    }
    assert!(
        std::env::var("OSIRIS_REQUIRE_MODELS").as_deref() != Ok("1"),
        "modeller yok ama OSIRIS_REQUIRE_MODELS=1"
    );
    eprintln!("modeller yok — atlandı");
    None
}

#[test]
fn ner_fixtures() {
    let Some(r) = root() else { return };
    let n = ner::OnnxNer::load(&r.join(ner::MODEL_SUBDIR)).unwrap();
    for case in fixture("ner")["cases"].as_array().unwrap() {
        let got: Vec<(String, String)> = n.extract(case["input"].as_str().unwrap()).unwrap();
        let want: Vec<(String, String)> = case["expected"]["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (e["type"].as_str().unwrap().to_owned(), e["value"].as_str().unwrap().to_owned())
            })
            .collect();
        assert_eq!(got, want, "{}", case["name"]);
    }
}

fn cos(a: &[f32], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(x, y)| f64::from(*x) * y).sum();
    let na: f64 = a.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    let nb: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    dot / (na * nb)
}

#[test]
fn embedding_fixtures() {
    let Some(r) = root() else { return };
    let e = embed::Embedder::load(&r, embed::DEFAULT_MODEL).unwrap();
    let fx = fixture("embedding");
    let min = fx["tolerance"]["min_cosine_to_expected"].as_f64().unwrap();
    for case in fx["cases"].as_array().unwrap() {
        let input = match case["input"].as_str() {
            Some(t) => t.to_owned(),
            None => case["input_repeat"]["text"]
                .as_str()
                .unwrap()
                .repeat(case["input_repeat"]["times"].as_u64().unwrap() as usize),
        };
        let got = e.embed(&input);
        match case["expected"]["vector"].as_array() {
            None => assert!(got.is_none(), "{}", case["name"]),
            Some(want) => {
                let want: Vec<f64> = want.iter().map(|v| v.as_f64().unwrap()).collect();
                let got = got.unwrap_or_else(|| panic!("{}: vektör yok", case["name"]));
                assert_eq!(got.len(), embed::DIM);
                let c = cos(&got, &want);
                assert!(c >= min, "{}: kosinüs {c}", case["name"]);
            }
        }
    }
}
