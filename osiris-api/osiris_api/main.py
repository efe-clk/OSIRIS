"""OSIRIS REST API — FastAPI uygulaması.

Bkz. doküman §5.8.
"""

from __future__ import annotations

import json
import logging
import os
import re
import threading
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from typing import Any

from fastapi import Depends, FastAPI, Header, HTTPException
from fastapi.middleware.cors import CORSMiddleware
from osiris_collector.manager import CollectorManager
from osiris_graph.engine import GraphEngine
from osiris_query.engine import QueryEngine
from pydantic import BaseModel, Field

logger = logging.getLogger(__name__)


def insecure_open_enabled() -> bool:
    """Kimliksiz açık mod yalnızca OSIRIS_INSECURE_OPEN=1 ile açıkça istenir."""
    return os.getenv("OSIRIS_INSECURE_OPEN", "") == "1"


def check_startup_config() -> None:
    """Başlatma denetimi (fail-closed): anahtar yoksa ve açık mod istenmediyse reddet."""
    if not API_KEY and not insecure_open_enabled():
        raise RuntimeError(
            "OSIRIS_API_KEY tanımlı değil — API başlatılmadı. Güçlü bir anahtar atayın "
            "(openssl rand -hex 32) veya yalnızca güvenilir ağda OSIRIS_INSECURE_OPEN=1 kullanın."
        )
    if not API_KEY:
        logger.warning("OSIRIS_INSECURE_OPEN=1 — API kimlik doğrulamasız çalışıyor "
                       "(yalnızca güvenilir ağda kullanın)")
    if not JWT_SECRET:
        logger.warning("OSIRIS_JWT_SECRET tanımlı değil — JWT kapalı (/auth/token 503 döner)")


@asynccontextmanager
async def lifespan(_app: FastAPI) -> AsyncIterator[None]:
    check_startup_config()
    yield


app = FastAPI(
    title="OSIRIS API",
    version="0.2.0",
    docs_url="/docs",
    redoc_url=None,
    lifespan=lifespan,
    description=(
        "OSIRIS OSINT platformu REST arayüzü. Kimlik: `X-API-Key` (operatör/kullanıcı) "
        "veya `Authorization: Bearer <JWT>`. Roller: viewer → analyst → admin. "
        "Ayrıntılı başvuru: docs/API.md"
    ),
)

app.add_middleware(
    CORSMiddleware,
    allow_origins=os.getenv("OSIRIS_CORS_ORIGINS", "http://localhost:3000").split(","),
    allow_methods=["GET", "POST", "DELETE"],
    allow_headers=["Content-Type", "X-API-Key", "Authorization"],
    max_age=600,
)

DATABASE_URL = os.getenv("OSIRIS_DATABASE_URL", "postgresql://osiris:osiris@localhost:5432/osiris")
REDIS_URL = os.getenv("OSIRIS_REDIS_URL", "redis://localhost:6379/0")
API_KEY = os.getenv("OSIRIS_API_KEY", "")
# API anahtarına düşme yedeği YOK: aynı sırrın iki işte kullanılması bir sızıntıyı
# hem operatör anahtarı hem JWT imzası olarak geçerli kılardı.
JWT_SECRET = os.getenv("OSIRIS_JWT_SECRET", "")

_collector = CollectorManager(redis_url=REDIS_URL)
_collector_loaded = False
_collector_lock = threading.Lock()
_query = QueryEngine(DATABASE_URL)
_graph = GraphEngine(database_url=DATABASE_URL)
_graph_loaded = False
# RLock sart: graph_add kilidi elde tutup get_graph() cagirir (ic ice alim).
# Lock olsaydi self-deadlock olurdu (kanit: test_graph_roundtrip asilmasi).
_graph_lock = threading.RLock()

_PLUGIN_ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]{0,63}$")
_UUID_RE = re.compile(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-"
                      r"[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")


def _uuid_or_404(value: str, label: str = "Kayıt") -> str:
    """UUID biçimi denetler; bozuksa 404 (500 yerine temiz hata)."""
    if not _UUID_RE.match(value or ""):
        raise HTTPException(status_code=404, detail=f"{label} bulunamadı")
    return value


def _page(limit: int = 100, offset: int = 0) -> tuple[int, int]:
    """Liste uçları için ortak sayfalama (limit 1-200)."""
    try:
        lim, off = int(limit), int(offset)
    except (TypeError, ValueError) as exc:
        raise HTTPException(status_code=422, detail="Sayfalama geçersiz") from exc
    if off < 0:
        raise HTTPException(status_code=422, detail="Sayfalama geçersiz")
    return max(1, min(lim, 200)), off


def get_graph() -> GraphEngine:
    """Grafı ilk kullanımda DB'den besler (best-effort kalıcılık)."""
    global _graph_loaded
    with _graph_lock:
        if not _graph_loaded:
            _graph_loaded = True
            try:
                _graph.load_from_db()
            except Exception as exc:  # noqa: BLE001
                logger.debug("Graf yüklenemedi: %s", exc)
    return _graph


def _lookup_db_key(raw_key: str) -> dict[str, str] | None:
    """Veritabanındaki kullanıcı anahtarını çözer (hash karşılaştırma)."""
    from osiris_api.auth import hash_api_key

    try:
        import psycopg

        with psycopg.connect(DATABASE_URL, connect_timeout=3) as conn:
            with conn.cursor() as cur:
                cur.execute(
                    """
                    SELECT u.username, u.role::text FROM api_keys k
                    JOIN users u ON u.id = k.user_id
                    WHERE k.key_hash = %s AND NOT k.revoked
                    """,
                    (hash_api_key(raw_key),),
                )
                row = cur.fetchone()
                if row is None:
                    return None
                cur.execute(
                    "UPDATE api_keys SET last_used_at = NOW() WHERE key_hash = %s",
                    (hash_api_key(raw_key),),
                )
                conn.commit()
                return {"user": row[0], "role": row[1]}
    except Exception as exc:  # noqa: BLE001
        logger.debug("Anahtar araması atlandı: %s", exc)
        return None


def _check_api_key(provided: str | None) -> bool:
    if not API_KEY or not provided:
        return False
    import hmac as _hmac

    return _hmac.compare_digest(provided, API_KEY)


def resolve_identity(
    x_api_key: str | None = None,
    authorization: str | None = None,
) -> dict[str, str] | None:
    """Kimliği çözer: operatör anahtarı → DB anahtarı → Bearer JWT."""
    if _check_api_key(x_api_key):
        return {"user": "operator", "role": "admin"}
    if authorization and authorization.lower().startswith("bearer "):
        from osiris_api.auth import verify_token_with_role

        try:
            sub, role = verify_token_with_role(authorization[7:].strip(), JWT_SECRET)
            return {"user": sub, "role": role}
        except ValueError:
            pass
    if x_api_key:
        ident = _lookup_db_key(x_api_key)
        if ident is not None:
            return ident
    return None


def _require_role(
    minimum: str,
    x_api_key: str | None = Header(default=None),
    authorization: str | None = Header(default=None),
) -> dict[str, str]:
    from osiris_api.auth import role_satisfies

    if not API_KEY:
        if insecure_open_enabled():
            return {"user": "anonymous", "role": "admin"}  # açıkça istenmiş açık mod
        # Başlatma denetimi atlatılmışsa (örn. lifespan'sız çalıştırma) bile kapalı kal
        raise HTTPException(status_code=503, detail="Kimlik doğrulama yapılandırılmamış")
    ident = resolve_identity(x_api_key, authorization)
    if ident is None:
        raise HTTPException(status_code=401, detail="Geçersiz kimlik bilgisi")
    if not role_satisfies(ident["role"], minimum):
        raise HTTPException(status_code=403, detail="Yetki yetersiz")
    return ident


def require_viewer(
    x_api_key: str | None = Header(default=None),
    authorization: str | None = Header(default=None),
) -> dict[str, str]:
    return _require_role("viewer", x_api_key, authorization)


def require_analyst(
    x_api_key: str | None = Header(default=None),
    authorization: str | None = Header(default=None),
) -> dict[str, str]:
    return _require_role("analyst", x_api_key, authorization)


def require_admin(
    x_api_key: str | None = Header(default=None),
    authorization: str | None = Header(default=None),
) -> dict[str, str]:
    return _require_role("admin", x_api_key, authorization)


def require_auth(
    x_api_key: str | None = Header(default=None),
    authorization: str | None = Header(default=None),
) -> None:
    """Geriye uyumlu: herhangi bir geçerli kimlik (viewer eşiği)."""
    require_viewer(x_api_key, authorization)


# Geriye uyumluluk: eski bağımlılık adı
require_api_key = require_auth


def get_collector() -> CollectorManager:
    global _collector_loaded
    with _collector_lock:
        if not _collector_loaded:
            _collector.load_plugins()
            _collector_loaded = True
    return _collector


def _audit(action: str, resource: str | None = None,
           detail: dict[str, Any] | None = None, user: str = "api") -> None:
    """Best-effort denetim kaydı (doküman §10.4). Asla isteği bozmaz."""
    try:
        import psycopg
        from osiris.audit import append_audit

        with psycopg.connect(DATABASE_URL, connect_timeout=3) as conn:
            append_audit(conn, user, action, resource, detail)
    except Exception as exc:  # noqa: BLE001
        logger.debug("Denetim kaydı atlandı: %s", exc)


class CollectRequest(BaseModel):
    plugin_id: str = Field(min_length=1, max_length=64, pattern=r"^[a-z0-9][a-z0-9-]{0,63}$")
    config: dict[str, Any] = Field(default_factory=dict, max_length=50)


class SearchRequest(BaseModel):
    query: str = Field(min_length=1, max_length=500)
    limit: int = Field(default=20, ge=1, le=100)


class EntitySearchRequest(BaseModel):
    entity_type: str = Field(min_length=1, max_length=32)
    value: str = Field(min_length=1, max_length=500)
    limit: int = Field(default=20, ge=1, le=100)


class GraphAddRequest(BaseModel):
    source: str = Field(min_length=1, max_length=500)
    target: str = Field(min_length=1, max_length=500)
    relation_type: str = Field(default="related", max_length=100)


@app.get("/health", tags=["Health"], summary="Servis durumu")
def health() -> dict[str, str]:
    return {"status": "ok", "service": "osiris-api"}


class TokenRequest(BaseModel):
    api_key: str = Field(min_length=1, max_length=500)
    role: str = Field(default="viewer", max_length=32)


@app.post("/auth/token", tags=["Auth"], summary="JWT üret")
def issue_token(req: TokenRequest) -> dict[str, object]:
    """Operatör anahtarı karşılığında kısa ömürlü JWT üretir (1 saat)."""
    if not API_KEY or not JWT_SECRET:
        raise HTTPException(status_code=503, detail="Jeton üretimi kapalı (OSIRIS_JWT_SECRET yok)")
    if not _check_api_key(req.api_key):
        raise HTTPException(status_code=401, detail="Geçersiz kimlik bilgisi")
    from osiris_api.auth import ROLES, create_token

    if req.role not in ROLES:
        raise HTTPException(status_code=400, detail="Geçersiz rol")
    token = create_token("api-client", JWT_SECRET, role=req.role)
    _audit("auth.token", None, {"role": req.role}, user="operator")
    return token


class ApiKeyRequest(BaseModel):
    username: str = Field(min_length=1, max_length=200)
    role: str = Field(default="viewer", max_length=32)
    name: str = Field(default="", max_length=200)


@app.post("/auth/keys", tags=["Auth"],
             summary="Kullanıcı anahtarı üret")
def create_api_key(req: ApiKeyRequest, ident: dict = Depends(require_admin)) -> dict[str, str]:
    """Kullanıcıya API anahtarı üretir (ham değer BİR KEZ döner)."""
    import secrets

    from osiris_api.auth import ROLES, hash_api_key

    if req.role not in ROLES:
        raise HTTPException(status_code=400, detail="Geçersiz rol")
    raw_key = f"osiris_{secrets.token_urlsafe(32)}"
    try:
        import psycopg

        with psycopg.connect(DATABASE_URL, connect_timeout=5) as conn:
            with conn.cursor() as cur:
                cur.execute(
                    """
                    INSERT INTO users (username, role)
                    VALUES (%s, %s)
                    ON CONFLICT (username)
                    DO UPDATE SET role = EXCLUDED.role
                    RETURNING id::text
                    """,
                    (req.username, req.role),
                )
                user_id = cur.fetchone()[0]
                cur.execute(
                    """
                    INSERT INTO api_keys (user_id, key_hash, key_prefix, name)
                    VALUES (%s::uuid, %s, %s, %s)
                    RETURNING id::text
                    """,
                    (user_id, hash_api_key(raw_key), raw_key[:8], req.name[:200]),
                )
                key_id = cur.fetchone()[0]
            conn.commit()
    except Exception as exc:  # noqa: BLE001
        logger.exception("Anahtar üretilemedi")
        raise HTTPException(status_code=500, detail="Anahtar üretilemedi") from exc
    _audit("auth.key.create", req.username, {"role": req.role, "key_id": key_id},
           user=ident["user"])
    return {"id": key_id, "api_key": raw_key, "role": req.role}


@app.get("/auth/keys", dependencies=[Depends(require_admin)], tags=["Auth"],
             summary="Anahtarları listele")
def list_api_keys() -> list[dict[str, Any]]:
    """Anahtarları listeler (ham değerler ASLA dönülmez)."""
    try:
        import psycopg
        from psycopg.rows import dict_row

        with psycopg.connect(DATABASE_URL, connect_timeout=5,
                             row_factory=dict_row) as conn:
            rows = conn.execute(
                """
                SELECT k.id::text AS id, u.username, u.role::text AS role,
                       k.key_prefix, k.name, k.revoked, k.created_at, k.last_used_at
                FROM api_keys k JOIN users u ON u.id = k.user_id
                ORDER BY k.created_at DESC LIMIT 200
                """
            ).fetchall()
        return [dict(r) for r in rows]
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Liste okunamadı") from exc


@app.delete("/auth/keys/{key_id}", tags=["Auth"],
             summary="API anahtarı iptal et")
def revoke_api_key(key_id: str, ident: dict = Depends(require_admin)) -> dict[str, str]:
    """Anahtarı iptal eder (silmez — denetim izi kalır)."""
    key_id = _uuid_or_404(key_id, "Anahtar")
    try:
        import psycopg

        with psycopg.connect(DATABASE_URL, connect_timeout=5) as conn:
            with conn.cursor() as cur:
                cur.execute(
                    "UPDATE api_keys SET revoked = TRUE WHERE id::text = %s",
                    (key_id,),
                )
                if cur.rowcount == 0:
                    raise HTTPException(status_code=404, detail="Anahtar bulunamadı")
            conn.commit()
    except HTTPException:
        raise
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="İptal başarısız") from exc
    _audit("auth.key.revoke", None, {"key_id": key_id}, user=ident["user"])
    return {"status": "revoked"}


@app.get("/plugins", dependencies=[Depends(require_viewer)], tags=["Collect"],
             summary="Plugin envanteri (+config şemaları)")
def list_plugins() -> list[dict[str, Any]]:
    collector = get_collector()
    out = []
    for pid, p in list(collector.plugins.items()):  # eşzamanlı güvenli anlık görüntü
        try:
            manifest = collector.get_manifest(pid)
        except (KeyError, ValueError):
            manifest = {}
        out.append({
            "id": pid,
            "name": p.name,
            "network_type": p.network_type,
            "description": manifest.get("description", ""),
            "config_schema": manifest.get("config_schema", {}),
            "schedule_default": manifest.get("schedule_default", ""),
        })
    return out


def _db():
    """dict satırlı kısa ömürlü DB bağlantısı (testlerde yamalanabilir)."""
    import psycopg
    from psycopg.rows import dict_row

    return psycopg.connect(DATABASE_URL, connect_timeout=5, row_factory=dict_row)


_known_columns: set[tuple[str, str]] = set()


def _has_column(conn: Any, table: str, column: str) -> bool:
    """Ekleyici migration'lar uygulanmış mı? (Yalnızca olumlu sonuç önbelleklenir.)"""
    if (table, column) in _known_columns:
        return True
    row = conn.execute(
        "SELECT 1 AS ok FROM information_schema.columns"
        " WHERE table_name = %s AND column_name = %s",
        (table, column),
    ).fetchone()
    if row:
        _known_columns.add((table, column))
    return bool(row)


@app.get("/stats", dependencies=[Depends(require_viewer)], tags=["Stats"],
         summary="Panel sayaçları")
def stats() -> dict[str, Any]:
    """Panel için sayaçlar: kaynak/öğe/varlık/kenar/sorgu."""
    # NOT: sorgular sabit dizgilerdir (tablo adı enterpolasyonu yok).
    queries = {
        "sources": "SELECT count(*) AS c FROM sources",
        "sources_enabled": "SELECT count(*) AS c FROM sources WHERE enabled",
        "items": "SELECT count(*) AS c FROM items",
        "entities": "SELECT count(*) AS c FROM entities",
        "edges": "SELECT count(*) AS c FROM graph_edges",
        "saved_queries": "SELECT count(*) AS c FROM saved_queries",
        "alerts": "SELECT count(*) AS c FROM saved_queries WHERE alert_enabled",
    }
    try:
        with _db() as conn:
            counts = {key: conn.execute(sql).fetchone()["c"]
                      for key, sql in queries.items()}
            latest = conn.execute(
                "SELECT collected_at FROM items ORDER BY collected_at DESC LIMIT 1"
            ).fetchone()
        counts["latest_item_at"] = latest["collected_at"] if latest else None
        return counts
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="İstatistik okunamadı") from exc


@app.get("/items/recent", dependencies=[Depends(require_viewer)], tags=["Items"],
         summary="Son öğeler")
def recent_items(limit: int = 20, offset: int = 0) -> list[dict[str, Any]]:
    """Sayfalama: `limit` (1-50), `offset`."""
    limit = max(1, min(int(limit), 50))
    offset = max(0, int(offset))
    try:
        with _db() as conn:
            rows = conn.execute(
                """
                SELECT id::text AS id, title, url, language,
                       LEFT(cleaned_content, 500) AS snippet,
                       collected_at, tags
                FROM items ORDER BY collected_at DESC LIMIT %s OFFSET %s
                """,
                (limit, offset),
            ).fetchall()
        return [dict(r) for r in rows]
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Öğeler okunamadı") from exc


@app.get("/items/{item_id}", dependencies=[Depends(require_viewer)], tags=["Items"],
         summary="Öğe detayı (+varlıklar)")
def item_detail(item_id: str) -> dict[str, Any]:
    """Öğeyi bağlı varlıklarıyla döndürür (detay görünümü)."""
    item_id = _uuid_or_404(item_id, "Öğe")
    try:
        with _db() as conn:
            row = conn.execute(
                """
                SELECT id::text AS id, source_id::text AS source_id, title, url,
                       language, cleaned_content, collected_at, published_at,
                       content_hash, metadata, tags
                FROM items WHERE id::text = %s
                """,
                (item_id,),
            ).fetchone()
            if row is None:
                raise HTTPException(status_code=404, detail="Öğe bulunamadı")
            entities = conn.execute(
                """
                SELECT e.type::text AS type, e.value, ie.mention_count
                FROM item_entities ie JOIN entities e ON e.id = ie.entity_id
                WHERE ie.item_id::text = %s
                ORDER BY ie.mention_count DESC LIMIT 200
                """,
                (item_id,),
            ).fetchall()
        out = dict(row)
        out["entities"] = [dict(e) for e in entities]
        return out
    except HTTPException:
        raise
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Öğe okunamadı") from exc


@app.get("/entities/top", dependencies=[Depends(require_viewer)], tags=["Items"],
         summary="En çok geçen varlıklar")
def top_entities(entity_type: str | None = None,
                 limit: int = 20) -> list[dict[str, Any]]:
    """Anma sayısına göre sıralı varlıklar. Filtre: `entity_type`. Limit 1-100."""
    from osiris_query.engine import _VALID_ENTITY_TYPES as _ETYPES  # noqa: PLC0415

    limit = max(1, min(int(limit), 100))
    clauses: list[str] = []
    params: list = []
    if entity_type:
        if entity_type not in _ETYPES:
            raise HTTPException(status_code=400, detail="Geçersiz entity_type")
        clauses.append("e.type = %s")
        params.append(entity_type)
    # Guvenlik (bandit B608 incelemesi): SQL yalnizca SABIT parcalardan kurulur;
    # kullanici girdisi sadece bagli parametre (%s) olarak gecer. Kanit:
    # test_main.py::test_sql_injection_attempts (tirnak/UNION/yorum denemeleri).
    # NOT: f-string satirina `# nosec` konulamaz (acilmamis uc-tirnak ici
    # sozluksel olarak dizgi sayilir) — bu yorum bilerek duz yorumdur.
    where = f"WHERE {' AND '.join(clauses)}" if clauses else ""
    try:
        with _db() as conn:
            rows = conn.execute(
                f"""
                SELECT e.type::text AS type, e.value,
                       COUNT(*) AS mentions, MAX(e.last_seen_at) AS last_seen
                FROM entities e JOIN item_entities ie ON ie.entity_id = e.id
                {where}
                GROUP BY e.type, e.value
                ORDER BY mentions DESC LIMIT %s
                """,
                (*params, limit),
            ).fetchall()
        return [dict(r) for r in rows]
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Varlıklar okunamadı") from exc


class BatchJob(BaseModel):
    plugin_id: str = Field(min_length=1, max_length=64, pattern=r"^[a-z0-9][a-z0-9-]{0,63}$")
    config: dict[str, Any] = Field(default_factory=dict, max_length=50)


class BatchRequest(BaseModel):
    jobs: list[BatchJob] = Field(min_length=1, max_length=10)


@app.post("/collect/batch", tags=["Collect"],
         summary="Toplu toplama (en fazla 10 iş)")
def collect_batch(req: BatchRequest, ident: dict = Depends(require_analyst)) -> dict[str, Any]:
    """İşleri sırayla çalıştırır; iş başına sonuç döner (kısmi başarı mümkün)."""
    collector = get_collector()
    results = []
    for job in req.jobs:
        if job.plugin_id not in collector.plugins:
            results.append({"plugin_id": job.plugin_id, "ok": False,
                            "error": "Plugin bulunamadı"})
            continue
        if len(str(job.config)) > 20_000:
            results.append({"plugin_id": job.plugin_id, "ok": False,
                            "error": "config çok büyük"})
            continue
        try:
            result = collector.run_collection(job.plugin_id, job.config)
        except KeyError as exc:
            results.append({"plugin_id": job.plugin_id, "ok": False,
                            "error": str(exc)})
            continue
        results.append({"plugin_id": job.plugin_id, "ok": result.success,
                        "items": len(result.items) if result.success else 0,
                        "error": result.error})
    ok_count = sum(1 for r in results if r["ok"])
    _audit("collect.batch", None, {"jobs": len(results), "ok": ok_count},
           user=ident["user"])
    return {"jobs": results, "ok": ok_count, "total": len(results)}


@app.post("/collect", tags=["Collect"],
             summary="Tek toplama görevi çalıştır")
def collect(req: CollectRequest, ident: dict = Depends(require_analyst)) -> dict[str, Any]:
    collector = get_collector()
    if req.plugin_id not in collector.plugins:
        raise HTTPException(status_code=404, detail="Plugin bulunamadı")
    if len(str(req.config)) > 20_000:
        raise HTTPException(status_code=413, detail="config çok büyük")
    try:
        result = collector.run_collection(req.plugin_id, req.config)
    except KeyError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc
    if not result.success:
        _audit("collect.failed", req.plugin_id, {"error": result.error},
               user=ident["user"])
        raise HTTPException(status_code=502, detail=result.error)
    _audit("collect", req.plugin_id, {"items": len(result.items)}, user=ident["user"])
    return {"items": len(result.items), "metadata": result.metadata}


@app.post("/search", tags=["Search"],
             summary="Tam metin arama")
def search(req: SearchRequest, ident: dict = Depends(require_viewer)) -> list[dict[str, Any]]:
    try:
        rows = _query.fulltext_search(req.query, req.limit)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    except Exception as exc:  # noqa: BLE001
        logger.exception("Arama hatası")
        raise HTTPException(status_code=500, detail="Arama başarısız") from exc
    _audit("search", None, {"query": req.query[:200], "hits": len(rows)},
           user=ident["user"])
    return rows


@app.post("/search/entity", dependencies=[Depends(require_viewer)], tags=["Search"],
             summary="Varlık bazlı arama")
def entity_search(req: EntitySearchRequest) -> list[dict[str, Any]]:
    try:
        return _query.entity_search(req.entity_type, req.value, req.limit)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    except Exception as exc:  # noqa: BLE001
        logger.exception("Varlık araması hatası")
        raise HTTPException(status_code=500, detail="Arama başarısız") from exc


@app.get("/graph", dependencies=[Depends(require_viewer)], tags=["Graph"],
             summary="Graf düğüm/kenarları")
def graph() -> dict[str, Any]:
    import json

    try:
        return json.loads(get_graph().to_json())
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Graf okunamadı") from exc


@app.post("/graph/relation", tags=["Graph"],
             summary="Graf ilişkisi ekle (kalıcı)")
def graph_add(req: GraphAddRequest, ident: dict = Depends(require_analyst)) -> dict[str, str]:
    with _graph_lock:
        g = get_graph()
        g.add_entity(req.source)
        g.add_entity(req.target)
        g.add_relation(req.source, req.target, req.relation_type)
    try:
        g.save_to_db()
    except Exception as exc:  # noqa: BLE001
        logger.debug("Graf persist atlandı: %s", exc)
    return {"status": "ok"}


class SourceRequest(BaseModel):
    name: str = Field(min_length=1, max_length=500)
    plugin_id: str = Field(min_length=1, max_length=64, pattern=r"^[a-z0-9][a-z0-9-]{0,63}$")
    url: str = Field(default="", max_length=2048)
    network_type: str = Field(default="www", max_length=32)
    schedule: str = Field(default="", max_length=100)
    priority: int = Field(default=5, ge=1, le=10)
    enabled: bool = True
    tags: list[str] = Field(default_factory=list, max_length=20)
    config: dict[str, Any] = Field(default_factory=dict, max_length=50)
    # KVKK saklama süresi: None → OSIRIS_RETENTION_DAYS, 0 → süreyle silinmez
    retention_days: int | None = Field(default=None, ge=0, le=36500)


_VALID_NETWORKS = {"www", "tor", "i2p", "p2p", "freenet", "zeronet", "irc",
                   "matrix", "rss", "api", "blockchain", "sdr", "custom"}


@app.get("/sources", dependencies=[Depends(require_viewer)], tags=["Sources"],
         summary="Kaynakları listele")
def list_sources(enabled: bool | None = None, plugin_id: str | None = None,
                 q: str | None = None, limit: int = 100, offset: int = 0,
                 ) -> list[dict[str, Any]]:
    """Filtreler: `enabled`, `plugin_id`, `q` (ad/URL alt-metin). Sayfalama: `limit` (1-200), `offset`."""
    limit, offset = _page(limit, offset)
    clauses, params = [], []
    if enabled is not None:
        clauses.append("enabled = %s")
        params.append(enabled)
    if plugin_id:
        clauses.append("plugin_id = %s")
        params.append(plugin_id[:64])
    if q:
        # LIKE jokerlerini etkisizleştir
        needle = q[:200].replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_")
        clauses.append("(name ILIKE %s ESCAPE '\\' OR url ILIKE %s ESCAPE '\\')")
        params.extend([f"%{needle}%", f"%{needle}%"])
    # Guvenlik (bandit B608 incelemesi): SQL yalnizca SABIT parcalardan kurulur;
    # kullanici girdisi sadece bagli parametre (%s) olarak gecer. Kanit:
    # test_main.py::test_sql_injection_attempts (tirnak/UNION/yorum denemeleri).
    # NOT: f-string satirina `# nosec` konulamaz (acilmamis uc-tirnak ici
    # sozluksel olarak dizgi sayilir) — bu yorum bilerek duz yorumdur.
    where = f"WHERE {' AND '.join(clauses)}" if clauses else ""
    try:
        with _db() as conn:
            # Migration 004 öncesi DB'de sütun yok → NULL (API önce dağıtılabilir)
            retention = ("retention_days" if _has_column(conn, "sources", "retention_days")
                         else "NULL::int AS retention_days")
            rows = conn.execute(
                f"""
                SELECT id::text AS id, name, url, network_type::text AS network_type,
                       plugin_id, schedule, priority, enabled, tags,
                       last_crawled_at, last_success_at, failure_count, avg_response_ms,
                       {retention}
                FROM sources {where} ORDER BY name LIMIT %s OFFSET %s
                """,
                (*params, limit, offset),
            ).fetchall()
        return [dict(r) for r in rows]
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Kaynaklar okunamadı") from exc


@app.post("/sources", tags=["Sources"],
             summary="Kaynak oluştur")
def create_source(req: SourceRequest, ident: dict = Depends(require_analyst)) -> dict[str, str]:
    if req.network_type not in _VALID_NETWORKS:
        raise HTTPException(status_code=400, detail="Geçersiz network_type")
    if req.plugin_id not in get_collector().plugins:
        raise HTTPException(status_code=404, detail="Plugin bulunamadı")
    try:
        with _db() as conn:
            has_retention = _has_column(conn, "sources", "retention_days")
            if req.retention_days is not None and not has_retention:
                raise HTTPException(status_code=503,
                                    detail="retention_days için migration 004 uygulanmamış")
            params = (req.name, req.url, req.network_type, req.plugin_id,
                      req.schedule, req.priority, req.enabled,
                      [str(t)[:100] for t in req.tags[:20]],
                      json.dumps({"config": req.config}, ensure_ascii=False))
            if has_retention:
                row = conn.execute(
                    """
                    INSERT INTO sources
                        (name, url, network_type, plugin_id, schedule, priority,
                         enabled, tags, metadata, retention_days)
                    VALUES (%s, NULLIF(%s,''), %s::network_type, %s,
                            NULLIF(%s,''), %s, %s, %s, %s::jsonb, %s)
                    RETURNING id::text AS id
                    """,
                    (*params, req.retention_days),
                ).fetchone()
            else:
                row = conn.execute(
                    """
                    INSERT INTO sources
                        (name, url, network_type, plugin_id, schedule, priority,
                         enabled, tags, metadata)
                    VALUES (%s, NULLIF(%s,''), %s::network_type, %s,
                            NULLIF(%s,''), %s, %s, %s, %s::jsonb)
                    RETURNING id::text AS id
                    """,
                    params,
                ).fetchone()
            conn.commit()
        _audit("source.create", req.name, {"plugin": req.plugin_id},
               user=ident["user"])
        return {"id": row["id"]}
    except HTTPException:
        raise
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Kaynak oluşturulamadı") from exc


@app.delete("/sources/{source_id}", tags=["Sources"],
             summary="Kaynak sil")
def delete_source(source_id: str, ident: dict = Depends(require_analyst)) -> dict[str, str]:
    source_id = _uuid_or_404(source_id, "Kaynak")
    try:
        with _db() as conn:
            with conn.cursor() as cur:
                cur.execute("DELETE FROM sources WHERE id::text = %s", (source_id,))
                if cur.rowcount == 0:
                    raise HTTPException(status_code=404, detail="Kaynak bulunamadı")
            conn.commit()
        _audit("source.delete", None, {"id": source_id}, user=ident["user"])
        return {"status": "deleted"}
    except HTTPException:
        raise
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Kaynak silinemedi") from exc


@app.post("/sources/{source_id}/collect", tags=["Sources"],
         summary="Kayıtlı kaynağı çalıştır")
def collect_source(source_id: str, ident: dict = Depends(require_analyst)) -> dict[str, Any]:
    """Kayıtlı kaynağı tek seferlik çalıştırır (sağlık takibi dahil)."""
    source_id = _uuid_or_404(source_id, "Kaynak")
    try:
        with _db() as conn:
            row = conn.execute(
                """
                SELECT plugin_id, url, enabled, metadata FROM sources WHERE id::text = %s
                """,
                (source_id,),
            ).fetchone()
        if row is None:
            raise HTTPException(status_code=404, detail="Kaynak bulunamadı")
        row = dict(row)
    except HTTPException:
        raise
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Kaynak okunamadı") from exc
    if not row.get("enabled", True):
        raise HTTPException(status_code=409, detail="Kaynak devre dışı")
    meta = row.get("metadata") or {}
    if isinstance(meta, str):
        try:
            meta = json.loads(meta)
        except ValueError:
            meta = {}
    task_config: dict[str, Any] = dict(meta.get("config") or {})
    task_config["source_id"] = source_id
    if row.get("url"):
        for key in ("url", "feed_url", "endpoint"):
            task_config.setdefault(key, row["url"])
    collector = get_collector()
    if row["plugin_id"] not in collector.plugins:
        raise HTTPException(status_code=404, detail="Plugin bulunamadı")
    try:
        result = collector.run_collection(row["plugin_id"], task_config)
    except KeyError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc
    if not result.success:
        raise HTTPException(status_code=502, detail=result.error)
    _audit("source.collect", row["plugin_id"],
           {"id": source_id, "items": len(result.items)}, user=ident["user"])
    return {"items": len(result.items), "metadata": result.metadata}


class SavedQueryRequest(BaseModel):
    name: str = Field(min_length=1, max_length=200)
    query_text: str = Field(min_length=1, max_length=500)
    query_type: str = Field(default="fts", max_length=32)
    alert_enabled: bool = False


_VALID_QUERY_TYPES = {"fts", "semantic", "regex", "entity", "graph"}


@app.get("/saved-queries", dependencies=[Depends(require_viewer)], tags=["Alerts"],
         summary="Kayıtlı sorguları listele")
def list_saved_queries(alert_enabled: bool | None = None,
                       limit: int = 100, offset: int = 0) -> list[dict[str, Any]]:
    """Filtre: `alert_enabled`. Sayfalama: `limit` (1-200), `offset`."""
    limit, offset = _page(limit, offset)
    where = ""
    params: list = []
    if alert_enabled is not None:
        where, params = "WHERE alert_enabled = %s", [alert_enabled]
    try:
        with _db() as conn:
            rows = conn.execute(
                f"""
                SELECT id::text AS id, name, query_text,
                       query_type::text AS query_type, alert_enabled, last_triggered_at
                FROM saved_queries {where} ORDER BY name LIMIT %s OFFSET %s
                """,
                (*params, limit, offset),
            ).fetchall()
        return [dict(r) for r in rows]
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Sorgular okunamadı") from exc


@app.post("/saved-queries", dependencies=[Depends(require_analyst)], tags=["Alerts"],
             summary="Kayıtlı sorgu oluştur")
def create_saved_query(req: SavedQueryRequest) -> dict[str, str]:
    if req.query_type not in _VALID_QUERY_TYPES:
        raise HTTPException(status_code=400, detail="Geçersiz query_type")
    try:
        with _db() as conn:
            row = conn.execute(
                """
                INSERT INTO saved_queries (name, query_text, query_type, alert_enabled)
                VALUES (%s, %s, %s::query_type, %s)
                RETURNING id::text AS id
                """,
                (req.name, req.query_text, req.query_type, req.alert_enabled),
            ).fetchone()
            conn.commit()
        return {"id": row["id"]}
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Sorgu oluşturulamadı") from exc


@app.delete("/saved-queries/{query_id}", dependencies=[Depends(require_analyst)], tags=["Alerts"],
             summary="Kayıtlı sorgu sil")
def delete_saved_query(query_id: str) -> dict[str, str]:
    query_id = _uuid_or_404(query_id, "Sorgu")
    try:
        with _db() as conn:
            with conn.cursor() as cur:
                cur.execute("DELETE FROM saved_queries WHERE id::text = %s", (query_id,))
                if cur.rowcount == 0:
                    raise HTTPException(status_code=404, detail="Sorgu bulunamadı")
            conn.commit()
        return {"status": "deleted"}
    except HTTPException:
        raise
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Sorgu silinemedi") from exc


class AlertTestRequest(BaseModel):
    text: str = Field(min_length=1, max_length=50_000)
    title: str = Field(default="", max_length=2000)


@app.post("/alerts/test", dependencies=[Depends(require_analyst)], tags=["Alerts"],
             summary="Metni kurallara karşı dene")
def test_alerts(req: AlertTestRequest) -> list[dict[str, Any]]:
    """Ham metni kayıtlı uyarı sorgularıyla eşleştirir (kayıt oluşturmaz)."""
    from osiris_alert.manager import AlertManager

    try:
        with _db() as conn:
            queries = conn.execute(
                """
                SELECT id::text AS id, name, query_text, alert_enabled
                FROM saved_queries WHERE alert_enabled LIMIT 100
                """
            ).fetchall()
        manager = AlertManager(redis_url=None)
        triggered = manager.check_item(
            {"cleaned_content": req.text, "title": req.title},
            [dict(q) for q in queries],
        )
        if triggered:
            try:
                with _db() as conn:
                    with conn.cursor() as cur:
                        cur.execute(
                            """
                            UPDATE saved_queries SET last_triggered_at = NOW()
                            WHERE id::text = ANY(%s)
                            """,
                            ([t["query_id"] for t in triggered
                              if t.get("query_id")],),
                        )
                    conn.commit()
            except Exception as exc:  # noqa: BLE001
                logger.debug("last_triggered yazılamadı: %s", exc)
        return triggered
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Eşleşme başarısız") from exc


class MarkdownReportRequest(BaseModel):
    title: str = Field(min_length=1, max_length=500)
    scope: str = Field(default="", max_length=1000)
    summary: str = Field(default="", max_length=20_000)
    findings: list[dict[str, Any]] = Field(default_factory=list, max_length=500)
    sources: list[str] = Field(default_factory=list, max_length=500)


@app.post("/reports/markdown", dependencies=[Depends(require_analyst)], tags=["Reports"],
             summary="Markdown rapor üret")
def report_markdown(req: MarkdownReportRequest) -> dict[str, str]:
    """Markdown rapor üretir (gövdeyle döner, diske yazmaz)."""
    from osiris_report.generator import ReportGenerator

    try:
        content = ReportGenerator().generate_markdown(
            req.title, req.scope, req.summary, req.findings, req.sources)
        return {"markdown": content}
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Rapor üretilemedi") from exc


# ---------------------------------------------------------------------------
# KVKK: saklama ve silme (docs/SOZLESME.md §6). Yalnızca admin.
# Denetim kaydına silinen kişisel veri YAZILMAZ — yalnızca SHA-256 özeti.
# ---------------------------------------------------------------------------


def _pii_digest(value: str) -> str:
    import hashlib

    return hashlib.sha256(value.encode()).hexdigest()


@app.delete("/items/{item_id}", tags=["Privacy"], summary="Öğeyi sil (KVKK)")
def delete_item(item_id: str, ident: dict = Depends(require_admin)) -> dict[str, Any]:
    """Öğeyi ve yalnızca ona bağlı kalan varlıkları kalıcı olarak siler."""
    from osiris.retention import purge_orphan_entities

    item_id = _uuid_or_404(item_id, "Öğe")
    try:
        with _db() as conn:
            with conn.cursor() as cur:
                cur.execute("DELETE FROM items WHERE id::text = %s", (item_id,))
                if cur.rowcount == 0:
                    raise HTTPException(status_code=404, detail="Öğe bulunamadı")
                orphans = purge_orphan_entities(cur)
            conn.commit()
    except HTTPException:
        raise
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Öğe silinemedi") from exc
    _audit("item.delete", None, {"id": item_id, "entities_deleted": orphans},
           user=ident["user"])
    return {"status": "deleted", "items_deleted": 1, "entities_deleted": orphans}


class EraseRequest(BaseModel):
    entity_type: str = Field(min_length=1, max_length=32)
    value: str = Field(min_length=1, max_length=500)
    delete_items: bool = True


@app.post("/entities/erase", tags=["Privacy"], summary="Varlık bazlı silme (KVKK)")
def erase_entity(req: EraseRequest, ident: dict = Depends(require_admin)) -> dict[str, Any]:
    """Bir varlığı (örn. e-posta) ve — `delete_items` ise — onu anan öğeleri siler.

    Eşleşme tip + büyük/küçük harf duyarsız değerdir. Graf kenarlarındaki aynı
    ad da silinir. Denetim kaydına değerin yalnızca SHA-256 özeti yazılır.
    """
    global _graph_loaded
    from osiris.retention import purge_orphan_entities
    from osiris_query.engine import _VALID_ENTITY_TYPES as _ETYPES  # noqa: PLC0415

    if req.entity_type not in _ETYPES:
        raise HTTPException(status_code=400, detail="Geçersiz entity_type")
    needle = req.value.strip().lower()
    if not needle:
        raise HTTPException(status_code=422, detail="value boş olamaz")
    try:
        with _db() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    "SELECT id FROM entities WHERE type = %s::entity_type"
                    " AND lower(value) = %s",
                    (req.entity_type, needle),
                )
                ids = [r["id"] for r in cur.fetchall()]
                items_deleted = 0
                if ids and req.delete_items:
                    cur.execute(
                        "DELETE FROM items WHERE id IN ("
                        " SELECT item_id FROM item_entities WHERE entity_id = ANY(%s))",
                        (ids,),
                    )
                    items_deleted = max(0, cur.rowcount or 0)
                entities_deleted = 0
                if ids:
                    cur.execute("DELETE FROM entities WHERE id = ANY(%s)", (ids,))
                    entities_deleted = max(0, cur.rowcount or 0)
                cur.execute(
                    "DELETE FROM graph_edges WHERE lower(source_name) = %s OR lower(target_name) = %s",
                    (needle, needle),
                )
                edges_deleted = max(0, cur.rowcount or 0)
                if items_deleted:
                    entities_deleted += purge_orphan_entities(cur)
            conn.commit()
    except HTTPException:
        raise
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Silme başarısız") from exc
    if edges_deleted:
        with _graph_lock:  # bellekteki grafı DB'den yeniden kur
            _graph.graph.clear()
            _graph_loaded = False
    out = {"items_deleted": items_deleted, "entities_deleted": entities_deleted,
           "edges_deleted": edges_deleted}
    _audit("entity.erase", None, {"entity_type": req.entity_type,
                                  "value_sha256": _pii_digest(needle), **out},
           user=ident["user"])
    return out


@app.post("/admin/retention/run", tags=["Privacy"], summary="Saklama temizliğini çalıştır")
def run_retention(ident: dict = Depends(require_admin)) -> dict[str, int]:
    """Süresi dolan öğeleri hemen siler (pipeline işçisi yoksa n8n/cron ile tetiklenebilir)."""
    from osiris.retention import purge_expired

    try:
        with _db() as conn:
            res = purge_expired(conn)
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail="Saklama temizliği başarısız") from exc
    _audit("retention.run", None, res, user=ident["user"])
    return res


def run() -> None:
    import uvicorn

    uvicorn.run("osiris_api.main:app", host="0.0.0.0", port=8000, reload=False)  # nosec B104 -- konteyner ici dinleme; host baglama compose'da localhost


if __name__ == "__main__":
    run()
