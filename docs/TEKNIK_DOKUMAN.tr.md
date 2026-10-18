> **English:** For the English version of this document, see [here](TECHNICAL_DOCUMENT.md).

# OSIRIS
## Açık Kaynak İstihbarat (OSINT) Platformu — Teknik Doküman

> **O**pen **S**ource **I**ntelligence **R**esearch & **I**nformation **S**ystem
> Sürüm: 1.0.0-rc — Mimari, Kurulum & İşletim Dokümanı
> Uygulama: Rust (tek ikili, tek distroless imaj) · Arayüz: React/TypeScript

---

## İçindekiler

1. [Proje Özeti](#1-proje-özeti)
2. [Temel Özellikler](#2-temel-özellikler)
3. [Sistem Mimarisi](#3-sistem-mimarisi)
4. [Teknoloji Yığını](#4-teknoloji-yığını)
5. [Kurulum](#5-kurulum)
6. [Yapılandırma](#6-yapılandırma)
7. [Servisler ve Profiller](#7-servisler-ve-profiller)
8. [Veri Kaynakları ve Plugin'ler](#8-veri-kaynakları-ve-pluginler)
9. [İşleme Hattı](#9-işleme-hattı)
10. [Veritabanı](#10-veritabanı)
11. [Arama, Graf, Uyarı, Rapor](#11-arama-graf-uyarı-rapor)
12. [Arayüzler ve Otomasyon](#12-arayüzler-ve-otomasyon)
13. [Güvenlik, Gizlilik ve KVKK](#13-güvenlik-gizlilik-ve-kvkk)
14. [İşletim](#14-işletim)
15. [Yol Haritası](#15-yol-haritası)

---

## 1. Proje Özeti

OSIRIS; yüzey web, karanlık ağlar (Tor, I2P), sohbet ağları (IRC, Matrix), RSS/Atom, REST API'ler, Shodan, DNS/WHOIS ve blokzincirlerden veri toplayan, varlıkları çıkarıp ilişkilendiren ve hepsini aranabilir kılan, kendi sunucunuzda (VDS) 7/24 çalışan bir OSINT platformudur. Hiçbir veri üçüncü taraf buluta gitmez.

### Temel Felsefe

- **Önce gizlilik:** karanlık ağ trafiği yalnızca operatörün proxy'lerinden; her istek SSRF denetiminden geçer.
- **Modülerlik:** her kaynak tek bir trait'in (`Collector`) arkasında bir plugin.
- **Şeffaflık:** her yazma işlemi hash zincirli denetim günlüğünde; doğrulanabilir.
- **Sözleşme:** davranış altın fixture'lar ve kara kutu API testleriyle sabit ([SOZLESME.md](SOZLESME.md)).

---

## 2. Temel Özellikler

- **Toplama:** 11 plugin (bkz. §8), tek seferlik (API/CLI) ya da cron ile zamanlanmış; plugin başına zorunlu alan denetimi, çökme yalıtımı, kaynak sağlık metrikleri.
- **İşleme:** HTML temizleme ve `content_hash` ile tekilleştirme, dil tespiti (16 dil), regex varlıkları (e-posta, IP, alan adı, telefon, kripto adres, CVE) + ONNX NER (kişi, kurum, yer), konu sınıflandırma, 384 boyutlu çok dilli vektörler.
- **Arama:** tam metin (PostgreSQL `tsvector`), varlık bazlı; semantik arama (pgvector) kütüphane olarak.
- **Graf:** varlık ilişki grafı (kalıcı), merkezilik ve topluluk tespiti (kütüphane), GraphML/GEXF.
- **Uyarı:** kayıtlı sorgu eşleşmesi, sessize alma, z-skor anomali, webhook/Telegram/e-posta işleyicileri (kütüphane).
- **Rapor:** Markdown (API), HTML/JSON/CSV/STIX 2.1 (kütüphane).
- **KVKK:** kaynak başına saklama süresi, öğe ve varlık bazlı silme, PII'siz denetim kaydı.
- **Otomasyon:** n8n iş akışı şablonu, Prometheus ölçümleri, JSON log.

---

## 3. Sistem Mimarisi

```
 Tarayıcı (React UI) ─┐       n8n ─┐
                      ▼            ▼
                 Nginx (TLS 8443, /api/* → osiris-api)
                     │
        ┌────────────▼─────────────┐        ┌────────────────────────┐
        │ osiris-server api        │        │ osiris-server collector│
        └──────┬───────────┬───────┘        └──────────┬─────────────┘
               │ SQL       │ XADD                      │ XADD
               ▼           ▼                           ▼
        PostgreSQL 16   Redis Streams  ──XREADGROUP──▶ osiris-server pipeline
```

Süreçler, veri akışı, crate sorumlulukları ve güvenlik modeli: [ARCHITECTURE.tr.md](ARCHITECTURE.tr.md).

---

## 4. Teknoloji Yığını

| Alan | Seçim |
|------|-------|
| Dil | Rust (sürüm `rust-toolchain.toml`'da sabit), `unsafe` yasak |
| HTTP sunucu / istemci | axum 0.8, tower-http; reqwest (rustls) |
| Veritabanı | PostgreSQL 16 + pgvector (TimescaleDB varsa `source_metrics` hypertable), sqlx |
| Kuyruk | Redis Streams (tüketici grubu, en az bir kez teslim, ölü-mektup) |
| ML | ONNX Runtime (`ort`) — mBERT-hrl NER int8; fastembed — paraphrase-multilingual-MiniLM-L12-v2 |
| Dil tespiti | lingua (deterministik) |
| API dokümanı | utoipa (OpenAPI 3) + Swagger UI |
| Arayüz | React + TypeScript + Tailwind (Vite) |
| Dağıtım | Docker Compose, distroless imaj, Nginx (TLS), n8n |

---

## 5. Kurulum

### 5.1 Sistem Gereksinimleri

| Bileşen | Minimum | Önerilen |
|---------|---------|---------|
| **CPU** | 2 çekirdek | 4-8 çekirdek |
| **RAM** | 4 GB | 8-16 GB (NER + embedding ile) |
| **Depolama** | 40 GB SSD | 200 GB+ NVMe |
| **OS** | Docker çalıştıran güncel bir Linux (Ubuntu 22.04+, Debian 12+) | |

İmajı kaynaktan derlemek ~10–15 dk sürer ve derleme sırasında ~4 GB RAM ister.

### 5.2 Adımlar

```bash
git clone https://github.com/efe-clk/OSIRIS.git && cd OSIRIS
cp .env.example .env && chmod 600 .env
#   POSTGRES_PASSWORD, N8N_PASSWORD, OSIRIS_API_KEY, OSIRIS_JWT_SECRET → her biri için
#   openssl rand -hex 32 (OSIRIS_API_KEY ile OSIRIS_JWT_SECRET FARKLI olmalı)
./deploy/nginx/make-certs.sh                       # öz-imzalı TLS sertifikası

docker compose --profile models run --rm --build osiris-models   # modeller (isteğe bağlı, ~420 MB)
docker compose --profile full up -d --build                       # tüm servisler
```

`osiris-models` dosyaları host kullanıcısıyla `./models`'e yazar (`OSIRIS_UID`/`OSIRIS_GID`, varsayılan 1000; farklıysa `.env`'e ekleyin). Model olmadan sistem çalışır: yalnızca regex varlıkları çıkarılır, vektör üretilmez.

Veritabanı şeması boş PostgreSQL biriminin ilk açılışında `db/migrations/` dosyalarıyla kurulur.

### 5.3 İlk Doğrulama

```bash
docker compose ps                                     # osiris-api "healthy"
curl -sk https://localhost:8443/api/health            # {"service":"osiris-api","status":"ok"}
curl -sk -H "X-API-Key: $OSIRIS_API_KEY" https://localhost:8443/api/plugins | head -c 200
docker compose exec osiris-api osiris status          # CLI
```

Web arayüzü: `https://<sunucu>:8443/ui/` (operatör anahtarıyla giriş). n8n: `https://<sunucu>:8443/n8n/`.

### 5.4 Ağ Topolojisi

```
İnternet ── [Nginx :80, :8443 TLS]
               ├── /api/* → osiris-api:8000   (önek sıyrılır)
               ├── /ui/*  → osiris-web-ui:3000
               └── /n8n/* → n8n:5678
[iç Docker ağı] osiris-api / osiris-pipeline / osiris-collector ←→ postgresql, redis
                osiris-api:9464/metrics (yalnızca iç ağ) · tor:9050 · i2pd:4444 (darknet profili)
```

Uygulama portları host'ta yalnızca `127.0.0.1`'e bağlıdır (API `8001`, UI `3001`, PostgreSQL `5432`, Redis `6379`, n8n `5678`). Referans makinede `:443` başka bir ters proxy'de olduğu için TLS `:8443`'tedir (bkz. `deploy/nginx/README.md`).

---

## 6. Yapılandırma

`.env` (asla commitlenmez; `.env.example` şablondur):

| Değişken | Amaç |
|----------|------|
| `POSTGRES_PASSWORD` | DB parolası |
| `N8N_USER` / `N8N_PASSWORD` | n8n kapısı (ilk kurulum değeri geçerlidir) |
| `OSIRIS_API_KEY` | Operatör anahtarı (admin). Zorunlu: yoksa API başlamaz |
| `OSIRIS_INSECURE_OPEN` | `1` → anahtarsız açık mod (her çağıran admin; yalnızca güvenilir ağ) |
| `OSIRIS_JWT_SECRET` | JWT imza sırrı (API anahtarına düşme yok; yoksa `/auth/token` 503) |
| `OSIRIS_CORS_ORIGINS` | İzinli tarayıcı kaynakları (virgülle) |
| `OSIRIS_API_RATE_PER_MIN` | Kimlik başına dakikalık kota (600; 0 = kapalı) |
| `OSIRIS_RETENTION_DAYS` | KVKK varsayılan saklama süresi, gün (180; 0 = süreyle silinmez) |
| `OSIRIS_RETENTION_INTERVAL` | İşçi döngüsünde temizlik aralığı, sn (3600; 0 = kapalı) |
| `OSIRIS_SCHEDULER` | `on` → collector kaynakları cron'a göre çalıştırır (varsayılan `off`) |
| `OSIRIS_EMBEDDING_MODEL` | Embedding modeli; boş = vektör yok (compose: MiniLM-L12-v2) |
| `OSIRIS_NER` | `off` → ONNX NER kapalı (yalnızca regex) |
| `OSIRIS_MODEL_DIR` | Model kökü (imajda `/app/models`) |
| `OSIRIS_STREAM_MAXLEN` | Kuyruk akışı yaklaşık üst sınırı (200000) |
| `OSIRIS_USERNAME_DB_DIR` | username-search `sites_db` için ek izinli dizin |
| `OSIRIS_TOR_SOCKS` / `OSIRIS_I2P_HTTP_PROXY` | Tor SOCKS5 / I2P HTTP proxy, `host:port` (compose: `tor:9050`, `i2pd:4444`) |
| `OSIRIS_UID` / `OSIRIS_GID` | `osiris-models`'in `./models`'e yazdığı kullanıcı (1000) |

Compose'un sabitledikleri: `OSIRIS_LOG_FORMAT=json`, `OSIRIS_METRICS_ADDR=0.0.0.0:9464` (API), `OSIRIS_PIPELINE_CONSUMER`. Uyarı işleyicilerinin değişkenleri (`OSIRIS_ALERT_WEBHOOK_URL`, `OSIRIS_ALERT_TELEGRAM_BOT_TOKEN`/`_CHAT_ID`, `OSIRIS_ALERT_SMTP_URL`, `OSIRIS_ALERT_EMAIL_FROM`/`_TO`) kütüphane düzeyindedir; hiçbir servis henüz uyarı yayımlamaz.

---

## 7. Servisler ve Profiller

| Servis | Profil | Açıklama |
|--------|--------|----------|
| `postgresql`, `redis`, `n8n`, `nginx` | (her zaman) | Altyapı |
| `osiris-api` | `api`, `full` | REST API (`api`), sağlık denetimi `osiris-server health` |
| `osiris-pipeline` | `worker`, `full` | Kuyruk işçisi + saklama temizliği; `./models` salt-okunur |
| `osiris-collector` | `worker`, `full` | Zamanlayıcı (`OSIRIS_SCHEDULER=on` olmadıkça boşta) |
| `osiris-web-ui` | `ui`, `full` | Tarayıcı arayüzü |
| `osiris-models` | `models` | Tek seferlik model indirme/doğrulama |
| `tor`, `i2pd` | `darknet` | Karanlık ağ proxy'leri (yalnızca iç ağ) |

Üç OSIRIS servisi aynı imajı (`osiris:1.0.0-rc`) farklı alt komutla çalıştırır. Tek seferlik işler aynı imajla: `docker compose run --rm osiris-api <alt komut>` (`migrate`, `audit-verify`, `purge`, `collect --plugin … --config … [--enqueue]`).

---

## 8. Veri Kaynakları ve Plugin'ler

| Plugin | Ağ | Özet |
|--------|----|------|
| `web-scraper` | www | Sayfa + CSS seçici, her yönlendirmede SSRF denetimi |
| `rss` | rss | RSS/Atom (feedparser uyumlu temizleme) |
| `rest-api` | api | Metot allowlist'i, JSON-path; kayıt başına kanonik JSON |
| `shodan` | api | Host araması; anahtar hata/metadata'ya yazılmaz |
| `dns-whois` | api | Allowlist kayıt türleri, sınırlı WHOIS |
| `blockchain` | blockchain | EVM (`eth_blockNumber`, bakiye, işlem sayısı), Bitcoin (allowlist Blockstream) |
| `username-search` | www | ~2500 sitede kullanıcı adı izi (Maigret + WhatsMyName birleşik DB) |
| `tor` | tor | `.onion` yalnızca operatörün SOCKS5h proxy'siyle |
| `i2p` | i2p | eepsite yalnızca operatörün HTTP proxy'siyle |
| `irc` | irc | Enjeksiyon-arındırılmış komutlar, SSRF denetimli bağlantı, süre kutulu |
| `matrix` | matrix | Oda geçmişi, URL-kodlu kimlikler |

Her plugin'in yapılandırma şeması `GET /plugins` ile döner (arayüz formları buradan çizilir). Yeni plugin yazma rehberi: [CONTRIBUTING.tr.md](CONTRIBUTING.tr.md). username-search site DB'si `plugins/username-search/tools/build_full_db.py` ile tazelenir (geliştirme aracı).

**Kaynak kaydı ve toplama:** kaynak `POST /sources` ile kaydedilir (`plugin_id`, `url`, `schedule` 5 alanlı cron (UTC; haftanın günü 0/7 = Pazar), `config`, `retention_days`). `POST /sources/{id}/collect` tek seferlik çalıştırır; `OSIRIS_SCHEDULER=on` ise collector her kaynağı cron'una göre (aynı anda tek çalıştırma) çalıştırır. Görev yapılandırması `metadata.config` + `source_id`; kaynağın `url`'si `url`/`feed_url`/`endpoint` alanlarına (boşsa) yazılır. Sağlık: `sources.last_crawled_at`, `last_success_at`, `failure_count`, `avg_response_ms` ve `source_metrics`.

---

## 9. İşleme Hattı

```
[Plugin] → (≤ 512 KB JSON mesaj) → Redis Stream osiris:stream:raw_items
   → osiris-server pipeline (tüketici grubu osiris-pipeline)
       ├─ temizleme: HTML → metin, boşluk birleştirme, 200 000 kod noktası → content_hash
       ├─ dil (lingua, 16 dil)
       ├─ varlıklar: regex (6 tür) + ONNX NER (PER/ORG/LOC), (tür, değer.lower()) tekilleştirme
       ├─ konu sınıflandırma (anahtar kelime)
       ├─ vektör: fastembed, ilk 8000 kod noktası, 128 token, L2-normalize → embedding_v2
       └─ tek işlemde depolama (items ON CONFLICT content_hash, entities, item_entities)
   → XACK + XDEL
```

Hata kuralları: bozuk mesaj → ölü-mektup akışı (`osiris:stream:raw_items:dead`, `reason`); DB hatası → mesaj beklemede kalır, 5 dk sonra geri alınır; 5 teslimden sonra ölü-mektup. Eski LIST kuyruğu (`osiris:raw_items`) önce boşaltılır. Ayrıntı: [SOZLESME.md](SOZLESME.md) §1, §5, §8, §10–11.

Vektörü olmayan eski öğeler: `docker compose run --rm osiris-pipeline pipeline --reembed`.

---

## 10. Veritabanı

| Migration | İçerik |
|-----------|--------|
| `001_init.sql` | `sources`, `items` (`content_hash UNIQUE`, FTS indeksi), `entities` (benzersiz `(type, value)`), `item_entities`, `entity_relations`, `saved_queries`, `audit_logs`, `source_metrics` (TimescaleDB varsa hypertable) |
| `002_roles.sql` | `user_role`, `users`, `api_keys` (yalnızca özet), `graph_edges` |
| `003_audit_chain_version.sql` | `audit_logs.chain_version` |
| `004_retention.sql` | `sources.retention_days` (0–36500, CHECK) |
| `005_embedding.sql` | `items.embedding_v2 VECTOR(384)` + HNSW, `embedding_model` |
| `006_r7_cleanup.sql` | Eski `items.embedding` (1536) sütununun silinmesi; audit varsayılan sürümü 2 |

Tüm migration'lar tekrar çalıştırılabilir ve ikiliye gömülüdür: `osiris-server migrate`. TimescaleDB yokluğu ölümcül değildir (NOTICE + normal tablo).

---

## 11. Arama, Graf, Uyarı, Rapor

| Yetenek | API ucu | Kütüphane (crate) |
|---------|---------|-------------------|
| Tam metin arama | `POST /search` | `osiris-query` |
| Varlık araması | `POST /search/entity`, `GET /entities/top` | `osiris-query` |
| Semantik arama | — | `osiris_query::semantic_search` |
| Graf | `GET /graph`, `POST /graph/relation` | `osiris-graph` (merkezilik, CNM topluluk, GraphML/GEXF) |
| Uyarı eşleşmesi | `POST /alerts/test`, `/saved-queries` | `osiris-alert` (anomali, işleyiciler) |
| Rapor | `POST /reports/markdown` | `osiris-report` (HTML, JSON, CSV, STIX 2.1) |

API'de olmayan fonksiyonlar bilinçli olarak kütüphane düzeyindedir (E11 kararı); yeni uç eklemek sözleşme değişikliğidir.

---

## 12. Arayüzler ve Otomasyon

- **CLI:** `osiris status|plugins|collect` — çıkış kodları 0 (tamam), 1 (plugin yok / toplama başarısız), 2 (hatalı girdi). Konteynerde: `docker compose exec osiris-api osiris …`.
- **REST API:** [API.tr.md](API.tr.md); etkileşimli doküman `/api/docs`, OpenAPI `/api/openapi.json`.
- **Web UI** (`/ui/`, kök adres `/` buraya yönlenir): panel, kullanıcı adı tarama (Maigret tarzı, 2500+ site), toplama (manifest şemasından üretilen plugin formları, toplu iş, kaynak olarak kaydet), arama (tam metin / semantik / varlık), öğeler, varlıklar, ilişki grafı (merkezilik, topluluklar, GraphML/GEXF), kaynaklar (oluştur, düzenle, aç/kapat, çalıştır), uyarı kuralları, raporlar (Markdown/HTML/JSON/STIX 2.1), erişim ve gizlilik (anahtarlar, JWT, KVKK silme). Rol bazlı menü; ayrıntı [osiris-web-ui/README.md](../osiris-web-ui/README.md).
- **n8n:** `deploy/n8n/osiris-workflow.json` — Zamanlayıcı (15 dk) → Ayarlar (`feed_url`, `query`) → OSIRIS Topla → OSIRIS Arama; `X-API-Key` n8n servisinin `OSIRIS_API_KEY` değişkeninden. İçe aktardıktan sonra **Ayarlar** düğümünü düzenleyin.

---

## 13. Güvenlik, Gizlilik ve KVKK

Güvenlik modeli (çıkış denetimi, kimlik, RBAC, kota, denetim, hatalar, taşıma, çalışma zamanı): [ARCHITECTURE.tr.md](ARCHITECTURE.tr.md#güvenlik-modeli).

**KVKK (saklama ve silme, [SOZLESME.md](SOZLESME.md) §6):**
- Süre kuralı: kaynağın `retention_days` değeri (NULL → `OSIRIS_RETENTION_DAYS`, 0 → süreyle silinmez); kaynaksız öğelere varsayılan uygulanır. İşçi `OSIRIS_RETENTION_INTERVAL`'de bir, admin `POST /admin/retention/run` ile anında siler; yetim varlıklar da silinir.
- `DELETE /items/{id}`: öğe + yalnızca ona bağlı varlıklar.
- `POST /entities/erase`: bir varlık (tip + büyük/küçük harf duyarsız değer), isteğe bağlı onu anan öğeler ve aynı adlı graf kenarları; denetim kaydına değerin yalnızca SHA-256 özeti yazılır.

---

## 14. İşletim

### 14.1 Loglar ve Ölçümler
- `docker compose logs -f osiris-api` — JSON satırları; erişim logu `target = osiris_api::access` (yöntem, rota şablonu, durum, ms; yol değeri ve sorgu dizgesi yazılmaz).
- Prometheus: `osiris-api:9464/metrics` (iç ağ; Prometheus'u aynı Compose ağına ekleyin): `osiris_http_requests_total`, `osiris_http_request_duration_seconds`, `osiris_api_rate_limited_total`, `osiris_audit_failures_total`, `osiris_build_info`.

### 14.2 Denetim Zinciri
```bash
docker compose run --rm osiris-api audit-verify --limit 100000   # {"ok":true,…}; bozuksa çıkış 1
```
Not: R7'de v1 (migration 003 öncesi) kayıt doğrulaması kaldırıldı; böyle kayıtlar "Bilinmeyen chain_version" olarak raporlanır.

### 14.3 Yedekleme
```bash
docker compose exec -T postgresql pg_dump -U osiris -Fc osiris > osiris-$(date +%F).dump
# geri yükleme: docker compose exec -T postgresql pg_restore -U osiris -d osiris --clean < osiris-….dump
```
`./models` yeniden indirilebilir; `.env` ve `deploy/nginx/certs/` ayrıca yedeklenmelidir.

### 14.4 Yükseltme
```bash
git pull
docker compose --profile full build
docker compose run --rm osiris-api migrate          # yeni migration'lar (tekrar çalıştırılabilir)
docker compose --profile full up -d
docker compose restart nginx                        # upstream IP'leri yeniden çözülsün
```
Salt yapılandırma değişikliklerinde DB konteynerini yeniden oluşturmayın; uygulama servislerinde `up -d --no-deps <servis>` yeterlidir. DB parola rotasyonu: `ALTER ROLE osiris PASSWORD '…'`, `.env` güncelle, OSIRIS servislerini yeniden oluştur.

### 14.5 Sorun Giderme

| Belirti | Neden / çözüm |
|---------|---------------|
| `osiris-api` başlamıyor, log: `OSIRIS_API_KEY tanımlı değil` | `.env`'de anahtarı atayın |
| `/auth/token` 503 | `OSIRIS_JWT_SECRET` boş |
| Nginx 502 (`/api/`) | API yeniden oluşturulduktan sonra `docker compose restart nginx` |
| Toplama 502 `İç ağ/özel host engellendi` ya da `Host çözülemedi` | Hedef özel/iç adres ya da çözülemiyor (SSRF denetimi — bilinçli) |
| Kaynaklar kendiliğinden toplanmıyor | `OSIRIS_SCHEDULER=on` değil ya da `schedule` boş/geçersiz |
| Varlıklar yalnızca regex, vektör yok | Modeller indirilmemiş (`osiris-models`) ya da `OSIRIS_NER=off` |
| 429 `İstek kotası aşıldı` | `OSIRIS_API_RATE_PER_MIN`'i artırın ya da istemci temposunu düşürün |

### 14.6 Test Matrisi

| Test | Komut |
|------|-------|
| Rust birim + fixture | `cargo test --workspace` |
| Biçim / lint / bağımlılık | `cargo fmt --check` · `cargo clippy --all-targets -- -D warnings` · `cargo deny check` |
| DB + kuyruk entegrasyonu | `cargo test -p osiris-server --test integration` (`OSIRIS_TEST_DATABASE_URL`, `OSIRIS_TEST_REDIS_URL`) |
| Model fixture'ları | `cargo test --release -p osiris-pipeline --test models` |
| API sözleşmesi | `pytest -q tests/contract` (`OSIRIS_BASE_URL`, `OSIRIS_CONTRACT_API_KEY`) |
| Compose | `docker compose config -q` |
| Web UI | `npm run build` (`osiris-web-ui/`) |

Ayrıntı: [CONTRIBUTING.tr.md](CONTRIBUTING.tr.md).

---

## 15. Yol Haritası

| Aşama | Kapsam | Durum |
|-------|--------|-------|
| Faz 1–5 | Çekirdek, 11 plugin, graf, semantik arama, uyarılar, raporlar, JWT/RBAC, TLS | ✅ |
| Rust geçişi R0–R7 | Sözleşmeler, Rust çekirdek, veri katmanı, API/CLI/analiz, Python'un kaldırılması | ✅ ([durum](RUST_GECIS_DURUMU.md)) |
| Sıradaki | Uyarıların işçide yayımlanması (işleyiciler hazır), 2FA, sır kasası, PDF rapor, WASM plugin'leri ([tasarım](WASM_PLUGIN_TASARIMI.md)), :443 geçişi | Planlı |

---

*Bu doküman gerçekleşen sistemi yansıtır; davranış değişikliklerinde güncelleyin.*
