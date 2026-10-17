"""Rust çekirdek seçimi (R4) — güvenlik, content_hash/audit ve regex çıkarma.

``OSIRIS_NATIVE`` ortam değişkeni (süreç başında bir kez okunur):

- ``rust`` (varsayılan) — ``osiris_native`` (crates/osiris-py, PyO3) kullanılır. Modül
  yüklenemezse süreç **açık bir hatayla durur**; Python uygulamasına sessizce düşülmez
  (plan R4: iki uygulama aynı anda farklı sonuç üretmesin).
- ``python`` — Python referans uygulaması (geçiş dönemi; testler iki yolu da koşar).

Kurulum: ``pip install ./crates/osiris-py`` (Rust araç zinciri gerekir).
"""

from __future__ import annotations

import os

BACKEND = (os.getenv("OSIRIS_NATIVE", "rust").strip().lower() or "rust")
if BACKEND not in ("rust", "python"):
    raise RuntimeError(f"OSIRIS_NATIVE yalnızca 'rust' ya da 'python' olabilir: {BACKEND!r}")

native = None
if BACKEND == "rust":
    try:
        import osiris_native as native  # noqa: F401 — dışarıya `native` olarak açılır
    except ImportError as exc:  # pragma: no cover — kurulum hatası
        raise ImportError(
            "Rust çekirdeği (osiris_native) yüklenemedi. Kurun: pip install ./crates/osiris-py "
            "— ya da geçici olarak OSIRIS_NATIVE=python ile Python uygulamasını seçin."
        ) from exc
