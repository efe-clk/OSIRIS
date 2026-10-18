> **English:** [CONTRIBUTING.md](CONTRIBUTING.md)

# OSIRIS'e Katkı

Bu rehber geliştirme ortamını kurmayı, testleri çalıştırmayı, yeni plugin eklemeyi ve
değişikliklerin hangi kurallarla kabul edildiğini anlatır. Sistemin yapısı için önce
[ARCHITECTURE.tr.md](ARCHITECTURE.tr.md)'yi okuyun.

## 1. Depo düzeni

| Yol | İçerik |
|-----|--------|
| `crates/` | Rust çalışma alanı (17 crate; sorumluluklar [ARCHITECTURE.tr.md §5](ARCHITECTURE.tr.md#5-crateler)) |
| `plugins/<id>/manifest.json` | Plugin manifest'leri (derleme zamanında gömülür); `username-search` site DB'leri |
| `db/migrations/` | SQL şeması (derleme zamanında gömülür; Docker ilk açılışta da uygular) |
| `schemas/` | JSON Şemaları: manifest, toplanan öğe, toplama sonucu, kuyruk mesajı |
| `fixtures/` | Dilden bağımsız altın veriler (hash, varlık, denetim, STIX, graf, rapor, uyarı, güvenlik, NER, embedding, plugin senaryoları) |
| `tests/contract/` | Çalışan API'ye karşı kara kutu sözleşme testleri (Python, pytest) |
| `tools/` | Üretilmiş tabloların üreticileri (bs4, feedparser) |
| `osiris-web-ui/` | React/TypeScript arayüzü |
| `deploy/` | Dockerfile, Nginx, Tor, n8n iş akışı |
| `docs/` | Belgeler (Türkçe + İngilizce) |

## 2. Geliştirme ortamı

- **Rust:** [rustup](https://rustup.rs). Araç zinciri ve bileşenleri (`rustfmt`, `clippy`)
  `rust-toolchain.toml` ile sabittir; ilk `cargo` çağrısında kendiliğinden kurulur.
- **Docker:** entegrasyon testleri için PostgreSQL (pgvector) ve Redis.
- **Node.js 20+:** yalnızca web arayüzü için.
- **Python 3.11+:** yalnızca sözleşme testleri ve `tools/` üreticileri için.

```bash
cargo build --workspace
cargo run -p osiris-server -- models        # NER + embedding modelleri ./models'e (~420 MB, isteğe bağlı)
cargo run -p osiris-cli -- plugins          # osiris CLI: gömülü plugin'ler
```

### 2.1 Yerel test yığını

Veritabanı adı **mutlaka** `_test` ile bitmelidir: entegrasyon testleri tabloları boşaltır ve
başka adla çalışmayı reddeder.

```bash
docker run -d --name osiris-test-pg -e POSTGRES_USER=osiris -e POSTGRES_PASSWORD=osiris \
  -e POSTGRES_DB=osiris_test -p 127.0.0.1:55432:5432 pgvector/pgvector:pg16
docker run -d --name osiris-test-redis -p 127.0.0.1:56379:6379 redis:7.4-alpine

export OSIRIS_TEST_DATABASE_URL=postgresql://osiris:osiris@127.0.0.1:55432/osiris_test
export OSIRIS_TEST_REDIS_URL=redis://127.0.0.1:56379/1
cargo run -p osiris-server -- --database-url "$OSIRIS_TEST_DATABASE_URL" migrate
```

### 2.2 API'yi yerelde çalıştırmak

```bash
OSIRIS_DATABASE_URL=$OSIRIS_TEST_DATABASE_URL OSIRIS_REDIS_URL=redis://127.0.0.1:56379/0 \
OSIRIS_API_KEY=gelistirme-anahtari OSIRIS_JWT_SECRET=gelistirme-jwt-sirri \
OSIRIS_API_RATE_PER_MIN=0 \
  cargo run -p osiris-server -- api --addr 127.0.0.1:8001

# Ayrı terminalde işçi (öğelerin veritabanına yazılması için)
OSIRIS_DATABASE_URL=$OSIRIS_TEST_DATABASE_URL OSIRIS_REDIS_URL=redis://127.0.0.1:56379/0 \
  cargo run -p osiris-server -- pipeline --loop
```

### 2.3 Web arayüzü

```bash
cd osiris-web-ui
npm ci
npm run dev               # http://localhost:3000; /api/* → http://127.0.0.1:8001 (önek sıyrılır)
npm run lint              # ESLint
npm run build             # tsc + vite → dist/
VITE_DEMO=1 npm run dev   # sunucusuz demo modu
```

Ayrıntı: [osiris-web-ui/README.md](../osiris-web-ui/README.md).

## 3. Testler

| Katman | Komut | Gerekenler |
|--------|-------|-----------|
| Birim + altın fixture'lar | `cargo test --workspace` | — |
| Biçim | `cargo fmt --all --check` | — |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | — |
| Bağımlılık denetimi | `cargo deny check` | `cargo install cargo-deny` |
| DB + kuyruk entegrasyonu | `cargo test -p osiris-server --test integration` | §2.1 yığını, `OSIRIS_TEST_*` |
| Model fixture'ları | `OSIRIS_REQUIRE_MODELS=1 cargo test --release -p osiris-pipeline --test models` | `./models` |
| API sözleşmesi | aşağıda | çalışan API |
| Web arayüzü | `npm run lint && npm run build` (`osiris-web-ui/`) | Node.js |
| Compose dosyası | `docker compose config -q` | Docker |

Entegrasyon testleri `OSIRIS_TEST_DATABASE_URL` yoksa, model testleri model yoksa atlanır.
`OSIRIS_REQUIRE_MODELS=1` model atlamasını hataya çevirir (CI'da açıktır).

**Sözleşme paketi** çalışan bir API'ye gerçek HTTP istekleri gönderir:

```bash
python3 -m venv .venv && .venv/bin/pip install -r tests/contract/requirements.txt
OSIRIS_BASE_URL=http://127.0.0.1:8001 OSIRIS_CONTRACT_API_KEY=gelistirme-anahtari \
  OSIRIS_CONTRACT_DESTRUCTIVE=1 .venv/bin/pytest -q tests/contract
```

| Değişken | Anlam |
|----------|-------|
| `OSIRIS_BASE_URL` | API adresi (Nginx üzerinden `https://…/api` de olur) |
| `OSIRIS_CONTRACT_API_KEY` | Operatör anahtarı |
| `OSIRIS_CONTRACT_VERIFY_TLS` | `0` → öz-imzalı sertifikayı kabul et |
| `OSIRIS_CONTRACT_DESTRUCTIVE` | `1` → kalıcı değişiklik yapan testleri de çalıştır (kendi kayıtlarını temizler) |

> Paketi Nginx üzerinden çalıştırırsanız IP başına 10 istek/sn sınırına takılırsınız; API'ye
> doğrudan (`127.0.0.1:8001`) bağlanın ve `OSIRIS_API_RATE_PER_MIN=0` verin.

> Test çıktısını `grep`/`tail`'e boruyla bağlayıp `&&` ile commit'e zincirlemeyin: borunun çıkış
> kodu testin başarısızlığını gizler. Testleri borusuz çalıştırın.

### 3.1 Fixture'lar bir sözleşmedir

`fixtures/` altındaki beklenen değerler, Rust geçişi sırasında özgün Python referans
uygulamasından üretilip dondurulmuştur. Bir `expected` değerini değiştirmek **sözleşme
değişikliğidir**: [SOZLESME.md](SOZLESME.md)'yi güncelleyin ve gerekçeyi commit iletisine yazın.
`tests/contract/` paketi değişiklikten sonra da yeşil kalmalıdır; yeni bir uç ya da davranış
ekliyorsanız pakete test ekleyin.

### 3.2 Üretilmiş tablolar

`crates/osiris-hash/src/html/tables.rs` ve `crates/osiris-feed/src/feed_tables.rs`,
`tools/gen_html_tables.py` ve `tools/gen_feed_tables.py` tarafından bs4/feedparser'dan üretilir
(sürümler `tools/requirements.txt`'de sabit). Elle düzenlemeyin, üreticiyi çalıştırın. CI iki
üreticiyi `--check` ile çalıştırır.

## 4. Yeni plugin ekleme

1. **Manifest** — `plugins/<plugin-id>/manifest.json`:

   ```json
   {
     "id": "ornek",
     "name": "Örnek",
     "version": "1.0.0",
     "network_type": "www",
     "author": "…",
     "description": "Ne topladığını tek cümleyle anlatın",
     "config_schema": {
       "url": {"type": "string", "required": true, "description": "Hedef adres"},
       "limit": {"type": "integer", "default": 20}
     },
     "capabilities": ["text"],
     "requires_proxy": false,
     "schedule_default": "0 */6 * * *"
   }
   ```

   Manifest `schemas/manifest.schema.json`'a uymalıdır (zorunlu: `id`, `name`, `version`,
   `network_type`, `description`, `config_schema`). Zorunlu alanları `"required": true` ile
   işaretleyin: yönetici eksik zorunlu alanlı çalıştırmayı plugin'i çağırmadan reddeder.
   `config_schema`'daki alan sırası arayüzdeki form sırasıdır.

2. **Kod** — `crates/osiris-plugins/src/ornek.rs`:

   ```rust
   use async_trait::async_trait;
   use osiris_plugin::{Collector, FetchOptions, Host, PluginCrash, fetch};
   use osiris_types::pyval::trunc;
   use osiris_types::{CollectedItem, CollectionResult, Config, Manifest};
   use serde_json::{Map, Value};

   pub struct Ornek(pub Manifest);

   #[async_trait]
   impl Collector for Ornek {
       fn manifest(&self) -> &Manifest {
           &self.0
       }

       async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash> {
           let Some(Value::String(url)) = cfg.get("url") else {
               return Ok(CollectionResult::fail("url gerekli"));
           };
           // Her istek ve yönlendirme SSRF denetiminden geçer; boyut ve süre açıkça sınırlanır.
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

   Ardından `crates/osiris-plugins/src/lib.rs`'te modülü ekleyin (`pub mod ornek;`) ve `all()`
   listesine kaydedin: `Arc::new(ornek::Ornek(manifest!("ornek")))`. `manifest!` makrosu
   `plugins/ornek/manifest.json`'ı derleme zamanında gömer.

3. **Fixture'lar** — en az bir mutlu yol ve bir ret senaryosu, ör.
   `fixtures/plugins/ornek_basari.json` ve `fixtures/plugins/ornek_ssrf_reddi.json`. Dosya
   `config`'i, kayıtlı `http` alışverişlerini (yöntem, URL, parametreler → durum, başlıklar,
   gövde), varsa `dns` ve `sockets` kayıtlarını, beklenen `expected` sonucu ve `expected_network`
   raporunu içerir. Oynatıcı yalnızca kayıtlı isteklere yanıt verir; başka her istek ve
   tüketilmeyen her kayıt testi başarısız kılar. Fixture'larda DNS her host'u `93.184.216.34`'e
   çözer. Örnek olarak `fixtures/plugins/rss_basari.json` ve `rss_ssrf_reddi.json`'a bakın.

4. **Doğrulama** — `cargo test -p osiris-plugins` ve `cargo run -p osiris-cli -- plugins`; sonra
   [PLUGINS.tr.md](PLUGINS.tr.md) ile [PLUGINS.md](PLUGINS.md)'ye plugin'in bölümünü ekleyin.

### 4.1 Güvenlik kontrol listesi

- Yalnızca host yeteneklerini kullanın: HTTP için `fetch` (varsayılan `UrlPolicy` özel, loopback,
  link-local ve metadata adreslerini engeller), DNS için `host.dns`, ham TCP için
  `host.safe_connect`. Asla doğrudan soket açmayın ya da kendi HTTP istemcinizi kurmayın.
- `.onion`/`.i2p` yalnızca operatörün proxy'leriyle: `osiris_plugin::proxy::resolve_proxy`
  (`OSIRIS_TOR_SOCKS` / `OSIRIS_I2P_HTTP_PROXY`). Kullanıcı yapılandırması proxy host'u seçemez.
- Host adlarını, kanalları ve belirteçleri `osiris_security::sanitize_*` ile doğrulayın.
- Sırları (API anahtarı, jeton) hata iletisine, üst veriye ya da loga yazmayın. Sır taşıyan
  `config` alanlarına `api_key`, `token`, `password`, `secret` gibi adlar verin: API bunları
  kaynak detayında maskeler.
- Yanıt boyutlarını, öğe sayılarını ve dize uzunluklarını sınırlayın; zaman aşımlarını açıkça verin.
- Beklenen hatalarda `CollectionResult::fail`; `Err(PluginCrash)` yalnızca gerçekten beklenmeyen
  girdide. Yönetici ikisini de yalıtır.

## 5. Kod standartları

- Push'tan önce temiz olmalı: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets
  -- -D warnings`, `cargo test --workspace`, `cargo deny check`; arayüz değiştiyse `npm run lint`
  ve `npm run build`.
- `unsafe` çalışma alanında yasaktır. Servis kodunda panik yerine hata döndürün.
- API hata gövdeleri `{"detail": …}` biçimindedir ve iç ayrıntı (SQL, sürücü iletisi) içermez:
  iç hatalar için `ApiError::internal("genel ileti", neden)` kullanın.
- Yeni davranış test ister; başka bileşenlerle paylaşılan davranış fixture ister; yeni API ucu
  sözleşme testi ister.
- Kalan Python (`tests/contract`, `tools/`, `plugins/username-search/tools`): `ruff check` temiz
  olmalı (`ruff.toml`).
- Kod yorumları ve kullanıcıya dönen iletiler Türkçedir; dosyanın mevcut üslubunu izleyin.

## 6. Belgeler

- Kullanıcıya dönük belgeler iki dildedir: `README.md` / `README.tr.md` ve `docs/` altındaki
  `X.md` / `X.tr.md` çiftleri. `SOZLESME.md`, Rust geçiş kayıtları ve WASM notu yalnızca
  Türkçedir. Bir davranışı değiştiren commit iki dili de günceller.
- Yeni API ucu → [API.tr.md](API.tr.md) + [API.md](API.md); yeni ortam değişkeni →
  [TEKNIK_DOKUMAN.tr.md §6](TEKNIK_DOKUMAN.tr.md#6-yapılandırma-başvurusu) + İngilizcesi ve
  `.env.example`; yeni plugin → [PLUGINS.tr.md](PLUGINS.tr.md) + İngilizcesi.
- CI belgelerdeki göreli bağlantıları denetler; kırık bağlantı işi kırar.

## 7. Commit ve CI

- Commit iletileri kısa, açıklayıcı, Türkçe ve alan önekli olur: `API: …`, `Web UI: …`,
  `Plugin rss: …`, `Doküman: …`, `CI: …`.
- Commit başına tek mantıksal değişiklik. Sır commitlemeyin (`.env`, `*.key`, `*.crt`
  `.gitignore`'dadır).
- Küçük işlerde doğrudan `master`'a çalışmak varsayılandır; riskli değişikliklerde dal + PR.

CI (`.github/workflows/ci.yml`) her push'ta şu işleri çalıştırır:

| İş | Ne denetler |
|----|-------------|
| `rust` | `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace` |
| `integration` | PostgreSQL + Redis ile migration'lar (psql ve `migrate`, ikişer kez), model indirme, entegrasyon + model testleri, gerçek API'ye karşı sözleşme paketi |
| `docker` | Distroless imajın derlenmesi ve duman testi |
| `cargo-deny` | Lisanslar, güvenlik danışmanlıkları, yasaklı bağımlılıklar |
| `tools-and-docs` | `ruff`, üretilmiş tabloların güncelliği, belge bağlantıları, `pip-audit` |
| `web-ui-build` | ESLint, derleme, demo derlemesi, demo kodunun üretim paketine girmediği, `npm audit` |
