//! ONNX model indirici (R7; eski `models/download.py`'nin yerine) — `osiris-server models`.
//!
//! Her dosya SABİT bir Hugging Face revizyonundan iner ve doğrulanır (LFS → sha256, diğerleri
//! → git blob sha1). Doğrulanmamış dosya hedefe yazılmaz (geçici dosya + yeniden adlandırma).
//! Modeller repoya ve imaja gömülmez (NER modeli AFL-3.0). Bkz. docs/SOZLESME.md §10–11.

use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Sha256(&'static str),
    GitSha1(&'static str),
}

pub struct ModelSpec {
    pub name: &'static str,
    pub repo: &'static str,
    pub revision: &'static str,
    pub license: &'static str,
    pub dir: &'static str,
    pub files: &'static [(&'static str, Check)],
}

pub const MODELS: [ModelSpec; 2] = [
    ModelSpec {
        name: "embedding",
        repo: "Qdrant/paraphrase-multilingual-MiniLM-L12-v2-onnx-Q",
        revision: "faf4aa4225822f3bc6376869cb1164e8e3feedd0",
        license: "Apache-2.0",
        dir: "embedding/paraphrase-multilingual-MiniLM-L12-v2-onnx-Q",
        files: &[
            (
                "model_optimized.onnx",
                Check::Sha256("634d0f66c29dc934c8fa72b8a4fe91dd4d420a22f1d82a241058d4316e659a99"),
            ),
            (
                "tokenizer.json",
                Check::Sha256("fa685fc160bbdbab64058d4fc91b60e62d207e8dc60b9af5c002c5ab946ded00"),
            ),
            ("config.json", Check::GitSha1("5b496dbbbe502a10e2d64525481c6f444d125403")),
            ("tokenizer_config.json", Check::GitSha1("6af3e8bba20e4425103afb1ae3dee3cacdbe7afb")),
            ("special_tokens_map.json", Check::GitSha1("b1879d702821e753ffe4245048eee415d54a9385")),
        ],
    },
    ModelSpec {
        name: "ner",
        repo: "onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX",
        revision: "cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8",
        license: "AFL-3.0 (Davlan/bert-base-multilingual-cased-ner-hrl)",
        dir: "ner/bert-base-multilingual-cased-ner-hrl",
        files: &[
            (
                "onnx/model_int8.onnx",
                Check::Sha256("1213fdd405d295768b0d41d8214062f2f278f0e3acff6af67d8fd47360d2be0f"),
            ),
            ("tokenizer.json", Check::GitSha1("21f54a4b56685f29358f3a8de1f5b8d827357d07")),
            ("config.json", Check::GitSha1("1a157cbca18ca26d60e57fa6a296c18ac5ef9fa7")),
            ("tokenizer_config.json", Check::GitSha1("4af6573d6efd88def3cd888217e2ddd09fc4dd82")),
            ("special_tokens_map.json", Check::GitSha1("a8b3208c2884c4efb86e49300fdd3dc877220cdf")),
        ],
    },
];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Dosya özeti (`gitsha1` = `sha1("blob <boyut>\0" + içerik)`).
pub fn digest(path: &Path, check: Check) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 1 << 20];
    match check {
        Check::Sha256(_) => {
            let mut h = Sha256::new();
            loop {
                let n = f.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            Ok(hex(&h.finalize()))
        }
        Check::GitSha1(_) => {
            let mut h = Sha1::new();
            h.update(format!("blob {}\0", f.metadata()?.len()).as_bytes());
            loop {
                let n = f.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            Ok(hex(&h.finalize()))
        }
    }
}

fn want(check: Check) -> &'static str {
    match check {
        Check::Sha256(w) | Check::GitSha1(w) => w,
    }
}

/// Modeli indirir/doğrular; önbellekteki doğru dosyalar yeniden inmez. Dönen: model dizini.
pub async fn fetch(spec: &ModelSpec, root: &Path) -> anyhow::Result<PathBuf> {
    let target = root.join(spec.dir);
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(600)).build()?;
    for (rel, check) in spec.files {
        let dest = target.join(rel);
        if dest.is_file() && digest(&dest, *check)? == want(*check) {
            println!("  ✓ {}/{rel} (önbellekte)", spec.name);
            continue;
        }
        let parent = dest.parent().unwrap_or(&target).to_path_buf();
        std::fs::create_dir_all(&parent)?;
        let url = format!("https://huggingface.co/{}/resolve/{}/{rel}", spec.repo, spec.revision);
        println!("  ↓ {}/{rel}", spec.name);
        let mut resp = client.get(&url).send().await?.error_for_status()?;
        let tmp = parent.join(format!(
            ".{}.indiriliyor",
            dest.file_name().unwrap_or_default().to_string_lossy()
        ));
        {
            use std::io::Write;
            let mut out = std::fs::File::create(&tmp)?;
            while let Some(chunk) = resp.chunk().await? {
                out.write_all(&chunk)?;
            }
        }
        let got = digest(&tmp, *check)?;
        if got != want(*check) {
            let _ = std::fs::remove_file(&tmp);
            anyhow::bail!("DOĞRULAMA HATASI {}/{rel}: {got} != {}", spec.name, want(*check));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644))?;
        }
        std::fs::rename(&tmp, &dest)?;
    }
    println!(
        "{}: {}  [{}@{}, {}]",
        spec.name,
        target.display(),
        spec.repo,
        &spec.revision[..10],
        spec.license
    );
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_blob_ozeti() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x");
        std::fs::write(&p, b"hello\n").unwrap();
        // `git hash-object` değeri
        assert_eq!(
            digest(&p, Check::GitSha1("")).unwrap(),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
        assert_eq!(
            digest(&p, Check::Sha256("")).unwrap(),
            "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
    }
}
