"""Tor plugin'i — .onion adreslerini SOCKS5 proxy üzerinden toplar."""

from __future__ import annotations

from typing import Any

import requests
from bs4 import BeautifulSoup
from osiris.plugin import BaseCollector, CollectedItem, CollectionResult
from osiris.security import fetch_url, resolve_proxy, safe_title

_HEADERS = {"User-Agent": "OSIRIS-OSINT/0.1 (+self-hosted)"}
_MAX_BYTES = 2_000_000


class TorCollector(BaseCollector):
    id = "tor"
    name = "Tor (.onion)"
    network_type = "tor"

    def _session(self, cfg: dict[str, Any]) -> requests.Session:
        session = requests.Session()
        host, port = resolve_proxy(cfg.get("socks5_host"), cfg.get("socks5_port"),
                                   "OSIRIS_TOR_SOCKS", 9050, "SOCKS5")
        if ":" in host:
            host = f"[{host}]"
        proxy = f"socks5h://{host}:{port}"
        session.proxies = {"http": proxy, "https": proxy}
        session.headers.update(_HEADERS)
        return session

    def collect(self, config: dict[str, Any] | None = None) -> CollectionResult:
        cfg = config or self.config
        url = cfg.get("url")
        if not url or not isinstance(url, str):
            return CollectionResult(items=[], success=False, error="url gerekli")
        if ".onion" not in url:
            return CollectionResult(items=[], success=False, error=".onion adresi gerekli")

        try:
            session = self._session(cfg)
        except ValueError as exc:
            return CollectionResult(items=[], success=False, error=str(exc))
        try:
            resp = fetch_url(session, "GET", url, allow_onion=True, timeout=60,
                             verify=True, max_bytes=_MAX_BYTES)
            resp.raise_for_status()
        except (requests.RequestException, ValueError) as exc:
            return CollectionResult(items=[], success=False, error=str(exc)[:500])

        soup = BeautifulSoup(resp.text[:_MAX_BYTES], "html.parser")
        text = soup.get_text(" ", strip=True)[:50_000]
        items = [
            CollectedItem(
                url=url[:2048],
                title=safe_title(soup),
                raw_content=text,
                metadata={"network": "tor"},
            )
        ]
        return CollectionResult(items=items, metadata={"url": url, "network": "tor"})

    def health_check(self) -> bool:
        url = self.config.get("url")
        if not url or not isinstance(url, str):
            return False
        try:
            return fetch_url(self._session(self.config), "HEAD", url, allow_onion=True,
                             timeout=30, verify=True, max_bytes=0).status_code < 500
        except (requests.RequestException, ValueError):
            return False
