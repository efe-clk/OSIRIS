"""OSIRIS API sözleşmesi — auth matrisi, uçlar, hata kodları, sayfalama sınırları.

Bkz. docs/API.md ve docs/SOZLESME.md §7. Sağlayıcıdan bağımsızdır: yalnızca HTTP.
"""

from __future__ import annotations

import pytest
from conftest import DESTRUCTIVE, random_uuid

ROLES = ["none", "bad", "viewer", "analyst", "admin"]
_RANK = {"viewer": 1, "analyst": 2, "admin": 3}


def _expected(role: str, minimum: str, ok: int) -> int:
    if role in ("none", "bad"):
        return 401
    return ok if _RANK[role] >= _RANK[minimum] else 403


# (yöntem, yol, gövde, en düşük rol, yetkili çağrıda beklenen kod)
# Gövdeler canlı sistemde güvenlidir: var olmayan kimlikler, iç ağ URL'si (SSRF → 502).
MATRIX = [
    ("GET", "/plugins", None, "viewer", 200),
    ("GET", "/stats", None, "viewer", 200),
    ("GET", "/items/recent", None, "viewer", 200),
    ("GET", f"/items/{random_uuid()}", None, "viewer", 404),
    ("GET", "/entities/top", None, "viewer", 200),
    ("POST", "/search", {"query": "osiris-contract-yok"}, "viewer", 200),
    ("POST", "/search/entity", {"entity_type": "email", "value": "yok@contract.invalid"}, "viewer", 200),
    ("GET", "/graph", None, "viewer", 200),
    ("GET", "/sources", None, "viewer", 200),
    ("GET", "/saved-queries", None, "viewer", 200),
    ("POST", "/collect", {"plugin_id": "rss", "config": {"feed_url": "http://127.0.0.1/"}}, "analyst", 502),
    ("POST", "/collect/batch", {"jobs": [{"plugin_id": "rss", "config": {"feed_url": "http://127.0.0.1/"}}]}, "analyst", 200),
    ("DELETE", f"/sources/{random_uuid()}", None, "analyst", 404),
    ("POST", f"/sources/{random_uuid()}/collect", None, "analyst", 404),
    ("DELETE", f"/saved-queries/{random_uuid()}", None, "analyst", 404),
    ("POST", "/alerts/test", {"text": "contract metni"}, "analyst", 200),
    ("POST", "/reports/markdown", {"title": "Contract"}, "analyst", 200),
    ("GET", "/auth/keys", None, "admin", 200),
    ("DELETE", f"/auth/keys/{random_uuid()}", None, "admin", 404),
    ("DELETE", f"/items/{random_uuid()}", None, "admin", 404),
    ("POST", "/entities/erase", {"entity_type": "email", "value": "yok-contract@contract.invalid"}, "admin", 200),
]


@pytest.mark.parametrize("role", ROLES)
@pytest.mark.parametrize("method,path,body,minimum,ok", MATRIX,
                         ids=[f"{m} {p.split('/')[1]}" for m, p, *_ in MATRIX])
def test_auth_matrix(client, keys, role, method, path, body, minimum, ok) -> None:
    r = client.request(method, path, headers=keys[role], json=body)
    assert r.status_code == _expected(role, minimum, ok), r.text


@pytest.mark.parametrize("role", ["viewer", "analyst"])
def test_admin_only_mutations_forbidden_below_admin(client, keys, role) -> None:
    # Bu çağrılar admin'de veri değiştirir; burada yalnızca reddi doğrulanır
    assert client.post("/admin/retention/run", headers=keys[role]).status_code == 403
    assert client.post("/auth/keys", headers=keys[role],
                       json={"username": "x", "role": "admin"}).status_code == 403


def test_health_is_open(client) -> None:
    r = client.get("/health")
    assert r.status_code == 200 and r.json()["status"] == "ok"


def test_token_flow(client, keys) -> None:
    assert client.post("/auth/token", json={"api_key": "yanlis"}).status_code in (401, 503)
    r = client.post("/auth/token", json={"api_key": keys["admin"]["X-API-Key"], "role": "viewer"})
    if r.status_code == 503:
        pytest.skip("JWT kapalı (OSIRIS_JWT_SECRET yok)")
    assert r.status_code == 200
    tok = r.json()
    assert tok["token_type"] == "bearer" and tok["expires_in"] > 0
    bearer = {"Authorization": f"Bearer {tok['access_token']}"}
    assert client.get("/plugins", headers=bearer).status_code == 200
    assert client.post("/alerts/test", headers=bearer, json={"text": "x"}).status_code == 403
    assert client.get("/plugins", headers={"Authorization": "Bearer a.b.c"}).status_code == 401
    assert client.post("/auth/token", json={"api_key": keys["admin"]["X-API-Key"],
                                             "role": "root"}).status_code == 400


def test_plugins_inventory_shape(client, keys) -> None:
    plugins = client.get("/plugins", headers=keys["viewer"]).json()
    ids = {p["id"] for p in plugins}
    assert {"rss", "web-scraper", "irc", "username-search"} <= ids
    for p in plugins:
        assert set(p) >= {"id", "name", "network_type", "description", "config_schema",
                          "schedule_default"}


# --------------------------------------------------------------- doğrulama / hata kodları
@pytest.mark.parametrize("method,path,body,status", [
    ("POST", "/search", {"query": "x" * 501}, 422),
    ("POST", "/search", {"query": ""}, 422),
    ("POST", "/search", {"query": "x", "limit": 0}, 422),
    ("POST", "/search", {"query": "x", "limit": 101}, 422),
    ("POST", "/search/entity", {"entity_type": "uzayli", "value": "x"}, 400),
    ("POST", "/collect", {"plugin_id": "YOK_BOYLE", "config": {}}, 422),
    ("POST", "/collect", {"plugin_id": "yok-boyle-plugin", "config": {}}, 404),
    ("POST", "/collect", {"plugin_id": "rss", "config": {f"k{i}": 1 for i in range(51)}}, 422),
    ("POST", "/collect", {"plugin_id": "rss", "config": {"feed_url": "x" * 21_000}}, 413),
    ("POST", "/collect", {"plugin_id": "rss", "config": {}}, 502),
    ("POST", "/collect/batch", {"jobs": []}, 422),
    ("POST", "/collect/batch", {"jobs": [{"plugin_id": "rss"}] * 11}, 422),
    ("POST", "/sources", {"name": "x", "plugin_id": "rss", "network_type": "uzay"}, 400),
    ("POST", "/sources", {"name": "x", "plugin_id": "rss", "priority": 11}, 422),
    ("POST", "/sources", {"name": "x", "plugin_id": "rss", "retention_days": -1}, 422),
    ("POST", "/sources", {"name": "x", "plugin_id": "yok-plugin"}, 404),
    ("POST", "/saved-queries", {"name": "x", "query_text": "y", "query_type": "sihir"}, 400),
    ("GET", "/items/bozuk-uuid", None, 404),
    ("DELETE", "/sources/1;DROP", None, 404),
    ("POST", "/entities/erase", {"entity_type": "uzayli", "value": "x"}, 400),
    ("POST", "/entities/erase", {"entity_type": "email", "value": "   "}, 422),
])
def test_validation_errors(client, admin, method, path, body, status) -> None:
    r = client.request(method, path, headers=admin, json=body)
    assert r.status_code == status, r.text
    detail = r.json().get("detail")
    assert detail is not None
    text = str(detail)
    for leak in ("Traceback", "psycopg", "File \"/", "SELECT ", "INSERT "):
        assert leak not in text, f"iç ayrıntı sızdı: {text[:200]}"


def test_entities_top_bad_type(client, keys) -> None:
    assert client.get("/entities/top", params={"entity_type": "uzayli"},
                      headers=keys["viewer"]).status_code == 400


# ------------------------------------------------------------------------- sayfalama
@pytest.mark.parametrize("path,cap", [("/sources", 200), ("/saved-queries", 200),
                                      ("/items/recent", 50)])
def test_pagination_bounds(client, keys, path, cap) -> None:
    h = keys["viewer"]
    assert client.get(path, params={"limit": 0}, headers=h).status_code == 200
    r = client.get(path, params={"limit": 100_000}, headers=h)
    assert r.status_code == 200 and len(r.json()) <= cap
    assert client.get(path, params={"limit": "abc"}, headers=h).status_code == 422


@pytest.mark.parametrize("path", ["/sources", "/saved-queries"])
def test_negative_offset_rejected(client, keys, path) -> None:
    assert client.get(path, params={"offset": -1}, headers=keys["viewer"]).status_code == 422


# ------------------------------------------------------------------- yaşam döngüleri
def test_source_lifecycle(client, keys, run_id) -> None:
    h = keys["analyst"]
    r = client.post("/sources", headers=h, json={
        "name": f"contract-{run_id}", "plugin_id": "rss", "network_type": "rss",
        "url": "http://127.0.0.1/feed", "enabled": False, "retention_days": 30,
        "tags": ["contract"]})
    assert r.status_code == 200, r.text
    sid = r.json()["id"]
    try:
        listed = client.get("/sources", params={"q": f"contract-{run_id}"}, headers=keys["viewer"]).json()
        assert [s["id"] for s in listed] == [sid]
        assert listed[0]["retention_days"] == 30 and listed[0]["enabled"] is False
        assert client.post(f"/sources/{sid}/collect", headers=h).status_code == 409
        # LIKE jokerleri kaçırılır: '%' her şeyi eşlememeli
        assert all("%" in s["name"] for s in client.get(
            "/sources", params={"q": "%"}, headers=keys["viewer"]).json())
    finally:
        assert client.delete(f"/sources/{sid}", headers=h).status_code == 200
    assert client.delete(f"/sources/{sid}", headers=h).status_code == 404


def test_saved_query_lifecycle_and_alert_match(client, keys, run_id) -> None:
    h = keys["analyst"]
    needle = f"contract-alarm-{run_id}"
    r = client.post("/saved-queries", headers=h, json={
        "name": f"contract-{run_id}", "query_text": needle, "alert_enabled": True})
    assert r.status_code == 200, r.text
    qid = r.json()["id"]
    try:
        hits = client.post("/alerts/test", headers=h,
                           json={"text": f"metinde {needle.upper()} geçiyor"}).json()
        assert any(t["query_id"] == qid for t in hits)
        miss = client.post("/alerts/test", headers=h, json={"text": "ilgisiz"}).json()
        assert not any(t["query_id"] == qid for t in miss)
    finally:
        assert client.delete(f"/saved-queries/{qid}", headers=h).status_code == 200


def test_api_key_lifecycle(client, admin, run_id) -> None:
    r = client.post("/auth/keys", headers=admin,
                    json={"username": f"contract-{run_id}-tmp", "role": "viewer"})
    assert r.status_code == 200
    body = r.json()
    assert body["api_key"].startswith("osiris_")
    listed = client.get("/auth/keys", headers=admin).json()
    row = next(k for k in listed if k["id"] == body["id"])
    assert "api_key" not in row and "key_hash" not in row  # ham değer/özet asla dönmez
    tmp = {"X-API-Key": body["api_key"]}
    assert client.get("/plugins", headers=tmp).status_code == 200
    assert client.delete(f"/auth/keys/{body['id']}", headers=admin).status_code == 200
    assert client.get("/plugins", headers=tmp).status_code == 401


def test_collect_batch_partial_results(client, keys) -> None:
    r = client.post("/collect/batch", headers=keys["analyst"], json={"jobs": [
        {"plugin_id": "rss", "config": {"feed_url": "http://10.0.0.1/"}},
        {"plugin_id": "yok-plugin", "config": {}},
    ]})
    assert r.status_code == 200
    body = r.json()
    assert body["total"] == 2 and body["ok"] == 0
    assert [j["ok"] for j in body["jobs"]] == [False, False]


def test_report_markdown_escapes_nothing_unsafe(client, keys) -> None:
    r = client.post("/reports/markdown", headers=keys["analyst"], json={
        "title": "Rapor", "findings": [{"title": "<script>x</script>", "description": "d"}]})
    assert r.status_code == 200
    assert "<script>" not in r.json()["markdown"]


# --------------------------------------------------------------- kalıcı değişiklikler
@pytest.mark.skipif(not DESTRUCTIVE, reason="OSIRIS_CONTRACT_DESTRUCTIVE=1 değil")
def test_graph_relation_and_erase(client, keys, admin, run_id) -> None:
    a, b = f"contract-a-{run_id}", f"contract-b-{run_id}"
    assert client.post("/graph/relation", headers=keys["analyst"],
                       json={"source": a, "target": b}).status_code == 200
    g = client.get("/graph", headers=keys["viewer"]).json()
    assert any(e["source"] in (a, b) and e["target"] in (a, b) for e in g["edges"])
    r = client.post("/entities/erase", headers=admin, json={"entity_type": "custom", "value": a.upper()})
    assert r.status_code == 200 and r.json()["edges_deleted"] >= 1
    g2 = client.get("/graph", headers=keys["viewer"]).json()
    assert not any(a in (e["source"], e["target"]) for e in g2["edges"])


@pytest.mark.skipif(not DESTRUCTIVE, reason="OSIRIS_CONTRACT_DESTRUCTIVE=1 değil")
def test_retention_run(client, admin) -> None:
    r = client.post("/admin/retention/run", headers=admin)
    assert r.status_code == 200
    assert set(r.json()) >= {"items_deleted", "entities_deleted", "default_days"}
