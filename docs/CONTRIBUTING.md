> **Türkçe:** Bu belgenin Türkçe sürümü için [CONTRIBUTING.tr.md](CONTRIBUTING.tr.md) dosyasına bakın.

# Contributing to OSIRIS

Thanks for contributing. These rules keep the codebase consistent and sustainable.

## Development Environment

- **Rust:** install [rustup](https://rustup.rs); the toolchain (with `rustfmt` and `clippy`) is pinned in `rust-toolchain.toml` and installed automatically on the first `cargo` call.
- **Services for integration tests:** Docker (PostgreSQL + pgvector, Redis).
- **Python (optional):** only for the black-box API contract suite (`tests/contract`) and the code-generation tools (`tools/`).

```bash
cargo build --workspace
cargo run -p osiris-server -- models          # NER + embedding models into ./models (~420 MB)
cargo run -p osiris-cli -- plugins            # the `osiris` CLI
```

## Tests

| Layer | Command | Needs |
|-------|---------|-------|
| Unit + golden fixtures | `cargo test --workspace` | — |
| Model fixtures (NER, embedding) | `OSIRIS_REQUIRE_MODELS=1 cargo test --release -p osiris-pipeline --test models` | `./models` |
| DB + queue integration | `cargo test -p osiris-server --test integration` with `OSIRIS_TEST_DATABASE_URL`, `OSIRIS_TEST_REDIS_URL` | PostgreSQL, Redis |
| API contract (black box) | `pytest -q tests/contract` with `OSIRIS_BASE_URL`, `OSIRIS_CONTRACT_API_KEY` | a running API |

Local services for the integration tests (the database name **must** end with `_test` — tables are truncated):

```bash
docker run -d --name osiris-test-pg -e POSTGRES_USER=osiris -e POSTGRES_PASSWORD=osiris \
  -e POSTGRES_DB=osiris_test -p 127.0.0.1:55432:5432 pgvector/pgvector:pg16
docker run -d --name osiris-test-redis -p 127.0.0.1:56379:6379 redis:7.4-alpine
export OSIRIS_TEST_DATABASE_URL=postgresql://osiris:osiris@127.0.0.1:55432/osiris_test
export OSIRIS_TEST_REDIS_URL=redis://127.0.0.1:56379/1
cargo run -p osiris-server -- --database-url "$OSIRIS_TEST_DATABASE_URL" migrate
cargo test -p osiris-server --test integration
```

Contract suite against a local API:

```bash
OSIRIS_API_KEY=dev-key OSIRIS_JWT_SECRET=dev-jwt OSIRIS_DATABASE_URL=$OSIRIS_TEST_DATABASE_URL \
  cargo run -p osiris-server -- api --addr 127.0.0.1:8000 &
python3 -m venv .venv && .venv/bin/pip install -r tests/contract/requirements.txt
OSIRIS_BASE_URL=http://127.0.0.1:8000 OSIRIS_CONTRACT_API_KEY=dev-key \
  OSIRIS_CONTRACT_DESTRUCTIVE=1 .venv/bin/pytest -q tests/contract
```

### Fixtures are a contract

`fixtures/` holds golden data shared by the tests (hash, extraction, audit, STIX, graph, report, alert, security, NER, embedding, and one replay file per plugin scenario). The expected values were produced by the original Python reference implementation and are frozen. Changing an `expected` value is a **contract change**: update [SOZLESME.md](SOZLESME.md) and explain it in the commit. `tests/contract/` must stay green against the API without modification.

### Generated tables

`crates/osiris-hash/src/html/tables.rs` and `crates/osiris-feed/src/feed_tables.rs` are generated from bs4 / feedparser by `tools/gen_*.py` (pinned in `tools/requirements.txt`); never edit them by hand. CI runs both generators with `--check`.

## Adding a New Plugin

1. **Manifest** — `plugins/<plugin-id>/manifest.json` (`id`, `name`, `version`, `network_type`, `description`, `config_schema`, optional `schedule_default`); it must validate against `schemas/manifest.schema.json`. Mark mandatory fields with `"required": true` — the collector rejects a run with missing required fields before calling the plugin.
2. **Code** — `crates/osiris-plugins/src/<plugin_id>.rs` implementing `osiris_plugin::Collector`, then register it in `osiris_plugins::all()` (`crates/osiris-plugins/src/lib.rs`):

   ```rust
   pub struct Example(pub Manifest);

   #[async_trait]
   impl Collector for Example {
       fn manifest(&self) -> &Manifest { &self.0 }

       async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash> {
           let Some(Value::String(url)) = cfg.get("url") else {
               return Ok(CollectionResult::fail("url gerekli"));
           };
           let opts = FetchOptions::new("GET", url).timeout_secs(30).max_bytes(2_000_000);
           let resp = match fetch(host, opts).await {          // SSRF check on every hop
               Ok(r) => r,
               Err(e) => return Ok(CollectionResult::fail(format!("istek başarısız: {}", e.type_name()))),
           };
           let item = CollectedItem::new(resp.text());
           Ok(CollectionResult::ok(vec![item], Map::new()))
       }

       async fn health_check(&self, _host: &Host, _cfg: &Config) -> bool { true }
   }
   ```
3. **Fixtures** — at least one `fixtures/plugins/<plugin_id>_<scenario>.json` (`plugin`, `config`, recorded `http` exchanges, `expected` result, `expected_network` report). The replay transport answers only the recorded requests; anything else fails the test. Add a denial scenario (SSRF, wrong network) next to the happy path.
4. **Verify** — `cargo test -p osiris-plugins` and `cargo run -p osiris-cli -- plugins`.

Security checklist:
- Use only the host capabilities: `fetch` for HTTP (the default `UrlPolicy` blocks private, loopback, link-local and metadata addresses), `host.dns` for DNS, `host.safe_connect` for raw TCP. Never open sockets or build an HTTP client directly.
- `.onion` / `.i2p` are allowed only through the operator's proxies (`osiris_plugin::proxy::resolve_proxy` → `Proxy::Socks5h` / `Proxy::Http` from `OSIRIS_TOR_SOCKS` / `OSIRIS_I2P_HTTP_PROXY`); user config can never choose a proxy host.
- Validate hostnames, channels and tokens with the `osiris_security::sanitize_*` helpers.
- Never put secrets (API keys, tokens) into errors, metadata or logs.
- Cap response sizes, item counts and string lengths; set explicit timeouts.
- Return `CollectionResult::fail` for expected failures; `Err(PluginCrash)` only for genuinely unexpected input — the manager isolates it either way.

## Code Standards

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny check` — all clean before pushing.
- `unsafe` is forbidden workspace-wide. Return errors instead of panicking in service code.
- New behaviour needs tests; behaviour shared with other components needs a fixture.
- Python that remains (`tests/contract`, `tools/`): `ruff check tests tools plugins/username-search/tools` (see `ruff.toml`).
- Commit messages: short, descriptive, in Turkish, prefixed with the area (e.g. `API: ...`, `Plugin rss: ...`).

## Commit & Sync Rules

- One logical change per commit; never commit secrets (`.env`, `*.key`, `*.crt` are ignored — keep it that way).
- Direct-to-`master` work is the default for small batches; use a branch + PR for risky changes.
