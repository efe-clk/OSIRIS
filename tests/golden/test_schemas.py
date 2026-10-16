"""schemas/ JSON Schema'ları gerçek veriyle tutarlı mı? (manifest, sonuç, kuyruk mesajı)"""

import json
import sys
from pathlib import Path
from unittest.mock import MagicMock

import pytest

jsonschema = pytest.importorskip("jsonschema")
from referencing import Registry, Resource  # noqa: E402

sys.path.insert(0, str(Path(__file__).parent))
import harness  # noqa: E402

SCHEMAS = harness.ROOT / "schemas"


def _validator(name: str):
    registry = Registry()
    for path in SCHEMAS.glob("*.schema.json"):
        schema = json.loads(path.read_text(encoding="utf-8"))
        registry = registry.with_resource(path.name, Resource.from_contents(schema))
        registry = registry.with_resource(schema["$id"], Resource.from_contents(schema))
    schema = json.loads((SCHEMAS / name).read_text(encoding="utf-8"))
    jsonschema.Draft202012Validator.check_schema(schema)
    return jsonschema.Draft202012Validator(schema, registry=registry)


@pytest.mark.parametrize("manifest", sorted(harness.PLUGINS.glob("*/manifest.json")),
                         ids=lambda p: p.parent.name)
def test_manifests_valid(manifest: Path) -> None:
    data = json.loads(manifest.read_text(encoding="utf-8"))
    _validator("manifest.schema.json").validate(data)
    assert data["id"] == manifest.parent.name


@pytest.mark.parametrize("path", sorted((harness.FIXTURES / "plugins").glob("*.json")),
                         ids=lambda p: p.stem)
def test_fixture_results_match_schema(path: Path) -> None:
    _validator("collection_result.schema.json").validate(harness.load(path)["expected"])


def test_queue_message_matches_schema() -> None:
    from osiris.plugin import BaseCollector, CollectedItem, CollectionResult
    from osiris_collector.manager import CollectorManager

    class P(BaseCollector):
        def collect(self, config=None):
            return CollectionResult(items=[CollectedItem(raw_content="x", url="https://e.com")])

    mgr = CollectorManager.__new__(CollectorManager)
    mgr.plugins, mgr.manifests, mgr.redis = {"p": P()}, {}, MagicMock()
    mgr.queue_name, mgr.database_url = "q", None
    v = _validator("queue_message.schema.json")
    for cfg in ({}, {"source_id": "3f2504e0-4f89-11d3-9a0c-0305e82c3301"}):
        mgr.run_collection("p", cfg)
        v.validate(json.loads(mgr.redis.rpush.call_args[0][1]))


def test_schema_rejects_bad_manifest() -> None:
    v = _validator("manifest.schema.json")
    bad = {"id": "Kötü ID", "name": "x", "version": "1", "network_type": "uzay",
           "description": "", "config_schema": {"a": {"type": "tarih"}}}
    assert len(list(v.iter_errors(bad))) >= 4
