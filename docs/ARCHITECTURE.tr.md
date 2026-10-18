> **English:** [ARCHITECTURE.md](ARCHITECTURE.md)

# OSIRIS Mimarisi

Bu belge OSIRIS 1.0'ın nasıl kurulduğunu anlatır: hangi süreçlerin çalıştığını, verinin
toplamadan aramaya kadar izlediği yolu, veritabanı modelini, her crate'in sorumluluğunu ve
güvenlik modelini. Kurulum ve işletim için [TEKNIK_DOKUMAN.tr.md](TEKNIK_DOKUMAN.tr.md),
değişmemesi gereken davranış kuralları için [SOZLESME.md](SOZLESME.md).

## 1. Genel bakış

```
  Tarayıcı (React UI)          n8n / betikler / CLI
          │                           │
          ▼                           ▼
   ┌──────────────────── Nginx ─────────────────────┐
   │ TLS :8443 · güvenlik başlıkları · 10 istek/sn/IP │
   │ /ui/* → web-ui   /api/* → api   /n8n/* → n8n   │
   └───────────────────────┬─────────────────────────┘
                           ▼
            ┌─────────────────────────────┐
            │ osiris-server api           │  kimlik · RBAC · kota · doğrulama
            │ (axum · 29 yol · /metrics)  │  denetim günlüğü · bellek içi graf
            └──┬────────────┬─────────────┘
       SQL     │            │ XADD (toplanan öğeler)
               ▼            ▼
      ┌──────────────┐  ┌───────────────────────────┐   XADD   ┌────────────────────────┐
      │ PostgreSQL 16│  │ Redis Stream              │◄─────────│ osiris-server collector│
      │ + pgvector   │  │ osiris:stream:raw_items   │          │ cron zamanlayıcı       │
      └──────▲───────┘  └────────────┬──────────────┘          └────────────────────────┘
             │ tek işlem             │ XREADGROUP (grup: osiris-pipeline)
             │                       ▼
             │          ┌──────────────────────────────────────────┐
             └──────────│ osiris-server pipeline                   │
                        │ temizle → dil → regex + NER → konu →     │
                        │ vektör → depola → XACK+XDEL              │
                        └──────────────────────────────────────────┘
```

Tasarımın ana fikirleri:

- **Tek ikili, çok rol.** `osiris-server` alt komutla API, işçi ya da zamanlayıcı olur. Tek bir
  imaj (`osiris:1.0.0-rc`, distroless) üç servis olarak çalıştırılır; servisler arasında sürüm
  uyumsuzluğu oluşmaz.
- **Toplama ile işleme ayrık.** Plugin'ler yalnızca toplar ve kuyruğa yazar; ağır işler (NER,
  vektör) işçide yapılır. API'den tetiklenen bir toplama, işçi meşgulken de hızlı döner.
- **Veritabanı tek doğruluk kaynağı.** Kaynaklar, öğeler, varlıklar, graf, anahtarlar ve denetim
  günlüğü PostgreSQL'dedir; Redis yalnızca geçici kuyruktur.
- **Ağ erişimi host'tan geçer.** Plugin'ler soket açamaz; HTTP, DNS ve TCP için host'un denetimli
  yeteneklerini kullanır. SSRF ve proxy kuralları tek yerde uygulanır.
- **Davranış veriyle sabitlenir.** Hash, varlık çıkarma, rapor, graf, kuyruk ve API kuralları
  `fixtures/` ve `tests/contract/` ile test edilir ([SOZLESME.md](SOZLESME.md)).

## 2. Süreçler

| Servis | Alt komut | Görev |
|--------|-----------|-------|
| `osiris-api` | `api --addr 0.0.0.0:8000` | REST API (axum): kimlik çözme, rol denetimi, kimlik başına kota, girdi doğrulama, uçlar, OpenAPI (`/openapi.json`, `/docs`); `OSIRIS_METRICS_ADDR`'de ayrı Prometheus dinleyicisi |
| `osiris-pipeline` | `pipeline --loop --interval 5 --batch 50` | Akışı tüketir, öğeleri işler ve yazar; `OSIRIS_RETENTION_INTERVAL`'de bir saklama temizliği yapar |
| `osiris-collector` | `collector` | `OSIRIS_SCHEDULER=on` ise kaynakları cron'larına göre çalıştırır; değilse boşta bekler |

Tek seferlik alt komutlar:

| Alt komut | İş |
|-----------|----|
| `migrate` | Gömülü SQL migration'larını uygular (tekrar çalıştırılabilir) |
| `models [--only ner\|embedding]` | ONNX modellerini sabit revizyonlardan indirir, özetle doğrular |
| `audit-verify [--limit N]` | Denetim zincirinin son N kaydını doğrular (bozuksa çıkış 1) |
| `purge` | Saklama süresi dolan öğeleri ve yetim varlıkları siler |
| `collect --plugin P --config JSON [--enqueue]` | Tek toplama; `--enqueue` yoksa yalnızca sonucu basar |
| `pipeline --once` / `pipeline --reembed` | Kuyruktan bir tur işler / vektörsüz öğeleri vektörleştirir |
| `health [--addr]` | Konteyner sağlık denetimi (`GET /health` 200 → çıkış 0) |

Aynı imajdaki `osiris` CLI'ı: `status`, `plugins`, `collect <plugin> --config JSON` (çıkış
kodları: 0 başarı, 1 plugin yok ya da toplama başarısız, 2 hatalı girdi).

Tüm servisler `SIGTERM`'de (ör. `docker stop`) bekleyen işleri bitirip kapanır.

## 3. Veri akışı

### 3.1 Toplama

1. Tetik: API (`POST /collect`, `/collect/batch`, `/sources/{id}/collect`), zamanlayıcı ya da CLI.
2. `osiris_collector::Manager::run_collection` plugin'i bulur; manifest'teki zorunlu alanları ve
   plugin'in kendi doğrulamasını denetler (`Zorunlu alanlar eksik: …`).
3. Plugin ayrı bir Tokio görevinde çalışır; çökme ya da panik o toplamayı `plugin hatası`
   sonucuna çevirir, süreç ayakta kalır.
4. Plugin ağa yalnızca host yetenekleriyle çıkar (bkz. [§6](#6-plugin-host-modeli)).
5. Başarılı sonuçtaki her öğe bir **kuyruk mesajına** dönüşür ve `XADD MAXLEN ~ 200000` ile
   akışa yazılır. Mesaj `{"plugin_id", "source_id", "item", "collected_at"}` biçimindedir, en
   çok 512 000 bayt (`schemas/queue_message.schema.json`).
6. Yapılandırmada `source_id` varsa kaynak sağlığı (`sources`, `source_metrics`) yazılır.

### 3.2 İşleme

İşçi her turda sırasıyla:

1. Eski LIST kuyruğunu (`osiris:raw_items`, geçiş uyumluluğu için) boşaltır;
2. 5 dakikadır onaylanmamış mesajları `XAUTOCLAIM` ile devralır;
3. Yeni mesajları `XREADGROUP` ile okur (grup `osiris-pipeline`).

Her mesaj için:

| Adım | Ne yapar | Crate |
|------|----------|-------|
| Ayrıştır | JSON ve şema denetimi; bozuksa ölü-mektup akışına (`osiris:stream:raw_items:dead`) | `osiris-pipeline` |
| Temizle | HTML → metin, boşluk daraltma, ilk 200 000 kod noktası; `content_hash` = SHA-256 | `osiris-hash` |
| Dil | lingua, 16 dil (ISO 639-1 kodu; belirsizse `unknown`) | `osiris-pipeline::lang` |
| Varlık | Regex (e-posta, IP, alan adı, telefon, kripto adres, CVE) + ONNX NER (kişi, kurum, yer); `(tip, küçük harf değer)` ile tekilleştirme | `osiris-extract`, `osiris-pipeline::ner` |
| Konu | Anahtar kelimeyle etiket: `threat`, `breach`, `vulnerability`, `phishing` | `osiris-pipeline` |
| Vektör | fastembed; ilk 8 000 kod noktası, 128 token, mean pooling, L2 normalize, 384 boyut | `osiris-pipeline::embed` |
| Depola | Tek işlemde `items` (`ON CONFLICT content_hash DO NOTHING`), `entities` (upsert), `item_entities` | `osiris-db` |
| Onayla | `XACK` + `XDEL` | `osiris-pipeline::stream` |

Hata kuralları: bozuk mesaj → ölü-mektup (`reason=invalid: …`). Veritabanı hatası → mesaj
bekleyen listede kalır, 5 dakika sonra yeniden alınır; 5 teslimden sonra ölü-mektup
(`reason=max_deliveries`). Teslim **en az bir kez**dir; aynı mesajın iki kez işlenmesi zararsızdır.

### 3.3 Sorgulama ve sunum

- Arama, varlık, istatistik ve liste uçları sonuçları PostgreSQL'de `json_agg` ile JSON'a çevirir
  ve metni olduğu gibi döndürür (sütun sırası ve zaman biçimi sabit kalır).
- Semantik aramada sorgu vektörü API sürecinde üretilir; model ilk aramada yüklenir.
- Graf API sürecinde bellekte tutulur ve ilk kullanımda `graph_edges`'ten yüklenir. Analiz ve dışa
  aktarım bir kopya üzerinde, async çalışanları bloklamadan hesaplanır.
- Web arayüzü ve n8n yalnızca REST API'yi kullanır; veritabanına doğrudan bağlanan istemci yoktur.

## 4. Veri modeli

Şema `db/migrations/` altındadır. Migration'lar ikiliye gömülüdür, `osiris-server migrate` ile
uygulanır ve tümü tekrar çalıştırılabilir (`IF NOT EXISTS`).

| Tablo | İçerik | Önemli sütunlar |
|-------|--------|-----------------|
| `sources` | Kayıtlı kaynaklar | `plugin_id`, `network_type`, `url`, `schedule` (cron), `priority` 1–10, `enabled`, `tags`, `metadata.config` (plugin ayarları), sağlık (`last_crawled_at`, `last_success_at`, `failure_count`, `avg_response_ms`), `retention_days` |
| `items` | İşlenmiş öğeler | `source_id` (kaynak silinince öğe de silinir), `raw_content`, `cleaned_content`, `url`, `title`, `language`, `collected_at`, `published_at`, `content_hash` **UNIQUE**, `metadata`, `tags`, `embedding_v2 VECTOR(384)`, `embedding_model` |
| `entities` | Varlıklar | `type` (enum), `value`; `(type, value)` benzersiz; `first_seen_at`, `last_seen_at` |
| `item_entities` | Öğe–varlık bağı | `mention_count`; iki yönde de kaskad silme |
| `graph_edges` | Graf kenarları | `source_name`, `target_name`, `relation_type`, `weight`, `evidence_count` |
| `saved_queries` | Kayıtlı sorgular / uyarı kuralları | `query_text`, `query_type` (enum), `alert_enabled`, `last_triggered_at` |
| `users`, `api_keys` | Kullanıcılar ve anahtarları | `users.role` (enum); `api_keys.key_hash` (SHA-256), `key_prefix` (ilk 8 karakter), `revoked`, `last_used_at` |
| `audit_logs` | Hash zincirli denetim günlüğü | `user_id`, `action`, `resource`, `detail`, `prev_hash`, `hash`, `chain_version` |
| `source_metrics` | Kaynak çalıştırma geçmişi | `time`, `source_id`, `response_ms`, `success`, `items_count`; TimescaleDB varsa hypertable |
| `entity_relations` | Kullanılmıyor (ileride otomatik ilişkiler için ayrılmış) | |

Sayım türleri: `network_type` = `www, tor, i2p, p2p, freenet, zeronet, irc, matrix, rss, api,
blockchain, sdr, custom`; `entity_type` = `person, org, location, ip, domain, email, phone,
crypto_address, hash, username, cve, custom`; `query_type` = `fts, semantic, regex, entity,
graph`; `user_role` = `admin, analyst, viewer`.

İndeksler: tam metin için `to_tsvector('simple', cleaned_content)` üzerinde GIN; vektör için
`embedding_v2` üzerinde HNSW (kosinüs); ayrıca kaynak, tarih, dil, varlık tipi/değeri ve anahtar
özeti indeksleri.

| Migration | Değişiklik |
|-----------|------------|
| `001_init.sql` | Temel tablolar ve sayım türleri; TimescaleDB varsa `source_metrics` hypertable |
| `002_roles.sql` | `users`, `api_keys`, `graph_edges` |
| `003_audit_chain_version.sql` | `audit_logs.chain_version` |
| `004_retention.sql` | `sources.retention_days` (0–36 500, CHECK) |
| `005_embedding.sql` | `items.embedding_v2 VECTOR(384)` + HNSW, `embedding_model` |
| `006_r7_cleanup.sql` | Eski `items.embedding` (1536) sütununun kaldırılması; denetim varsayılan sürümü 2 |

## 5. Crate'ler

Cargo çalışma alanı `crates/` altındadır; araç zinciri `rust-toolchain.toml` ile sabittir ve
`unsafe` kod yasaktır.

```
osiris-server ─┬─ osiris-collector ─┬─ osiris-plugins ─┬─ osiris-plugin ─┬─ osiris-security
               │                    │                  │                 └─ osiris-types
               │                    │                  └─ osiris-feed
               │                    ├─ osiris-queue
               │                    └─ osiris-db
               ├─ osiris-pipeline ──┬─ osiris-extract
               │                    └─ osiris-db, osiris-queue, osiris-types
               ├─ osiris-query ──── osiris-db
               ├─ osiris-graph, osiris-alert, osiris-report
               └─ (hepsinin altında) osiris-hash
osiris-cli ──── osiris-collector, osiris-db, osiris-plugin
```

| Crate | Sorumluluk |
|-------|-----------|
| `osiris-server` | Servis ikilisi ve `api` modülü (rotalar, kimlik, girdi doğrulama, hatalar, ölçümler, OpenAPI) |
| `osiris-cli` | `osiris` komut satırı aracı |
| `osiris-collector` | Toplama çalıştırma, kaynaktan görev yapılandırması, kuyruğa yazma, sağlık kaydı, cron zamanlayıcı |
| `osiris-queue` | Kuyruk üreticisi (`XADD`); ML çalışma zamanını bağlamadan kullanılabilir |
| `osiris-pipeline` | Akış tüketicisi, işleme adımları, NER (`ort`), embedding (`fastembed`), dil tespiti, model indirici |
| `osiris-plugin` | `Collector` trait'i, host yetenekleri (denetimli HTTP/DNS/TCP, proxy'ler), fixture oynatıcı |
| `osiris-plugins` | 11 plugin: `rss`, `web-scraper`, `dns-whois`, `rest-api`, `shodan`, `blockchain`, `username-search`, `tor`, `i2p`, `matrix`, `irc` |
| `osiris-feed` | RSS/Atom ayrıştırma ve temizleme |
| `osiris-db` | PostgreSQL erişimi (sqlx), gömülü migration'lar, öğe yazma, denetim yazma/doğrulama, saklama, graf kenarları |
| `osiris-query` | Tam metin, semantik ve varlık araması |
| `osiris-graph` | Yönsüz varlık grafı, derece merkeziliği, CNM toplulukları, GraphML/GEXF |
| `osiris-alert` | Kural eşleşmesi, sessize alma, z-skor anomali, Redis yayını, webhook/Telegram/e-posta işleyicileri |
| `osiris-report` | Markdown, HTML (kaçışlı), JSON, CSV (formül koruması), STIX 2.1 |
| `osiris-hash` | `content_hash` temizleme, HTML metin çıkarma, kanonik JSON (RFC 8785), denetim zinciri özeti |
| `osiris-extract` | Regex varlık çıkarma |
| `osiris-security` | SSRF denetimi (URL politikası, DNS denetimi, IPv4-mapped IPv6) ve girdi temizleyiciler |
| `osiris-types` | Ortak türler: manifest, toplanan öğe, toplama sonucu, kuyruk mesajı (`schemas/` ile doğrulanır) |

Derleme zamanında gömülenler: `plugins/*/manifest.json`, `username-search` site veritabanları
(`sites.json`, `sites_full.json`) ve `db/migrations/*.sql`. Modeller gömülmez (bkz.
[models/README.md](../models/README.md)).

## 6. Plugin host modeli

Her plugin `osiris_plugin::Collector` trait'ini uygular:

```rust
#[async_trait]
pub trait Collector: Send + Sync {
    fn manifest(&self) -> &Manifest;
    async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash>;
    async fn health_check(&self, host: &Host, cfg: &Config) -> bool;
    /// İsteğe bağlı ek doğrulama: hatalı alan adlarını döndürür.
    fn validate_config(&self, _cfg: &Config) -> Vec<String> { Vec::new() }
}
```

`Host` plugin'e yalnızca şu yetenekleri verir:

| Yetenek | Kural |
|---------|-------|
| HTTP (`osiris_plugin::http::fetch`) | Yalnızca `http`/`https`; URL'de kimlik bilgisi yok; her yönlendirme adımında SSRF denetimi; en çok 3 yönlendirme; gövde `max_bytes` ile kesilir; açık zaman aşımı; bağlantı denetlenen IP'ye kurulur |
| DNS (`host.dns`) | Kayıt türü allowlist'i plugin'de; sonuç sayısı sınırlı |
| TCP (`host.safe_connect`) | Çöz → denetle → **doğrulanmış IP'ye** bağlan |
| Proxy | Tor (SOCKS5h) ve I2P (HTTP) yalnızca operatörün `OSIRIS_TOR_SOCKS` / `OSIRIS_I2P_HTTP_PROXY` adresleri; kullanıcı yapılandırması başka bir proxy host'u seçemez |

Sonuç `CollectionResult { success, items, error, metadata }`'dir; `success=false` ise öğe yoktur.
Beklenen hatalar `CollectionResult::fail`, beklenmeyenler `Err(PluginCrash)` ile döner; ikisi de
yalıtılır. Testlerde aynı `Host` arayüzü, kayıtlı HTTP/DNS/soket alışverişlerini oynatan sahte
bir host ile kurulur (`fixtures/plugins/*.json`); oynatıcı kayıtlı olmayan her isteği hata
sayar. Yeni plugin yazma: [CONTRIBUTING.tr.md](CONTRIBUTING.tr.md). WASM plugin'leri için
tasarım notu: [WASM_PLUGIN_TASARIMI.md](WASM_PLUGIN_TASARIMI.md).

## 7. Güvenlik modeli

| Katman | Önlem |
|--------|-------|
| **Çıkış (SSRF)** | Her plugin URL'si/host'u doğrulanır: özel (RFC 1918), loopback, link-local/metadata, CGNAT, TEST-NET, multicast, `0.0.0.0`, `::`, `fc00::/7`, `fe80::/10` engellidir; IPv4-mapped IPv6 açılarak denetlenir; DNS kayıtlarından biri bile engelliyse ya da host çözülemiyorsa istek reddedilir (fail-closed); ters eğik çizgi ve kontrol karakteri içeren URL'ler reddedilir. `.onion`/`.i2p` yalnızca ilgili plugin'de ve proxy üzerinden. |
| **Kimlik** | Operatör anahtarı (sabit süreli karşılaştırma); kullanıcı anahtarları (yalnızca SHA-256 özeti saklanır, ham değer bir kez gösterilir); HS256 JWT (algoritma sabit, ayrı sır, 1 saat). Anahtar yoksa API başlamaz (`OSIRIS_INSECURE_OPEN=1` hariç). |
| **Yetki** | `viewer` < `analyst` < `admin`; her uç en düşük rolünü kendisi denetler; bilinmeyen rol reddedilir. |
| **Kötüye kullanım** | Kimlik başına dakikalık kota (`OSIRIS_API_RATE_PER_MIN`), Nginx'te IP başına 10 istek/sn; 5 MiB gövde tavanı; alan uzunluk sınırları; `config` ≤ 50 anahtar ve ≤ 20 KB; liste uçlarında sayfa sınırları. |
| **Sırlar** | Kaynak `config`'indeki sır alanları okumada her derinlikte maskelenir, güncellemede maske eski değeri korur; plugin'ler sırları hataya, üst veriye ya da loga yazmaz. |
| **Denetim** | Yalnızca eklenen, hash zincirli `audit_logs` (RFC 8785 kanonik JSON, danışma kilitli yazıcılar); `audit-verify` ile doğrulanır. KVKK silmelerinde değer yerine SHA-256 özeti yazılır. |
| **Hatalar** | Yanıt gövdesi her zaman `{"detail": …}`; SQL, sürücü iletisi, yığın izi dışarı sızmaz (loga yazılır). |
| **Taşıma** | Nginx TLS 1.2/1.3, HSTS, CSP (`script-src 'self'`), `X-Frame-Options DENY`, `nosniff`, `Referrer-Policy no-referrer`. Uygulama portları yalnızca `127.0.0.1`'de; ölçümler yalnızca iç ağda. |
| **Çalışma zamanı** | Distroless imaj: kabuk ve paket yöneticisi yok, root olmayan kullanıcı (65532); modeller ve sırlar imajda değil; bağımlılıklar `cargo deny` ile denetlenir. |
| **Web arayüzü** | Kimlik bilgisi yalnızca sekmede (`sessionStorage`); satır içi betik yok; arayüzdeki rol kısıtları yalnızca kolaylıktır, asıl denetim sunucudadır. |

## 8. Gözlemlenebilirlik

- **Loglar:** `tracing`; `OSIRIS_LOG_FORMAT=json` ile JSON. Erişim logu (`osiris_api::access`)
  yöntem, rota şablonu, durum ve süreyi yazar; yol değeri ve sorgu dizgesi yazmaz.
- **Ölçümler:** API'nin ayrı dinleyicisinde Prometheus metni (istek sayısı/süresi, kota,
  denetim hataları, sürüm).
- **Sağlık:** `GET /health` (kimliksiz) ve `osiris-server health` (konteyner denetimi); kaynak
  sağlığı `sources` ve `source_metrics`'te.

## 9. Teknoloji seçimleri

| Alan | Seçim |
|------|-------|
| HTTP sunucu | axum 0.8, tower-http, governor (kota), utoipa (OpenAPI) |
| HTTP istemci | reqwest (rustls, gzip/deflate) |
| Veritabanı | PostgreSQL 16 + pgvector, sqlx |
| Kuyruk | Redis Streams (tüketici grubu) |
| ML | ONNX Runtime (`ort`) — mBERT-hrl NER (int8); fastembed — paraphrase-multilingual-MiniLM-L12-v2 |
| Dil tespiti | lingua |
| Arayüz | React 18, TypeScript, Tailwind, Vite (grafik kütüphanesi yok; graf canvas'ta çizilir) |
| Dağıtım | Docker Compose, distroless imaj, Nginx, n8n |

Rust'a geçişin gerekçesi, kararları ve aşama kaydı: [RUST_GECIS_PLANI.md](RUST_GECIS_PLANI.md),
[RUST_GECIS_DURUMU.md](RUST_GECIS_DURUMU.md).
