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
    "internal.test", "v6.test", "forum.i2p", "X.I2P", ":80", ":0", ":99999", ":abc", ":", "%25eth0", "fe80::1",
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
        i2p = rng.random() < 0.2
        want = _outcome(sec._py_assert_safe_url, url, allow_onion=onion, allow_i2p=i2p)
        got = _outcome(native.assert_safe_url, url, onion, i2p, _resolver)
        assert got == want, f"#{i} {url!r} onion={onion} i2p={i2p}"


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


def test_record_json_parity() -> None:
    """Plugin raw_content: Python json.loads + _py_record_json ↔ Rust parse_py + record_json."""
    from osiris.records import _py_record_json

    rng = random.Random(8)
    for i in range(N):
        value = _rand_json(rng)
        assert native.record_json(value) == _py_record_json(value), f"#{i} {value!r}"
        text = json.dumps(value, ensure_ascii=rng.random() < 0.5)
        # rastgele bozma: hata davranışı da aynı olmalı
        if rng.random() < 0.5 and text:
            pos = rng.randrange(len(text) + 1)
            text = text[:pos] + rng.choice(["", ",", "]", "}", "\"", "\\", "1", " ", "\x01", "e", ".", "-", "NaN"]) + text[pos + 1:]
        try:
            parsed = json.loads(text)
        except ValueError:
            parsed = _INVALID
        try:
            want = ("ok", _py_record_json(parsed)) if parsed is not _INVALID else ("error",)
        except ValueError:
            want = ("error",)
        if parsed is not _INVALID and not _utf8_ok(json.dumps(parsed, ensure_ascii=False)):
            # Eşlenmemiş vekil: Python dizede taşır (kuyruğa yazarken UTF-8 hatası verir),
            # Rust U+FFFD yazar — belgelenmiş fark (SOZLESME §4)
            continue
        try:
            got = ("ok", native.record_json_from_text(text))
        except ValueError:
            got = ("error",)
        assert got == want, f"#{i} {text!r}"


_INVALID = object()


def _utf8_ok(s: str) -> bool:
    try:
        s.encode("utf-8")
    except UnicodeEncodeError:
        return False
    return True


# ------------------------------------------------------------------ R5: bs4 ağacı ve seçiciler
_ASCII_SPACES = "\x20\x0a\x09\x0c\x0d"
_LIST_ATTRS_ALL = {"class", "accesskey", "dropzone"}


def _bs4_tree(node):
    from bs4 import NavigableString

    if isinstance(node, NavigableString):
        return ("#text", type(node).__name__, str(node))
    attrs = [(k, " ".join(v) if isinstance(v, list) else v) for k, v in node.attrs.items()]
    return (node.name, attrs, [_bs4_tree(c) for c in node.contents])


def _norm(tree, *, rust: bool):
    """Yalnız-boşluk dizeleri at (bs4 bunları " "/"\\n" yapar); çok değerli öznitelikleri birleştir."""
    if tree[0] == "#text":
        return None if not tree[2].strip(_ASCII_SPACES) else tree
    name, attrs, kids = tree
    if rust:
        attrs = [(k, " ".join(v.split()) if k in _LIST_ATTRS_ALL or (name, k) in {
            ("a", "rel"), ("a", "rev"), ("link", "rel"), ("link", "rev"), ("td", "headers"),
            ("th", "headers"), ("form", "accept-charset"), ("object", "archive"), ("area", "rel"),
            ("icon", "sizes"), ("iframe", "sandbox"), ("output", "for")} else v) for k, v in attrs]
    out_kids = []
    for k in kids:
        n = _norm(k, rust=rust)
        if n is None:
            continue
        # bitişik metinler bs4'te zaten tek dizedir
        out_kids.append(n)
    return (name, sorted(attrs), out_kids)


ATTR_TOKENS = [" a", " a=b", " a='b c'", ' a="b>c"', " a=", " /", "/", " a='x", "\t", "=x",
               " class='x  y'", " class=x", " id=i1", " id='i&amp;2'", " href='/p?a=1&b=2'",
               " title=&lt;t&gt;", " data-x=&notit;", " A=B", " a=1 a=2", " v=&#x41;&#65", " w=&ampx"]


def _structured_html(rng: random.Random) -> str:
    parts = []
    for _ in range(rng.randint(1, 8)):
        tag = rng.choice(["p", "b", "div", "span", "script", "style", "title", "a", "br", "rt",
                          "template", "ul", "li", "x-y", "img", "textarea", "pre"])
        attrs = "".join(rng.choice(ATTR_TOKENS) for _ in range(rng.randint(0, 3)))
        close = rng.choice([">", "/>", " >", ">", ">"])
        body = _join(rng, TEXT_TOKENS[:20] + HTML_TOKENS[:30], 0, 4)
        end = rng.choice([f"</{tag}>", "", f"</{tag.upper()}>", "</div>", f"</{tag} x>"])
        parts.append(f"<{tag}{attrs}{close}{body}{end}")
    return "".join(parts)


def test_html_tree_parity() -> None:
    from bs4 import BeautifulSoup

    rng = random.Random(9)
    for i in range(N):
        markup = _structured_html(rng) if rng.random() < 0.7 else _join(rng, HTML_TOKENS, 1, 30)
        try:
            soup = BeautifulSoup(markup, "html.parser")
        except Exception:  # noqa: BLE001 — ParserRejectedMarkup / int sınırı
            with pytest.raises(ValueError):
                native.html_tree(markup)
            continue
        want = _norm(_bs4_tree(soup), rust=False)
        got = _norm(native.html_tree(markup), rust=True)
        assert got[2] == want[2], f"#{i} {markup!r}"


SELECTORS = ["p", "div.post", ".x", "#i1", "a[href]", "[a=b]", "[class~=y]", "div > p", "div p",
             "p + span", "p ~ span", "li:first-child", "li:last-child", "li:nth-child(2n+1)",
             "li:nth-of-type(2)", "p:not(.x)", ":is(b, span)", "*", "div, span", "[id^=i]",
             "[href*='a=1']", "[title$='t>']", "ul li", "span:empty", "b:only-child", "[A=B]",
             "DIV", "p:nth-last-child(1)", "[class='x y']", "[a|=b]", "x-y"]
BAD_SELECTORS = ["p[", "[title$=t>]", "..x", "p >", ":nth-child(x)", "[a=]", ""]


def test_select_parity() -> None:
    from bs4 import BeautifulSoup

    rng = random.Random(10)
    for i in range(N // 3):
        markup = _structured_html(rng)
        soup = BeautifulSoup(markup, "html.parser")
        for sel in SELECTORS:
            want = [(n.name, n.get_text(" ", strip=True)) for n in soup.select(sel)]
            got = [tuple(x) for x in native.html_select(markup, sel)]
            assert got == want, f"#{i} {sel!r} {markup!r}"
        assert native.html_title(markup) == _py_title(soup), f"#{i} {markup!r}"
    for sel in BAD_SELECTORS:
        with pytest.raises(Exception):  # noqa: B017 — soupsieve: SelectorSyntaxError
            BeautifulSoup("<p>x</p>", "html.parser").select(sel)
        with pytest.raises(ValueError):
            native.html_select("<p>x</p>", sel)


def _py_title(soup):
    from osiris.security import safe_title

    return safe_title(soup)


def test_unescape_and_urljoin_parity() -> None:
    import html
    from urllib.parse import urljoin

    rng = random.Random(11)
    ent = ["&", "&amp", "&amp;", "&notit;", "&not", "&#", "&#x", "&#65", "&#x41;", "&#0;", "&#128;",
           "&#xD800;", "&#1114112;", "&lt", "&ltx", "&#65x", "a", ";", "ş", " ", "&nbsp", "&quot;"]
    for i in range(N):
        s = _join(rng, ent, 0, 8)
        assert native.html_unescape(s) == html.unescape(s), f"#{i} {s!r}"
    bases = ["https://h.example/a/b/c?q=1#f", "http://h/", "https://h", "https://h/a;p/b"]
    rels = ["/x", "x", "../x", "./x", "?y", "#z", "//o/x", "https://o", "", ".", "..", "a/../../b",
            "x;p", "http:x", "https:/x", "%2e%2e/x", "x?y#z"]
    for i in range(N // 3):
        b = rng.choice(bases)
        r = "".join(rng.choice(rels) for _ in range(rng.randint(1, 2)))
        assert native.urljoin(b, r) == urljoin(b, r), f"#{i} {b!r} {r!r}"


# ---------------------------------------------------------------- R5: RSS/Atom (feedparser)
def _feed_docs(rng: random.Random, n: int):
    TXT = ["Merhaba", "a &amp; b", "&lt;p&gt;x&lt;/p&gt;", "&lt;b&gt;kalın&lt;/b&gt; yazı", "&lt;script&gt;k()&lt;/script&gt;ok",
           "ş ğ ü", "&#252;", "&#x41;", "  boşluk  ", "<![CDATA[<i>c</i> & d]]>", "&lt;a href='javascript:x'&gt;l&lt;/a&gt;",
           "&lt;img src=x onerror=y&gt;", "&lt;iframe&gt;i&lt;/iframe&gt;", "1 &lt; 2", "&lt;br&gt;", "&quot;q&quot;", "&lt;!-- y --&gt;", "€"]
    def txt(): return "".join(rng.choice(TXT) for _ in range(rng.randint(0, 4)))
    def rss_item():
        parts = []
        for tag in rng.sample(["title", "link", "description", "pubDate", "guid", "content:encoded", "dc:date", "description"], rng.randint(1, 6)):
            if tag == "link":
                parts.append(f"<link>https://x.example/{rng.randint(1,9)}</link>")
            elif tag == "guid":
                parts.append(f"<guid isPermaLink=\"{rng.choice(['true','false'])}\">https://g.example/{rng.randint(1,9)}</guid>")
            elif tag == "pubDate":
                parts.append(f"<pubDate> Mon, 05 Jan 2026 1{rng.randint(0,9)}:00:00 GMT </pubDate>")
            elif tag == "dc:date":
                parts.append("<dc:date>2026-01-01</dc:date>")
            else:
                parts.append(f"<{tag}>{txt()}</{tag}>")
        return "<item>" + "".join(parts) + "</item>"
    def atom_entry():
        parts = []
        for tag in rng.sample(["title", "link", "summary", "content", "published", "id", "updated", "link2"], rng.randint(1, 6)):
            ty = rng.choice(["", " type=\"text\"", " type=\"html\"", " type=\"xhtml\""])
            if tag == "link":
                parts.append(f"<link href=\"https://a.example/{rng.randint(1,9)}\"/>")
            elif tag == "link2":
                parts.append(f"<link rel=\"{rng.choice(['self','alternate','enclosure'])}\" href=\"https://b.example/\"/>")
            elif tag in ("published", "updated"):
                parts.append(f"<{tag}>2026-01-0{rng.randint(1,9)}T00:00:00Z</{tag}>")
            elif tag == "id":
                parts.append("<id>urn:uuid:1</id>")
            elif "xhtml" in ty:
                parts.append(f"<{tag}{ty}><div xmlns=\"http://www.w3.org/1999/xhtml\"><p>{rng.choice(['x','ş & y','<b>k</b>'])}</p></div></{tag}>")
            else:
                parts.append(f"<{tag}{ty}>{txt()}</{tag}>")
        return "<entry>" + "".join(parts) + "</entry>"
    for _ in range(n):
        if rng.random() < 0.6:
            yield ('<?xml version="1.0" encoding="utf-8"?><rss version="2.0" '
                   'xmlns:content="http://purl.org/rss/1.0/modules/content/" '
                   'xmlns:dc="http://purl.org/dc/elements/1.1/"><channel><title>K</title>'
                   + "".join(rss_item() for _ in range(rng.randint(1, 3))) + "</channel></rss>")
        else:
            yield ('<?xml version="1.0" encoding="utf-8"?><feed xmlns="http://www.w3.org/2005/Atom">'
                   "<title>K</title>" + "".join(atom_entry() for _ in range(rng.randint(1, 3))) + "</feed>")


def _unescape_markup(s):
    return (s or "").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">") \
        .replace('"', "").replace("'", "")


def test_feed_parity(pipeline) -> None:
    """rss plugin'i: sözleşme content_hash düzeyindedir (SOZLESME §4) — girdi sayısı, link,
    published birebir; summary clean() sonrası aynı; başlık yalnızca kaçış biçiminde farklı olabilir."""
    feedparser = pytest.importorskip("feedparser")
    rng = random.Random(12)
    for i, doc in enumerate(_feed_docs(rng, N // 3)):
        fp = feedparser.parse(doc.encode())
        ents, _bozo = native.feed_parse(doc.encode())
        assert len(ents) == len(fp.entries), f"#{i} {doc!r}"
        for (link, title, summary, published), e in zip(ents, fp.entries, strict=True):
            assert link == (e.get("link", "") or None), f"#{i} link {doc!r}"
            assert published == (e.get("published", "") or None), f"#{i} published {doc!r}"
            want_summary = e.get("summary", "") or e.get("description", "")
            assert pipeline._py_clean(summary or "") == pipeline._py_clean(want_summary), f"#{i} {doc!r}"
            assert _unescape_markup(title) == _unescape_markup(str(e.get("title", "")) or None), f"#{i} {doc!r}"
