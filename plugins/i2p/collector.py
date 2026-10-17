"""I2P plugin'i — eepsite'ları I2P HTTP proxy üzerinden toplar."""

from __future__ import annotations

from typing import Any

import requests
from bs4 import BeautifulSoup
from osiris.plugin import BaseCollector, CollectedItem, CollectionResult
from osiris.security import fetch_url, resolve_proxy, safe_title

_HEADERS = {"User-Agent": "OSIRIS-OSINT/0.1 (+self-hosted)"}
_MAX_BYTES = 2_000_000


class I2PCollector(BaseCollector):
    id = "i2p"
    name = "I2P (eepsite)"
    network_type = "i2p"

    def _session(self, cfg: dict[str, Any]) -> requests.Session:
        session = requests.Session()
        host, port = resolve_proxy(cfg.get("http_proxy_host"), cfg.get("http_proxy_port"),
                                   "OSIRIS_I2P_HTTP_PROXY", 4444, "I2P")
        if ":" in host:
            host = f"[{host}]"
        proxy = f"http://{host}:{port}"
        session.proxies = {"http": proxy, "https": proxy}
        session.headers.update(_HEADERS)
        return session

    def collect(self, config: dict[str, Any] | None = None) -> CollectionResult:
        cfg = config or self.config
        url = cfg.get("url")
        if not url or not isinstance(url, str):
            return CollectionResult(items=[], success=False, error="url gerekli")

        try:
            session = self._session(cfg)
        except ValueError as exc:
            return CollectionResult(items=[], success=False, error=str(exc))
        try:
            resp = fetch_url(session, "GET", url, allow_i2p=True, timeout=60, verify=True,
                             max_bytes=_MAX_BYTES)
            resp.raise_for_status()
        except (requests.RequestException, ValueError) as exc:
            return CollectionResult(items=[], success=False, error=str(exc)[:500])

        soup = BeautifulSoup(resp.text[:_MAX_BYTES], "html.parser")
        items = [
            CollectedItem(
                url=url[:2048],
                title=safe_title(soup),
                raw_content=soup.get_text(" ", strip=True)[:50_000],
                metadata={"network": "i2p"},
            )
        ]
        return CollectionResult(items=items, metadata={"url": url, "network": "i2p"})

    def health_check(self) -> bool:
        url = self.config.get("url")
        if not url or not isinstance(url, str):
            return False
        try:
            return fetch_url(self._session(self.config), "HEAD", url, allow_i2p=True, timeout=30,
                             verify=True, max_bytes=0).status_code < 500
        except (requests.RequestException, ValueError):
            return False
