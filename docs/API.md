> **Türkçe:** Bu belgenin Türkçe sürümü için [API.tr.md](API.tr.md) dosyasına bakın.

# OSIRIS API Reference

> Version: 1.0.0-rc · Implementation: Rust (`osiris-server api`) — contract pinned by `tests/contract/`
> Interactive docs: `GET /docs` (Swagger UI) behind Nginx at `/api/docs`; OpenAPI 3: `GET /openapi.json`
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
| `POST /search`, `/search/semantic`, `/search/entity`, `GET /items/*`, `/entities/top` | ✓ | ✓ | ✓ |
| `GET /sources`, `/sources/{id}`, `/saved-queries`, `/graph`, `/graph/analysis`, `/graph/export` | ✓ | ✓ | ✓ |
| `POST /collect`, `/collect/batch` | ✗ (403) | ✓ | ✓ |
| `POST /sources`, `PATCH /sources/*`, `DELETE /sources/*`, `/sources/*/collect` | ✗ | ✓ | ✓ |
| `POST /saved-queries`, `DELETE`, `/alerts/test` | ✗ | ✓ | ✓ |
| `POST /graph/relation`, `/reports/markdown`, `/reports/generate` | ✗ | ✓ | ✓ |
| `DELETE /items/{id}`, `POST /entities/erase`, `POST /admin/retention/run` | ✗ | ✗ | ✓ |

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

# With previews: include_items → "results" (up to 1000 items; snippet ≤ 500 chars). The web UI
# (username scan, collect) uses this. For /sources/<id>/collect pass ?include_items=true.
curl -X POST https://<host>:8443/api/collect -H "X-API-Key: <k>" -H 'Content-Type: application/json' \
  -d '{"plugin_id":"username-search","config":{"username":"example"},"include_items":true}'
# → {"items":2,"metadata":{...},"results":[{"url":"...","title":"...","snippet":"...","published_at":null,"metadata":{...},"tags":[...]}]}

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

### Semantic search

```bash
curl -X POST https://<host>:8443/api/search/semantic -H "X-API-Key: <k>" \
  -H 'Content-Type: application/json' -d '{"query":"ransomware attack","limit":20}'
# → [{"id":"...","title":"...","url":"...","cleaned_content":"...","collected_at":"...","similarity":0.83}]
```

The API computes the query vector (same model as the worker, SOZLESME §10). Without `OSIRIS_EMBEDDING_MODEL` or the downloaded models (`docker compose --profile models run --rm osiris-models`) it returns `503`; the rest of the API is unaffected. The model loads on the first search. Only items with `embedding_v2` are searched.

## 8. Sources

```bash
# List (filters + pagination)
curl -H "X-API-Key: <k>" "https://<host>:8443/api/sources?enabled=true&limit=50"

# Create — plugin-specific knobs go in config (see /plugins schemas)
curl -X POST https://<host>:8443/api/sources -H "X-API-Key: <k>" \
  -H 'Content-Type: application/json' \
  -d '{"name":"Örnek RSS","plugin_id":"rss","network_type":"rss",
       "url":"https://example.com/rss","schedule":"*/15 * * * *",
       "config":{"feed_url":"https://example.com/rss","max_items":50},
       "retention_days":90}'
# → {"id":"..."}

# Run once (records source_id → health metrics land in sources/source_metrics)
curl -X POST https://<host>:8443/api/sources/<id>/collect -H "X-API-Key: <k>"

# Delete
curl -X DELETE https://<host>:8443/api/sources/<id> -H "X-API-Key: <k>"
# Detail (includes plugin config; keys containing api_key/token/password/secret come back as "********")
curl -H "X-API-Key: <k>" https://<host>:8443/api/sources/<id>

# Update — only the given fields change: enabled, schedule, priority, name, url, tags,
# retention_days, config. config merges shallowly: null removes a key, "********" keeps the stored secret.
curl -X PATCH https://<host>:8443/api/sources/<id> -H "X-API-Key: <k>" \
  -H 'Content-Type: application/json' -d '{"enabled":false,"schedule":"0 */6 * * *"}'
# → {"status":"updated"}   (empty body 422, unknown source 404, config > 20 KB 413)
```

Source rows carry health: `last_crawled_at`, `last_success_at`, `failure_count`, `avg_response_ms`.

`retention_days` (KVKK): omitted/`null` → server default (`OSIRIS_RETENTION_DAYS`, 180), `0` → never expire, `1-36500` → items older than N days are purged. Returns `503` if migration 004 is not applied yet.

## 9. Items & Entities

```bash
curl -H "X-API-Key: <k>" "https://<host>:8443/api/items/recent?limit=20&offset=0"
curl -H "X-API-Key: <k>" https://<host>:8443/api/items/<id>        # + embedded entities[]
curl -H "X-API-Key: <k>" "https://<host>:8443/api/entities/top?entity_type=domain&limit=20"
# → [{"type":"domain","value":"example.com","mentions":12,"last_seen":"..."}]
```

### Privacy / erasure (KVKK, admin only)

```bash
# Delete one item (and entities no longer referenced by any item)
curl -X DELETE https://<host>:8443/api/items/<id> -H "X-API-Key: <admin>"
# → {"status":"deleted","items_deleted":1,"entities_deleted":2}

# Erase a data subject: entity + every item mentioning it + graph edges with that name
curl -X POST https://<host>:8443/api/entities/erase -H "X-API-Key: <admin>" \
  -H 'Content-Type: application/json' \
  -d '{"entity_type":"email","value":"ali@example.com","delete_items":true}'
# → {"items_deleted":4,"entities_deleted":3,"edges_deleted":1}

# Run retention purge now (normally done by osiris-pipeline --loop)
curl -X POST https://<host>:8443/api/admin/retention/run -H "X-API-Key: <admin>"
# → {"items_deleted":120,"entities_deleted":37,"default_days":180}
```

Value matching is case-insensitive. The audit log records only `value_sha256`, never the erased value.

## 10. Graph

```bash
curl -H "X-API-Key: <k>" https://<host>:8443/api/graph
# {"nodes":[{"id":"..."}],"edges":[{"source":"...","target":"...","relation_type":"..."}]}

curl -X POST https://<host>:8443/api/graph/relation -H "X-API-Key: <k>" \
  -H 'Content-Type: application/json' \
  -d '{"source":"example.com","target":"1.2.3.4","relation_type":"resolves"}'
# persisted to graph_edges (survives restarts)
# Analysis: degree centrality (top N, 1-200), CNM communities (largest 50; up to 5000 nodes),
# neighbours when node is given
curl -H "X-API-Key: <k>" "https://<host>:8443/api/graph/analysis?top=10&node=example.com"

# Export (Gephi etc.): format ∈ graphml|gexf → XML body
curl -H "X-API-Key: <k>" "https://<host>:8443/api/graph/export?format=gexf" -o osiris.gexf
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
# Multi-format: format ∈ markdown|html|json|stix. STIX/JSON indicators go in `entities`
# (STIX only takes ip, domain, email, crypto_address, hash, cve). Never written to disk.
curl -X POST https://<host>:8443/api/reports/generate -H "X-API-Key: <k>" \
  -H 'Content-Type: application/json' \
  -d '{"title":"Case 12","format":"stix","entities":[{"type":"ip","value":"203.0.113.7"}]}'
# → {"format":"stix","media_type":"application/stix+json","filename":"Case-12.stix.json","content":"{...}"}
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
| 409 | Source disabled (`/sources/{id}/collect`) |
| 413 | Config object too large, or request body over 5 MiB |
| 422 | Schema violation (see `detail` array) |
| 429 | Per-identity quota exceeded (`OSIRIS_API_RATE_PER_MIN`) |
| 500 | Server/DB failure (no internals leaked) |
| 502 | Plugin ran but failed (`detail` carries the plugin message) |
| 503 | Feature not configured (`/auth/token` without `OSIRIS_JWT_SECRET`) |

## 15. Rate Limits

Two layers:

- **Nginx** (per IP): `10 req/s` (burst 20) on `/api/*`.
- **API** (per identity — operator key, DB key user or JWT subject): `OSIRIS_API_RATE_PER_MIN` requests per minute (default 600, `0` = off); exceeding it returns `429` `{"detail":"İstek kotası aşıldı"}`.

Front large batch jobs with `/collect/batch` (≤10) and client-side pacing.

**Metrics:** Prometheus text format on a separate listener, `OSIRIS_METRICS_ADDR` (compose: `osiris-api:9464/metrics`, internal network only — not under `/api/`). Labels are route templates, never path values or query strings.

## 16. Changelog

- **Web UI** — `POST /search/semantic`, `GET`/`PATCH /sources/{id}` (secrets masked, config merge), `GET /graph/analysis`, `GET /graph/export`, `POST /reports/generate` (Markdown/HTML/JSON/STIX); `include_items` preview for `/collect` and `/sources/{id}/collect`; `PATCH` in CORS. E11 updated: functions already in the libraries are now exposed as endpoints.
- **1.0.0-rc (Rust R7)** — Python implementation removed; same contract. `osiris-server migrate` applies the embedded migrations (006 drops the legacy `items.embedding` column; audit chain is v2-only).
- **Rust R6** — API reimplemented in Rust (axum) with an unchanged contract; per-identity quota (`429`); `/metrics` (separate listener); structured JSON logs (`OSIRIS_LOG_FORMAT=json`); `/openapi.json`; timestamps keep the FastAPI format (`…Z`). `/graph/relation` with an id that is blank after trimming now returns `400` instead of `500`.
- **Unreleased (Rust R2)** — KVKK: `DELETE /items/{id}`, `POST /entities/erase`, `POST /admin/retention/run`, `retention_days` on sources; refuses to start without `OSIRIS_API_KEY`; JWT secret has no API-key fallback (`/auth/token` → 503); audit chain v2 (RFC 8785).
- **0.2.0** — pagination + filters on lists; `GET /items/{id}` (+entities), `GET /entities/top`, `POST /collect/batch`; UUID-safe id paths; OpenAPI tags/summaries; LIKE-wildcard escaping on `q`.
- **0.1.x** — sources CRUD + collect trigger, saved-queries CRUD, `/alerts/test`, `/reports/markdown`, `/stats`, `/items/recent`, JWT + RBAC, `/auth/keys`.
