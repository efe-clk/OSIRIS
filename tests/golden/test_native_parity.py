"""Rust (osiris_native) ↔ Python referans uygulaması eşdeğerlik testleri (R4).

Fixture'lar sözleşmenin bilinen noktalarını sabitler; bu dosya ise iki uygulamayı
rastgele (tohumlu, tekrarlanabilir) girdilerle karşılaştırır: HTML soyma, regex
çıkarma, audit kanonikleştirme, URL/host denetimi ve sanitizer'lar. Python
uygulamaları R7'de silinince bu dosya da kalkar.

Daha uzun koşu: ``OSIRIS_PARITY_N=200000 pytest tests/golden/test_native_parity.py``
"""

from __future__ import annotations

import json
import os
import random

import pytest

native = pytest.importorskip("osiris_native")

N = int(os.getenv("OSIRIS_PARITY_N", "3000"))

HTML_TOKENS = [
    "<", ">", "</", "<!--", "-->", "--!>", "--", "-", "!", "<!", "<?", "?>", "<![CDATA[", "]]>",
    "<![if x]>", "<!DOCTYPE html>", "<!doctype", "&", "&#", "&#x", "&#X", ";", "amp", "lt", "gt",
    "nbsp", "copy", "AMP", "foo", "x", "41", "65", "0", "128", "150", "1114112", "55296", "9999",
    "a", "b", "p", "br", "BR", "script", "SCRIPT", "style", "template", "rt", "rp", "ruby",
    "title", "textarea", "plaintext", "xmp", "iframe", "img", "hr", "/", "=", "'", '"', " ",
    "\t", "\n", "\r", "\x0c", "\x0b", "\xa0", "ş", "İ", "​", "\x1c", " ", "x=1",
    "class", "-.", "]", "[", "e", "1", ".", "&amp;", "&lt;", "&#39;", "&#x41;", "<p>", "</p>",
    "<b>", "</b>", "<script>", "</script>", "</script ", "<style>", "</style>", "<template>",
    "</template>", "<br/>", "</br>", "<title>", "</title>", "<a href='x'>", "</a>", "<img src=x>",
]

TEXT_TOKENS = [
    "test@example.com", "a.b@c.co", "@", ".", "-", "_", " ", "\n", "\xa0", "\x1c", "‍",
    "́", "8.8.8.8", "1.2.3.4", "999.1.1.1", "01.2.3.4", "١.٢.٣.٤", "10.0.0.256",
    "example.org", "EXAMPLE.ORG", "BİLGİ.COM", "ıi.com", "ſ.de", "K.com", "sub.domain.com.tr",
    "+90 (532) 123-45-67", "555-1234", "(0212) 555 12 34", "12345678901234567",
    "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa", "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq",
    "0x52908400098527886E0F7030069857D2E4169EE7", "3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy",
    "CVE-2024-12345", "cve-2021-44228", "CVE-١٢٣٤-٥٦٧٨", "CVE-2024-123", "x", "ş", "Ж", "中",
    "0", "9", "a1", "_x", "é", "Ⅻ", "²",
]

URL_TOKENS = [
    "http", "https", "HTTP", "ftp", "file", "javascript", ":", "//", "/", "?", "#", "@", ":@",
    "user", "pass", "[", "]", "::1", "::ffff:127.0.0.1", "127.0.0.1", "10.0.0.1", "93.184.216.34",
    "example.com", "EXAMPLE.COM", "localhost", "LOCALHOST.", "abc.onion", "x.ONION", "mixed.test",
    "internal.test", "v6.test", ":80", ":0", ":99999", ":abc", ":", "%25eth0", "fe80::1",
    "2606:2800::1", "v1.x", "\\", "\t", "\n", " ", "\x00", "\x7f", "ℂ", "℀", "ⓛ", "münchen.test",
    "0x7f.1", "2130706433", "..", ".", "a", "-", "%2e",
]

DNS = {
    "example.com": ["93.184.216.34"], "mixed.test": ["93.184.216.34", "192.168.0.9"],
    "internal.test": ["10.0.0.7"], "v6.test": ["2606:2800:220:1:248:1893:25c8:1946"],
    "münchen.test": ["93.184.216.34"], "0x7f.1": ["127.0.0.1"], "2130706433": ["127.0.0.1"],
    "bad.test": ["not-an-ip"], "scoped.test": ["fe80::1%2"],
}


def _resolver(host: str) -> list[str]:
    return DNS.get(host, [])


def _join(rng: random.Random, tokens: list[str], lo: int, hi: int) -> str:
    return "".join(rng.choice(tokens) for _ in range(rng.randint(lo, hi)))


def _outcome(fn, *args, **kwargs):
    try:
        return ("ok", fn(*args, **kwargs))
    except ValueError as exc:
        return ("error", str(exc))


@pytest.fixture(scope="module")
def pipeline():
    from osiris_pipeline.pipeline import ProcessingPipeline

    p = ProcessingPipeline.__new__(ProcessingPipeline)
    p._nlp = False
    return p


def test_clean_parity(pipeline) -> None:
    rng = random.Random(1)
    for i in range(N):
        text = _join(rng, HTML_TOKENS, 1, 40)
        assert native.clean_text(text) == pipeline._py_clean(text), f"#{i} {text!r}"


def test_clean_parity_structured(pipeline) -> None:
    rng = random.Random(2)
    for i in range(N):
        parts = []
        for _ in range(rng.randint(1, 8)):
            tag = rng.choice(["p", "b", "script", "style", "title", "a", "br", "rt", "template", "x-y"])
            attrs = "".join(
                rng.choice([" a", " a=b", " a='b c'", ' a="b>c"', " a=", " /", "/", " a='x", "\t", "=x"])
                for _ in range(rng.randint(0, 3)))
            close = rng.choice([">", "/>", " >", "", ">"])
            body = _join(rng, TEXT_TOKENS + HTML_TOKENS[:20], 0, 4)
            end = rng.choice([f"</{tag}>", f"</{tag.upper()}>", "", f"</{tag} x>", f"</{tag}"])
            parts.append(f"<{tag}{attrs}{close}{body}{end}")
        text = "".join(parts)
        assert native.clean_text(text) == pipeline._py_clean(text), f"#{i} {text!r}"


def test_extract_parity(pipeline) -> None:
    rng = random.Random(3)
    for i in range(N):
        text = _join(rng, TEXT_TOKENS, 1, 25)
        assert native.extract_entities(text) == pipeline._py_extract_regex(text), f"#{i} {text!r}"


def _rand_json(rng: random.Random, depth: int = 0):
    kind = rng.randint(0, 9 if depth < 3 else 5)
    if kind == 0:
        return None
    if kind == 1:
        return rng.choice([True, False])
    if kind == 2:
        return rng.choice([0, 1, -1, 2**53 - 1, 2**53, -(2**53), 2**63, 10**30, rng.randint(-10**6, 10**6)])
    if kind == 3:
        return rng.choice([0.0, -0.0, 1.0, 0.1, 1e16, 1e21, 1e-7, 1.5e-5, 123.456, 1e300, 5e-324,
                           rng.uniform(-1e6, 1e6), rng.random() * 10 ** rng.randint(-30, 30)])
    if kind in (4, 5):
        return _join(rng, ["a", "ş", "\n", "\"", "\\", "\x01", " ", "😀", "Ａ", "é", "\x7f"], 0, 6)
    if kind in (6, 7):
        return [_rand_json(rng, depth + 1) for _ in range(rng.randint(0, 4))]
    keys = ["a", "b", "ş", "😀", "Ａ", "A", "aa", "", "\x00"]
    return {rng.choice(keys): _rand_json(rng, depth + 1) for _ in range(rng.randint(0, 4))}


def test_audit_canonical_parity() -> None:
    from osiris import audit

    rng = random.Random(4)
    for i in range(N):
        detail = _rand_json(rng)
        user = rng.choice([None, "", "operator", "ş" * 3])
        res = rng.choice([None, "", "rss"])
        for version in (1, 2):
            want = _outcome(audit._py_canonical_payload, user, "act", res, detail, version)
            if version == 1:
                detail_v = json.loads(json.dumps(detail, default=str))
            else:
                detail_v = detail
            got = _outcome(native.audit_canonical_payload, user, "act", res, detail_v, version)
            if want[0] == "error":
                assert got[0] == "error", f"#{i} v{version} {detail!r}: {got}"
            else:
                assert got == want, f"#{i} v{version} {detail!r}"
            if want[0] == "ok":
                for prev in (None, "", "p"):
                    assert native.audit_chain_hash(prev, want[1]) == audit._py_chain_hash(prev, want[1])


def test_url_parity(monkeypatch) -> None:
    import osiris.security as sec

    monkeypatch.setattr(sec, "_host_ips", _resolver)
    rng = random.Random(5)
    for i in range(N):
        url = _join(rng, URL_TOKENS, 1, 12)
        onion = rng.random() < 0.2
        want = _outcome(sec._py_assert_safe_url, url, allow_onion=onion)
        got = _outcome(native.assert_safe_url, url, onion, _resolver)
        assert got == want, f"#{i} {url!r} onion={onion}"


def test_host_parity(monkeypatch) -> None:
    import osiris.security as sec

    monkeypatch.setattr(sec, "_host_ips", _resolver)
    rng = random.Random(6)
    hosts = list(DNS) + ["", " ", "localhost", "LOCALHOST..", "::", "0.0.0.0", "::ffff:8.8.8.8",
                         "fe80::1%eth0", "2001:db8::1%eth0", "1:0:1:1:1:1:1:1", "::1:2:3:4:5:6:7",
                         "93.184.216.34", "127.0.0.1", "01.2.3.4", "nope.test", " Example.COM. "]
    for i in range(N // 3):
        host = rng.choice(hosts) if rng.random() < 0.5 else _join(rng, URL_TOKENS[16:40], 1, 3)
        want = _outcome(sec._py_resolve_safe_ips, host)
        got = _outcome(native.resolve_safe_ips, host, _resolver)
        assert got == want, f"#{i} {host!r}"


def test_sanitizer_parity() -> None:
    import osiris.security as sec

    rng = random.Random(7)
    alphabet = ["a", "B", "-", ".", "_", " ", "\r", "\n", "\0", "/", "@", ":", "#", "&", ",", "*",
                "ş", "İ", "1", "é", "́", "x" * 63, "y" * 64]
    for i in range(N):
        v = _join(rng, alphabet, 0, 6)
        for py_fn, rs_fn, args in [
            (sec._py_sanitize_domain, native.sanitize_domain, ()),
            (sec._py_sanitize_hostname, native.sanitize_hostname, (253,)),
            (sec._py_sanitize_irc_token, native.sanitize_irc_token, (64,)),
            (sec._py_sanitize_channel, native.sanitize_channel, ()),
        ]:
            assert _outcome(rs_fn, v, *args) == _outcome(py_fn, v, *args), f"#{i} {py_fn.__name__} {v!r}"
        assert native.mask_secret(v or None, 4) == sec._py_mask_secret(v, 4)
