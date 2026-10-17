> **English:** See [CONTRIBUTING.md](CONTRIBUTING.md) for the English version of this document.

# OSIRIS'e Katkı

Katkınız için teşekkürler. Bu kurallar kod tabanını tutarlı ve sürdürülebilir tutar.

## Geliştirme Ortamı

- **Rust:** [rustup](https://rustup.rs) kurun; araç zinciri (`rustfmt` ve `clippy` dahil) `rust-toolchain.toml` ile sabittir ve ilk `cargo` çağrısında kendiliğinden kurulur.
- **Entegrasyon testleri için servisler:** Docker (PostgreSQL + pgvector, Redis).
- **Python (isteğe bağlı):** yalnızca kara kutu API sözleşme paketi (`tests/contract`) ve kod üretim araçları (`tools/`) için.

```bash
cargo build --workspace
cargo run -p osiris-server -- models          # NER + embedding modelleri ./models'e (~420 MB)
cargo run -p osiris-cli -- plugins            # `osiris` CLI
```

## Testler

| Katman | Komut | Gerekenler |
|--------|-------|-----------|
| Birim + altın fixture'lar | `cargo test --workspace` | — |
| Model fixture'ları (NER, embedding) | `OSIRIS_REQUIRE_MODELS=1 cargo test --release -p osiris-pipeline --test models` | `./models` |
| DB + kuyruk entegrasyonu | `OSIRIS_TEST_DATABASE_URL`, `OSIRIS_TEST_REDIS_URL` ile `cargo test -p osiris-server --test integration` | PostgreSQL, Redis |
| API sözleşmesi (kara kutu) | `OSIRIS_BASE_URL`, `OSIRIS_CONTRACT_API_KEY` ile `pytest -q tests/contract` | çalışan bir API |

Entegrasyon testleri için yerel servisler (veritabanı adı **mutlaka** `_test` ile bitmeli — tablolar boşaltılır):

```bash
docker run -d --name osiris-test-pg -e POSTGRES_USER=osiris -e POSTGRES_PASSWORD=osiris \
  -e POSTGRES_DB=osiris_test -p 127.0.0.1:55432:5432 pgvector/pgvector:pg16
docker run -d --name osiris-test-redis -p 127.0.0.1:56379:6379 redis:7.4-alpine
export OSIRIS_TEST_DATABASE_URL=postgresql://osiris:osiris@127.0.0.1:55432/osiris_test
export OSIRIS_TEST_REDIS_URL=redis://127.0.0.1:56379/1
cargo run -p osiris-server -- --database-url "$OSIRIS_TEST_DATABASE_URL" migrate
cargo test -p osiris-server --test integration
```

Yerel API'ye karşı sözleşme paketi:

```bash
OSIRIS_API_KEY=dev-key OSIRIS_JWT_SECRET=dev-jwt OSIRIS_DATABASE_URL=$OSIRIS_TEST_DATABASE_URL \
  cargo run -p osiris-server -- api --addr 127.0.0.1:8000 &
python3 -m venv .venv && .venv/bin/pip install -r tests/contract/requirements.txt
OSIRIS_BASE_URL=http://127.0.0.1:8000 OSIRIS_CONTRACT_API_KEY=dev-key \
  OSIRIS_CONTRACT_DESTRUCTIVE=1 .venv/bin/pytest -q tests/contract
```

### Fixture'lar bir sözleşmedir

`fixtures/` testlerin paylaştığı altın verileri tutar (hash, varlık çıkarma, denetim, STIX, graf, rapor, uyarı, güvenlik, NER, embedding ve her plugin senaryosu için bir oynatma dosyası). Beklenen değerler özgün Python referans uygulamasından üretildi ve donduruldu. Bir `expected` değerini değiştirmek **sözleşme değişikliğidir**: [SOZLESME.md](SOZLESME.md)'yi güncelleyin ve commit'te gerekçelendirin. `tests/contract/` değiştirilmeden API'ye karşı yeşil kalmalıdır.

### Üretilmiş tablolar

`crates/osiris-hash/src/html/tables.rs` ve `crates/osiris-feed/src/feed_tables.rs`, `tools/gen_*.py` tarafından bs4 / feedparser'dan üretilir (sürümler `tools/requirements.txt`'de sabit); elle düzenlemeyin. CI iki üreticiyi `--check` ile çalıştırır.

## Yeni Plugin Ekleme

1. **Manifest** — `plugins/<plugin-id>/manifest.json` (`id`, `name`, `version`, `network_type`, `description`, `config_schema`, isteğe bağlı `schedule_default`); `schemas/manifest.schema.json`'a uymalı. Zorunlu alanları `"required": true` ile işaretleyin — collector, eksik zorunlu alanlı çalıştırmayı plugin'i çağırmadan reddeder.
2. **Kod** — `crates/osiris-plugins/src/<plugin_id>.rs` içinde `osiris_plugin::Collector`'ı uygulayın, sonra `osiris_plugins::all()`'a kaydedin (`crates/osiris-plugins/src/lib.rs`):

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
           let resp = match fetch(host, opts).await {          // her adımda SSRF denetimi
               Ok(r) => r,
               Err(e) => return Ok(CollectionResult::fail(format!("istek başarısız: {}", e.type_name()))),
           };
           let item = CollectedItem::new(resp.text());
           Ok(CollectionResult::ok(vec![item], Map::new()))
       }

       async fn health_check(&self, _host: &Host, _cfg: &Config) -> bool { true }
   }
   ```
3. **Fixture'lar** — en az bir `fixtures/plugins/<plugin_id>_<senaryo>.json` (`plugin`, `config`, kayıtlı `http` alışverişleri, `expected` sonuç, `expected_network` raporu). Oynatıcı yalnızca kayıtlı isteklere yanıt verir; başka her istek testi başarısız kılar. Mutlu yolun yanına bir ret senaryosu (SSRF, yanlış ağ) ekleyin.
4. **Doğrulama** — `cargo test -p osiris-plugins` ve `cargo run -p osiris-cli -- plugins`.

Güvenlik kontrol listesi:
- Yalnızca host yeteneklerini kullanın: HTTP için `fetch` (varsayılan `UrlPolicy` özel, loopback, link-local ve metadata adreslerini engeller), DNS için `host.dns`, ham TCP için `host.safe_connect`. Asla doğrudan soket açmayın ya da HTTP istemcisi kurmayın.
- `.onion` / `.i2p` yalnızca operatörün proxy'leriyle (`osiris_plugin::proxy::resolve_proxy` → `OSIRIS_TOR_SOCKS` / `OSIRIS_I2P_HTTP_PROXY`'den `Proxy::Socks5h` / `Proxy::Http`); kullanıcı yapılandırması proxy host'u seçemez.
- Host adlarını, kanalları ve belirteçleri `osiris_security::sanitize_*` yardımcılarıyla doğrulayın.
- Sırları (API anahtarları, jetonlar) hataya, metadata'ya ya da loga asla yazmayın.
- Yanıt boyutlarını, öğe sayılarını ve dize uzunluklarını sınırlayın; zaman aşımlarını açıkça verin.
- Beklenen hatalarda `CollectionResult::fail`; `Err(PluginCrash)` yalnızca gerçekten beklenmeyen girdide — yönetici her iki durumu da yalıtır.

## Kod Standartları

- Push'tan önce `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny check` temiz olmalı.
- `unsafe` çalışma alanı genelinde yasaktır. Servis kodunda panik yerine hata döndürün.
- Yeni davranış test ister; başka bileşenlerle paylaşılan davranış fixture ister.
- Kalan Python (`tests/contract`, `tools/`): `ruff check tests tools plugins/username-search/tools` (bkz. `ruff.toml`).
- Commit mesajları: kısa, açıklayıcı, Türkçe ve alan önekli (örn. `API: ...`, `Plugin rss: ...`).

## Commit ve Senkron Kuralları

- Commit başına tek mantıksal değişiklik; sır commitlemeyin (`.env`, `*.key`, `*.crt` ignore'dadır — öyle kalsın).
- Küçük işlerde doğrudan `master`'a çalışmak varsayılandır; riskli değişikliklerde dal + PR kullanın.
