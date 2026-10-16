"""CollectorManager testleri — sahte plugin ve mock redis ile."""

from pathlib import Path
from unittest.mock import MagicMock

import pytest
from osiris.plugin import BaseCollector, CollectedItem, CollectionResult
from osiris_collector.manager import CollectorManager


class OkPlugin(BaseCollector):
    id = "ok"
    name = "OK"
    network_type = "www"

    def collect(self, config=None):
        return CollectionResult(
            items=[CollectedItem(raw_content="hello", title="T")], metadata={"n": 1}
        )


class FailPlugin(BaseCollector):
    id = "fail"
    name = "Fail"
    network_type = "www"

    def collect(self, config=None):
        return CollectionResult(items=[], success=False, error="boom")


class CrashPlugin(BaseCollector):
    id = "crash"
    name = "Crash"
    network_type = "www"

    def collect(self, config=None):
        raise RuntimeError("patladi")


def make_manager(**kwargs):
    mgr = CollectorManager.__new__(CollectorManager)
    mgr.plugins_dir = Path("plugins")
    mgr.plugins = {}
    mgr.manifests = {}
    mgr.redis = MagicMock()
    mgr.queue_name = "q"
    mgr.scheduler = MagicMock()
    mgr.database_url = None
    for k, v in kwargs.items():
        setattr(mgr, k, v)
    return mgr


def test_run_collection_success_queues_items() -> None:
    mgr = make_manager()
    mgr.plugins["ok"] = OkPlugin()
    result = mgr.run_collection("ok", {})
    assert result.success and len(result.items) == 1
    assert mgr.redis.xadd.call_count == 1


def test_run_collection_unknown_plugin() -> None:
    mgr = make_manager()
    with pytest.raises(KeyError):
        mgr.run_collection("yok", {})


def test_run_collection_bad_config() -> None:
    mgr = make_manager()
    mgr.plugins["ok"] = OkPlugin()
    result = mgr.run_collection("ok", "degil-dict")  # type: ignore[arg-type]
    assert not result.success


def test_run_collection_failure_and_crash() -> None:
    mgr = make_manager()
    mgr.plugins["fail"] = FailPlugin()
    assert not mgr.run_collection("fail", {}).success
    assert mgr.redis.xadd.call_count == 0
    mgr.plugins["crash"] = CrashPlugin()
    result = mgr.run_collection("crash", {})
    assert not result.success and "plugin hatası" in (result.error or "")


def test_run_collection_oversize_and_serialize_skip() -> None:
    mgr = make_manager()
    mgr.plugins["ok"] = OkPlugin()
    big = OkPlugin()

    def _big_collect(config=None):
        return CollectionResult(items=[CollectedItem(raw_content="x" * 600_000)])

    big.collect = _big_collect  # type: ignore[method-assign]
    mgr.plugins["big"] = big
    result = mgr.run_collection("big", {})
    assert result.success  # öğe atlanır ama görev başarılı
    assert mgr.redis.xadd.call_count == 0


def test_run_collection_redis_error() -> None:
    mgr = make_manager()
    import redis as redis_lib

    mgr.redis.xadd.side_effect = redis_lib.RedisError("down")
    mgr.plugins["ok"] = OkPlugin()
    result = mgr.run_collection("ok", {})
    assert not result.success and "kuyruk" in (result.error or "")


def test_load_plugins_missing_dir_and_bad_manifest(tmp_path: Path) -> None:
    mgr = make_manager(plugins_dir=tmp_path / "yok")
    assert mgr.load_plugins() == 0
    mgr2 = make_manager(plugins_dir=tmp_path)
    (tmp_path / "broken").mkdir()
    (tmp_path / "broken" / "manifest.json").write_text("{bozuk")
    (tmp_path / "noid").mkdir()
    (tmp_path / "noid" / "manifest.json").write_text('{"name": "x"}')
    (tmp_path / "nocol").mkdir()
    (tmp_path / "nocol" / "manifest.json").write_text('{"id": "nocol"}')
    assert mgr2.load_plugins() == 0


def test_load_plugins_real_dir_loads_all() -> None:
    mgr = make_manager()
    # Ortama göre 8-11 arası yüklenir (rss/dns-whois opsiyonel bağımlılıklı);
    # stdlib-only irc her zaman, requests'li username-search genelde yüklenir
    n = mgr.load_plugins()
    assert 8 <= n <= 11
    assert "irc" in mgr.plugins
    assert "username-search" in mgr.plugins


def test_schedule_and_cron_validation() -> None:
    mgr = make_manager()
    mgr.plugins["ok"] = OkPlugin()
    mgr.schedule("ok", "*/15 * * * *", {})
    assert mgr.scheduler.add_job.called
    import pytest

    with pytest.raises(KeyError):
        mgr.schedule("yok-boyle", "*/5 * * * *", {})
    for bad in ["every minute", "* * *", "a b c d e", ""]:
        with pytest.raises(ValueError):
            mgr.schedule("ok", bad, {})


def test_record_health_noop_without_db() -> None:
    mgr = make_manager()
    # database_url yok → sessizce geçer, istisna yok
    mgr._record_health({"source_id": "x"}, CollectionResult(items=[]), 5)
    mgr.database_url = "postgresql://localhost/db"
    # DB yok → hata yutulur
    mgr._record_health({"source_id": "x"}, CollectionResult(items=[]), 5)
    mgr._record_health({}, CollectionResult(items=[]), 5)


def test_start_shutdown_delegate() -> None:
    mgr = make_manager()
    mgr.start()
    mgr.scheduler.start.assert_called_once()
    mgr.shutdown()
    mgr.scheduler.shutdown.assert_called_once()


def test_load_plugins_duplicate_and_exec_error(tmp_path) -> None:
    import textwrap

    mgr = make_manager(plugins_dir=tmp_path)
    d1 = tmp_path / "p1"
    d1.mkdir()
    (d1 / "manifest.json").write_text('{"id": "dup"}')
    (d1 / "collector.py").write_text(textwrap.dedent("""
        from osiris.plugin import BaseCollector, CollectionResult
        class C(BaseCollector):
            id = "dup"
            def collect(self, config=None):
                return CollectionResult(items=[])
    """))
    d2 = tmp_path / "p2"
    d2.mkdir()
    (d2 / "manifest.json").write_text('{"id": "dup"}')
    (d2 / "collector.py").write_text("raise RuntimeError('bozuk plugin')")
    d3 = tmp_path / "p3"
    d3.mkdir()
    (d3 / "manifest.json").write_text('{"id": "noclass"}')
    (d3 / "collector.py").write_text("X = 1")
    assert mgr.load_plugins() == 1
    assert list(mgr.plugins) == ["dup"]


def test_run_collection_unserializable_item() -> None:
    from unittest.mock import MagicMock

    from osiris.plugin import BaseCollector, CollectionResult

    class Weird(BaseCollector):
        id = "weird"
        name = "W"
        network_type = "www"

        def collect(self, config=None):
            item = MagicMock()
            item.model_dump.side_effect = TypeError("serilesmez")
            return CollectionResult(items=[item])

    mgr = make_manager()
    mgr.plugins["weird"] = Weird()
    result = mgr.run_collection("weird", {})
    assert result.success  # öğe atlanır, görev başarılı
    assert mgr.redis.xadd.call_count == 0


def test_record_health_with_fake_db(monkeypatch) -> None:
    import sys
    import types

    executed = []

    class FakeCur:
        def execute(self, q, p=None):
            executed.append(q)

        def __enter__(self):
            return self

        def __exit__(self, *a):
            return False

    class FakeConn:
        def __enter__(self):
            return self

        def __exit__(self, *a):
            return False

        def cursor(self):
            return FakeCur()

        def commit(self):
            pass

    fake = types.ModuleType("psycopg")
    fake.connect = lambda *a, **k: FakeConn()  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, "psycopg", fake)
    mgr = make_manager(database_url="postgresql://localhost/db")
    mgr._record_health({"source_id": "sid"}, CollectionResult(items=[]), 7)
    assert any("UPDATE sources" in q for q in executed)
    assert any("source_metrics" in q for q in executed)


def test_required_fields_enforced_from_manifest(tmp_path) -> None:
    import json as _json

    mgr = make_manager()
    d = tmp_path / "req"
    d.mkdir()
    (d / "manifest.json").write_text(_json.dumps({
        "id": "req",
        "config_schema": {"url": {"type": "string", "required": True},
                          "opt": {"type": "string"}},
    }))
    (d / "collector.py").write_text(
        "from osiris.plugin import BaseCollector, CollectionResult\n"
        "class C(BaseCollector):\n"
        "    id = 'req'\n"
        "    def collect(self, config=None):\n"
        "        return CollectionResult(items=[])\n")
    mgr.plugins_dir = tmp_path
    assert mgr.load_plugins() == 1
    bad = mgr.run_collection("req", {})
    assert not bad.success and "url" in (bad.error or "")
    ok = mgr.run_collection("req", {"url": "https://example.com"})
    assert ok.success


def test_schedule_unknown_plugin_rejected() -> None:
    import pytest

    mgr = make_manager()
    with pytest.raises(KeyError):
        mgr.schedule("yok", "*/5 * * * *", {})


def test_source_id_carried_in_queue_message() -> None:
    import json

    sid = "3F2504E0-4F89-11D3-9A0C-0305E82C3301"
    mgr = make_manager()
    mgr.plugins["ok"] = OkPlugin()
    mgr.run_collection("ok", {"source_id": sid})
    msg = json.loads(mgr.redis.xadd.call_args[0][1]["data"])
    assert msg["source_id"] == sid.lower()


def test_invalid_source_id_dropped_from_queue_message() -> None:
    import json

    mgr = make_manager()
    mgr.plugins["ok"] = OkPlugin()
    mgr.run_collection("ok", {"source_id": "x'; DROP TABLE items;--"})
    assert json.loads(mgr.redis.xadd.call_args[0][1]["data"])["source_id"] is None


def test_enqueue_uses_stream_with_maxlen_cap(monkeypatch) -> None:
    from osiris import queue as q

    monkeypatch.setenv("OSIRIS_STREAM_MAXLEN", "5000")
    mgr = make_manager(queue_name=q.STREAM)
    mgr.plugins["ok"] = OkPlugin()
    mgr.run_collection("ok", {})
    args, kwargs = mgr.redis.xadd.call_args
    assert args[0] == "osiris:stream:raw_items" and set(args[1]) == {"data"}
    assert kwargs == {"maxlen": 5000, "approximate": True}
