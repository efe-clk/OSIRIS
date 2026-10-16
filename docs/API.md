> **Türkçe:** Bu belgenin Türkçe sürümü için [API.tr.md](API.tr.md) dosyasına bakın.

# OSIRIS API Reference

> Version: 0.2.0 · Interactive docs: `GET /docs` (Swagger UI) behind Nginx at `/api/docs`
> Base URL (production): `https://<host>:8443/api` · Direct (dev): `http://localhost:8001`

All endpoints return JSON. Timestamps are ISO-8601 UTC. IDs are UUID strings.

## Contents

1. [Authentication](#1-authentication)
2. [Roles & Access Matrix](#2-roles--access-matrix)
3. [Conventions](#3-conventions)
4. [Health](#4-health)
5. [Auth Endpoints](#5-auth-endpoints)
6. [Plugins & Collection](#6-plugins--collection)
7. [Search](#7-search)
8. [Sources](#8-sources)
9. [Items & Entities](#9-items--entities)
10. [Graph](#10-graph)
11. [Alerts](#11-alerts)
12. [Reports](#12-reports)
13. [Stats](#13-stats)
14. [Errors](#14-errors)
15. [Rate Limits](#15-rate-limits)
16. [Changelog](#16-changelog)

---

## 1. Authentication

Three interchangeable credentials (send **one** per request):

```bash
# Operator / user API key
curl -H "X-API-Key: <key>" https://<host>:8443/api/plugins

# Short-lived JWT (1h, HS256)
curl -H "Authorization: Bearer <token>" https://<host>:8443/api/plugins
```

Mint a token (operator key required):

```bash
curl -X POST https://<host>:8443/api/auth/token \
  -H 'Content-Type: application/json' \
  -d '{"api_key":"<OSIRIS_API_KEY>","role":"analyst"}'
# → {"access_token":"...","token_type":"bearer","expires_in":3600}
```

Per-user keys (admin only) — raw value is returned **once** and never stored:

```bash
curl -X POST https://<host>:8443/api/auth/keys \
  -H "X-API-Key: <OSIRIS_API_KEY>" -H 'Content-Type: application/json' \
  -d '{"username":"ali","role":"analyst","name":"laptop"}'
# → {"id":"...","api_key":"osiris_...","role":"analyst"}
```

If `OSIRIS_API_KEY` is unset, the API **refuses to start**. Unauthenticated mode must be requested explicitly with `OSIRIS_INSECURE_OPEN=1` (trusted network only; every caller is `admin`).

JWTs are signed **only** with `OSIRIS_JWT_SECRET` (no fallback to the API key). If it is unset, `POST /auth/token` returns `503` and bearer tokens are rejected; API keys keep working.

## 2. Roles & Access Matrix

Roles ascend: `viewer` → `analyst` → `admin`. Unknown roles are denied.

| Endpoint | viewer | analyst | admin |
|----------|:------:|:-------:|:-----:|
| `GET /health` | open | open | open |
| `POST /auth/token` | operator key (body) | — | — |
| `GET/POST /auth/keys`, `DELETE /auth/keys/{id}` | ✗ | ✗ | ✓ |
| `GET /plugins`, `GET /stats` | ✓ | ✓ | ✓ |
| `POST /search`, `/search/entity`, `GET /items/*`, `/entities/top` | ✓ | ✓ | ✓ |
| `GET /sources`, `/saved-queries`, `/graph` | ✓ | ✓ | ✓ |
| `POST /collect`, `/collect/batch` | ✗ (403) | ✓ | ✓ |
| `POST /sources`, `DELETE /sources/*`, `/sources/*/collect` | ✗ | ✓ | ✓ |
| `POST /saved-queries`, `DELETE`, `/alerts/test` | ✗ | ✓ | ✓ |
| `POST /graph/relation`, `/reports/markdown` | ✗ | ✓ | ✓ |

## 3. Conventions

- **Pagination** (list endpoints): `?limit=` (default 100, max 200) + `?offset=` (≥0). `/items/recent` caps at 50.
- **Filters**: `GET /sources?enabled=true&plugin_id=rss&q=name`; `GET /saved-queries?alert_enabled=true`; `GET /entities/top?entity_type=email&limit=20`.
- **IDs in paths** must be UUIDs — anything else returns `404` (never `500`).
- **Request caps**: config objects ≤ 50 keys and ≤ 20 KB serialized; queries ≤ 500 chars; bodies have per-field `max_length`.
- **Partial success**: `/collect/batch` always returns `200` with per-job `{ok, items, error}` plus `{ok, total}` summary.

## 4. Health

```bash
curl https://<host>:8443/api/health
# {"status":"ok","service":"osiris-api"}
```

## 5. Auth Endpoints

| Method & Path | Role | Description |
|---------------|------|-------------|
| `POST /auth/token` | operator key in body | Mint JWT (`role`: viewer/analyst/admin) |
| `POST /auth/keys` | admin | Create user key (`{username, role, name?}`) |
| `GET /auth/keys` | admin | List keys (**hashes/prefixes only**, never raw values) |
| `DELETE /auth/keys/{id}` | admin | Revoke (row kept for audit) |

## 6. Plugins & Collection

```bash
# Inventory incl. dynamic config schemas (drive source forms from this)
curl -H "X-API-Key: <k>" https://<host>:8443/api/plugins
# [{"id":"rss","name":"RSS/Atom Feed","network_type":"rss",
#   "description":"...","config_schema":{"feed_url":{...}},"schedule_default":"*/15 * * * *"}]

# Single job
curl -X POST https://<host>:8443/api/collect -H "X-API-Key: <k>" \
  -H 'Content-Type: application/json' \
  -d '{"plugin_id":"rss","config":{"feed_url":"https://example.com/rss"}}'
# → {"items":3,"metadata":{...}}  (502 with plugin error message on failure)

# Batch (≤10 jobs, sequential, per-job results)
curl -X POST https://<host>:8443/api/collect/batch -H "X-API-Key: <k>" \
  -H 'Content-Type: application/json' \
  -d '{"jobs":[{"plugin_id":"rss","config":{...}},{"plugin_id":"dns-whois","config":{"domain":"example.com"}}]}'
# → {"jobs":[{"plugin_id":"rss","ok":true,"items":3,"error":null},...],"ok":1,"total":2}
```

## 7. Search

```bash
curl -X POST https://<host>:8443/api/search -H "X-API-Key: <k>" \
  -H 'Content-Type: application/json' -d '{"query":"saldırı","limit":20}'
# → [{"id":"...","title":"...","url":"...","cleaned_content":"...","collected_at":"..."}]

curl -X POST https://<host>:8443/api/search/entity -H "X-API-Key: <k>" \
  -H 'Content-Type: application/json' -d '{"entity_type":"email","value":"a@b.co","limit":20}'
```

Entity types: `person, org, location, ip, domain, email, phone, crypto_address, hash, username, cve, custom`.

## 8. Sources

```bash
# List (filters + pagination)
curl -H "X-API-Key: <k>" "https://<host>:8443/api/sources?enabled=true&limit=50"

# Create — plugin-specific knobs go in config (see /plugins schemas)
curl -X POST https://<host>:8443/api/sources -H "X-API-Key: <k>" \
  -H 'Content-Type: application/json' \
  -d '{"name":"Örnek RSS","plugin_id":"rss","network_type":"rss",
       "url":"https://example.com/rss","schedule":"*/15 * * * *",
       "config":{"feed_url":"https://example.com/rss","max_items":50}}'
# → {"id":"..."}

# Run once (records source_id → health metrics land in sources/source_metrics)
curl -X POST https://<host>:8443/api/sources/<id>/collect -H "X-API-Key: <k>"

# Delete
curl -X DELETE https://<host>:8443/api/sources/<id> -H "X-API-Key: <k>"
```

Source rows carry health: `last_crawled_at`, `last_success_at`, `failure_count`, `avg_response_ms`.

## 9. Items & Entities

```bash
curl -H "X-API-Key: <k>" "https://<host>:8443/api/items/recent?limit=20&offset=0"
curl -H "X-API-Key: <k>" https://<host>:8443/api/items/<id>        # + embedded entities[]
curl -H "X-API-Key: <k>" "https://<host>:8443/api/entities/top?entity_type=domain&limit=20"
# → [{"type":"domain","value":"example.com","mentions":12,"last_seen":"..."}]
```

## 10. Graph

```bash
curl -H "X-API-Key: <k>" https://<host>:8443/api/graph
# {"nodes":[{"id":"..."}],"edges":[{"source":"...","target":"...","relation_type":"..."}]}

curl -X POST https://<host>:8443/api/graph/relation -H "X-API-Key: <k>" \
  -H 'Content-Type: application/json' \
  -d '{"source":"example.com","target":"1.2.3.4","relation_type":"resolves"}'
# persisted to graph_edges (survives restarts)
```

## 11. Alerts

```bash
curl -X POST https://<host>:8443/api/saved-queries -H "X-API-Key: <k>" \
  -H 'Content-Type: application/json' \
  -d '{"name":"Kritik","query_text":"saldırı","query_type":"fts","alert_enabled":true}'
# query_type ∈ fts|semantic|regex|entity|graph

# Dry-run text against enabled rules (creates nothing)
curl -X POST https://<host>:8443/api/alerts/test -H "X-API-Key: <k>" \
  -H 'Content-Type: application/json' -d '{"text":"büyük saldırı oldu"}'
# → [{"query_id":"...","query_name":"Kritik","item_id":null,"matched":"saldırı"}]
```

## 12. Reports

```bash
curl -X POST https://<host>:8443/api/reports/markdown -H "X-API-Key: <k>" \
  -H 'Content-Type: application/json' \
  -d '{"title":"R","scope":"S","summary":"O",
       "findings":[{"title":"B","description":"D"}],"sources":["k"]}'
# → {"markdown":"# R\n..."}
```

## 13. Stats

```bash
curl -H "X-API-Key: <k>" https://<host>:8443/api/stats
# {"sources":4,"sources_enabled":4,"items":128,"entities":342,"edges":57,
#  "saved_queries":3,"alerts":2,"latest_item_at":"..."}
```

## 14. Errors

| Code | Meaning |
|------|---------|
| 400 | Bad value (`query_type`, `entity_type`, role, network) |
| 401 | Missing/invalid credential |
| 403 | Valid credential, insufficient role |
| 404 | Unknown plugin/source/item/key, or malformed UUID |
| 413 | Config object too large |
| 422 | Schema violation (see `detail` array) |
| 500 | Server/DB failure (no internals leaked) |
| 502 | Plugin ran but failed (`detail` carries the plugin message) |
| 503 | Feature not configured (`/auth/token` without `OSIRIS_JWT_SECRET`) |

## 15. Rate Limits

Enforced at Nginx: `10 req/s` (burst 20) on `/api/*`. No per-key quotas yet — front large batch jobs with `/collect/batch` (≤10) and client-side pacing.

## 16. Changelog

- **0.2.0** — pagination + filters on lists; `GET /items/{id}` (+entities), `GET /entities/top`, `POST /collect/batch`; UUID-safe id paths; OpenAPI tags/summaries; LIKE-wildcard escaping on `q`.
- **0.1.x** — sources CRUD + collect trigger, saved-queries CRUD, `/alerts/test`, `/reports/markdown`, `/stats`, `/items/recent`, JWT + RBAC, `/auth/keys`.
