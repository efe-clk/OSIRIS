> **Türkçe:** Bu belgenin Türkçe sürümü için [buraya](TEKNIK_DOKUMAN.tr.md) bakın.

# OSIRIS
## Open Source Intelligence (OSINT) Platform — Technical Document

> **O**pen **S**ource **I**ntelligence **R**esearch & **I**nformation **S**ystem
> Version: 1.0.0-rc — Architecture, Installation & Operations
> Implementation: Rust (single binary, single distroless image) · UI: React/TypeScript

---

## Contents

1. [Project Summary](#1-project-summary)
2. [Key Features](#2-key-features)
3. [System Architecture](#3-system-architecture)
4. [Technology Stack](#4-technology-stack)
5. [Installation](#5-installation)
6. [Configuration](#6-configuration)
7. [Services and Profiles](#7-services-and-profiles)
8. [Data Sources and Plugins](#8-data-sources-and-plugins)
9. [Processing Pipeline](#9-processing-pipeline)
10. [Database](#10-database)
11. [Search, Graph, Alerts, Reports](#11-search-graph-alerts-reports)
12. [Interfaces and Automation](#12-interfaces-and-automation)
13. [Security, Privacy and KVKK](#13-security-privacy-and-kvkk)
14. [Operations](#14-operations)
15. [Roadmap](#15-roadmap)

---

## 1. Project Summary

OSIRIS is an OSINT platform that runs 24/7 on your own server (VDS). It collects data from the surface web, darknets (Tor, I2P), chat networks (IRC, Matrix), RSS/Atom, REST APIs, Shodan, DNS/WHOIS and blockchains, extracts and correlates entities, and makes everything searchable. No data leaves for a third-party cloud.

### Philosophy

- **Privacy first:** darknet traffic only through the operator's proxies; every request passes an SSRF guard.
- **Modularity:** every source is a plugin behind one trait (`Collector`).
- **Transparency:** every write is in a hash-chained audit log and can be verified.
- **Contracts:** behaviour is pinned by golden fixtures and black-box API tests ([SOZLESME.md](SOZLESME.md)).

---

## 2. Key Features

- **Collection:** 11 plugins (see §8), one-off (API/CLI) or cron-scheduled; per-plugin required-field checks, crash isolation, source health metrics.
- **Processing:** HTML cleaning and `content_hash` deduplication, language detection (16 languages), regex entities (e-mail, IP, domain, phone, crypto address, CVE) + ONNX NER (person, organisation, location), topic classification, 384-dimensional multilingual vectors.
- **Search:** full text (PostgreSQL `tsvector`), entity based; semantic search (pgvector) as a library.
- **Graph:** persistent entity relationship graph, centrality and community detection (library), GraphML/GEXF.
- **Alerts:** saved-query matching, muting, z-score anomalies, webhook/Telegram/e-mail handlers (library).
- **Reports:** Markdown (API), HTML/JSON/CSV/STIX 2.1 (library).
- **KVKK:** per-source retention, item and entity erasure, PII-free audit records.
- **Automation:** n8n workflow template, Prometheus metrics, JSON logs.

---

## 3. System Architecture

```
 Browser (React UI) ─┐        n8n ─┐
                     ▼             ▼
                 Nginx (TLS 8443, /api/* → osiris-api)
                     │
        ┌────────────▼─────────────┐        ┌────────────────────────┐
        │ osiris-server api        │        │ osiris-server collector│
        └──────┬───────────┬───────┘        └──────────┬─────────────┘
               │ SQL       │ XADD                      │ XADD
               ▼           ▼                           ▼
        PostgreSQL 16   Redis Streams  ──XREADGROUP──▶ osiris-server pipeline
```

Processes, data flow, crate responsibilities and the security model: [ARCHITECTURE.md](ARCHITECTURE.md).

---

## 4. Technology Stack

| Area | Choice |
|------|--------|
| Language | Rust (version pinned in `rust-toolchain.toml`), `unsafe` forbidden |
| HTTP server / client | axum 0.8, tower-http; reqwest (rustls) |
| Database | PostgreSQL 16 + pgvector (`source_metrics` hypertable if TimescaleDB is present), sqlx |
| Queue | Redis Streams (consumer group, at-least-once, dead letter) |
| ML | ONNX Runtime (`ort`) — mBERT-hrl NER int8; fastembed — paraphrase-multilingual-MiniLM-L12-v2 |
| Language detection | lingua (deterministic) |
| API docs | utoipa (OpenAPI 3) + Swagger UI |
| UI | React + TypeScript + Tailwind (Vite) |
| Deployment | Docker Compose, distroless image, Nginx (TLS), n8n |

---

## 5. Installation

### 5.1 System Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| **CPU** | 2 cores | 4-8 cores |
| **RAM** | 4 GB | 8-16 GB (with NER + embeddings) |
| **Storage** | 40 GB SSD | 200 GB+ NVMe |
| **OS** | A current Linux running Docker (Ubuntu 22.04+, Debian 12+) | |

Building the image from source takes ~10–15 min and needs ~4 GB RAM while compiling.

### 5.2 Steps

```bash
git clone https://github.com/efe-clk/OSIRIS.git && cd OSIRIS
cp .env.example .env && chmod 600 .env
#   POSTGRES_PASSWORD, N8N_PASSWORD, OSIRIS_API_KEY, OSIRIS_JWT_SECRET → openssl rand -hex 32
#   each (OSIRIS_API_KEY and OSIRIS_JWT_SECRET must DIFFER)
./deploy/nginx/make-certs.sh                       # self-signed TLS certificate

docker compose --profile models run --rm --build osiris-models   # models (optional, ~420 MB)
docker compose --profile full up -d --build                       # all services
```

`osiris-models` writes into `./models` as the host user (`OSIRIS_UID`/`OSIRIS_GID`, default 1000; add them to `.env` if yours differ). The system works without models: only regex entities are extracted and no vectors are produced.

The schema is created from `db/migrations/` on the first start of an empty PostgreSQL volume.

### 5.3 First Checks

```bash
docker compose ps                                     # osiris-api "healthy"
curl -sk https://localhost:8443/api/health            # {"service":"osiris-api","status":"ok"}
curl -sk -H "X-API-Key: $OSIRIS_API_KEY" https://localhost:8443/api/plugins | head -c 200
docker compose exec osiris-api osiris status          # CLI
```

Web UI: `https://<server>:8443/ui/` (log in with the operator key). n8n: `https://<server>:8443/n8n/`.

### 5.4 Network Topology

```
Internet ── [Nginx :80, :8443 TLS]
               ├── /api/* → osiris-api:8000   (prefix stripped)
               ├── /ui/*  → osiris-web-ui:3000
               └── /n8n/* → n8n:5678
[internal Docker network] osiris-api / osiris-pipeline / osiris-collector ←→ postgresql, redis
                osiris-api:9464/metrics (internal only) · tor:9050 · i2pd:4444 (darknet profile)
```

Application ports are bound to `127.0.0.1` on the host only (API `8001`, UI `3001`, PostgreSQL `5432`, Redis `6379`, n8n `5678`). On the reference machine `:443` belongs to another reverse proxy, so TLS is on `:8443` (see `deploy/nginx/README.md`).

---

## 6. Configuration

`.env` (never committed; `.env.example` is the template):

| Variable | Purpose |
|----------|---------|
| `POSTGRES_PASSWORD` | Database password |
| `N8N_USER` / `N8N_PASSWORD` | n8n gate (the first-setup value wins) |
| `OSIRIS_API_KEY` | Operator key (admin). Mandatory: the API refuses to start without it |
| `OSIRIS_INSECURE_OPEN` | `1` → open mode without a key (every caller is admin; trusted networks only) |
| `OSIRIS_JWT_SECRET` | JWT signing secret (no API-key fallback; empty → `/auth/token` 503) |
| `OSIRIS_CORS_ORIGINS` | Allowed browser origins (comma separated) |
| `OSIRIS_API_RATE_PER_MIN` | Per-identity quota per minute (600; 0 = off) |
| `OSIRIS_RETENTION_DAYS` | KVKK default retention in days (180; 0 = never by age) |
| `OSIRIS_RETENTION_INTERVAL` | Purge interval in the worker loop, seconds (3600; 0 = off) |
| `OSIRIS_SCHEDULER` | `on` → the collector runs sources on their cron (default `off`) |
| `OSIRIS_EMBEDDING_MODEL` | Embedding model; empty = no vectors (compose: MiniLM-L12-v2) |
| `OSIRIS_NER` | `off` → ONNX NER disabled (regex only) |
| `OSIRIS_MODEL_DIR` | Model root (`/app/models` in the image) |
| `OSIRIS_STREAM_MAXLEN` | Approximate cap of the queue stream (200000) |
| `OSIRIS_USERNAME_DB_DIR` | Extra allowed directory for username-search `sites_db` |
| `OSIRIS_TOR_SOCKS` / `OSIRIS_I2P_HTTP_PROXY` | Tor SOCKS5 / I2P HTTP proxy, `host:port` (compose: `tor:9050`, `i2pd:4444`) |
| `OSIRIS_UID` / `OSIRIS_GID` | User that `osiris-models` writes `./models` as (1000) |

Set by compose: `OSIRIS_LOG_FORMAT=json`, `OSIRIS_METRICS_ADDR=0.0.0.0:9464` (API), `OSIRIS_PIPELINE_CONSUMER`. The alert-handler variables (`OSIRIS_ALERT_WEBHOOK_URL`, `OSIRIS_ALERT_TELEGRAM_BOT_TOKEN`/`_CHAT_ID`, `OSIRIS_ALERT_SMTP_URL`, `OSIRIS_ALERT_EMAIL_FROM`/`_TO`) are library-level; no service publishes alerts yet.

---

## 7. Services and Profiles

| Service | Profile | Description |
|---------|---------|-------------|
| `postgresql`, `redis`, `n8n`, `nginx` | (always) | Infrastructure |
| `osiris-api` | `api`, `full` | REST API (`api`), healthcheck `osiris-server health` |
| `osiris-pipeline` | `worker`, `full` | Queue worker + retention purge; `./models` read-only |
| `osiris-collector` | `worker`, `full` | Scheduler (idle unless `OSIRIS_SCHEDULER=on`) |
| `osiris-web-ui` | `ui`, `full` | Browser interface |
| `osiris-models` | `models` | One-shot model download/verification |
| `tor`, `i2pd` | `darknet` | Darknet proxies (internal network only) |

The three OSIRIS services run the same image (`osiris:1.0.0-rc`) with different subcommands. One-off jobs use the same image: `docker compose run --rm osiris-api <subcommand>` (`migrate`, `audit-verify`, `purge`, `collect --plugin … --config … [--enqueue]`).

---

## 8. Data Sources and Plugins

| Plugin | Network | Summary |
|--------|---------|---------|
| `web-scraper` | www | Page + CSS selector, SSRF check on every redirect |
| `rss` | rss | RSS/Atom (feedparser-compatible sanitising) |
| `rest-api` | api | Method allowlist, JSON path; canonical JSON per record |
| `shodan` | api | Host search; the key never reaches errors or metadata |
| `dns-whois` | api | Allowlisted record types, bounded WHOIS |
| `blockchain` | blockchain | EVM (`eth_blockNumber`, balance, tx count), Bitcoin (allowlisted Blockstream) |
| `username-search` | www | Username footprint across ~2500 sites (merged Maigret + WhatsMyName DB) |
| `tor` | tor | `.onion` only through the operator's SOCKS5h proxy |
| `i2p` | i2p | Eepsites only through the operator's HTTP proxy |
| `irc` | irc | Injection-sanitised commands, SSRF-checked connection, time-boxed |
| `matrix` | matrix | Room history, URL-encoded identifiers |

Each plugin's configuration schema is returned by `GET /plugins` (the UI renders forms from it). Writing a new plugin: [CONTRIBUTING.md](CONTRIBUTING.md). The username-search site DB is refreshed with `plugins/username-search/tools/build_full_db.py` (development tool).

**Sources and collection:** a source is registered with `POST /sources` (`plugin_id`, `url`, `schedule` as a 5-field cron (UTC; day-of-week 0/7 = Sunday), `config`, `retention_days`). `POST /sources/{id}/collect` runs it once; with `OSIRIS_SCHEDULER=on` the collector runs every source on its cron (one run at a time per source). The task config is `metadata.config` + `source_id`; the source `url` is copied into `url`/`feed_url`/`endpoint` when those are empty. Health: `sources.last_crawled_at`, `last_success_at`, `failure_count`, `avg_response_ms` and `source_metrics`.

---

## 9. Processing Pipeline

```
[Plugin] → (≤ 512 KB JSON message) → Redis Stream osiris:stream:raw_items
   → osiris-server pipeline (consumer group osiris-pipeline)
       ├─ clean: HTML → text, collapse whitespace, 200 000 code points → content_hash
       ├─ language (lingua, 16 languages)
       ├─ entities: regex (6 types) + ONNX NER (PER/ORG/LOC), dedupe by (type, value.lower())
       ├─ topic classification (keywords)
       ├─ vector: fastembed, first 8000 code points, 128 tokens, L2-normalised → embedding_v2
       └─ one-transaction store (items ON CONFLICT content_hash, entities, item_entities)
   → XACK + XDEL
```

Failure rules: malformed message → dead-letter stream (`osiris:stream:raw_items:dead`, with `reason`); database error → the message stays pending and is reclaimed after 5 min; after 5 deliveries it is dead-lettered. The legacy LIST queue (`osiris:raw_items`) is drained first. Details: [SOZLESME.md](SOZLESME.md) §1, §5, §8, §10–11.

Back-fill vectors for old items: `docker compose run --rm osiris-pipeline pipeline --reembed`.

---

## 10. Database

| Migration | Content |
|-----------|---------|
| `001_init.sql` | `sources`, `items` (`content_hash UNIQUE`, FTS index), `entities` (unique `(type, value)`), `item_entities`, `entity_relations`, `saved_queries`, `audit_logs`, `source_metrics` (hypertable with TimescaleDB) |
| `002_roles.sql` | `user_role`, `users`, `api_keys` (hashes only), `graph_edges` |
| `003_audit_chain_version.sql` | `audit_logs.chain_version` |
| `004_retention.sql` | `sources.retention_days` (0–36500, CHECK) |
| `005_embedding.sql` | `items.embedding_v2 VECTOR(384)` + HNSW, `embedding_model` |
| `006_r7_cleanup.sql` | Drops the legacy `items.embedding` (1536) column; audit default version 2 |

All migrations are idempotent and embedded in the binary: `osiris-server migrate`. A missing TimescaleDB is not fatal (NOTICE + plain table).

---

## 11. Search, Graph, Alerts, Reports

| Capability | API endpoint | Library (crate) |
|------------|--------------|-----------------|
| Full-text search | `POST /search` | `osiris-query` |
| Entity search | `POST /search/entity`, `GET /entities/top` | `osiris-query` |
| Semantic search | — | `osiris_query::semantic_search` |
| Graph | `GET /graph`, `POST /graph/relation` | `osiris-graph` (centrality, CNM communities, GraphML/GEXF) |
| Alert matching | `POST /alerts/test`, `/saved-queries` | `osiris-alert` (anomalies, handlers) |
| Reports | `POST /reports/markdown` | `osiris-report` (HTML, JSON, CSV, STIX 2.1) |

Functions without an endpoint are deliberately library-level (decision E11); adding an endpoint is a contract change.

---

## 12. Interfaces and Automation

- **CLI:** `osiris status|plugins|collect` — exit codes 0 (ok), 1 (unknown plugin / collection failed), 2 (bad input). In the container: `docker compose exec osiris-api osiris …`.
- **REST API:** [API.md](API.md); interactive docs at `/api/docs`, OpenAPI at `/api/openapi.json`.
- **Web UI:** dashboard, sources, search, graph, alerts, reports (`/ui/`).
- **n8n:** `deploy/n8n/osiris-workflow.json` — Schedule (15 min) → Settings (`feed_url`, `query`) → OSIRIS Collect → OSIRIS Search; `X-API-Key` comes from the n8n service's `OSIRIS_API_KEY`. Edit the **Ayarlar** (settings) node after importing.

---

## 13. Security, Privacy and KVKK

Security model (egress control, identity, RBAC, quotas, audit, errors, transport, runtime): [ARCHITECTURE.md](ARCHITECTURE.md#security-model).

**KVKK (retention and erasure, [SOZLESME.md](SOZLESME.md) §6):**
- Retention rule: the source's `retention_days` (NULL → `OSIRIS_RETENTION_DAYS`, 0 → never by age); items without a source get the default. The worker purges every `OSIRIS_RETENTION_INTERVAL`, admins immediately with `POST /admin/retention/run`; orphaned entities are removed too.
- `DELETE /items/{id}`: the item + entities linked only to it.
- `POST /entities/erase`: one entity (type + case-insensitive value), optionally the items mentioning it, and graph edges with the same name; the audit record stores only a SHA-256 digest of the value.

---

## 14. Operations

### 14.1 Logs and Metrics
- `docker compose logs -f osiris-api` — JSON lines; access log `target = osiris_api::access` (method, route template, status, ms; no path values or query strings).
- Prometheus: `osiris-api:9464/metrics` (internal network; attach Prometheus to the same Compose network): `osiris_http_requests_total`, `osiris_http_request_duration_seconds`, `osiris_api_rate_limited_total`, `osiris_audit_failures_total`, `osiris_build_info`.

### 14.2 Audit Chain
```bash
docker compose run --rm osiris-api audit-verify --limit 100000   # {"ok":true,…}; exit 1 if broken
```
Note: verification of v1 records (before migration 003) was removed in R7; such records are reported as "Bilinmeyen chain_version" (unknown version).

### 14.3 Backups
```bash
docker compose exec -T postgresql pg_dump -U osiris -Fc osiris > osiris-$(date +%F).dump
# restore: docker compose exec -T postgresql pg_restore -U osiris -d osiris --clean < osiris-….dump
```
`./models` can be downloaded again; back up `.env` and `deploy/nginx/certs/` separately.

### 14.4 Upgrades
```bash
git pull
docker compose --profile full build
docker compose run --rm osiris-api migrate          # new migrations (idempotent)
docker compose --profile full up -d
docker compose restart nginx                        # re-resolve upstream IPs
```
Never recreate the database container for configuration-only changes; `up -d --no-deps <service>` is enough for application services. Rotating the DB password: `ALTER ROLE osiris PASSWORD '…'`, update `.env`, recreate the OSIRIS services.

### 14.5 Troubleshooting

| Symptom | Cause / fix |
|---------|-------------|
| `osiris-api` does not start, log: `OSIRIS_API_KEY tanımlı değil` | Set the key in `.env` |
| `/auth/token` returns 503 | `OSIRIS_JWT_SECRET` is empty |
| Nginx 502 on `/api/` | `docker compose restart nginx` after the API was recreated |
| Collection 502 `İç ağ/özel host engellendi` or `Host çözülemedi` | Target is a private/internal address or does not resolve (SSRF guard — intended) |
| Sources are never collected automatically | `OSIRIS_SCHEDULER` is not `on`, or `schedule` is empty/invalid |
| Only regex entities, no vectors | Models not downloaded (`osiris-models`) or `OSIRIS_NER=off` |
| 429 `İstek kotası aşıldı` | Raise `OSIRIS_API_RATE_PER_MIN` or slow the client down |

### 14.6 Test Matrix

| Test | Command |
|------|---------|
| Rust unit + fixtures | `cargo test --workspace` |
| Format / lint / dependencies | `cargo fmt --check` · `cargo clippy --all-targets -- -D warnings` · `cargo deny check` |
| DB + queue integration | `cargo test -p osiris-server --test integration` (`OSIRIS_TEST_DATABASE_URL`, `OSIRIS_TEST_REDIS_URL`) |
| Model fixtures | `cargo test --release -p osiris-pipeline --test models` |
| API contract | `pytest -q tests/contract` (`OSIRIS_BASE_URL`, `OSIRIS_CONTRACT_API_KEY`) |
| Compose | `docker compose config -q` |
| Web UI | `npm run build` (`osiris-web-ui/`) |

Details: [CONTRIBUTING.md](CONTRIBUTING.md).

---

## 15. Roadmap

| Stage | Scope | Status |
|-------|-------|--------|
| Phases 1–5 | Core, 11 plugins, graph, semantic search, alerts, reports, JWT/RBAC, TLS | ✅ |
| Rust migration R0–R7 | Contracts, Rust core, data layer, API/CLI/analysis, Python removal | ✅ ([status](RUST_GECIS_DURUMU.md)) |
| Next | Publishing alerts from the worker (handlers ready), 2FA, secret vault, PDF reports, WASM plugins ([design](WASM_PLUGIN_TASARIMI.md)), move to :443 | Planned |

---

*This document reflects the implemented system; update it whenever behaviour changes.*
