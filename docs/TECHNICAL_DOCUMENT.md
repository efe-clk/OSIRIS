> **Türkçe:** [TEKNIK_DOKUMAN.tr.md](TEKNIK_DOKUMAN.tr.md)

# OSIRIS — Installation and Operations Guide

> **O**pen **S**ource **I**ntelligence **R**esearch & **I**nformation **S**ystem · Version **1.0.0-rc**
>
> This guide explains how to install, configure, use and operate OSIRIS from scratch. For the
> internals see [ARCHITECTURE.md](ARCHITECTURE.md), for the endpoint-by-endpoint API reference
> [API.md](API.md), and for the data sources [PLUGINS.md](PLUGINS.md).

---

## Contents

1. [What is OSIRIS?](#1-what-is-osiris)
2. [Concepts](#2-concepts)
3. [Requirements](#3-requirements)
4. [Installation](#4-installation)
5. [End-to-end first use](#5-end-to-end-first-use)
6. [Configuration reference](#6-configuration-reference)
7. [Services, profiles and ports](#7-services-profiles-and-ports)
8. [Identity, roles and access management](#8-identity-roles-and-access-management)
9. [Collection and scheduling](#9-collection-and-scheduling)
10. [Search, graph, alerts and reports](#10-search-graph-alerts-and-reports)
11. [Personal data: retention and erasure (KVKK/GDPR)](#11-personal-data-retention-and-erasure-kvkkgdpr)
12. [Day-to-day operations](#12-day-to-day-operations)
13. [Troubleshooting](#13-troubleshooting)
14. [Known limitations](#14-known-limitations)
15. [Roadmap](#15-roadmap)

---

## 1. What is OSIRIS?

OSIRIS is a **self-hosted open source intelligence (OSINT) platform**. It collects data from
surface web pages, RSS/Atom feeds, REST APIs, Shodan, DNS/WHOIS, blockchains, IRC and Matrix
chats, and the Tor and I2P networks. It cleans the text, extracts entities (e-mail, IP, domain,
person, organisation…), turns it into semantic vectors and makes everything searchable. No data
is sent to a third-party cloud.

The application is written in Rust: every server role lives in **one binary** (`osiris-server`)
and **one Docker image** (`osiris:1.0.0-rc`). The browser UI is React/TypeScript.

What you can do:

| Task | How |
|------|-----|
| Trace a username across hundreds of sites | Web UI → *Username scan* (`username-search` plugin) |
| Watch a news feed, web page or API regularly | A source with a cron schedule on the *Sources* screen |
| Search collected text by keyword or by meaning | *Search*: full-text, semantic, entity |
| Find every record that mentions an e-mail/IP/domain | *Entities* or `POST /search/entity` |
| Map relationships between entities | *Relationship graph* (centrality, communities, GraphML/GEXF) |
| Define rules for specific keywords | *Alert rules* |
| Turn findings into a Markdown/HTML/JSON/STIX 2.1 report | *Reports* |
| Erase a data subject's data (KVKK/GDPR) | *Access & privacy* or `POST /entities/erase` |

## 2. Concepts

| Term | Meaning |
|------|---------|
| **Plugin** | A component that collects one kind of data (e.g. `rss`, `web-scraper`). The 11 plugins are compiled into the binary; each one's configuration schema is returned by `GET /plugins`. Details: [PLUGINS.md](PLUGINS.md). |
| **Source** | A saved plugin configuration: which plugin, which settings, which cron, how long to keep the data. Run manually or by the scheduler; tracks health. |
| **Collection** | One run of a plugin, either standalone (`POST /collect`) or through a source (`POST /sources/{id}/collect`). |
| **Queue** | Every collected item is written to the Redis Stream `osiris:stream:raw_items`; the processing pipeline reads from it. Collection and processing are thus decoupled. |
| **Item** | One processed record stored in the database (a page, a feed entry, a profile…). Deduplicated by the SHA-256 digest of the cleaned text (`content_hash`). |
| **Entity** | A value extracted from items: `email`, `ip`, `domain`, `phone`, `crypto_address`, `cve` (regex) and `person`, `org`, `location` (NER model). |
| **Graph** | Relationship edges between entity names (`graph_edges`). Edges are added with `POST /graph/relation` (*Add relation* in the UI, *Link to graph* in the username scan). |
| **Saved query / alert rule** | A named query; when `alert_enabled` it is matched as an alert rule. |
| **Audit log** | Hash-chained, append-only record of write and sensitive read operations (`audit_logs`). |

## 3. Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| CPU | 2 cores | 4–8 cores |
| RAM | 4 GB (without models) | 8–16 GB (with NER + embedding models) |
| Disk | 40 GB SSD | 200 GB+ NVMe (depending on data volume) |
| Operating system | Current Linux running Docker Engine + Compose v2 (Ubuntu 22.04+, Debian 12+) | |
| Network | Internet access to the target sources | Unblocked egress for Tor/I2P |

Notes:

- Building the image from source takes 10–15 minutes and needs about 4 GB of RAM while building.
  On a server with little RAM, build on another machine and move the image with
  `docker save | ssh … docker load` (see [§12.5](#125-upgrading)).
- The models (~420 MB) are optional. Without them the system works, but only regex entities are
  extracted and semantic search is disabled.
- The Compose file puts a memory cap on every service (`mem_limit`). The caps add up to about
  9 GB; they are upper bounds, actual usage is much lower.

## 4. Installation

### 4.1 Get the repository

```bash
git clone https://github.com/efe-clk/OSIRIS.git
cd OSIRIS
```

### 4.2 Prepare the environment file

```bash
cp .env.example .env
chmod 600 .env
```

Fill in at least these four values in `.env`. Generate a separate random value for each:

```bash
openssl rand -hex 32
```

| Variable | Purpose |
|----------|---------|
| `POSTGRES_PASSWORD` | Database password |
| `OSIRIS_API_KEY` | **Operator key** — the fully privileged (admin) API key. If empty, the API **does not start**. |
| `OSIRIS_JWT_SECRET` | Secret that signs short-lived JWTs. Must be **different** from `OSIRIS_API_KEY`. If empty, JWT is disabled. |
| `N8N_PASSWORD` | Password of the n8n automation UI (together with `N8N_USER`) |

All other variables are optional; the full list is in [§6](#6-configuration-reference).

> Never commit `.env` (it is in `.gitignore`). `POSTGRES_PASSWORD` and `N8N_PASSWORD` only take
> effect when the respective data volume is **first** created; to change them later see
> [§12.6](#126-secret-rotation).

### 4.3 Generate a TLS certificate

Nginx serves TLS on port `:8443` and does not start without a certificate. On first install,
generate a self-signed certificate:

```bash
./deploy/nginx/make-certs.sh       # deploy/nginx/certs/osiris.local.{crt,key}, 825 days
```

The script does not overwrite an existing certificate. Moving to a real domain and certificate:
[deploy/nginx/README.md](../deploy/nginx/README.md).

### 4.4 Download the models (optional, recommended)

```bash
docker compose --profile models run --rm --build osiris-models
```

This downloads the NER (person/organisation/location) and embedding (semantic vector) models from
pinned revisions, verifies their digests and writes them to `./models`. Files are written as the
host user `OSIRIS_UID:OSIRIS_GID` (default `1000:1000`); if your ids differ (`id -u`, `id -g`),
add these two variables to `.env`. Details: [models/README.md](../models/README.md).

### 4.5 Start the services

```bash
docker compose --profile full up -d --build
```

The `full` profile starts PostgreSQL (pgvector), Redis, n8n, Nginx, the API, the pipeline worker,
the collector (scheduler) and the web UI. The Tor and I2P proxies are in a separate profile:

```bash
docker compose --profile full --profile darknet up -d      # for the Tor/I2P plugins
```

The database schema is installed automatically from `db/migrations/` the **first** time an empty
PostgreSQL volume starts. When upgrading an existing installation, apply migrations manually (see
[§12.5](#125-upgrading)).

### 4.6 Verify the installation

```bash
docker compose ps                                        # osiris-api should be "healthy"
curl -sk https://localhost:8443/api/health               # {"service":"osiris-api","status":"ok"}
curl -sk -H "X-API-Key: <OSIRIS_API_KEY>" https://localhost:8443/api/auth/me
#   {"role":"admin","user":"operator"}
docker compose exec osiris-api osiris status             # CLI: version and plugin count
docker compose exec osiris-api osiris plugins            # 11 plugins
```

In a browser, `https://<server>:8443/` redirects to `/ui/`. Accept the self-signed certificate
warning, choose the **API key** credential type and enter `OSIRIS_API_KEY`.

| Address | Content |
|---------|---------|
| `https://<server>:8443/ui/` | Web UI |
| `https://<server>:8443/api/` | REST API (`/api/docs` interactive docs, `/api/openapi.json`) |
| `https://<server>:8443/n8n/` | n8n automation UI |

## 5. End-to-end first use

The steps below add an RSS feed as a source, collect it, search the result and produce a report.
Everything can also be done in the UI; the equivalent API calls are shown here.

```bash
API=https://localhost:8443/api
K=<OSIRIS_API_KEY>

# 1) Create an analyst key for daily use instead of the operator key
curl -sk -X POST $API/auth/keys -H "X-API-Key: $K" -H 'Content-Type: application/json' \
  -d '{"username":"ayse","role":"analyst","name":"laptop"}'
# → {"id":"…","api_key":"osiris_…","role":"analyst"}   (the raw key is shown only in this response)
A=<returned api_key>

# 2) Create a source (every 15 minutes; keep items for 90 days)
curl -sk -X POST $API/sources -H "X-API-Key: $A" -H 'Content-Type: application/json' -d '{
  "name": "Example news feed", "plugin_id": "rss", "network_type": "rss",
  "url": "https://example.com/feed.xml", "schedule": "*/15 * * * *",
  "config": {"max_items": 50}, "retention_days": 90}'
# → {"id":"<source-id>"}

# 3) Run it once now and preview
curl -sk -X POST "$API/sources/<source-id>/collect?include_items=true" -H "X-API-Key: $A"
# → {"items":12,"metadata":{…},"results":[{"url":…,"title":…,"snippet":…}, …]}

# 4) The worker processes the items within seconds; then search
curl -sk -X POST $API/search -H "X-API-Key: $A" -H 'Content-Type: application/json' \
  -d '{"query":"ransomware","limit":10}'

# 5) Produce a report
curl -sk -X POST $API/reports/generate -H "X-API-Key: $A" -H 'Content-Type: application/json' -d '{
  "title":"Weekly summary","format":"html","summary":"…",
  "findings":[{"title":"Finding 1","description":"…"}],"sources":["https://example.com/feed.xml"]}'
```

For the source to run by itself on schedule, set `OSIRIS_SCHEDULER=on` in `.env` and recreate the
collector (`docker compose --profile full up -d osiris-collector`). The scheduler is deliberately
**off** by default: you are expected to review the sources' cron values first.

## 6. Configuration reference

All settings are environment variables. With Compose you put them in `.env`;
`docker-compose.yml` passes them to the relevant service. The "Service" column shows which role
reads the variable.

### 6.1 Required and security

| Variable | Default | Service | Description |
|----------|---------|---------|-------------|
| `OSIRIS_API_KEY` | — | api, n8n | Operator key (admin). If empty, the API does not start. Compared in constant time. |
| `OSIRIS_JWT_SECRET` | — | api | HS256 JWT signing secret. If empty, `POST /auth/token` returns `503` and Bearer tokens are rejected. There is **no** fallback to the API key. |
| `OSIRIS_INSECURE_OPEN` | — | api | `1` → open mode without a key: every caller is `admin`. Only on an isolated development network. |
| `OSIRIS_CORS_ORIGINS` | `http://localhost:3000` | api | Allowed origins (comma separated) for browsers calling the API directly. The UI calls it same-origin through Nginx, so production usually needs no change. |
| `OSIRIS_API_RATE_PER_MIN` | `600` | api | Requests per minute per identity; `0` = off. Exceeding it returns `429`. |
| `POSTGRES_PASSWORD` | `osiris_dev_password` | postgresql, all | Database password; Compose builds the connection URL from it. |
| `N8N_USER` / `N8N_PASSWORD` | `admin` / `osiris_dev_password` | n8n | n8n basic authentication. |

### 6.2 Connections and addresses

| Variable | Default | Service | Description |
|----------|---------|---------|-------------|
| `OSIRIS_DATABASE_URL` | Compose: `postgresql://osiris:…@postgresql:5432/osiris` | all | PostgreSQL connection. Set it yourself outside Compose. |
| `OSIRIS_REDIS_URL` | `redis://localhost:6379/0` (Compose: `redis://redis:6379/0`) | all | Redis connection (queue). |
| `OSIRIS_API_ADDR` | `0.0.0.0:8000` | api | API listen address (also `--addr`). |
| `OSIRIS_METRICS_ADDR` | empty (Compose: `0.0.0.0:9464`) | api | Prometheus `/metrics` listener; empty = off. Not served on the API port. |
| `OSIRIS_LOG_FORMAT` | text (Compose: `json`) | all | `json` → one JSON object per line. Verbosity via `RUST_LOG` (e.g. `RUST_LOG=debug`). |

### 6.3 Pipeline and models

| Variable | Default | Service | Description |
|----------|---------|---------|-------------|
| `OSIRIS_EMBEDDING_MODEL` | Compose: `sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2` | pipeline, api | Embedding model. Empty → no vectors, semantic search `503`. The API and the worker must use the **same** value. |
| `OSIRIS_MODEL_DIR` | `models` (in the image `/app/models`) | pipeline, api | Model root directory. |
| `OSIRIS_NER` | on | pipeline | `off`/`0`/`false` → ONNX NER disabled, regex entities only. |
| `OSIRIS_PIPELINE_CONSUMER` | `host-pid` (Compose: `osiris-pipeline-1`) | pipeline | Consumer name inside the Redis consumer group. Give each worker a different name if you run several. |
| `OSIRIS_STREAM_MAXLEN` | `200000` | api, collector, CLI | Approximate cap of the queue stream (1 000–10 000 000). |
| `OSIRIS_UID` / `OSIRIS_GID` | `1000` | osiris-models | Host user that writes `./models`. |

### 6.4 Collection

| Variable | Default | Service | Description |
|----------|---------|---------|-------------|
| `OSIRIS_SCHEDULER` | `off` | collector | `on`/`1`/`true` → sources run on their cron. |
| `OSIRIS_TOR_SOCKS` | Compose: `tor:9050` | api, collector | Tor SOCKS5 proxy (`host:port`). When set it **overrides** proxy fields in user configuration. |
| `OSIRIS_I2P_HTTP_PROXY` | Compose: `i2pd:4444` | api, collector | I2P HTTP proxy (`host:port`); same rule. |
| `OSIRIS_USERNAME_DB_DIR` | — | api, collector | Extra directory from which the `username-search` plugin's `sites_db` setting may read a custom site database. |

### 6.5 Retention (KVKK/GDPR)

| Variable | Default | Service | Description |
|----------|---------|---------|-------------|
| `OSIRIS_RETENTION_DAYS` | `180` | api, pipeline | Retention period (days) applied when a source has no `retention_days`. `0` → default off (only sources with a positive period are purged). |
| `OSIRIS_RETENTION_INTERVAL` | `3600` | pipeline | How often (seconds) the worker runs the retention purge; `0` = off. |

### 6.6 Library-level only

`OSIRIS_ALERT_WEBHOOK_URL`, `OSIRIS_ALERT_TELEGRAM_BOT_TOKEN`, `OSIRIS_ALERT_TELEGRAM_CHAT_ID`,
`OSIRIS_ALERT_SMTP_URL`, `OSIRIS_ALERT_EMAIL_FROM` and `OSIRIS_ALERT_EMAIL_TO` configure the alert
handlers of the `osiris-alert` library. **No service publishes alerts yet** (see
[§14](#14-known-limitations)); setting them has no effect.

### 6.7 Used by tests

`OSIRIS_TEST_DATABASE_URL`, `OSIRIS_TEST_REDIS_URL`, `OSIRIS_REQUIRE_MODELS` and the contract
suite's `OSIRIS_BASE_URL`, `OSIRIS_CONTRACT_*` variables are for tests only:
[CONTRIBUTING.md](CONTRIBUTING.md).

## 7. Services, profiles and ports

Compose profiles select which services start; services without a profile always start.

| Service | Profile | Image / command | Role |
|---------|---------|-----------------|------|
| `postgresql` | — | `pgvector/pgvector:pg16` | Database (with pgvector) |
| `redis` | — | `redis:7.4-alpine` (AOF on) | Queue |
| `n8n` | — | `n8nio/n8n:1.114.4` | Workflow automation |
| `nginx` | — | `nginx:1.31-alpine` | TLS, routing, security headers, per-IP rate limit |
| `osiris-api` | `api`, `full` | `osiris-server api` | REST API |
| `osiris-pipeline` | `worker`, `full` | `osiris-server pipeline --loop --interval 5 --batch 50` | Queue worker + retention purge |
| `osiris-collector` | `worker`, `full` | `osiris-server collector` | Cron scheduler (idles unless `OSIRIS_SCHEDULER=on`) |
| `osiris-web-ui` | `ui`, `full` | `osiris-web-ui:1.0.0-rc` | Browser UI (static nginx, port 3000) |
| `osiris-models` | `models` | `osiris-server models` | One-off model download |
| `tor`, `i2pd` | `darknet` | `osiris-tor:0.1.0`, `purplei2p/i2pd` | Dark-net proxies (internal network only) |

The three OSIRIS services run the same image with different subcommands. One-off jobs with the
same image:

```bash
docker compose run --rm osiris-api migrate                 # apply migrations
docker compose run --rm osiris-api audit-verify --limit 100000
docker compose run --rm osiris-api purge                   # retention purge now
docker compose run --rm osiris-api collect --plugin rss --config '{"feed_url":"https://…"}'
docker compose run --rm osiris-pipeline pipeline --reembed # vectorise items without vectors
```

### 7.1 Network topology

```
Internet ──► Nginx  :80 (HTTP)  and  :8443 (TLS)
              ├─ /api/*  → osiris-api:8000      (/api prefix stripped: /api/search → /search)
              ├─ /ui/*   → osiris-web-ui:3000   (/ and /ui redirect to /ui/)
              └─ /n8n/*  → n8n:5678

Internal Docker network:
  osiris-api, osiris-pipeline, osiris-collector ──► postgresql:5432, redis:6379
  osiris-api:9464/metrics (internal network only)
  tor:9050, i2pd:4444 (darknet profile; not published to the host)
```

Ports published on the host:

| Port | Bound to | Service |
|------|----------|---------|
| `80`, `8443` | all interfaces | Nginx |
| `8001` | `127.0.0.1` | API (direct, without Nginx) |
| `3001` | `127.0.0.1` | Web UI (direct) |
| `5432` | `127.0.0.1` | PostgreSQL |
| `6379` | `127.0.0.1` | Redis (unauthenticated — never expose it) |
| `5678` | `127.0.0.1` | n8n |

Reach the `127.0.0.1` ports from outside with an SSH tunnel (`ssh -L 8001:127.0.0.1:8001 server`).
Why TLS is on `8443` and how to move it to `443`: [deploy/nginx/README.md](../deploy/nginx/README.md).

## 8. Identity, roles and access management

### 8.1 Credential types

Every request carries **one** credential. The API tries them in this order:

1. **Operator key** — `X-API-Key: <OSIRIS_API_KEY>`. Always `admin`, user name `operator`.
2. **JWT** — `Authorization: Bearer <token>`. Issued by `POST /auth/token` in exchange for the
   operator key; valid for 1 hour; the role is inside the token; the user name is `api-client`.
3. **User key** — `X-API-Key: osiris_…`. Issued by `POST /auth/keys`; only its SHA-256 digest is
   stored. Its role is the role of the user the key belongs to.

Use `GET /auth/me` to find out who a client is and what role it has. The endpoint has no side
effects; the web UI calls it at login.

### 8.2 Roles

Roles are cumulative: `viewer` < `analyst` < `admin`.

| Role | Can do |
|------|--------|
| `viewer` | Read: plugins, statistics, items, entities, search (full-text/semantic/entity), graph and analysis, source list/detail, saved queries |
| `analyst` | + collection, creating/updating/deleting/running sources, creating/deleting saved queries, testing alerts, adding graph relations, generating reports |
| `admin` | + key management, deleting items, erasing entities (KVKK/GDPR), retention purge |

Full permission matrix: [API.md §2](API.md#2-roles-and-permission-matrix).

### 8.3 Recommended practice

- Use the operator key only for administration and emergencies; issue a user key per person for
  daily use (UI: *Access & privacy*).
- Give automations (n8n, scripts) the lowest sufficient role; `viewer` for a read-only dashboard.
- Revoke the key of someone who leaves with `DELETE /auth/keys/{id}`. The row is not deleted but
  marked `revoked` (the audit trail is kept).
- Issuing a new key for an existing `username` **updates that user's role**. This also changes
  the permissions of the user's other non-revoked keys.

## 9. Collection and scheduling

### 9.1 Ways to collect

| Path | When | Enqueues? |
|------|------|-----------|
| `POST /collect`, `POST /collect/batch` (≤ 10 jobs) | One-off, without saving a source | Yes |
| `POST /sources/{id}/collect` | Run a saved source now | Yes + source health |
| Scheduler (`OSIRIS_SCHEDULER=on`) | According to the sources' cron | Yes + source health |
| `osiris collect <plugin> --config '<json>'` (CLI) | From the command line | Yes |
| `osiris-server collect --plugin … --config …` | Trial: prints the result as JSON | Only with `--enqueue` |

Each collection first checks the plugin's required fields (`Zorunlu alanlar eksik: …` — "required
fields missing"), then runs the plugin in a separate task. If a plugin crashes, only that
collection fails with `plugin hatası: …` ("plugin error"); the service is unaffected. Items of a
successful collection go to the queue and from there to the worker: **`osiris-pipeline` must be
running for items to become searchable.**

### 9.2 Source configuration

When creating a source, plugin-specific settings go into the `config` object (fields:
[PLUGINS.md](PLUGINS.md) or `GET /plugins`). When the source runs:

- `source_id` is added to `config`;
- if the source's `url` is set and `config` lacks them, the value is copied into the `url`,
  `feed_url` and `endpoint` keys. So for an RSS source it is enough to give the address once as `url`.

In the source detail (`GET /sources/{id}`) secret-bearing fields are returned as `********`. This
applies to every field whose key name contains `api_key`, `access_token`, `token`, `password`,
`passwd`, `secret`, `authorization`, `cookie`, `credential` or `private_key`, also inside nested
objects (e.g. `rest-api`'s `headers.Authorization`). Sending `********` back in an update keeps
the stored secret, so the UI can edit a source without ever seeing its secrets.

### 9.3 Scheduling rules

- `schedule`: standard 5-field cron, **UTC**. Day of week `0` and `7` are Sunday. Examples:
  `*/15 * * * *` (every 15 minutes), `0 */6 * * *` (every 6 hours), `30 3 * * 1` (Monday 03:30).
- An invalid cron is rejected with `422` on source create/update.
- The scheduler re-reads the source list every minute and runs due sources every 15 seconds. A
  source has at most **one** run at a time; a trigger arriving while the previous run is still in
  progress is skipped.
- Missed triggers (while the service was down) are not run retroactively.
- Disabled sources (`enabled=false`) or sources with an empty cron are not scheduled. Running a
  disabled source manually returns `409`.

### 9.4 Source health

Every collection through a source updates `last_crawled_at`; on success `last_success_at` and
`failure_count = 0`, on failure `failure_count + 1`; `avg_response_ms` (duration of the last run);
and adds a row to `source_metrics` (a hypertable if TimescaleDB is installed). The UI's
*Dashboard* and *Sources* screens show these.

## 10. Search, graph, alerts and reports

### 10.1 Search

| Kind | Endpoint | How it works |
|------|----------|--------------|
| Full-text | `POST /search` | PostgreSQL `to_tsvector('simple', …)` + `plainto_tsquery`, ordered by `ts_rank`. The `simple` dictionary does no stemming: "attack" and "attacks" are different words. |
| Semantic | `POST /search/semantic` | The query is turned into a 384-dimensional vector with the same model as the worker and ranked by cosine similarity against `embedding_v2` (`similarity` 0–1). The model is multilingual: a Turkish query can find English text. `503` if there is no model. |
| Entity | `POST /search/entity` | Type + value substring (case-insensitive). |

Older items without a vector do not appear in semantic search. If you installed the models later,
run `docker compose run --rm osiris-pipeline pipeline --reembed`.

### 10.2 Graph

The graph consists of undirected edges between entity names and is persisted in `graph_edges`.
Edges are added **manually** (`POST /graph/relation`; *Add relation* in the UI, *Link to graph* in
the username scan). The pipeline does not create edges from items automatically.

- `GET /graph` — nodes and edges (≤ 5 000 nodes, 20 000 edges).
- `GET /graph/analysis` — degree centrality, Clauset–Newman–Moore communities (for ≤ 5 000 nodes),
  neighbours of a requested node.
- `GET /graph/export?format=graphml|gexf` — a file for tools such as Gephi or Cytoscape.

The API loads the graph into memory from the database (at most 10 000 edges) on first use.

### 10.3 Alert rules

A saved query (`POST /saved-queries`) with `alert_enabled: true` becomes an alert rule. Matching
is simple: the rule matches when its text (trimmed and lower-cased) occurs as a **substring** of
the item's title or content. The `query_type` field (`fts`, `semantic`, `regex`, `entity`,
`graph`) is stored but does not change matching today.

`POST /alerts/test` tests a text against the enabled rules (creates no record, sends no
notification) and updates `last_triggered_at` of the matching rules. Automatically matching
collected items against rules and sending notifications does not exist yet
([§14](#14-known-limitations)).

### 10.4 Reports

`POST /reports/generate` builds a report from a title, scope, summary, findings, sources and
indicators. The report is returned in the response body; it is not stored on the server.

| Format | Media type | Note |
|--------|-----------|------|
| `markdown` | `text/markdown` | HTML-escaped |
| `html` | `text/html` | Single file, inline style |
| `json` | `application/json` | Machine-readable |
| `stix` | `application/stix+json` | STIX 2.1 bundle; only `ip`, `domain`, `email`, `crypto_address`, `hash`, `cve` indicators |

The *Add to report* buttons in the web UI collect findings in a draft kept in the browser; the
*Reports* screen generates the report from that draft.

## 11. Personal data: retention and erasure (KVKK/GDPR)

OSIRIS collects open source records that may contain personal data. These mechanisms handle
retention and erasure.

**Time-based retention**

- Each source has a `retention_days` value: empty → `OSIRIS_RETENTION_DAYS` (180), `0` → never
  purged by age, `1–36 500` → items whose `collected_at` is older than that many days are deleted.
- Items without a source (e.g. collected with `POST /collect`) get the default period.
- The worker purges every `OSIRIS_RETENTION_INTERVAL` seconds; an admin can trigger it
  immediately with `POST /admin/retention/run`. The entity links of deleted items go too;
  entities no longer linked to any item are deleted.

**Erasure on request (admin)**

| Operation | Endpoint | Effect |
|-----------|----------|--------|
| Delete one item | `DELETE /items/{id}` | The item + entities linked only to it |
| Erase a data subject's data | `POST /entities/erase` | The entity (type + case-insensitive value), with `delete_items: true` **all items** mentioning it, and graph edges with the same name |

The erased value is not written to the audit log; only the SHA-256 digest of the lower-cased value
(`value_sha256`) is. This proves "this value was erased at this time" without keeping the value in
the log.

**Note:** erasure only affects the live database; earlier backups still contain the data. Choose
your backup retention accordingly.

## 12. Day-to-day operations

### 12.1 Logs

```bash
docker compose logs -f osiris-api          # JSON lines (OSIRIS_LOG_FORMAT=json)
docker compose logs -f osiris-pipeline
```

Access logs are written with `target = osiris_api::access` and contain the method, route template
(e.g. `/sources/{source_id}`), status and duration (ms). Path values, query strings and bodies are
not logged. Compose caps each service's log at 3 × 10 MB.

### 12.2 Metrics

The API serves Prometheus text-format metrics on a separate listener
(`osiris-api:9464/metrics`, internal network only). Add Prometheus to the same Compose network.

| Metric | Type | Description |
|--------|------|-------------|
| `osiris_http_requests_total{method,route,status}` | counter | Request count |
| `osiris_http_request_duration_seconds{method,route}` | histogram | Request duration |
| `osiris_api_rate_limited_total` | counter | Requests rejected by the quota |
| `osiris_audit_failures_total` | counter | Audit records that could not be written (investigate if non-zero) |
| `osiris_build_info{version}` | gauge | Version |

### 12.3 Verifying the audit chain

```bash
docker compose run --rm osiris-api audit-verify --limit 100000
# {"ok":true,"checked":…,"truncated":false,"error":null}   exit code 1 if broken
```

Each record is bound to the digest of the previous one; inserting, deleting or modifying a record
breaks the chain. Records from before migration 003 (v1) are not verified and are reported as
`Bilinmeyen chain_version: 1` ("unknown chain_version").

Audited actions: `auth.token`, `auth.key.create`, `auth.key.revoke`, `collect`, `collect.failed`,
`collect.batch`, `search`, `search.semantic`, `source.create`, `source.update`, `source.delete`,
`source.collect`, `item.delete`, `entity.erase`, `retention.run`.

### 12.4 Backup and restore

```bash
# Backup
docker compose exec -T postgresql pg_dump -U osiris -Fc osiris > osiris-$(date +%F).dump

# Restore (replaces existing data)
docker compose exec -T postgresql pg_restore -U osiris -d osiris --clean < osiris-YYYY-MM-DD.dump
```

Also back up `.env`, `deploy/nginx/certs/`, the n8n volume (`n8ndata`) and, if used, the Tor volume
(`tordata`). `./models` can be downloaded again. The Redis queue is transient; with AOF enabled,
pending messages survive a restart.

### 12.5 Upgrading

```bash
docker compose exec -T postgresql pg_dump -U osiris -Fc osiris > before-upgrade.dump   # back up first
git pull
docker compose --profile full build
docker compose run --rm osiris-api migrate        # new migrations (safe to re-run)
docker compose --profile full up -d
docker compose restart nginx                      # re-resolve upstream IPs
```

- Migrations are embedded in the binary and safe to re-run; applied ones are skipped.
- For configuration-only changes do not recreate the database container:
  `docker compose up -d --no-deps <service>` is enough.
- To avoid building on the server, build the images elsewhere and transfer them:
  `docker save osiris:1.0.0-rc osiris-web-ui:1.0.0-rc | gzip | ssh server 'gunzip | docker load'`.

### 12.6 Secret rotation

| Secret | Steps |
|--------|-------|
| `OSIRIS_API_KEY` | Update `.env` → `docker compose up -d --no-deps osiris-api n8n`. The old key stops working immediately. |
| `OSIRIS_JWT_SECRET` | Update `.env` → recreate `osiris-api`. All issued JWTs become invalid. |
| `POSTGRES_PASSWORD` | `docker compose exec postgresql psql -U osiris -c "ALTER ROLE osiris PASSWORD '…'"` → update `.env` → recreate the OSIRIS services. |
| User key | Revoke it in the UI and issue a new one. |

### 12.7 n8n workflow

`deploy/n8n/osiris-workflow.json` is a ready template: *Zamanlayıcı* (Scheduler, 15 minutes) →
*Ayarlar* (Settings: `feed_url`, `query`) → *OSIRIS Topla* (Collect, `POST /collect`) →
*OSIRIS Arama* (Search, `POST /search`). It reads the API key from the n8n service's
`OSIRIS_API_KEY` environment variable. Import it in the n8n UI, edit the **Ayarlar** node and
activate it. In your own workflows call the API at `http://osiris-api:8000` on the internal network.

## 13. Troubleshooting

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| `osiris-api` does not start; log: `OSIRIS_API_KEY tanımlı değil` | Operator key empty | Set `OSIRIS_API_KEY` in `.env` |
| `POST /auth/token` → `503` | `OSIRIS_JWT_SECRET` empty | Set the secret and recreate the API |
| Nginx `502` (`/api/`) | The API was recreated and Nginx still holds the old IP | `docker compose restart nginx` |
| Nginx does not start; log: certificate not found | `make-certs.sh` was not run | `./deploy/nginx/make-certs.sh` |
| Collection `502`: `İç ağ/özel host engellendi` or `Host çözülemedi` | The target is a private/internal address or does not resolve (SSRF protection) | Intended behaviour; check the target |
| Collection succeeds but no item appears | The worker is not running, or the same content already exists (`content_hash`) | `docker compose ps osiris-pipeline`, `docker compose logs osiris-pipeline` |
| Sources are not collected automatically | `OSIRIS_SCHEDULER` not on, source disabled or cron empty | `.env` + `up -d osiris-collector`; check the source |
| Semantic search `503` | No models or `OSIRIS_EMBEDDING_MODEL` empty | Run `osiris-models`; `./models` must be mounted into the API |
| No persons/organisations among entities | No NER model or `OSIRIS_NER=off` | Download the models |
| `429 İstek kotası aşıldı` | Per-identity quota exhausted | Slow the client down or raise `OSIRIS_API_RATE_PER_MIN` |
| `504` on long username scans | Nginx `proxy_read_timeout` is 60 s | Lower `max_sites` or raise `rate_per_sec` |
| Tor/I2P collection times out | `darknet` profile not running, or the proxy has not joined the network yet | `--profile darknet up -d`; Tor may take a few minutes to bootstrap |
| `osiris-models` permission error | `./models` owner differs from `OSIRIS_UID` | Set `OSIRIS_UID`/`OSIRIS_GID` in `.env` to `id -u`/`id -g` |

## 14. Known limitations

- **No alert notifications:** alert rules are stored and can be tested with `POST /alerts/test`,
  but the worker does not match collected items against rules or send webhook/Telegram/e-mail
  notifications. The handlers exist in the `osiris-alert` library.
- **Graph edges are added manually:** the pipeline does not derive relationships from entities that
  appear in the same item. (The `entity_relations` table exists in the schema but is unused.)
- **`web-scraper` collects a single page:** the `depth` and `follow_links` settings are accepted
  but links are not followed; `depth` is only written to item metadata.
- **Alert matching is substring-based:** `query_type` does not change how matching works.
- **Full-text search does no stemming** (`simple` dictionary).
- **Single node:** the components are designed for one server. Several workers can share the
  consumer group (with different `OSIRIS_PIPELINE_CONSUMER` values), but the API's in-memory graph
  and quota counters are per process.
- **No two-factor auth, secret vault, PDF reports or WASM plugins** (on the roadmap).

## 15. Roadmap

| Stage | Scope | Status |
|-------|-------|--------|
| Phases 1–5 | Core, 11 plugins, graph, semantic search, alert rules, reports, JWT/RBAC, TLS | Done |
| Rust migration R0–R7 | Freezing contracts, Rust core, data layer, API/CLI, removing Python | Done ([log](RUST_GECIS_DURUMU.md), Turkish) |
| Web UI | 11 screens, demo mode; semantic search, source editing, graph analysis/export, multi-format report endpoints | Done |
| Next | Publishing alerts from the worker, automatic graph edges, 2FA, secret vault, PDF reports, WASM plugins ([design](WASM_PLUGIN_TASARIMI.md), Turkish), `:443` | Planned |
