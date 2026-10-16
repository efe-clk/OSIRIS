"""ONNX NER birleştirme kuralları — modelsiz (sahte tokenizer + sahte oturum).

Bu testler SOZLESME §11'deki kuralları sabitler; Rust aynı kuralları uygular.
"""

from types import SimpleNamespace

import numpy as np
import pytest
from osiris_pipeline import ner as ner_mod
from osiris_pipeline.ner import OnnxNer, _trim_punct

LABELS = ["O", "B-DATE", "I-DATE", "B-PER", "I-PER", "B-ORG", "I-ORG", "B-LOC", "I-LOC"]


def _make(tokens, labels, monkeypatch=None, window=None):
    """tokens: [(metin_parçası, başlangıç, bitiş, kelime_no)], labels: token başına etiket."""
    n = OnnxNer.__new__(OnnxNer)
    n.id2label = dict(enumerate(LABELS))
    n.cls_id, n.sep_id = 101, 102
    enc = SimpleNamespace(ids=list(range(1000, 1000 + len(tokens))),
                          offsets=[(s, e) for _, s, e, _ in tokens],
                          word_ids=[w for *_, w in tokens])
    n.tokenizer = SimpleNamespace(encode=lambda text, add_special_tokens=False: enc)
    calls = []

    class Session:
        def run(self, _names, feed):
            ids = feed["input_ids"][0].tolist()
            assert ids[0] == 101 and ids[-1] == 102
            assert feed["token_type_ids"].sum() == 0 and feed["attention_mask"].min() == 1
            calls.append(len(ids) - 2)
            logits = np.zeros((1, len(ids), len(LABELS)), dtype=np.float32)
            for pos, tok_id in enumerate(ids[1:-1], start=1):
                logits[0, pos, LABELS.index(labels[tok_id - 1000])] = 5.0
            return [logits]

    n.session = Session()
    if window:
        monkeypatch.setattr(ner_mod, "WINDOW", window)
    return n, calls


TEXT = "Ali Veli İzmir'de Acme Ltd ile 2024'te"


def _toks():
    #           metin     baş son kelime
    return [("Ali", 0, 3, 0), ("Ve", 4, 6, 1), ("##li", 6, 8, 1), ("İzmir", 9, 14, 2),
            ("'", 14, 15, 3), ("de", 15, 17, 4), ("Acme", 18, 22, 5), ("Ltd", 23, 26, 6),
            ("ile", 27, 30, 7), ("2024", 31, 35, 8), ("'", 35, 36, 9), ("te", 36, 38, 10)]


def test_bio_first_subtoken_and_punct_trim() -> None:
    labels = ["B-PER", "I-PER", "B-ORG",  # Veli'nin 2. alt-token etiketi (B-ORG) yok sayılır
              "B-LOC", "I-LOC",           # "'" I-LOC ile uzar → uçtaki noktalama atılır
              "O", "B-ORG", "I-ORG", "O", "B-DATE", "I-DATE", "I-DATE"]
    n, _ = _make(_toks(), labels)
    assert n.extract(TEXT) == [
        {"type": "person", "value": "Ali Veli"},
        {"type": "location", "value": "İzmir"},
        {"type": "org", "value": "Acme Ltd"},
    ]


def test_i_tag_after_other_type_starts_new_entity() -> None:
    labels = ["B-PER", "I-ORG", "I-ORG", "I-LOC", "O", "O", "I-ORG", "B-ORG", "O", "O", "O", "O"]
    n, _ = _make(_toks(), labels)
    assert n.extract(TEXT) == [
        {"type": "person", "value": "Ali"},
        {"type": "org", "value": "Veli"},
        {"type": "location", "value": "İzmir"},
        {"type": "org", "value": "Acme"},   # B-ORG yeni varlık başlatır (Ltd ayrı)
        {"type": "org", "value": "Ltd"},
    ]


def test_short_values_dropped_and_windowing(monkeypatch) -> None:
    labels = ["B-PER"] + ["O"] * 11
    toks = _toks()
    toks[0] = ("A", 0, 1, 0)  # tek karakterlik varlık atılır
    n, calls = _make(toks, labels, monkeypatch, window=5)
    assert n.extract("A" + TEXT[1:]) == []
    assert calls == [5, 5, 2]  # 12 token, 5'lik pencereler


def test_empty_and_whitespace() -> None:
    n, calls = _make([], [])
    assert n.extract("   ") == [] and n.extract("") == [] and calls == []


@pytest.mark.parametrize("raw,want", [("'İzmir'", "İzmir"), ("«NATO»,", "NATO"), ("  a-b  ", "a-b"), ("...", "")])
def test_trim_punct(raw, want) -> None:
    assert _trim_punct(raw) == want


def test_available(tmp_path) -> None:
    assert OnnxNer.available(tmp_path) is False
