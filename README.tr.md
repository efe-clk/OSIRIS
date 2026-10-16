> **English:** For the English version of this document, see [here](README.md).

# OSIRIS

> **O**pen **S**ource **I**ntelligence **R**esearch & **I**nformation **S**ystem

OSIRIS, çoklu ağlardan (açık internet, karanlık ağlar, P2P, şifreli ağlar) veri toplayan, bu verileri ilişkilendirip analiz eden; tamamen self-host çalışan gelişmiş bir açık kaynak istihbarat (OSINT) platformudur.

Sistem, bir VDS üzerinde 7/24 kesintisiz çalışacak şekilde tasarlanmıştır. Tüm bileşenler kullanıcı kontrolündedir; dışarıya veri sızdırmaz, üçüncü taraf bulut servislerine bağımlı değildir.

## Temel Felsefe

- **Gizlilik önce gelir** — tüm bağlantılar proxy/VPN/Tor üzerinden yapılabilir
- **Modülerlik** — her veri kaynağı bağımsız bir plugin'dir; sistem durdurulmadan eklenir/çıkarılır
- **Genişletilebilirlik** — yeni ağ türleri, protokoller ve analiz yöntemleri kolayca entegre edilir
- **Şeffaflık** — kaynak kodu açık, her işlem loglanır, denetlenebilir

## Mimari Genel Bakış

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
└─────────────────────────────────────────────────────────────────┘
```

Katmanlar, veri akışı ve güvenlik modeli için bkz. [docs/ARCHITECTURE.tr.md](docs/ARCHITECTURE.tr.md).

## Modüller

| Modül | Dil | Rol | Durum |
|-------|-----|-----|-------|
| `osiris-core` | C++20 | Merkezi koordinatör, görev zamanlama | İskelet (zamanlayıcı + plugin yaşam döngüsü) |
| `osiris-collector` | Python | Toplama plugin'lerini yönetir | ✅ Çalışıyor |
| `osiris-pipeline` | Python | Temizleme, NER, sınıflandırma, embedding, depolama | ✅ Çalışıyor (embedding opsiyonel) |
| `osiris-query` | Python | Tam metin + semantik arama | ✅ Çalışıyor |
| `osiris-graph` | Python | Varlık ilişki grafı (kalıcı) | ✅ Çalışıyor |
| `osiris-alert` | Python | Uyarı + anomali tespiti | ✅ Çalışıyor |
| `osiris-report` | Python | Rapor üretimi ve dışa aktarım | ✅ MD/HTML/JSON/CSV/STIX |
| `osiris-api` | Python | REST API (JWT + RBAC) | ✅ Çalışıyor |
| `osiris-cli` | Python | Komut satırı arayüzü | ✅ Çalışıyor |
| `osiris-web-ui` | React/TS | Tarayıcı arayüzü | Temel ekranlar |
| `osiris-sdk` | Python | Paylaşılan SDK (plugin, model, güvenlik, denetim) | ✅ Çalışıyor |

## Teknoloji Stack'i

- **Diller:** C++20, Python 3.11+, TypeScript
- **Veritabanı:** PostgreSQL 16 (+ pgvector, TimescaleDB opsiyonel), Redis, MinIO
- **Arayüz:** CLI, REST API, React + Tailwind Web UI
- **Otomasyon:** N8N, FreshRSS, APScheduler
- **Güvenlik:** TLS (8443), API anahtarı + JWT + RBAC, SSRF korumaları, hash-zincirli denetim logları
- **Dağıtım:** Docker, Docker Compose, Nginx

## Hızlı Başlangıç

```bash
# Ortam şablonunu kopyalayıp sırları doldurun
cp .env.example .env

# TLS bootstrap sertifikasını üretin (yalnızca taze kurulum)
./deploy/nginx/make-certs.sh

# Altyapıyı başlat (PostgreSQL, Redis, MinIO, N8N, FreshRSS, Nginx)
docker compose up -d

# API + Web UI ile birlikte
docker compose --profile full up -d --build

# CLI'ı kurun
pip install -e ./osiris-cli

osiris --help
```

### API kimlik doğrulama

```bash
# Jeton üret (.env'deki operatör anahtarıyla)
curl -X POST http://localhost:80/api/auth/token \
  -H 'Content-Type: application/json' \
  -d '{"api_key":"<OSIRIS_API_KEY>","role":"analyst"}'

# Kullanım
curl -X POST http://localhost:80/api/search \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer <jeton>' \
  -d '{"query":"ornek","limit":5}'
```

Roller: `viewer` (okuma) → `analyst` (toplama + graf yazma) → `admin` (anahtar yönetimi).

## Testler

```bash
pip install -e ./osiris-sdk -e ./osiris-collector -e ./osiris-pipeline \
  -e ./osiris-graph -e ./osiris-alert -e ./osiris-report \
  -e ./osiris-query -e ./osiris-api -e ./osiris-cli
pip install pytest pytest-cov httpx
pytest -q  # %80 kapsam eşiği zorunlu
```

## Dokümantasyon

- Tam teknik doküman: [docs/TEKNIK_DOKUMAN.tr.md](docs/TEKNIK_DOKUMAN.tr.md) ([English](docs/TECHNICAL_DOCUMENT.md))
- API başvurusu: [docs/API.tr.md](docs/API.tr.md) ([English](docs/API.md))
- Mimari özet: [docs/ARCHITECTURE.tr.md](docs/ARCHITECTURE.tr.md)
- Katkı: [docs/CONTRIBUTING.tr.md](docs/CONTRIBUTING.tr.md)

## Lisans

GNU General Public License v3.0 — bkz. [LICENSE](LICENSE)
