> **English:** See [README.md](README.md) for the English version of this document.

# OSIRIS

> **O**pen **S**ource **I**ntelligence **R**esearch & **I**nformation **S**ystem

OSIRIS kendi sunucunuzda çalışan bir açık kaynak istihbarat (OSINT) platformudur: birden çok ağdan (yüzey web, Tor, I2P, IRC, Matrix, RSS, API'ler, blokzincirler) veri toplar, varlıkları çıkarıp ilişkilendirir ve hepsini aranabilir kılar — üçüncü taraf bulut bağımlılığı olmadan.

Sürüm **1.0.0-rc**. Uygulama **Rust** ile yazılmıştır (tek ikili, tek Docker imajı); tarayıcı arayüzü React/TypeScript'tir.

## Temel İlkeler

- **Önce gizlilik** — karanlık ağ trafiği yalnızca operatörün Tor/I2P proxy'lerinden geçer; her istek SSRF denetiminden geçer
- **Modülerlik** — her veri kaynağı tek bir `Collector` trait'inin arkasında bir plugin'dir
- **Şeffaflık** — açık kaynak; her yazma işlemi hash zincirli denetim günlüğüne kaydedilir
- **Sözleşmeler** — davranış altın fixture'lar ve kara kutu API sözleşme testleriyle sabitlenmiştir

## Mimari Genel Bakış

```
 Tarayıcı (React UI) ─┐       n8n ─┐
                      ▼            ▼
                 Nginx (TLS 8443, /api/* → osiris-api)
                     │
        ┌────────────▼─────────────┐        ┌────────────────────────┐
        │ osiris-server api        │        │ osiris-server collector│
        │ REST · JWT/RBAC · kota   │        │ cron · 11 plugin       │
        └──────┬───────────┬───────┘        └──────────┬─────────────┘
               │ SQL       │ XADD                      │ XADD
               ▼           ▼                           ▼
        PostgreSQL 16   Redis Streams  ──XREADGROUP──▶ osiris-server pipeline
        (+ pgvector)    osiris:stream:raw_items        temizle · regex+NER · vektör · depola
```

Tek imaj (`osiris:1.0.0-rc`) tüm rolleri çalıştırır; rol bir alt komuttur. Katmanlar, veri akışı ve güvenlik modeli için [docs/ARCHITECTURE.tr.md](docs/ARCHITECTURE.tr.md).

## Bileşenler

| Yol | Rol |
|-----|-----|
| `crates/osiris-server` | Servis ikilisi: `api`, `pipeline`, `collector`, `migrate`, `models`, `audit-verify`, `purge`, `health` |
| `crates/osiris-cli` | `osiris` CLI: `status`, `plugins`, `collect` (çıkış kodları 0/1/2) |
| `crates/osiris-plugins`, `osiris-plugin` | 11 collector ve kullanabilecekleri host yetenekleri (denetimli HTTP, DNS, TCP) |
| `crates/osiris-collector`, `osiris-queue` | Toplama çalıştırma, kaynak sağlığı, cron zamanlayıcı, kuyruk üreticisi |
| `crates/osiris-pipeline` | Kuyruk tüketicisi: temizleme, dil, regex + ONNX NER, sınıflandırma, fastembed vektörleri |
| `crates/osiris-db`, `osiris-query` | PostgreSQL erişimi, migration'lar, denetim zinciri, saklama; tam metin / semantik / varlık araması |
| `crates/osiris-graph`, `osiris-alert`, `osiris-report` | Varlık grafı (merkezilik, CNM topluluk), uyarı + anomali, Markdown/HTML/JSON/CSV/STIX raporları |
| `crates/osiris-hash`, `osiris-extract`, `osiris-security`, `osiris-feed`, `osiris-types` | `content_hash`, kanonik JSON, varlık desenleri, SSRF denetimi + temizleyiciler, RSS/Atom ayrıştırma, ortak türler |
| `osiris-web-ui` | Tarayıcı arayüzü (panel, arama, kaynaklar, graf, uyarılar, raporlar) |
| `db/migrations` | SQL şeması (tekrar çalıştırılabilir; ikiliye gömülü) |
| `plugins/` | Plugin manifest'leri ve username-search site veritabanları (derleme zamanında gömülür) |

## Teknoloji Yığını

- **Dil:** Rust (`rust-toolchain.toml` ile sabit) — axum, sqlx, redis, ort (ONNX Runtime), fastembed, utoipa; arayüz için TypeScript + React
- **Veri:** PostgreSQL 16 + pgvector (TimescaleDB isteğe bağlı), Redis Streams
- **Güvenlik:** TLS (8443), operatör anahtarı / DB anahtarları (SHA-256) / JWT HS256, RBAC, kimlik başına kota, SSRF denetimi, hash zincirli denetim günlüğü (RFC 8785)
- **İşletim:** Docker Compose, distroless imaj, Prometheus `/metrics`, JSON log, n8n

## Hızlı Başlangıç

```bash
cp .env.example .env              # POSTGRES_PASSWORD, OSIRIS_API_KEY, OSIRIS_JWT_SECRET doldurun
                                  # (her sır için openssl rand -hex 32)
./deploy/nginx/make-certs.sh      # öz-imzalı TLS sertifikası (yeni kurulum)

# İsteğe bağlı ama önerilir: NER + embedding modelleri (~420 MB, özet doğrulamalı)
docker compose --profile models run --rm --build osiris-models

# Hepsi: PostgreSQL, Redis, n8n, Nginx, API, işçi, collector, Web UI
docker compose --profile full up -d --build

curl -sk https://localhost:8443/api/health        # {"service":"osiris-api","status":"ok"}
docker compose exec osiris-api osiris plugins     # konteyner içinde CLI
```

Veritabanı şeması boş bir PostgreSQL biriminin ilk açılışında kurulur. Mevcut bir kurulumu yükseltirken yeni migration'ları `docker compose run --rm osiris-api migrate` ile uygulayın (tekrar çalıştırılabilir). Zamanlanmış toplama `OSIRIS_SCHEDULER=on` yapılana kadar **kapalıdır**. Adım adım kurulum, yapılandırma ve işletim: [docs/TEKNIK_DOKUMAN.tr.md](docs/TEKNIK_DOKUMAN.tr.md).

### API kimlik doğrulama

```bash
# Operatör anahtarı karşılığında kısa ömürlü JWT (1 saat)
curl -sk -X POST https://localhost:8443/api/auth/token \
  -H 'Content-Type: application/json' \
  -d '{"api_key":"<OSIRIS_API_KEY>","role":"analyst"}'

curl -sk -X POST https://localhost:8443/api/search \
  -H 'Content-Type: application/json' -H 'Authorization: Bearer <jeton>' \
  -d '{"query":"örnek","limit":5}'
```

Roller: `viewer` (okuma) → `analyst` (toplama, kaynaklar, graf) → `admin` (anahtarlar, silme, saklama). Başvuru: [docs/API.tr.md](docs/API.tr.md); etkileşimli doküman `/api/docs`.

## Geliştirme

```bash
cargo test --workspace                     # birim + altın fixture testleri
cargo clippy --workspace --all-targets -- -D warnings
```

Veritabanı/kuyruk entegrasyon testleri, model fixture'ları ve API sözleşme paketi [docs/CONTRIBUTING.tr.md](docs/CONTRIBUTING.tr.md) içinde.

## Dokümantasyon

- Teknik doküman (kurulum, yapılandırma, işletim): [docs/TEKNIK_DOKUMAN.tr.md](docs/TEKNIK_DOKUMAN.tr.md)
- API başvurusu: [docs/API.tr.md](docs/API.tr.md) ([English](docs/API.md))
- Mimari: [docs/ARCHITECTURE.tr.md](docs/ARCHITECTURE.tr.md)
- Katkı, plugin rehberi: [docs/CONTRIBUTING.tr.md](docs/CONTRIBUTING.tr.md)
- Davranış sözleşmesi: [docs/SOZLESME.md](docs/SOZLESME.md); Rust geçiş kaydı: [docs/RUST_GECIS_DURUMU.md](docs/RUST_GECIS_DURUMU.md)

## Lisans

GNU General Public License v3.0 — bkz. [LICENSE](LICENSE)
