"""Processing Pipeline testleri."""

import pytest
from osiris_pipeline.pipeline import ProcessingPipeline


@pytest.fixture
def pipeline() -> ProcessingPipeline:
    return ProcessingPipeline()


def test_clean_strips_html(pipeline: ProcessingPipeline) -> None:
    html = "<html><body><h1>Başlık</h1><p>İçerik   metni</p></body></html>"
    assert pipeline.clean(html) == "Başlık İçerik metni"


def test_detect_language(pipeline: ProcessingPipeline) -> None:
    assert pipeline.detect_language("Bu bir Türkçe cümledir.") == "tr"


def test_extract_entities_email_ip_domain(pipeline: ProcessingPipeline) -> None:
    text = "İletişim: test@example.com, sunucu 192.168.1.1, site example.org"
    entities = pipeline.extract_entities(text)
    types = {e["type"] for e in entities}
    assert "email" in types
    assert "ip" in types
    assert "domain" in types


def test_process_one_bad_payloads() -> None:
    import pytest
    from osiris_pipeline.pipeline import ProcessingPipeline

    p = ProcessingPipeline.__new__(ProcessingPipeline)
    p._nlp = False
    for bad in ["bozuk", "[1,2]", '{"item": {}}', '{"plugin_id": "x"}']:
        with pytest.raises(ValueError):
            p.process_one(bad)


def test_run_handles_bad_and_store_errors() -> None:
    from unittest.mock import MagicMock

    from osiris_pipeline.pipeline import ProcessingPipeline

    p = ProcessingPipeline.__new__(ProcessingPipeline)
    p._nlp = False
    p.queue_name = "q"
    p.embedding_model_name = None
    p._embedder = None
    p.redis = MagicMock()
    p.redis.lpop.side_effect = [
        '{"bozuk": ',
        '{"plugin_id":"t","item":{"raw_content":"selam"}}',
        '{"plugin_id":"t","item":{"raw_content":"merhaba"}}',
        None,
    ]
    p.store = MagicMock(side_effect=[RuntimeError("db"), None])  # type: ignore[method-assign]
    assert p.run(batch_size=10) == 1
    p.redis.lpop.side_effect = Exception("redis down")
    p.redis.lpop.side_effect = __import__("redis").RedisError("down")
    assert p.run() == 0


def test_nlp_fallback_without_spacy(monkeypatch) -> None:
    import sys

    from osiris_pipeline.pipeline import ProcessingPipeline

    monkeypatch.setitem(sys.modules, "spacy", None)
    p = ProcessingPipeline.__new__(ProcessingPipeline)
    p._nlp = None
    assert p.nlp is None
    assert any(e["type"] == "email" for e in p.extract_entities("a@b.co"))


def test_store_rejects_malformed_result() -> None:
    import pytest
    from osiris_pipeline.pipeline import ProcessingPipeline

    p = ProcessingPipeline.__new__(ProcessingPipeline)
    p.database_url = "postgresql://localhost/db"
    with pytest.raises(ValueError):
        p.store({})
    with pytest.raises(ValueError):
        p.store({"item": "degil-dict"})
    # bozuk topics/tags/entities zararsızca temizlenir (DB yok → None öncesi değil;
    # burada yalnızca şema katmanı test edilir: database_url None ile atlanır)
    p2 = ProcessingPipeline.__new__(ProcessingPipeline)
    p2.database_url = None
    assert p2.store({"item": {"content_hash": "x"}, "topics": "str",
                     "entities": {"a": 1}}) is None


def test_process_one_propagates_source_id() -> None:
    import json

    p = ProcessingPipeline.__new__(ProcessingPipeline)
    p._nlp = False
    p._embedder = False
    p.embedding_model_name = None
    sid = "3f2504e0-4f89-11d3-9a0c-0305e82c3301"
    msg = {"plugin_id": "rss", "source_id": sid, "item": {"raw_content": "x"}}
    assert p.process_one(json.dumps(msg))["item"]["source_id"] == sid
    msg["source_id"] = "bozuk"
    assert p.process_one(json.dumps(msg))["item"]["source_id"] is None
    del msg["source_id"]  # eski biçim mesaj: alan yok → NULL
    assert p.process_one(json.dumps(msg))["item"]["source_id"] is None
