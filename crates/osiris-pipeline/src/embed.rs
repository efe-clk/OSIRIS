//! fastembed (ONNX) — `paraphrase-multilingual-MiniLM-L12-v2` (384 boyut, SOZLESME §10).

use anyhow::{Context, Result};
use fastembed::{
    InitOptionsUserDefined, Pooling, QuantizationMode, TextEmbedding, TokenizerFiles,
    UserDefinedEmbeddingModel,
};
use std::path::Path;
use std::sync::Mutex;

pub const DEFAULT_MODEL: &str = "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2";
pub const LOCAL_DIR: &str = "embedding/paraphrase-multilingual-MiniLM-L12-v2-onnx-Q";
pub const DIM: usize = 384;
pub const MAX_EMBED_CHARS: usize = 8000;

pub struct Embedder {
    model: Mutex<TextEmbedding>,
    pub name: String,
}

impl Embedder {
    /// Yalnızca `models/download.py`'nin indirdiği yerel dosyalar (ağdan indirme yok).
    pub fn load(root: &Path, name: &str) -> Result<Self> {
        if name != DEFAULT_MODEL {
            anyhow::bail!("desteklenen embedding modeli yalnızca {DEFAULT_MODEL}");
        }
        let d = root.join(LOCAL_DIR);
        let read = |f: &str| std::fs::read(d.join(f)).with_context(|| format!("{f} okunamadı"));
        let files = TokenizerFiles {
            tokenizer_file: read("tokenizer.json")?,
            config_file: read("config.json")?,
            special_tokens_map_file: read("special_tokens_map.json")?,
            tokenizer_config_file: read("tokenizer_config.json")?,
        };
        // fastembed'in ParaphraseMLMiniLML12V2Q tanımıyla aynı: mean pooling, statik kuantizasyon
        let model = UserDefinedEmbeddingModel::new(read("model_optimized.onnx")?, files)
            .with_pooling(Pooling::Mean)
            .with_quantization(QuantizationMode::Static);
        // Python fastembed `_resolve_max_context`: model_max_length ve max_length'in küçüğü
        // (bu model için 128); Rust fastembed yalnızca model_max_length'e bakar.
        let tc: serde_json::Value = serde_json::from_slice(&read("tokenizer_config.json")?)?;
        let ctx = |k: &str| tc.get(k).and_then(serde_json::Value::as_u64).filter(|v| *v > 0);
        let max_len = [ctx("model_max_length"), ctx("max_length")]
            .into_iter()
            .flatten()
            .min()
            .context("bağlam uzunluğu yok")?;
        let opts = InitOptionsUserDefined::default().with_max_length(max_len as usize);
        let te = TextEmbedding::try_new_from_user_defined(model, opts)?;
        Ok(Self { model: Mutex::new(te), name: name.to_owned() })
    }

    /// L2-normalize vektör; boş metin, sonlu olmayan değer ya da sıfır vektör → None.
    pub fn embed(&self, text: &str) -> Option<Vec<f32>> {
        if osiris_hash::py_strip(text).is_empty() {
            return None;
        }
        let t: String = text.chars().take(MAX_EMBED_CHARS).collect();
        let mut m = self.model.lock().ok()?;
        let v = m.embed(vec![t], None).ok()?.into_iter().next()?;
        if v.is_empty() || !v.iter().all(|x| x.is_finite()) {
            return None;
        }
        let norm = v.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>().sqrt();
        if norm == 0.0 {
            return None;
        }
        Some(v.iter().map(|x| (f64::from(*x) / norm) as f32).collect())
    }
}
