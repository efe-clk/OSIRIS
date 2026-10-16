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
│                    │   Core Engine     │  (C++ / Python)        │
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
| `osiris-core` | C++20 | Central coordinator, task scheduling, IPC | Skeleton (scheduler + plugin lifecycle) |
| `osiris-collector` | Python | Manages collection plugins | ✅ Working |
| `osiris-pipeline` | Python | Cleaning, NER, classification, embedding, storage | ✅ Working (embedding optional) |
| `osiris-query` | Python | Full-text + semantic search | ✅ Working |
| `osiris-graph` | Python | Entity relationship graph (persistent) | ✅ Working |
| `osiris-alert` | Python | Alerts + anomaly detection | ✅ Working |
| `osiris-report` | Python | Report generation and export | ✅ MD/HTML/JSON/CSV/STIX |
| `osiris-api` | Python | REST API (JWT + RBAC) | ✅ Working |
| `osiris-cli` | Python | Command-line interface | ✅ Working |
| `osiris-web-ui` | React/TS | Browser interface | Basic screens |
| `osiris-sdk` | Python | Shared SDK (plugins, models, security, audit) | ✅ Working |

## Technology Stack

- **Languages:** C++20, Python 3.11+, TypeScript
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
