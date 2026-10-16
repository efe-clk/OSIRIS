"""fastembed + pgvector (migration 005) — gerçek model ve gerçek PostgreSQL."""

import json
import os
import sys
from pathlib import Path

import psycopg
import pytest

sys.path.insert(0, str(Path(__file__).parents[1] / "golden"))
import harness  # noqa: E402

pytestmark = pytest.mark.skipif(not harness.embedding_available(),
                                reason="embedding modeli yok (python models/download.py)")


@pytest.fixture
def pipeline(db_url, monkeypatch):
    from osiris_pipeline.pipeline import DEFAULT_EMBEDDING_MODEL, ProcessingPipeline

    monkeypatch.setenv("OSIRIS_MODEL_DIR", os.getenv("OSIRIS_MODEL_DIR", str(harness.ROOT / "models")))
    p = ProcessingPipeline.__new__(ProcessingPipeline)
    p.database_url, p._nlp, p._embedder = db_url, False, None
    p.embedding_model_name, p._embedding_usable = DEFAULT_EMBEDDING_MODEL, None
    return p


def _store(p, text):
    return p.store(p.process_one(json.dumps({"plugin_id": "rss", "item": {"raw_content": text}})))


def test_store_writes_embedding_v2_and_semantic_search(pipeline, db_url) -> None:
    from osiris_query.engine import QueryEngine

    ids = {t: _store(pipeline, t) for t in (
        "Ransomware gang encrypts hospital servers and demands bitcoin",
        "Yerel futbol takımı şampiyonluğu kutladı",
        "Neues Kochrezept für Apfelkuchen",
    )}
    with psycopg.connect(db_url) as conn:
        rows = conn.execute("SELECT embedding_model, vector_dims(embedding_v2), embedding IS NULL"
                            " FROM items").fetchall()
    assert all(r == (pipeline.embedding_model_name, 384, True) for r in rows)

    q = pipeline.generate_embedding("hastanelere fidye yazılımı saldırısı")
    hits = QueryEngine(db_url).semantic_search(q, limit=3)
    assert hits[0]["id"] == ids["Ransomware gang encrypts hospital servers and demands bitcoin"]
    assert hits[0]["similarity"] > hits[1]["similarity"]


def test_reembed_backfills_old_items(pipeline, db_url) -> None:
    with psycopg.connect(db_url) as conn:
        for i, t in enumerate(("eski öğe sızıntı verisi", "", "başka eski öğe")):
            conn.execute("INSERT INTO items (raw_content, cleaned_content, content_hash)"
                         " VALUES (%s, %s, %s)", (t, t, f"{i:064d}"))
        conn.commit()
    assert pipeline.reembed(batch=2) == 3
    with psycopg.connect(db_url) as conn:
        got = dict(conn.execute("SELECT cleaned_content, embedding_model FROM items").fetchall())
        filled = conn.execute("SELECT count(*) FROM items WHERE embedding_v2 IS NOT NULL").fetchone()[0]
    assert filled == 2
    assert got[""].endswith("#skip")  # boş metin bir daha seçilmez
    assert pipeline.reembed() == 0
