"""ONNX NER — kişi / kurum / yer çıkarma (R3/K4, docs/SOZLESME.md §11).

Model: onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX (`onnx/model_int8.onnx`),
`models/download.py` ile sabit revizyondan indirilir. Çalışma zamanı: onnxruntime (CPU)
+ HF tokenizers. Rust (R5) aynı model, aynı tokenizer ve AŞAĞIDAKİ birleştirme
kurallarıyla aynı sonucu üretmelidir:

1. Metin: ilk ``MAX_CHARS`` kod noktası.
2. Tokenize: ``tokenizer.json``, özel tokensız; ``WINDOW`` (510) token'lık ardışık
   pencereler, her biri ``[CLS] … [SEP]`` ile; ``token_type_ids = 0``, ``attention_mask = 1``.
3. Etiket: token başına ``argmax(logits)`` → ``config.json`` ``id2label``.
4. Kelime etiketi = kelimenin İLK alt-token'ının etiketi ("first" stratejisi).
5. Kelimeler soldan sağa: ``B-X`` yeni varlık başlatır; ``I-X`` önceki kelime aynı tipte
   bir varlığa aitse onu uzatır, değilse yeni varlık başlatır; ``O`` varlığı kapatır.
6. Varlık metni = özgün metinde ilk kelimenin başlangıcından son kelimenin sonuna;
   iki uçtan boşluk (Python ``str.strip``) ve Unicode noktalama (genel kategori ``P*``)
   karakterleri atılır; 2 kod noktasından kısa olanlar atılır. ``DATE`` atılır;
   PER→person, ORG→org, LOC→location.
"""

from __future__ import annotations

import json
import logging
import unicodedata
from pathlib import Path

logger = logging.getLogger(__name__)

MODEL_SUBDIR = "ner/bert-base-multilingual-cased-ner-hrl"
MAX_CHARS = 10_000
WINDOW = 510
_TYPE_MAP = {"PER": "person", "ORG": "org", "LOC": "location"}


def _trim_punct(value: str) -> str:
    """Uçlardaki boşluk ve noktalamayı (Unicode P*) atar: "İzmir'" → "İzmir"."""
    value = value.strip()
    start, end = 0, len(value)
    while start < end and (unicodedata.category(value[start]).startswith("P") or value[start].isspace()):
        start += 1
    while end > start and (unicodedata.category(value[end - 1]).startswith("P") or value[end - 1].isspace()):
        end -= 1
    return value[start:end]


class OnnxNer:
    """onnxruntime oturumu + tokenizer; ``extract`` iş parçacığı güvenlidir."""

    def __init__(self, model_dir: Path) -> None:
        import onnxruntime as ort
        from tokenizers import Tokenizer

        model_dir = Path(model_dir)
        opts = ort.SessionOptions()
        opts.intra_op_num_threads = 1  # deterministik ve işçi başına öngörülebilir CPU
        self.session = ort.InferenceSession(str(model_dir / "onnx" / "model_int8.onnx"),
                                            sess_options=opts,
                                            providers=["CPUExecutionProvider"])
        self.tokenizer = Tokenizer.from_file(str(model_dir / "tokenizer.json"))
        config = json.loads((model_dir / "config.json").read_text(encoding="utf-8"))
        self.id2label = {int(k): v for k, v in config["id2label"].items()}
        self.cls_id = self.tokenizer.token_to_id("[CLS]")
        self.sep_id = self.tokenizer.token_to_id("[SEP]")

    @classmethod
    def available(cls, root: Path) -> bool:
        d = Path(root) / MODEL_SUBDIR
        return all((d / f).is_file() for f in ("onnx/model_int8.onnx", "tokenizer.json", "config.json"))

    def _labels(self, ids: list[int]) -> list[str]:
        import numpy as np

        seq = [self.cls_id, *ids, self.sep_id]
        feed = {
            "input_ids": np.array([seq], dtype=np.int64),
            "attention_mask": np.ones((1, len(seq)), dtype=np.int64),
            "token_type_ids": np.zeros((1, len(seq)), dtype=np.int64),
        }
        logits = self.session.run(["logits"], feed)[0][0]
        return [self.id2label[int(i)] for i in logits.argmax(axis=-1)[1:-1]]

    def extract(self, text: str) -> list[dict[str, str]]:
        """``[{"type": "person"|"org"|"location", "value": str}]`` — metindeki sırayla."""
        text = (text or "")[:MAX_CHARS]
        if not text.strip():
            return []
        enc = self.tokenizer.encode(text, add_special_tokens=False)
        ids, offsets, word_ids = enc.ids, enc.offsets, enc.word_ids
        labels: list[str] = []
        for start in range(0, len(ids), WINDOW):
            labels.extend(self._labels(ids[start:start + WINDOW]))

        # (4) kelime düzeyi: ilk alt-token'ın etiketi, kelimenin tüm karakter aralığı
        words: list[tuple[str, int, int]] = []  # (etiket, başlangıç, bitiş)
        prev_word = None
        for label, (s, e), wid in zip(labels, offsets, word_ids, strict=True):
            if wid is None:
                continue
            if wid != prev_word:
                words.append((label, s, e))
                prev_word = wid
            else:
                lab, ws, _ = words[-1]
                words[-1] = (lab, ws, e)

        # (5) BIO birleştirme
        spans: list[tuple[str, int, int]] = []
        current: tuple[str, int, int] | None = None
        for label, s, e in words:
            tag, _, etype = label.partition("-")
            if label == "O" or etype not in _TYPE_MAP:
                if current:
                    spans.append(current)
                current = None
                continue
            if tag == "I" and current and current[0] == etype:
                current = (etype, current[1], e)
                continue
            if current:
                spans.append(current)
            current = (etype, s, e)
        if current:
            spans.append(current)

        out = []
        for etype, s, e in spans:
            value = _trim_punct(text[s:e])
            if len(value) >= 2:
                out.append({"type": _TYPE_MAP[etype], "value": value})
        return out
