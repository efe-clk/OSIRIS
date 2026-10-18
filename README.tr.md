> **English:** [README.md](README.md)

# OSIRIS

> **O**pen **S**ource **I**ntelligence **R**esearch & **I**nformation **S**ystem

OSIRIS, kendi sunucunuzda çalışan bir açık kaynak istihbarat (OSINT) platformudur. Yüzey web,
RSS/Atom, REST API'ler, Shodan, DNS/WHOIS, blokzincirler, IRC, Matrix, Tor ve I2P'den veri
toplar; metni temizler, varlıkları (e-posta, IP, alan adı, kişi, kurum…) çıkarır, anlamsal
vektöre çevirir ve hepsini aranabilir kılar. Hiçbir veri üçüncü taraf bir buluta gitmez.

Sürüm **1.0.0-rc**. Sunucu Rust ile yazılmıştır (tek ikili, tek distroless Docker imajı);
tarayıcı arayüzü React/TypeScript'tir. Lisans GPL-3.0.

## Neler yapar?

| Yetenek | Ayrıntı |
|---------|---------|
| **Toplama** | 11 plugin: `rss`, `web-scraper`, `rest-api`, `shodan`, `dns-whois`, `blockchain`, `username-search` (~2 500 site), `tor`, `i2p`, `irc`, `matrix`. Tek seferlik ya da cron ile zamanlanmış; kaynak başına sağlık takibi. |
| **İşleme** | HTML temizleme ve içerik özetiyle tekilleştirme, 16 dilde dil tespiti, regex varlıkları + ONNX NER (kişi/kurum/yer), konu etiketleri, 384 boyutlu çok dilli vektörler. |
| **Arama** | Tam metin (PostgreSQL), semantik (pgvector; Türkçe sorgu İngilizce metni bulabilir), varlık bazlı. |
| **Analiz** | İlişki grafı: merkezilik, topluluk tespiti, GraphML/GEXF dışa aktarım. |
| **Rapor** | Markdown, HTML, JSON, STIX 2.1. |
| **Güvenlik** | TLS, roller (viewer/analyst/admin), kullanıcı anahtarları + JWT, kimlik başına kota, her çıkış isteğinde SSRF denetimi, hash zincirli denetim günlüğü. |
| **KVKK** | Kaynak başına saklama süresi, öğe ve kişi bazlı silme; silinen değer günlüğe yalnızca özet olarak yazılır. |
| **Arayüz ve otomasyon** | 11 ekranlı web arayüzü, REST API (OpenAPI), `osiris` CLI, n8n iş akışı şablonu, Prometheus ölçümleri. |

## Mimari

```
 Tarayıcı (Web UI) ─┐      n8n / CLI ─┐
                    ▼                 ▼
              Nginx (TLS :8443; /ui, /api, /n8n)
                    │
     ┌──────────────▼──────────────┐         ┌─────────────────────────┐
     │ osiris-server api           │         │ osiris-server collector │
     │ kimlik · RBAC · kota · uçlar│         │ cron zamanlayıcı        │
     └──────┬───────────────┬──────┘         └────────────┬────────────┘
            │ SQL           │ XADD                         │ XADD
            ▼               ▼                              ▼
     PostgreSQL 16     Redis Stream ──XREADGROUP──▶ osiris-server pipeline
     + pgvector        (kuyruk)                     temizle · varlık · vektör · depola
```

Tek imaj (`osiris:1.0.0-rc`) tüm rolleri alt komutla çalıştırır. Ayrıntı:
[docs/ARCHITECTURE.tr.md](docs/ARCHITECTURE.tr.md).

## Hızlı başlangıç

Gereksinim: Docker Engine + Compose v2, en az 4 GB RAM (modellerle 8 GB önerilir).

```bash
git clone https://github.com/efe-clk/OSIRIS.git && cd OSIRIS

cp .env.example .env && chmod 600 .env
#   POSTGRES_PASSWORD, OSIRIS_API_KEY, OSIRIS_JWT_SECRET, N8N_PASSWORD
#   → her biri için ayrı bir değer: openssl rand -hex 32

./deploy/nginx/make-certs.sh                                     # öz-imzalı TLS sertifikası
docker compose --profile models run --rm --build osiris-models   # NER + embedding (~420 MB, isteğe bağlı)
docker compose --profile full up -d --build                      # tüm servisler

curl -sk https://localhost:8443/api/health                       # {"service":"osiris-api","status":"ok"}
```

Ardından tarayıcıda `https://<sunucu>:8443/` adresini açın ve `OSIRIS_API_KEY` ile giriş yapın.
Tor/I2P için `--profile darknet` ekleyin. Zamanlanmış toplama `OSIRIS_SCHEDULER=on` yapılana
kadar kapalıdır.

API'yi doğrudan kullanmak:

```bash
curl -sk -X POST https://localhost:8443/api/collect \
  -H "X-API-Key: <OSIRIS_API_KEY>" -H 'Content-Type: application/json' \
  -d '{"plugin_id":"rss","config":{"feed_url":"https://example.com/feed.xml"}}'

curl -sk -X POST https://localhost:8443/api/search \
  -H "X-API-Key: <OSIRIS_API_KEY>" -H 'Content-Type: application/json' \
  -d '{"query":"fidye yazılımı","limit":10}'
```

Adım adım kurulum, yapılandırma, yedekleme ve yükseltme:
[docs/TEKNIK_DOKUMAN.tr.md](docs/TEKNIK_DOKUMAN.tr.md).

## Dokümantasyon

| Belge | İçerik |
|-------|--------|
| [Kurulum ve işletim kılavuzu](docs/TEKNIK_DOKUMAN.tr.md) | Gereksinimler, kurulum, tüm ortam değişkenleri, servisler, kimlik/roller, toplama, KVKK, yedek, yükseltme, sorun giderme |
| [API başvurusu](docs/API.tr.md) | Tüm uçlar, istek/yanıt örnekleri, sınırlar, hata kodları |
| [Plugin başvurusu](docs/PLUGINS.tr.md) | 11 plugin'in alanları, çıktıları ve sınırları |
| [Mimari](docs/ARCHITECTURE.tr.md) | Süreçler, veri akışı, veri modeli, crate'ler, güvenlik modeli |
| [Katkı rehberi](docs/CONTRIBUTING.tr.md) | Geliştirme ortamı, testler, yeni plugin, kod ve commit kuralları |
| [Davranış sözleşmesi](docs/SOZLESME.md) | Değişmemesi gereken kurallar (özet, denetim, kuyruk, SSRF, rapor) |
| [Web arayüzü](osiris-web-ui/README.md) | Ekranlar, geliştirme, demo modu |
| [Modeller](models/README.md) | NER ve embedding modelleri, lisanslar |
| [Rust geçiş kaydı](docs/RUST_GECIS_DURUMU.md) | Python → Rust geçişinin aşama aşama kaydı |

## Geliştirme

```bash
cargo test --workspace                                   # birim + altın fixture testleri
cargo clippy --workspace --all-targets -- -D warnings
cd osiris-web-ui && npm ci && npm run lint && npm run build
```

Entegrasyon, model ve API sözleşme testleri: [docs/CONTRIBUTING.tr.md](docs/CONTRIBUTING.tr.md).

## Lisans

GNU General Public License v3.0 — bkz. [LICENSE](LICENSE). NER modeli (AFL-3.0) depoyla
dağıtılmaz; kullanıcı tarafından indirilir (bkz. [models/README.md](models/README.md)).
