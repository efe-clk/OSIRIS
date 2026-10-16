"""Gerçek PostgreSQL'e karşı entegrasyon testleri.

Çalıştırmak için: OSIRIS_TEST_DATABASE_URL=postgresql://.../osiris_test pytest tests/integration
Güvenlik: tablolar TRUNCATE edildiği için veritabanı adı `_test` ile bitmelidir.
Değişken yoksa testler atlanır (birim test koşusunu etkilemez).
"""

import os

import pytest

DB_URL = os.getenv("OSIRIS_TEST_DATABASE_URL", "")

_TABLES = ("audit_logs", "item_entities", "entity_relations", "entities", "items",
           "source_metrics", "sources", "graph_edges", "saved_queries", "api_keys", "users")


@pytest.fixture
def db_url() -> str:
    if not DB_URL:
        pytest.skip("OSIRIS_TEST_DATABASE_URL tanımlı değil")
    import psycopg

    with psycopg.connect(DB_URL) as conn:
        name = conn.execute("SELECT current_database()").fetchone()[0]
        if not name.endswith("_test"):
            pytest.fail(f"Güvenlik: test DB adı '_test' ile bitmeli (gelen: {name})")
        conn.execute(f"TRUNCATE {', '.join(_TABLES)} RESTART IDENTITY CASCADE")
        conn.commit()
    return DB_URL


REDIS_URL = os.getenv("OSIRIS_TEST_REDIS_URL", "")


@pytest.fixture
def redis_keys():
    """Test başına benzersiz akış/liste anahtarları; yalnızca bunlar silinir (FLUSHDB yok)."""
    if not REDIS_URL:
        pytest.skip("OSIRIS_TEST_REDIS_URL tanımlı değil")
    import uuid

    import redis

    client = redis.Redis.from_url(REDIS_URL, decode_responses=True)
    tag = uuid.uuid4().hex[:8]
    keys = {"stream": f"test:{tag}:stream", "legacy": f"test:{tag}:list",
            "dead": "osiris:stream:raw_items:dead"}
    dead_before = client.xlen(keys["dead"]) if client.exists(keys["dead"]) else 0
    yield client, keys, dead_before
    client.delete(keys["stream"], keys["legacy"])
