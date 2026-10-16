"""Collector → pipeline kuyruğu (Redis Streams) — docs/SOZLESME.md §5.

Tek alanlı girdiler: ``XADD osiris:stream:raw_items * data <json mesaj>``.
Tüketici grubu ``osiris-pipeline``; işlenen (ya da bozuk) girdiler ``XACK`` +
``XDEL`` ile silinir, böylece akış yalnızca işlenmemiş/bekleyen girdileri tutar.
Depolama hatası alan girdi bekleyen listesinde kalır ve ``XAUTOCLAIM`` ile
yeniden denenir; ``MAX_DELIVERIES`` aşılırsa ölü-mektup akışına taşınır.
Geçiş: eski LIST (``osiris:raw_items``) varsa önce o boşaltılır.
"""

from __future__ import annotations

import os

STREAM = "osiris:stream:raw_items"
DEAD_LETTER = "osiris:stream:raw_items:dead"
GROUP = "osiris-pipeline"
LEGACY_LIST = "osiris:raw_items"
FIELD = "data"
MAX_MESSAGE_BYTES = 512_000
MAX_DELIVERIES = 5
# Güvenlik tavanı (yaklaşık): tüketici durursa Redis belleği sınırsız büyümesin
DEFAULT_MAXLEN = 200_000
DEFAULT_CLAIM_IDLE_MS = 300_000


def stream_maxlen() -> int:
    try:
        return max(1_000, min(int(os.getenv("OSIRIS_STREAM_MAXLEN", DEFAULT_MAXLEN)), 10_000_000))
    except ValueError:
        return DEFAULT_MAXLEN


def ensure_group(client, stream: str = STREAM, group: str = GROUP) -> None:
    """Tüketici grubunu (ve akışı) oluşturur; zaten varsa sessizce geçer."""
    import redis

    try:
        client.xgroup_create(stream, group, id="0", mkstream=True)
    except redis.ResponseError as exc:
        if "BUSYGROUP" not in str(exc):
            raise


def enqueue(client, payload: str, stream: str = STREAM) -> str:
    """Mesajı akışa ekler (MAXLEN ~ tavanıyla). Girdi kimliğini döner."""
    return client.xadd(stream, {FIELD: payload}, maxlen=stream_maxlen(), approximate=True)
