"""OSIRIS ONNX modellerini indirir ve doğrular (repoya ikili dosya gömülmez).

Kullanım:  python models/download.py [--dir models] [--only ner|embedding]
Hedef dizin varsayılanı: $OSIRIS_MODEL_DIR veya bu betiğin dizini.

Her dosya SABİT bir Hugging Face revizyonundan iner ve doğrulanır (LFS → sha256,
diğerleri → git blob sha1). Python ve Rust (R5) aynı dosyaları kullanır —
bkz. docs/SOZLESME.md §10–11. Yalnızca standart kütüphane kullanır.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import sys
import tempfile
import urllib.request
from pathlib import Path

MODELS = {
    "ner": {
        "repo": "onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX",
        "revision": "cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8",
        "license": "AFL-3.0 (Davlan/bert-base-multilingual-cased-ner-hrl)",
        "dir": "ner/bert-base-multilingual-cased-ner-hrl",
        "files": {
            "onnx/model_int8.onnx": ("sha256", "1213fdd405d295768b0d41d8214062f2f278f0e3acff6af67d8fd47360d2be0f"),
            "tokenizer.json": ("gitsha1", "21f54a4b56685f29358f3a8de1f5b8d827357d07"),
            "config.json": ("gitsha1", "1a157cbca18ca26d60e57fa6a296c18ac5ef9fa7"),
            "tokenizer_config.json": ("gitsha1", "4af6573d6efd88def3cd888217e2ddd09fc4dd82"),
            "special_tokens_map.json": ("gitsha1", "a8b3208c2884c4efb86e49300fdd3dc877220cdf"),
        },
    },
    "embedding": {
        "repo": "Qdrant/paraphrase-multilingual-MiniLM-L12-v2-onnx-Q",
        "revision": "faf4aa4225822f3bc6376869cb1164e8e3feedd0",
        "license": "Apache-2.0",
        "dir": "embedding/paraphrase-multilingual-MiniLM-L12-v2-onnx-Q",
        "files": {
            "model_optimized.onnx": ("sha256", "634d0f66c29dc934c8fa72b8a4fe91dd4d420a22f1d82a241058d4316e659a99"),
            "tokenizer.json": ("sha256", "fa685fc160bbdbab64058d4fc91b60e62d207e8dc60b9af5c002c5ab946ded00"),
            "config.json": ("gitsha1", "5b496dbbbe502a10e2d64525481c6f444d125403"),
            "tokenizer_config.json": ("gitsha1", "6af3e8bba20e4425103afb1ae3dee3cacdbe7afb"),
            "special_tokens_map.json": ("gitsha1", "b1879d702821e753ffe4245048eee415d54a9385"),
        },
    },
}


def _digest(path: Path, kind: str) -> str:
    h = hashlib.sha256() if kind == "sha256" else hashlib.sha1()
    if kind == "gitsha1":
        h.update(f"blob {path.stat().st_size}\0".encode())
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def fetch(name: str, root: Path) -> Path:
    spec = MODELS[name]
    target = root / spec["dir"]
    for rel, (kind, want) in spec["files"].items():
        dest = target / rel
        if dest.is_file() and _digest(dest, kind) == want:
            dest.chmod(0o644)
            print(f"  ✓ {name}/{rel} (önbellekte)")
            continue
        dest.parent.mkdir(parents=True, exist_ok=True)
        url = f"https://huggingface.co/{spec['repo']}/resolve/{spec['revision']}/{rel}"
        print(f"  ↓ {name}/{rel}")
        with tempfile.NamedTemporaryFile(dir=dest.parent, delete=False) as tmp:
            with urllib.request.urlopen(url, timeout=120) as resp:  # nosec B310 — sabit https
                for chunk in iter(lambda: resp.read(1 << 20), b""):
                    tmp.write(chunk)
        tmp_path = Path(tmp.name)
        got = _digest(tmp_path, kind)
        if got != want:
            tmp_path.unlink(missing_ok=True)
            raise SystemExit(f"DOĞRULAMA HATASI {name}/{rel}: {kind} {got} != {want}")
        tmp_path.chmod(0o644)  # tempfile 0600 oluşturur; konteyner kullanıcısı okuyabilmeli
        tmp_path.replace(dest)
    print(f"{name}: {target}  [{spec['repo']}@{spec['revision'][:10]}, {spec['license']}]")
    return target


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dir", default=os.getenv("OSIRIS_MODEL_DIR") or str(Path(__file__).parent))
    ap.add_argument("--only", choices=sorted(MODELS))
    args = ap.parse_args(argv)
    root = Path(args.dir).resolve()
    for name in ([args.only] if args.only else sorted(MODELS)):
        fetch(name, root)
    return 0


if __name__ == "__main__":
    sys.exit(main())
