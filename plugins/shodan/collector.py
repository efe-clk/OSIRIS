"""Shodan plugin'i — internet tarama API entegrasyonu."""

from __future__ import annotations

from typing import Any

import requests
from osiris.plugin import BaseCollector, CollectedItem, CollectionResult
from osiris.records import record_json
from osiris.security import fetch_url

_HEADERS = {"User-Agent": "OSIRIS-OSINT/0.1 (+self-hosted)"}


class ShodanCollector(BaseCollector):
    id = "shodan"
    name = "Shodan"
    network_type = "api"

    def collect(self, config: dict[str, Any] | None = None) -> CollectionResult:
        cfg = config or self.config
        api_key = cfg.get("api_key")
        query = cfg.get("query")
        if not api_key or not query or not isinstance(query, str):
            return CollectionResult(items=[], success=False, error="api_key ve query gerekli")
        if len(query) > 500:
            return CollectionResult(items=[], success=False, error="query çok uzun")

        try:
            resp = fetch_url(
                requests,
                "GET",
                "https://api.shodan.io/shodan/host/search",
                params={"key": str(api_key), "query": query},
                timeout=30,
                headers=_HEADERS,
                verify=True,
            )
            resp.raise_for_status()
        except ValueError as exc:
            return CollectionResult(items=[], success=False, error=f"Shodan hatası: {exc}"[:300])
        except requests.RequestException as exc:
            # API anahtarını hataya yazma
            status = getattr(getattr(exc, "response", None), "status_code", None)
            if status == 401:
                return CollectionResult(items=[], success=False, error="Shodan: yetkisiz (API anahtarı geçersiz)")
            if status == 429:
                return CollectionResult(items=[], success=False, error="Shodan: hız limiti aşıldı")
            return CollectionResult(items=[], success=False, error=f"Shodan hatası: {type(exc).__name__}")

        try:
            payload = resp.json()
        except ValueError:
            return CollectionResult(items=[], success=False, error="Shodan: geçersiz yanıt")

        items = []
        for match in payload.get("matches", [])[:100]:
            if not isinstance(match, dict):
                continue
            items.append(
                CollectedItem(
                    url=f"http://{match.get('ip_str')}:{match.get('port')}"[:2048],
                    title=str(match.get("product", ""))[:500] or None,
                    raw_content=record_json(match)[:50_000],
                    metadata={"ip": match.get("ip_str"), "port": match.get("port")},
                )
            )
        # api_key'yi metadata'ya yazma!
        return CollectionResult(items=items, metadata={"query": query, "count": len(items)})

    def health_check(self) -> bool:
        return bool(self.config.get("api_key"))
