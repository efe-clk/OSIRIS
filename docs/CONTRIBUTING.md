> **Türkçe:** [CONTRIBUTING.tr.md](CONTRIBUTING.tr.md)

# Contributing to OSIRIS

This guide covers setting up a development environment, running the tests, adding a new plugin
and the rules changes are accepted under. Read [ARCHITECTURE.md](ARCHITECTURE.md) first for the
structure of the system.

## 1. Repository layout

| Path | Content |
|------|---------|
| `crates/` | Rust workspace (17 crates; responsibilities in [ARCHITECTURE.md §5](ARCHITECTURE.md#5-crates)) |
| `plugins/<id>/manifest.json` | Plugin manifests (embedded at compile time); `username-search` site DBs |
| `db/migrations/` | SQL schema (embedded at compile time; Docker also applies it on first start) |
| `schemas/` | JSON Schemas: manifest, collected item, collection result, queue message |
| `fixtures/` | Language-neutral golden data (hash, entities, audit, STIX, graph, report, alert, security, NER, embedding, plugin scenarios) |
| `tests/contract/` | Black-box contract tests against a running API (Python, pytest) |
| `tools/` | Generators of the generated tables (bs4, feedparser) |
| `osiris-web-ui/` | React/TypeScript UI |
| `deploy/` | Dockerfile, Nginx, Tor, n8n workflow |
| `docs/` | Documentation (Turkish + English) |

## 2. Development environment

- **Rust:** [rustup](https://rustup.rs). The toolchain and its components (`rustfmt`, `clippy`)
  are pinned by `rust-toolchain.toml` and installed automatically on the first `cargo` call.
- **Docker:** PostgreSQL (pgvector) and Redis for the integration tests.
- **Node.js 20+:** only for the web UI.
- **Python 3.11+:** only for the contract tests and the `tools/` generators.

```bash
cargo build --workspace
cargo run -p osiris-server -- models        # NER + embedding models into ./models (~420 MB, optional)
cargo run -p osiris-cli -- plugins          # osiris CLI: embedded plugins
```

### 2.1 Local test stack

The database name **must** end in `_test`: the integration tests truncate the tables and refuse
to run against any other name.

```bash
docker run -d --name osiris-test-pg -e POSTGRES_USER=osiris -e POSTGRES_PASSWORD=osiris \
  -e POSTGRES_DB=osiris_test -p 127.0.0.1:55432:5432 pgvector/pgvector:pg16
docker run -d --name osiris-test-redis -p 127.0.0.1:56379:6379 redis:7.4-alpine

export OSIRIS_TEST_DATABASE_URL=postgresql://osiris:osiris@127.0.0.1:55432/osiris_test
export OSIRIS_TEST_REDIS_URL=redis://127.0.0.1:56379/1
cargo run -p osiris-server -- --database-url "$OSIRIS_TEST_DATABASE_URL" migrate
```

### 2.2 Running the API locally

```bash
OSIRIS_DATABASE_URL=$OSIRIS_TEST_DATABASE_URL OSIRIS_REDIS_URL=redis://127.0.0.1:56379/0 \
OSIRIS_API_KEY=dev-key OSIRIS_JWT_SECRET=dev-jwt-secret \
OSIRIS_API_RATE_PER_MIN=0 \
  cargo run -p osiris-server -- api --addr 127.0.0.1:8001

# In another terminal, the worker (so that items are written to the database)
OSIRIS_DATABASE_URL=$OSIRIS_TEST_DATABASE_URL OSIRIS_REDIS_URL=redis://127.0.0.1:56379/0 \
  cargo run -p osiris-server -- pipeline --loop
```

### 2.3 Web UI

```bash
cd osiris-web-ui
npm ci
npm run dev               # http://localhost:3000; /api/* → http://127.0.0.1:8001 (prefix stripped)
npm run lint              # ESLint
npm run build             # tsc + vite → dist/
VITE_DEMO=1 npm run dev   # serverless demo mode
```

Details: [osiris-web-ui/README.md](../osiris-web-ui/README.md).

## 3. Tests

| Layer | Command | Needs |
|-------|---------|-------|
| Unit + golden fixtures | `cargo test --workspace` | — |
| Formatting | `cargo fmt --all --check` | — |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | — |
| Dependency audit | `cargo deny check` | `cargo install cargo-deny` |
| DB + queue integration | `cargo test -p osiris-server --test integration` | §2.1 stack, `OSIRIS_TEST_*` |
| Model fixtures | `OSIRIS_REQUIRE_MODELS=1 cargo test --release -p osiris-pipeline --test models` | `./models` |
| API contract | below | a running API |
| Web UI | `npm run lint && npm run build` (`osiris-web-ui/`) | Node.js |
| Compose file | `docker compose config -q` | Docker |

Integration tests are skipped without `OSIRIS_TEST_DATABASE_URL`, model tests without models.
`OSIRIS_REQUIRE_MODELS=1` turns a skipped model test into a failure (enabled in CI).

The **contract suite** sends real HTTP requests to a running API:

```bash
python3 -m venv .venv && .venv/bin/pip install -r tests/contract/requirements.txt
OSIRIS_BASE_URL=http://127.0.0.1:8001 OSIRIS_CONTRACT_API_KEY=dev-key \
  OSIRIS_CONTRACT_DESTRUCTIVE=1 .venv/bin/pytest -q tests/contract
```

| Variable | Meaning |
|----------|---------|
| `OSIRIS_BASE_URL` | API address (`https://…/api` through Nginx works too) |
| `OSIRIS_CONTRACT_API_KEY` | Operator key |
| `OSIRIS_CONTRACT_VERIFY_TLS` | `0` → accept a self-signed certificate |
| `OSIRIS_CONTRACT_DESTRUCTIVE` | `1` → also run tests that make persistent changes (they clean up their own records) |

> Running the suite through Nginx hits the 10 req/s per-IP limit; connect to the API directly
> (`127.0.0.1:8001`) and set `OSIRIS_API_RATE_PER_MIN=0`.

> Do not pipe test output into `grep`/`tail` and chain it to a commit with `&&`: the pipeline's
> exit code hides a test failure. Run tests without a pipe.

### 3.1 Fixtures are a contract

The expected values in `fixtures/` were generated from the original Python reference
implementation during the Rust migration and then frozen. Changing an `expected` value is a
**contract change**: update [SOZLESME.md](SOZLESME.md) (Turkish) and give the reason in the commit
message. The `tests/contract/` suite must stay green after a change; when you add an endpoint or a
behaviour, add a test to the suite as well.

### 3.2 Generated tables

`crates/osiris-hash/src/html/tables.rs` and `crates/osiris-feed/src/feed_tables.rs` are generated
from bs4/feedparser by `tools/gen_html_tables.py` and `tools/gen_feed_tables.py` (versions pinned
in `tools/requirements.txt`). Do not edit them by hand; run the generator. CI runs both generators
with `--check`.

## 4. Adding a new plugin

1. **Manifest** — `plugins/<plugin-id>/manifest.json`:

   ```json
   {
     "id": "example",
     "name": "Example",
     "version": "1.0.0",
     "network_type": "www",
     "author": "…",
     "description": "One sentence on what it collects",
     "config_schema": {
       "url": {"type": "string", "required": true, "description": "Target address"},
       "limit": {"type": "integer", "default": 20}
     },
     "capabilities": ["text"],
     "requires_proxy": false,
     "schedule_default": "0 */6 * * *"
   }
   ```

   The manifest must satisfy `schemas/manifest.schema.json` (required: `id`, `name`, `version`,
   `network_type`, `description`, `config_schema`). Mark required fields with `"required": true`:
   the manager rejects a run with missing required fields without calling the plugin. The order
   of fields in `config_schema` is the form order in the UI.

2. **Code** — `crates/osiris-plugins/src/example.rs`:

   ```rust
   use async_trait::async_trait;
   use osiris_plugin::{Collector, FetchOptions, Host, PluginCrash, fetch};
   use osiris_types::pyval::trunc;
   use osiris_types::{CollectedItem, CollectionResult, Config, Manifest};
   use serde_json::{Map, Value};

   pub struct Example(pub Manifest);

   #[async_trait]
   impl Collector for Example {
       fn manifest(&self) -> &Manifest {
           &self.0
       }

       async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash> {
           let Some(Value::String(url)) = cfg.get("url") else {
               return Ok(CollectionResult::fail("url gerekli"));
           };
           // Every request and redirect passes the SSRF check; size and time are bounded explicitly.
           let opts = FetchOptions::new("GET", url).timeout_secs(30).max_bytes(2_000_000);
           let resp = match fetch(host, opts).await {
               Ok(r) => r,
               Err(e) => return Ok(CollectionResult::fail(trunc(&e.to_string(), 500))),
           };
           if let Err(e) = resp.raise_for_status() {
               return Ok(CollectionResult::fail(trunc(&e.to_string(), 500)));
           }
           let item = CollectedItem::new(trunc(&resp.text(), 50_000));
           Ok(CollectionResult::ok(vec![item], Map::new()))
       }

       async fn health_check(&self, _host: &Host, cfg: &Config) -> bool {
           cfg.get("url").is_some()
       }
   }
   ```

   Then add the module in `crates/osiris-plugins/src/lib.rs` (`pub mod example;`) and register it
   in the `all()` list: `Arc::new(example::Example(manifest!("example")))`. The `manifest!` macro
   embeds `plugins/example/manifest.json` at compile time.

3. **Fixtures** — at least one happy path and one rejection scenario, e.g.
   `fixtures/plugins/example_success.json` and `fixtures/plugins/example_ssrf_rejected.json`. A
   file contains the `config`, recorded `http` exchanges (method, URL, params → status, headers,
   body), `dns` and `sockets` records if any, the expected `expected` result and the
   `expected_network` report. The replayer only answers recorded requests; any other request, and
   any unconsumed record, fails the test. In fixtures DNS resolves every host to `93.184.216.34`.
   See `fixtures/plugins/rss_basari.json` and `rss_ssrf_reddi.json` for examples.

4. **Verification** — `cargo test -p osiris-plugins` and `cargo run -p osiris-cli -- plugins`; then
   add the plugin's section to [PLUGINS.md](PLUGINS.md) and [PLUGINS.tr.md](PLUGINS.tr.md).

### 4.1 Security checklist

- Use only host capabilities: `fetch` for HTTP (the default `UrlPolicy` blocks private, loopback,
  link-local and metadata addresses), `host.dns` for DNS, `host.safe_connect` for raw TCP. Never
  open sockets directly or build your own HTTP client.
- `.onion`/`.i2p` only through the operator's proxies: `osiris_plugin::proxy::resolve_proxy`
  (`OSIRIS_TOR_SOCKS` / `OSIRIS_I2P_HTTP_PROXY`). User configuration cannot choose the proxy host.
- Validate host names, channels and tokens with `osiris_security::sanitize_*`.
- Never write secrets (API keys, tokens) to error messages, metadata or logs. Give secret-bearing
  `config` fields names such as `api_key`, `token`, `password`, `secret`: the API masks them in the
  source detail.
- Bound response sizes, item counts and string lengths; set timeouts explicitly.
- Use `CollectionResult::fail` for expected errors; `Err(PluginCrash)` only for truly unexpected
  input. The manager isolates both.

## 5. Code standards

- Before pushing, these must be clean: `cargo fmt --all --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny check`; and if the UI
  changed, `npm run lint` and `npm run build`.
- `unsafe` is forbidden in the workspace. Return errors instead of panicking in service code.
- API error bodies are `{"detail": …}` and contain no internal details (SQL, driver messages):
  use `ApiError::internal("generic message", cause)` for internal errors.
- New behaviour needs a test; behaviour shared with other components needs a fixture; a new API
  endpoint needs a contract test.
- Remaining Python (`tests/contract`, `tools/`, `plugins/username-search/tools`): `ruff check`
  must be clean (`ruff.toml`).
- Code comments and user-facing messages are in Turkish; follow the existing style of the file.

## 6. Documentation

- User-facing documents exist in two languages: `README.md` / `README.tr.md` and the
  `X.md` / `X.tr.md` pairs under `docs/`. `SOZLESME.md`, the Rust migration logs and the WASM note
  are Turkish only. A commit that changes behaviour updates both languages.
- New API endpoint → [API.md](API.md) + [API.tr.md](API.tr.md); new environment variable →
  [TECHNICAL_DOCUMENT.md §6](TECHNICAL_DOCUMENT.md#6-configuration-reference) + its Turkish
  counterpart and `.env.example`; new plugin → [PLUGINS.md](PLUGINS.md) + its Turkish counterpart.
- CI checks relative links in the documentation; a broken link fails the job.

## 7. Commits and CI

- Commit messages are short, descriptive, in Turkish and prefixed with the area: `API: …`,
  `Web UI: …`, `Plugin rss: …`, `Doküman: …`, `CI: …`.
- One logical change per commit. Never commit secrets (`.env`, `*.key`, `*.crt` are in
  `.gitignore`).
- Working directly on `master` is the default for small changes; use a branch + PR for risky ones.

CI (`.github/workflows/ci.yml`) runs these jobs on every push:

| Job | What it checks |
|-----|----------------|
| `rust` | `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace` |
| `integration` | Migrations with PostgreSQL + Redis (psql and `migrate`, twice each), model download, integration + model tests, the contract suite against a real API |
| `docker` | Building the distroless image and a smoke test |
| `cargo-deny` | Licences, security advisories, banned dependencies |
| `tools-and-docs` | `ruff`, freshness of the generated tables, documentation links, `pip-audit` |
| `web-ui-build` | ESLint, build, demo build, demo code absent from the production bundle, `npm audit` |
