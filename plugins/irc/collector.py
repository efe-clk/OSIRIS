"""IRC plugin'i — kanal mesajlarını toplar."""

from __future__ import annotations

import socket
from typing import Any

from osiris.plugin import BaseCollector, CollectedItem, CollectionResult
from osiris.security import (
    safe_create_connection,
    sanitize_channel,
    sanitize_hostname,
    sanitize_irc_token,
)


class IrcCollector(BaseCollector):
    id = "irc"
    name = "IRC"
    network_type = "irc"

    def collect(self, config: dict[str, Any] | None = None) -> CollectionResult:
        cfg = config or self.config
        server = cfg.get("server")
        channels = cfg.get("channels") or []
        if not server or not channels or not isinstance(server, str):
            return CollectionResult(items=[], success=False, error="server ve channels gerekli")
        try:
            server = sanitize_hostname(server)
            channels = [sanitize_channel(str(c)) for c in channels[:10]]
            nick = sanitize_irc_token(str(cfg.get("nickname", "osiris-bot")))
            try:
                port = int(cfg.get("port", 6667))
            except (TypeError, ValueError):
                return CollectionResult(items=[], success=False, error="Geçersiz port")
            if not 1 <= port <= 65535:
                return CollectionResult(items=[], success=False, error="Geçersiz port")
        except ValueError as exc:
            return CollectionResult(items=[], success=False, error=f"Geçersiz config: {exc}")

        sock: socket.socket | None = None
        try:
            # SSRF: iç ağ host'ları reddedilir, bağlantı doğrulanmış IP'ye açılır
            sock = safe_create_connection(server, port, timeout=15)
            sock.settimeout(10)
            sock.sendall(f"NICK {nick}\r\n".encode("utf-8", errors="ignore"))
            sock.sendall(f"USER {nick} 0 * :OSIRIS\r\n".encode("utf-8", errors="ignore"))

            items: list[CollectedItem] = []
            for channel in channels:
                sock.sendall(f"JOIN {channel}\r\n".encode("utf-8", errors="ignore"))
            # Kısa bir süre mesajları dinle (toplam bütçe ~20 sn)
            import time

            deadline = time.monotonic() + 20
            sock.settimeout(5)
            try:
                while time.monotonic() < deadline and len(items) < 200:
                    try:
                        data = sock.recv(4096).decode(errors="replace")
                    except TimeoutError:
                        break
                    if not data:
                        break
                    for line in data.splitlines()[:100]:
                        if line.startswith("PING"):
                            token = line.split(" ", 1)[1] if " " in line else ""
                            try:
                                sock.sendall(f"PONG {token}\r\n".encode("utf-8", errors="ignore"))
                            except OSError:
                                pass
                            continue
                        if " PRIVMSG " in line:
                            items.append(
                                CollectedItem(
                                    raw_content=line[:2000],
                                    metadata={"server": server, "protocol": "irc"},
                                )
                            )
            except TimeoutError:
                pass
            return CollectionResult(items=items, metadata={"server": server})
        except ValueError as exc:
            return CollectionResult(items=[], success=False, error=f"Geçersiz config: {exc}")
        except OSError as exc:
            return CollectionResult(items=[], success=False, error=str(exc)[:500])
        finally:
            if sock is not None:
                try:
                    sock.close()
                except OSError:
                    pass

    def health_check(self) -> bool:
        server = self.config.get("server")
        if not server or not isinstance(server, str):
            return False
        try:
            server = sanitize_hostname(server)
            port = int(self.config.get("port", 6667))
            if not 1 <= port <= 65535:
                return False
            sock = safe_create_connection(server, port, timeout=10)
            sock.close()
            return True
        except (OSError, ValueError):
            return False
