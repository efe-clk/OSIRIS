"""Kara kutu API sözleşme testleri — çalışan bir OSIRIS API'sine gerçek HTTP atar.

Aynı paket Python API'sine ve (R6'da) Rust API'sine DEĞİŞTİRİLMEDEN karşı koşar.

Ortam değişkenleri:
  OSIRIS_BASE_URL               örn. https://sunucu:8443/api veya http://127.0.0.1:8001
  OSIRIS_CONTRACT_API_KEY       operatör (admin) anahtarı
  OSIRIS_CONTRACT_VERIFY_TLS    0 → öz-imzalı sertifikayı kabul et (varsayılan 1)
  OSIRIS_CONTRACT_DESTRUCTIVE   1 → kalıcı değişiklik yapan testleri de koş
                                 (graf kenarı ekleme, saklama temizliği). Canlıda AÇMAYIN.
Değişkenler yoksa testler atlanır. Testler oluşturdukları kaynak/sorgu kayıtlarını
siler; oluşturdukları kullanıcı anahtarlarını sonunda iptal eder (API anahtar
silmeyi değil iptali destekler).
"""

from __future__ import annotations

import os
import uuid

import httpx
import pytest

BASE_URL = os.getenv("OSIRIS_BASE_URL", "").rstrip("/")
ADMIN_KEY = os.getenv("OSIRIS_CONTRACT_API_KEY", "")
VERIFY = os.getenv("OSIRIS_CONTRACT_VERIFY_TLS", "1") != "0"
DESTRUCTIVE = os.getenv("OSIRIS_CONTRACT_DESTRUCTIVE", "") == "1"
RUN_ID = uuid.uuid4().hex[:8]


def pytest_collection_modifyitems(config, items):
    if BASE_URL and ADMIN_KEY:
        return
    skip = pytest.mark.skip(reason="OSIRIS_BASE_URL ve OSIRIS_CONTRACT_API_KEY gerekli")
    for item in items:
        if "tests/contract" in str(item.fspath).replace("\\", "/"):
            item.add_marker(skip)


@pytest.fixture(scope="session")
def client():
    with httpx.Client(base_url=BASE_URL, verify=VERIFY, timeout=60) as c:
        yield c


def _mint_key(client: httpx.Client, role: str) -> tuple[str, str]:
    r = client.post("/auth/keys", headers={"X-API-Key": ADMIN_KEY},
                    json={"username": f"contract-{RUN_ID}-{role}", "role": role,
                          "name": "contract-test"})
    assert r.status_code == 200, r.text
    body = r.json()
    return body["id"], body["api_key"]


@pytest.fixture(scope="session")
def keys(client):
    """Rol başına kimlik başlıkları: none / bad / viewer / analyst / admin."""
    minted = {role: _mint_key(client, role) for role in ("viewer", "analyst")}
    yield {
        "none": {},
        "bad": {"X-API-Key": "osiris_gecersiz_" + RUN_ID},
        "viewer": {"X-API-Key": minted["viewer"][1]},
        "analyst": {"X-API-Key": minted["analyst"][1]},
        "admin": {"X-API-Key": ADMIN_KEY},
    }
    for key_id, _ in minted.values():
        client.delete(f"/auth/keys/{key_id}", headers={"X-API-Key": ADMIN_KEY})


@pytest.fixture
def admin(keys):
    return keys["admin"]


@pytest.fixture
def run_id() -> str:
    return RUN_ID


def random_uuid() -> str:
    return str(uuid.uuid4())
