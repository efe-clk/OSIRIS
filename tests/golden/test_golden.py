"""Golden fixture testleri — Python uygulaması fixtures/ altındaki sözleşmeye uymalı.

Rust tarafı aynı JSON dosyalarını kendi testleriyle okur (docs/SOZLESME.md).
Beklenenleri yenilemek: `python tests/golden/generate.py` (sözleşme değişikliği!).
"""

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent))
import harness  # noqa: E402


def _cases(area: str):
    data = harness.load(harness.FIXTURES / area / "cases.json")
    return [pytest.param(c, id=c["name"]) for c in data["cases"]]


@pytest.mark.parametrize("area", sorted(harness.RUNNERS))
def test_every_case_has_expected(area: str) -> None:
    for case in harness.load(harness.FIXTURES / area / "cases.json")["cases"]:
        assert "expected" in case, f"{area}/{case['name']}: generate.py çalıştırın"


@pytest.mark.parametrize("case", _cases("hash"))
def test_content_hash(case) -> None:
    assert harness.run_hash(case) == case["expected"]


@pytest.mark.parametrize("case", _cases("extract"))
def test_extract(case) -> None:
    assert harness.run_extract(case) == case["expected"]


@pytest.mark.parametrize("case", _cases("audit"))
def test_audit_chain(case) -> None:
    assert harness.run_audit(case) == case["expected"]
    # Zincir, doğrulayıcıdan da geçmeli (saklanan normalize değerlerle)
    from osiris.audit import _normalize, verify_records

    rows = []
    for i, (rec, link) in enumerate(zip(case["records"], case["expected"]["chain"], strict=True)):
        user_id, resource, detail = _normalize(rec.get("user_id"), rec.get("resource"), rec.get("detail"))
        rows.append((i + 1, user_id, rec["action"], resource, detail,
                     link["prev_hash"], link["hash"], rec["chain_version"]))
    assert verify_records(rows)["ok"] is True


@pytest.mark.parametrize("case", _cases("stix"))
def test_stix_bytes(case) -> None:
    assert harness.run_stix(case) == case["expected"]


@pytest.mark.parametrize("case", _cases("graph"))
def test_graph(case) -> None:
    assert harness.run_graph(case) == case["expected"]


_PLUGIN_FILES = sorted((harness.FIXTURES / "plugins").glob("*.json"))


def test_every_plugin_has_fixture() -> None:
    covered = {harness.load(p)["plugin"] for p in _PLUGIN_FILES}
    plugins = {p.parent.name for p in harness.PLUGINS.glob("*/manifest.json")}
    assert plugins <= covered, f"fixture'sız plugin: {sorted(plugins - covered)}"


@pytest.mark.parametrize("path", _PLUGIN_FILES, ids=[p.stem for p in _PLUGIN_FILES])
def test_plugin_fixture(path: Path) -> None:
    case = harness.load(path)
    result, report = harness.run_plugin(case)
    assert report["http_unconsumed"] == 0, "kayıtlı HTTP yanıtlarının hepsi kullanılmalı"
    assert report == case["expected_network"]
    assert result == case["expected"]


_EMB = harness.load(harness.FIXTURES / "embedding" / "cases.json")
_needs_model = pytest.mark.skipif(not harness.embedding_available(),
                                  reason="embedding modeli yok (python models/download.py)")


def _cos(a, b) -> float:
    return sum(x * y for x, y in zip(a, b, strict=True))


@_needs_model
@pytest.mark.parametrize("case", [pytest.param(c, id=c["name"]) for c in _EMB["cases"]])
def test_embedding_matches_within_tolerance(case) -> None:
    got = harness.run_embedding(case)["vector"]
    want = case["expected"]["vector"]
    assert len(got) == _EMB["dim"] == len(want)
    assert _cos(got, want) >= _EMB["tolerance"]["min_cosine_to_expected"]


def test_embedding_fixture_semantics() -> None:
    """Beklenen vektörler kendi içinde anlamlı (model olmadan da koşar)."""
    vec = {c["name"]: c["expected"]["vector"] for c in _EMB["cases"]}
    for v in vec.values():
        assert abs(sum(x * x for x in v) - 1.0) < 1e-4  # L2-normalize
    for pair in _EMB["pairs"]:
        c = _cos(vec[pair["a"]], vec[pair["b"]])
        assert c >= pair.get("min_cosine", -1) and c <= pair.get("max_cosine", 1)
