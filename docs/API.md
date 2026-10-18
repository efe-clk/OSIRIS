> **Türkçe:** [API.tr.md](API.tr.md)

# OSIRIS REST API Reference

> Version 1.0.0-rc · Implementation: `osiris-server api` (Rust, axum) · Binding definition: `tests/contract/`

This document covers every endpoint, the request/response formats, the limits and the error
codes. Values in the examples are made up (`example.com`, the `203.0.113.0/24` documentation
block). Error messages (`detail`) are in Turkish; their meaning is given in the tables.

## Contents

1. [Basics](#1-basics)
2. [Roles and permission matrix](#2-roles-and-permission-matrix)
3. [Common rules](#3-common-rules)
4. [Health and identity](#4-health-and-identity)
5. [Plugins and collection](#5-plugins-and-collection)
6. [Search](#6-search)
7. [Items and entities](#7-items-and-entities)
8. [Sources](#8-sources)
9. [Graph](#9-graph)
10. [Saved queries and alerts](#10-saved-queries-and-alerts)
11. [Reports](#11-reports)
12. [Privacy and retention (KVKK/GDPR)](#12-privacy-and-retention-kvkkgdpr)
13. [Statistics](#13-statistics)
14. [Errors](#14-errors)
15. [Rate limits and metrics](#15-rate-limits-and-metrics)
16. [Change history](#16-change-history)

---

## 1. Basics

| | |
|---|---|
| Base URL (through Nginx) | `https://<server>:8443/api` — Nginx strips the `/api` prefix |
| Direct (on the server only / via SSH tunnel) | `http://127.0.0.1:8001` |
| Internal Docker network (n8n, other services) | `http://osiris-api:8000` |
| OpenAPI 3 document | `GET /openapi.json` |
| Swagger UI | `GET /docs` |
| Format | JSON request and response bodies (`Content-Type: application/json`); graph export is XML |
| Time | UTC, ISO 8601: `2026-10-18T12:49:29.085604Z` (no fraction when microseconds are zero) |
| Identifiers | UUID strings |

> **Swagger UI and Nginx:** the `/docs` page loads the Swagger UI files from `cdn.jsdelivr.net`
> and runs an inline script. Nginx's Content-Security-Policy (`default-src 'self'`) does not allow
> this, so `https://<server>:8443/api/docs` renders blank in a browser. For interactive docs reach
> the API directly (`ssh -L 8001:127.0.0.1:8001 server` → `http://127.0.0.1:8001/docs`) or load
> `/api/openapi.json` into an OpenAPI viewer.

### 1.1 Authentication

Send **one** credential per request:

```bash
# API key (operator key or user key)
curl -H "X-API-Key: <key>" https://<server>:8443/api/plugins

# Short-lived JWT
curl -H "Authorization: Bearer <token>" https://<server>:8443/api/plugins
```

The API resolves the identity in this order:

1. `X-API-Key` equals the operator key (`OSIRIS_API_KEY`) → user `operator`, role `admin`.
2. `Authorization: Bearer …` is a valid JWT → user `api-client`, the role in the token.
3. `X-API-Key` matches the digest of a non-revoked user key → that key's user and its role (the
   key's `last_used_at` is updated).

If nothing matches: `401 {"detail":"Geçersiz kimlik bilgisi"}` ("invalid credentials").

- Without `OSIRIS_API_KEY` the API **does not start**. The only exception is
  `OSIRIS_INSECURE_OPEN=1`: authentication is skipped and every caller is `anonymous`/`admin`
  (isolated development networks only).
- JWTs are signed and verified **only** with `OSIRIS_JWT_SECRET` (HS256, fixed algorithm). If the
  secret is unset, `POST /auth/token` returns `503` and all Bearer tokens are rejected; API keys
  keep working. Expiry checks allow 60 seconds of clock skew.

## 2. Roles and permission matrix

Roles are cumulative: `viewer` < `analyst` < `admin`. A call with an insufficient role gets
`403 {"detail":"Yetki yetersiz"}` ("insufficient permission").

| Endpoint | Minimum role |
|----------|--------------|
| `GET /health`, `GET /openapi.json`, `GET /docs` | unauthenticated |
| `POST /auth/token` | operator key in the body |
| `GET /auth/me` | viewer |
| `GET /plugins`, `GET /stats` | viewer |
| `GET /items/recent`, `GET /items/{id}`, `GET /entities/top` | viewer |
| `POST /search`, `POST /search/semantic`, `POST /search/entity` | viewer |
| `GET /graph`, `GET /graph/analysis`, `GET /graph/export` | viewer |
| `GET /sources`, `GET /sources/{id}`, `GET /saved-queries` | viewer |
| `POST /collect`, `POST /collect/batch` | analyst |
| `POST /sources`, `PATCH /sources/{id}`, `DELETE /sources/{id}`, `POST /sources/{id}/collect` | analyst |
| `POST /saved-queries`, `DELETE /saved-queries/{id}`, `POST /alerts/test` | analyst |
| `POST /graph/relation` | analyst |
| `POST /reports/markdown`, `POST /reports/generate` | analyst |
| `GET /auth/keys`, `POST /auth/keys`, `DELETE /auth/keys/{id}` | admin |
| `DELETE /items/{id}`, `POST /entities/erase`, `POST /admin/retention/run` | admin |

## 3. Common rules

**Processing order.** Every endpoint: parses the JSON body → checks identity and role → charges
the quota → validates fields (`422`) → applies business rules (`400/404/409/413/502/503`) →
responds. The audit record is written best-effort before responding; a failed write never breaks
the request.

**Pagination.** List endpoints accept `?limit=` and `?offset=`:

| Endpoint | `limit` default | `limit` range | Note |
|----------|-----------------|---------------|------|
| `GET /sources`, `GET /saved-queries` | 100 | 1–200 (out-of-range values are clamped) | `offset < 0` → `422` |
| `GET /items/recent` | 20 | 1–50 (clamped) | negative `offset` is treated as 0 |
| `GET /entities/top` | 20 | 1–100 (clamped) | no paging |
| `POST /search*` | 20 | 1–100 | out of range → `422` |

**Path identifiers.** A `{id}` value that is not a UUID returns `404` (never `500`).

**Lenient types.** Integer and boolean fields may also arrive as strings (for tools such as n8n):
`"20"` → 20, `5.0` → 5; `"true"`, `"1"`, `"yes"`, `"on"` → true; `"false"`, `"0"`, `"no"`,
`"off"` → false.

**Size limits.** Request body ≤ 5 MiB (same in Nginx). Every `config` object ≤ 50 keys and ≤
20 000 characters serialised (otherwise `413`). Field length limits are given per endpoint.

**Partial success.** `POST /collect/batch` always returns `200`; each job reports its own result.

## 4. Health and identity

### `GET /health` — unauthenticated

```json
{"service": "osiris-api", "status": "ok"}
```

### `POST /auth/token` — issues a JWT

Body: `api_key` (required, the operator key, ≤ 500), `role` (default `viewer`; `viewer`,
`analyst`, `admin`).

```bash
curl -X POST https://<server>:8443/api/auth/token -H 'Content-Type: application/json' \
  -d '{"api_key":"<OSIRIS_API_KEY>","role":"analyst"}'
```
```json
{"access_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9…", "token_type": "bearer", "expires_in": 3600}
```

Errors: `401` (wrong key), `400 Geçersiz rol` (invalid role), `503` (no `OSIRIS_JWT_SECRET`). Only
the operator key can issue tokens; user keys cannot.

### `GET /auth/me` — the caller's identity (viewer)

No side effects (only the quota is charged). The web UI learns the role from here at login.

```json
{"role": "analyst", "user": "ayse"}
```

### `POST /auth/keys` — issues a user key (admin)

Body: `username` (required, 1–200), `role` (default `viewer`), `name` (≤ 200, e.g. device name).

```json
{"id": "6f1c…", "api_key": "osiris_Xb3…", "role": "analyst"}
```

- The raw key (`osiris_` + 43 characters) is returned **only in this response**; the database
  stores its SHA-256 digest and its first 8 characters (`key_prefix`).
- The user is created if missing. **If the user exists, its role is updated to the given role**,
  which also affects the user's other keys.

### `GET /auth/keys` — key list (admin)

The newest 200 keys; the raw value or digest is never returned.

```json
[{"id": "6f1c…", "username": "ayse", "role": "analyst", "key_prefix": "osiris_X", "name": "laptop",
  "revoked": false, "created_at": "2026-10-18T09:12:00Z", "last_used_at": "2026-10-18T11:40:31.5Z"}]
```

### `DELETE /auth/keys/{id}` — revokes a key (admin)

`{"status": "revoked"}`. The row is kept (`revoked: true`); an unknown id returns `404`.

## 5. Plugins and collection

### `GET /plugins` — plugin inventory (viewer)

Sorted by id; `config_schema` fields keep manifest order (the UI draws forms in this order). All
plugin fields: [PLUGINS.md](PLUGINS.md).

```json
[{"id": "rss", "name": "RSS/Atom Feed", "network_type": "rss", "description": "RSS/Atom feed okuyucu",
  "config_schema": {"feed_url": {"type": "string", "required": true},
                    "max_items": {"type": "integer", "default": 50}},
  "schedule_default": "*/15 * * * *"}]
```

### `POST /collect` — single collection (analyst)

Body: `plugin_id` (required, ≤ 64, lower-case letters/digits/`-`/`_`), `config` (object),
`include_items` (default `false`).

```bash
curl -X POST https://<server>:8443/api/collect -H "X-API-Key: <a>" -H 'Content-Type: application/json' \
  -d '{"plugin_id":"rss","config":{"feed_url":"https://example.com/feed.xml"},"include_items":true}'
```
```json
{"items": 12, "metadata": {…},
 "results": [{"url": "https://example.com/news/1", "title": "…", "snippet": "…",
              "published_at": "2026-10-18T07:30:00+00:00", "metadata": {…}, "tags": []}]}
```

- Items are enqueued; the worker writes them to the database within seconds.
- `include_items: true` → a `results` preview: at most 1 000 items, `snippet` is the first 500
  characters of the raw content. The preview is only in the response; items are still enqueued.
- Errors: `404 Plugin bulunamadı` (plugin not found), `413 config çok büyük` (config too large),
  `502 <plugin message>` (the plugin ran but failed; e.g. `Zorunlu alanlar eksik: feed_url` —
  required fields missing, `İç ağ/özel host engellendi` — internal/private host blocked).

### `POST /collect/batch` — batch collection (analyst)

Body: `jobs` (1–10 jobs, each like a `/collect` body). Jobs run sequentially.

```json
{"jobs": [{"plugin_id": "rss", "ok": true, "items": 12, "error": null},
          {"plugin_id": "dns-whois", "ok": false, "items": 0, "error": "Domain bulunamadı"},
          {"plugin_id": "missing", "ok": false, "error": "Plugin bulunamadı"}],
 "ok": 1, "total": 3}
```

## 6. Search

Common field of the three search endpoints: `limit` (1–100, default 20). `cleaned_content` in
results is the first 2 000 characters.

### `POST /search` — full-text (viewer)

Body: `query` (required, 1–500), `limit`. `plainto_tsquery` with PostgreSQL's `simple`
dictionary, ordered by `ts_rank`. No stemming: the query words must occur verbatim in the text
(case-insensitive).

```json
[{"id": "…", "title": "…", "url": "https://example.com/news/1",
  "cleaned_content": "…", "collected_at": "2026-10-18T08:00:00Z"}]
```

### `POST /search/semantic` — semantic (viewer)

Body: `query` (required, 1–500), `limit`. The query is turned into a 384-dimensional vector with
the worker's model and ranked by cosine similarity against `embedding_v2`. Results have the same
fields as `/search` plus `similarity` (0–1, higher = closer).

```json
[{"id": "…", "title": "…", "url": "…", "cleaned_content": "…", "collected_at": "…", "similarity": 0.83}]
```

- The model loads in the API process on the first search; the first request may take a few seconds.
- If `OSIRIS_EMBEDDING_MODEL` is empty or the model files are missing, `503` is returned; the rest
  of the API is unaffected. Download the models: `docker compose --profile models run --rm osiris-models`.
- Only items that have a vector are searched.

### `POST /search/entity` — entity (viewer)

Body: `entity_type` (required), `value` (1–500; substring, case-insensitive), `limit`.

```bash
-d '{"entity_type":"email","value":"@example.com","limit":20}'
```

Result fields: `id`, `title`, `url`, `cleaned_content`. Invalid type → `400`.

Entity types: `person`, `org`, `location`, `ip`, `domain`, `email`, `phone`, `crypto_address`,
`hash`, `username`, `cve`, `custom`. (Today the pipeline produces `person`, `org`, `location`,
`ip`, `domain`, `email`, `phone`, `crypto_address`, `cve`.)

## 7. Items and entities

### `GET /items/recent` — recent items (viewer)

`?limit=` (1–50, default 20), `?offset=`. Newest first by `collected_at`.

```json
[{"id": "…", "title": "…", "url": "…", "language": "tr", "snippet": "first 500 characters…",
  "collected_at": "…", "tags": ["threat"]}]
```

### `GET /items/{id}` — item detail (viewer)

```json
{"id": "…", "source_id": "…", "title": "…", "url": "…", "language": "en",
 "cleaned_content": "…", "collected_at": "…", "published_at": null,
 "content_hash": "9f86d0…", "metadata": {…}, "tags": ["breach"],
 "entities": [{"type": "email", "value": "info@example.com", "mention_count": 1}]}
```

`entities` contains at most 200 entities ordered by mention count. Unknown id → `404`.

### `GET /entities/top` — most mentioned entities (viewer)

`?entity_type=` (optional), `?limit=` (1–100, default 20).

```json
[{"type": "domain", "value": "example.com", "mentions": 12, "last_seen": "…"}]
```

## 8. Sources

### Source object

| Field | Type | Rule |
|-------|------|------|
| `name` | string | required, 1–500 |
| `plugin_id` | string | required, a registered plugin (otherwise `404`) |
| `url` | string | ≤ 2048; empty → `null`. When run, copied into `config`'s `url`/`feed_url`/`endpoint` if those are empty |
| `network_type` | string | `www` (default), `tor`, `i2p`, `p2p`, `freenet`, `zeronet`, `irc`, `matrix`, `rss`, `api`, `blockchain`, `sdr`, `custom` (otherwise `400`) |
| `schedule` | string | empty or a 5-field cron (UTC); invalid → `422` |
| `priority` | integer | 1–10, default 5 |
| `enabled` | boolean | default `true` |
| `tags` | string array | ≤ 20 tags, each ≤ 100 characters |
| `config` | object | plugin settings; ≤ 50 keys, ≤ 20 KB |
| `retention_days` | integer \| null | `null` → `OSIRIS_RETENTION_DAYS`; `0` → never purged by age; 1–36 500 days |

Read responses also carry health fields: `last_crawled_at`, `last_success_at`, `failure_count`
(consecutive failures), `avg_response_ms` (duration of the last run).

### `GET /sources` — list (viewer)

Filters: `?enabled=true|false`, `?plugin_id=rss`, `?q=` (substring of name or URL; `%` and `_` are
literal). Paging: `limit` (1–200, default 100), `offset`. Sorted by name. The list has no `config`.

### `POST /sources` — create (analyst)

```bash
curl -X POST https://<server>:8443/api/sources -H "X-API-Key: <a>" -H 'Content-Type: application/json' -d '{
  "name": "Example API", "plugin_id": "rest-api", "network_type": "api",
  "schedule": "0 * * * *", "retention_days": 30,
  "config": {"endpoint": "https://api.example.com/v1/events", "json_path": "data.items",
             "headers": {"Authorization": "Bearer secret-value"}}}'
```
```json
{"id": "2b0e…"}
```

### `GET /sources/{id}` — detail (viewer)

List fields + `created_at`, `updated_at` and `config`. **Secrets are masked:** every field whose
key name contains `api_key`, `access_token`, `token`, `password`, `passwd`, `secret`,
`authorization`, `cookie`, `credential` or `private_key` (case-insensitive, `-`/`_` optional) and
whose value is not empty is returned as `"********"`, also in nested objects and arrays (up to 8
levels).

```json
{"id": "2b0e…", "name": "Example API", "plugin_id": "rest-api", "network_type": "api", "url": null,
 "schedule": "0 * * * *", "priority": 5, "enabled": true, "tags": [], "retention_days": 30,
 "last_crawled_at": null, "last_success_at": null, "failure_count": 0, "avg_response_ms": null,
 "created_at": "…", "updated_at": "…",
 "config": {"endpoint": "https://api.example.com/v1/events", "json_path": "data.items",
            "headers": {"Authorization": "********"}}}
```

### `PATCH /sources/{id}` — partial update (analyst)

Only fields present in the body change: `name`, `url`, `schedule`, `priority`, `enabled`, `tags`,
`config`, `retention_days`.

- `url: null` / `schedule: null` → field cleared; `retention_days: null` → back to the server default.
- `name`, `priority`, `enabled` cannot be `null` (`422`).
- `config` is **shallow-merged**: keys sent are written, a key with value `null` is deleted, keys
  not sent are kept. Wherever the value is `"********"` (also nested), the stored old value is
  kept; if there is none, that key is not written. Editing the masked `config` from `GET` and
  sending it back therefore never destroys secrets.

```bash
curl -X PATCH https://<server>:8443/api/sources/<id> -H "X-API-Key: <a>" -H 'Content-Type: application/json' \
  -d '{"enabled": false, "schedule": "0 */6 * * *", "config": {"json_path": null}}'
```
```json
{"status": "updated"}
```

Errors: empty body `422 Güncellenecek alan yok` (nothing to update), unknown source `404`, large
`config` `413`.

### `DELETE /sources/{id}` — delete (analyst)

`{"status": "deleted"}`. The source's items are deleted too (database cascade).

### `POST /sources/{id}/collect` — run now (analyst)

Runs the source's `config` with `source_id` and (if needed) `url` added; updates the health fields
and `source_metrics`. `?include_items=true` → a `results` preview as in `/collect`.

The response is the same as `/collect`. Errors: `404` (source or plugin missing),
`409 Kaynak devre dışı` (source disabled), `502` (plugin failed).

## 9. Graph

The graph is undirected; a pair of nodes has one edge (adding a new `relation_type` to the same
pair replaces the previous one); self-loops are ignored. Node ids are entity names (trimmed strings).

### `GET /graph` — nodes and edges (viewer)

At most 5 000 nodes and 20 000 edges.

```json
{"nodes": [{"id": "example.com"}, {"id": "203.0.113.7"}],
 "edges": [{"source": "example.com", "target": "203.0.113.7", "relation_type": "resolves", "weight": 1.0}]}
```

### `POST /graph/relation` — add a relation (analyst)

Body: `source`, `target` (required, 1–500), `relation_type` (≤ 100, default `related`). The edge is
written to the in-memory graph and to the `graph_edges` table (survives restarts).

```json
{"status": "ok"}
```

An id that is empty after trimming → `400`.

### `GET /graph/analysis` — analysis (viewer)

`?top=` (1–200, default 25), `?node=` (optional).

```json
{"nodes": 42, "edges": 57,
 "centrality": [{"id": "example.com", "score": 0.121951, "degree": 5}],
 "communities": [{"size": 9, "members": ["203.0.113.7", "example.com", …]}],
 "neighbors": ["203.0.113.7"]}
```

- `centrality`: degree centrality `degree / (n − 1)`, top `top` nodes in descending order (6 decimals).
- `communities`: Clauset–Newman–Moore modularity communities, the largest 50, each with at most 200
  members. `null` if the graph has more than 5 000 nodes.
- `neighbors`: neighbours of `node` if given (at most 500), otherwise `null`.

### `GET /graph/export` — export (viewer)

`?format=graphml` (default) or `gexf`. The response is `application/xml` with
`Content-Disposition: attachment; filename="osiris-graf.<ext>"`. Any other format → `400`.

```bash
curl -H "X-API-Key: <a>" "https://<server>:8443/api/graph/export?format=gexf" -o osiris.gexf
```

## 10. Saved queries and alerts

### `GET /saved-queries` — list (viewer)

`?alert_enabled=true|false`, `limit` (1–200), `offset`. Sorted by name.

```json
[{"id": "…", "name": "Critical", "query_text": "ransomware", "query_type": "fts",
  "alert_enabled": true, "last_triggered_at": null}]
```

### `POST /saved-queries` — create (analyst)

Body: `name` (1–200), `query_text` (1–500), `query_type` (default `fts`; `fts`, `semantic`,
`regex`, `entity`, `graph` — anything else `400`), `alert_enabled` (default `false`). Response
`{"id": "…"}`.

### `DELETE /saved-queries/{id}` — delete (analyst)

`{"status": "deleted"}`; unknown id → `404`.

### `POST /alerts/test` — test the rules (analyst)

Body: `text` (required, 1–50 000), `title` (≤ 2000). The text is matched against at most 100 rules
with `alert_enabled`: a rule matches when its text (trimmed and lower-cased) occurs as a substring
of the title or content. `query_type` does not change matching. No record is created and no
notification is sent; `last_triggered_at` of matching rules is updated.

```json
[{"kind": "match", "query_id": "…", "query_name": "Critical", "item_id": null, "matched": "ransomware"}]
```

## 11. Reports

Reports are returned in the response body; no file is created on the server.

### `POST /reports/generate` — multi-format report (analyst)

| Field | Rule |
|-------|------|
| `title` | required, 1–500 |
| `scope` | ≤ 1000 |
| `summary` | ≤ 20 000 |
| `findings` | ≤ 500 items: `{"title", "description"}` (title ≤ 500, description ≤ 5000) |
| `sources` | ≤ 500 strings (each ≤ 1000) |
| `format` | `markdown` (default), `html`, `json`, `stix` |
| `entities` | ≤ 500 items: `{"type", "value"}` — indicators for JSON and STIX |
| `identity_name` | Name of the STIX `identity` object (default `OSIRIS`) |

```bash
curl -X POST https://<server>:8443/api/reports/generate -H "X-API-Key: <a>" -H 'Content-Type: application/json' \
  -d '{"title":"Case 12","format":"stix","entities":[{"type":"ip","value":"203.0.113.7"},{"type":"cve","value":"CVE-2024-3094"}]}'
```
```json
{"format": "stix", "media_type": "application/stix+json", "filename": "Case-12.stix.json",
 "content": "{\n  \"type\": \"bundle\", …}"}
```

| `format` | `media_type` | Extension | Content |
|----------|-------------|-----------|---------|
| `markdown` | `text/markdown` | `.md` | Title, generation time, scope, summary, findings, sources; HTML characters escaped |
| `html` | `text/html` | `.html` | Same content, single file, inline style, escaped |
| `json` | `application/json` | `.json` | `{title, generated_at, scope, summary, findings, sources, entities}` |
| `stix` | `application/stix+json` | `.stix.json` | STIX 2.1 `bundle`: one `identity`; `cve` → `vulnerability`, `hash` → `file`, `ip`/`domain`/`email`/`crypto_address` → `indicator`; other types skipped |

`filename` is derived from the title: Turkish letters are simplified to ASCII (`ı→i`, `ş→s`, …),
other characters become `-`, at most 60 characters; `osiris-rapor` if nothing remains.

### `POST /reports/markdown` — Markdown only (analyst)

Legacy endpoint; takes `title`, `scope`, `summary`, `findings`, `sources` and returns
`{"markdown": "…"}`.

## 12. Privacy and retention (KVKK/GDPR)

### `DELETE /items/{id}` — delete an item (admin)

Deletes the item and the entities no longer linked to any item.

```json
{"status": "deleted", "items_deleted": 1, "entities_deleted": 2}
```

### `POST /entities/erase` — erase an entity and what mentions it (admin)

Body: `entity_type` (required), `value` (required, 1–500), `delete_items` (default `true`).

```bash
curl -X POST https://<server>:8443/api/entities/erase -H "X-API-Key: <admin>" -H 'Content-Type: application/json' \
  -d '{"entity_type":"email","value":"ali@example.com","delete_items":true}'
```
```json
{"items_deleted": 4, "entities_deleted": 3, "edges_deleted": 1}
```

- Matching is type + trimmed, case-insensitive value.
- `delete_items: true` → every item mentioning the entity is deleted; orphaned entities are then
  deleted as well.
- Graph edges whose source or target name equals the value are deleted and the in-memory graph is
  reloaded.
- The audit record stores `value_sha256` (SHA-256 of the lower-cased value), not the value.
- Invalid type → `400`; a value empty after trimming → `422`.

### `POST /admin/retention/run` — run the retention purge now (admin)

```json
{"items_deleted": 120, "entities_deleted": 37, "default_days": 180}
```

Rules: the source's `retention_days` (if empty, `OSIRIS_RETENTION_DAYS`; `0` → never purged);
items without a source get the default. Normally the worker does this every
`OSIRIS_RETENTION_INTERVAL` seconds.

## 13. Statistics

### `GET /stats` (viewer)

```json
{"sources": 4, "sources_enabled": 3, "items": 1280, "entities": 342, "edges": 57,
 "saved_queries": 3, "alerts": 2, "latest_item_at": "2026-10-18T11:59:02Z"}
```

`alerts` is the number of saved queries with `alert_enabled`.

## 14. Errors

The error body is always `{"detail": …}`. Internal details (SQL, driver messages, stack traces) are
never put in the body; they are logged.

| Code | Meaning | Example `detail` (translation) |
|------|---------|--------------------------------|
| 400 | Invalid value | `Geçersiz rol` (invalid role), `Geçersiz network_type`, `Geçersiz entity_type`, `format graphml veya gexf olmalı` |
| 401 | Missing or invalid credential | `Geçersiz kimlik bilgisi` (invalid credentials) |
| 403 | Insufficient role | `Yetki yetersiz` (insufficient permission) |
| 404 | No such record, malformed UUID or unknown path | `Kaynak bulunamadı` (source not found), `Plugin bulunamadı`, `Not Found` |
| 405 | Method not allowed | `Method Not Allowed` |
| 409 | State conflict | `Kaynak devre dışı` (source disabled) |
| 413 | Too large | `config çok büyük` (config too large) or body > 5 MiB |
| 422 | Schema/field violation | array: `[{"type": "…", "loc": ["body", "limit"], "msg": "…"}]` or a single message |
| 429 | Quota exceeded | `İstek kotası aşıldı` (request quota exceeded) |
| 500 | Server/database error | `Kaynaklar okunamadı` (generic message) |
| 502 | The plugin ran but failed | The plugin's error message |
| 503 | Feature not configured | `Jeton üretimi kapalı (OSIRIS_JWT_SECRET yok)`, `Semantik arama yapılandırılmamış: …` |

## 15. Rate limits and metrics

There are two layers:

| Layer | Key | Limit | When exceeded |
|-------|-----|-------|---------------|
| Nginx | Client IP | 10 req/s on `/api/*`, burst 20 | `503` (from Nginx) |
| API | Identity (`role:user`) | `OSIRIS_API_RATE_PER_MIN` requests/min (default 600; `0` = off) | `429 İstek kotası aşıldı` |

All JWTs carry the same user name (`api-client`), so JWTs of the same role share one quota. For
large jobs use `/collect/batch` (≤ 10) and pace the client.

**Metrics** are on a separate listener (`OSIRIS_METRICS_ADDR`, in Compose
`osiris-api:9464/metrics`, internal network only; not served under `/api/`):
`osiris_http_requests_total`, `osiris_http_request_duration_seconds`,
`osiris_api_rate_limited_total`, `osiris_audit_failures_total`, `osiris_build_info`. Labels are
route templates; path values and query strings are never recorded.

## 16. Change history

- **Web UI release** — New: `GET /auth/me`; `POST /search/semantic`; `GET` and
  `PATCH /sources/{id}` (secrets masked at every depth, the mask keeps the old value on update,
  `config` merging); cron validation of source `schedule`; `GET /graph/analysis`;
  `GET /graph/export`; `POST /reports/generate` (Markdown/HTML/JSON/STIX); `include_items`
  preview for `/collect` and `/sources/{id}/collect`; `PATCH` in CORS.
- **1.0.0-rc (Rust R7)** — Python implementation removed, contract unchanged. `osiris-server migrate`;
  migration 006 drops the old `items.embedding` column; audit chain v2 only.
- **Rust R6** — API rewritten in Rust (axum) with an unchanged contract. Per-identity quota
  (`429`), `/metrics` (separate listener), JSON logs, `/openapi.json`. In `/graph/relation` an id
  that is empty after trimming now returns `400` instead of `500`.
- **Rust R2** — KVKK/GDPR endpoints (`DELETE /items/{id}`, `POST /entities/erase`,
  `POST /admin/retention/run`), `retention_days` on sources; refusing to start without
  `OSIRIS_API_KEY`; the JWT secret no longer falls back to the API key; audit chain v2 (RFC 8785).
- **0.2.0** — Paging and filters on lists; `GET /items/{id}`, `GET /entities/top`,
  `POST /collect/batch`; UUID-safe paths; LIKE wildcard escaping in the `q` filter.
- **0.1.x** — Source CRUD and runs, saved queries, `/alerts/test`, `/reports/markdown`, `/stats`,
  `/items/recent`, JWT + RBAC, `/auth/keys`.
