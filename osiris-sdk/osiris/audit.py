"""Denetim logları — append-only, hash zincirli (doküman §10.4, docs/SOZLESME.md §2).

`audit_logs` tablosuna yazar; her kayıt bir öncekinin SHA-256 özetini içerir.
psycopg bağımlılığı almamak için DBAPI-uyumlu bir bağlantı (duck-typing)
kabul eder: yalnızca ``.cursor()`` + context manager protokolü gerekir.

Zincir sürümleri (kayıt başına ``chain_version`` sütunu):
- **1** — ``json.dumps(sort_keys, separators=(",", ":"), ensure_ascii=False)``
- **2** — RFC 8785 (JCS) kanonik JSON; diller arası (Rust) birebir üretilebilir
Her iki sürümde: ``hash = sha256(utf8(prev + "|" + payload))``, ilk kayıtta
``prev = "GENESIS"``. Doğrulayıcı iki sürümü de tanır.
"""

from __future__ import annotations

import hashlib
import json
import logging
from typing import Any, Protocol

import rfc8785

from osiris._native import native as _rust

logger = logging.getLogger(__name__)

GENESIS = "GENESIS"
CURRENT_CHAIN_VERSION = 2
# JCS (I-JSON) tamsayı alanı: ±(2^53 - 1); dışındakiler dize olarak saklanır
_MAX_SAFE_INT = 2**53 - 1


class _Cursor(Protocol):
    def execute(self, query: str, params: Any = ...) -> Any: ...
    def fetchone(self) -> Any: ...


class _Connection(Protocol):
    def cursor(self) -> Any: ...
    def commit(self) -> None: ...


def _record(user_id: str | None, action: str, resource: str | None,
            detail: dict[str, Any] | None) -> dict[str, Any]:
    return {
        "user_id": user_id,
        "action": action,
        "resource": resource,
        "detail": detail or {},
    }


def canonical_payload(user_id: str | None, action: str, resource: str | None,
                      detail: dict[str, Any] | None, chain_version: int) -> str:
    """Kaydın hash'e giren kanonik dizgesi (sürüme göre). Varsayılan: Rust (osiris._native)."""
    if _rust is None:
        return _py_canonical_payload(user_id, action, resource, detail, chain_version)
    if chain_version == 1:
        # v1 json.dumps(default=str) idi: JSON dışı nesneler önce dizeye çevrilir
        detail = json.loads(json.dumps(detail, default=str))
    return _rust.audit_canonical_payload(user_id, action, resource, detail, chain_version)


def _py_canonical_payload(user_id: str | None, action: str, resource: str | None,
                          detail: dict[str, Any] | None, chain_version: int) -> str:
    record = _record(user_id, action, resource, detail)
    if chain_version == 1:
        return json.dumps(record, ensure_ascii=False, sort_keys=True,
                          separators=(",", ":"), default=str)
    if chain_version == 2:
        return rfc8785.dumps(_jsonb_numbers(record)).decode("utf-8")
    raise ValueError(f"Bilinmeyen chain_version: {chain_version!r}")


def chain_hash(prev: str | None, payload: str) -> str:
    """``sha256(prev | payload)`` — prev None ise GENESIS."""
    if _rust is None:
        return _py_chain_hash(prev, payload)
    return _rust.audit_chain_hash(prev, payload)


def _py_chain_hash(prev: str | None, payload: str) -> str:
    anchor = prev if prev else GENESIS
    return hashlib.sha256(f"{anchor}|{payload}".encode()).hexdigest()


# Geriye uyumluluk (eski iç ad): v1 kanonik biçim
def _canonical(user_id: str | None, action: str, resource: str | None,
               detail: dict[str, Any] | None) -> str:
    return canonical_payload(user_id, action, resource, detail, 1)


def _jcs_safe(value: Any) -> Any:
    """JCS'nin kabul etmediği tamsayıları (|n| > 2^53-1) dizeye çevirir."""
    if isinstance(value, bool) or value is None:
        return value
    if isinstance(value, int):
        return value if -_MAX_SAFE_INT <= value <= _MAX_SAFE_INT else str(value)
    if isinstance(value, dict):
        return {k: _jcs_safe(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_jcs_safe(v) for v in value]
    return value


def _jsonb_numbers(value: Any) -> Any:
    """Güvenli aralık dışındaki tamsayıları float'a çevirir (yalnızca kanonikleştirmede).

    Yazarken bu tamsayılar dizeye çevrildiği için (bkz. ``_jcs_safe``) burada
    görülen büyük tamsayı ancak jsonb'nin büyük bir float'ı (örn. ``1e+16``)
    tamsayı olarak geri vermesinden gelir; float'a dönmek aynı JCS çıktısını verir.
    """
    if isinstance(value, bool) or value is None:
        return value
    if isinstance(value, int):
        return value if -_MAX_SAFE_INT <= value <= _MAX_SAFE_INT else float(value)
    if isinstance(value, dict):
        return {k: _jsonb_numbers(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_jsonb_numbers(v) for v in value]
    return value


def _normalize(user_id: str | None, resource: str | None,
               detail: dict[str, Any] | None) -> tuple[str, str, dict[str, Any]]:
    """Saklanan değerlerle kanonik yükü aynı yapar (kesme ÖNCE yapılır)."""
    try:
        # allow_nan=False: NaN/Infinity ne jsonb'ye ne JCS'ye sığar
        detail_json = json.loads(json.dumps(detail or {}, ensure_ascii=False,
                                            default=str, allow_nan=False))
    except (TypeError, ValueError):
        detail_json = {"_unserializable": True}
    if not isinstance(detail_json, dict):
        detail_json = {"value": detail_json}
    return (user_id or "")[:200], (resource or "")[:500], _jcs_safe(detail_json)


def _close(cur: Any) -> None:
    close = getattr(cur, "close", None)
    if callable(close):
        close()


def _has_chain_version_column(cur: Any) -> bool:
    """Migration 003 uygulanmış mı? (Uygulanmadıysa v1 yazılır — kayıt kaybolmaz.)"""
    try:
        cur.execute(
            "SELECT 1 FROM information_schema.columns"
            " WHERE table_name = 'audit_logs' AND column_name = 'chain_version'"
        )
        return cur.fetchone() is not None
    except Exception as exc:  # noqa: BLE001
        logger.debug("chain_version sütunu sorgulanamadı: %s", exc)
        return False


def append_audit(conn: _Connection, user_id: str | None, action: str,
                 resource: str | None = None,
                 detail: dict[str, Any] | None = None) -> dict[str, Any]:
    """Zincire yeni bir denetim kaydı ekler. ``{'prev_hash', 'hash', 'chain_version'}`` döner."""
    if not action or len(action) > 200:
        raise ValueError("action 1-200 karakter olmalı")
    if resource is not None and len(resource) > 500:
        raise ValueError("resource çok uzun")
    user_id, resource, detail = _normalize(user_id, resource, detail)

    cur = conn.cursor()
    try:
        # Eşzamanlı yazıcılar zinciri çatallamasın (PostgreSQL'de işlem-kapsamlı
        # kilit; desteklemeyen sürücülerde sessizce atlanır).
        try:
            cur.execute("SELECT pg_advisory_xact_lock(hashtext('osiris_audit_chain'))")
        except Exception as exc:  # noqa: BLE001
            logger.debug("Danışma kilidi atlandı (PG dışı?): %s", exc)
        version = CURRENT_CHAIN_VERSION if _has_chain_version_column(cur) else 1
        cur.execute("SELECT hash FROM audit_logs ORDER BY id DESC LIMIT 1")
        row = cur.fetchone()
        prev_hash = row[0] if row and row[0] else GENESIS
        payload = canonical_payload(user_id, action, resource, detail, version)
        digest = chain_hash(prev_hash, payload)
        params: tuple = (
            user_id,
            action,
            resource,
            json.dumps(detail, ensure_ascii=False, default=str),
            None if prev_hash == GENESIS else prev_hash,
            digest,
        )
        if version == 1:
            cur.execute(
                """
                INSERT INTO audit_logs (user_id, action, resource, detail, prev_hash, hash)
                VALUES (%s, %s, %s, %s::jsonb, %s, %s)
                """,
                params,
            )
        else:
            cur.execute(
                """
                INSERT INTO audit_logs
                    (user_id, action, resource, detail, prev_hash, hash, chain_version)
                VALUES (%s, %s, %s, %s::jsonb, %s, %s, %s)
                """,
                (*params, version),
            )
    finally:
        _close(cur)
    conn.commit()
    return {"prev_hash": prev_hash, "hash": digest, "chain_version": version}


def _parse_detail(detail: Any) -> Any:
    if isinstance(detail, str):
        try:
            return json.loads(detail)
        except ValueError:
            return {}
    return detail


def verify_records(rows: list[tuple]) -> dict[str, Any]:
    """Artan id sıralı kayıtları doğrular (saf fonksiyon — fixture'lar da kullanır).

    Satır: ``(id, user_id, action, resource, detail, prev_hash, hash[, chain_version])``.
    İlk satırın beyan ettiği öncül kabul edilir (pencere başı).
    """
    if _rust is None:
        return _py_verify_records(rows)
    return _rust.audit_verify_records(
        [(*row[:4], _parse_detail(row[4]), *row[5:]) for row in rows])


def _py_verify_records(rows: list[tuple]) -> dict[str, Any]:
    if not rows:
        return {"ok": True, "checked": 0}
    prev: str | None = rows[0][5]
    checked = 0
    for row in rows:
        _rid, user_id, action, resource, detail, prev_hash, digest = row[:7]
        version = row[7] if len(row) > 7 and row[7] is not None else 1
        if prev_hash != prev:
            return {"ok": False, "checked": checked, "error": "prev_hash uyuşmazlığı"}
        detail = _parse_detail(detail)
        try:
            payload = _py_canonical_payload(user_id, action, resource, detail, int(version))
        except (ValueError, TypeError) as exc:
            return {"ok": False, "checked": checked, "error": f"kanonikleştirme: {exc}"}
        if _py_chain_hash(prev, payload) != digest:
            return {"ok": False, "checked": checked, "error": "hash uyuşmazlığı"}
        prev = digest
        checked += 1
    return {"ok": True, "checked": checked}


def verify_chain(conn: _Connection, limit: int = 1000) -> dict[str, Any]:
    """Son N kaydın zincir bütünlüğünü doğrular.

    NOT: ilk kaydın öncülü GENESIS sayılır; tablonun başından değil sonundan
    N kayıt alınır, ancak ilk N kayıttan öncesi doğrulanamazsa `truncated`
    döner (zincirin görünen kısmı tutarlıysa ok=True).
    """
    limit = max(1, min(int(limit), 100_000))
    cur = conn.cursor()
    try:
        cols = "id, user_id, action, resource, detail, prev_hash, hash"
        if _has_chain_version_column(cur):
            cols += ", chain_version"
        cur.execute(
            f"SELECT {cols} FROM audit_logs ORDER BY id DESC LIMIT %s",  # nosec B608 — sabit sütunlar
            (limit,),
        )
        rows = list(reversed(cur.fetchall()))
    finally:
        _close(cur)
    if not rows:
        return {"ok": True, "checked": 0}
    # Pencere tablo başını içermiyorsa ilk kaydın öncülü bilinemez
    cur2 = conn.cursor()
    try:
        cur2.execute("SELECT min(id) FROM audit_logs")
        min_row = cur2.fetchone()
    finally:
        _close(cur2)
    truncated = bool(min_row and min_row[0] is not None and rows[0][0] != min_row[0])
    result = verify_records(rows)
    if result["ok"]:
        result["truncated"] = truncated
    return result
