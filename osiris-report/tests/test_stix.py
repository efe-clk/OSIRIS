"""STIX 2.1 rapor testleri."""

import json

from osiris_report.generator import ReportGenerator


def test_generate_stix_bundle_structure() -> None:
    rg = ReportGenerator(output_dir="/tmp/opencode/reports-stix")
    raw = rg.generate_stix(
        "OSIRIS-Test",
        "Test Raporu",
        [
            {"type": "ip", "value": "1.2.3.4"},
            {"type": "domain", "value": "evil.example"},
            {"type": "cve", "value": "cve-2024-1234"},
            {"type": "person", "value": "Ali"},  # IOC değil → atlanır
            {"type": "email", "value": "a@b.co"},
        ],
    )
    bundle = json.loads(raw)
    assert bundle["type"] == "bundle"
    assert bundle["spec_version"] == "2.1"
    types = [o["type"] for o in bundle["objects"]]
    assert "identity" in types
    assert "indicator" in types
    assert "vulnerability" in types
    assert not any(o.get("name") == "Ali" for o in bundle["objects"])
    vuln = next(o for o in bundle["objects"] if o["type"] == "vulnerability")
    assert vuln["name"] == "CVE-2024-1234"


def test_generate_stix_empty_entities() -> None:
    rg = ReportGenerator(output_dir="/tmp/opencode/reports-stix")
    bundle = json.loads(rg.generate_stix("X", "T", []))
    assert len(bundle["objects"]) == 1  # yalnızca identity


def test_generate_html_escapes() -> None:
    rg = ReportGenerator(output_dir="/tmp/opencode/reports-html")
    html = rg.generate_html("T", "S", "O", [{"title": "<b>B</b>", "description": "D"}], ["s"])
    assert "<b>B</b>" not in html
    assert "&lt;b&gt;" in html
    assert html.startswith("<!DOCTYPE html>")


def test_ipv6_maps_to_ipv6_addr() -> None:
    import json

    from osiris_report.generator import ReportGenerator

    rg = ReportGenerator(output_dir="/tmp/opencode/reports-ipv6")
    bundle = json.loads(rg.generate_stix(
        "X", "T", [{"type": "ip", "value": "::1"}]))
    kinds = {o["type"] for o in bundle["objects"]}
    patterns = [o.get("pattern", "") for o in bundle["objects"]]
    assert any("ipv6-addr" in p for p in patterns)
    assert "identity" in kinds


def test_stix_deterministic_with_injected_clock_and_id(tmp_path) -> None:
    from datetime import UTC, datetime

    import pytest
    from osiris_report.generator import ReportGenerator

    gen = ReportGenerator(output_dir=tmp_path)
    ents = [{"type": "ip", "value": "93.184.216.34"}, {"type": "cve", "value": "cve-2024-1"}]
    kw = {"now": datetime(2026, 1, 2, 3, 4, 5, tzinfo=UTC),
          "bundle_id": "bundle--00000000-0000-4000-8000-000000000001"}
    a = gen.generate_stix("OSIRIS", "t", ents, **kw)
    assert a == gen.generate_stix("OSIRIS", "t", ents, **kw)
    assert '"created": "2026-01-02T03:04:05.000Z"' in a
    with pytest.raises(ValueError):
        gen.generate_stix("OSIRIS", "t", ents, bundle_id="nope")
    with pytest.raises(ValueError):
        gen.generate_stix("OSIRIS", "t", ents, now=datetime(2026, 1, 1))
