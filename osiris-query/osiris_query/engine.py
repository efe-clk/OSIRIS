"""Query Engine — tam metin ve semantik arama.

Bkz. doküman §5.4.
"""

from __future__ import annotations

from typing import Any

import psycopg
from psycopg.rows import dict_row

_VALID_ENTITY_TYPES = {
    "person", "org", "location", "ip", "domain", "email",
    "phone", "crypto_address", "hash", "username", "cve", "custom",
}


class QueryEngine:
    """PostgreSQL full-text + pgvector semantik arama."""

    def __init__(self, database_url: str) -> None:
        if not database_url:
            raise ValueError("database_url gerekli")
        self.database_url = database_url

    def _connect(self) -> psycopg.Connection:
        return psycopg.connect(self.database_url, row_factory=dict_row)

    @staticmethod
    def _validate_limit(limit: int) -> int:
        try:
            n = int(limit)
        except (TypeError, ValueError) as exc:
            raise ValueError("limit tamsayı olmalı") from exc
        if n < 1:
            raise ValueError("limit en az 1 olmalı")
        return min(n, 100)

    @staticmethod
    def _validate_query(query: str) -> str:
        if not isinstance(query, str) or not query.strip():
            raise ValueError("query boş olamaz")
        q = query.strip()
        if len(q) > 500:
            raise ValueError("query çok uzun (max 500)")
        return q

    def fulltext_search(self, query: str, limit: int = 20) -> list[dict[str, Any]]:
        """Tam metin arama (tsvector)."""
        query = self._validate_query(query)
        limit = self._validate_limit(limit)
        with self._connect() as conn:
            rows = conn.execute(
                """
                SELECT id::text AS id, title, url,
                       LEFT(cleaned_content, 2000) AS cleaned_content, collected_at
                FROM items
                WHERE to_tsvector('simple', COALESCE(cleaned_content,'')) @@ plainto_tsquery('simple', %s)
                ORDER BY ts_rank(to_tsvector('simple', COALESCE(cleaned_content,'')),
                                 plainto_tsquery('simple', %s)) DESC
                LIMIT %s
                """,
                (query, query, limit),
            ).fetchall()
        return list(rows)

    def semantic_search(self, embedding: list[float], limit: int = 20) -> list[dict[str, Any]]:
        """pgvector kosinüs benzerliği ile semantik arama."""
        limit = self._validate_limit(limit)
        if not isinstance(embedding, list) or not embedding:
            raise ValueError("embedding boş olamaz")
        if len(embedding) > 4096 or not all(isinstance(x, (int, float)) for x in embedding):
            raise ValueError("embedding geçersiz")
        vector_literal = "[" + ",".join(str(float(x)) for x in embedding) + "]"
        # R3/K5: 384 boyut → embedding_v2 (fastembed); diğerleri eski `embedding` sütunu.
        # Sütun adı yalnızca bu iki sabitten seçilir (kullanıcı girdisi değil).
        column = "embedding_v2" if len(embedding) == 384 else "embedding"
        with self._connect() as conn:
            rows = conn.execute(
                f"""
                SELECT id::text AS id, title, url,
                       LEFT(cleaned_content, 2000) AS cleaned_content, collected_at,
                       1 - ({column} <=> %s::vector) AS similarity
                FROM items
                WHERE {column} IS NOT NULL
                ORDER BY {column} <=> %s::vector
                LIMIT %s
                """,  # nosec B608 — sabit sütun adı
                (vector_literal, vector_literal, limit),
            ).fetchall()
        return list(rows)

    def entity_search(self, entity_type: str, value: str, limit: int = 20) -> list[dict[str, Any]]:
        """Varlık bazlı arama."""
        if entity_type not in _VALID_ENTITY_TYPES:
            raise ValueError(f"Geçersiz entity_type: {entity_type!r}")
        if not isinstance(value, str) or not value.strip() or len(value) > 500:
            raise ValueError("value geçersiz")
        limit = self._validate_limit(limit)
        # ILIKE jokerlerini etkisizleştir
        escaped = value.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_")
        with self._connect() as conn:
            rows = conn.execute(
                """
                SELECT i.id::text AS id, i.title, i.url,
                       LEFT(i.cleaned_content, 2000) AS cleaned_content
                FROM items i
                JOIN item_entities ie ON ie.item_id = i.id
                JOIN entities e ON e.id = ie.entity_id
                WHERE e.type = %s AND e.value ILIKE %s ESCAPE '\\'
                LIMIT %s
                """,
                (entity_type, f"%{escaped}%", limit),
            ).fetchall()
        return list(rows)
