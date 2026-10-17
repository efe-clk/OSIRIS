> **English:** For the English version of this document, see [here](ARCHITECTURE.md).

# OSIRIS Mimari Dokümanı

Bu doküman, OSIRIS platformunun yüksek seviyeli mimarisini ve modül sorumluluklarını özetler. Tam teknik şartname için bkz. [TEKNIK_DOKUMAN.tr.md](TEKNIK_DOKUMAN.tr.md).

## Katmanlar

```
┌────────────────────────────────────────────────────────────┐
│ Arayüz Katmanı                                             │
│  CLI · Web UI · REST API · (Qt masaüstü planlı)            │
└──────────────────────────┬─────────────────────────────────┘
                            │
┌──────────────────────────▼─────────────────────────────────┐
│ Çekirdek Katmanı (crates/, Rust — osiris_native/PyO3)      │
│  SSRF denetimi · content_hash + denetim zinciri · regex    │
└──────────────────────────┬─────────────────────────────────┘
                            │
┌──────────────────────────▼─────────────────────────────────┐
│ Veri Katmanı                                               │
│  Collector Manager → Plugin'ler → Pipeline → Depolama      │
└──────────────────────────┬─────────────────────────────────┘
                            │
┌──────────────────────────▼─────────────────────────────────┐
│ Analiz Katmanı                                             │
│  Query · Graph (kalıcı) · Alert (+anomali) · Report        │
└────────────────────────────────────────────────────────────┘
```

## Veri Akışı

1. **Toplama** — Collector Manager zamanlanmış plugin'leri çalıştırır (tam 5 alanlı cron).
2. **Kuyruk** — ham veri Redis Stream'e yazılır (`osiris:stream:raw_items`, consumer group, en az bir kez, öğe başına 512 KB sınır).
3. **İşleme** — pipeline temizler, dil tespiti yapar, varlık çıkarır, sınıflandırır, embedding üretir (opsiyonel model), içerik hash'iyle tekilleştirir.
4. **Depolama** — işlenmiş veri PostgreSQL'e yazılır (`items`, `entities`, `item_entities`, opsiyonel `embedding` vektörü).
5. **Analiz** — Query/Graph/Alert/Report katmanları veriyi tüketir; graf kenarları `graph_edges`'te kalıcıdır.
6. **Sunum** — arayüzler sonuçları REST API (JWT + RBAC) üzerinden gösterir.

## Modül Sorumlulukları

| Modül | Sorumluluk | Notlar |
|-------|-----------|--------|
| `crates/` (Rust) | SSRF denetimi, `content_hash`/denetim zinciri, regex çıkarma | Tüm Python servisleri `osiris_native` üzerinden kullanır (`OSIRIS_NATIVE`); zamanlama `osiris-collector`'da |
| `osiris-collector` | Plugin yükleme, görev çalıştırma, kuyruğa iletme | Crash izolasyonu, tam cron, kaynak sağlık takibi |
| `osiris-pipeline` | Temizleme, NER, sınıflandırma, embedding, depolama | Regex + ONNX NER (mBERT-hrl), anahtar kelime konuları, fastembed embedding (384 boyut); Redis Streams tüketicisi |
| `osiris-query` | Tam metin + semantik + varlık araması | `dict_row`, doğrulanmış limitler, LIKE kaçışı |
| `osiris-graph` | Varlık grafı, merkezilik, kümeleme | Bellek-içi + `graph_edges` kalıcılığı |
| `osiris-alert` | Kayıtlı sorgu eşleşme, anomali tespiti | Alt metin eşleşme, sessize alma, z-skor tabanları |
| `osiris-report` | Markdown/HTML/JSON/CSV/STIX 2.1 raporları | Jinja autoescape, CSV-enjeksiyon koruması, dizin kilidi |
| `osiris-api` | REST API sunucusu | CORS, giriş doğrulama, API anahtarı + JWT + RBAC, denetim kancaları |
| `osiris-cli` | Komut satırı arayüzü | `status`, `plugins`, `collect`; temiz çıkış kodları |
| `osiris-web-ui` | Tarayıcı arayüzü | Dashboard / Kaynaklar / Arama (temel) |
| `osiris-sdk` | `plugin`, `models`, `security` (SSRF korumaları), `audit` (hash zinciri) | Tüm Python modüllerince paylaşılır |

## Güvenlik Modeli

- **Çıkış kontrolü:** her plugin URL/host'u doğrulanır (`osiris.security`); özel/loopback/link-local/metadata IP'leri, URL içi kimlik bilgileri ve http(s) dışı şemalar reddedilir (fail-closed, çözülemeyen host dahil). Tor/I2P proxy'leri yalnızca localhost olabilir.
- **Kimlik doğrulama:** operatör `X-API-Key`, kullanıcı bazlı DB anahtarları (yalnızca SHA-256 özeti saklanır), kısa ömürlü HS256 JWT'ler (yalnızca stdlib, sabit `alg`, sabit-süreli doğrulama).
- **Yetkilendirme:** `viewer` → `analyst` → `admin` rolleri; bilinmeyen rol reddedilir.
- **Denetim:** eklemeli hash-zincirli `audit_logs` (`prev_hash` + kilitli yazıcılar, `verify_chain` denetleyici); API toplama/arama/anahtar işlemleri best-effort loglanır.
- **Taşıma:** Nginx `:8443`'te TLS sonlandırır (TLS 1.2/1.3, HSTS, CSP, hız sınırları); `:80` LAN/kurulum için durur.
- **Sırlar:** `.env` (git-dışı, mod 600); DB parolaları döndürülebilir; hiçbir sır loglanmaz veya API ile döndürülmez.
