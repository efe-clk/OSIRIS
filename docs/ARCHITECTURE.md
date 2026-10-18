> **Türkçe:** [ARCHITECTURE.tr.md](ARCHITECTURE.tr.md)

# OSIRIS Architecture

This document describes how OSIRIS 1.0 is built: which processes run, the path data takes from
collection to search, the database model, the responsibility of each crate and the security
model. For installation and operations see [TECHNICAL_DOCUMENT.md](TECHNICAL_DOCUMENT.md); for
the behaviour rules that must not change see [SOZLESME.md](SOZLESME.md) (Turkish).

## 1. Overview

```
  Browser (React UI)           n8n / scripts / CLI
          │                           │
          ▼                           ▼
   ┌──────────────────── Nginx ─────────────────────┐
   │ TLS :8443 · security headers · 10 req/s per IP │
   │ /ui/* → web-ui   /api/* → api   /n8n/* → n8n   │
   └───────────────────────┬────────────────────────┘
                           ▼
            ┌─────────────────────────────┐
            │ osiris-server api           │  identity · RBAC · quota · validation
            │ (axum · 29 paths · /metrics)│  audit log · in-memory graph
            └──┬────────────┬─────────────┘
       SQL     │            │ XADD (collected items)
               ▼            ▼
      ┌──────────────┐  ┌───────────────────────────┐   XADD   ┌────────────────────────┐
      │ PostgreSQL 16│  │ Redis Stream              │◄─────────│ osiris-server collector│
      │ + pgvector   │  │ osiris:stream:raw_items   │          │ cron scheduler         │
      └──────▲───────┘  └────────────┬──────────────┘          └────────────────────────┘
             │ one transaction       │ XREADGROUP (group: osiris-pipeline)
             │                       ▼
             │          ┌──────────────────────────────────────────┐
             └──────────│ osiris-server pipeline                   │
                        │ clean → language → regex + NER → topic → │
                        │ vector → store → XACK+XDEL               │
                        └──────────────────────────────────────────┘
```

Key design ideas:

- **One binary, many roles.** `osiris-server` becomes the API, the worker or the scheduler via a
  subcommand. A single image (`osiris:1.0.0-rc`, distroless) runs as three services, so the
  services can never drift apart in version.
- **Collection is decoupled from processing.** Plugins only collect and enqueue; heavy work (NER,
  vectors) happens in the worker. A collection triggered from the API returns quickly even while
  the worker is busy.
- **The database is the single source of truth.** Sources, items, entities, graph, keys and the
  audit log live in PostgreSQL; Redis is only a transient queue.
- **Network access goes through the host.** Plugins cannot open sockets; they use the host's
  controlled HTTP, DNS and TCP capabilities. SSRF and proxy rules are enforced in one place.
- **Behaviour is pinned by data.** Hashing, entity extraction, reports, graph, queue and API rules
  are tested with `fixtures/` and `tests/contract/` ([SOZLESME.md](SOZLESME.md)).

## 2. Processes

| Service | Subcommand | Role |
|---------|------------|------|
| `osiris-api` | `api --addr 0.0.0.0:8000` | REST API (axum): identity resolution, role checks, per-identity quota, input validation, endpoints, OpenAPI (`/openapi.json`, `/docs`); separate Prometheus listener on `OSIRIS_METRICS_ADDR` |
| `osiris-pipeline` | `pipeline --loop --interval 5 --batch 50` | Consumes the stream, processes and stores items; runs the retention purge every `OSIRIS_RETENTION_INTERVAL` |
| `osiris-collector` | `collector` | Runs sources on their cron when `OSIRIS_SCHEDULER=on`; otherwise idles |

One-off subcommands:

| Subcommand | Job |
|------------|-----|
| `migrate` | Applies the embedded SQL migrations (safe to re-run) |
| `models [--only ner\|embedding]` | Downloads the ONNX models from pinned revisions and verifies their digests |
| `audit-verify [--limit N]` | Verifies the last N records of the audit chain (exit 1 if broken) |
| `purge` | Deletes items past their retention period and orphaned entities |
| `collect --plugin P --config JSON [--enqueue]` | One collection; without `--enqueue` it only prints the result |
| `pipeline --once` / `pipeline --reembed` | Processes one round from the queue / vectorises items without vectors |
| `health [--addr]` | Container health check (`GET /health` 200 → exit 0) |

The `osiris` CLI in the same image: `status`, `plugins`, `collect <plugin> --config JSON` (exit
codes: 0 success, 1 unknown plugin or failed collection, 2 invalid input).

All services finish in-flight work and exit on `SIGTERM` (e.g. `docker stop`).

## 3. Data flow

### 3.1 Collection

1. Trigger: the API (`POST /collect`, `/collect/batch`, `/sources/{id}/collect`), the scheduler or the CLI.
2. `osiris_collector::Manager::run_collection` looks up the plugin and checks the manifest's
   required fields and the plugin's own validation (`Zorunlu alanlar eksik: …`).
3. The plugin runs in a separate Tokio task; a crash or panic turns that collection into a
   `plugin hatası` (plugin error) result while the process stays up.
4. The plugin reaches the network only through host capabilities (see [§6](#6-plugin-host-model)).
5. Every item of a successful result becomes a **queue message** and is written to the stream
   with `XADD MAXLEN ~ 200000`. The message is `{"plugin_id", "source_id", "item", "collected_at"}`,
   at most 512 000 bytes (`schemas/queue_message.schema.json`).
6. If the configuration contains `source_id`, source health (`sources`, `source_metrics`) is recorded.

### 3.2 Processing

On every round the worker, in order:

1. drains the legacy LIST queue (`osiris:raw_items`, kept for migration compatibility);
2. claims messages unacknowledged for 5 minutes with `XAUTOCLAIM`;
3. reads new messages with `XREADGROUP` (group `osiris-pipeline`).

For each message:

| Step | What it does | Crate |
|------|--------------|-------|
| Parse | JSON and schema check; malformed messages go to the dead-letter stream (`osiris:stream:raw_items:dead`) | `osiris-pipeline` |
| Clean | HTML → text, whitespace collapsing, first 200 000 code points; `content_hash` = SHA-256 | `osiris-hash` |
| Language | lingua, 16 languages (ISO 639-1 code; `unknown` if ambiguous) | `osiris-pipeline::lang` |
| Entities | Regex (e-mail, IP, domain, phone, crypto address, CVE) + ONNX NER (person, organisation, location); deduplicated by `(type, lower-case value)` | `osiris-extract`, `osiris-pipeline::ner` |
| Topic | Keyword tags: `threat`, `breach`, `vulnerability`, `phishing` | `osiris-pipeline` |
| Vector | fastembed; first 8 000 code points, 128 tokens, mean pooling, L2 normalised, 384 dimensions | `osiris-pipeline::embed` |
| Store | One transaction: `items` (`ON CONFLICT content_hash DO NOTHING`), `entities` (upsert), `item_entities` | `osiris-db` |
| Acknowledge | `XACK` + `XDEL` | `osiris-pipeline::stream` |

Error rules: malformed message → dead letter (`reason=invalid: …`). Database error → the message
stays pending and is reclaimed after 5 minutes; after 5 deliveries it goes to the dead letter
(`reason=max_deliveries`). Delivery is **at least once**; processing a message twice is harmless.

### 3.3 Querying and presentation

- Search, entity, statistics and list endpoints build JSON in PostgreSQL with `json_agg` and return
  the text as is (column order and timestamp format stay stable).
- For semantic search the query vector is produced in the API process; the model loads on the
  first search.
- The graph is held in memory in the API process and loaded from `graph_edges` on first use.
  Analysis and export run on a copy, without blocking the async workers.
- The web UI and n8n use only the REST API; no client connects to the database directly.

## 4. Data model

The schema lives in `db/migrations/`. Migrations are embedded in the binary, applied with
`osiris-server migrate`, and all are safe to re-run (`IF NOT EXISTS`).

| Table | Content | Important columns |
|-------|---------|-------------------|
| `sources` | Saved sources | `plugin_id`, `network_type`, `url`, `schedule` (cron), `priority` 1–10, `enabled`, `tags`, `metadata.config` (plugin settings), health (`last_crawled_at`, `last_success_at`, `failure_count`, `avg_response_ms`), `retention_days` |
| `items` | Processed items | `source_id` (items are deleted with their source), `raw_content`, `cleaned_content`, `url`, `title`, `language`, `collected_at`, `published_at`, `content_hash` **UNIQUE**, `metadata`, `tags`, `embedding_v2 VECTOR(384)`, `embedding_model` |
| `entities` | Entities | `type` (enum), `value`; unique `(type, value)`; `first_seen_at`, `last_seen_at` |
| `item_entities` | Item–entity links | `mention_count`; cascading delete in both directions |
| `graph_edges` | Graph edges | `source_name`, `target_name`, `relation_type`, `weight`, `evidence_count` |
| `saved_queries` | Saved queries / alert rules | `query_text`, `query_type` (enum), `alert_enabled`, `last_triggered_at` |
| `users`, `api_keys` | Users and their keys | `users.role` (enum); `api_keys.key_hash` (SHA-256), `key_prefix` (first 8 characters), `revoked`, `last_used_at` |
| `audit_logs` | Hash-chained audit log | `user_id`, `action`, `resource`, `detail`, `prev_hash`, `hash`, `chain_version` |
| `source_metrics` | Source run history | `time`, `source_id`, `response_ms`, `success`, `items_count`; a hypertable if TimescaleDB is present |
| `entity_relations` | Unused (reserved for automatic relationships) | |

Enumerations: `network_type` = `www, tor, i2p, p2p, freenet, zeronet, irc, matrix, rss, api,
blockchain, sdr, custom`; `entity_type` = `person, org, location, ip, domain, email, phone,
crypto_address, hash, username, cve, custom`; `query_type` = `fts, semantic, regex, entity,
graph`; `user_role` = `admin, analyst, viewer`.

Indexes: GIN on `to_tsvector('simple', cleaned_content)` for full-text; HNSW (cosine) on
`embedding_v2` for vectors; plus indexes on source, date, language, entity type/value and key digest.

| Migration | Change |
|-----------|--------|
| `001_init.sql` | Core tables and enumerations; `source_metrics` hypertable if TimescaleDB is present |
| `002_roles.sql` | `users`, `api_keys`, `graph_edges` |
| `003_audit_chain_version.sql` | `audit_logs.chain_version` |
| `004_retention.sql` | `sources.retention_days` (0–36 500, CHECK) |
| `005_embedding.sql` | `items.embedding_v2 VECTOR(384)` + HNSW, `embedding_model` |
| `006_r7_cleanup.sql` | Drops the old `items.embedding` (1536) column; audit default version 2 |

## 5. Crates

The Cargo workspace lives in `crates/`; the toolchain is pinned by `rust-toolchain.toml` and
`unsafe` code is forbidden.

```
osiris-server ─┬─ osiris-collector ─┬─ osiris-plugins ─┬─ osiris-plugin ─┬─ osiris-security
               │                    │                  │                 └─ osiris-types
               │                    │                  └─ osiris-feed
               │                    ├─ osiris-queue
               │                    └─ osiris-db
               ├─ osiris-pipeline ──┬─ osiris-extract
               │                    └─ osiris-db, osiris-queue, osiris-types
               ├─ osiris-query ──── osiris-db
               ├─ osiris-graph, osiris-alert, osiris-report
               └─ (beneath all of them) osiris-hash
osiris-cli ──── osiris-collector, osiris-db, osiris-plugin
```

| Crate | Responsibility |
|-------|----------------|
| `osiris-server` | Service binary and the `api` module (routes, identity, input validation, errors, metrics, OpenAPI) |
| `osiris-cli` | The `osiris` command-line tool |
| `osiris-collector` | Running collections, task configuration from sources, enqueuing, health recording, cron scheduler |
| `osiris-queue` | Queue producer (`XADD`); usable without pulling in the ML runtime |
| `osiris-pipeline` | Stream consumer, processing steps, NER (`ort`), embedding (`fastembed`), language detection, model downloader |
| `osiris-plugin` | The `Collector` trait, host capabilities (controlled HTTP/DNS/TCP, proxies), fixture replayer |
| `osiris-plugins` | 11 plugins: `rss`, `web-scraper`, `dns-whois`, `rest-api`, `shodan`, `blockchain`, `username-search`, `tor`, `i2p`, `matrix`, `irc` |
| `osiris-feed` | RSS/Atom parsing and sanitising |
| `osiris-db` | PostgreSQL access (sqlx), embedded migrations, item writes, audit write/verify, retention, graph edges |
| `osiris-query` | Full-text, semantic and entity search |
| `osiris-graph` | Undirected entity graph, degree centrality, CNM communities, GraphML/GEXF |
| `osiris-alert` | Rule matching, muting, z-score anomalies, Redis publishing, webhook/Telegram/e-mail handlers |
| `osiris-report` | Markdown, HTML (escaped), JSON, CSV (formula protection), STIX 2.1 |
| `osiris-hash` | `content_hash` cleaning, HTML text extraction, canonical JSON (RFC 8785), audit chain digest |
| `osiris-extract` | Regex entity extraction |
| `osiris-security` | SSRF checks (URL policy, DNS check, IPv4-mapped IPv6) and input sanitisers |
| `osiris-types` | Shared types: manifest, collected item, collection result, queue message (validated against `schemas/`) |

Embedded at compile time: `plugins/*/manifest.json`, the `username-search` site databases
(`sites.json`, `sites_full.json`) and `db/migrations/*.sql`. Models are not embedded (see
[models/README.md](../models/README.md)).

## 6. Plugin host model

Every plugin implements the `osiris_plugin::Collector` trait:

```rust
#[async_trait]
pub trait Collector: Send + Sync {
    fn manifest(&self) -> &Manifest;
    async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash>;
    async fn health_check(&self, host: &Host, cfg: &Config) -> bool;
    /// Optional extra validation: returns the names of invalid fields.
    fn validate_config(&self, _cfg: &Config) -> Vec<String> { Vec::new() }
}
```

`Host` gives the plugin only these capabilities:

| Capability | Rule |
|------------|------|
| HTTP (`osiris_plugin::http::fetch`) | `http`/`https` only; no credentials in the URL; SSRF check on every redirect hop; at most 3 redirects; body truncated at `max_bytes`; explicit timeout; the connection goes to the checked IP |
| DNS (`host.dns`) | Record-type allowlist in the plugin; limited number of answers |
| TCP (`host.safe_connect`) | Resolve → check → connect to the **verified IP** |
| Proxy | Tor (SOCKS5h) and I2P (HTTP) only through the operator's `OSIRIS_TOR_SOCKS` / `OSIRIS_I2P_HTTP_PROXY`; user configuration cannot choose another proxy host |

The result is `CollectionResult { success, items, error, metadata }`; when `success=false` there
are no items. Expected errors return `CollectionResult::fail`, unexpected ones `Err(PluginCrash)`;
both are isolated. In tests the same `Host` interface is backed by a fake host that replays
recorded HTTP/DNS/socket exchanges (`fixtures/plugins/*.json`); the replayer treats every
unrecorded request as an error. Writing a new plugin: [CONTRIBUTING.md](CONTRIBUTING.md). Design
note for WASM plugins: [WASM_PLUGIN_TASARIMI.md](WASM_PLUGIN_TASARIMI.md) (Turkish).

## 7. Security model

| Layer | Measure |
|-------|---------|
| **Egress (SSRF)** | Every plugin URL/host is validated: private (RFC 1918), loopback, link-local/metadata, CGNAT, TEST-NET, multicast, `0.0.0.0`, `::`, `fc00::/7`, `fe80::/10` are blocked; IPv4-mapped IPv6 is unwrapped and checked; the request is rejected if any DNS record is blocked or the host does not resolve (fail-closed); URLs with backslashes or control characters are rejected. `.onion`/`.i2p` only in the respective plugin and through the proxy. |
| **Identity** | Operator key (constant-time comparison); user keys (only the SHA-256 digest is stored, the raw value is shown once); HS256 JWT (fixed algorithm, separate secret, 1 hour). Without a key the API does not start (except `OSIRIS_INSECURE_OPEN=1`). |
| **Authorisation** | `viewer` < `analyst` < `admin`; every endpoint checks its own minimum role; unknown roles are rejected. |
| **Abuse limits** | Per-identity quota per minute (`OSIRIS_API_RATE_PER_MIN`), 10 req/s per IP in Nginx; 5 MiB body cap; field length limits; `config` ≤ 50 keys and ≤ 20 KB; page limits on list endpoints. |
| **Secrets** | Secret fields in a source's `config` are masked at every depth on read; on update the mask keeps the old value; plugins never write secrets to errors, metadata or logs. |
| **Audit** | Append-only, hash-chained `audit_logs` (RFC 8785 canonical JSON, writers serialised by an advisory lock); verified with `audit-verify`. KVKK/GDPR erasures log a SHA-256 digest instead of the value. |
| **Errors** | The response body is always `{"detail": …}`; SQL, driver messages and stack traces never leak (they are logged). |
| **Transport** | Nginx TLS 1.2/1.3, HSTS, CSP (`script-src 'self'`), `X-Frame-Options DENY`, `nosniff`, `Referrer-Policy no-referrer`. Application ports only on `127.0.0.1`; metrics only on the internal network. |
| **Runtime** | Distroless image: no shell or package manager, non-root user (65532); no models or secrets in the image; dependencies checked with `cargo deny`. |
| **Web UI** | Credentials only in the tab (`sessionStorage`); no inline scripts; role restrictions in the UI are a convenience, the real check is on the server. |

## 8. Observability

- **Logs:** `tracing`; JSON with `OSIRIS_LOG_FORMAT=json`. The access log (`osiris_api::access`)
  records method, route template, status and duration; never path values or query strings.
- **Metrics:** Prometheus text on the API's separate listener (request count/duration, quota,
  audit failures, version).
- **Health:** `GET /health` (unauthenticated) and `osiris-server health` (container check); source
  health in `sources` and `source_metrics`.

## 9. Technology choices

| Area | Choice |
|------|--------|
| HTTP server | axum 0.8, tower-http, governor (quota), utoipa (OpenAPI) |
| HTTP client | reqwest (rustls, gzip/deflate) |
| Database | PostgreSQL 16 + pgvector, sqlx |
| Queue | Redis Streams (consumer group) |
| ML | ONNX Runtime (`ort`) — mBERT-hrl NER (int8); fastembed — paraphrase-multilingual-MiniLM-L12-v2 |
| Language detection | lingua |
| UI | React 18, TypeScript, Tailwind, Vite (no chart library; the graph is drawn on canvas) |
| Deployment | Docker Compose, distroless image, Nginx, n8n |

The rationale, decisions and stage log of the Rust migration (Turkish):
[RUST_GECIS_PLANI.md](RUST_GECIS_PLANI.md), [RUST_GECIS_DURUMU.md](RUST_GECIS_DURUMU.md).
