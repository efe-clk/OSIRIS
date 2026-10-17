> **Türkçe:** Bu belgenin Türkçe sürümü için [ARCHITECTURE.tr.md](ARCHITECTURE.tr.md) dosyasına bakın.

# OSIRIS Architecture

This document summarizes the architecture of OSIRIS 1.0 (Rust) and the responsibility of each crate. Installation and operations: [TECHNICAL_DOCUMENT.md](TECHNICAL_DOCUMENT.md). Behaviour contracts (hash, audit, queue, API, …): [SOZLESME.md](SOZLESME.md).

## Processes

One binary, `osiris-server`, shipped in one distroless image (`osiris:1.0.0-rc`). Compose runs it three times with different subcommands:

| Service | Command | Does |
|---------|---------|------|
| `osiris-api` | `api` | REST API (axum), auth/RBAC, quotas, OpenAPI, `/metrics` on a separate internal port |
| `osiris-pipeline` | `pipeline --loop` | Consumes the Redis Stream, processes and stores items, periodic retention purge |
| `osiris-collector` | `collector` | Runs sources on their cron schedule (off unless `OSIRIS_SCHEDULER=on`) |

One-shot subcommands: `migrate` (embedded idempotent SQL migrations), `models` (download + verify ONNX models), `audit-verify` (check the audit hash chain), `purge` (retention now), `collect` (single run), `health` (container healthcheck). The `osiris` CLI (`status`, `plugins`, `collect`) is in the same image.

## Data Flow

1. **Collect** — the collector (scheduled) or the API (`/collect`, `/sources/{id}/collect`) runs a plugin. Plugins never open sockets themselves: HTTP, DNS and TCP go through the host's guarded capabilities (SSRF check on every hop, redirect/size/time limits, Tor/I2P only via the operator's proxies). Source health is written to `sources` / `source_metrics`.
2. **Queue** — each collected item is a JSON message on the Redis Stream `osiris:stream:raw_items` (`XADD MAXLEN ~`, ≤ 512 KB). Delivery is at-least-once: consumer group `osiris-pipeline`, `XAUTOCLAIM` after 5 min idle, dead-letter stream after 5 deliveries or for malformed messages.
3. **Process** — clean (`content_hash` normalisation), detect language (lingua), extract entities (regex + ONNX NER, mBERT-hrl), classify topics, embed (fastembed, 384-d, L2-normalised).
4. **Store** — one transaction: `items` (deduplicated by `content_hash`), `entities`, `item_entities`; vectors in `items.embedding_v2`.
5. **Analyse** — full-text / semantic / entity search, entity graph (`graph_edges`), saved-query alerts, reports.
6. **Present** — Web UI and n8n call the REST API through Nginx (`/api/*`).

## Crates

| Crate | Responsibility |
|-------|----------------|
| `osiris-server` | Binary + `api` module (routes, auth, input validation, metrics, OpenAPI) |
| `osiris-cli` | `osiris` CLI |
| `osiris-collector` | Collection runs, task config from sources, queue writes, health records, cron scheduler |
| `osiris-queue` | Queue producer (`XADD`) — shared by collector, API and CLI without pulling in the ML runtime |
| `osiris-pipeline` | Stream consumer, processing steps, NER (`ort`), embeddings (`fastembed`), model downloader |
| `osiris-plugin` | `Collector` trait, host capabilities (guarded HTTP/DNS/TCP, proxies), fixture replay transport |
| `osiris-plugins` | The 11 plugins: rss, web-scraper, dns-whois, rest-api, shodan, blockchain, username-search, tor, i2p, matrix, irc |
| `osiris-feed` | RSS/Atom parsing and sanitising (feedparser-compatible) |
| `osiris-db` | PostgreSQL access (sqlx), embedded migrations, audit append/verify, retention, graph edges |
| `osiris-query` | Full-text (tsvector), semantic (pgvector cosine), entity search |
| `osiris-graph` | Undirected entity graph, degree centrality, Clauset–Newman–Moore communities, GraphML/GEXF |
| `osiris-alert` | Saved-query matching, mute, z-score anomalies, Redis publish, webhook / Telegram / e-mail handlers |
| `osiris-report` | Markdown/HTML (escaped), JSON, CSV (formula guard, directory lock), STIX 2.1 |
| `osiris-hash` | `content_hash` cleaning, HTML text extraction, canonical JSON (RFC 8785), audit chain hash |
| `osiris-extract` | Regex entity extraction (email, IP, domain, phone, crypto address, CVE) |
| `osiris-security` | SSRF guard (URL policy, DNS checks, IPv4-mapped IPv6) and input sanitizers |
| `osiris-types` | Shared types: manifest, collected item, collection result, queue message (JSON Schemas in `schemas/`) |

Plugin manifests (`plugins/*/manifest.json`) and the username-search site databases are embedded at compile time. A WASM plugin interface is a design note only: [WASM_PLUGIN_TASARIMI.md](WASM_PLUGIN_TASARIMI.md).

## Security Model

- **Egress control:** every plugin URL/host is validated (`osiris-security`); private/loopback/link-local/metadata addresses, credentials in URLs, backslashes/control characters and non-http(s) schemes are rejected — fail-closed, including unresolvable hosts. The connection goes to the IP that was checked. `.onion` / `.i2p` only through the configured proxies.
- **Authentication:** operator `X-API-Key` (constant-time compare), per-user API keys (only SHA-256 hashes stored), short-lived HS256 JWTs (`alg` pinned, separate secret, no API-key fallback). The API refuses to start without a key unless `OSIRIS_INSECURE_OPEN=1`.
- **Authorization:** roles `viewer` → `analyst` → `admin`; unknown roles are denied.
- **Abuse limits:** per-identity quota (`OSIRIS_API_RATE_PER_MIN`), body cap 5 MiB, field length caps, config ≤ 50 keys / 20 KB, plus Nginx per-IP rate limits.
- **Audit:** append-only hash-chained `audit_logs` (RFC 8785 payload, advisory-locked writers); `osiris-server audit-verify` checks the chain. Personal data erased under KVKK is logged only as a SHA-256 digest.
- **Errors:** response bodies are always `{"detail": …}` without internals (SQL, driver messages, stack traces).
- **Transport:** Nginx terminates TLS on `:8443` (TLS 1.2/1.3, HSTS, CSP); `:80` kept for LAN/bootstrap. Metrics listen on an internal port only.
- **Runtime:** distroless image, non-root user, no shell; models and secrets are never baked into the image.
