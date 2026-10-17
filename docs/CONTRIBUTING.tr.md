> **English:** For the English version of this document, see [here](CONTRIBUTING.md).

# OSIRIS'e Katkı

Katkınız için teşekkürler. Bu kurallar kod tabanını tutarlı ve sürdürülebilir tutar.

## Geliştirme Ortamı

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

# Testleri çalıştırın (%80 kapsam eşiği)
pytest -q
```

## Yeni Plugin Ekleme

1. `plugins/<plugin-id>/` dizini oluşturun.
2. `manifest.json` yazın (id, name, network_type, config_schema).
3. `collector.py` içinde `BaseCollector`'dan türeyen bir sınıf yazın.
4. `requirements.txt` ekleyin.
5. `osiris plugins` komutuyla doğrulayın.
6. Yeni collector için güvenlik kontrol listesi:
   - Her URL'yi `osiris.security.assert_safe_url` ile doğrulayın
   - Ham TCP soketini yalnızca `osiris.security.safe_create_connection` ile açın (kullanıcının verdiği host'a asla doğrudan `socket.create_connection` yapmayın)
     (`.onion` yalnızca Tor plugin'inde, `allow_onion=True` ile).
   - Host/domain/kanal için `sanitize_*` yardımcılarını kullanın.
   - Sırları (API anahtarı, jeton) asla loglamayın veya döndürmeyin.
   - Yanıt boyutlarını ve öğe sayılarını sınırlayın; açık zaman aşımı verin.

## Kod Standartları

- Python: `ruff` ile lint (bkz. `pyproject.toml`); push öncesi tüm bulguları düzeltin.
- Rust: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace --exclude osiris-py`, `cargo deny check` — hepsi temiz.
  `crates/` değişince köprüyü yeniden kurun (`pip install ./crates/osiris-py`) ve `pytest`'i iki
  kez koşun: varsayılan (Rust) ve `OSIRIS_NATIVE=python` (referans).
- Testler: `pytest`; her modül testlerini kendi `tests/` dizininde tutar.
  Yeni özellikler test gerektirir; CI kapsam eşiği (`--cov-fail-under=80`) geçmelidir.
- Test dosyası adları repo genelinde benzersiz olmalıdır (iki `test_cli.py` olmaz).
- Commit mesajları: kısa, açıklayıcı, Türkçe; faz öneki
  (örn. `Faz 5: ...`) veya alan öneki (örn. `Infra: ...`) kullanın.

## Commit & Senkron Kuralları

- Her mantıksal değişiklik ayrı commit; asla sır commitlemeyin
  (`.env`, `*.key`, `*.crt` ignore'lu — öyle kalmalı).
- Küçük işler için varsayılan doğrudan `master`'dır; riskli değişiklikte dal + PR açın.
- Push öncesi: `py_compile` temiz, `pytest` yeşil, `ruff` temiz.
