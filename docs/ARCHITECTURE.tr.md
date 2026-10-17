> **English:** See [ARCHITECTURE.md](ARCHITECTURE.md) for the English version of this document.

# OSIRIS Mimarisi

Bu belge OSIRIS 1.0'ın (Rust) mimarisini ve her crate'in sorumluluğunu özetler. Kurulum ve işletim: [TEKNIK_DOKUMAN.tr.md](TEKNIK_DOKUMAN.tr.md). Davranış sözleşmeleri (hash, denetim, kuyruk, API, …): [SOZLESME.md](SOZLESME.md).

## Süreçler

Tek ikili (`osiris-server`), tek distroless imajda (`osiris:1.0.0-rc`). Compose onu farklı alt komutlarla üç kez çalıştırır:

| Servis | Komut | Görev |
|--------|-------|-------|
| `osiris-api` | `api` | REST API (axum), kimlik/RBAC, kota, OpenAPI, ayrı iç portta `/metrics` |
| `osiris-pipeline` | `pipeline --loop` | Redis Stream'i tüketir, öğeleri işler ve depolar, periyodik saklama temizliği |
| `osiris-collector` | `collector` | Kaynakları cron zamanlamasıyla çalıştırır (`OSIRIS_SCHEDULER=on` olmadıkça kapalı) |

Tek seferlik alt komutlar: `migrate` (gömülü, tekrar çalıştırılabilir SQL migration'ları), `models` (ONNX modellerini indir + doğrula), `audit-verify` (denetim zincirini doğrula), `purge` (saklama temizliği şimdi), `collect` (tek çalıştırma), `health` (konteyner sağlık denetimi). `osiris` CLI (`status`, `plugins`, `collect`) aynı imajdadır.

## Veri Akışı

1. **Toplama** — collector (zamanlanmış) ya da API (`/collect`, `/sources/{id}/collect`) bir plugin çalıştırır. Plugin'ler kendileri soket açmaz: HTTP, DNS ve TCP host'un denetimli yeteneklerinden geçer (her adımda SSRF denetimi, yönlendirme/boyut/süre sınırları, Tor/I2P yalnızca operatörün proxy'leriyle). Kaynak sağlığı `sources` / `source_metrics`'e yazılır.
2. **Kuyruk** — toplanan her öğe Redis Stream `osiris:stream:raw_items`'e JSON mesaj olarak yazılır (`XADD MAXLEN ~`, ≤ 512 KB). Teslim en az bir kez: tüketici grubu `osiris-pipeline`, 5 dk boşta kalan mesaj `XAUTOCLAIM` ile geri alınır, 5 teslimden sonra ya da bozuk mesajda ölü-mektup akışı.
3. **İşleme** — temizleme (`content_hash` normalizasyonu), dil tespiti (lingua), varlık çıkarma (regex + ONNX NER, mBERT-hrl), konu sınıflandırma, vektör (fastembed, 384 boyut, L2-normalize).
4. **Depolama** — tek işlem: `items` (`content_hash` ile tekilleştirilir), `entities`, `item_entities`; vektörler `items.embedding_v2`.
5. **Analiz** — tam metin / semantik / varlık araması, varlık grafı (`graph_edges`), kayıtlı sorgu uyarıları, raporlar.
6. **Sunum** — Web UI ve n8n REST API'yi Nginx üzerinden çağırır (`/api/*`).

## Crate'ler

| Crate | Sorumluluk |
|-------|-----------|
| `osiris-server` | İkili + `api` modülü (rotalar, kimlik, girdi doğrulama, ölçümler, OpenAPI) |
| `osiris-cli` | `osiris` CLI |
| `osiris-collector` | Toplama çalıştırma, kaynaktan görev yapılandırması, kuyruğa yazma, sağlık kaydı, cron zamanlayıcı |
| `osiris-queue` | Kuyruk üreticisi (`XADD`) — collector, API ve CLI'ın ML çalışma zamanını bağlamadan kullanması için |
| `osiris-pipeline` | Akış tüketicisi, işleme adımları, NER (`ort`), embedding (`fastembed`), model indirici |
| `osiris-plugin` | `Collector` trait'i, host yetenekleri (denetimli HTTP/DNS/TCP, proxy'ler), fixture oynatıcı |
| `osiris-plugins` | 11 plugin: rss, web-scraper, dns-whois, rest-api, shodan, blockchain, username-search, tor, i2p, matrix, irc |
| `osiris-feed` | RSS/Atom ayrıştırma ve temizleme (feedparser uyumlu) |
| `osiris-db` | PostgreSQL erişimi (sqlx), gömülü migration'lar, denetim yazma/doğrulama, saklama, graf kenarları |
| `osiris-query` | Tam metin (tsvector), semantik (pgvector kosinüs), varlık araması |
| `osiris-graph` | Yönsüz varlık grafı, derece merkeziliği, Clauset–Newman–Moore toplulukları, GraphML/GEXF |
| `osiris-alert` | Kayıtlı sorgu eşleşmesi, sessize alma, z-skor anomali, Redis yayını, webhook / Telegram / e-posta |
| `osiris-report` | Markdown/HTML (kaçışlı), JSON, CSV (formül koruması, dizin kilidi), STIX 2.1 |
| `osiris-hash` | `content_hash` temizleme, HTML metin çıkarma, kanonik JSON (RFC 8785), denetim zinciri özeti |
| `osiris-extract` | Regex varlık çıkarma (e-posta, IP, alan adı, telefon, kripto adres, CVE) |
| `osiris-security` | SSRF denetimi (URL politikası, DNS denetimi, IPv4-mapped IPv6) ve girdi temizleyiciler |
| `osiris-types` | Ortak türler: manifest, toplanan öğe, toplama sonucu, kuyruk mesajı (JSON Şemaları `schemas/`) |

Plugin manifest'leri (`plugins/*/manifest.json`) ve username-search site veritabanları derleme zamanında gömülür. WASM plugin arayüzü yalnızca tasarım notudur: [WASM_PLUGIN_TASARIMI.md](WASM_PLUGIN_TASARIMI.md).

## Güvenlik Modeli

- **Çıkış denetimi:** her plugin URL'si/host'u doğrulanır (`osiris-security`); özel/loopback/link-local/metadata adresleri, URL'de kimlik bilgisi, ters eğik çizgi/kontrol karakteri ve http(s) dışı şemalar reddedilir — çözülemeyen host dahil fail-closed. Bağlantı, denetlenen IP'ye kurulur. `.onion` / `.i2p` yalnızca yapılandırılmış proxy'lerle.
- **Kimlik doğrulama:** operatör `X-API-Key` (sabit süreli karşılaştırma), kullanıcı başına API anahtarları (yalnızca SHA-256 özeti saklanır), kısa ömürlü HS256 JWT (`alg` sabit, ayrı sır, API anahtarına düşme yok). Anahtar yoksa API başlamaz (`OSIRIS_INSECURE_OPEN=1` hariç).
- **Yetkilendirme:** roller `viewer` → `analyst` → `admin`; bilinmeyen rol reddedilir.
- **Kötüye kullanım sınırları:** kimlik başına kota (`OSIRIS_API_RATE_PER_MIN`), 5 MiB gövde tavanı, alan uzunluk sınırları, config ≤ 50 anahtar / 20 KB, ayrıca Nginx IP sınırı.
- **Denetim:** append-only, hash zincirli `audit_logs` (RFC 8785 yükü, danışma kilitli yazıcılar); `osiris-server audit-verify` zinciri doğrular. KVKK kapsamında silinen kişisel veri günlüğe yalnızca SHA-256 özeti olarak yazılır.
- **Hatalar:** yanıt gövdesi her zaman `{"detail": …}`, iç ayrıntı (SQL, sürücü iletisi, yığın izi) içermez.
- **Taşıma:** Nginx TLS'i `:8443`'te sonlandırır (TLS 1.2/1.3, HSTS, CSP); `:80` LAN/kurulum için. Ölçümler yalnızca iç portta.
- **Çalışma zamanı:** distroless imaj, root olmayan kullanıcı, kabuk yok; modeller ve sırlar imaja gömülmez.
