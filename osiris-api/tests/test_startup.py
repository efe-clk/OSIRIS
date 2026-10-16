"""Başlatma güvenliği testleri (K9): anahtarsız açık mod yalnızca açıkça istenirse."""

import os
import subprocess
import sys

os.environ.setdefault("OSIRIS_DATABASE_URL", "postgresql://localhost/db")
os.environ.setdefault("OSIRIS_REDIS_URL", "redis://localhost:6379/0")

import osiris_api.main as api_main  # noqa: E402
import pytest  # noqa: E402
from fastapi.testclient import TestClient  # noqa: E402
from osiris_api.auth import create_token  # noqa: E402
from osiris_collector.manager import CollectorManager  # noqa: E402


def _client(monkeypatch, api_key: str, jwt_secret: str, insecure: str | None) -> TestClient:
    monkeypatch.setattr(api_main, "API_KEY", api_key)
    monkeypatch.setattr(api_main, "JWT_SECRET", jwt_secret)
    if insecure is None:
        monkeypatch.delenv("OSIRIS_INSECURE_OPEN", raising=False)
    else:
        monkeypatch.setenv("OSIRIS_INSECURE_OPEN", insecure)
    monkeypatch.setattr(api_main, "_collector_loaded", True)
    api_main._collector = CollectorManager.__new__(CollectorManager)
    api_main._collector.plugins = {}
    api_main._collector.manifests = {}
    return TestClient(api_main.app, raise_server_exceptions=False)


def test_startup_refused_without_key(monkeypatch) -> None:
    c = _client(monkeypatch, "", "", None)
    with pytest.raises(RuntimeError, match="OSIRIS_API_KEY"):
        with c:  # lifespan → check_startup_config
            pass


@pytest.mark.parametrize("value", ["", "0", "true", "yes"])
def test_insecure_flag_must_be_exactly_one(monkeypatch, value: str) -> None:
    _client(monkeypatch, "", "", value)
    with pytest.raises(RuntimeError):
        api_main.check_startup_config()


def test_insecure_open_mode_is_explicit(monkeypatch) -> None:
    c = _client(monkeypatch, "", "", "1")
    with c:
        assert c.get("/plugins").status_code == 200


def test_no_key_without_lifespan_fails_closed(monkeypatch) -> None:
    # Başlatma denetimi atlansa bile uçlar kimliksiz açılmamalı
    c = _client(monkeypatch, "", "", None)
    assert c.get("/plugins").status_code == 503


def test_startup_ok_with_key(monkeypatch) -> None:
    c = _client(monkeypatch, "k" * 32, "s" * 32, None)
    with c:
        assert c.get("/plugins").status_code == 401
        assert c.get("/plugins", headers={"X-API-Key": "k" * 32}).status_code == 200


def test_token_endpoint_503_without_jwt_secret(monkeypatch) -> None:
    c = _client(monkeypatch, "k" * 32, "", None)
    r = c.post("/auth/token", json={"api_key": "k" * 32})
    assert r.status_code == 503


def test_api_key_is_not_a_jwt_secret(monkeypatch) -> None:
    # Eski davranış: JWT_SECRET boşsa API_KEY ile imzalanan jetonlar geçerliydi
    key = "k" * 32
    c = _client(monkeypatch, key, "", None)
    token = create_token("attacker", key, role="admin")["access_token"]
    r = c.get("/plugins", headers={"Authorization": f"Bearer {token}"})
    assert r.status_code == 401


def test_jwt_secret_has_no_api_key_fallback() -> None:
    env = {k: v for k, v in os.environ.items() if k != "OSIRIS_JWT_SECRET"}
    env["OSIRIS_API_KEY"] = "operator-key"
    out = subprocess.run(
        [sys.executable, "-c", "import osiris_api.main as m; print(repr(m.JWT_SECRET))"],
        env=env, capture_output=True, text=True, check=True, timeout=60,
    )
    assert out.stdout.strip() == "''"
