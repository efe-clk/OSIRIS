> **English:** For the English version of this document, see [here](TECHNICAL_DOCUMENT.md).

# OSIRIS
## Açık Kaynak İstihbarat (OSINT) Platformu — Teknik Doküman

> **O**pen **S**ource **I**ntelligence **R**esearch & **I**nformation **S**ystem
> Sürüm: 0.5 — Mimari & Operasyon Dokümanı
> Durum: Üretim pilotu (self-host referans kurulum canlıda)

---

## İçindekiler

1. [Proje Özeti](#1-proje-özeti)
2. [Temel Özellikler](#2-temel-özellikler)
3. [Sistem Mimarisi](#3-sistem-mimarisi)
4. [Teknoloji Stack'i](#4-teknoloji-stacki)
5. [Modüller ve Bileşenler](#5-modüller-ve-bileşenler)
6. [Desteklenen Ağ ve Protokoller](#6-desteklenen-ağ-ve-protokoller)
7. [Kaynak Yönetim Sistemi](#7-kaynak-yönetim-sistemi)
8. [Veri Akışı](#8-veri-akışı)
9. [Veritabanı Tasarımı](#9-veritabanı-tasarımı)
10. [Güvenlik ve Gizlilik](#10-güvenlik-ve-gizlilik)
11. [Self-Host Altyapı Gereksinimleri](#11-self-host-altyapı-gereksinimleri)
12. [Plugin Mimarisi](#12-plugin-mimarisi)
13. [Arayüz Katmanları](#13-arayüz-katmanları)
14. [Otomasyon ve İş Akışları](#14-otomasyon-ve-iş-akışları)
15. [Yol Haritası Durumu](#15-yol-haritası-durumu)
16. [Operasyon: Test, Dağıtım, Yapılandırma](#16-operasyon-test-dağıtım-yapılandırma)

---

## 1. Proje Özeti

OSIRIS, çoklu ağlardan (açık internet, karanlık ağlar, P2P, şifreli ağlar) veri toplayan, ilişkilendirip analiz eden; tamamen self-host çalışan gelişmiş bir açık kaynak istihbarat (OSINT) platformudur.

Sistem bir VDS üzerinde 7/24 çalışır. Tüm bileşenler kullanıcı kontrolündedir; dışarı veri sızmaz, üçüncü taraf bulut bağımlılığı yoktur.

### Temel Felsefe

- **Gizlilik önce gelir** — tüm bağlantılar proxy/VPN/Tor üzerinden yapılabilir
- **Modülerlik** — her veri kaynağı bağımsız bir plugin'dir
- **Genişletilebilirlik** — yeni ağlar, protokoller ve analiz yöntemleri kolayca entegre edilir
- **Şeffaflık** — kaynak kodu açık, her işlem loglu ve denetlenebilir

---

## 2. Temel Özellikler

### 2.1 Veri Toplama
- Çoklu ağ desteği (WWW, Tor, I2P, RSS/Atom, REST API, IRC, Matrix, DNS/WHOIS, Shodan, blockchain)
- Paralel ve zamanlanmış çekme (APScheduler ile tam 5 alanlı cron)
- Plugin çöküş izolasyonu (bozulan plugin yöneticiyi düşürmez)
- Kuyruk taşma koruması (öğe başına 512 KB sınır, serileşemeyen atlanır)

### 2.2 Kaynak Yönetimi
- Kaynak sağlık takibi: `last_crawled_at`, `last_success_at`, `failure_count`, `avg_response_ms`
- `source_metrics` zaman serisi (TimescaleDB varsa hypertable)
- Kaynak başına öncelik ve cron zamanlama

### 2.3 Veri İşleme & Analiz
- HTML soyma, boşluk normalizasyonu, kodlama onarımı
- Dil tespiti, anahtar kelime konu sınıflandırma
- Varlık çıkarma: e-posta, IP (doğrulamalı), domain, telefon, kripto adresi, CVE + spaCy NER yedeği
- Opsiyonel semantik embedding (`OSIRIS_EMBEDDING_MODEL`, boyut uyumsuzluğunda pgvector yedeği)
- İçerik-hash tekilleştirme

### 2.4 Arama & Sorgulama
- Tam metin arama (PostgreSQL FTS + sıralama)
- Semantik arama (pgvector kosinüs)
- Varlık bazlı arama (LIKE kaçışlı)
- Doğrulanmış limitler (1–100), sorgu uzunluk sınırı

### 2.5 Görselleştirme
- İlişki grafı (merkezilik, topluluk tespiti, GraphML/GEXF dışa aktarım)
- `graph_edges` ile kalıcı graf kenarları (yeniden başlatmada uçmaz)

### 2.6 Raporlama & Dışa Aktarım
- Markdown, HTML (autoescape'li), JSON, CSV (formül-enjeksiyon korumalı)
- STIX 2.1 paketleri (identity/indicator/vulnerability, deterministik UUID)
- Çıktı dizin kilidi (path traversal yok), boyut sınırları

### 2.7 Otomasyon
- N8N iş akışı (`X-API-Key` başlıklı search + collect düğümleri, zaman aşımlı)
- Kayıtlı sorgu + uyarı işleyicilerle tetikleyici-eylem (e-posta/Telegram/webhook)
- Anomali tespiti (kayan z-skor tabanları, ısınma dönemi, sessize alma)

---

## 3. Sistem Mimarisi

```
┌─────────────────────────────────────────────────────────────────┐
│                        OSIRIS PLATFORM                          │
│                                                                 │
│  ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   │
│  │   CLI    │   │ Web UI   │   │   Core   │   │  REST    │   │
│  │          │   │(React/TS)│   │  (C++)   │   │  API     │   │
│  └────┬─────┘   └────┬─────┘   └────┬─────┘   └────┬─────┘   │
│       └──────────────┴──────────────┴──────────────┘          │
│                              │                                  │
│                    ┌─────────▼─────────┐                       │
│                    │   API Gateway /   │                        │
│                    │   Core Engine     │  (C++ + Python)        │
│                    └─────────┬─────────┘                       │
│              ┌───────────────┼───────────────┐                 │
│              │               │               │                  │
│    ┌─────────▼───┐  ┌────────▼──────┐  ┌────▼────────────┐    │
│    │  Collector  │  │   Processing  │  │   Storage       │    │
│    │  Manager    │  │   Pipeline    │  │   Layer         │    │
│    │  (Python)   │  │   (Python)    │  │   (PostgreSQL)  │    │
│    └──────┬──────┘  └───────────────┘  └─────────────────┘    │
│           │                                                      │
│    ┌──────▼──────────────────────────────────────────────┐      │
│    │              Plugin Koleksiyonu (11 plugin)          │      │
│    │  [WWW] [Tor] [I2P] [RSS] [API] [IRC] [Shodan] ...  │      │
│    └─────────────────────────────────────────────────────┘      │
│                                                                  │
│    ┌───────────────┐                         ┌──────────────┐   │
│    │   N8N         │                         │  Message     │   │
│    │  Otomasyon    │                         │  Queue       │   │
│    └───────────────┘                         │  (Redis)     │   │
│                                              └──────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

### Katman Açıklamaları

| Katman | Rol | Teknoloji |
|--------|-----|-----------|
| **Arayüz** | Kullanıcı etkileşimi | CLI (Click), Web UI (React), REST (FastAPI) |
| **API Gateway** | Bileşenleri bağlar, auth + doğrulama | Python/FastAPI (zamanlama: APScheduler, Python — `osiris-collector`) |
| **Core Engine** | Görev yönetimi, plugin yaşam döngüsü | C++20 |
| **Collector Manager** | Plugin orkestrasyonu, kuyruk iletimi | Python |
| **Processing Pipeline** | Temizle, zenginleştir, tekilleştir, depola | Python |
| **Storage Layer** | Kalıcı depolama | PostgreSQL + Redis |
| **Otomasyon** | İş akışları ve tetikleyiciler | N8N |
| **Feed Engine** | RSS/Atom yönetimi | `rss` plugin'i (FreshRSS R3'te kaldırıldı) |
| **Message Queue** | Asenkron bileşen iletişimi | Redis |

---

## 4. Teknoloji Stack'i

### 4.1 Diller

| Dil | Alan | Gerekçe |
|-----|------|---------|
| **C++** (C++20) | Core Engine | Bellek verimliliği, zamanlama |
| **Python 3.11+** | Plugin'ler, pipeline, API, analiz | Ekosistem, hız |
| **TypeScript** | Web UI | Tipli frontend |
| **SQL** | Sorgular, migration'lar | — |

### 4.2 Veritabanı & Depolama

| Teknoloji | Kullanım |
|-----------|----------|
| **PostgreSQL 16** | Ana ilişkisel depo |
| **pgvector** | Embedding'ler — semantik arama |
| **Redis** | Önbellek + `osiris:raw_items` kuyruğu + `osiris:alerts` pub/sub |
| **TimescaleDB** | Opsiyonel; varsa `source_metrics` hypertable (yoksa zarif yedek) |

### 4.3 Arayüz Teknolojileri

| Teknoloji | Kullanım | Durum |
|-----------|----------|-------|
| **CLI** (Click + Rich) | Operasyon, toplama işleri | Çalışıyor |
| **REST API** (FastAPI) | Birincil programatik arayüz | Çalışıyor (JWT + RBAC) |
| **Web UI** (React + TS + Tailwind) | Tarayıcı erişimi | Temel ekranlar |
| **Qt6** | Yerel masaüstü istemcisi | Planlı |

### 4.4 Ağ & Bağlantı
`requests` (HTTP, açık `verify=True`), `feedparser`, `dnspython`, SOCKS5 (Tor), düz HTTP proxy (I2P), ham soketler (zaman aşımlı IRC/WHOIS).

### 4.5 Veri İşleme & ML
`beautifulsoup4`, `langdetect`, `spaCy` (opsiyonel, zarif yedek), `sentence-transformers` (opsiyonel `osiris-pipeline[embedding]`), `networkx`, `pandas`.

### 4.6 Otomasyon & Entegrasyon
N8N (self-host, API anahtar başlıklı), APScheduler (cron), Redis pub/sub (uyarılar).

### 4.7 Güvenlik & Şifreleme
TLS 1.2/1.3 (Nginx `:8443`), SHA-256 API anahtar özetleri, HS256 JWT (yalnızca stdlib), hash-zincirli denetim logları, SSRF izin/engelle motoru, `.env` sırları (git-dışı, mod 600).

### 4.8 Konteyner & Dağıtım
Docker, Docker Compose (profiller: varsayılan altyapı, `api`, `ui`, `full`), Nginx ters proxy, takas edilebilir self-signed bootstrap sertifikası.

---

## 5. Modüller ve Bileşenler

### 5.1 Core Engine (`osiris-core`)
- **Dil:** C++20, `-Wall -Wextra -Wpedantic`, sıfır uyarı
- **Rol:** merkezi koordinatör
- **Gerçekleşen:** plugin kayıt/başlat/durdur, görev listesi, atomik durdurma bayraklı (`stop_scheduler`) 1 sn zamanlayıcı döngüsü, sürümlü CLI
- **Ertelenen:** tam cron ayrıştırma, IPC sunucusu (REST `osiris-api`'de)

### 5.2 Collector Manager (`osiris-collector`)
- Manifest doğrulamalı plugin yükleme (bozuklar loglanıp atlanır)
- Toplama çağrısı başına crash izolasyonu; iri/serileşemeyen öğeler atlanır
- `CronTrigger` ile tam 5 alanlı cron (`*/15` gibi adım değerleri destekli)
- Bayt sınırlı kuyruk iletimi; Redis hataları çökmeden raporlanır
- `source_id` verilmişse opsiyonel kaynak sağlık yazımı (`sources` + `source_metrics`)

### 5.3 Processing Pipeline (`osiris-pipeline`)
Aşamalar: temizle → dil → NER → sınıflandır → embed (opsiyonel) → tekilleştir → depola.
- `store()` upsert + `content_hash` tekilleştirme, varlık upsert'leri, `item_entities` sayaçları kullanır
- Embedding yazımı SAVEPOINT'lidir: boyut uyumsuzluğu vektörsüz yazıma düşer (karar önbelleklenir)

### 5.4 Query Engine (`osiris-query`)
- `fulltext_search`, `semantic_search` (vektör-literal uyarlamalı), `entity_search`
- `dict_row` sonuçları, limit kelepçesi (1–100, `<1` reddedilir), LIKE-joker kaçışı, varlık-türü allowlist'i

### 5.5 Graph Engine (`osiris-graph`)
- Ağırlıklı kenarlar, düğüm/kenar sınırları, limit aşımında `OverflowError`
- `centrality`, `communities`, `neighbors` (boş-graf güvenli)
- JSON/GraphML/GEXF dışa aktarım
- **Kalıcılık:** `graph_edges`'e `save_to_db()` / `load_from_db()`

### 5.6 Alert Manager (`osiris-alert`)
- Açma/sessize kurallı kayıtlı-sorgu alt-metin eşleşmesi
- Tembel Redis (çevrimdışı mod destekli), işleyici hata izolasyonu
- **Anomali tespiti:** metrik başına kayan tabanlar, z-skor eşiği (varsayılan 3.0), ısınma minimumu, sıfır-varyans ve NaN korumaları

### 5.7 Report Generator (`osiris-report`)
- Markdown, HTML (autoescape'li), JSON, CSV (formül korumalı), STIX 2.1
- Çıktı `output_dir` ile kilitli; iri raporlar reddedilir

### 5.8 REST API (`osiris-api`)

Tam uç başvurusu: [API.tr.md](API.tr.md).

Auth: `X-API-Key` (operatör) veya kullanıcı DB anahtarları veya `Authorization: Bearer <JWT>`. CORS allowlist, Pydantic limitleri, hatalarda iç detay yok, best-effort denetim yazımı.

### 5.9 SDK (`osiris-sdk`)
`plugin` (BaseCollector/CollectedItem/CollectionResult), `models` (Source/Entity/Item), `security` (SSRF + arındırıcılar), `audit` (ekleme/doğrulama hash zinciri, kilitli yazıcılar).

---

## 6. Desteklenen Ağ ve Protokoller

Gerçekleşen collector'lar (11): **web-scraper** (WWW), **tor** (localhost SOCKS5 ile `.onion`), **i2p** (localhost HTTP proxy ile eepsite), **rss** (sınırlı RSS/Atom), **rest-api** (metot allowlist, JSON-path), **irc** (enjeksiyon-arındırılmış, SSRF denetimli, PING-duyarlı, süre kutulu), **matrix** (oda geçmişi, URL-kodlu ID), **dns-whois** (doğrulanmış domain, allowlist kayıt türleri, sınırlı WHOIS), **shodan** (anahtar asla loglanmaz), **blockchain** (EVM `eth_blockNumber`/bakiye/txcount, allowlist Blockstream ile Bitcoin), **username-search** (~2500 gömülü sitede Maigret tarzı kullanıcı izi: Maigret + WhatsMyName birleşik DB, `tools/build_full_db.py` ile tazelenir).

Planlı: Freenet/ZeroNet/RetroShare, BitTorrent/IPFS, XMPP/Nostr/Fediverse, BGP/sertifika şeffaflığı, SDR/ADS-B/AIS, Monero sınır dokümantasyonu.

---

## 7. Kaynak Yönetim Sistemi

### 7.1 Kaynak Kaydı

```sql
sources (
  id UUID PRIMARY KEY,
  name TEXT NOT NULL,
  url TEXT,
  network_type network_type NOT NULL,   -- www|tor|i2p|p2p|freenet|zeronet|irc|
                                        -- matrix|rss|api|blockchain|sdr|custom
  plugin_id TEXT NOT NULL,
  auth_config JSONB,        -- kimlik bilgileri (Vault'a taşınacak)
  proxy_config JSONB,
  schedule TEXT,            -- 5 alanlı cron
  priority INTEGER DEFAULT 5,
  enabled BOOLEAN DEFAULT TRUE,
  tags TEXT[],
  last_crawled_at TIMESTAMPTZ,
  last_success_at TIMESTAMPTZ,
  failure_count INTEGER DEFAULT 0,
  avg_response_ms INTEGER,
  metadata JSONB,
  created_at TIMESTAMPTZ DEFAULT NOW(),
  updated_at TIMESTAMPTZ DEFAULT NOW()
)
```

### 7.2 Toplama Akışı
1. Operatör kaynak türü + plugin yapılandırır (UI'da şema-güdümlü form yol haritasında)
2. Opsiyonel bağlantı kontrolü (`health_check`)
3. Kaynak kaydedilir; Collector Manager ilk çekimi hemen veya cron ile tetikler
4. Sağlık panosu `sources` + `source_metrics` okur

---

## 8. Veri Akışı

```
[Kaynak]
    │
    ▼
[Collector Plugin]  (doğrulanmış config, zaman aşımı, boyut sınırı)
    │  ham öğeler
    ▼
[Redis osiris:raw_items]
    │
    ▼
[Processing Pipeline]
    ├─ Temizleme & Normalizasyon
    ├─ Dil Tespiti
    ├─ Varlık Çıkarma (regex + spaCy)
    ├─ Sınıflandırma (konular)
    ├─ Embedding (opsiyonel model)
    └─ Tekilleştirme (content_hash)
    │
    ▼
[PostgreSQL]
    │
    ├─ Query Engine ──────► UI / API
    ├─ Graph Engine ──────► Görselleştirme (+ kalıcı kenarlar)
    ├─ Alert Manager ────► Bildirimler (+ anomaliler)
    └─ Report Generator ► Markdown/HTML/JSON/CSV/STIX
```

---

## 9. Veritabanı Tasarımı

Migration `001_init.sql`: `sources`, `items` (`content_hash UNIQUE`, `embedding VECTOR(1536)`, FTS indeksi), `entities` (+ benzersiz `(type, value)`), `item_entities`, `entity_relations`, `saved_queries`, `audit_logs` (hash zinciri), `source_metrics` (TimescaleDB varsa hypertable, yoksa normal tablo).

Migration `002_roles.sql`: `user_role` enum'u, `users`, `api_keys` (yalnızca özet, önek indeksi, iptal bayrağı), `graph_edges` (bileşik PK, CHECK'ler, kaynak/hedef indeksleri).

TimescaleDB yokluğu ölümcül değildir (NOTICE + yedek), stok `pgvector:pg16` üzerinde doğrulandı.

---

## 10. Güvenlik ve Gizlilik

### 10.1 Operasyonel Güvenlik (OPSEC)
- Yapılandırılabilir proxy zincirleri; kaynak başına proxy profilleri (şema-hazır)
- Kodda localhost-zorunlu Tor/I2P proxy'si
- Fail-closed DNS: çözülemeyen host engellenir, izin verilmez

### 10.2 Veri Güvenliği
- Sırlar `.env`'de (git-dışı); DB parolaları `ALTER ROLE` runbook'uyla döndürülür
- API anahtarları SHA-256 saklanır; ham değer bir kez döndürülür, asla listelenmez
- Raporlar için imzalı/şifreli paketleme adımı (GPG) dışa aktarım akışında dokümante

### 10.3 Erişim Kontrolü
- Roller: viewer → analyst → admin (bilinmeyen reddedilir); uç başına zorunlu
- API anahtarları (operatör + kullanıcı) ve kısa ömürlü JWT'ler (HS256, sabit alg, sabit-süreli doğrulama)
- 2FA (TOTP): planlı, parolalı kullanıcılar sonrası

### 10.4 Denetim & Log
- Hash-zincirli `audit_logs` (eşzamanlı-yazıcı kilidi + `verify_chain` denetleyici)
- API toplama/arama/anahtar işlemleri best-effort loglanır (istekleri bozmaz)
- Sır enterpolasyonsuz Python loglama; C++ mutex-korumalı logger

---

## 11. Self-Host Altyapı Gereksinimleri

### 11.1 Minimum Sistem Gereksinimleri

| Bileşen | Minimum | Önerilen |
|---------|---------|---------|
| **CPU** | 4 çekirdek | 8-16 çekirdek |
| **RAM** | 8 GB | 32-64 GB (yerel embedding modeliyle) |
| **Depolama** | 100 GB SSD | 1 TB+ NVMe |
| **Bant Genişliği** | 100 Mbps | 1 Gbps |
| **OS** | Ubuntu 22.04 LTS | Ubuntu 24.04 LTS |

### 11.2 VDS Servis Haritası (Docker Compose)

```yaml
servisler:
  postgresql:        # ana DB (host 5432)
  redis:             # önbellek + kuyruk (host 6379)
  n8n:               # otomasyon (host 5678)
  osiris-api:        # REST API (127.0.0.1:8001 → 8000, profil: api/full)
  osiris-web-ui:     # arayüz (127.0.0.1:3001 → 3000, profil: ui/full)
  tor:               # Tor SOCKS5 istemcisi (iç ağ :9050, profil: darknet)
  i2pd:              # I2P HTTP proxy (iç ağ :4444, profil: darknet)
  nginx:             # ters proxy + TLS (:80, :8443)
```

Uygulama portları yalnızca localhost'a bağlıdır; dış erişim Nginx üzerinden.

### 11.3 Ağ Topolojisi

```
İnternet
   │
[Nginx] (:80, :8443 ssl)
   │
   ├── /api/*      → osiris-api:8000   (önek sıyrılır)
   ├── /n8n/*      → n8n:5678
   └── /ui/*       → osiris-web-ui:3000

[Dahili Docker Ağı]
   osiris-api ←→ postgresql / redis
   collector/pipeline ←→ redis (kuyruk)
   pipeline/query ←→ postgresql
```

Referans makinede `:443` başka bir ters proxy tarafından tutulduğu için TLS `:8443`'te sunulur (alan adı geçişine kadar; bkz. `deploy/nginx/README.md`).

---

## 12. Plugin Mimarisi

### 12.1 Plugin Yapısı

```
plugins/
└── web-scraper/
    ├── manifest.json     # meta veri + config_schema
    ├── collector.py      # toplama mantığı
    ├── requirements.txt  # Python bağımlılıkları
    └── README.md
```

### 12.2 manifest.json Örneği

```json
{
  "id": "web-scraper",
  "name": "Web Scraper",
  "version": "1.0.0",
  "network_type": "www",
  "config_schema": {
    "url": {"type": "string", "required": true},
    "css_selector": {"type": "string"}
  },
  "capabilities": ["html", "text"],
  "requires_proxy": false,
  "schedule_default": "0 */6 * * *"
}
```

### 12.3 Plugin Arayüzü (Python)

```python
from osiris.plugin import BaseCollector, CollectionResult

class MyCollector(BaseCollector):
    def collect(self, config: dict) -> CollectionResult:
        ...  # doğrula, çek, öğeleri döndür
        return CollectionResult(items=[...], metadata={...})

    def health_check(self) -> bool:
        ...
```

Güvenlik yükümlülükleri: URL'lerde `assert_safe_url`, ham soketlerde `safe_create_connection` (çöz → denetle → doğrulanmış IP'ye bağlan), host/domain/IRC jetonlarında `sanitize_*`, sır loglamama, sınırlı G/Ç.

---

## 13. Arayüz Katmanları

### 13.1 CLI
`osiris status|plugins|collect` — temiz çıkış kodları (0 tamam, 1 hata, 2 hatalı girdi).

### 13.2 REST API
Birincil arayüz (uç başvurusu: [API.tr.md](API.tr.md)). Makine istemcileri API anahtarı veya JWT kullanır.

### 13.3 Web UI
React + TypeScript + Tailwind: Dashboard, Kaynaklar, Arama. API-bağlı arama ve kaynak sağlık görünümleri sıradadır; statik derleme Nginx ile sunulur.

### 13.4 Masaüstü (planlı)
Qt6 güç istemcisi ve Tauri hafif istemcisi yol haritasındadır (§15).

---

## 14. Otomasyon ve İş Akışları

### 14.1 N8N Entegrasyonu
`deploy/n8n/osiris-workflow.json`: zamanlayıcı → arama → toplama; `X-API-Key` başlıkları (`{{ $env.OSIRIS_API_KEY }}` — n8n servisinde tanımlı olmalı) ve zaman aşımlı.

### 14.2 RSS
Akışlar `rss` plugin'i ile doğrudan toplanır. Bağımsız okuyucu olarak çalışan ve
hiçbir kodun kullanmadığı FreshRSS (ve nesne deposu MinIO) Rust geçişi R3'te
compose'dan kaldırıldı (K2/K3).

### 14.3 Webhook'lar & Uyarılar
Uyarı işleyiciler (e-posta/Telegram/webhook) `AlertManager.register_handler`'a takılır; anomali sıçramaları aynı kanaldan gider.

---

## 15. Yol Haritası Durumu

| Faz | Kapsam | Durum |
|-----|--------|------|
| **Faz 1** | Core iskeleti, DB şeması, ilk 3 plugin, pipeline, CLI | ✅ Bitti |
| **Faz 2** | Tor, I2P, IRC/Matrix, Shodan, DNS/WHOIS, Web UI temelleri | ✅ Bitti |
| **Faz 3** | Graf, semantik arama, uyarılar, raporlar | ✅ Bitti |
| **Faz 4** | Otomasyon, blockchain, JWT, STIX, sertleştirme, TLS | ✅ Bitti |
| **Faz 5.1** | Test kapsamı ≥%80 (CI eşiği) | ✅ Bitti (%90) |
| **Faz 5.2** | RBAC + graf kalıcılığı + anomali tespiti | ✅ Bitti |
| **Sıradaki** | 2FA, Vault sırları, PDF raporlar, Qt/Tauri istemcileri, :443 geçişi, doküman cilası | Planlı |

---

## 16. Operasyon: Test, Dağıtım, Yapılandırma

### 16.1 Test Matrisi (tamamı koşulabilir)

| Test | Komut | Eşik |
|------|-------|------|
| Python birim + kapsam | `pytest -q` | `--cov-fail-under=80` |
| Lint | `ruff check .` | sıfır bulgu |
| C++ derleme | `cmake --build` | `-Wall -Wextra -Wpedantic`, sıfır uyarı |
| Compose geçerlilik | `docker compose config` | çıkış 0 |
| DB migration | `psql -v ON_ERROR_STOP=1 -f db/migrations/*.sql` | stok `pgvector:pg16`'da temiz |
| Canlı API duman | TestClient + Nginx üzerinden curl | auth matrisi (401/403/200) |
| Web UI derleme | `npm run build` | `tsc` + `vite` temiz |

### 16.2 Ortam (`.env`, asla commitlenmez)

| Değişken | Amaç |
|----------|------|
| `POSTGRES_PASSWORD` | DB parolası (`ALTER ROLE` ile döndürülür, sonra API restart) |
| `N8N_USER/PASSWORD` | n8n kapısı (ilk kurulum değeri geçerlidir) |
| `OSIRIS_API_KEY` | Operatör anahtarı (admin). Zorunlu: yoksa API başlamaz |
| `OSIRIS_INSECURE_OPEN` | `1` ise anahtarsız açık mod (yalnızca güvenilir ağ) |
| `OSIRIS_JWT_SECRET` | JWT imza sırrı (yedeği yok; yoksa `/auth/token` 503) |
| `OSIRIS_CORS_ORIGINS` | İzinli tarayıcı kaynakları |
| `OSIRIS_RETENTION_DAYS` | KVKK varsayılan saklama süresi, gün (180; 0 = kapalı) |
| `OSIRIS_RETENTION_INTERVAL` | Pipeline döngüsünde temizlik aralığı, sn (3600; 0 = kapalı) |
| `OSIRIS_USERNAME_DB_DIR` | username-search `sites_db` için ek izinli dizin |
| `OSIRIS_TOR_SOCKS` | tor plugin'inin SOCKS5 proxy'si, `host:port` (compose: `tor:9050`) |
| `OSIRIS_I2P_HTTP_PROXY` | i2p plugin'inin HTTP proxy'si, `host:port` (compose: `i2pd:4444`) |

### 16.3 Dağıtım Notları
- Taze kurulum: `cp .env.example .env` (güçlü değerler üretin), `docker compose --profile full up -d --build`, `db/migrations/*.sql` dosyalarını sırayla uygulayın.
- Mevcut stack: uygulama servislerinde `up -d --no-deps` / `--no-recreate` tercih edin; salt-config değişiklikleri için DB konteynerını asla yeniden oluşturmayın.
- Canlı DB parola rotasyonu: `ALTER ROLE …`, `.env` güncelleme, yalnızca `osiris-api` recreate, operatör anahtarıyla `/api/plugins` doğrulama.

---

*Bu doküman gerçekleşen sistemi yansıtır. Her fazda güncelleyin.*
