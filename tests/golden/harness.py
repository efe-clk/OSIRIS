"""Golden fixture koşum altyapısı (Python referans uygulaması).

Veri dilden bağımsızdır ve `fixtures/` altında durur (bkz. fixtures/README.md).
Bu modül hem testlerce (`test_golden.py`) hem de beklenen çıktıları üreten
`generate.py` tarafından kullanılır. Rust tarafı aynı JSON'ları kendi
harness'ıyla okur.
"""

from __future__ import annotations

import base64
import importlib.util
import io
import json
import socket
from contextlib import ExitStack
from datetime import datetime
from pathlib import Path
from typing import Any
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "fixtures"
PLUGINS = ROOT / "plugins"
# Fixture'larda DNS çözümü sabit: her host bu genel IP'ye çözülür (SSRF denetimi geçer)
PUBLIC_IP = "93.184.216.34"


def load(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def dump(path: Path, data: Any) -> None:
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


# --------------------------------------------------------------------------- hash
def run_hash(case: dict) -> dict:
    import hashlib

    from osiris_pipeline.pipeline import ProcessingPipeline

    p = ProcessingPipeline.__new__(ProcessingPipeline)
    text = case["input"] if "input" in case else case["input_repeat"]["text"] * case["input_repeat"]["times"]
    cleaned = p.clean(text)
    return {"cleaned": cleaned, "content_hash": hashlib.sha256(cleaned.encode()).hexdigest()}


# ------------------------------------------------------------------------ extract
def run_extract(case: dict) -> dict:
    from osiris_pipeline.pipeline import ProcessingPipeline

    p = ProcessingPipeline.__new__(ProcessingPipeline)
    p._nlp = False  # sözleşme yalnızca regex çıkarıcıyı kapsar (NER R3'te ayrı fixture)
    return {"entities": p.extract_entities(case["input"])}


# -------------------------------------------------------------------------- audit
def run_audit(case: dict) -> dict:
    from osiris.audit import _normalize, canonical_payload, chain_hash

    prev = None
    out = []
    for rec in case["records"]:
        user_id, resource, detail = _normalize(rec.get("user_id"), rec.get("resource"),
                                               rec.get("detail"))
        payload = canonical_payload(user_id, rec["action"], resource, detail,
                                    rec["chain_version"])
        digest = chain_hash(prev, payload)
        out.append({"payload": payload, "prev_hash": prev, "hash": digest})
        prev = digest
    return {"chain": out}


# --------------------------------------------------------------------------- stix
def run_stix(case: dict) -> dict:
    from osiris_report.generator import ReportGenerator

    gen = ReportGenerator.__new__(ReportGenerator)
    now = datetime.fromisoformat(case["now"].replace("Z", "+00:00"))
    return {"bundle": gen.generate_stix(case["identity_name"], case["title"], case["entities"],
                                        now=now, bundle_id=case["bundle_id"])}


# -------------------------------------------------------------------------- graph
def run_graph(case: dict) -> dict:
    from osiris_graph.engine import GraphEngine

    g = GraphEngine()
    for node in case.get("nodes", []):
        g.add_entity(node)
    for e in case["edges"]:
        g.add_relation(e[0], e[1], e[2] if len(e) > 2 else "related")
    centrality = {k: round(v, 12) for k, v in sorted(g.centrality().items())}
    # Topluluk SIRASI sözleşme dışıdır: her topluluk ve liste sıralanır
    communities = sorted(sorted(c) for c in g.communities())
    return {"degree_centrality": centrality, "communities": communities}


# ------------------------------------------------------------------------ plugins
class _FakeHTTP:
    """Kayıtlı HTTP değiş tokuşlarını oynatır; beklenmeyen istek = hata.

    Eşleştirme sıradan bağımsızdır (paralel istek atan plugin'ler için): gelen
    istek, henüz tüketilmemiş ilk eşdeğer kayıtla eşleşir. Hepsi tüketilmeli.
    """

    def __init__(self, exchanges: list[dict]):
        import threading

        self.exchanges = list(exchanges)
        self.used = [False] * len(self.exchanges)
        self.lock = threading.Lock()

    @property
    def unconsumed(self) -> int:
        return self.used.count(False)

    def __call__(self, method, url, **kwargs):  # requests.request imzası
        import requests

        got = {"method": method.upper(), "url": url}
        if kwargs.get("params"):
            got["params"] = {str(k): str(v) for k, v in kwargs["params"].items()}
        if kwargs.get("json") is not None:
            got["json"] = kwargs["json"]
        with self.lock:
            for i, ex in enumerate(self.exchanges):
                req = ex["request"]
                want = {k: req[k] for k in ("method", "url", "params", "json") if k in req}
                if not self.used[i] and want == got:
                    self.used[i] = True
                    break
            else:
                raise AssertionError(f"beklenmeyen istek: {got}")
        res = ex["response"]
        if "error" in res:  # ağ hatası benzetimi
            raise getattr(requests, res["error"])("fixture ağ hatası")
        resp = requests.models.Response()
        resp.status_code = res.get("status", 200)
        resp.reason = res.get("reason", "")
        resp.headers = requests.structures.CaseInsensitiveDict(res.get("headers", {}))
        body = base64.b64decode(res["body_b64"]) if "body_b64" in res else \
            res.get("body", "").encode("utf-8")
        resp.raw = io.BytesIO(body)
        resp.encoding = res.get("encoding", "utf-8")
        resp.url = url
        return resp


class _FakeAnswer:
    def __init__(self, text: str):
        self.text = text

    def __str__(self) -> str:
        return self.text


class _FakeSocket:
    def __init__(self, chunks: list[str]):
        self.chunks = [c.encode("utf-8") for c in chunks]
        self.sent: list[bytes] = []

    def settimeout(self, _t):
        pass

    def sendall(self, data: bytes):
        self.sent.append(data)

    def recv(self, _n):
        return self.chunks.pop(0) if self.chunks else b""

    def close(self):
        pass

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


def load_plugin(plugin_id: str):
    path = PLUGINS / plugin_id / "collector.py"
    spec = importlib.util.spec_from_file_location(
        f"golden_{plugin_id.replace('-', '_')}_collector", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    from osiris.plugin import BaseCollector

    for attr in vars(module).values():
        if isinstance(attr, type) and issubclass(attr, BaseCollector) and attr is not BaseCollector:
            return attr
    raise ValueError(f"collector yok: {plugin_id}")


def _result_dict(result) -> dict:
    return {
        "success": result.success,
        "error": result.error,
        "items": [i.model_dump() for i in result.items],
        "metadata": result.metadata,
    }


def run_plugin(case: dict) -> tuple[dict, dict]:
    """Plugin'i kayıtlı ağ etkileşimleriyle çalıştırır. (sonuç, tüketim raporu) döner."""
    import dns.resolver
    import osiris.security as sec
    import requests

    cls = load_plugin(case["plugin"])
    http = _FakeHTTP(case.get("http", []))
    sockets = [_FakeSocket(s["recv"]) for s in case.get("sockets", [])]
    connects: list[tuple] = []

    def _connect(addr, timeout=None, *a, **k):
        connects.append(tuple(addr))
        if not sockets:
            raise OSError("fixture: soket yok")
        return sockets.pop(0)

    dns_map = case.get("dns", {})

    def _resolve(_self, name, rtype="A", *a, **k):
        val = dns_map.get(rtype, "NoAnswer")
        if val == "NoAnswer":
            raise dns.resolver.NoAnswer()
        if val == "NXDOMAIN":
            raise dns.resolver.NXDOMAIN()
        return [_FakeAnswer(v) for v in val]

    with ExitStack() as st:
        st.enter_context(mock.patch.object(requests, "request", http))
        st.enter_context(mock.patch.object(
            requests.Session, "request", lambda _s, method, url, **kw: http(method, url, **kw)))
        st.enter_context(mock.patch.object(sec, "_host_ips", lambda h: [PUBLIC_IP]))
        st.enter_context(mock.patch.object(socket, "create_connection", _connect))
        st.enter_context(mock.patch.object(dns.resolver.Resolver, "resolve", _resolve))
        result = cls().collect(case["config"])
    # ThreadPool sırası belirsiz olabilir (username-search): items zaten sıralı döner
    report = {"http_unconsumed": http.unconsumed, "connects": [list(c) for c in connects]}
    return _result_dict(result), report


RUNNERS = {"hash": run_hash, "extract": run_extract, "audit": run_audit,
           "stix": run_stix, "graph": run_graph}
