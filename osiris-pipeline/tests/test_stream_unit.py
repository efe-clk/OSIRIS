"""Stream tüketici dalları — sahte Redis (gerçek Redis senaryoları tests/integration'da)."""

import json
from unittest.mock import MagicMock

import redis
from osiris import queue as q
from osiris_pipeline.pipeline import ProcessingPipeline


def _p():
    p = ProcessingPipeline.__new__(ProcessingPipeline)
    p.redis, p.queue_name, p.stream_name = MagicMock(), "", "s"
    p.group, p.consumer, p.claim_idle_ms, p._group_ready = "g", "c", 1000, True
    p._nlp, p._embedder, p.embedding_model_name = False, False, None
    p.store = MagicMock()
    return p


GOOD = {"data": json.dumps({"plugin_id": "rss", "item": {"raw_content": "x"}})}


def test_reclaimed_branches() -> None:
    p = _p()
    p.redis.xautoclaim.return_value = ["0-0", [("1-0", None), ("2-0", GOOD), ("3-0", GOOD)], []]
    p.redis.xpending_range.side_effect = [[{"times_delivered": 9}], [{"times_delivered": 1}]]
    p.redis.xreadgroup.return_value = []
    assert p.run(batch_size=5) == 1
    p.redis.xack.assert_any_call("s", "g", "1-0")  # silinmiş girdi yalnızca onaylanır
    dead = p.redis.xadd.call_args
    assert dead[0][0] == q.DEAD_LETTER and dead[0][1]["reason"] == "max_deliveries"


def test_new_entries_success_invalid_and_store_failure() -> None:
    p = _p()
    p.redis.xautoclaim.return_value = ["0-0", [], []]
    p.redis.xreadgroup.return_value = [["s", [("1-0", GOOD), ("2-0", {"data": "{bozuk"}), ("3-0", GOOD)]]]
    p.store.side_effect = [None, RuntimeError("db")]
    assert p.run(batch_size=5) == 1
    acked = [c[0][2] for c in p.redis.xack.call_args_list]
    assert acked == ["1-0", "2-0"]  # 3-0 depolanamadı → bekleyen listesinde kalır
    assert p.redis.xadd.call_args[0][1]["reason"].startswith("invalid")


def test_redis_errors_and_group_creation() -> None:
    p = _p()
    p._group_ready = False
    p.redis.xgroup_create.side_effect = redis.ResponseError("BUSYGROUP Consumer Group name already exists")
    p.redis.xautoclaim.side_effect = redis.RedisError("down")
    assert p.run() == 0 and p._group_ready is True
    p.redis.xpending_range.side_effect = redis.RedisError("x")
    assert p._deliveries("1-0") == 0


def test_ensure_group_reraises_other_errors() -> None:
    import pytest

    client = MagicMock()
    client.xgroup_create.side_effect = redis.ResponseError("WRONGTYPE")
    with pytest.raises(redis.ResponseError):
        q.ensure_group(client)


def test_stream_maxlen_env(monkeypatch) -> None:
    monkeypatch.setenv("OSIRIS_STREAM_MAXLEN", "abc")
    assert q.stream_maxlen() == q.DEFAULT_MAXLEN
    monkeypatch.setenv("OSIRIS_STREAM_MAXLEN", "5")
    assert q.stream_maxlen() == 1000


def test_legacy_drain_redis_error() -> None:
    p = _p()
    p.queue_name = "legacy"
    p.redis.lpop.side_effect = redis.RedisError("down")
    assert p._drain_legacy(5) == (0, 0)


def test_reembed_requires_model() -> None:
    import pytest

    p = _p()
    p.database_url = None
    with pytest.raises(RuntimeError):
        p.reembed()
