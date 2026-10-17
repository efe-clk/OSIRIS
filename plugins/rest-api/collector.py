"""REST API plugin'i — JSON tabanlı API veri toplama."""

from __future__ import annotations

from typing import Any

import requests
from osiris.plugin import BaseCollector, CollectedItem, CollectionResult
from osiris.records import record_json
from osiris.security import fetch_url

_ALLOWED_METHODS = {"GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"}
_HEADERS_BASE = {"User-Agent": "OSIRIS-OSINT/0.1 (+self-hosted)"}


def _dig(obj: Any, path: str) -> Any:
    """Nokta ile ayrılmış JSON yolu üzerinde gezinir."""
    for part in path.split("."):
        if isinstance(obj, dict):
            obj = obj.get(part)
        elif isinstance(obj, list) and part.isdigit():
            obj = obj[int(part)]
        else:
            return None
    return obj


class RestApiCollector(BaseCollector):
    id = "rest-api"
    name = "REST API"
    network_type = "api"

    def collect(self, config: dict[str, Any] | None = None) -> CollectionResult:
        cfg = config or self.config
        endpoint = cfg.get("endpoint")
        if not endpoint or not isinstance(endpoint, str):
            return CollectionResult(items=[], success=False, error="endpoint gerekli")
        method = str(cfg.get("method", "GET")).upper()
        if method not in _ALLOWED_METHODS:
            return CollectionResult(items=[], success=False, error=f"İzin verilmeyen metod: {method}")
        headers = cfg.get("headers", {})
        params = cfg.get("params", {})
        if not isinstance(headers, dict) or not isinstance(params, dict):
            return CollectionResult(items=[], success=False, error="headers/params dict olmalı")

        try:
            resp = fetch_url(
                requests,
                method,
                endpoint,
                headers={**_HEADERS_BASE, **{str(k)[:200]: str(v)[:2000] for k, v in list(headers.items())[:20]}},
                params={str(k)[:200]: str(v)[:2000] for k, v in list(params.items())[:20]},
                timeout=30,
                verify=True,
            )
            resp.raise_for_status()
        except (requests.RequestException, ValueError) as exc:
            return CollectionResult(items=[], success=False, error=str(exc)[:500])

        try:
            data = resp.json()
        except ValueError:
            return CollectionResult(items=[], success=False, error="Geçersiz JSON yanıtı")
        json_path = cfg.get("json_path")
        if json_path:
            if not isinstance(json_path, str) or len(json_path) > 500:
                return CollectionResult(items=[], success=False, error="json_path geçersiz")
            data = _dig(data, json_path)

        if not isinstance(data, list):
            data = [data]

        items = []
        for record in data[:200]:
            title = record.get("title") if isinstance(record, dict) else None
            items.append(
                CollectedItem(
                    url=endpoint[:2048],
                    title=str(title)[:500] if title else None,
                    raw_content=record_json(record)[:50_000],
                    metadata={"endpoint": endpoint[:1000]},
                )
            )
        return CollectionResult(items=items, metadata={"endpoint": endpoint})

    def health_check(self) -> bool:
        endpoint = self.config.get("endpoint")
        if not endpoint or not isinstance(endpoint, str):
            return False
        try:
            return fetch_url(requests, "HEAD", endpoint, timeout=10,
                             headers=_HEADERS_BASE, verify=True,
                             max_bytes=0).status_code < 500
        except (requests.RequestException, ValueError):
            return False
