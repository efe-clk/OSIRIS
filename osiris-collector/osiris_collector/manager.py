"""Collector Manager — plugin yükleme ve görev yönetimi."""

from __future__ import annotations

import importlib.util
import json
import logging
import re
import time
from pathlib import Path
from typing import Any

import redis
from apscheduler.schedulers.background import BackgroundScheduler
from apscheduler.triggers.cron import CronTrigger
from osiris import queue as osiris_queue
from osiris.plugin import BaseCollector, CollectionResult

logger = logging.getLogger(__name__)

_MAX_QUEUE_ITEM_BYTES = osiris_queue.MAX_MESSAGE_BYTES
_CRON_PART_RE = re.compile(r"^[\d,/*-]+$")
_PLUGIN_ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]{0,63}$")
_UUID_RE = re.compile(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")


class CollectorManager:
    """Plugin'leri yükler, zamanlar ve koleksiyon görevlerini çalıştırır."""

    def __init__(
        self,
        plugins_dir: str | Path = "plugins",
        redis_url: str = "redis://localhost:6379/0",
        queue_name: str = osiris_queue.STREAM,
        database_url: str | None = None,
    ) -> None:
        self.plugins_dir = Path(plugins_dir)
        self.redis = redis.Redis.from_url(redis_url, decode_responses=True)
        self.queue_name = queue_name
        self.database_url = database_url
        self.plugins: dict[str, BaseCollector] = {}
        self.manifests: dict[str, dict[str, Any]] = {}
        self.scheduler = BackgroundScheduler()

    def load_plugins(self) -> int:
        """plugins/ dizinindeki tüm plugin'leri yükler."""
        loaded = 0
        if not self.plugins_dir.is_dir():
            logger.error("Plugin dizini bulunamadı: %s", self.plugins_dir)
            return 0
        for manifest_path in sorted(self.plugins_dir.glob("*/manifest.json")):
            try:
                manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError) as exc:
                logger.error("manifest okunamadı %s: %s", manifest_path, exc)
                continue
            plugin_id = manifest.get("id")
            if not plugin_id or not isinstance(plugin_id, str):
                logger.error("Geçersiz manifest (id yok): %s", manifest_path)
                continue
            if not _PLUGIN_ID_RE.match(plugin_id):
                logger.error("Geçersiz manifest id biçimi: %s", manifest_path)
                continue
            if plugin_id in self.plugins:
                logger.warning("Plugin zaten yüklü, atlanıyor: %s", plugin_id)
                continue
            collector_path = manifest_path.parent / "collector.py"
            if not collector_path.is_file():
                logger.error("collector.py bulunamadı: %s", plugin_id)
                continue
            try:
                spec = importlib.util.spec_from_file_location(
                    f"{plugin_id.replace('-', '_')}_collector", collector_path
                )
                if spec is None or spec.loader is None:
                    raise ImportError("modül spec oluşturulamadı")
                module = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(module)
                collector_cls = self._find_collector(module)
                self.plugins[plugin_id] = collector_cls()
                self.manifests[plugin_id] = manifest
            except Exception as exc:  # noqa: BLE001
                logger.error("Plugin yüklenemedi %s: %s", plugin_id, exc)
                continue
            logger.info("Plugin yüklendi: %s", plugin_id)
            loaded += 1
        return loaded

    def _missing_required(self, plugin_id: str, config: dict[str, Any]) -> list[str]:
        """Manifest config_schema'daki required alanları denetler."""
        schema = (getattr(self, "manifests", {}).get(plugin_id) or {}).get("config_schema") or {}
        if not isinstance(schema, dict):
            return []
        missing = []
        for field, spec in schema.items():
            if isinstance(spec, dict) and spec.get("required"):
                if config.get(field) in (None, ""):
                    missing.append(str(field))
        # Plugin sınıfının kendi doğrulayıcısına da danış
        try:
            extra = self.plugins[plugin_id].validate_config(config) or []
            missing.extend(m for m in extra if m not in missing)
        except Exception:  # noqa: BLE001
            pass
        return missing

    @staticmethod
    def _find_collector(module: Any) -> type[BaseCollector]:
        for attr in vars(module).values():
            if (
                isinstance(attr, type)
                and issubclass(attr, BaseCollector)
                and attr is not BaseCollector
            ):
                return attr
        raise ValueError("collector sınıfı bulunamadı")

    def run_collection(self, plugin_id: str, config: dict[str, Any]) -> CollectionResult:
        """Tek bir koleksiyon görevi çalıştırır ve sonucu kuyruğa iletir."""
        plugin = self.plugins.get(plugin_id)
        if plugin is None:
            raise KeyError(f"Plugin bulunamadı: {plugin_id}")
        if not isinstance(config, dict):
            return CollectionResult(items=[], success=False, error="config dict olmalı")
        missing = self._missing_required(plugin_id, config)
        if missing:
            return CollectionResult(
                items=[], success=False,
                error=f"Zorunlu alanlar eksik: {', '.join(missing)}",
            )

        start = time.monotonic()
        try:
            result = plugin.collect(config)
        except Exception as exc:  # noqa: BLE001 — plugin crash'i yöneticiyi düşürmemeli
            logger.exception("%s: plugin çöktü", plugin_id)
            return CollectionResult(items=[], success=False, error=f"plugin hatası: {exc}")
        elapsed_ms = int((time.monotonic() - start) * 1000)

        if result.success:
            enqueued = 0
            # Kaynak kimliği mesaja taşınır (items.source_id; KVKK saklama süresi buna bağlı)
            raw_sid = config.get("source_id")
            source_id = str(raw_sid).lower() if isinstance(raw_sid, str) and _UUID_RE.match(raw_sid) else None
            for item in result.items:
                try:
                    payload = json.dumps(
                        {
                            "plugin_id": plugin_id,
                            "source_id": source_id,
                            "item": item.model_dump(),
                            "collected_at": time.time(),
                        },
                        ensure_ascii=False,
                    )
                except (TypeError, ValueError) as exc:
                    logger.warning("%s: öğe serileştirilemedi: %s", plugin_id, exc)
                    continue
                if len(payload.encode("utf-8")) > _MAX_QUEUE_ITEM_BYTES:
                    logger.warning("%s: öğe çok büyük, atlandı", plugin_id)
                    continue
                try:
                    osiris_queue.enqueue(self.redis, payload, self.queue_name)
                    enqueued += 1
                except redis.RedisError as exc:
                    logger.error("%s: kuyruğa yazılamadı: %s", plugin_id, exc)
                    return CollectionResult(
                        items=result.items, success=False, error=f"kuyruk hatası: {exc}"
                    )
            logger.info(
                "%s: %d öğe toplandı (%d ms, %d kuyrukta)",
                plugin_id,
                len(result.items),
                elapsed_ms,
                enqueued,
            )
        else:
            logger.warning("%s: başarısız — %s", plugin_id, result.error)
        self._record_health(config, result, elapsed_ms)
        return result

    def _record_health(self, config: dict[str, Any], result: CollectionResult,
                       elapsed_ms: int) -> None:
        """Kaynak sağlık takibi (doküman §7): sources + source_metrics.

        Yalnızca config'de `source_id` varsa ve database_url tanımlıysa çalışır.
        Asla koleksiyon sonucunu bozmaz — tüm hatalar yutulur.
        """
        source_id = config.get("source_id")
        if not source_id or not self.database_url:
            return
        try:
            import psycopg

            with psycopg.connect(self.database_url, connect_timeout=5) as conn:
                with conn.cursor() as cur:
                    if result.success:
                        cur.execute(
                            """
                            UPDATE sources
                            SET last_crawled_at = NOW(), last_success_at = NOW(),
                                failure_count = 0, avg_response_ms = %s,
                                updated_at = NOW()
                            WHERE id::text = %s
                            """,
                            (elapsed_ms, str(source_id)),
                        )
                    else:
                        cur.execute(
                            """
                            UPDATE sources
                            SET last_crawled_at = NOW(),
                                failure_count = failure_count + 1,
                                avg_response_ms = %s, updated_at = NOW()
                            WHERE id::text = %s
                            """,
                            (elapsed_ms, str(source_id)),
                        )
                    cur.execute(
                        """
                        INSERT INTO source_metrics (time, source_id, response_ms, success, items_count)
                        VALUES (NOW(), NULLIF(%s,'')::uuid, %s, %s, %s)
                        """,
                        (
                            str(source_id),
                            elapsed_ms,
                            result.success,
                            len(result.items) if result.success else 0,
                        ),
                    )
                conn.commit()
        except Exception as exc:  # noqa: BLE001
            logger.debug("Sağlık kaydı yazılamadı: %s", exc)

    def get_manifest(self, plugin_id: str) -> dict[str, Any]:
        """Yüklü bir plugin'in manifestini döner (dinamik formlar için)."""
        if plugin_id not in self.plugins:
            raise KeyError(f"Plugin bulunamadı: {plugin_id}")
        if not _PLUGIN_ID_RE.match(plugin_id):
            raise ValueError("Geçersiz plugin id")
        manifest_path = self.plugins_dir / plugin_id / "manifest.json"
        try:
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as exc:
            raise ValueError(f"Manifest okunamadı: {exc}") from exc
        if not isinstance(manifest, dict):
            raise ValueError("Manifest geçersiz")
        return {
            "id": manifest.get("id", plugin_id),
            "name": manifest.get("name", plugin_id),
            "network_type": manifest.get("network_type", "custom"),
            "description": str(manifest.get("description", ""))[:1000],
            "config_schema": manifest.get("config_schema", {})
            if isinstance(manifest.get("config_schema"), dict) else {},
            "schedule_default": str(manifest.get("schedule_default", ""))[:100],
        }

    def schedule(self, plugin_id: str, cron: str, config: dict[str, Any]) -> None:
        """Bir plugin için cron tabanlı zamanlama ekler (5 alanlı cron)."""
        if plugin_id not in self.plugins:
            raise KeyError(f"Plugin bulunamadı: {plugin_id}")
        trigger = self._cron_trigger(cron)
        self.scheduler.add_job(
            self.run_collection,
            trigger,
            args=[plugin_id, config],
            id=f"{plugin_id}-{cron}",
            replace_existing=True,
            max_instances=1,
            coalesce=True,
        )
        logger.info("Zamanlandı: %s (%s)", plugin_id, cron)

    @staticmethod
    def _cron_trigger(cron: str) -> CronTrigger:
        """'m h dom mon dow' cron ifadesini doğrular ve trigger üretir."""
        parts = cron.strip().split()
        if len(parts) != 5 or any(not _CRON_PART_RE.match(p) for p in parts):
            raise ValueError(f"Geçersiz cron (5 alan gerekli): {cron!r}")
        minute, hour, day, month, dow = parts
        return CronTrigger(
            minute=minute, hour=hour, day=day, month=month, day_of_week=dow
        )

    def start(self) -> None:
        self.scheduler.start()
        logger.info("Collector Manager başlatıldı")

    def shutdown(self) -> None:
        self.scheduler.shutdown(wait=False)
        logger.info("Collector Manager durduruldu")
