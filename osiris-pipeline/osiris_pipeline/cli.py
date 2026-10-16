"""Processing Pipeline CLI — kuyruk işçisi.

Kullanım:
  osiris-pipeline --once                # kuyruğu bir kez boşalt
  osiris-pipeline --loop --interval 10  # sürekli çalış (Ctrl+C ile dur)
  osiris-pipeline --purge               # süresi dolan öğeleri sil (KVKK) ve çık
  osiris-pipeline --reembed             # embedding_v2'si olmayan öğeleri vektörleştir ve çık
"""

from __future__ import annotations

import argparse
import logging
import os
import sys
import time

from osiris_pipeline.pipeline import ProcessingPipeline

logger = logging.getLogger(__name__)


def build_pipeline(args: argparse.Namespace) -> ProcessingPipeline:
    return ProcessingPipeline(
        redis_url=args.redis_url,
        queue_name=args.queue,
        stream_name=args.stream,
        database_url=args.database_url,
        embedding_model=args.embedding_model,
    )


def run_purge(database_url: str) -> dict[str, int]:
    import psycopg
    from osiris.retention import purge_expired

    with psycopg.connect(database_url, connect_timeout=10) as conn:
        return purge_expired(conn)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="osiris-pipeline", description="OSIRIS Processing Pipeline işçisi")
    parser.add_argument("--redis-url",
                        default=os.getenv("OSIRIS_REDIS_URL", "redis://localhost:6379/0"))
    parser.add_argument("--queue", default="osiris:raw_items",
                        help="Eski LIST kuyruğu (geçiş: önce boşaltılır)")
    parser.add_argument("--stream", default="osiris:stream:raw_items",
                        help="Redis Stream (asıl kuyruk; grup osiris-pipeline)")
    parser.add_argument("--database-url", default=os.getenv("OSIRIS_DATABASE_URL"))
    parser.add_argument("--embedding-model", default=os.getenv("OSIRIS_EMBEDDING_MODEL"))
    parser.add_argument("--batch", type=int, default=50)
    parser.add_argument("--once", action="store_true", help="Tek tur çalışıp çık")
    parser.add_argument("--loop", action="store_true", help="Sürekli çalış")
    parser.add_argument("--interval", type=int, default=10, help="Tur arası saniye")
    parser.add_argument("--purge", action="store_true",
                        help="Saklama süresi dolan öğeleri silip çık (KVKK)")
    parser.add_argument("--reembed", action="store_true",
                        help="embedding_v2 boş öğeleri OSIRIS_EMBEDDING_MODEL ile doldurup çık")
    parser.add_argument("--purge-interval", type=int,
                        default=int(os.getenv("OSIRIS_RETENTION_INTERVAL", "3600")),
                        help="--loop'ta saklama temizliği aralığı, sn (0 = kapalı)")
    parser.add_argument("-v", "--verbose", action="store_true")
    args = parser.parse_args(argv)

    logging.basicConfig(
        level=logging.DEBUG if args.verbose else logging.INFO,
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )
    if args.batch < 1 or args.batch > 1000:
        parser.error("--batch 1-1000 aralığında olmalı")
    if args.interval < 1 or args.interval > 3600:
        parser.error("--interval 1-3600 aralığında olmalı")

    if args.purge_interval < 0 or args.purge_interval > 7 * 86400:
        parser.error("--purge-interval 0-604800 aralığında olmalı")

    if args.purge:
        if not args.database_url:
            parser.error("--purge için --database-url (OSIRIS_DATABASE_URL) gerekli")
        res = run_purge(args.database_url)
        print(f"{res['items_deleted']} öğe, {res['entities_deleted']} yetim varlık silindi")
        return 0

    pipeline = build_pipeline(args)
    if args.reembed:
        if not args.database_url or not pipeline.embedding_model_name:
            parser.error("--reembed için OSIRIS_DATABASE_URL ve OSIRIS_EMBEDDING_MODEL gerekli")
        total = pipeline.reembed()
        print(f"{total} öğe yeniden vektörleştirildi")
        return 0
    if args.once or not args.loop:
        total = pipeline.run(batch_size=args.batch)
        print(f"{total} kayıt işlendi")
        return 0
    logger.info("Pipeline döngüsü başladı (her %ds)", args.interval)
    next_purge = time.monotonic()
    try:
        while True:
            if args.database_url and args.purge_interval and time.monotonic() >= next_purge:
                next_purge = time.monotonic() + args.purge_interval
                try:
                    run_purge(args.database_url)
                except Exception as exc:  # noqa: BLE001 — temizlik hatası işçiyi düşürmez
                    logger.error("Saklama temizliği başarısız: %s", exc)
            total = pipeline.run(batch_size=args.batch)
            if total:
                logger.info("%d kayıt işlendi", total)
            time.sleep(args.interval)
    except KeyboardInterrupt:
        logger.info("Durduruldu")
        return 0


if __name__ == "__main__":
    sys.exit(main())
