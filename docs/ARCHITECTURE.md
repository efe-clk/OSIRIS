> **Türkçe:** Bu belgenin Türkçe sürümü için [ARCHITECTURE.tr.md](ARCHITECTURE.tr.md) dosyasına bakın.

# OSIRIS Architecture

This document summarizes the high-level architecture of the OSIRIS platform and module responsibilities. For the full technical specification, see [TECHNICAL_DOCUMENT.md](TECHNICAL_DOCUMENT.md).

## Layers

```
┌────────────────────────────────────────────────────────────┐
│ Interface Layer                                            │
│  CLI · Web UI · REST API · (Qt desktop planned)            │
└──────────────────────────┬─────────────────────────────────┘
                            │
┌──────────────────────────▼─────────────────────────────────┐
│ Core Layer (osiris-core, C++)                              │
│  Coordination · Scheduling · Plugin lifecycle              │
└──────────────────────────┬─────────────────────────────────┘
                            │
┌──────────────────────────▼─────────────────────────────────┐
│ Data Layer                                                 │
│  Collector Manager → Plugins → Pipeline → Storage          │
└──────────────────────────┬─────────────────────────────────┘
                            │
┌──────────────────────────▼─────────────────────────────────┐
│ Analysis Layer                                             │
│  Query · Graph (persistent) · Alert (+anomaly) · Report    │
└────────────────────────────────────────────────────────────┘
```

## Data Flow

1. **Collect** — Collector Manager runs scheduled plugins (full 5-field cron).
2. **Queue** — raw data goes to the Redis queue (`osiris:raw_items`, 512 KB item cap).
3. **Process** — pipeline cleans, detects language, extracts entities, classifies, embeds (optional model), deduplicates by content hash.
4. **Store** — processed data is written to PostgreSQL (`items`, `entities`, `item_entities`, optional `embedding` vector).
5. **Analyze** — Query/Graph/Alert/Report layers consume the data; graph edges persist in `graph_edges`.
6. **Present** — interfaces show results via the REST API (JWT + RBAC).

## Module Responsibilities

| Module | Responsibility | Notes |
|--------|---------------|-------|
| `osiris-core` | Central coordinator, plugin lifecycle, scheduling | Atomic stop flag; REST runs in `osiris-api` |
| `osiris-collector` | Plugin loading, task execution, queue delivery | Crash isolation, full cron, source health tracking |
| `osiris-pipeline` | Cleaning, NER, classification, embedding, storage | Regex + ONNX NER (mBERT-hrl), keyword topics, fastembed embeddings (384-d); Redis Streams consumer |
| `osiris-query` | Full-text + semantic + entity search | `dict_row`, validated limits, LIKE-escape |
| `osiris-graph` | Entity graph, centrality, communities | In-memory + `graph_edges` persistence |
| `osiris-alert` | Saved-query matching, anomaly detection | Substring match, mute rules, z-score baselines |
| `osiris-report` | Markdown/HTML/JSON/CSV/STIX 2.1 reports | Jinja autoescape, CSV-injection guard, traversal lock |
| `osiris-api` | REST API server | CORS, validation, API key + JWT + RBAC, audit hooks |
| `osiris-cli` | Command-line interface | `status`, `plugins`, `collect` with clean exit codes |
| `osiris-web-ui` | Browser interface | Dashboard / Sources / Search (basic) |
| `osiris-sdk` | `plugin`, `models`, `security` (SSRF guards), `audit` (hash chain) | Shared by all Python modules |

## Security Model

- **Egress control:** every plugin URL/host is validated (`osiris.security`); private/loopback/link-local/metadata IPs, credentials in URLs, and non-http(s) schemes are rejected (fail-closed, incl. unresolvable hosts). Tor/I2P proxies are localhost-only.
- **Authentication:** operator `X-API-Key`, per-user API keys (SHA-256 hashes only in DB), short-lived HS256 JWTs (stdlib-only, `alg` pinned, constant-time verify).
- **Authorization:** roles `viewer` → `analyst` → `admin`; unknown roles denied.
- **Audit:** append-only hash-chained `audit_logs` (`prev_hash` + advisory-locked writers, `verify_chain` checker); API collects/searches are logged best-effort.
- **Transport:** Nginx terminates TLS on `:8443` (TLS 1.2/1.3, HSTS, CSP, rate limits); `:80` kept for LAN/bootstrap.
- **Secrets:** `.env` (git-ignored, mode 600); DB passwords rotatable; no secret is ever logged or returned by APIs.
