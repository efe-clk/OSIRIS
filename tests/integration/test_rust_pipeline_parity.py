"""R5: Python ↔ Rust pipeline işçisi — aynı mesajlar, iki ayrı veritabanı, birebir sonuç.

Gereken: OSIRIS_TEST_DATABASE_URL (Python), OSIRIS_TEST_RUST_DATABASE_URL (Rust, adı `_test`
ile biter), OSIRIS_TEST_REDIS_URL, OSIRIS_SERVER_BIN (cargo build --release -p osiris-server).
Modeller (OSIRIS_MODEL_DIR) varsa NER ve embedding de karşılaştırılır.

Karşılaştırılan: öğeler (content_hash anahtarıyla) — raw/cleaned_content, url, title,
published_at, metadata, source_id, etiket kümesi; varlık kümeleri; embedding kosinüs ≥ 0.999;
ölü-mektup sayısı. `language` sözleşme dışıdır (langdetect ↔ lingua) — yalnızca raporlanır.
Ardından `osiris-server shadow` Python'un yazdığı veritabanında temiz çıkmalıdır.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
BIN = os.getenv("OSIRIS_SERVER_BIN", "")
RUST_DB = os.getenv("OSIRIS_TEST_RUST_DATABASE_URL", "")
_TABLES = "item_entities, entity_relations, entities, items, source_metrics, sources"


def _corpus() -> list[str]:
    """Plugin fixture öğeleri + sentetik HTML + tekrar + bozuk mesajlar."""
    msgs: list[str] = []
    for path in sorted((ROOT / "fixtures/plugins").glob("*.json")):
        case = json.loads(path.read_text(encoding="utf-8"))
        for item in case["expected"]["items"]:
            msgs.append(json.dumps({"plugin_id": case["plugin"], "source_id": None, "item": item,
                                    "collected_at": 1.0}, ensure_ascii=False))
    html = [
        "<html><body><h1>Fidye saldırısı</h1><p>soc@ornek.com.tr 8.8.8.8 CVE-2024-3094</p>"
        "<script>x()</script></body></html>",
        "Recep Tayyip Erdoğan Ankara'da NATO yetkilileriyle görüştü. bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq",
        "Ransomware attack hits hospital; leak of 2 GB dump at evil.example.org (+90 532 123 45 67)",
        "<p>Datenleck bei Telekom in Berlin</p>&nbsp;&amp;&lt;b&gt;",
        "",
        "   ",
    ]
    for i, text in enumerate(html):
        item = {"url": f"https://x.example/{i}", "title": f"Başlık {i}", "raw_content": text,
                "published_at": ["Mon, 05 Jan 2026 10:00:00 GMT", "2026-01-01T12:00:00Z", None][i % 3],
                "metadata": {"i": i, "k": "ş"}, "tags": ["a", i, True, None]}
        msgs.append(json.dumps({"plugin_id": "web-scraper", "item": item, "collected_at": 2.0},
                               ensure_ascii=False))
    msgs.append(msgs[-3])  # tekrar → tekilleştirme
    msgs += ["{bozuk json", json.dumps([1, 2]), json.dumps({"item": {}}),
             json.dumps({"plugin_id": "rss", "item": "x"})]
    return msgs


def _drain(run, client, stream: str) -> None:
    for _ in range(50):
        run()
        if client.xlen(stream) == 0:
            return
    raise AssertionError("akış boşalmadı")


def _snapshot(url: str) -> dict:
    import psycopg

    with psycopg.connect(url) as conn:
        rows = conn.execute(
            "SELECT content_hash, raw_content, cleaned_content, url, title, published_at, metadata,"
            " source_id, tags, language, embedding_v2::text FROM items").fetchall()
        ents = conn.execute(
            "SELECT i.content_hash, e.type::text, e.value FROM item_entities ie"
            " JOIN items i ON i.id = ie.item_id JOIN entities e ON e.id = ie.entity_id").fetchall()
    out = {}
    for r in rows:
        out[r[0]] = {"raw": r[1], "cleaned": r[2], "url": r[3], "title": r[4],
                     "published_at": r[5], "metadata": r[6], "source_id": r[7],
                     "tags": sorted(r[8] or []), "language": r[9],
                     "embedding": json.loads(r[10]) if r[10] else None, "entities": set()}
    for h, t, v in ents:
        out[h]["entities"].add((t, v))
    return out


def _cos(a, b) -> float:
    dot = sum(x * y for x, y in zip(a, b, strict=True))
    na = sum(x * x for x in a) ** 0.5
    nb = sum(y * y for y in b) ** 0.5
    return dot / (na * nb)


def test_python_and_rust_workers_store_the_same(db_url, redis_keys):
    if not BIN or not Path(BIN).is_file():
        pytest.skip("OSIRIS_SERVER_BIN tanımlı değil")
    if not RUST_DB:
        pytest.skip("OSIRIS_TEST_RUST_DATABASE_URL tanımlı değil")
    import psycopg

    with psycopg.connect(RUST_DB) as conn:
        assert conn.execute("SELECT current_database()").fetchone()[0].endswith("_test")
        conn.execute(f"TRUNCATE {_TABLES} RESTART IDENTITY CASCADE")
        conn.commit()
    client, keys, _ = redis_keys
    sys.path.insert(0, str(ROOT / "osiris-pipeline"))
    from osiris import queue as q
    from osiris_pipeline.pipeline import ProcessingPipeline

    corpus = _corpus()
    dead = client.xlen(q.DEAD_LETTER) if client.exists(q.DEAD_LETTER) else 0

    # --- Python
    for m in corpus:
        q.enqueue(client, m, keys["stream"])
    redis_url = os.environ["OSIRIS_TEST_REDIS_URL"]
    py = ProcessingPipeline(redis_url=redis_url, queue_name=keys["legacy"], database_url=db_url,
                            embedding_model=os.getenv("OSIRIS_EMBEDDING_MODEL"),
                            stream_name=keys["stream"], consumer="py-parity")
    _drain(lambda: py.run(batch_size=100), client, keys["stream"])
    dead_py = client.xlen(q.DEAD_LETTER) - dead

    # --- Rust (aynı mesajlar, ayrı DB)
    for m in corpus:
        q.enqueue(client, m, keys["stream"])
    env = {**os.environ, "OSIRIS_DATABASE_URL": RUST_DB, "OSIRIS_REDIS_URL": redis_url,
           "OSIRIS_PIPELINE_CONSUMER": "rust-parity", "OSIRIS_RETENTION_INTERVAL": "0"}

    def rust_once():
        subprocess.run([BIN, "pipeline", "--once", "--batch", "100", "--stream", keys["stream"],
                        "--queue", keys["legacy"]], env=env, check=True, capture_output=True, timeout=600)

    _drain(rust_once, client, keys["stream"])
    dead_rust = client.xlen(q.DEAD_LETTER) - dead - dead_py

    assert dead_py == dead_rust == 4
    a, b = _snapshot(db_url), _snapshot(RUST_DB)
    assert set(a) == set(b), "content_hash kümeleri farklı"
    lang_agree = 0
    for h in a:
        pa, rb = a[h], b[h]
        for k in ("raw", "cleaned", "url", "title", "published_at", "metadata", "source_id",
                  "tags", "entities"):
            assert pa[k] == rb[k], f"{h[:12]} {k}: {pa[k]!r} != {rb[k]!r}"
        assert (pa["embedding"] is None) == (rb["embedding"] is None), f"{h[:12]} embedding varlığı"
        if pa["embedding"]:
            assert _cos(pa["embedding"], rb["embedding"]) >= 0.999, f"{h[:12]} embedding"
        lang_agree += pa["language"] == rb["language"]
    print(f"\n{len(a)} öğe birebir; dil uyumu {lang_agree}/{len(a)} (sözleşme dışı)")

    # --- gölge: Rust, Python'un yazdığı DB'de yeniden hesaplar
    shadow = subprocess.run([BIN, "shadow", "--minutes", "60", "--limit", "1000"],
                            env={**env, "OSIRIS_DATABASE_URL": db_url}, capture_output=True,
                            text=True, timeout=600)
    report = json.loads(shadow.stdout)
    assert shadow.returncode == 0 and report["ok"], report
    assert report["checked"] == len(a)
