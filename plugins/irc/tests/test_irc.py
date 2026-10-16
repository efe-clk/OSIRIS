"""IRC plugin testleri — SSRF koruması (ağ gerektirmez)."""

import importlib.util
from pathlib import Path

import osiris.security as sec
import pytest

_spec = importlib.util.spec_from_file_location(
    "irc_collector_under_test", Path(__file__).parent.parent / "collector.py")
irc = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(irc)


@pytest.fixture
def no_network(monkeypatch):
    """Herhangi bir gerçek soket açılırsa testi düşür."""
    def _forbidden(*args, **kwargs):
        raise AssertionError(f"soket açılmamalıydı: {args!r}")
    monkeypatch.setattr(sec.socket, "create_connection", _forbidden)


@pytest.mark.parametrize("server", [
    "127.0.0.1", "10.0.0.5", "192.168.1.10", "169.254.169.254",
    "::1", "::ffff:127.0.0.1", "localhost", "0.0.0.0",
])
def test_internal_servers_rejected(no_network, server: str) -> None:
    res = irc.IrcCollector().collect({"server": server, "channels": ["#osint"]})
    assert res.success is False
    assert "engellendi" in (res.error or "")


def test_dns_name_resolving_to_internal_rejected(no_network, monkeypatch) -> None:
    monkeypatch.setattr(sec, "_host_ips", lambda h: ["93.184.216.34", "10.0.0.7"])
    res = irc.IrcCollector().collect({"server": "irc.example.com", "channels": ["#osint"]})
    assert res.success is False


def test_health_check_rejects_internal(no_network) -> None:
    assert irc.IrcCollector({"server": "127.0.0.1"}).health_check() is False


def test_connects_to_vetted_ip_not_hostname(monkeypatch) -> None:
    """DNS rebinding: bağlantı çözülüp denetlenen IP'ye yapılmalı."""
    monkeypatch.setattr(sec, "_host_ips", lambda h: ["93.184.216.34"])
    seen = []

    class FakeSock:
        def settimeout(self, _t): pass
        def sendall(self, _b): pass
        def recv(self, _n): return b""
        def close(self): pass

    def _fake_connect(addr, timeout=None):
        seen.append(addr)
        return FakeSock()

    monkeypatch.setattr(sec.socket, "create_connection", _fake_connect)
    res = irc.IrcCollector().collect({"server": "irc.example.com", "channels": ["#osint"]})
    assert res.success is True
    assert seen == [("93.184.216.34", 6667)]

