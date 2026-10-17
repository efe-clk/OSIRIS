"""Python ↔ Rust API eşdeğerliği (R6) — aynı istekler iki API'ye, durum + gövde karşılaştırılır.

Sözleşme paketinin (tests/contract) kapsamadığı köşe durumlar: lax tamsayı/bool, sayfalama,
UUID biçimi, kimlik başlıkları, boyut sınırları, zaman damgası biçimi, sütun içerikleri.
R7'de Python API silinince bu paket de kalkar.

Ortam: OSIRIS_PARITY_PY_URL, OSIRIS_PARITY_RS_URL, OSIRIS_PARITY_API_KEY (ikisinin ortak
operatör anahtarı), OSIRIS_PARITY_DATABASE_URL (iki API'nin ortak veritabanı; test verisi
eklenir — canlıda KOŞMAYIN). Yoksa atlanır.
"""

from __future__ import annotations

import json
import os
import uuid

import httpx
import pytest

PY = os.getenv("OSIRIS_PARITY_PY_URL", "").rstrip("/")
RS = os.getenv("OSIRIS_PARITY_RS_URL", "").rstrip("/")
KEY = os.getenv("OSIRIS_PARITY_API_KEY", "")
DB = os.getenv("OSIRIS_PARITY_DATABASE_URL", "")

pytestmark = pytest.mark.skipif(not (PY and RS and KEY and DB), reason="OSIRIS_PARITY_* gerekli")

ADMIN = {"X-API-Key": KEY}
U = str(uuid.uuid4())
RUN = uuid.uuid4().hex[:8]

EDGE = [
    ("GET", "/nope", {}, None),
    ("PUT", "/sources", ADMIN, None),
    ("POST", "/search", ADMIN, b"{bozuk"),
    ("POST", "/search", {}, b"{bozuk"),
    ("POST", "/search", ADMIN, None),
    ("POST", "/search", ADMIN, {"query": "   "}),
    ("POST", "/search", ADMIN, {"query": "x", "limit": "20"}),
    ("POST", "/search", ADMIN, {"query": "x", "limit": 5.0}),
    ("POST", "/search", ADMIN, {"query": "x", "limit": 5.5}),
    ("POST", "/search", ADMIN, {"query": "x", "limit": None}),
    ("POST", "/search", ADMIN, {"query": 5}),
    ("POST", "/search", ADMIN, [1, 2]),
    ("POST", "/search", ADMIN, {"query": "ş" * 500}),
    ("POST", "/search", ADMIN, {"query": "ş" * 501}),
    ("POST", "/search/entity", ADMIN, {"entity_type": "email", "value": "   "}),
    ("POST", "/search/entity", ADMIN, {"entity_type": "x" * 33, "value": "a"}),
    ("POST", "/search/entity", ADMIN, {"entity_type": "email", "value": "a%_\\"}),
    ("GET", "/items/recent?limit=abc", ADMIN, None),
    ("GET", "/items/recent?limit=-5&offset=-3", ADMIN, None),
    ("GET", f"/items/{U.upper()}", ADMIN, None),
    ("GET", "/entities/top?entity_type=", ADMIN, None),
    ("GET", "/entities/top?entity_type=email&limit=0", ADMIN, None),
    ("GET", "/entities/top?limit=x", ADMIN, None),
    ("GET", "/sources?enabled=yes", ADMIN, None),
    ("GET", "/sources?enabled=maybe", ADMIN, None),
    ("GET", "/sources?offset=-1", ADMIN, None),
    ("GET", "/saved-queries?alert_enabled=1&limit=1000", ADMIN, None),
    ("POST", "/collect", ADMIN, {"plugin_id": "rss"}),
    ("POST", "/collect", ADMIN, {"plugin_id": "rss", "config": "x"}),
    ("POST", "/collect", ADMIN, {"plugin_id": "rss\n", "config": {}}),
    ("POST", "/collect", ADMIN, {"plugin_id": "-rss", "config": {}}),
    ("POST", "/collect", ADMIN, {"plugin_id": "a" * 65, "config": {}}),
    ("POST", "/collect", ADMIN, {"plugin_id": "rss", "config": {"feed_url": "http://10.0.0.1/"}}),
    ("POST", "/collect", ADMIN, {"plugin_id": "rss", "config": {"feed_url": "file:///etc/passwd"}}),
    ("POST", "/collect", ADMIN, {"plugin_id": "irc", "config": {}}),
    ("POST", "/collect", ADMIN, {"plugin_id": "rss", "config": {"feed_url": "x" * 19980}}),
    ("POST", "/collect", ADMIN, {"plugin_id": "rss", "config": {"feed_url": "x" * 19990}}),
    ("POST", "/collect/batch", ADMIN, {"jobs": [{"plugin_id": "rss", "config": {"feed_url": "y" * 21000}},
                                               {"plugin_id": "web-scraper", "config": {}}]}),
    ("POST", "/collect/batch", ADMIN, {"jobs": "x"}),
    ("POST", "/sources", ADMIN, {"name": "", "plugin_id": "rss"}),
    ("POST", "/sources", ADMIN, {"name": "x", "plugin_id": "rss", "tags": ["a"] * 21}),
    ("POST", "/sources", ADMIN, {"name": "x", "plugin_id": "rss", "tags": [1]}),
    ("POST", "/sources", ADMIN, {"name": "x", "plugin_id": "rss", "priority": "3", "enabled": "no",
                                 "network_type": "x" * 33}),
    ("POST", "/sources", ADMIN, {"name": "x", "plugin_id": "rss", "retention_days": 36501}),
    ("POST", "/sources", ADMIN, {"name": "x", "plugin_id": "rss", "retention_days": None, "network_type": "uzay"}),
    ("POST", "/sources", ADMIN, {"name": "x", "plugin_id": "rss", "url": "u" * 2049}),
    ("POST", "/saved-queries", ADMIN, {"name": "x", "query_text": ""}),
    ("POST", "/saved-queries", ADMIN, {"name": "x" * 201, "query_text": "y"}),
    ("POST", "/saved-queries", ADMIN, {"name": "x", "query_text": "y", "alert_enabled": "maybe"}),
    ("DELETE", f"/saved-queries/{U}", ADMIN, None),
    ("POST", "/alerts/test", ADMIN, {"text": ""}),
    ("POST", "/alerts/test", ADMIN, {"text": "x", "title": "t" * 2001}),
    ("POST", "/reports/markdown", ADMIN, {"title": ""}),
    ("POST", "/reports/markdown", ADMIN, {"title": "x", "findings": ["a"]}),
    ("POST", "/reports/markdown", ADMIN, {"title": "x", "sources": [1]}),
    ("POST", "/reports/markdown", ADMIN, {"title": "x", "findings": [{}] * 501}),
    ("POST", "/auth/token", {}, {}),
    ("POST", "/auth/token", {}, {"api_key": KEY, "role": "x" * 33}),
    ("POST", "/auth/keys", ADMIN, {"username": "", "role": "viewer"}),
    ("POST", "/auth/keys", ADMIN, {"username": "u", "role": "root"}),
    ("DELETE", "/auth/keys/bozuk", ADMIN, None),
    ("GET", "/plugins", {"Authorization": "Bearer  x"}, None),
    ("GET", "/plugins", {"Authorization": "bearer x.y.z"}, None),
    ("GET", "/plugins", {"Authorization": "Basic abc"}, None),
    ("GET", "/plugins", {"X-API-Key": ""}, None),
    ("POST", "/entities/erase", ADMIN, {"entity_type": "email", "value": "x", "delete_items": "hayir"}),
    ("POST", "/entities/erase", ADMIN, {"entity_type": "", "value": "x"}),
    ("POST", "/graph/relation", ADMIN, {"source": "a", "target": ""}),
    ("POST", "/graph/relation", ADMIN, {"source": "a", "target": "b", "relation_type": "r" * 101}),
    ("POST", f"/sources/{U}/collect", ADMIN, None),
    ("POST", "/sources/x/collect", ADMIN, None),
    ("GET", "/health", {}, None),
]


@pytest.fixture(scope="module")
def client():
    with httpx.Client(timeout=60) as c:
        yield c


def _send(client, base, method, path, headers, body):
    kw: dict = {"headers": dict(headers)}
    if isinstance(body, bytes):
        kw["content"] = body
        kw["headers"]["Content-Type"] = "application/json"
    elif body is not None:
        kw["json"] = body
    r = client.request(method, base + path, **kw)
    try:
        data = r.json()
    except ValueError:
        data = None
    detail = data.get("detail") if isinstance(data, dict) else None
    # 422 listelerinin metni pydantic'e özgüdür: yalnızca biçim karşılaştırılır
    return r.status_code, "LIST" if isinstance(detail, list) else detail, type(data).__name__


@pytest.mark.parametrize("method,path,headers,body", EDGE,
                         ids=[f"{i}-{m} {p[:40]}" for i, (m, p, *_) in enumerate(EDGE)])
def test_edge_cases(client, method, path, headers, body) -> None:
    assert _send(client, PY, method, path, headers, body) == _send(client, RS, method, path, headers, body)


@pytest.fixture(scope="module")
def seeded():
    import psycopg

    with psycopg.connect(DB) as c:
        sid = c.execute(
            "INSERT INTO sources (name, url, network_type, plugin_id, schedule, tags, metadata, retention_days)"
            " VALUES (%s, 'https://ex.example/f', 'rss', 'rss', '*/5 * * * *', ARRAY['a','ç'],"
            " '{\"config\":{\"max_items\":3}}', 7) RETURNING id::text", (f"parity-{RUN}",)).fetchone()[0]
        items = []
        for i, (title, body) in enumerate([(f"Parity {RUN} İstanbul", f"parity{RUN} fidye x@parity.example 93.184.216.34"),
                                           (f"Parity {RUN} ikinci", f"parity{RUN} başka"), (None, f"parity{RUN} başlıksız")]):
            items.append(c.execute(
                "INSERT INTO items (source_id, raw_content, cleaned_content, url, title, language, collected_at,"
                " published_at, content_hash, metadata, tags) VALUES (%s, %s, %s, %s, %s, 'tr',"
                " NOW() - make_interval(secs => %s), '2026-01-02T03:04:05.12Z', %s, %s, %s) RETURNING id::text",
                (sid, body, body, f"https://ex.example/{RUN}/{i}", title, i * 10 + 0.5, uuid.uuid4().hex,
                 json.dumps({"k": i, "f": 1.5}), ["etiket"])).fetchone()[0])
        for value, etype in [(f"x{RUN}@parity.example", "email"), ("93.184.216.34", "ip")]:
            eid = c.execute(
                "INSERT INTO entities (type, value, normalized_value, last_seen_at) VALUES (%s, %s, %s, NOW())"
                " ON CONFLICT (type, value) DO UPDATE SET last_seen_at = NOW() RETURNING id::text",
                (etype, value, value.lower())).fetchone()[0]
            c.execute("INSERT INTO item_entities (item_id, entity_id, mention_count) VALUES (%s, %s, 2)"
                      " ON CONFLICT DO NOTHING", (items[0], eid))
        c.execute("INSERT INTO saved_queries (name, query_text, query_type, alert_enabled, last_triggered_at)"
                  " VALUES (%s, %s, 'fts', true, '2026-01-02T03:04:05Z')", (f"parity-{RUN}", f"parity{RUN}"))
        c.commit()
    return {"source": sid, "items": items}


def _bodies(seeded):
    i0, i2 = seeded["items"][0], seeded["items"][2]
    return [
        ("GET", "/stats", None), ("GET", "/items/recent?limit=3", None), ("GET", f"/items/{i0}", None),
        ("GET", f"/items/{i2}", None), ("GET", "/entities/top?entity_type=ip", None),
        ("GET", f"/sources?q=parity-{RUN}", None), ("GET", "/sources?enabled=true&plugin_id=rss&limit=200", None),
        ("GET", "/saved-queries?alert_enabled=true&limit=200", None), ("GET", "/plugins", None),
        ("POST", "/search", {"query": f"parity{RUN}"}), ("POST", "/search", {"query": f"parity{RUN}", "limit": 1}),
        ("POST", "/search/entity", {"entity_type": "email", "value": f"X{RUN}"}),
        ("POST", "/alerts/test", {"text": f"PARITY{RUN} geldi"}),
    ]


def test_bodies(client, seeded) -> None:
    bad = []
    for method, path, body in _bodies(seeded):
        a = client.request(method, PY + path, headers=ADMIN, json=body)
        b = client.request(method, RS + path, headers=ADMIN, json=body)
        if (a.status_code, a.json()) != (b.status_code, b.json()):
            bad.append(f"{method} {path}\n py={a.text[:400]}\n rs={b.text[:400]}")
    assert not bad, "\n".join(bad)


def test_cross_language_jwt(client) -> None:
    for src, dst in ((PY, RS), (RS, PY)):
        tok = client.post(src + "/auth/token", json={"api_key": KEY, "role": "analyst"})
        if tok.status_code == 503:
            pytest.skip("JWT kapalı")
        h = {"Authorization": "Bearer " + tok.json()["access_token"]}
        assert client.get(dst + "/plugins", headers=h).status_code == 200
        assert client.get(dst + "/auth/keys", headers=h).status_code == 403
