"""Veri saklama ve silme (KVKK) — docs/SOZLESME.md §6, migration 004.

Süre kuralı: kaynağın ``retention_days`` değeri (NULL → varsayılan, 0 → hiç
silme, N → N gün); kaynaksız öğelere varsayılan uygulanır. Varsayılan
``OSIRIS_RETENTION_DAYS`` (180; 0 → varsayılan kapalı).

DBAPI-uyumlu bağlantı alır (psycopg); toplu silme sınırlı partilerle yapılır.
"""

from __future__ import annotations

import logging
import os
from typing import Any

logger = logging.getLogger(__name__)

DEFAULT_RETENTION_DAYS = 180
_MAX_DAYS = 36_500


def default_retention_days() -> int:
    """``OSIRIS_RETENTION_DAYS`` (0-36500); geçersizse 180."""
    raw = os.getenv("OSIRIS_RETENTION_DAYS", "")
    if raw == "":
        return DEFAULT_RETENTION_DAYS
    try:
        days = int(raw)
    except ValueError:
        logger.warning("OSIRIS_RETENTION_DAYS geçersiz (%r); %d kullanılıyor",
                       raw, DEFAULT_RETENTION_DAYS)
        return DEFAULT_RETENTION_DAYS
    return max(0, min(days, _MAX_DAYS))


def has_column(cur: Any, table: str, column: str) -> bool:
    cur.execute(
        "SELECT 1 FROM information_schema.columns WHERE table_name = %s AND column_name = %s",
        (table, column),
    )
    return cur.fetchone() is not None


_PURGE_SQL = """
    DELETE FROM items WHERE id IN (
        SELECT i.id FROM items i
        LEFT JOIN sources s ON s.id = i.source_id
        WHERE COALESCE(s.retention_days, %(default)s) > 0
          AND i.collected_at < NOW() - make_interval(days => COALESCE(s.retention_days, %(default)s))
        LIMIT %(batch)s
    )
"""
# Migration 004 uygulanmamışsa: yalnızca varsayılan süre
_PURGE_SQL_DEFAULT_ONLY = """
    DELETE FROM items WHERE id IN (
        SELECT id FROM items
        WHERE %(default)s > 0
          AND collected_at < NOW() - make_interval(days => %(default)s)
        LIMIT %(batch)s
    )
"""
_ORPHAN_ENTITIES_SQL = """
    DELETE FROM entities e
    WHERE NOT EXISTS (SELECT 1 FROM item_entities ie WHERE ie.entity_id = e.id)
"""


def purge_orphan_entities(cur: Any) -> int:
    """Hiçbir öğeye bağlı olmayan varlıkları siler (PII kalıntısı bırakmaz)."""
    cur.execute(_ORPHAN_ENTITIES_SQL)
    return max(0, cur.rowcount or 0)


def purge_expired(conn: Any, default_days: int | None = None, batch: int = 1000,
                  max_batches: int = 1000) -> dict[str, int]:
    """Süresi dolan öğeleri ve yetim varlıkları siler. Silinen sayıları döner."""
    days = default_retention_days() if default_days is None else max(0, min(int(default_days), _MAX_DAYS))
    batch = max(1, min(int(batch), 10_000))
    deleted = 0
    with conn.cursor() as cur:
        sql = _PURGE_SQL if has_column(cur, "sources", "retention_days") else _PURGE_SQL_DEFAULT_ONLY
        for _ in range(max_batches):
            cur.execute(sql, {"default": days, "batch": batch})
            n = max(0, cur.rowcount or 0)
            conn.commit()  # kısa işlemler: uzun kilit tutma
            deleted += n
            if n < batch:
                break
        entities = purge_orphan_entities(cur)
        conn.commit()
    if deleted or entities:
        logger.info("Saklama temizliği: %d öğe, %d yetim varlık silindi", deleted, entities)
    return {"items_deleted": deleted, "entities_deleted": entities, "default_days": days}
