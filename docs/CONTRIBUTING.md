> **Türkçe:** Bu belgenin Türkçe sürümü için [CONTRIBUTING.tr.md](CONTRIBUTING.tr.md) dosyasına bakın.

# Contributing to OSIRIS

Thanks for contributing. These rules keep the codebase consistent and sustainable.

## Development Environment

```bash
python3 -m venv .venv
source .venv/bin/activate

pip install -e ./osiris-sdk
pip install -e ./osiris-collector
pip install -e ./osiris-pipeline
pip install -e ./osiris-graph
pip install -e ./osiris-alert
pip install -e ./osiris-report
pip install -e ./osiris-query
pip install -e ./osiris-api
pip install -e ./osiris-cli
pip install pytest pytest-cov httpx ruff jsonschema

# Run tests (80% coverage gate)
pytest -q
```

## Adding a New Plugin

1. Create a `plugins/<plugin-id>/` directory.
2. Write `manifest.json` (id, name, network_type, config_schema).
3. In `collector.py`, write a class derived from `BaseCollector`.
4. Add `requirements.txt`.
5. Verify with the `osiris plugins` command.
6. Security checklist for the new collector:
   - Validate every URL with `osiris.security.assert_safe_url`
   - Open raw TCP sockets only via `osiris.security.safe_create_connection` (never `socket.create_connection` on a user-supplied host)
     (allow `.onion` only in the Tor plugin via `allow_onion=True`).
   - Validate hostnames/domains/channels with the `sanitize_*` helpers.
   - Never log or return secrets (API keys, tokens).
   - Cap response sizes and item counts; set explicit timeouts.

## Code Standards

- Python: `ruff` lint (see `pyproject.toml`); fix all findings before pushing.
- Rust: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace --exclude osiris-py`, `cargo deny check` — all clean.
  After changing `crates/`, reinstall the bridge (`pip install ./crates/osiris-py`) and run
  `pytest` twice: default (Rust) and `OSIRIS_NATIVE=python` (reference).
- Tests: `pytest`; every module keeps tests under its `tests/` directory.
  New features require tests; the CI coverage gate (`--cov-fail-under=80`) must pass.
- Test files must have unique basenames across the repo (no two `test_cli.py`).
- Commit messages: short, descriptive, in Turkish, prefixed with the phase
  (e.g. `Faz 5: ...`) or area (e.g. `Infra: ...`).

## Commit & Sync Rules

- One logical change per commit; never commit secrets (`.env`, `*.key`, `*.crt` are ignored — keep it that way).
- Direct-to-`master` work is the default for small batches; use a branch + PR for risky changes.
- Before pushing: `py_compile` clean, `pytest` green, `ruff` clean.
