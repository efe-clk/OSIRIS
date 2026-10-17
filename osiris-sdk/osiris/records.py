"""Plugin kayıtlarının `raw_content` biçimi (R5, docs/SOZLESME.md §4).

JSON kaynaklı plugin'ler (rest-api, shodan) kaydı **kanonik JSON** olarak yazar:
RFC 8785 anahtar sırası (UTF-16) ve sayı/dize biçimi; ancak tamsayılar her büyüklükte
tam ondalık yazılır, sonlu olmayan float ``null`` olur. Dize kayıt olduğu gibi kalır.
Önceki biçim Python ``str(dict)`` idi (diller arası üretilemez).
"""

from __future__ import annotations

import math
from typing import Any

import rfc8785

from osiris._native import native as _rust


def record_json(value: Any) -> str:
    """Kaydın kanonik metni (varsayılan: Rust)."""
    if _rust is None:
        return _py_record_json(value)
    return _rust.record_json(value)


def _py_record_json(value: Any) -> str:
    if isinstance(value, str):
        return value
    out: list[str] = []
    _write(value, out)
    return "".join(out)


def _write(value: Any, out: list[str]) -> None:
    if value is None or isinstance(value, bool):
        out.append({None: "null", True: "true", False: "false"}[value])
    elif isinstance(value, int):
        out.append(str(value))
    elif isinstance(value, float):
        out.append("null" if not math.isfinite(value) else rfc8785.dumps(value).decode())
    elif isinstance(value, str):
        out.append(rfc8785.dumps(value).decode())
    elif isinstance(value, (list, tuple)):
        out.append("[")
        for i, v in enumerate(value):
            if i:
                out.append(",")
            _write(v, out)
        out.append("]")
    elif isinstance(value, dict):
        if not all(isinstance(k, str) for k in value):
            raise TypeError("object keys must be strings")
        out.append("{")
        for i, k in enumerate(sorted(value, key=lambda k: k.encode("utf-16be"))):
            if i:
                out.append(",")
            out.append(rfc8785.dumps(k).decode())
            out.append(":")
            _write(value[k], out)
        out.append("}")
    else:
        raise TypeError(f"unsupported type: {type(value).__name__}")
