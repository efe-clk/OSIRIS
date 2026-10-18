> **English:** [TECHNICAL_DOCUMENT.md](TECHNICAL_DOCUMENT.md)

# OSIRIS — Kurulum ve İşletim Kılavuzu

> **O**pen **S**ource **I**ntelligence **R**esearch & **I**nformation **S**ystem · Sürüm **1.0.0-rc**
>
> Bu kılavuz OSIRIS'i sıfırdan kurmayı, yapılandırmayı, günlük kullanmayı ve işletmeyi anlatır.
> Sistemin iç yapısı için [ARCHITECTURE.tr.md](ARCHITECTURE.tr.md), uç uç API başvurusu için
> [API.tr.md](API.tr.md), veri kaynakları için [PLUGINS.tr.md](PLUGINS.tr.md) belgelerine bakın.

---

## İçindekiler

1. [OSIRIS nedir?](#1-osiris-nedir)
2. [Kavramlar](#2-kavramlar)
3. [Gereksinimler](#3-gereksinimler)
4. [Kurulum](#4-kurulum)
5. [Uçtan uca ilk kullanım](#5-uçtan-uca-ilk-kullanım)
6. [Yapılandırma başvurusu](#6-yapılandırma-başvurusu)
7. [Servisler, profiller ve portlar](#7-servisler-profiller-ve-portlar)
8. [Kimlik, roller ve erişim yönetimi](#8-kimlik-roller-ve-erişim-yönetimi)
9. [Toplama ve zamanlama](#9-toplama-ve-zamanlama)
10. [Arama, graf, uyarı ve rapor](#10-arama-graf-uyarı-ve-rapor)
11. [Kişisel veri: saklama ve silme (KVKK)](#11-kişisel-veri-saklama-ve-silme-kvkk)
12. [Günlük işletim](#12-günlük-işletim)
13. [Sorun giderme](#13-sorun-giderme)
14. [Bilinen sınırlamalar](#14-bilinen-sınırlamalar)
15. [Yol haritası](#15-yol-haritası)

---

## 1. OSIRIS nedir?

OSIRIS, kendi sunucunuzda çalışan bir **açık kaynak istihbarat (OSINT) platformudur**. Yüzey
web sayfalarından, RSS/Atom akışlarından, REST API'lerden, Shodan'dan, DNS/WHOIS'ten,
blokzincirlerden, IRC ve Matrix sohbetlerinden, Tor ve I2P ağlarından veri toplar. Topladığı
metni temizler, içindeki varlıkları (e-posta, IP, alan adı, kişi, kurum…) çıkarır, anlamsal
vektöre çevirir ve hepsini aranabilir kılar. Hiçbir veri üçüncü taraf bir buluta gönderilmez.

Uygulama Rust ile yazılmıştır: tüm sunucu rolleri **tek bir ikili** (`osiris-server`) ve **tek
bir Docker imajı** (`osiris:1.0.0-rc`) içindedir. Tarayıcı arayüzü React/TypeScript'tir.

Neler yapabilirsiniz:

| İş | Nasıl |
|----|-------|
| Bir kullanıcı adının yüzlerce sitede izini sürmek | Web arayüzü → *Kullanıcı adı tarama* (`username-search` plugin'i) |
| Bir haber akışını, web sayfasını ya da API'yi düzenli izlemek | *Kaynaklar* ekranında kaynak + cron zamanlaması |
| Toplanan metinlerde kelimeyle ya da anlamca aramak | *Arama*: tam metin, semantik, varlık |
| Bir e-posta/IP/alan adının geçtiği tüm kayıtları bulmak | *Varlıklar* ya da `POST /search/entity` |
| Varlıklar arası ilişkileri haritalamak | *İlişki grafı* (merkezilik, topluluklar, GraphML/GEXF) |
| Belirli kelimeler için kural tanımlamak | *Uyarı kuralları* |
| Bulguları Markdown/HTML/JSON/STIX 2.1 rapora dökmek | *Raporlar* |
| İlgili kişinin verisini silmek (KVKK) | *Erişim ve gizlilik* ya da `POST /entities/erase` |

## 2. Kavramlar

| Terim | Anlamı |
|-------|--------|
| **Plugin** | Bir veri türünü toplayan bileşen (ör. `rss`, `web-scraper`). 11 plugin ikiliye gömülüdür; her birinin yapılandırma şeması `GET /plugins` ile okunur. Ayrıntı: [PLUGINS.tr.md](PLUGINS.tr.md). |
| **Kaynak (source)** | Kaydedilmiş bir plugin yapılandırması: hangi plugin, hangi ayarlar, hangi cron, ne kadar süre saklanacak. Elle ya da zamanlayıcıyla çalıştırılır ve sağlık bilgisi tutar. |
| **Toplama (collection)** | Bir plugin'in bir kez çalıştırılması. Kaynaktan bağımsız (`POST /collect`) ya da bir kaynak üzerinden (`POST /sources/{id}/collect`) yapılabilir. |
| **Kuyruk** | Toplanan her öğe Redis Stream `osiris:stream:raw_items`'e yazılır; işleme hattı (pipeline) buradan okur. Toplama ile işleme bu sayede birbirinden ayrıdır. |
| **Öğe (item)** | İşlenip veritabanına yazılmış tek bir kayıt (bir sayfa, bir akış girdisi, bir profil…). Temizlenmiş metnin SHA-256 özetiyle (`content_hash`) tekilleştirilir. |
| **Varlık (entity)** | Öğelerden çıkarılan değer: `email`, `ip`, `domain`, `phone`, `crypto_address`, `cve` (regex) ve `person`, `org`, `location` (NER modeli). |
| **Graf** | Varlık adları arasındaki ilişki kenarları (`graph_edges`). Kenarlar `POST /graph/relation` ile eklenir (arayüzde *İlişki ekle*, kullanıcı adı taramasında *Grafa bağla*). |
| **Kayıtlı sorgu / uyarı kuralı** | Adı ve metni olan sorgu; `alert_enabled` ise uyarı kuralı olarak eşleştirilir. |
| **Denetim günlüğü** | Yazma ve hassas okuma işlemlerinin hash zincirli, yalnızca eklenen kaydı (`audit_logs`). |

## 3. Gereksinimler

| Bileşen | En az | Önerilen |
|---------|-------|----------|
| CPU | 2 çekirdek | 4–8 çekirdek |
| RAM | 4 GB (modelsiz) | 8–16 GB (NER + embedding modelleriyle) |
| Disk | 40 GB SSD | 200 GB+ NVMe (veri hacmine göre) |
| İşletim sistemi | Docker Engine + Compose v2 çalıştıran güncel Linux (Ubuntu 22.04+, Debian 12+) | |
| Ağ | Hedef kaynaklara giden internet erişimi | Tor/I2P için çıkışın engellenmemesi |

Notlar:

- İmajı kaynaktan derlemek 10–15 dakika sürer ve derleme sırasında yaklaşık 4 GB RAM ister.
  RAM'i kıt bir sunucuda imajı başka bir makinede derleyip `docker save | ssh … docker load`
  ile taşıyabilirsiniz (bkz. [§12.5](#125-yükseltme)).
- Modeller (~420 MB) isteğe bağlıdır. Olmadan sistem çalışır, ama yalnızca regex varlıkları
  çıkarılır ve semantik arama kapalı kalır.
- Compose dosyası her servise bellek tavanı koyar (`mem_limit`). Toplamı yaklaşık 9 GB'tır;
  bunlar üst sınırdır, gerçek kullanım çok daha azdır.

## 4. Kurulum

### 4.1 Depoyu alın

```bash
git clone https://github.com/efe-clk/OSIRIS.git
cd OSIRIS
```

### 4.2 Ortam dosyasını hazırlayın

```bash
cp .env.example .env
chmod 600 .env
```

`.env` içinde en az şu dört değeri doldurun. Her biri için ayrı bir rastgele değer üretin:

```bash
openssl rand -hex 32
```

| Değişken | Ne için |
|----------|---------|
| `POSTGRES_PASSWORD` | Veritabanı parolası |
| `OSIRIS_API_KEY` | **Operatör anahtarı** — tam yetkili (admin) API anahtarı. Boşsa API **başlamaz**. |
| `OSIRIS_JWT_SECRET` | Kısa ömürlü JWT'leri imzalayan sır. `OSIRIS_API_KEY`'den **farklı** olmalı. Boşsa JWT kapalıdır. |
| `N8N_PASSWORD` | n8n otomasyon arayüzünün parolası (`N8N_USER` ile birlikte) |

Diğer tüm değişkenler isteğe bağlıdır; tam liste [§6](#6-yapılandırma-başvurusu)'dadır.

> `.env` dosyasını asla depoya eklemeyin (`.gitignore`'dadır). `POSTGRES_PASSWORD` ve
> `N8N_PASSWORD` yalnızca ilgili veri birimi **ilk** oluşturulurken uygulanır; sonradan
> değiştirmek için [§12.6](#126-sır-rotasyonu)'ya bakın.

### 4.3 TLS sertifikası üretin

Nginx `:8443` portunda TLS ile hizmet verir ve sertifika dosyası olmadan açılmaz. İlk
kurulumda öz-imzalı bir sertifika üretin:

```bash
./deploy/nginx/make-certs.sh       # deploy/nginx/certs/osiris.local.{crt,key}, 825 gün
```

Betik var olan sertifikanın üzerine yazmaz. Gerçek alan adı ve sertifikaya geçiş:
[deploy/nginx/README.md](../deploy/nginx/README.md).

### 4.4 Modelleri indirin (isteğe bağlı, önerilir)

```bash
docker compose --profile models run --rm --build osiris-models
```

Bu komut NER (kişi/kurum/yer) ve embedding (anlamsal vektör) modellerini sabit
revizyonlardan indirir, özetlerini doğrular ve `./models` dizinine yazar. Dosyalar host'taki
`OSIRIS_UID:OSIRIS_GID` (varsayılan `1000:1000`) kullanıcısıyla yazılır; kullanıcı kimliğiniz
farklıysa (`id -u`, `id -g`) bu iki değişkeni `.env`'e ekleyin. Ayrıntı:
[models/README.md](../models/README.md).

### 4.5 Servisleri başlatın

```bash
docker compose --profile full up -d --build
```

`full` profili şunları başlatır: PostgreSQL (pgvector), Redis, n8n, Nginx, API, işleme hattı
işçisi, collector (zamanlayıcı) ve web arayüzü. Tor ve I2P proxy'leri ayrı bir profildedir:

```bash
docker compose --profile full --profile darknet up -d      # Tor/I2P plugin'leri için
```

Veritabanı şeması, boş bir PostgreSQL biriminin **ilk** açılışında `db/migrations/`
dosyalarından kendiliğinden kurulur. Var olan bir kurulumu yükseltirken migration'ları elle
uygulayın (bkz. [§12.5](#125-yükseltme)).

### 4.6 Kurulumu doğrulayın

```bash
docker compose ps                                        # osiris-api "healthy" olmalı
curl -sk https://localhost:8443/api/health               # {"service":"osiris-api","status":"ok"}
curl -sk -H "X-API-Key: <OSIRIS_API_KEY>" https://localhost:8443/api/auth/me
#   {"role":"admin","user":"operator"}
docker compose exec osiris-api osiris status             # CLI: sürüm ve plugin sayısı
docker compose exec osiris-api osiris plugins            # 11 plugin
```

Tarayıcıdan `https://<sunucu>:8443/` adresi `/ui/`'ye yönlenir. Öz-imzalı sertifika uyarısını
kabul edin, **API anahtarı** kimlik türünü seçip `OSIRIS_API_KEY` değerini girin.

| Adres | İçerik |
|-------|--------|
| `https://<sunucu>:8443/ui/` | Web arayüzü |
| `https://<sunucu>:8443/api/` | REST API (`/api/docs` etkileşimli doküman, `/api/openapi.json`) |
| `https://<sunucu>:8443/n8n/` | n8n otomasyon arayüzü |

## 5. Uçtan uca ilk kullanım

Aşağıdaki adımlar bir RSS akışını kaynak olarak ekler, toplar, sonucu arar ve rapora döker.
Hepsi arayüzden de yapılabilir; burada eşdeğer API çağrıları gösterilmiştir.

```bash
API=https://localhost:8443/api
K=<OSIRIS_API_KEY>

# 1) Günlük kullanım için operatör anahtarı yerine bir analist anahtarı üretin
curl -sk -X POST $API/auth/keys -H "X-API-Key: $K" -H 'Content-Type: application/json' \
  -d '{"username":"ayse","role":"analyst","name":"dizüstü"}'
# → {"id":"…","api_key":"osiris_…","role":"analyst"}   (ham anahtar yalnızca bu yanıtta görünür)
A=<dönen api_key>

# 2) Kaynak oluşturun (15 dakikada bir; öğeler 90 gün saklansın)
curl -sk -X POST $API/sources -H "X-API-Key: $A" -H 'Content-Type: application/json' -d '{
  "name": "Örnek haber akışı", "plugin_id": "rss", "network_type": "rss",
  "url": "https://example.com/feed.xml", "schedule": "*/15 * * * *",
  "config": {"max_items": 50}, "retention_days": 90}'
# → {"id":"<kaynak-id>"}

# 3) Hemen bir kez çalıştırıp önizleyin
curl -sk -X POST "$API/sources/<kaynak-id>/collect?include_items=true" -H "X-API-Key: $A"
# → {"items":12,"metadata":{…},"results":[{"url":…,"title":…,"snippet":…}, …]}

# 4) İşçi öğeleri birkaç saniye içinde işler; sonra arayın
curl -sk -X POST $API/search -H "X-API-Key: $A" -H 'Content-Type: application/json' \
  -d '{"query":"fidye yazılımı","limit":10}'

# 5) Rapor üretin
curl -sk -X POST $API/reports/generate -H "X-API-Key: $A" -H 'Content-Type: application/json' -d '{
  "title":"Haftalık özet","format":"html","summary":"…",
  "findings":[{"title":"Bulgu 1","description":"…"}],"sources":["https://example.com/feed.xml"]}'
```

Kaynağın zamanlamaya göre kendiliğinden çalışması için `.env`'de `OSIRIS_SCHEDULER=on`
yapıp collector'ı yeniden oluşturun (`docker compose --profile full up -d osiris-collector`).
Zamanlayıcı bilinçli olarak varsayılan **kapalıdır**: önce kaynakların cron değerlerini gözden
geçirmeniz beklenir.

## 6. Yapılandırma başvurusu

Tüm ayarlar ortam değişkeniyle verilir. Compose kurulumunda bunları `.env` dosyasına yazarsınız;
`docker-compose.yml` ilgili servise aktarır. "Servis" sütunu değişkeni okuyan rolü gösterir.

### 6.1 Zorunlu ve güvenlik

| Değişken | Varsayılan | Servis | Açıklama |
|----------|-----------|--------|----------|
| `OSIRIS_API_KEY` | — | api, n8n | Operatör anahtarı (admin). Boşsa API başlamaz. Sabit süreli karşılaştırılır. |
| `OSIRIS_JWT_SECRET` | — | api | HS256 JWT imza sırrı. Boşsa `POST /auth/token` `503` döner ve Bearer jetonlar reddedilir. API anahtarına geri düşme **yoktur**. |
| `OSIRIS_INSECURE_OPEN` | — | api | `1` → anahtarsız açık mod: her çağıran `admin` sayılır. Yalnızca yalıtılmış geliştirme ağında. |
| `OSIRIS_CORS_ORIGINS` | `http://localhost:3000` | api | Tarayıcıdan doğrudan API'ye gelen istekler için izinli kökenler (virgülle). Arayüz Nginx üzerinden aynı kökenden çağırdığı için üretimde genellikle değiştirmek gerekmez. |
| `OSIRIS_API_RATE_PER_MIN` | `600` | api | Kimlik başına dakikalık istek kotası; `0` = kapalı. Aşılınca `429`. |
| `POSTGRES_PASSWORD` | `osiris_dev_password` | postgresql, tümü | Veritabanı parolası; Compose bağlantı adresini bundan kurar. |
| `N8N_USER` / `N8N_PASSWORD` | `admin` / `osiris_dev_password` | n8n | n8n temel kimlik doğrulaması. |

### 6.2 Bağlantılar ve adresler

| Değişken | Varsayılan | Servis | Açıklama |
|----------|-----------|--------|----------|
| `OSIRIS_DATABASE_URL` | Compose: `postgresql://osiris:…@postgresql:5432/osiris` | tümü | PostgreSQL bağlantısı. Compose dışında elle verin. |
| `OSIRIS_REDIS_URL` | `redis://localhost:6379/0` (Compose: `redis://redis:6379/0`) | tümü | Redis bağlantısı (kuyruk). |
| `OSIRIS_API_ADDR` | `0.0.0.0:8000` | api | API dinleme adresi (`--addr` ile de verilir). |
| `OSIRIS_METRICS_ADDR` | boş (Compose: `0.0.0.0:9464`) | api | Prometheus `/metrics` dinleyicisi; boş = kapalı. API portunda sunulmaz. |
| `OSIRIS_LOG_FORMAT` | metin (Compose: `json`) | tümü | `json` → satır başına bir JSON nesnesi. Ayrıntı düzeyi `RUST_LOG` ile (ör. `RUST_LOG=debug`). |

### 6.3 İşleme hattı ve modeller

| Değişken | Varsayılan | Servis | Açıklama |
|----------|-----------|--------|----------|
| `OSIRIS_EMBEDDING_MODEL` | Compose: `sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2` | pipeline, api | Embedding modeli. Boş → vektör üretilmez, semantik arama `503`. API ile işçi **aynı** değeri kullanmalıdır. |
| `OSIRIS_MODEL_DIR` | `models` (imajda `/app/models`) | pipeline, api | Model kök dizini. |
| `OSIRIS_NER` | açık | pipeline | `off`/`0`/`false` → ONNX NER kapalı, yalnızca regex varlıkları. |
| `OSIRIS_PIPELINE_CONSUMER` | `host-pid` (Compose: `osiris-pipeline-1`) | pipeline | Redis tüketici grubu içindeki tüketici adı. Birden çok işçi çalıştırırsanız her birine farklı ad verin. |
| `OSIRIS_STREAM_MAXLEN` | `200000` | api, collector, CLI | Kuyruk akışının yaklaşık üst sınırı (1 000–10 000 000). |
| `OSIRIS_UID` / `OSIRIS_GID` | `1000` | osiris-models | `./models`'e yazan host kullanıcısı. |

### 6.4 Toplama

| Değişken | Varsayılan | Servis | Açıklama |
|----------|-----------|--------|----------|
| `OSIRIS_SCHEDULER` | `off` | collector | `on`/`1`/`true` → kaynaklar cron'larına göre çalışır. |
| `OSIRIS_TOR_SOCKS` | Compose: `tor:9050` | api, collector | Tor SOCKS5 proxy'si (`host:port`). Tanımlıysa kullanıcı yapılandırmasındaki proxy alanlarını **ezer**. |
| `OSIRIS_I2P_HTTP_PROXY` | Compose: `i2pd:4444` | api, collector | I2P HTTP proxy'si (`host:port`); aynı kural. |
| `OSIRIS_USERNAME_DB_DIR` | — | api, collector | `username-search` plugin'inin `sites_db` ayarıyla özel site veritabanı okuyabileceği ek dizin. |

### 6.5 Saklama (KVKK)

| Değişken | Varsayılan | Servis | Açıklama |
|----------|-----------|--------|----------|
| `OSIRIS_RETENTION_DAYS` | `180` | api, pipeline | Kaynakta `retention_days` verilmediğinde uygulanan saklama süresi (gün). `0` → varsayılan kapalı (yalnızca kaynakta pozitif süre verilenler silinir). |
| `OSIRIS_RETENTION_INTERVAL` | `3600` | pipeline | İşçinin saklama temizliğini kaç saniyede bir yaptığı; `0` = kapalı. |

### 6.6 Kütüphane düzeyindekiler

`OSIRIS_ALERT_WEBHOOK_URL`, `OSIRIS_ALERT_TELEGRAM_BOT_TOKEN`, `OSIRIS_ALERT_TELEGRAM_CHAT_ID`,
`OSIRIS_ALERT_SMTP_URL`, `OSIRIS_ALERT_EMAIL_FROM` ve `OSIRIS_ALERT_EMAIL_TO`, `osiris-alert`
kütüphanesinin uyarı işleyicilerini yapılandırır. **Şu an hiçbir servis uyarı yayımlamaz**
(bkz. [§14](#14-bilinen-sınırlamalar)); bu değişkenleri ayarlamanın etkisi yoktur.

### 6.7 Testlerde kullanılanlar

`OSIRIS_TEST_DATABASE_URL`, `OSIRIS_TEST_REDIS_URL`, `OSIRIS_REQUIRE_MODELS` ve sözleşme
paketinin `OSIRIS_BASE_URL`, `OSIRIS_CONTRACT_*` değişkenleri yalnızca testler içindir:
[CONTRIBUTING.tr.md](CONTRIBUTING.tr.md).

## 7. Servisler, profiller ve portlar

Compose profilleri hangi servislerin başlatılacağını seçer; profilsiz servisler her zaman
başlar.

| Servis | Profil | İmaj / komut | Görev |
|--------|--------|--------------|-------|
| `postgresql` | — | `pgvector/pgvector:pg16` | Veritabanı (pgvector eklentili) |
| `redis` | — | `redis:7.4-alpine` (AOF açık) | Kuyruk |
| `n8n` | — | `n8nio/n8n:1.114.4` | İş akışı otomasyonu |
| `nginx` | — | `nginx:1.31-alpine` | TLS, yönlendirme, güvenlik başlıkları, IP başına hız sınırı |
| `osiris-api` | `api`, `full` | `osiris-server api` | REST API |
| `osiris-pipeline` | `worker`, `full` | `osiris-server pipeline --loop --interval 5 --batch 50` | Kuyruk işçisi + saklama temizliği |
| `osiris-collector` | `worker`, `full` | `osiris-server collector` | Cron zamanlayıcı (`OSIRIS_SCHEDULER=on` değilse boşta bekler) |
| `osiris-web-ui` | `ui`, `full` | `osiris-web-ui:1.0.0-rc` | Tarayıcı arayüzü (statik nginx, port 3000) |
| `osiris-models` | `models` | `osiris-server models` | Tek seferlik model indirme |
| `tor`, `i2pd` | `darknet` | `osiris-tor:0.1.0`, `purplei2p/i2pd` | Karanlık ağ proxy'leri (yalnızca iç ağ) |

Üç OSIRIS servisi aynı imajı farklı alt komutla çalıştırır. Aynı imajla tek seferlik işler:

```bash
docker compose run --rm osiris-api migrate                 # migration'ları uygula
docker compose run --rm osiris-api audit-verify --limit 100000
docker compose run --rm osiris-api purge                   # saklama temizliği şimdi
docker compose run --rm osiris-api collect --plugin rss --config '{"feed_url":"https://…"}'
docker compose run --rm osiris-pipeline pipeline --reembed # vektörsüz öğeleri vektörleştir
```

### 7.1 Ağ topolojisi

```
İnternet ──► Nginx  :80 (HTTP)  ve  :8443 (TLS)
              ├─ /api/*  → osiris-api:8000      (/api öneki sıyrılır: /api/search → /search)
              ├─ /ui/*   → osiris-web-ui:3000   (/ ve /ui → /ui/ yönlendirmesi)
              └─ /n8n/*  → n8n:5678

İç Docker ağı:
  osiris-api, osiris-pipeline, osiris-collector ──► postgresql:5432, redis:6379
  osiris-api:9464/metrics (yalnızca iç ağ)
  tor:9050, i2pd:4444 (darknet profili; host'a yayınlanmaz)
```

Host'a yayınlanan portlar:

| Port | Bağlandığı adres | Servis |
|------|------------------|--------|
| `80`, `8443` | tüm arayüzler | Nginx |
| `8001` | `127.0.0.1` | API (doğrudan, Nginx'siz) |
| `3001` | `127.0.0.1` | Web arayüzü (doğrudan) |
| `5432` | `127.0.0.1` | PostgreSQL |
| `6379` | `127.0.0.1` | Redis (kimliksizdir; asla dışa açmayın) |
| `5678` | `127.0.0.1` | n8n |

`127.0.0.1` portlarına dışarıdan SSH tüneliyle erişin (`ssh -L 8001:127.0.0.1:8001 sunucu`).
TLS'in neden `8443`'te olduğu ve `443`'e taşınması: [deploy/nginx/README.md](../deploy/nginx/README.md).

## 8. Kimlik, roller ve erişim yönetimi

### 8.1 Kimlik türleri

Her istek **tek** bir kimlik bilgisi taşır. API şu sırayla dener:

1. **Operatör anahtarı** — `X-API-Key: <OSIRIS_API_KEY>`. Her zaman `admin`, kullanıcı adı `operator`.
2. **JWT** — `Authorization: Bearer <jeton>`. `POST /auth/token` ile operatör anahtarı karşılığında
   üretilir; 1 saat geçerlidir, rolü jetonun içindedir, kullanıcı adı `api-client`'tır.
3. **Kullanıcı anahtarı** — `X-API-Key: osiris_…`. `POST /auth/keys` ile üretilir; veritabanında
   yalnızca SHA-256 özeti tutulur. Rolü, anahtarın bağlı olduğu kullanıcının rolüdür.

Bir istemcinin kim olduğunu ve rolünü öğrenmek için `GET /auth/me` kullanın. Bu uç yan
etkisizdir; web arayüzü giriş sırasında onu çağırır.

### 8.2 Roller

Roller birbirini kapsar: `viewer` < `analyst` < `admin`.

| Rol | Yapabildikleri |
|-----|----------------|
| `viewer` | Okuma: plugin'ler, istatistik, öğeler, varlıklar, arama (tam metin/semantik/varlık), graf ve analizi, kaynak listesi/detayı, kayıtlı sorgular |
| `analyst` | + toplama, kaynak oluşturma/güncelleme/silme/çalıştırma, kayıtlı sorgu oluşturma/silme, uyarı denemesi, graf ilişkisi ekleme, rapor üretme |
| `admin` | + anahtar yönetimi, öğe silme, varlık silme (KVKK), saklama temizliği |

Tam yetki matrisi: [API.tr.md §2](API.tr.md#2-roller-ve-yetki-matrisi).

### 8.3 Önerilen işletim

- Operatör anahtarını yalnızca yönetim işleri ve acil durum için kullanın; günlük kullanım için
  kişi başına kullanıcı anahtarı üretin (arayüz: *Erişim ve gizlilik*).
- Otomasyonlara (n8n, betikler) en düşük yeterli rolü verin; salt okuyan bir pano için `viewer`.
- Ayrılan bir kişinin anahtarını `DELETE /auth/keys/{id}` ile iptal edin. Satır silinmez,
  `revoked` olarak işaretlenir (denetim izi korunur).
- Var olan bir `username` için yeni anahtar üretmek o kullanıcının **rolünü günceller**. Bu,
  kullanıcının iptal edilmemiş diğer anahtarlarının yetkisini de değiştirir.

## 9. Toplama ve zamanlama

### 9.1 Toplama yolları

| Yol | Ne zaman | Kuyruğa yazar mı |
|-----|----------|------------------|
| `POST /collect`, `POST /collect/batch` (≤ 10 iş) | Tek seferlik, kaynak kaydetmeden | Evet |
| `POST /sources/{id}/collect` | Kayıtlı kaynağı şimdi çalıştırmak | Evet + kaynak sağlığı |
| Zamanlayıcı (`OSIRIS_SCHEDULER=on`) | Kaynakların cron'una göre | Evet + kaynak sağlığı |
| `osiris collect <plugin> --config '<json>'` (CLI) | Komut satırından | Evet |
| `osiris-server collect --plugin … --config …` | Deneme: sonuç JSON olarak basılır | Yalnızca `--enqueue` ile |

Her toplamada önce plugin'in zorunlu alanları denetlenir (`Zorunlu alanlar eksik: …`), sonra
plugin ayrı bir görevde çalışır. Plugin çökerse yalnızca o toplama `plugin hatası: …` ile
başarısız olur, servis etkilenmez. Başarılı toplamanın öğeleri kuyruğa, oradan işçiye gider:
**öğelerin aranabilir olması için `osiris-pipeline` çalışıyor olmalıdır.**

### 9.2 Kaynak yapılandırması

Kaynak oluştururken plugin'e özgü ayarlar `config` nesnesine yazılır (alanlar için
[PLUGINS.tr.md](PLUGINS.tr.md) ya da `GET /plugins`). Kaynak çalıştırılırken:

- `config`'e `source_id` eklenir;
- kaynağın `url` alanı doluysa ve `config`'te yoksa, değer `url`, `feed_url` ve `endpoint`
  anahtarlarına kopyalanır. Böylece RSS kaynağında adresi bir kez `url` olarak vermek yeter.

Kaynak detayında (`GET /sources/{id}`) sır taşıyan alanlar `********` olarak döner. Bu kural,
anahtar adı `api_key`, `access_token`, `token`, `password`, `passwd`, `secret`, `authorization`,
`cookie`, `credential` ya da `private_key` içeren her alana, iç içe nesnelerde de (ör.
`rest-api`'nin `headers.Authorization` alanı) uygulanır. Güncellemede `********` göndermek eski
sırrı korur; bu sayede arayüz sırları hiç görmeden kaynak düzenleyebilir.

### 9.3 Zamanlama kuralları

- `schedule`: standart 5 alanlı cron, **UTC**. Haftanın günü `0` ve `7` Pazar'dır. Örnekler:
  `*/15 * * * *` (15 dakikada bir), `0 */6 * * *` (6 saatte bir), `30 3 * * 1` (Pazartesi 03:30).
- Geçersiz cron, kaynak oluşturma/güncellemede `422` ile reddedilir.
- Zamanlayıcı kaynak listesini dakikada bir yeniden okur ve 15 saniyede bir vadesi gelenleri
  çalıştırır. Bir kaynağın aynı anda **tek** çalışması olur; önceki sürerken gelen tetik atlanır.
- Kaçırılan tetikler (servis kapalıyken) geriye dönük çalıştırılmaz.
- Devre dışı (`enabled=false`) ya da cron'u boş kaynaklar zamanlanmaz. Devre dışı bir kaynağı
  elle çalıştırmak `409` döner.

### 9.4 Kaynak sağlığı

Kaynak üzerinden yapılan her toplama şunları günceller: `last_crawled_at`; başarıda
`last_success_at` ve `failure_count = 0`, başarısızlıkta `failure_count + 1`; `avg_response_ms`
(son çalışmanın süresi); ve `source_metrics` tablosuna bir satır (TimescaleDB kuruluysa
hypertable). Arayüzün *Panel* ve *Kaynaklar* ekranları bunları gösterir.

## 10. Arama, graf, uyarı ve rapor

### 10.1 Arama

| Tür | Uç | Nasıl çalışır |
|-----|----|---------------|
| Tam metin | `POST /search` | PostgreSQL `to_tsvector('simple', …)` + `plainto_tsquery`, `ts_rank`'e göre sıralı. `simple` sözlüğü kök bulma yapmaz: "saldırı" ile "saldırıları" farklı kelimelerdir. |
| Semantik | `POST /search/semantic` | Sorgu, işçiyle aynı modelle 384 boyutlu vektöre çevrilir ve `embedding_v2` ile kosinüs benzerliğine göre sıralanır (`similarity` 0–1). Model çok dillidir: Türkçe sorgu İngilizce metni bulabilir. Model yoksa `503`. |
| Varlık | `POST /search/entity` | Tip + değer alt dizesi (büyük/küçük harf duyarsız). |

Vektörü olmayan eski öğeler semantik aramada görünmez. Modelleri sonradan kurduysanız
`docker compose run --rm osiris-pipeline pipeline --reembed` çalıştırın.

### 10.2 Graf

Graf, varlık adları arasındaki yönsüz kenarlardan oluşur ve `graph_edges` tablosunda kalıcıdır.
Kenarlar **elle** eklenir (`POST /graph/relation`; arayüzde *İlişki ekle*, kullanıcı adı
taramasında *Grafa bağla*). İşleme hattı öğelerden kendiliğinden kenar üretmez.

- `GET /graph` — düğümler ve kenarlar (≤ 5 000 düğüm, 20 000 kenar).
- `GET /graph/analysis` — derece merkeziliği, Clauset–Newman–Moore toplulukları (≤ 5 000 düğümde),
  istenen düğümün komşuları.
- `GET /graph/export?format=graphml|gexf` — Gephi, Cytoscape gibi araçlar için dosya.

API grafı ilk kullanımda veritabanından (en çok 10 000 kenar) belleğe yükler.

### 10.3 Uyarı kuralları

Kayıtlı sorgu (`POST /saved-queries`) `alert_enabled: true` ile uyarı kuralı olur. Eşleşme
kuralı basittir: kuralın metni (kırpılıp küçük harfe çevrilmiş) öğenin başlığında ya da
içeriğinde **alt dize** olarak geçiyorsa kural eşleşir. `query_type` alanı (`fts`, `semantic`,
`regex`, `entity`, `graph`) saklanır, ama bugün eşleşmeyi değiştirmez.

`POST /alerts/test` bir metni etkin kurallara karşı dener (kayıt oluşturmaz, bildirim
göndermez) ve eşleşen kuralların `last_triggered_at` alanını günceller. Toplanan öğelerin
kurallarla otomatik eşleştirilmesi ve bildirim gönderilmesi henüz yoktur
([§14](#14-bilinen-sınırlamalar)).

### 10.4 Raporlar

`POST /reports/generate` başlık, kapsam, özet, bulgular, kaynaklar ve göstergelerden rapor
üretir. Rapor yanıt gövdesinde döner, sunucuda saklanmaz.

| Biçim | Medya türü | Not |
|-------|-----------|-----|
| `markdown` | `text/markdown` | HTML kaçışlı |
| `html` | `text/html` | Tek dosya, satır içi stil |
| `json` | `application/json` | Makine okunur |
| `stix` | `application/stix+json` | STIX 2.1 paketi; yalnızca `ip`, `domain`, `email`, `crypto_address`, `hash`, `cve` göstergeleri |

Web arayüzündeki *Rapora ekle* düğmeleri bulguları tarayıcıdaki bir taslakta biriktirir;
*Raporlar* ekranı bu taslaktan rapor üretir.

## 11. Kişisel veri: saklama ve silme (KVKK)

OSIRIS kişisel veri içerebilecek açık kaynak kayıtları toplar. Saklama ve silme için şu
mekanizmalar vardır.

**Süreli saklama**

- Her kaynağın `retention_days` değeri vardır: boş → `OSIRIS_RETENTION_DAYS` (180), `0` → süreyle
  silinmez, `1–36 500` → `collected_at`'i bu kadar günden eski öğeler silinir.
- Kaynaksız öğelere (ör. `POST /collect` ile toplananlar) varsayılan süre uygulanır.
- İşçi temizliği `OSIRIS_RETENTION_INTERVAL` saniyede bir yapar; admin `POST /admin/retention/run`
  ile hemen tetikleyebilir. Silinen öğelerin varlık bağları da gider; hiçbir öğeye bağlı
  kalmayan varlıklar silinir.

**Talep üzerine silme (admin)**

| İşlem | Uç | Etki |
|-------|----|------|
| Tek öğeyi sil | `DELETE /items/{id}` | Öğe + yalnızca ona bağlı varlıklar |
| İlgili kişinin verisini sil | `POST /entities/erase` | Varlık (tip + büyük/küçük harf duyarsız değer), `delete_items: true` ise onu anan **tüm öğeler**, aynı adlı graf kenarları |

Denetim günlüğüne silinen değer yazılmaz; yalnızca küçük harfli değerin SHA-256 özeti
(`value_sha256`) yazılır. Böylece "bu verinin şu tarihte silindiği" kanıtlanabilir, ama değer
günlükte kalmaz.

**Dikkat:** silme yalnızca canlı veritabanını etkiler; daha önce alınmış yedeklerde veri kalır.
Yedek saklama sürenizi buna göre belirleyin.

## 12. Günlük işletim

### 12.1 Loglar

```bash
docker compose logs -f osiris-api          # JSON satırları (OSIRIS_LOG_FORMAT=json)
docker compose logs -f osiris-pipeline
```

Erişim logları `target = osiris_api::access` ile yazılır ve yöntem, rota şablonu (ör.
`/sources/{source_id}`), durum ve süreyi (ms) içerir. Yol değerleri, sorgu dizgeleri ve gövdeler
loga yazılmaz. Compose her servisin log dosyasını 3 × 10 MB ile sınırlar.

### 12.2 Ölçümler

API, Prometheus metin biçimindeki ölçümleri ayrı bir dinleyicide sunar
(`osiris-api:9464/metrics`, yalnızca iç ağ). Prometheus'u aynı Compose ağına ekleyin.

| Ölçüm | Tür | Açıklama |
|-------|-----|----------|
| `osiris_http_requests_total{method,route,status}` | sayaç | İstek sayısı |
| `osiris_http_request_duration_seconds{method,route}` | histogram | İstek süresi |
| `osiris_api_rate_limited_total` | sayaç | Kotaya takılan istekler |
| `osiris_audit_failures_total` | sayaç | Yazılamayan denetim kayıtları (sıfırdan farklıysa inceleyin) |
| `osiris_build_info{version}` | gösterge | Sürüm |

### 12.3 Denetim zincirini doğrulama

```bash
docker compose run --rm osiris-api audit-verify --limit 100000
# {"ok":true,"checked":…,"truncated":false,"error":null}   bozuksa çıkış kodu 1
```

Her kayıt bir öncekinin özetine bağlıdır; araya kayıt eklemek, kayıt silmek ya da değiştirmek
zinciri bozar. Migration 003 öncesinden kalan (v1) kayıtlar doğrulanmaz ve
`Bilinmeyen chain_version: 1` olarak raporlanır.

Denetlenen işlemler: `auth.token`, `auth.key.create`, `auth.key.revoke`, `collect`,
`collect.failed`, `collect.batch`, `search`, `search.semantic`, `source.create`,
`source.update`, `source.delete`, `source.collect`, `item.delete`, `entity.erase`,
`retention.run`.

### 12.4 Yedekleme ve geri yükleme

```bash
# Yedek
docker compose exec -T postgresql pg_dump -U osiris -Fc osiris > osiris-$(date +%F).dump

# Geri yükleme (var olan verinin yerine geçer)
docker compose exec -T postgresql pg_restore -U osiris -d osiris --clean < osiris-AAAA-AA-GG.dump
```

Ayrıca yedekleyin: `.env`, `deploy/nginx/certs/`, n8n birimi (`n8ndata`) ve kullanıyorsanız
Tor birimi (`tordata`). `./models` yeniden indirilebilir. Redis kuyruğu geçicidir; AOF açık
olduğu için yeniden başlatmada bekleyen mesajlar korunur.

### 12.5 Yükseltme

```bash
docker compose exec -T postgresql pg_dump -U osiris -Fc osiris > yedek-oncesi.dump   # önce yedek
git pull
docker compose --profile full build
docker compose run --rm osiris-api migrate        # yeni migration'lar (tekrar çalıştırılabilir)
docker compose --profile full up -d
docker compose restart nginx                      # upstream IP'leri yeniden çözülsün
```

- Migration'lar ikiliye gömülüdür ve tekrar çalıştırılabilir; uygulanmış olanlar atlanır.
- Yalnızca yapılandırma değiştiyse veritabanı konteynerini yeniden oluşturmayın:
  `docker compose up -d --no-deps <servis>` yeterlidir.
- Sunucuda derleme yapmak istemiyorsanız imajları başka bir makinede derleyip taşıyın:
  `docker save osiris:1.0.0-rc osiris-web-ui:1.0.0-rc | gzip | ssh sunucu 'gunzip | docker load'`.

### 12.6 Sır rotasyonu

| Sır | Adımlar |
|-----|---------|
| `OSIRIS_API_KEY` | `.env`'i güncelleyin → `docker compose up -d --no-deps osiris-api n8n`. Eski anahtar hemen geçersizleşir. |
| `OSIRIS_JWT_SECRET` | `.env`'i güncelleyin → `osiris-api`'yi yeniden oluşturun. Verilmiş tüm JWT'ler geçersizleşir. |
| `POSTGRES_PASSWORD` | `docker compose exec postgresql psql -U osiris -c "ALTER ROLE osiris PASSWORD '…'"` → `.env`'i güncelleyin → OSIRIS servislerini yeniden oluşturun. |
| Kullanıcı anahtarı | Arayüzde iptal edip yenisini üretin. |

### 12.7 n8n iş akışı

`deploy/n8n/osiris-workflow.json` hazır bir şablondur: *Zamanlayıcı* (15 dakika) → *Ayarlar*
(`feed_url`, `query`) → *OSIRIS Topla* (`POST /collect`) → *OSIRIS Arama* (`POST /search`). API
anahtarını n8n servisinin `OSIRIS_API_KEY` ortam değişkeninden okur. n8n arayüzünden içe
aktarın, **Ayarlar** düğümünü düzenleyin ve etkinleştirin. Kendi iş akışlarınızda API'yi iç
ağdaki `http://osiris-api:8000` adresinden çağırın.

## 13. Sorun giderme

| Belirti | Olası neden | Çözüm |
|---------|-------------|-------|
| `osiris-api` başlamıyor; log: `OSIRIS_API_KEY tanımlı değil` | Operatör anahtarı boş | `.env`'de `OSIRIS_API_KEY` atayın |
| `POST /auth/token` → `503` | `OSIRIS_JWT_SECRET` boş | Sırrı atayıp API'yi yeniden oluşturun |
| Nginx `502` (`/api/`) | API yeniden oluşturuldu, Nginx eski IP'yi tutuyor | `docker compose restart nginx` |
| Nginx açılmıyor; log: sertifika bulunamadı | `make-certs.sh` çalıştırılmadı | `./deploy/nginx/make-certs.sh` |
| Toplama `502`: `İç ağ/özel host engellendi` ya da `Host çözülemedi` | Hedef özel/iç bir adres ya da DNS çözülemiyor (SSRF koruması) | Bilinçli davranış; hedefi kontrol edin |
| Toplama başarılı ama öğe görünmüyor | İşçi çalışmıyor ya da aynı içerik zaten var (`content_hash`) | `docker compose ps osiris-pipeline`, `docker compose logs osiris-pipeline` |
| Kaynaklar kendiliğinden toplanmıyor | `OSIRIS_SCHEDULER` açık değil, kaynak devre dışı ya da cron boş | `.env` + `up -d osiris-collector`; kaynağı kontrol edin |
| Semantik arama `503` | Modeller yok ya da `OSIRIS_EMBEDDING_MODEL` boş | `osiris-models`'i çalıştırın; API'ye `./models` bağlı olmalı |
| Varlıklarda kişi/kurum yok | NER modeli yok ya da `OSIRIS_NER=off` | Modelleri indirin |
| `429 İstek kotası aşıldı` | Kimlik başına kota doldu | İstemci temposunu düşürün ya da `OSIRIS_API_RATE_PER_MIN`'i artırın |
| Uzun kullanıcı adı taramasında `504` | Nginx `proxy_read_timeout` 60 sn | `max_sites`'ı düşürün ya da `rate_per_sec`'i artırın |
| Tor/I2P toplamasında zaman aşımı | `darknet` profili açık değil ya da proxy henüz ağa bağlanmadı | `--profile darknet up -d`; Tor'un ağa bağlanması birkaç dakika sürebilir |
| `osiris-models` izin hatası | `./models` sahibi `OSIRIS_UID`'den farklı | `.env`'de `OSIRIS_UID`/`OSIRIS_GID`'i `id -u`/`id -g` yapın |

## 14. Bilinen sınırlamalar

- **Uyarı bildirimi yok:** uyarı kuralları kaydedilir ve `POST /alerts/test` ile denenir, ama
  işçi toplanan öğeleri kurallarla eşleştirip webhook/Telegram/e-posta göndermez. İşleyiciler
  `osiris-alert` kütüphanesinde hazırdır.
- **Graf kenarları elle eklenir:** işleme hattı aynı öğede geçen varlıklardan otomatik ilişki
  üretmez. (`entity_relations` tablosu şemada vardır ama kullanılmaz.)
- **`web-scraper` tek sayfa toplar:** `depth` ve `follow_links` ayarları kabul edilir ama
  bağlantılar izlenmez; `depth` yalnızca öğe üst verisine yazılır.
- **Uyarı eşleşmesi alt dizedir:** `query_type` eşleşme biçimini değiştirmez.
- **Tam metin arama kök bulmaz** (`simple` sözlüğü).
- **Tek düğüm:** bileşenler tek sunucu için tasarlanmıştır. Birden çok işçi aynı tüketici grubunu
  paylaşabilir (farklı `OSIRIS_PIPELINE_CONSUMER` ile), ama API'nin bellek içi grafı ve kota
  sayaçları süreç başınadır.
- **İki adımlı doğrulama, sır kasası, PDF rapor ve WASM plugin'leri yoktur** (yol haritasında).

## 15. Yol haritası

| Aşama | Kapsam | Durum |
|-------|--------|-------|
| Faz 1–5 | Çekirdek, 11 plugin, graf, semantik arama, uyarı kuralları, raporlar, JWT/RBAC, TLS | Tamam |
| Rust geçişi R0–R7 | Sözleşmelerin sabitlenmesi, Rust çekirdeği, veri katmanı, API/CLI, Python'un kaldırılması | Tamam ([kayıt](RUST_GECIS_DURUMU.md)) |
| Web arayüzü | 11 ekran, demo modu; semantik arama, kaynak düzenleme, graf analizi/dışa aktarım, çok biçimli rapor uçları | Tamam |
| Sıradaki | Uyarıların işçide yayımlanması, otomatik graf kenarları, 2FA, sır kasası, PDF rapor, WASM plugin'leri ([tasarım](WASM_PLUGIN_TASARIMI.md)), `:443` | Planlı |
