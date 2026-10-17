"""Rust çekirdek seçimi (osiris._native, R4)."""

import importlib
import os

import pytest


def test_backend_matches_environment() -> None:
    from osiris import _native

    expected = os.getenv("OSIRIS_NATIVE", "rust").strip().lower() or "rust"
    assert _native.BACKEND == expected
    if expected == "rust":
        # CI'da Rust yolu gerçekten koşmalı: modül yoksa içe aktarma zaten hata verirdi
        assert _native.native is not None
        assert _native.native.__version__
    else:
        assert _native.native is None


def test_invalid_backend_rejected(monkeypatch) -> None:
    from osiris import _native

    monkeypatch.setenv("OSIRIS_NATIVE", "cpp")
    with pytest.raises(RuntimeError, match="OSIRIS_NATIVE"):
        importlib.reload(_native)
    monkeypatch.undo()
    importlib.reload(_native)


def test_public_functions_dispatch_to_rust() -> None:
    from osiris import _native, audit, security

    if _native.native is None:
        pytest.skip("OSIRIS_NATIVE=python")
    # Rust yolu: ayrıştırıcı farkı karakterleri Rust uygulamasında reddedilir
    with pytest.raises(ValueError, match="geçersiz karakter"):
        security.assert_safe_url("http://a\\b/")
    assert audit.chain_hash(None, "x") == audit._py_chain_hash(None, "x")
