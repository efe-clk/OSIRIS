> **Türkçe:** Bu belgenin Türkçe sürümü için [README.tr.md](README.tr.md) dosyasına bakın.

# OSIRIS

> **O**pen **S**ource **I**ntelligence **R**esearch & **I**nformation **S**ystem

OSIRIS is a self-hosted open-source intelligence (OSINT) platform: it collects data from multiple networks (surface web, Tor, I2P, IRC, Matrix, RSS, APIs, blockchains), extracts and correlates entities, and makes everything searchable — on your own server, with no third-party cloud dependency.

Version **1.0.0-rc**. The application is written in **Rust** (single binary, single Docker image); the browser interface is React/TypeScript.

## Core Principles

- **Privacy first** — darknet traffic goes only through the operator's Tor/I2P proxies; every fetch passes an SSRF guard
- **Modularity** — every data source is a plugin behind one `Collector` trait
- **Transparency** — open source; every write is recorded in a hash-chained audit log
- **Contracts** — behaviour is pinned by golden fixtures and a black-box API contract suite

## Architecture Overview

```
 Browser (React UI) ─┐        n8n ─┐
                     ▼             ▼
                 Nginx (TLS 8443, /api/* → osiris-api)
                     │
        ┌────────────▼─────────────┐        ┌────────────────────────┐
        │ osiris-server api        │        │ osiris-server collector│
        │ REST · JWT/RBAC · quota  │        │ cron · 11 plugins      │
        └──────┬───────────┬───────┘        └──────────┬─────────────┘
               │ SQL       │ XADD                      │ XADD
               ▼           ▼                           ▼
        PostgreSQL 16   Redis Streams  ──XREADGROUP──▶ osiris-server pipeline
        (+ pgvector)    osiris:stream:raw_items        clean · regex+NER · embed · store
```

One image (`osiris:1.0.0-rc`) runs every role; the role is a subcommand. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for layers, data flow and the security model.

## Components

| Path | Role |
|------|------|
| `crates/osiris-server` | Service binary: `api`, `pipeline`, `collector`, `migrate`, `models`, `audit-verify`, `purge`, `health` |
| `crates/osiris-cli` | `osiris` CLI: `status`, `plugins`, `collect` (exit codes 0/1/2) |
| `crates/osiris-plugins`, `osiris-plugin` | The 11 collectors and the host capabilities they may use (guarded HTTP, DNS, TCP) |
| `crates/osiris-collector`, `osiris-queue` | Collection runs, source health, cron scheduler, queue producer |
| `crates/osiris-pipeline` | Queue consumer: cleaning, language, regex + ONNX NER, classification, fastembed vectors |
| `crates/osiris-db`, `osiris-query` | PostgreSQL access, migrations, audit chain, retention; full-text / semantic / entity search |
| `crates/osiris-graph`, `osiris-alert`, `osiris-report` | Entity graph (centrality, CNM communities), alerts + anomalies, Markdown/HTML/JSON/CSV/STIX reports |
| `crates/osiris-hash`, `osiris-extract`, `osiris-security`, `osiris-feed`, `osiris-types` | `content_hash`, canonical JSON, entity regexes, SSRF guard + sanitizers, RSS/Atom parsing, shared types |
| `osiris-web-ui` | Browser interface (dashboard, search, sources, graph, alerts, reports) |
| `db/migrations` | SQL schema (idempotent; embedded in the binary) |
| `plugins/` | Plugin manifests and the username-search site databases (embedded at build time) |

## Technology Stack

- **Language:** Rust (pinned in `rust-toolchain.toml`) — axum, sqlx, redis, ort (ONNX Runtime), fastembed, utoipa; TypeScript + React for the UI
- **Data:** PostgreSQL 16 + pgvector (TimescaleDB optional), Redis Streams
- **Security:** TLS (8443), operator key / DB keys (SHA-256) / JWT HS256, RBAC, per-identity quotas, SSRF guards, hash-chained audit log (RFC 8785)
- **Operations:** Docker Compose, distroless image, Prometheus `/metrics`, JSON logs, n8n

## Quick Start

```bash
cp .env.example .env              # set POSTGRES_PASSWORD, OSIRIS_API_KEY, OSIRIS_JWT_SECRET
                                  # (openssl rand -hex 32 for each secret)
./deploy/nginx/make-certs.sh      # self-signed TLS certificate (fresh installs)

# Optional but recommended: NER + embedding models (~420 MB, hash-verified)
docker compose --profile models run --rm --build osiris-models

# Everything: PostgreSQL, Redis, n8n, Nginx, API, worker, collector, Web UI
docker compose --profile full up -d --build

curl -sk https://localhost:8443/api/health        # {"service":"osiris-api","status":"ok"}
docker compose exec osiris-api osiris plugins     # CLI inside the container
```

The database schema is created on the first start of an empty PostgreSQL volume. When upgrading an existing installation, apply new migrations with `docker compose run --rm osiris-api migrate` (idempotent). Scheduled collection is **off** until you set `OSIRIS_SCHEDULER=on`. Step-by-step installation, configuration and operations: [docs/TECHNICAL_DOCUMENT.md](docs/TECHNICAL_DOCUMENT.md).

### API authentication

```bash
# Short-lived JWT for the operator key (1 h)
curl -sk -X POST https://localhost:8443/api/auth/token \
  -H 'Content-Type: application/json' \
  -d '{"api_key":"<OSIRIS_API_KEY>","role":"analyst"}'

curl -sk -X POST https://localhost:8443/api/search \
  -H 'Content-Type: application/json' -H 'Authorization: Bearer <token>' \
  -d '{"query":"example","limit":5}'
```

Roles: `viewer` (read) → `analyst` (collect, sources, graph) → `admin` (keys, deletion, retention). Reference: [docs/API.md](docs/API.md); interactive docs at `/api/docs`.

## Development

```bash
cargo test --workspace                     # unit + golden fixture tests
cargo clippy --workspace --all-targets -- -D warnings
```

Database/queue integration tests, model fixtures and the API contract suite are described in [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md).

## Documentation

- Technical document (install, configuration, operations): [docs/TECHNICAL_DOCUMENT.md](docs/TECHNICAL_DOCUMENT.md)
- API reference: [docs/API.md](docs/API.md) ([Türkçe](docs/API.tr.md))
- Architecture: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- Contributing, plugin guide: [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md)
- Behaviour contract (Turkish): [docs/SOZLESME.md](docs/SOZLESME.md); Rust migration record: [docs/RUST_GECIS_DURUMU.md](docs/RUST_GECIS_DURUMU.md)

## License

GNU General Public License v3.0 — see [LICENSE](LICENSE)
