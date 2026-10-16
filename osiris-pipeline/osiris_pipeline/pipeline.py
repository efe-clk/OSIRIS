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
import math
import os
import re
import socket
from pathlib import Path
from typing import Any

import redis
from bs4 import BeautifulSoup
from langdetect import detect
from osiris import queue as osiris_queue

logger = logging.getLogger(__name__)

_WHITESPACE_RE = re.compile(r"\s+")
_EMAIL_RE = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
_IP_RE = re.compile(r"\b(?:\d{1,3}\.){3}\d{1,3}\b")
_DOMAIN_RE = re.compile(r"\b(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,}\b", re.IGNORECASE)
_PHONE_RE = re.compile(r"\+?\d[\d\s\-().]{7,}\d")
_CRYPTO_RE = re.compile(r"\b(bc1|[13])[a-zA-HJ-NP-Z0-9]{25,39}\b|\b0x[a-fA-F0-9]{40}\b")
_CVE_RE = re.compile(r"\bCVE-\d{4}-\d{4,7}\b", re.IGNORECASE)

_MAX_CONTENT_CHARS = 200_000
# Embedding (R3/K5): fastembed ONNX; sütun embedding_v2 VECTOR(384) (migration 005)
DEFAULT_EMBEDDING_MODEL = "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2"
EMBEDDING_DIM = 384
_MAX_EMBED_CHARS = 8000
_LOCAL_EMBEDDING_DIR = "embedding/paraphrase-multilingual-MiniLM-L12-v2-onnx-Q"


def model_dir() -> Path:
    """ONNX model kök dizini: OSIRIS_MODEL_DIR (varsayılan ./models)."""
    return Path(os.getenv("OSIRIS_MODEL_DIR", "models"))
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
        queue_name: str = osiris_queue.LEGACY_LIST,
        database_url: str | None = None,
        embedding_model: str | None = None,
        stream_name: str = osiris_queue.STREAM,
        consumer: str | None = None,
    ) -> None:
        self.redis = redis.Redis.from_url(redis_url, decode_responses=True)
        # queue_name: geçiş dönemi için eski LIST (önce boşaltılır); asıl kuyruk Stream
        self.queue_name = queue_name
        self.stream_name = stream_name
        self.group = osiris_queue.GROUP
        self.consumer = consumer or os.getenv("OSIRIS_PIPELINE_CONSUMER") or \
            f"{socket.gethostname()}-{os.getpid()}"
        self.claim_idle_ms = osiris_queue.DEFAULT_CLAIM_IDLE_MS
        self._group_ready = False
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
        crypto_spans = [m.span() for m in _CRYPTO_RE.finditer(text)]

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
            # IP, CVE ve kripto adresi içindeki sayı gruplarını telefon sayma
            if (_overlaps(match.span(), ip_spans) or _overlaps(match.span(), cve_spans)
                    or _overlaps(match.span(), crypto_spans)):
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
        """fastembed (ONNX) modelini tembel yükleme (yoksa None). Bkz. SOZLESME §10."""
        if self._embedder is None and self.embedding_model_name:
            try:
                from fastembed import TextEmbedding

                kwargs: dict[str, Any] = {}
                local = model_dir() / _LOCAL_EMBEDDING_DIR
                if self.embedding_model_name == DEFAULT_EMBEDDING_MODEL and \
                        (local / "model_optimized.onnx").is_file():
                    # models/download.py ile sabit revizyondan indirilmiş, doğrulanmış dosyalar
                    kwargs["specific_model_path"] = str(local)
                self._embedder = TextEmbedding(model_name=self.embedding_model_name,
                                               cache_dir=str(model_dir()), **kwargs)
            except ImportError:
                logger.warning(
                    "fastembed kurulu değil; embedding atlanıyor "
                    "(pip install 'osiris-pipeline[embedding]')"
                )
                self._embedder = False
            except Exception as exc:  # noqa: BLE001
                logger.warning("Embedding modeli yüklenemedi: %s", exc)
                self._embedder = False
        return self._embedder or None

    def generate_embedding(self, text: str) -> list[float] | None:
        """Semantik vektör üretir (L2-normalize). Model yoksa None döner."""
        if not text or not text.strip():
            return None
        model = self.embedder
        if model is None:
            return None
        try:
            vec = next(iter(model.embed([text[:_MAX_EMBED_CHARS]])))
            floats = [float(x) for x in vec]
        except Exception as exc:  # noqa: BLE001
            logger.warning("Embedding üretilemedi: %s", exc)
            return None
        if not floats or not all(math.isfinite(x) for x in floats):
            logger.warning("Embedding geçersiz değer içeriyor, atlandı")
            return None
        norm = math.sqrt(sum(x * x for x in floats))
        if norm == 0:
            return None
        return [x / norm for x in floats]

    def run(self, batch_size: int = 10) -> int:
        """Kuyruktan batch halinde kayıt işler. Depolanan kayıt sayısını döndürür.

        Sıra: (1) eski LIST'i boşalt (geçiş), (2) zaman aşımına uğramış bekleyen
        girdileri geri al (XAUTOCLAIM), (3) yeni girdileri oku (XREADGROUP).
        """
        batch_size = max(1, min(int(batch_size), 100))
        stored, consumed = self._drain_legacy(batch_size)
        remaining = batch_size - consumed
        if remaining <= 0:
            return stored
        try:
            if not self._group_ready:
                osiris_queue.ensure_group(self.redis, self.stream_name, self.group)
                self._group_ready = True
            claimed = self.redis.xautoclaim(
                self.stream_name, self.group, self.consumer,
                min_idle_time=self.claim_idle_ms, start_id="0-0", count=remaining,
            )
            reclaimed = list(claimed[1]) if claimed and len(claimed) > 1 else []
            for entry_id, fields in reclaimed:
                if fields is None:  # akıştan silinmiş ama bekleyen kalmış
                    self.redis.xack(self.stream_name, self.group, entry_id)
                    continue
                if self._deliveries(entry_id) > osiris_queue.MAX_DELIVERIES:
                    self._dead_letter(entry_id, fields, "max_deliveries")
                    continue
                stored += self._handle_entry(entry_id, fields)
            remaining -= len(reclaimed)
            if remaining > 0:
                resp = self.redis.xreadgroup(self.group, self.consumer,
                                             {self.stream_name: ">"}, count=remaining)
                for _stream, entries in resp or []:
                    for entry_id, fields in entries:
                        stored += self._handle_entry(entry_id, fields)
        except redis.RedisError as exc:
            logger.error("Akış okunamadı: %s", exc)
        return stored

    def _drain_legacy(self, limit: int) -> tuple[int, int]:
        """Eski LIST kuyruğunu boşaltır (geçiş uyumluluğu). (depolanan, tüketilen)."""
        stored = consumed = 0
        if not self.queue_name:
            return 0, 0
        for _ in range(limit):
            try:
                raw = self.redis.lpop(self.queue_name)
            except redis.RedisError as exc:
                logger.error("Eski kuyruk okunamadı: %s", exc)
                break
            if raw is None:
                break
            consumed += 1
            try:
                self.store(self.process_one(raw))
                stored += 1
            except ValueError as exc:
                logger.warning("Bozuk kayıt atlandı: %s", exc)
            except Exception as exc:  # noqa: BLE001
                logger.error("Depolama hatası (eski kuyruk; kayıt akışa taşındı): %s", exc)
                try:  # kaybolmasın: yeniden denenmek üzere akışa aktar
                    osiris_queue.enqueue(self.redis, raw, self.stream_name)
                except redis.RedisError:
                    logger.error("Kayıt akışa taşınamadı, kayboldu")
        return stored, consumed

    def _handle_entry(self, entry_id: str, fields: dict) -> int:
        """Tek akış girdisini işler. Başarı → 1 (ack+del); kalıcı hata → 0 (ack+del);
        geçici (depolama) hatası → 0, girdi bekleyen listesinde kalır."""
        raw = (fields or {}).get(osiris_queue.FIELD)
        try:
            result = self.process_one(raw if isinstance(raw, str) else "")
        except ValueError as exc:
            logger.warning("Bozuk kayıt atlandı (%s): %s", entry_id, exc)
            self._dead_letter(entry_id, fields or {}, f"invalid: {exc}"[:200])
            return 0
        try:
            self.store(result)
        except Exception as exc:  # noqa: BLE001 — yeniden denenecek (XAUTOCLAIM)
            logger.error("Depolama hatası (%s, yeniden denenecek): %s", entry_id, exc)
            return 0
        self._ack(entry_id)
        return 1

    def _ack(self, entry_id: str) -> None:
        self.redis.xack(self.stream_name, self.group, entry_id)
        self.redis.xdel(self.stream_name, entry_id)

    def _deliveries(self, entry_id: str) -> int:
        try:
            info = self.redis.xpending_range(self.stream_name, self.group,
                                             min=entry_id, max=entry_id, count=1)
            return int(info[0]["times_delivered"]) if info else 0
        except (redis.RedisError, KeyError, IndexError, TypeError, ValueError):
            return 0

    def _dead_letter(self, entry_id: str, fields: dict, reason: str) -> None:
        """İşlenemeyen girdiyi ölü-mektup akışına taşır ve asıl akıştan siler."""
        try:
            self.redis.xadd(osiris_queue.DEAD_LETTER,
                            {**{str(k): str(v) for k, v in fields.items()},
                             "source_id": str(entry_id), "reason": reason},
                            maxlen=10_000, approximate=True)
        finally:
            self._ack(entry_id)

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

    def reembed(self, batch: int = 64, limit: int | None = None) -> int:
        """Migration 005 sonrası: embedding_v2'si boş öğeleri doldurur (arka plan işi).

        Kısa işlemlerle partiler halinde çalışır; yarıda kesilirse kaldığı yerden sürer.
        Boş metinli öğeler atlanır (bir daha seçilmemeleri için model adı işaretlenir).
        """
        import psycopg

        if not self.database_url or self.embedder is None:
            raise RuntimeError("database_url ve kullanılabilir bir embedding modeli gerekli")
        batch = max(1, min(int(batch), 1000))
        done = 0
        model_name = (self.embedding_model_name or "")[:200]
        with psycopg.connect(self.database_url) as conn:
            while limit is None or done < limit:
                rows = conn.execute(
                    "SELECT id::text, cleaned_content FROM items"
                    " WHERE embedding_v2 IS NULL AND embedding_model IS NULL"
                    " ORDER BY collected_at LIMIT %s", (batch,),
                ).fetchall()
                if not rows:
                    break
                for item_id, text in rows:
                    vec = self.generate_embedding(text or "")
                    if vec is not None and len(vec) == EMBEDDING_DIM:
                        literal = "[" + ",".join(repr(float(x)) for x in vec) + "]"
                        conn.execute("UPDATE items SET embedding_v2 = %s::vector, embedding_model = %s"
                                     " WHERE id::text = %s", (literal, model_name, item_id))
                    else:  # vektörlenemeyen (boş) öğe: tekrar seçilmesin
                        conn.execute("UPDATE items SET embedding_model = %s WHERE id::text = %s",
                                     (f"{model_name}#skip"[:200], item_id))
                    done += 1
                conn.commit()
                logger.info("Yeniden vektörleştirme: %d öğe", done)
        return done

    def _has_embedding_v2(self, cur: Any) -> bool:
        """Migration 005 uygulanmış mı? (Yalnızca olumlu sonuç önbelleklenir.)"""
        if getattr(self, "_embedding_v2", False):
            return True
        cur.execute("SELECT 1 FROM information_schema.columns"
                    " WHERE table_name = 'items' AND column_name = 'embedding_v2'")
        self._embedding_v2 = cur.fetchone() is not None
        return self._embedding_v2

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
        if embedding is not None and len(embedding) == EMBEDDING_DIM and self._has_embedding_v2(cur):
            # Migration 005: yeni sütun + hangi modelle üretildiği (karışık modelleri ayırmak için)
            vector_literal = "[" + ",".join(repr(float(x)) for x in embedding) + "]"
            cur.execute(
                """
                INSERT INTO items
                    (source_id, raw_content, cleaned_content, url, title,
                     language, collected_at, published_at, content_hash, metadata, tags,
                     embedding_v2, embedding_model)
                VALUES
                    ((SELECT id FROM sources WHERE id = NULLIF(%s,'')::uuid), %s, %s, %s, %s,
                     %s, NOW(), NULLIF(%s,'')::timestamptz, %s, %s::jsonb, %s, %s::vector, %s)
                ON CONFLICT (content_hash) DO NOTHING
                RETURNING id::text
                """,
                (*base_params, vector_literal, (self.embedding_model_name or "")[:200]),
            )
            row = cur.fetchone()
            return row[0] if row else None
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
