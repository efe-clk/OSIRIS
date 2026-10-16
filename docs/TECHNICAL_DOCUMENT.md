> **Türkçe:** Bu belgenin Türkçe sürümü için [TEKNIK_DOKUMAN.tr.md](TEKNIK_DOKUMAN.tr.md) dosyasına bakın.

# OSIRIS
## Open Source Intelligence (OSINT) Platform — Technical Document

> **O**pen **S**ource **I**ntelligence **R**esearch & **I**nformation **S**ystem
> Version: 0.5 — Architecture & Operations Document
> Status: Production pilot (self-hosted reference deployment live)

---

## Contents

1. [Project Summary](#1-project-summary)
2. [Core Features](#2-core-features)
3. [System Architecture](#3-system-architecture)
4. [Technology Stack](#4-technology-stack)
5. [Modules and Components](#5-modules-and-components)
6. [Supported Networks and Protocols](#6-supported-networks-and-protocols)
7. [Source Management System](#7-source-management-system)
8. [Data Flow](#8-data-flow)
9. [Database Design](#9-database-design)
10. [Security and Privacy](#10-security-and-privacy)
11. [Self-Host Infrastructure Requirements](#11-self-host-infrastructure-requirements)
12. [Plugin Architecture](#12-plugin-architecture)
13. [Interface Layers](#13-interface-layers)
14. [Automation and Workflows](#14-automation-and-workflows)
15. [Roadmap Status](#15-roadmap-status)
16. [Operations: Testing, Deployment, Configuration](#16-operations-testing-deployment-configuration)

---

## 1. Project Summary

OSIRIS collects data from multiple networks (surface web, darknets, P2P, encrypted networks), correlates and analyzes it, running fully self-hosted.

The system runs 24/7 on a VDS. All components are user-controlled; nothing leaks out, no third-party cloud dependencies.

### Core Philosophy

- **Privacy first** — all connections can go through proxy/VPN/Tor
- **Modularity** — every data source is an independent plugin
- **Extensibility** — new networks, protocols, and analysis methods integrate easily
- **Transparency** — open source, every operation logged and auditable

---

## 2. Core Features

### 2.1 Data Collection
- Multi-network support (WWW, Tor, I2P, RSS/Atom, REST APIs, IRC, Matrix, DNS/WHOIS, Shodan, blockchain)
- Parallel and scheduled fetching (full 5-field cron via APScheduler)
- Collection crash isolation (a failing plugin never takes down the manager)
- Queue backpressure guard (512 KB per-item cap, serialization skips)

### 2.2 Source Management
- Source health tracking: `last_crawled_at`, `last_success_at`, `failure_count`, `avg_response_ms`
- Time-series metrics in `source_metrics` (TimescaleDB hypertable when available)
- Priority and cron scheduling per source

### 2.3 Data Processing & Analysis
- HTML stripping, whitespace normalization, encoding repair
- Language detection, keyword topic classification
- Entity extraction: email, IP (validated), domain, phone, crypto address, CVE + spaCy NER fallback
- Optional semantic embeddings (`OSIRIS_EMBEDDING_MODEL`, pgvector with dimension-mismatch fallback)
- Content-hash deduplication

### 2.4 Search & Query
- Full-text search (PostgreSQL FTS + ranking)
- Semantic search (pgvector cosine)
- Entity-based search (LIKE-escaped)
- Validated limits (1–100), query length caps

### 2.5 Visualization
- Relationship graph (centrality, community detection, GraphML/GEXF export)
- Persistent graph edges in `graph_edges` (survives restarts)

### 2.6 Reporting & Export
- Markdown, HTML (autoescaped), JSON, CSV (formula-injection guarded)
- STIX 2.1 bundles (identity/indicator/vulnerability, deterministic UUIDs)
- Output-directory lock (no path traversal), size caps

### 2.7 Automation
- N8N workflow (search + collect nodes with `X-API-Key` headers and timeouts)
- Trigger-action patterns via saved queries + alert handlers (email/Telegram/webhook)
- Anomaly detection (rolling z-score baselines, warmup period, mute rules)

---

## 3. System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        OSIRIS PLATFORM                          │
│                                                                 │
│  ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   │
│  │   CLI    │   │ Web UI   │   │   Core   │   │  REST    │   │
│  │          │   │(React/TS)│   │  (C++)   │   │  API     │   │
│  └────┬─────┘   └────┬─────┘   └────┬─────┘   └────┬─────┘   │
│       └──────────────┴──────────────┴──────────────┘          │
│                              │                                  │
│                    ┌─────────▼─────────┐                       │
│                    │   API Gateway /   │                        │
│                    │   Core Engine     │  (C++ + Python)        │
│                    └─────────┬─────────┘                       │
│              ┌───────────────┼───────────────┐                 │
│              │               │               │                  │
│    ┌─────────▼───┐  ┌────────▼──────┐  ┌────▼────────────┐    │
│    │  Collector  │  │   Processing  │  │   Storage       │    │
│    │  Manager    │  │   Pipeline    │  │   Layer         │    │
│    │  (Python)   │  │   (Python)    │  │   (PostgreSQL)  │    │
│    └──────┬──────┘  └───────────────┘  └─────────────────┘    │
│           │                                                      │
│    ┌──────▼──────────────────────────────────────────────┐      │
│    │              Plugin Collection (11 plugins)          │      │
│    │  [WWW] [Tor] [I2P] [RSS] [API] [IRC] [Shodan] ...  │      │
│    └─────────────────────────────────────────────────────┘      │
│                                                                  │
│    ┌───────────────┐                         ┌──────────────┐   │
│    │   N8N         │                         │  Message     │   │
│    │  Automation   │                         │  Queue       │   │
│    └───────────────┘                         │  (Redis)     │   │
│                                              └──────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

### Layer Descriptions

| Layer | Role | Technology |
|-------|------|-----------|
| **Interface** | User interaction | CLI (Click), Web UI (React), REST (FastAPI) |
| **API Gateway** | Connects all components, auth + validation | Python/FastAPI (scheduling: APScheduler, Python — `osiris-collector`) |
| **Core Engine** | Task management, plugin lifecycle | C++20 |
| **Collector Manager** | Plugin orchestration, queue delivery | Python |
| **Processing Pipeline** | Clean, enrich, deduplicate, store | Python |
| **Storage Layer** | Persistent storage | PostgreSQL + Redis |
| **Automation** | Workflows and triggers | N8N |
| **Feed Engine** | RSS/Atom management | `rss` plugin (FreshRSS removed in R3) |
| **Message Queue** | Async inter-component messaging | Redis |

---

## 4. Technology Stack

### 4.1 Languages

| Language | Area | Rationale |
|----------|------|-----------|
| **C++** (C++20) | Core Engine | Memory efficiency, scheduling |
| **Python 3.11+** | Plugins, pipeline, API, analysis | Ecosystem, velocity |
| **TypeScript** | Web UI | Type-safe frontend |
| **SQL** | Queries, migrations | — |

### 4.2 Database & Storage

| Technology | Use |
|-----------|-----|
| **PostgreSQL 16** | Primary relational store |
| **pgvector** | Embeddings — semantic search |
| **Redis** | Cache + `osiris:raw_items` queue + `osiris:alerts` pub/sub |
| **TimescaleDB** | Optional; `source_metrics` hypertable when present (graceful fallback) |

### 4.3 Interface Technologies

| Technology | Use | Status |
|-----------|-----|--------|
| **CLI** (Click + Rich) | Operations, collection jobs | Working |
| **REST API** (FastAPI) | Primary programmatic interface | Working (JWT + RBAC) |
| **Web UI** (React + TS + Tailwind) | Browser access | Basic screens |
| **Qt6** | Native desktop client | Planned |

### 4.4 Network Libraries

`requests` (HTTP, explicit `verify=True`), `feedparser`, `dnspython`, SOCKS5 (Tor), plain HTTP proxy (I2P), raw sockets (IRC/WHOIS with timeouts).

### 4.5 Data Processing & ML

`beautifulsoup4`, `langdetect`, `spaCy` (optional, graceful fallback), `sentence-transformers` (optional extra `osiris-pipeline[embedding]`), `networkx`, `pandas`.

### 4.6 Automation & Integration

N8N (self-hosted, API-key headers), APScheduler (cron), Redis pub/sub (alerts).

### 4.7 Security & Cryptography

TLS 1.2/1.3 (Nginx `:8443`), SHA-256 API-key hashes, HS256 JWT (stdlib-only), hash-chained audit logs, SSRF allow/deny engine, `.env` secrets (git-ignored, mode 600).

### 4.8 Containers & Deployment

Docker, Docker Compose (profiles: default infra, `api`, `ui`, `full`), Nginx reverse proxy, self-signed bootstrap cert (replaceable with Let's Encrypt).

---

## 5. Modules and Components

### 5.1 Core Engine (`osiris-core`)
- **Language:** C++20, `-Wall -Wextra -Wpedantic`, zero warnings
- **Role:** central coordinator
- **Implemented:** plugin register/start/stop, task list, 1-second scheduler loop with atomic stop flag (`stop_scheduler`), versioned CLI
- **Deferred:** full cron parsing, IPC server (REST lives in `osiris-api`)

### 5.2 Collector Manager (`osiris-collector`)
- Plugin loading with manifest validation (bad manifests logged, skipped)
- Crash isolation per collection call; oversized/unserializable items skipped
- Full 5-field cron via `CronTrigger` (step values like `*/15` supported)
- Queue delivery with byte cap; Redis errors reported without crashing
- Optional source health writes (`sources` + `source_metrics`) when `source_id` is configured

### 5.3 Processing Pipeline (`osiris-pipeline`)
Stages: clean → language → NER → classify → embed (optional) → dedup → store.
- `store()` uses upsert + `content_hash` dedup, entity upserts, `item_entities` mention counts
- Embedding insert uses a SAVEPOINT: dimension mismatch falls back to vectorless insert (cached decision)

### 5.4 Query Engine (`osiris-query`)
- `fulltext_search`, `semantic_search` (vector-literal adaptation), `entity_search`
- `dict_row` results, limit clamp (1–100, `<1` rejected), LIKE-wildcard escaping, entity-type allowlist

### 5.5 Graph Engine (`osiris-graph`)
- Weighted edges, node/edge caps, `OverflowError` on limit breach
- `centrality`, `communities`, `neighbors` (all empty-graph safe)
- JSON/GraphML/GEXF export
- **Persistence:** `save_to_db()` / `load_from_db()` against `graph_edges`

### 5.6 Alert Manager (`osiris-alert`)
- Saved-query substring matching with enable/mute rules
- Lazy Redis (offline mode supported), handler exception isolation
- **Anomaly detection:** per-metric rolling baselines, z-score threshold (default 3.0), warmup minimum, zero-variance and NaN guards

### 5.7 Report Generator (`osiris-report`)
- Markdown, HTML (autoescaped), JSON, CSV (formula guard), STIX 2.1
- Output confined to `output_dir`; oversized reports rejected

### 5.8 REST API (`osiris-api`)

Full endpoint reference: [API.md](API.md).

Auth: `X-API-Key` (operator) or per-user DB keys or `Authorization: Bearer <JWT>`. CORS allowlist, Pydantic limits, no internal details in errors, best-effort audit writes.

### 5.9 SDK (`osiris-sdk`)
`plugin` (BaseCollector/CollectedItem/CollectionResult), `models` (Source/Entity/Item), `security` (SSRF + sanitizers), `audit` (append/verify hash chain, advisory-locked writers).

---

## 6. Supported Networks and Protocols

Implemented collectors (11): **web-scraper** (WWW), **tor** (`.onion` via localhost SOCKS5), **i2p** (eepsites via localhost HTTP proxy), **rss** (RSS/Atom, capped), **rest-api** (method allowlist, JSON-path dig), **irc** (injection-sanitized, SSRF-guarded, PING-aware, time-boxed), **matrix** (room history, URL-encoded IDs), **dns-whois** (validated domains, allowlisted record types, bounded WHOIS), **shodan** (key never logged), **blockchain** (EVM `eth_blockNumber`/balance/txcount, Bitcoin via Blockstream allowlist), **username-search** (Maigret-style username footprint across ~2500 bundled sites: Maigret + WhatsMyName merged DB, refreshable via `tools/build_full_db.py`).

Planned: Freenet/ZeroNet/RetroShare, BitTorrent/IPFS, XMPP/Nostr/Fediverse, BGP/cert-transparency, SDR/ADS-B/AIS, Monero limits documentation.

---

## 7. Source Management System

### 7.1 Source Record

```sql
sources (
  id UUID PRIMARY KEY,
  name TEXT NOT NULL,
  url TEXT,
  network_type network_type NOT NULL,   -- www|tor|i2p|p2p|freenet|zeronet|irc|
                                        -- matrix|rss|api|blockchain|sdr|custom
  plugin_id TEXT NOT NULL,
  auth_config JSONB,        -- credentials (to be Vault-backed)
  proxy_config JSONB,
  schedule TEXT,            -- 5-field cron
  priority INTEGER DEFAULT 5,
  enabled BOOLEAN DEFAULT TRUE,
  tags TEXT[],
  last_crawled_at TIMESTAMPTZ,
  last_success_at TIMESTAMPTZ,
  failure_count INTEGER DEFAULT 0,
  avg_response_ms INTEGER,
  metadata JSONB,
  created_at TIMESTAMPTZ DEFAULT NOW(),
  updated_at TIMESTAMPTZ DEFAULT NOW()
)
```

### 7.2 Collection Flow
1. Operator configures source type + plugin (schema-driven form in UI roadmap)
2. Optional connectivity check (`health_check`)
3. Source stored; Collector Manager triggers first run immediately or on cron
4. Health dashboard reads `sources` + `source_metrics`

---

## 8. Data Flow

```
[Source]
    │
    ▼
[Collector Plugin]  (validated config, timeouts, size caps)
    │  raw items
    ▼
[Redis osiris:raw_items]
    │
    ▼
[Processing Pipeline]
    ├─ Clean & Normalize
    ├─ Language Detect
    ├─ Entity Extract (regex + spaCy)
    ├─ Classify (topics)
    ├─ Embed (optional model)
    └─ Deduplicate (content_hash)
    │
    ▼
[PostgreSQL]
    │
    ├─ Query Engine ──────► UI / API
    ├─ Graph Engine ──────► Visualization (+ persistent edges)
    ├─ Alert Manager ────► Notifications (+ anomalies)
    └─ Report Generator ► Markdown/HTML/JSON/CSV/STIX
```

---

## 9. Database Design

Migration `001_init.sql`: `sources`, `items` (`content_hash UNIQUE`, `embedding VECTOR(1536)`, FTS index), `entities` (+ unique `(type, value)`), `item_entities`, `entity_relations`, `saved_queries`, `audit_logs` (hash chain), `source_metrics` (hypertable when TimescaleDB exists, plain table otherwise).

Migration `002_roles.sql`: `user_role` enum, `users`, `api_keys` (hash-only, prefix index, revoke flag), `graph_edges` (composite PK, CHECKs, source/target indexes).

TimescaleDB absence is non-fatal (NOTICE + fallback), verified on stock `pgvector:pg16`.

---

## 10. Security and Privacy

### 10.1 Operational Security (OPSEC)
- Configurable proxy chains; per-source proxy profiles (schema-ready)
- Localhost-only Tor/I2P proxy enforcement in code
- Fail-closed DNS: unresolvable hosts are blocked, not allowed

### 10.2 Data Security
- Secrets in `.env` (git-ignored); DB passwords rotated via `ALTER ROLE` runbook
- API keys stored as SHA-256; raw values returned once, never listed
- Reports escapable to signed/encrypted packaging (GPG step documented for export flow)

### 10.3 Access Control
- Roles: viewer → analyst → admin (unknown denied); enforced per endpoint
- API keys (operator + per-user) and short-lived JWTs (HS256, pinned alg, constant-time verify)
- 2FA (TOTP): planned, after password-based users land

### 10.4 Audit & Logging
- Hash-chained `audit_logs` with concurrent-writer lock and `verify_chain` checker
- API collect/search/key operations logged best-effort (never break requests)
- Python logging without secret interpolation; C++ mutex-guarded logger

---

## 11. Self-Host Infrastructure Requirements

### 11.1 Minimum System Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| **CPU** | 4 cores | 8–16 cores |
| **RAM** | 8 GB | 32–64 GB (with local embedding model) |
| **Storage** | 100 GB SSD | 1 TB+ NVMe |
| **Bandwidth** | 100 Mbps | 1 Gbps |
| **OS** | Ubuntu 22.04 LTS | Ubuntu 24.04 LTS |

### 11.2 VDS Service Map (Docker Compose)

```yaml
services:
  postgresql:        # main DB (host 5432)
  redis:             # cache + queue (host 6379)
  n8n:               # automation (host 5678)
  osiris-api:        # REST API (127.0.0.1:8001 → 8000, profile: api/full)
  osiris-web-ui:     # frontend (127.0.0.1:3001 → 3000, profile: ui/full)
  nginx:             # reverse proxy + TLS (:80, :8443)
```

Direct app ports bind localhost-only; external access goes through Nginx.

### 11.3 Network Topology

```
Internet
   │
[Nginx] (:80, :8443 ssl)
   │
   ├── /api/*      → osiris-api:8000   (prefix stripped)
   ├── /n8n/*      → n8n:5678
   └── /ui/*       → osiris-web-ui:3000

[Internal Docker Network]
   osiris-api ←→ postgresql / redis
   collector/pipeline ←→ redis (queue)
   pipeline/query ←→ postgresql
```

Port `:443` is occupied on the reference host by another reverse proxy; TLS is served on `:8443` until a domain cutover (see `deploy/nginx/README.md`).

---

## 12. Plugin Architecture

### 12.1 Plugin Layout

```
plugins/
└── web-scraper/
    ├── manifest.json     # metadata + config_schema
    ├── collector.py      # collection logic
    ├── requirements.txt  # Python dependencies
    └── README.md
```

### 12.2 manifest.json Example

```json
{
  "id": "web-scraper",
  "name": "Web Scraper",
  "version": "1.0.0",
  "network_type": "www",
  "config_schema": {
    "url": {"type": "string", "required": true},
    "css_selector": {"type": "string"}
  },
  "capabilities": ["html", "text"],
  "requires_proxy": false,
  "schedule_default": "0 */6 * * *"
}
```

### 12.3 Plugin Interface (Python)

```python
from osiris.plugin import BaseCollector, CollectionResult

class MyCollector(BaseCollector):
    def collect(self, config: dict) -> CollectionResult:
        ...  # validate, fetch, return items
        return CollectionResult(items=[...], metadata={...})

    def health_check(self) -> bool:
        ...
```

Security obligations: `assert_safe_url` for URLs, `safe_create_connection` for raw sockets (resolve → vet → connect to the vetted IP), `sanitize_*` for hosts/domains/IRC tokens, no secret logging, bounded I/O.

---

## 13. Interface Layers

### 13.1 CLI
`osiris status|plugins|collect` — clean exit codes (0 ok, 1 failure, 2 bad input).

### 13.2 REST API
Primary interface (endpoint reference: [API.md](API.md)). Machine clients use API keys or JWTs.

### 13.3 Web UI
React + TypeScript + Tailwind: Dashboard, Sources, Search. API-wired search and source health views are next; static build served by Nginx.

### 13.4 Desktop (planned)
Qt6 power client and Tauri lightweight client remain roadmap items (§15).

---

## 14. Automation and Workflows

### 14.1 N8N Integration
`deploy/n8n/osiris-workflow.json`: schedule → search → collect, with `X-API-Key` headers (`{{ $env.OSIRIS_API_KEY }}` — set it on the n8n service) and timeouts.

### 14.2 RSS
Feeds are collected directly by the `rss` plugin. FreshRSS (a standalone reader no
code used) and the MinIO object store were removed from compose in Rust migration
R3 (K2/K3).

### 14.3 Webhooks & Alerts
Alert handlers (email/Telegram/webhook) plug into `AlertManager.register_handler`; anomaly spikes emit through the same channel.

---

## 15. Roadmap Status

| Phase | Scope | Status |
|-------|-------|--------|
| **Phase 1** | Core skeleton, DB schema, first 3 plugins, pipeline, CLI | ✅ Done |
| **Phase 2** | Tor, I2P, IRC/Matrix, Shodan, DNS/WHOIS, Web UI basics | ✅ Done |
| **Phase 3** | Graph, semantic search, alerts, reports | ✅ Done |
| **Phase 4** | Automation, blockchain, JWT, STIX, hardening, TLS | ✅ Done |
| **Phase 5.1** | Test coverage ≥80% (CI gate) | ✅ Done (90%) |
| **Phase 5.2** | RBAC + graph persistence + anomaly detection | ✅ Done |
| **Next** | 2FA, Vault secrets, PDF reports, Qt/Tauri clients, :443 cutover, full docs polish | Planned |

---

## 16. Operations: Testing, Deployment, Configuration

### 16.1 Test Matrix (all runnable)

| Test | Command | Gate |
|------|---------|------|
| Python unit + coverage | `pytest -q` | `--cov-fail-under=80` |
| Lint | `ruff check .` | zero findings |
| C++ build | `cmake --build` | `-Wall -Wextra -Wpedantic`, zero warnings |
| Compose validity | `docker compose config` | exit 0 |
| DB migration | `psql -v ON_ERROR_STOP=1 -f db/migrations/*.sql` | clean on stock `pgvector:pg16` |
| Live API smoke | TestClient + curl via Nginx | auth matrix (401/403/200) |
| Web UI build | `npm run build` | `tsc` + `vite` clean |

### 16.2 Environment (`.env`, never committed)

| Variable | Purpose |
|----------|---------|
| `POSTGRES_PASSWORD` | DB password (rotate via `ALTER ROLE`, then restart API) |
| `N8N_USER/PASSWORD` | n8n gate (first-setup value wins) |
| `OSIRIS_API_KEY` | Operator key (admin). Required: the API refuses to start without it |
| `OSIRIS_INSECURE_OPEN` | `1` enables keyless open mode (trusted network only) |
| `OSIRIS_JWT_SECRET` | JWT signing secret (no fallback; unset → `/auth/token` 503) |
| `OSIRIS_CORS_ORIGINS` | Allowed browser origins |
| `OSIRIS_RETENTION_DAYS` | KVKK default retention, days (180; 0 = off) |
| `OSIRIS_RETENTION_INTERVAL` | Purge interval in the pipeline loop, s (3600; 0 = off) |
| `OSIRIS_USERNAME_DB_DIR` | Extra allowed directory for username-search `sites_db` |

### 16.3 Deployment Notes
- Fresh install: `cp .env.example .env` (generate strong values), `docker compose --profile full up -d --build`, apply `db/migrations/*.sql` in order.
- Existing stack: prefer `up -d --no-deps` / `--no-recreate` for app services; never recreate the database container for config-only changes.
- live DB password rotation: `ALTER ROLE …`, update `.env`, recreate `osiris-api` only, verify `/api/plugins` with the operator key.

---

*This document reflects the implemented system. Update it with every phase.*
