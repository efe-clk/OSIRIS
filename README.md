> **Türkçe:** [README.tr.md](README.tr.md)

# OSIRIS

> **O**pen **S**ource **I**ntelligence **R**esearch & **I**nformation **S**ystem

OSIRIS is a self-hosted open source intelligence (OSINT) platform. It collects data from the
surface web, RSS/Atom, REST APIs, Shodan, DNS/WHOIS, blockchains, IRC, Matrix, Tor and I2P; cleans
the text, extracts entities (e-mail, IP, domain, person, organisation…), turns it into semantic
vectors and makes everything searchable. No data leaves for a third-party cloud.

Version **1.0.0-rc**. The server is written in Rust (one binary, one distroless Docker image); the
browser UI is React/TypeScript. Licence GPL-3.0. The UI, log messages and API error messages are
in Turkish; the documentation is available in English and Turkish.

## What does it do?

| Capability | Details |
|------------|---------|
| **Collection** | 11 plugins: `rss`, `web-scraper`, `rest-api`, `shodan`, `dns-whois`, `blockchain`, `username-search` (~2 500 sites), `tor`, `i2p`, `irc`, `matrix`. One-off or cron-scheduled; per-source health tracking. |
| **Processing** | HTML cleaning and content-digest deduplication, language detection in 16 languages, regex entities + ONNX NER (person/organisation/location), topic tags, 384-dimensional multilingual vectors. |
| **Search** | Full-text (PostgreSQL), semantic (pgvector; a Turkish query can find English text), entity-based. |
| **Analysis** | Relationship graph: centrality, community detection, GraphML/GEXF export. |
| **Reports** | Markdown, HTML, JSON, STIX 2.1. |
| **Security** | TLS, roles (viewer/analyst/admin), user keys + JWT, per-identity quota, SSRF checks on every outbound request, hash-chained audit log. |
| **Privacy (KVKK/GDPR)** | Per-source retention, per-item and per-person erasure; the erased value is logged only as a digest. |
| **UI and automation** | 11-screen web UI, REST API (OpenAPI), `osiris` CLI, n8n workflow template, Prometheus metrics. |

## Architecture

```
 Browser (Web UI) ──┐      n8n / CLI ─┐
                    ▼                 ▼
              Nginx (TLS :8443; /ui, /api, /n8n)
                    │
     ┌──────────────▼──────────────┐         ┌─────────────────────────┐
     │ osiris-server api           │         │ osiris-server collector │
     │ identity · RBAC · quota     │         │ cron scheduler          │
     └──────┬───────────────┬──────┘         └────────────┬────────────┘
            │ SQL           │ XADD                         │ XADD
            ▼               ▼                              ▼
     PostgreSQL 16     Redis Stream ──XREADGROUP──▶ osiris-server pipeline
     + pgvector        (queue)                      clean · entities · vector · store
```

One image (`osiris:1.0.0-rc`) runs every role through a subcommand. Details:
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Quick start

Requirements: Docker Engine + Compose v2, at least 4 GB of RAM (8 GB recommended with models).

```bash
git clone https://github.com/efe-clk/OSIRIS.git && cd OSIRIS

cp .env.example .env && chmod 600 .env
#   POSTGRES_PASSWORD, OSIRIS_API_KEY, OSIRIS_JWT_SECRET, N8N_PASSWORD
#   → a separate value for each: openssl rand -hex 32

./deploy/nginx/make-certs.sh                                     # self-signed TLS certificate
docker compose --profile models run --rm --build osiris-models   # NER + embedding (~420 MB, optional)
docker compose --profile full up -d --build                      # all services

curl -sk https://localhost:8443/api/health                       # {"service":"osiris-api","status":"ok"}
```

Then open `https://<server>:8443/` in a browser and log in with `OSIRIS_API_KEY`. Add
`--profile darknet` for Tor/I2P. Scheduled collection stays off until `OSIRIS_SCHEDULER=on`.

Using the API directly:

```bash
curl -sk -X POST https://localhost:8443/api/collect \
  -H "X-API-Key: <OSIRIS_API_KEY>" -H 'Content-Type: application/json' \
  -d '{"plugin_id":"rss","config":{"feed_url":"https://example.com/feed.xml"}}'

curl -sk -X POST https://localhost:8443/api/search \
  -H "X-API-Key: <OSIRIS_API_KEY>" -H 'Content-Type: application/json' \
  -d '{"query":"ransomware","limit":10}'
```

Step-by-step installation, configuration, backup and upgrades:
[docs/TECHNICAL_DOCUMENT.md](docs/TECHNICAL_DOCUMENT.md).

## Documentation

| Document | Content |
|----------|---------|
| [Installation and operations guide](docs/TECHNICAL_DOCUMENT.md) | Requirements, installation, every environment variable, services, identity/roles, collection, privacy, backup, upgrades, troubleshooting |
| [API reference](docs/API.md) | Every endpoint, request/response examples, limits, error codes |
| [Plugin reference](docs/PLUGINS.md) | Fields, outputs and limits of the 11 plugins |
| [Architecture](docs/ARCHITECTURE.md) | Processes, data flow, data model, crates, security model |
| [Contributing](docs/CONTRIBUTING.md) | Development environment, tests, new plugins, code and commit rules |
| [Behaviour contract](docs/SOZLESME.md) (Turkish) | Rules that must not change (digest, audit, queue, SSRF, reports) |
| [Web UI](osiris-web-ui/README.md) (Turkish, English summary) | Screens, development, demo mode |
| [Models](models/README.md) (Turkish, English summary) | NER and embedding models, licences |
| [Rust migration log](docs/RUST_GECIS_DURUMU.md) (Turkish) | Stage-by-stage log of the Python → Rust migration |

## Development

```bash
cargo test --workspace                                   # unit + golden fixture tests
cargo clippy --workspace --all-targets -- -D warnings
cd osiris-web-ui && npm ci && npm run lint && npm run build
```

Integration, model and API contract tests: [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md).

## Licence

GNU General Public License v3.0 — see [LICENSE](LICENSE). The NER model (AFL-3.0) is not
distributed with the repository; users download it themselves (see [models/README.md](models/README.md)).
