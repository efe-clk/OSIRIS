"""security.py testleri — SSRF ve enjeksiyon korumaları."""

import pytest
from osiris.security import (
    assert_safe_url,
    cap_list,
    is_blocked_host,
    mask_secret,
    resolve_safe_ips,
    safe_create_connection,
    safe_title,
    sanitize_channel,
    sanitize_domain,
    sanitize_hostname,
    sanitize_irc_token,
)


def test_public_url_ok() -> None:
    assert assert_safe_url("https://example.com/a?q=1") == "https://example.com/a?q=1"


@pytest.mark.parametrize("url", [
    "http://169.254.169.254/latest/meta-data",
    "http://localhost:8000/x",
    "http://127.0.0.1/",
    "http://10.0.0.5/",
    "http://192.168.1.1/",
    "http://172.16.0.1/",
    "file:///etc/passwd",
    "ftp://x.com/f",
    "http://user:pass@example.com/",
    "http://abc.onion/",
    "http://example.com:99999/",
    "",
    "x" * 3000,
])
def test_blocked_urls(url: str) -> None:
    with pytest.raises(ValueError):
        assert_safe_url(url)


def test_onion_allowed_for_tor() -> None:
    assert assert_safe_url("http://abc.onion/", allow_onion=True)


def test_is_blocked_host() -> None:
    assert is_blocked_host("localhost")
    assert is_blocked_host("")
    assert is_blocked_host("10.1.2.3")
    # .invalid TLD asla çözülmez → fail-closed
    assert is_blocked_host("yok-boyle-host.invalid")


@pytest.mark.parametrize("host", [
    "::ffff:127.0.0.1", "::ffff:10.0.0.1", "::ffff:169.254.169.254", "::", "0.0.0.0",
])
def test_mapped_and_unspecified_ips_blocked(host: str) -> None:
    assert is_blocked_host(host)


def test_mapped_ipv6_url_blocked() -> None:
    with pytest.raises(ValueError):
        assert_safe_url("http://[::ffff:127.0.0.1]/")


def test_public_ip_literal_allowed() -> None:
    assert resolve_safe_ips("93.184.216.34") == ["93.184.216.34"]
    assert not is_blocked_host("93.184.216.34")


def test_any_internal_record_rejects_host(monkeypatch) -> None:
    import osiris.security as sec

    monkeypatch.setattr(sec, "_host_ips", lambda h: ["93.184.216.34", "192.168.0.9"])
    with pytest.raises(ValueError, match="engellendi"):
        resolve_safe_ips("mixed.example.com")


def test_safe_create_connection_rejects_before_connecting(monkeypatch) -> None:
    import osiris.security as sec

    def _forbidden(*a, **k):
        raise AssertionError("bağlanılmamalıydı")
    monkeypatch.setattr(sec.socket, "create_connection", _forbidden)
    for host in ("127.0.0.1", "localhost", "yok-boyle-host.invalid"):
        with pytest.raises(ValueError):
            safe_create_connection(host, 6667)
    with pytest.raises(ValueError):
        safe_create_connection("93.184.216.34", 0)


def test_safe_create_connection_tries_next_ip(monkeypatch) -> None:
    import osiris.security as sec

    monkeypatch.setattr(sec, "_host_ips", lambda h: ["93.184.216.34", "93.184.216.35"])
    tried = []

    def _connect(addr, timeout=None):
        tried.append(addr[0])
        if addr[0] == "93.184.216.34":
            raise OSError("refused")
        return "sock"
    monkeypatch.setattr(sec.socket, "create_connection", _connect)
    assert safe_create_connection("irc.example.com", 6697) == "sock"
    assert tried == ["93.184.216.34", "93.184.216.35"]


def test_sanitize_domain() -> None:
    assert sanitize_domain("Example.COM.") == "example.com"
    for bad in ["a\r\nBCC:x", "x y", "nodot", "a" * 300, "x/y", "-.com", ""]:
        with pytest.raises(ValueError):
            sanitize_domain(bad)


def test_sanitize_channel_and_nick() -> None:
    assert sanitize_channel("#osiris") == "#osiris"
    for bad in ["general", "#a\r\nJOIN #b", "#a,b", "", "x" * 100]:
        with pytest.raises(ValueError):
            sanitize_channel(bad)
    assert sanitize_irc_token("osiris-bot") == "osiris-bot"
    for bad in ["nick\r\nCMD", "a b", "a,b", "", "x" * 100]:
        with pytest.raises(ValueError):
            sanitize_irc_token(bad)


def test_sanitize_hostname() -> None:
    assert sanitize_hostname("Example.COM") == "example.com"
    for bad in ["", "a/b", "x" * 300, "a\rb"]:
        with pytest.raises(ValueError):
            sanitize_hostname(bad)


def test_safe_title() -> None:
    from bs4 import BeautifulSoup

    assert safe_title(None) is None
    assert safe_title(BeautifulSoup("<html></html>", "html.parser")) is None
    soup = BeautifulSoup("<html><head><title>Hi</title></head></html>", "html.parser")
    assert safe_title(soup) == "Hi"


def test_cap_list_and_mask() -> None:
    assert cap_list([1, 2, 3], 2) == [1, 2]
    assert cap_list("notalist", 2) == []
    assert mask_secret(None) == "***"
    assert mask_secret("abcdef") == "**cdef"
    assert mask_secret("ab") == "***"


class _FakeResp:
    def __init__(self, status=200, headers=None, body=b"ok"):
        self.status_code = status
        self.headers = headers or {}
        self._body = body
        self.url = ""
        self.closed = False

    @property
    def content(self):
        return getattr(self, "_content", self._body)

    @property
    def text(self):
        data = self.content
        return data.decode("utf-8", errors="replace") if isinstance(data, bytes) else data

    def iter_content(self, size):
        yield self._body

    def close(self):
        self.closed = True


class _FakeSession:
    def __init__(self, handler):
        self.handler = handler
        self.calls = []

    def request(self, method, url, **kwargs):
        self.calls.append((method, url))
        return self.handler(method, url, kwargs)


def _session_for(mapping):
    def handler(method, url, kwargs):
        assert kwargs.get("allow_redirects") is False
        assert kwargs.get("stream") is True
        return mapping(url)

    return _FakeSession(handler)


def test_fetch_url_direct() -> None:
    from osiris.security import fetch_url

    s = _session_for(lambda url: _FakeResp(200, {}, b"hello"))
    r = fetch_url(s, "GET", "https://example.com/")
    assert r.status_code == 200 and r.text == "hello"


def test_fetch_url_follows_safe_redirect() -> None:
    from osiris.security import fetch_url

    def mapping(url):
        if url == "https://example.com/a":
            return _FakeResp(302, {"Location": "/b"})
        return _FakeResp(200, {}, b"final")

    s = _session_for(mapping)
    r = fetch_url(s, "GET", "https://example.com/a")
    assert r.text == "final"
    assert [c[1] for c in s.calls] == ["https://example.com/a", "https://example.com/b"]


def test_fetch_url_blocks_redirect_to_private() -> None:
    import pytest
    from osiris.security import fetch_url

    s = _session_for(lambda url: _FakeResp(302, {"Location": "http://169.254.169.254/"}))
    with pytest.raises(ValueError):
        fetch_url(s, "GET", "https://example.com/a")


def test_fetch_url_redirect_loop_and_body_cap() -> None:
    import pytest
    from osiris.security import fetch_url

    s = _session_for(lambda url: _FakeResp(302, {"Location": "/loop"}))
    with pytest.raises(ValueError, match="yönlendirme"):
        fetch_url(s, "GET", "https://example.com/loop", max_redirects=2)

    big = _session_for(lambda url: _FakeResp(200, {}, b"x" * 100))
    r = fetch_url(big, "GET", "https://example.com/", max_bytes=10)
    assert r.content == b"x" * 10

    with pytest.raises(ValueError):
        fetch_url(big, "GET", "https://example.com/", max_redirects=99)


def test_fetch_url_post_becomes_get_on_303() -> None:
    from osiris.security import fetch_url

    seen = []

    def mapping(url):
        seen.append(url)
        if len(seen) == 1:
            return _FakeResp(303, {"Location": "/other"})
        return _FakeResp(200, {}, b"done")

    s = _session_for(mapping)
    fetch_url(s, "POST", "https://example.com/submit")
    assert s.calls[0][0] == "POST" and s.calls[1][0] == "GET"


def test_obfuscated_loopback_blocked() -> None:
    """Ondalık/hex/sekizlik loopback yazımları da engellenmeli."""
    import pytest
    from osiris.security import assert_safe_url, is_blocked_host

    assert is_blocked_host("2130706433")  # 127.0.0.1
    assert is_blocked_host("0x7f.0.0.1")
    for url in ["http://2130706433/", "http://0x7f.0.0.1/", "http://0177.0.0.1/"]:
        with pytest.raises(ValueError):
            assert_safe_url(url)


def test_resolve_proxy_without_operator_env(monkeypatch) -> None:
    from osiris.security import resolve_proxy

    monkeypatch.delenv("OSIRIS_TOR_SOCKS", raising=False)
    assert resolve_proxy(None, None, "OSIRIS_TOR_SOCKS", 9050, "SOCKS5") == ("127.0.0.1", 9050)
    assert resolve_proxy("::1", "9150", "OSIRIS_TOR_SOCKS", 9050, "SOCKS5") == ("::1", 9150)
    for host, port in (("tor", 9050), ("10.0.0.5", 9050), ("127.0.0.1", 0), ("127.0.0.1", "x")):
        with pytest.raises(ValueError):
            resolve_proxy(host, port, "OSIRIS_TOR_SOCKS", 9050, "SOCKS5")


def test_resolve_proxy_operator_env_wins(monkeypatch) -> None:
    from osiris.security import resolve_proxy

    monkeypatch.setenv("OSIRIS_TOR_SOCKS", "tor:9050")
    # Boş / localhost / aynı host → operatör proxy'si (UI'nin önceden doldurduğu localhost dahil)
    for host in (None, "", "127.0.0.1", "tor"):
        assert resolve_proxy(host, 1234, "OSIRIS_TOR_SOCKS", 9050, "SOCKS5") == ("tor", 9050)
    # Kullanıcı başka bir host seçemez (iç ağ taraması / proxy kaçırma)
    with pytest.raises(ValueError, match="OSIRIS_TOR_SOCKS"):
        resolve_proxy("evil.example.com", 9050, "OSIRIS_TOR_SOCKS", 9050, "SOCKS5")
    monkeypatch.setenv("OSIRIS_TOR_SOCKS", "[fd00::5]:9050")
    assert resolve_proxy(None, None, "OSIRIS_TOR_SOCKS", 9050, "SOCKS5") == ("fd00::5", 9050)
    for bad in ("tor", "tor:", ":9050", "tor:99999", "tor:abc"):
        monkeypatch.setenv("OSIRIS_TOR_SOCKS", bad)
        with pytest.raises(ValueError):
            resolve_proxy(None, None, "OSIRIS_TOR_SOCKS", 9050, "SOCKS5")


@pytest.mark.parametrize("url", [
    "http://127.0.0.1\\x.saldirgan.example/",   # urllib3 127.0.0.1'e bağlanır (E14)
    "http://example.com\\@127.0.0.1/",
    "http://example.com/\ta",
    "http://exa\nmple.com/",
    "http://example.com/\x00",
])
def test_parser_differential_chars_rejected(monkeypatch, url: str) -> None:
    import osiris.security as sec

    # Saldırganın joker DNS kaydı her adı genel bir IP'ye çözer
    monkeypatch.setattr(sec, "_host_ips", lambda h: ["93.184.216.34"])
    with pytest.raises(ValueError, match="geçersiz karakter"):
        assert_safe_url(url)
