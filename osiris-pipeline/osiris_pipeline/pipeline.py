"""Processing Pipeline — aşamalı veri işleme.

Aşamalar (doküman §5.3):
1. Temizleme    — HTML soyutlama, boşluk normalizasyonu
2. Dil Tespiti  — otomatik dil etiketleme
3. Varlık Çıkarma (NER)
4. Sınıflandırma
5. Embedding    — semantik vektör (opsiyonel, OSIRIS_EMBEDDING_MODEL)
6. İlişkilendirme
7. Depolama     — PostgreSQL'e yazma
"""

from __future__ import annotations

import hashlib
import ipaddress
import json
import logging
import os
import re
from typing import Any

import redis
from bs4 import BeautifulSoup
from langdetect import detect

logger = logging.getLogger(__name__)

_WHITESPACE_RE = re.compile(r"\s+")
_EMAIL_RE = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
_IP_RE = re.compile(r"\b(?:\d{1,3}\.){3}\d{1,3}\b")
_DOMAIN_RE = re.compile(r"\b(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,}\b")
_PHONE_RE = re.compile(r"\+?\d[\d\s\-().]{7,}\d")
_CRYPTO_RE = re.compile(r"\b(bc1|[13])[a-zA-HJ-NP-Z0-9]{25,39}\b|\b0x[a-fA-F0-9]{40}\b")
_CVE_RE = re.compile(r"\bCVE-\d{4}-\d{4,7}\b", re.IGNORECASE)

_MAX_CONTENT_CHARS = 200_000
_UUID_RE = re.compile(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
_VALID_ENTITY_TYPES = {
    "person", "org", "location", "ip", "domain", "email",
    "phone", "crypto_address", "hash", "username", "cve", "custom",
}

# Basit konu sınıflandırma (Faz 3: ML modeline yükseltilecek)
_TOPIC_KEYWORDS = {
    "threat": ["saldırı", "attack", "exploit", "malware", "cve", "fidye", "ransomware"],
    "breach": ["sızıntı", "leak", "breach", "ifşa", "dump"],
    "vulnerability": ["zafiyet", "vulnerability", "patch", "yama"],
    "phishing": ["oltalama", "phishing", "dolandırıcılık", "scam"],
}


class ProcessingPipeline:
    """Kuyruktan ham veriyi alır, işler ve veritabanına yazar."""

    def __init__(
        self,
        redis_url: str = "redis://localhost:6379/0",
        queue_name: str = "osiris:raw_items",
        database_url: str | None = None,
        embedding_model: str | None = None,
    ) -> None:
        self.redis = redis.Redis.from_url(redis_url, decode_responses=True)
        self.queue_name = queue_name
        self.database_url = database_url
        self.embedding_model_name = embedding_model or os.getenv("OSIRIS_EMBEDDING_MODEL")
        self._nlp = None
        self._embedder = None
        self._embedding_usable: bool | None = None  # pgvector boyut uyumu önbelleği

    @property
    def nlp(self):
        """spaCy modelini tembel yükleme (yoksa None, regex'e düşer)."""
        if self._nlp is None:
            try:
                import spacy

                try:
                    self._nlp = spacy.load("xx_ent_wiki_sm")
                except OSError:
                    # Model kurulu değilse boş pipeline (regex NER yeterli)
                    self._nlp = spacy.blank("xx")
            except ImportError:
                self._nlp = False
        return self._nlp or None

    def process_one(self, raw: str) -> dict[str, Any]:
        """Tek bir ham kaydı tüm aşamalardan geçirir."""
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError as exc:
            raise ValueError(f"Geçersiz JSON: {exc}") from exc
        if not isinstance(payload, dict):
            raise ValueError("Payload dict olmalı")
        item = payload.get("item")
        plugin_id = payload.get("plugin_id")
        if not isinstance(item, dict) or not plugin_id:
            raise ValueError("Payload 'item' ve 'plugin_id' içermeli")

        cleaned = self.clean(str(item.get("raw_content", "")))
        language = self.detect_language(cleaned)
        entities = self.extract_entities(cleaned)
        topics = self.classify(cleaned)
        content_hash = hashlib.sha256(cleaned.encode()).hexdigest()
        embedding = self.generate_embedding(cleaned)

        # Mesaj üst düzeyindeki source_id öncelikli; yalnızca geçerli UUID kabul edilir
        sid = payload.get("source_id") or item.get("source_id")
        source_id = sid if isinstance(sid, str) and _UUID_RE.match(sid) else None

        return {
            "plugin_id": str(plugin_id),
            "item": {
                **item,
                "source_id": source_id,
                "cleaned_content": cleaned,
                "language": language,
                "content_hash": content_hash,
            },
            "entities": entities,
            "topics": topics,
            "embedding": embedding,
        }

    def clean(self, text: str) -> str:
        """HTML soyutlama ve boşluk normalizasyonu (DoS korumalı)."""
        if not text:
            return ""
        text = text[:_MAX_CONTENT_CHARS]
        try:
            soup = BeautifulSoup(text, "html.parser")
            plain = soup.get_text(" ", strip=True)
        except Exception:  # noqa: BLE001
            plain = text
        return _WHITESPACE_RE.sub(" ", plain).strip()[:_MAX_CONTENT_CHARS]

    def detect_language(self, text: str) -> str:
        """Dil tespiti."""
        if not text:
            return "unknown"
        try:
            return detect(text[:500])
        except Exception:  # noqa: BLE001
            return "unknown"

    def extract_entities(self, text: str) -> list[dict[str, Any]]:
        """Varlık çıkarma: email, IP, domain, telefon, kripto, CVE + spaCy NER."""
        entities: list[dict[str, Any]] = []
        seen: set[tuple[str, str]] = set()

        def _add(etype: str, value: str) -> None:
            v = value.strip()[:500]
            if not v or (etype, v.lower()) in seen:
                return
            seen.add((etype, v.lower()))
            entities.append({"type": etype, "value": v})

        for match in _EMAIL_RE.finditer(text):
            # Email içindeki domain'i ayrıca ekleme (çift sayımı önle)
            _add("email", match.group(0))
        email_spans = [m.span() for m in _EMAIL_RE.finditer(text)]
        ip_spans = [m.span() for m in _IP_RE.finditer(text)]
        cve_spans = [m.span() for m in _CVE_RE.finditer(text)]

        def _overlaps(span: tuple[int, int], spans: list[tuple[int, int]]) -> bool:
            return any(s <= span[0] < e or s < span[1] <= e for s, e in spans)
        for match in _IP_RE.finditer(text):
            try:
                ipaddress.ip_address(match.group(0))
            except ValueError:
                continue  # 999.999.999.999 gibi geçersiz IP'leri ele
            _add("ip", match.group(0))
        for match in _DOMAIN_RE.finditer(text):
            pos = match.start()
            if any(s <= pos < e for s, e in email_spans):
                continue
            _add("domain", match.group(0).lower())
        for match in _PHONE_RE.finditer(text):
            # IP ve CVE içindeki sayı gruplarını telefon sayma
            if _overlaps(match.span(), ip_spans) or _overlaps(match.span(), cve_spans):
                continue
            digits = re.sub(r"\D", "", match.group(0))
            if 7 <= len(digits) <= 15:
                _add("phone", match.group(0).strip())
        for match in _CRYPTO_RE.finditer(text):
            _add("crypto_address", match.group(0))
        for match in _CVE_RE.finditer(text):
            _add("cve", match.group(0).upper())

        nlp = self.nlp
        if nlp is not None:
            try:
                doc = nlp(text[:10000])
                for ent in doc.ents:
                    label = ent.label_.lower()
                    etype = label if label in _VALID_ENTITY_TYPES else "custom"
                    _add(etype, ent.text)
            except Exception:  # noqa: BLE001
                logger.debug("NER başarısız, regex sonuçları kullanılıyor")

        return entities

    def classify(self, text: str) -> list[str]:
        """Basit anahtar kelime tabanlı konu sınıflandırma."""
        lowered = text.lower()
        return [t for t, kws in _TOPIC_KEYWORDS.items() if any(k in lowered for k in kws)]

    @property
    def embedder(self):
        """Sentence-transformers modelini tembel yükleme (yoksa None)."""
        if self._embedder is None and self.embedding_model_name:
            try:
                from sentence_transformers import SentenceTransformer

                self._embedder = SentenceTransformer(self.embedding_model_name)
            except ImportError:
                logger.warning(
                    "sentence-transformers kurulu değil; embedding atlanıyor "
                    "(pip install 'osiris-pipeline[embedding]')"
                )
                self._embedder = False
            except Exception as exc:  # noqa: BLE001
                logger.warning("Embedding modeli yüklenemedi: %s", exc)
                self._embedder = False
        return self._embedder or None

    def generate_embedding(self, text: str) -> list[float] | None:
        """Semantik vektör üretir. Model yoksa None döner."""
        if not text or not text.strip():
            return None
        model = self.embedder
        if model is None:
            return None
        try:
            vec = model.encode(text[:8000], normalize_embeddings=True)
            floats = [float(x) for x in vec]
        except Exception as exc:  # noqa: BLE001
            logger.warning("Embedding üretilemedi: %s", exc)
            return None
        import math

        if not floats or not all(math.isfinite(x) for x in floats):
            logger.warning("Embedding geçersiz değer içeriyor, atlandı")
            return None
        return floats

    def run(self, batch_size: int = 10) -> int:
        """Kuyruktan batch halinde kayıt işler. İşlenen kayıt sayısını döndürür."""
        batch_size = max(1, min(int(batch_size), 100))
        processed = 0
        for _ in range(batch_size):
            try:
                raw = self.redis.lpop(self.queue_name)
            except redis.RedisError as exc:
                logger.error("Kuyruk okunamadı: %s", exc)
                break
            if raw is None:
                break
            try:
                result = self.process_one(raw)
            except ValueError as exc:
                logger.warning("Bozuk kayıt atlandı: %s", exc)
                continue
            try:
                self.store(result)
            except Exception as exc:  # noqa: BLE001
                logger.error("Depolama hatası: %s", exc)
                continue
            processed += 1
        return processed

    def store(self, result: dict[str, Any]) -> str | None:
        """İşlenmiş veriyi PostgreSQL'e yazar (upsert + dedup). Item id döner."""
        if not self.database_url:
            logger.debug("database_url yok, depolama atlandı")
            return None
        import psycopg

        item = result.get("item")
        if not isinstance(item, dict):
            raise ValueError("result 'item' içermeli")
        content_hash = item.get("content_hash")
        if not content_hash:
            raise ValueError("content_hash yok")
        topics = result.get("topics") or []
        if not isinstance(topics, list):
            topics = []
        raw_tags = item.get("tags") or []
        if not isinstance(raw_tags, list):
            raw_tags = []
        tags = list({str(t)[:100] for t in [*raw_tags, *topics]
                     if isinstance(t, (str, int, float))})[:50]
        embedding = result.get("embedding")
        if not isinstance(embedding, list) or not embedding:
            embedding = None
        entities = result.get("entities") or []
        if not isinstance(entities, list):
            entities = []

        with psycopg.connect(self.database_url) as conn:
            with conn.cursor() as cur:
                item_id = self._insert_item(cur, item, content_hash, tags, embedding)
                if item_id is None:
                    # Dedup: aynı hash zaten var
                    cur.execute(
                        "SELECT id::text FROM items WHERE content_hash = %s",
                        (content_hash,),
                    )
                    existing = cur.fetchone()
                    conn.commit()
                    return existing[0] if existing else None
                for ent in entities[:200]:
                    if not isinstance(ent, dict):
                        continue
                    etype = ent.get("type", "custom")
                    if etype not in _VALID_ENTITY_TYPES:
                        etype = "custom"
                    value = str(ent.get("value", ""))[:500]
                    if not value:
                        continue
                    cur.execute(
                        """
                        INSERT INTO entities (type, value, normalized_value, last_seen_at)
                        VALUES (%s, %s, %s, NOW())
                        ON CONFLICT (type, value)
                        DO UPDATE SET last_seen_at = NOW()
                        RETURNING id::text
                        """,
                        (etype, value, value.lower()),
                    )
                    entity_id = cur.fetchone()[0]
                    cur.execute(
                        """
                        INSERT INTO item_entities (item_id, entity_id, mention_count)
                        VALUES (%s::uuid, %s::uuid, 1)
                        ON CONFLICT (item_id, entity_id)
                        DO UPDATE SET mention_count = item_entities.mention_count + 1
                        """,
                        (item_id, entity_id),
                    )
            conn.commit()
        logger.info("Depolandı: %s (%s)", content_hash[:12], item_id)
        return item_id

    def _insert_item(self, cur: Any, item: dict[str, Any], content_hash: str,
                     tags: list[str], embedding: list[float] | None) -> str | None:
        """Item satırını yazar. Boyut uyumsuz embedding'i atlayıp vektörsüz yazar."""
        published = item.get("published_at") or ""
        if hasattr(published, "isoformat"):  # datetime/date nesnesi gelebilir
            published = published.isoformat()
        base_params: tuple = (
            item.get("source_id") or "",
            (item.get("raw_content") or "")[:_MAX_CONTENT_CHARS],
            (item.get("cleaned_content") or "")[:_MAX_CONTENT_CHARS],
            (item.get("url") or "")[:2048],
            (item.get("title") or "")[:500],
            (item.get("language") or "unknown")[:5],
            str(published)[:100],
            content_hash,
            json.dumps(item.get("metadata") or {}, ensure_ascii=False),
            tags,
        )
        if embedding is not None and self._embedding_usable is not False:
            vector_literal = "[" + ",".join(str(float(x)) for x in embedding) + "]"
            cur.execute("SAVEPOINT osiris_embedding")
            try:
                cur.execute(
                    """
                    INSERT INTO items
                        (source_id, raw_content, cleaned_content, url, title,
                         language, collected_at, published_at, content_hash, metadata, tags, embedding)
                    VALUES
                        ((SELECT id FROM sources WHERE id = NULLIF(%s,'')::uuid), %s, %s, %s, %s,
                         %s, NOW(), NULLIF(%s,'')::timestamptz, %s, %s::jsonb, %s, %s::vector)
                    ON CONFLICT (content_hash) DO NOTHING
                    RETURNING id::text
                    """,
                    (*base_params, vector_literal),
                )
                cur.execute("RELEASE SAVEPOINT osiris_embedding")
                self._embedding_usable = True
                row = cur.fetchone()
                return row[0] if row else None
            except Exception as exc:  # noqa: BLE001 — örn. pgvector boyut uyumsuzluğu
                cur.execute("ROLLBACK TO SAVEPOINT osiris_embedding")
                logger.warning(
                    "Embedding ile yazılamadı (%s); vektörsüz deneniyor",
                    type(exc).__name__,
                )
                self._embedding_usable = False
        cur.execute(
            """
            INSERT INTO items
                (source_id, raw_content, cleaned_content, url, title,
                 language, collected_at, published_at, content_hash, metadata, tags)
            VALUES
                ((SELECT id FROM sources WHERE id = NULLIF(%s,'')::uuid), %s, %s, %s, %s,
                 %s, NOW(), NULLIF(%s,'')::timestamptz, %s, %s::jsonb, %s)
            ON CONFLICT (content_hash) DO NOTHING
            RETURNING id::text
            """,
            base_params,
        )
        row = cur.fetchone()
        return row[0] if row else None
