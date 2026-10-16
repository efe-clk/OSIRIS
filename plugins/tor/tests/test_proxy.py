"""tor/i2p plugin'leri — operatör proxy'si (OSIRIS_TOR_SOCKS / OSIRIS_I2P_HTTP_PROXY)."""

import importlib.util
from pathlib import Path

import pytest


@pytest.mark.parametrize("plugin,env,expected", [
    ("tor", "OSIRIS_TOR_SOCKS", "socks5h://tor:9050"),
    ("i2p", "OSIRIS_I2P_HTTP_PROXY", "http://i2pd:4444"),
])
def test_darknet_plugins_use_operator_proxy(monkeypatch, plugin, env, expected) -> None:
    """Docker'da proxy ayrı konteynerde: operatör env'i oturum proxy'sini belirler."""
    spec = importlib.util.spec_from_file_location(
        f"{plugin}_collector_proxy_test", Path(__file__).parents[2] / plugin / "collector.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    cls = next(v for v in vars(mod).values() if isinstance(v, type) and v.__module__ == mod.__name__)
    monkeypatch.setenv(env, expected.split("://")[1])
    session = cls()._session({})
    assert session.proxies == {"http": expected, "https": expected}
