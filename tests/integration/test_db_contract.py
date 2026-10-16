"""Sözleşme maddelerinin gerçek PostgreSQL üzerinde doğrulanması (jsonb, FK, SQL)."""

import json
import uuid

import psycopg
import pytest
from osiris.audit import append_audit, canonical_payload, chain_hash, verify_chain
from osiris.retention import purge_expired


def test_audit_v2_survives_jsonb_roundtrip(db_url) -> None:
    with psycopg.connect(db_url) as conn:
        details = [
            {"ratio": 1.0, "big": 1e16, "tiny": 1e-7, "neg0": -0.0},
            {"n": 2**60, "nested": {"b": [1, 2.5, None, True], "a": "ş"}},
            None,
            {"q": "x" * 50, "emoji": "🔒"},
        ]
        for i, d in enumerate(details):
            out = append_audit(conn, f"u{i}", "test.action", f"r{i}", d)
            assert out["chain_version"] == 2
        res = verify_chain(conn)
        assert res == {"ok": True, "checked": 4, "truncated": False}


def test_audit_mixed_v1_rows_then_v2(db_url) -> None:
    """Migration öncesi yazılmış v1 kayıtlar + sonrası v2: zincir kesintisiz doğrulanır."""
    with psycopg.connect(db_url) as conn:
        p1 = canonical_payload("api", "search", "", {"f": 1.0}, 1)
        h1 = chain_hash(None, p1)
        conn.execute(
            "INSERT INTO audit_logs (user_id, action, resource, detail, prev_hash, hash, chain_version)"
            " VALUES ('api', 'search', '', %s::jsonb, NULL, %s, 1)",
            (json.dumps({"f": 1.0}), h1),
        )
        conn.commit()
        append_audit(conn, "api", "search", None, {"f": 1.0})
        assert verify_chain(conn)["ok"] is True
        # Kurcalama: v1 kaydın detail'i değişirse zincir kırılır
        conn.execute("UPDATE audit_logs SET detail = '{\"f\": 2}'::jsonb WHERE id = 1")
        conn.commit()
        assert verify_chain(conn)["ok"] is False


def _source(cur, name: str, retention_days) -> str:
    cur.execute(
        "INSERT INTO sources (name, network_type, plugin_id, retention_days)"
        " VALUES (%s, 'rss', 'rss', %s) RETURNING id::text",
        (name, retention_days),
    )
    return cur.fetchone()[0]


def _item(cur, source_id, age_days: int, text: str) -> str:
    cur.execute(
        "INSERT INTO items (source_id, raw_content, cleaned_content, content_hash, collected_at)"
        " VALUES (%s::uuid, %s, %s, %s, NOW() - make_interval(days => %s)) RETURNING id::text",
        (source_id, text, text, uuid.uuid4().hex * 2, age_days),
    )
    return cur.fetchone()[0]


def test_retention_rules(db_url) -> None:
    with psycopg.connect(db_url) as conn:
        with conn.cursor() as cur:
            keep_forever = _source(cur, "sonsuz", 0)
            short = _source(cur, "kisa", 10)
            default = _source(cur, "varsayilan", None)
            items = {
                "forever_old": _item(cur, keep_forever, 999, "a"),
                "short_old": _item(cur, short, 11, "b"),
                "short_new": _item(cur, short, 9, "c"),
                "default_old": _item(cur, default, 31, "d"),
                "default_new": _item(cur, default, 29, "e"),
                "orphan_src_old": _item(cur, None, 31, "f"),
            }
            cur.execute("INSERT INTO entities (type, value, normalized_value) VALUES"
                        " ('email', 'gider@x.com', 'gider@x.com'),"
                        " ('email', 'kalir@x.com', 'kalir@x.com') RETURNING id::text")
            gone_ent, kept_ent = [r[0] for r in cur.fetchall()]
            cur.execute("INSERT INTO item_entities (item_id, entity_id) VALUES"
                        " (%s::uuid, %s::uuid), (%s::uuid, %s::uuid)",
                        (items["short_old"], gone_ent, items["short_new"], kept_ent))
        conn.commit()

        res = purge_expired(conn, default_days=30, batch=2)  # partileme de denenir
        assert res["items_deleted"] == 3
        assert res["entities_deleted"] == 1
        left = {r[0] for r in conn.execute("SELECT id::text FROM items").fetchall()}
        assert left == {items["forever_old"], items["short_new"], items["default_new"]}
        ents = {r[0] for r in conn.execute("SELECT value FROM entities").fetchall()}
        assert ents == {"kalir@x.com"}

        # Varsayılan 0 → yalnızca kaynağa özel pozitif süreler uygulanır
        assert purge_expired(conn, default_days=0)["items_deleted"] == 0


def test_pipeline_store_links_source_and_tolerates_deleted_source(db_url) -> None:
    from osiris_pipeline.pipeline import ProcessingPipeline

    with psycopg.connect(db_url) as conn, conn.cursor() as cur:
        sid = _source(cur, "kaynak", None)
        conn.commit()
    p = ProcessingPipeline.__new__(ProcessingPipeline)
    p.database_url, p._nlp, p._embedder, p.embedding_model_name = db_url, False, False, None
    p._embedding_usable = None

    def msg(text, source_id):
        return json.dumps({"plugin_id": "rss", "source_id": source_id,
                           "item": {"raw_content": text}})

    item_id = p.store(p.process_one(msg("ilk öğe a@b.com", sid)))
    gone = str(uuid.uuid4())  # hiç var olmamış / silinmiş kaynak
    item2 = p.store(p.process_one(msg("ikinci öğe", gone)))
    with psycopg.connect(db_url) as conn:
        rows = dict(conn.execute("SELECT id::text, source_id::text FROM items").fetchall())
    assert rows[item_id] == sid
    assert rows[item2] is None


@pytest.mark.parametrize("bad", [-1, 36501])
def test_retention_days_check_constraint(db_url, bad) -> None:
    with psycopg.connect(db_url) as conn, conn.cursor() as cur:
        with pytest.raises(psycopg.errors.CheckViolation):
            _source(cur, "x", bad)
