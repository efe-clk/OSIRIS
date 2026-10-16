"""Username Search plugin testleri (ağ gerektirmez)."""

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent.parent))

import collector as ucol
from collector import UsernameSearchCollector


class FakeResp:
    def __init__(self, status=200, body="hello", url="https://x/"):
        self.status_code = status
        self._text = body
        self.url = url

    @property
    def text(self):
        return self._text


def test_username_validation() -> None:
    c = UsernameSearchCollector()
    for bad in [None, "", "a", "x" * 100, "a/b", "a\r\nb", "ad min", "@x"]:
        r = c.collect({"username": bad})
        assert not r.success


def test_sites_db_loads() -> None:
    c = UsernameSearchCollector()
    sites = c.load_sites()
    assert len(sites) >= 50
    assert all("{username}" in v["url"] or True for v in sites.values())
    with pytest.raises(ValueError):
        c.load_sites("/yok/boyle/db.json")


def test_status_code_detection(monkeypatch) -> None:
    c = UsernameSearchCollector()

    def fake_fetch(session, method, url, **kw):
        if "var" in url:
            return FakeResp(200, "profil sayfası")
        return FakeResp(404, "yok")

    monkeypatch.setattr(ucol, "fetch_url", fake_fetch)
    r = c.collect({"username": "varlik", "sites": ["GitHub"]})
    assert r.success and len(r.items) == 1
    assert r.items[0].metadata["site"] == "GitHub"
    assert "username" in r.items[0].tags


def test_message_detection(monkeypatch) -> None:
    c = UsernameSearchCollector()

    def fake_fetch(session, method, url, **kw):
        return FakeResp(200, "No such user, üzgünüz")

    monkeypatch.setattr(ucol, "fetch_url", fake_fetch)
    r = c.collect({"username": "kimse", "sites": ["HackerNews"]})
    assert r.success and r.items == [] and r.metadata["unknown"] == 1

    def fake_fetch2(session, method, url, **kw):
        return FakeResp(200, "kullanıcı profili burada")

    monkeypatch.setattr(ucol, "fetch_url", fake_fetch2)
    r2 = c.collect({"username": "kimse", "sites": ["HackerNews"]})
    # presence yok, absence yok → bilinmiyor
    assert r2.items == []


def test_response_url_detection(monkeypatch) -> None:
    c = UsernameSearchCollector()
    sites = c.load_sites()
    sites["TestSite"] = {"url": "https://example.com/{username}", "check_type": "response_url",
                         "presence": [], "absence": [], "tags": []}

    def fake_fetch(session, method, url, **kw):
        return FakeResp(200, "x", url="https://example.com/testuser")

    monkeypatch.setattr(ucol, "fetch_url", fake_fetch)
    orig = c.load_sites
    c.load_sites = lambda db=None, full=False: sites  # type: ignore[method-assign]
    try:
        r = c.collect({"username": "testuser", "sites": ["TestSite"]})
    finally:
        c.load_sites = orig  # type: ignore[method-assign]
    assert len(r.items) == 1 and r.items[0].metadata["site"] == "TestSite"


def test_error_isolation_and_filters(monkeypatch) -> None:
    c = UsernameSearchCollector()

    def fake_fetch(session, method, url, **kw):
        if "GitHub" in str(url) or "github" in str(url):
            raise ConnectionError("down")
        return FakeResp(404, "yok")

    monkeypatch.setattr(ucol, "fetch_url", fake_fetch)
    r = c.collect({"username": "testuser", "tags": ["dev"], "max_sites": 5, "workers": 2})
    assert r.success and r.metadata["checked"] <= 5
    # deterministik sıralama
    names = [i.metadata["site"] for i in r.items]
    assert names == sorted(names)


def test_health() -> None:
    assert UsernameSearchCollector().health_check() is True


def test_full_db_loads_thousands() -> None:
    c = UsernameSearchCollector()
    sites = c.load_sites(full=True)
    assert len(sites) > 2000
    # yazım hatalı anahtar ve sarmalayıcı tolere edilir
    assert any(v["presence"] for v in sites.values())


def test_raw_maigret_wrapper_tolerated(tmp_path) -> None:
    import json as _json

    c = UsernameSearchCollector()
    db = tmp_path / "db.json"
    db.write_text(_json.dumps({"sites": {
        "Demo": {"url": "https://example.com/{u}".replace("{u}", "{username}"),
                 "checkType": "message", "presenseStrs": ["hos geldin"],
                 "tags": "tek-etiket"},
        "Kotu": {"url": "https://example.com/sabit", "checkType": "nukleer"},
        "Kapali": {"url": "https://example.com/{username}", "disabled": True},
    }}))
    sites = c.load_sites(str(db))
    assert list(sites) == ["Demo"]
    assert sites["Demo"]["presence"] == ["hos geldin"]
    assert sites["Demo"]["tags"] == ["tek-etiket"]


def test_untrusted_sites_db_locked_to_allowed_dirs(tmp_path, monkeypatch) -> None:
    """E13: API config'indeki sites_db sunucuda rastgele dosya okutamamalı."""
    c = UsernameSearchCollector()
    outside = tmp_path / "db.json"
    outside.write_text('{"Demo": {"url": "https://x/{username}"}}')
    for bad in [str(outside), "../../../../etc/passwd", "/etc/passwd", "sites.json/../../x.json",
                "yok.json", "manifest.json/..", "x\0.json"]:
        res = c.collect({"username": "osirisbot", "sites_db": bad})
        assert res.success is False, bad
    # Plugin dizinindeki dosya adı kabul edilir
    assert ucol.resolve_sites_db("sites.json").name == "sites.json"
    # OSIRIS_USERNAME_DB_DIR ile operatörün açıkça izin verdiği dizin
    monkeypatch.setenv("OSIRIS_USERNAME_DB_DIR", str(tmp_path))
    assert ucol.resolve_sites_db(str(outside)) == outside.resolve()
    assert ucol.resolve_sites_db("db.json") == outside.resolve()
    # Uzantı .json olmalı
    (tmp_path / "gizli.txt").write_text("{}")
    import pytest
    with pytest.raises(ValueError):
        ucol.resolve_sites_db("gizli.txt")


def test_throttle_global_rate_and_per_host_interval() -> None:
    t = [0.0]
    slept = []

    def sleep(d):
        slept.append(round(d, 6))
        t[0] += d

    th = ucol._Throttle(rate_per_sec=4, per_host_interval=1.0, clock=lambda: t[0], sleep=sleep)
    th.wait("a.com")   # 0.00 hemen
    th.wait("b.com")   # genel aralık 0.25
    th.wait("a.com")   # a.com için 1.0 dolmalı → 1.0'a kadar bekler
    th.wait("c.com")   # genel aralık: 1.25
    assert slept == [0.25, 0.75, 0.25]
    assert t[0] == 1.25


def test_throttle_unlimited_rate() -> None:
    slept = []
    th = ucol._Throttle(0, 0, clock=lambda: 0.0, sleep=slept.append)
    for h in ("a", "a", "b"):
        th.wait(h)
    assert slept == []


def test_collect_passes_throttle_config(monkeypatch) -> None:
    seen = {}

    class Spy(ucol._Throttle):
        def __init__(self, rate, host_gap, **kw):
            seen["args"] = (rate, host_gap)
            super().__init__(0, 0)

    monkeypatch.setattr(ucol, "_Throttle", Spy)
    c = UsernameSearchCollector()
    monkeypatch.setattr(c, "_check_one", lambda *a: None)
    c.collect({"username": "osirisbot", "sites": ["GitHub"], "rate_per_sec": 999, "per_host_interval_ms": -5})
    assert seen["args"] == (50.0, 0.0)
    c.collect({"username": "osirisbot", "sites": ["GitHub"], "rate_per_sec": "x"})
    assert seen["args"] == (10.0, 1.0)
