> **Türkçe:** Bu belgenin Türkçe sürümü için [README.tr.md](README.tr.md) dosyasına bakın.

# OSIRIS

> **O**pen **S**ource **I**ntelligence **R**esearch & **I**nformation **S**ystem

OSIRIS is an advanced open-source intelligence (OSINT) platform that collects data from multiple networks (surface web, darknets, P2P, encrypted networks), correlates it, and analyzes it — running fully self-hosted.

The system is designed to run 24/7 on a VDS (Virtual Dedicated Server). All components are under user control; no data leaks out, no third-party cloud dependencies.

## Core Principles

- **Privacy first** — all connections can go through proxy/VPN/Tor
- **Modularity** — every data source is an independent plugin; added/removed without stopping the system
- **Extensibility** — new network types, protocols, and analysis methods integrate easily
- **Transparency** — open source, every operation logged, auditable

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        OSIRIS PLATFORM                          │
│                                                                 │
│  ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   │
│  │  Qt GUI  │   │ Tauri/   │   │   CLI    │   │  REST    │   │
│  │(planned) │   │Electron  │   │          │   │  API     │   │
│  └────┬─────┘   └────┬─────┘   └────┬─────┘   └────┬─────┘   │
│       └──────────────┴──────────────┴──────────────┘          │
│                              │                                  │
│                    ┌─────────▼─────────┐                       │
│                    │   API Gateway /   │                        │
│                    │   Core Engine     │  (Python + Rust)       │
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
│    │              Plugin Collection                       │      │
│    │  [WWW] [Tor] [I2P] [RSS] [API] [IRC] [Shodan] ...  │      │
│    └─────────────────────────────────────────────────────┘      │
└─────────────────────────────────────────────────────────────────┘
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for layers, data flow, and the security model.

## Modules

| Module | Language | Role | Status |
|--------|----------|------|--------|
| `crates/` | Rust | Shared core: SSRF guard, `content_hash` + audit chain, regex entity extraction — used by the Python services through `osiris_native` (PyO3) | ✅ R4 (Rust migration in progress — [plan](docs/RUST_GECIS_PLANI.md)) |
| `osiris-collector` | Python | Manages collection plugins | ✅ Working |
| `osiris-server` | Rust | REST API (`api`, JWT + RBAC + quotas), queue worker, collector scheduler and the 11 plugins | ✅ R6 API (`api` profile), R5 worker (`worker` profile) |
| `osiris-pipeline` | Python | Cleaning, NER, classification, embedding, storage | ✅ Fallback worker (`worker-python` profile) |
| `osiris-query` | Python → Rust | Full-text + semantic search (`crates/osiris-query`) | ✅ R6 Rust |
| `osiris-graph` | Python → Rust | Entity relationship graph, centrality, CNM communities (`crates/osiris-graph`) | ✅ R6 Rust |
| `osiris-alert` | Python → Rust | Alerts + anomaly detection, webhook/Telegram/e-mail (`crates/osiris-alert`) | ✅ R6 Rust |
| `osiris-report` | Python → Rust | Report generation: MD/HTML/JSON/CSV/STIX (`crates/osiris-report`) | ✅ R6 Rust |
| `osiris-api` | Python | REST API (JWT + RBAC) | ✅ Fallback (`api-python` profile; replaced by `osiris-server api`) |
| `osiris-cli` | Python → Rust | Command-line interface — `osiris` is now Rust (`crates/osiris-cli`), Python version is `osiris-py` | ✅ R6 Rust |
| `osiris-web-ui` | React/TS | Browser interface | Basic screens |
| `osiris-sdk` | Python | Shared SDK (plugins, models, security, audit) | ✅ Working |

## Technology Stack

- **Languages:** Python 3.11+, Rust (stable, pinned in `rust-toolchain.toml`), TypeScript
- **Database:** PostgreSQL 16 (+ pgvector, TimescaleDB optional), Redis
- **Interface:** CLI, REST API, React + Tailwind Web UI
- **Automation:** N8N, APScheduler
- **Security:** TLS (8443), API keys + JWT + RBAC, SSRF guards, hash-chained audit logs
- **Deployment:** Docker, Docker Compose, Nginx

## Quick Start

```bash
# Copy environment template and fill in secrets
cp .env.example .env

# Generate the TLS bootstrap certificate (fresh installs only)
./deploy/nginx/make-certs.sh

# Start infrastructure (PostgreSQL, Redis, N8N, Nginx)
docker compose up -d

# Start API + Web UI as well
docker compose --profile full up -d --build

# Install the CLI
pip install -e ./osiris-cli

# CLI usage
osiris --help
```

### API authentication

```bash
# Issue a token (operator key from .env)
curl -X POST http://localhost:80/api/auth/token \
  -H 'Content-Type: application/json' \
  -d '{"api_key":"<OSIRIS_API_KEY>","role":"analyst"}'

# Use it
curl -X POST http://localhost:80/api/search \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer <token>' \
  -d '{"query":"example","limit":5}'
```

Roles: `viewer` (read) → `analyst` (collect + graph write) → `admin` (key management).

## Testing

```bash
pip install -e ./osiris-sdk -e ./osiris-collector -e ./osiris-pipeline \
  -e ./osiris-graph -e ./osiris-alert -e ./osiris-report \
  -e ./osiris-query -e ./osiris-api -e ./osiris-cli
pip install pytest pytest-cov httpx
pytest -q  # 80% coverage gate enforced
```

## Documentation

- Full technical document: [docs/TECHNICAL_DOCUMENT.md](docs/TECHNICAL_DOCUMENT.md)
- API reference: [docs/API.md](docs/API.md) ([Türkçe](docs/API.tr.md))
- Architecture summary: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- Contributing: [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md)

## License

GNU General Public License v3.0 — see [LICENSE](LICENSE)
