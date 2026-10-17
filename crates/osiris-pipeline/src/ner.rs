//! ONNX NER — `osiris_pipeline/ner.py` ile aynı kurallar (docs/SOZLESME.md §11).
//!
//! 1. Metin: ilk `MAX_CHARS` kod noktası. 2. Özel tokensız tokenize; `WINDOW` token'lık
//!    pencereler `[CLS] … [SEP]`, `token_type_ids = 0`, `attention_mask = 1`.
//! 3. Token başına argmax → `id2label`. 4. Kelime etiketi = ilk alt-token'ınki.
//! 5. BIO birleştirme. 6. Değer = ilk kelime başı..son kelime sonu; uçlardan boşluk ve Unicode
//!    `P*` atılır; < 2 kod noktası atılır; PER→person, ORG→org, LOC→location.
//!
//! Rust tokenizers bayt ofseti döndürür; aynı dize üzerinde dilimlendiği için Python'un kod
//! noktası ofsetleriyle aynı alt dizeyi verir.

use anyhow::{Context, Result};
use ort::session::Session;
use ort::value::Tensor;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};
use tokenizers::Tokenizer;

pub const MODEL_SUBDIR: &str = "ner/bert-base-multilingual-cased-ner-hrl";
pub const MAX_CHARS: usize = 10_000;
pub const WINDOW: usize = 510;

pub struct OnnxNer {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    id2label: HashMap<usize, String>,
    cls: i64,
    sep: i64,
}

fn map_type(t: &str) -> Option<&'static str> {
    match t {
        "PER" => Some("person"),
        "ORG" => Some("org"),
        "LOC" => Some("location"),
        _ => None,
    }
}

/// Uçlardaki Python boşluğu ve Unicode noktalaması (P*) atılır.
pub fn trim_punct(v: &str) -> &str {
    static P: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"^\p{P}$").expect("P"));
    let is_p = |c: char| {
        let mut b = [0u8; 4];
        osiris_hash::py_is_space(c) || P.is_match(c.encode_utf8(&mut b))
    };
    osiris_hash::py_strip(v).trim_matches(is_p)
}

impl OnnxNer {
    pub fn available(root: &Path) -> bool {
        let d = root.join(MODEL_SUBDIR);
        ["onnx/model_int8.onnx", "tokenizer.json", "config.json"]
            .iter()
            .all(|f| d.join(f).is_file())
    }

    pub fn load(dir: &Path) -> Result<Self> {
        let session = Session::builder()?
            .with_intra_threads(1)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .commit_from_file(dir.join("onnx/model_int8.onnx"))
            .map_err(|e| anyhow::anyhow!("NER modeli yüklenemedi: {e}"))?;
        let tokenizer =
            Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|e| anyhow::anyhow!("{e}"))?;
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("config.json"))?)?;
        let id2label = config["id2label"]
            .as_object()
            .context("id2label yok")?
            .iter()
            .filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?.to_owned())))
            .collect();
        let tok = |t: &str| tokenizer.token_to_id(t).map(i64::from).context("özel token yok");
        Ok(Self {
            cls: tok("[CLS]")?,
            sep: tok("[SEP]")?,
            session: Mutex::new(session),
            tokenizer,
            id2label,
        })
    }

    fn labels(&self, ids: &[u32]) -> Result<Vec<String>> {
        let mut seq: Vec<i64> = Vec::with_capacity(ids.len() + 2);
        seq.push(self.cls);
        seq.extend(ids.iter().map(|&i| i64::from(i)));
        seq.push(self.sep);
        let n = seq.len();
        let input_ids = Tensor::from_array(([1usize, n], seq))?;
        let mask = Tensor::from_array(([1usize, n], vec![1i64; n]))?;
        let types = Tensor::from_array(([1usize, n], vec![0i64; n]))?;
        let mut session =
            self.session.lock().map_err(|_| anyhow::anyhow!("NER oturumu kilitlenemedi"))?;
        let outputs = session.run(ort::inputs!["input_ids" => input_ids, "attention_mask" => mask, "token_type_ids" => types])?;
        let (shape, logits) = outputs["logits"].try_extract_tensor::<f32>()?;
        let labels = shape[2] as usize;
        let mut out = Vec::with_capacity(n - 2);
        for t in 1..n - 1 {
            let row = &logits[t * labels..(t + 1) * labels];
            // numpy argmax: ilk en büyük
            let mut best = 0;
            for (i, &v) in row.iter().enumerate() {
                if v > row[best] {
                    best = i;
                }
            }
            out.push(self.id2label.get(&best).cloned().unwrap_or_else(|| "O".into()));
        }
        Ok(out)
    }

    /// `[{"type", "value"}]` — metindeki sırayla.
    pub fn extract(&self, text: &str) -> Result<Vec<(String, String)>> {
        let text: String = text.chars().take(MAX_CHARS).collect();
        if osiris_hash::py_strip(&text).is_empty() {
            return Ok(Vec::new());
        }
        let enc =
            self.tokenizer.encode(text.as_str(), false).map_err(|e| anyhow::anyhow!("{e}"))?;
        let ids = enc.get_ids();
        let offsets = enc.get_offsets();
        let word_ids = enc.get_word_ids();
        let mut labels = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(WINDOW) {
            labels.extend(self.labels(chunk)?);
        }
        // kelime düzeyi: ilk alt-token etiketi, kelimenin tüm aralığı
        let mut words: Vec<(String, usize, usize)> = Vec::new();
        let mut prev: Option<u32> = None;
        for ((label, &(s, e)), wid) in labels.iter().zip(offsets).zip(word_ids) {
            let Some(w) = *wid else { continue };
            if Some(w) != prev {
                words.push((label.clone(), s, e));
                prev = Some(w);
            } else if let Some(last) = words.last_mut() {
                last.2 = e;
            }
        }
        // BIO birleştirme
        let mut spans: Vec<(String, usize, usize)> = Vec::new();
        let mut cur: Option<(String, usize, usize)> = None;
        for (label, s, e) in words {
            let (tag, etype) = label.split_once('-').map_or((label.as_str(), ""), |(a, b)| (a, b));
            if label == "O" || map_type(etype).is_none() {
                spans.extend(cur.take());
                continue;
            }
            if tag == "I"
                && let Some(c) = cur.as_mut()
                && c.0 == etype
            {
                c.2 = e;
                continue;
            }
            spans.extend(cur.take());
            cur = Some((etype.to_owned(), s, e));
        }
        spans.extend(cur);
        Ok(spans
            .into_iter()
            .filter_map(|(etype, s, e)| {
                let v = trim_punct(text.get(s..e)?);
                if v.chars().count() < 2 {
                    return None;
                }
                Some((map_type(&etype)?.to_owned(), v.to_owned()))
            })
            .collect())
    }
}
