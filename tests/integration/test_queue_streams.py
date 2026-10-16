"""Redis Streams kuyruğu — gerçek Redis'e karşı (docs/SOZLESME.md §5)."""

import json

from osiris import queue as q


def _pipeline(client, keys, fail_store: int = 0, consumer: str = "c1"):
    from osiris_pipeline.pipeline import ProcessingPipeline

    p = ProcessingPipeline.__new__(ProcessingPipeline)
    p.redis, p.queue_name, p.stream_name = client, keys["legacy"], keys["stream"]
    p.group, p.consumer, p.claim_idle_ms, p._group_ready = q.GROUP, consumer, 300_000, False
    p._nlp, p._embedder, p.embedding_model_name = False, False, None
    p.stored, p.failures = [], fail_store

    def store(result):
        if p.failures:
            p.failures -= 1
            raise RuntimeError("db geçici olarak yok")
        p.stored.append(result["item"]["raw_content"])
    p.store = store
    return p


def _msg(text: str) -> str:
    return json.dumps({"plugin_id": "rss", "source_id": None,
                       "item": {"raw_content": text}, "collected_at": 1.0})


def _pending(client, keys) -> int:
    return client.xpending(keys["stream"], q.GROUP)["pending"]


def test_happy_path_acks_and_deletes(redis_keys) -> None:
    client, keys, _ = redis_keys
    for t in ("bir", "iki", "üç"):
        q.enqueue(client, _msg(t), keys["stream"])
    p = _pipeline(client, keys)
    assert p.run(batch_size=10) == 3
    assert p.stored == ["bir", "iki", "üç"]
    assert client.xlen(keys["stream"]) == 0 and _pending(client, keys) == 0


def test_store_failure_stays_pending_then_reclaimed(redis_keys) -> None:
    client, keys, _ = redis_keys
    q.enqueue(client, _msg("tekrar"), keys["stream"])
    p = _pipeline(client, keys, fail_store=1)
    assert p.run() == 0
    assert _pending(client, keys) == 1 and client.xlen(keys["stream"]) == 1
    p.claim_idle_ms = 0  # zaman aşımını beklemeden geri al
    assert p.run() == 1 and p.stored == ["tekrar"]
    assert client.xlen(keys["stream"]) == 0 and _pending(client, keys) == 0


def test_poison_message_goes_to_dead_letter(redis_keys) -> None:
    client, keys, dead_before = redis_keys
    q.enqueue(client, "{bozuk json", keys["stream"])
    assert _pipeline(client, keys).run() == 0
    assert client.xlen(keys["stream"]) == 0 and _pending(client, keys) == 0
    last = client.xrevrange(q.DEAD_LETTER, count=1)[0][1]
    assert client.xlen(q.DEAD_LETTER) == dead_before + 1
    assert last["data"] == "{bozuk json" and last["reason"].startswith("invalid")


def test_max_deliveries_then_dead_letter(redis_keys) -> None:
    client, keys, dead_before = redis_keys
    q.enqueue(client, _msg("hep başarısız"), keys["stream"])
    p = _pipeline(client, keys, fail_store=1000)
    p.claim_idle_ms = 0
    for _ in range(q.MAX_DELIVERIES + 2):
        p.run()
    assert client.xlen(keys["stream"]) == 0 and _pending(client, keys) == 0
    assert client.xrevrange(q.DEAD_LETTER, count=1)[0][1]["reason"] == "max_deliveries"
    assert client.xlen(q.DEAD_LETTER) == dead_before + 1


def test_legacy_list_drained_first(redis_keys) -> None:
    client, keys, _ = redis_keys
    client.rpush(keys["legacy"], _msg("eski-1"), _msg("eski-2"))
    q.enqueue(client, _msg("yeni"), keys["stream"])
    p = _pipeline(client, keys, fail_store=1)  # eski-1 depolanamaz → akışa taşınır
    # Aynı turda: eski-2 (LIST), sonra akıştan yeni + akışa taşınmış eski-1 → kayıp yok
    assert p.run(batch_size=10) == 3
    assert client.llen(keys["legacy"]) == 0
    assert p.stored[0] == "eski-2" and sorted(p.stored) == ["eski-1", "eski-2", "yeni"]
    assert client.xlen(keys["stream"]) == 0 and _pending(client, keys) == 0


def test_two_consumers_never_double_process(redis_keys) -> None:
    client, keys, _ = redis_keys
    for i in range(20):
        q.enqueue(client, _msg(f"m{i}"), keys["stream"])
    a = _pipeline(client, keys, consumer="a")
    b = _pipeline(client, keys, consumer="b")
    while a.run(batch_size=3) + b.run(batch_size=3):
        pass
    assert sorted(a.stored + b.stored) == sorted(f"m{i}" for i in range(20))
    assert not set(a.stored) & set(b.stored)
    assert a.stored and b.stored


def test_collector_message_roundtrip(redis_keys) -> None:

    from osiris.plugin import BaseCollector, CollectedItem, CollectionResult
    from osiris_collector.manager import CollectorManager

    client, keys, _ = redis_keys

    class P(BaseCollector):
        def collect(self, config=None):
            return CollectionResult(items=[CollectedItem(raw_content="<b>uçtan</b> uca")])

    mgr = CollectorManager.__new__(CollectorManager)
    mgr.plugins, mgr.manifests, mgr.redis = {"p": P()}, {}, client
    mgr.queue_name, mgr.database_url = keys["stream"], None
    mgr.run_collection("p", {"source_id": "3f2504e0-4f89-11d3-9a0c-0305e82c3301"})
    p = _pipeline(client, keys)
    captured = []
    p.store = lambda r: captured.append(r)
    assert p.run() == 1
    assert captured[0]["item"]["cleaned_content"] == "uçtan uca"
    assert captured[0]["item"]["source_id"] == "3f2504e0-4f89-11d3-9a0c-0305e82c3301"
