> **English:** For the English version of this document, see [here](API.md).

# OSIRIS API Başvurusu

> Sürüm: 1.0.0-rc · Uygulama: Rust (`osiris-server api`) — sözleşme `tests/contract/` ile sabit
> İnteraktif doküman: `GET /docs` (Swagger UI), Nginx arkasında `/api/docs`; OpenAPI 3: `GET /openapi.json`
> Taban URL (üretim): `https://<sunucu>:8443/api` · Doğrudan (dev): `http://localhost:8001`

Tüm uçlar JSON döner. Zamanlar ISO-8601 UTC'dir. ID'ler UUID dizgisidir.

## İçindekiler

1. [Kimlik Doğrulama](#1-kimlik-doğrulama)
2. [Roller & Yetki Matrisi](#2-roller--yetki-matrisi)
3. [Uzlaşımlar](#3-uzlaşımlar)
4. [Sağlık](#4-sağlık)
5. [Auth Uçları](#5-auth-uçları)
6. [Plugin & Toplama](#6-plugin--toplama)
7. [Arama](#7-arama)
8. [Kaynaklar](#8-kaynaklar)
9. [Öğeler & Varlıklar](#9-öğeler--varlıklar)
10. [Graf](#10-graf)
11. [Uyarılar](#11-uyarılar)
12. [Raporlar](#12-raporlar)
13. [İstatistik](#13-istatistik)
14. [Hatalar](#14-hatalar)
15. [Hız Sınırları](#15-hız-sınırları)
16. [Değişiklikler](#16-değişiklikler)

---

## 1. Kimlik Doğrulama

İstek başına **bir** kimlik bilgisi gönderilir:

```bash
# Operatör / kullanıcı API anahtarı
curl -H "X-API-Key: <anahtar>" https://<sunucu>:8443/api/plugins

# Kısa ömürlü JWT (1 saat, HS256)
curl -H "Authorization: Bearer <jeton>" https://<sunucu>:8443/api/plugins
```

Jeton üretimi (operatör anahtarı gerekir):

```bash
curl -X POST https://<sunucu>:8443/api/auth/token \
  -H 'Content-Type: application/json' \
  -d '{"api_key":"<OSIRIS_API_KEY>","role":"analyst"}'
# → {"access_token":"...","token_type":"bearer","expires_in":3600}
```

Kullanıcı anahtarları (yalnızca admin) — ham değer **bir kez** döner, saklanmaz:

```bash
curl -X POST https://<sunucu>:8443/api/auth/keys \
  -H "X-API-Key: <OSIRIS_API_KEY>" -H 'Content-Type: application/json' \
  -d '{"username":"ali","role":"analyst","name":"laptop"}'
# → {"id":"...","api_key":"osiris_...","role":"analyst"}
```

`OSIRIS_API_KEY` tanımsızsa API **başlamaz**. Kimliksiz mod yalnızca `OSIRIS_INSECURE_OPEN=1` ile açıkça istenebilir (yalnızca güvenilir ağda; her çağıran `admin` sayılır).

JWT'ler **yalnızca** `OSIRIS_JWT_SECRET` ile imzalanır (API anahtarına düşme yedeği yok). Tanımsızsa `POST /auth/token` `503` döner ve Bearer jetonlar reddedilir; API anahtarları çalışmaya devam eder.

## 2. Roller & Yetki Matrisi

Roller yükselir: `viewer` → `analyst` → `admin`. Bilinmeyen rol reddedilir.

| Uç | viewer | analyst | admin |
|----|:------:|:-------:|:-----:|
| `GET /health` | açık | açık | açık |
| `POST /auth/token` | gövdede operatör anahtarı | — | — |
| `GET/POST /auth/keys`, `DELETE /auth/keys/{id}` | ✗ | ✗ | ✓ |
| `GET /plugins`, `GET /stats` | ✓ | ✓ | ✓ |
| `POST /search`, `/search/entity`, `GET /items/*`, `/entities/top` | ✓ | ✓ | ✓ |
| `GET /sources`, `/saved-queries`, `/graph` | ✓ | ✓ | ✓ |
| `POST /collect`, `/collect/batch` | ✗ (403) | ✓ | ✓ |
| `POST /sources`, `DELETE /sources/*`, `/sources/*/collect` | ✗ | ✓ | ✓ |
| `POST /saved-queries`, `DELETE`, `/alerts/test` | ✗ | ✓ | ✓ |
| `POST /graph/relation`, `/reports/markdown` | ✗ | ✓ | ✓ |
| `DELETE /items/{id}`, `POST /entities/erase`, `POST /admin/retention/run` | ✗ | ✗ | ✓ |

## 3. Uzlaşımlar

- **Sayfalama** (liste uçları): `?limit=` (varsayılan 100, en fazla 200) + `?offset=` (≥0). `/items/recent` en fazla 50.
- **Filtreler:** `GET /sources?enabled=true&plugin_id=rss&q=ad`; `GET /saved-queries?alert_enabled=true`; `GET /entities/top?entity_type=email&limit=20`.
- **Yoldaki ID'ler** UUID olmalı — değilse `404` döner (`500` asla).
- **İstek sınırları:** config nesneleri ≤ 50 anahtar ve serileşmiş ≤ 20 KB; sorgular ≤ 500 karakter; gövdelerde alan `max_length` sınırları.
- **Kısmi başarı:** `/collect/batch` her zaman `200` + iş başına `{ok, items, error}` ve `{ok, total}` özeti döner.

## 4. Sağlık

```bash
curl https://<sunucu>:8443/api/health
# {"status":"ok","service":"osiris-api"}
```

## 5. Auth Uçları

| Yöntem & Yol | Rol | Açıklama |
|--------------|-----|----------|
| `POST /auth/token` | gövdede operatör anahtarı | JWT üret (`role`: viewer/analyst/admin) |
| `POST /auth/keys` | admin | Kullanıcı anahtarı üret (`{username, role, name?}`) |
| `GET /auth/keys` | admin | Anahtarları listele (**yalnızca özet/önek**, ham değer asla) |
| `DELETE /auth/keys/{id}` | admin | İptal et (satır kalır — denetim izi) |

## 6. Plugin & Toplama

```bash
# Envanter + dinamik config şemaları (kaynak formlarını buradan besleyin)
curl -H "X-API-Key: <a>" https://<sunucu>:8443/api/plugins
# [{"id":"rss","name":"RSS/Atom Feed","network_type":"rss",
#   "description":"...","config_schema":{"feed_url":{...}},"schedule_default":"*/15 * * * *"}]

# Tek iş
curl -X POST https://<sunucu>:8443/api/collect -H "X-API-Key: <a>" \
  -H 'Content-Type: application/json' \
  -d '{"plugin_id":"rss","config":{"feed_url":"https://example.com/rss"}}'
# → {"items":3,"metadata":{...}}  (başarısızlıkta plugin mesajıyla 502)

# Toplu (≤10 iş, sıralı, iş başına sonuç)
curl -X POST https://<sunucu>:8443/api/collect/batch -H "X-API-Key: <a>" \
  -H 'Content-Type: application/json' \
  -d '{"jobs":[{"plugin_id":"rss","config":{...}},{"plugin_id":"dns-whois","config":{"domain":"example.com"}}]}'
# → {"jobs":[{"plugin_id":"rss","ok":true,"items":3,"error":null},...],"ok":1,"total":2}
```

## 7. Arama

```bash
curl -X POST https://<sunucu>:8443/api/search -H "X-API-Key: <a>" \
  -H 'Content-Type: application/json' -d '{"query":"saldırı","limit":20}'
# → [{"id":"...","title":"...","url":"...","cleaned_content":"...","collected_at":"..."}]

curl -X POST https://<sunucu>:8443/api/search/entity -H "X-API-Key: <a>" \
  -H 'Content-Type: application/json' -d '{"entity_type":"email","value":"a@b.co","limit":20}'
```

Varlık türleri: `person, org, location, ip, domain, email, phone, crypto_address, hash, username, cve, custom`.

## 8. Kaynaklar

```bash
# Liste (filtre + sayfalama)
curl -H "X-API-Key: <a>" "https://<sunucu>:8443/api/sources?enabled=true&limit=50"

# Oluştur — plugin'e özel ayarlar config'e (/plugins şemalarına bakın)
curl -X POST https://<sunucu>:8443/api/sources -H "X-API-Key: <a>" \
  -H 'Content-Type: application/json' \
  -d '{"name":"Örnek RSS","plugin_id":"rss","network_type":"rss",
       "url":"https://example.com/rss","schedule":"*/15 * * * *",
       "config":{"feed_url":"https://example.com/rss","max_items":50},
       "retention_days":90}'
# → {"id":"..."}

# Tek seferlik çalıştır (source_id işlenir → sources/source_metrics'e sağlık düşer)
curl -X POST https://<sunucu>:8443/api/sources/<id>/collect -H "X-API-Key: <a>"

# Sil
curl -X DELETE https://<sunucu>:8443/api/sources/<id> -H "X-API-Key: <a>"
```

Kaynak satırları sağlık taşır: `last_crawled_at`, `last_success_at`, `failure_count`, `avg_response_ms`.

`retention_days` (KVKK): verilmez/`null` → sunucu varsayılanı (`OSIRIS_RETENTION_DAYS`, 180), `0` → süreyle silinmez, `1-36500` → N günden eski öğeler silinir. Migration 004 henüz uygulanmadıysa `503` döner.

## 9. Öğeler & Varlıklar

```bash
curl -H "X-API-Key: <a>" "https://<sunucu>:8443/api/items/recent?limit=20&offset=0"
curl -H "X-API-Key: <a>" https://<sunucu>:8443/api/items/<id>        # + gömülü entities[]
curl -H "X-API-Key: <a>" "https://<sunucu>:8443/api/entities/top?entity_type=domain&limit=20"
# → [{"type":"domain","value":"example.com","mentions":12,"last_seen":"..."}]
```

### Gizlilik / silme (KVKK, yalnızca admin)

```bash
# Tek öğeyi sil (ve artık hiçbir öğeye bağlı olmayan varlıkları)
curl -X DELETE https://<sunucu>:8443/api/items/<id> -H "X-API-Key: <admin>"
# → {"status":"deleted","items_deleted":1,"entities_deleted":2}

# İlgili kişiyi sil: varlık + onu anan tüm öğeler + aynı adlı graf kenarları
curl -X POST https://<sunucu>:8443/api/entities/erase -H "X-API-Key: <admin>" \
  -H 'Content-Type: application/json' \
  -d '{"entity_type":"email","value":"ali@example.com","delete_items":true}'
# → {"items_deleted":4,"entities_deleted":3,"edges_deleted":1}

# Saklama temizliğini hemen çalıştır (normalde osiris-pipeline --loop yapar)
curl -X POST https://<sunucu>:8443/api/admin/retention/run -H "X-API-Key: <admin>"
# → {"items_deleted":120,"entities_deleted":37,"default_days":180}
```

Değer eşleşmesi büyük/küçük harf duyarsızdır. Denetim kaydına silinen değer değil, yalnızca `value_sha256` yazılır.

## 10. Graf

```bash
curl -H "X-API-Key: <a>" https://<sunucu>:8443/api/graph
# {"nodes":[{"id":"..."}],"edges":[{"source":"...","target":"...","relation_type":"..."}]}

curl -X POST https://<sunucu>:8443/api/graph/relation -H "X-API-Key: <a>" \
  -H 'Content-Type: application/json' \
  -d '{"source":"example.com","target":"1.2.3.4","relation_type":"resolves"}'
# graph_edges'e yazılır (yeniden başlatmada korunur)
```

## 11. Uyarılar

```bash
curl -X POST https://<sunucu>:8443/api/saved-queries -H "X-API-Key: <a>" \
  -H 'Content-Type: application/json' \
  -d '{"name":"Kritik","query_text":"saldırı","query_type":"fts","alert_enabled":true}'
# query_type ∈ fts|semantic|regex|entity|graph

# Metni kurallara karşı kuru-sıkı dene (kayıt oluşmaz)
curl -X POST https://<sunucu>:8443/api/alerts/test -H "X-API-Key: <a>" \
  -H 'Content-Type: application/json' -d '{"text":"büyük saldırı oldu"}'
# → [{"query_id":"...","query_name":"Kritik","item_id":null,"matched":"saldırı"}]
```

## 12. Raporlar

```bash
curl -X POST https://<sunucu>:8443/api/reports/markdown -H "X-API-Key: <a>" \
  -H 'Content-Type: application/json' \
  -d '{"title":"R","scope":"S","summary":"O",
       "findings":[{"title":"B","description":"D"}],"sources":["k"]}'
# → {"markdown":"# R\n..."}
```

## 13. İstatistik

```bash
curl -H "X-API-Key: <a>" https://<sunucu>:8443/api/stats
# {"sources":4,"sources_enabled":4,"items":128,"entities":342,"edges":57,
#  "saved_queries":3,"alerts":2,"latest_item_at":"..."}
```

## 14. Hatalar

| Kod | Anlam |
|-----|-------|
| 400 | Hatalı değer (`query_type`, `entity_type`, rol, ağ) |
| 401 | Eksik/geçersiz kimlik |
| 403 | Kimlik geçerli, rol yetersiz |
| 404 | Bilinmeyen plugin/kaynak/öğe/anahtar veya bozuk UUID |
| 409 | Kaynak devre dışı (`/sources/{id}/collect`) |
| 413 | Config nesnesi çok büyük ya da istek gövdesi 5 MiB üstü |
| 422 | Şema ihlali (ayrıntı `detail` dizisinde) |
| 429 | Kimlik başına kota aşıldı (`OSIRIS_API_RATE_PER_MIN`) |
| 500 | Sunucu/DB hatası (iç detay sızmaz) |
| 502 | Plugin çalıştı ama başarısız oldu (`detail` plugin mesajını taşır) |
| 503 | Özellik yapılandırılmamış (`OSIRIS_JWT_SECRET` olmadan `/auth/token`) |

## 15. Hız Sınırları

İki katman:

- **Nginx** (IP başına): `/api/*` üzerinde `10 istek/sn` (taşma 20).
- **API** (kimlik başına — operatör anahtarı, DB anahtarı kullanıcısı ya da JWT öznesi): dakikada `OSIRIS_API_RATE_PER_MIN` istek (varsayılan 600, `0` = kapalı); aşılınca `429` `{"detail":"İstek kotası aşıldı"}`.

Büyük toplu işlerde `/collect/batch` (≤10) kullanın ve istemcide tempo koyun.

**Ölçümler:** Prometheus metin biçimi ayrı dinleyicide, `OSIRIS_METRICS_ADDR` (compose: `osiris-api:9464/metrics`, yalnızca iç ağ — `/api/` altında değil). Etiketler rota şablonudur; yol değerleri ve sorgu dizgeleri asla yazılmaz.

## 16. Değişiklikler

- **1.0.0-rc (Rust R7)** — Python uygulaması kaldırıldı; sözleşme aynı. `osiris-server migrate` gömülü migration'ları uygular (006 eski `items.embedding` sütununu siler; denetim zinciri yalnızca v2).
- **Rust R6** — API Rust'ta (axum) yeniden yazıldı, sözleşme değişmedi; kimlik başına kota (`429`); `/metrics` (ayrı dinleyici); yapılandırılmış JSON log (`OSIRIS_LOG_FORMAT=json`); `/openapi.json`; zaman damgaları FastAPI biçiminde (`…Z`). `/graph/relation`'da kırpınca boş kalan kimlik artık `500` yerine `400` döner.
- **Yayımlanmadı (Rust R2)** — KVKK: `DELETE /items/{id}`, `POST /entities/erase`, `POST /admin/retention/run`, kaynaklarda `retention_days`; `OSIRIS_API_KEY` olmadan başlamaz; JWT sırrı API anahtarına düşmez (`/auth/token` → 503); audit zinciri v2 (RFC 8785).
- **0.2.0** — listelerde sayfalama + filtreler; `GET /items/{id}` (+varlıklar), `GET /entities/top`, `POST /collect/batch`; UUID-güvenli id yolları; OpenAPI etiket/özetleri; `q` filtresinde LIKE-joker kaçışı.
- **0.1.x** — sources CRUD + çalıştırma tetikleme, saved-queries CRUD, `/alerts/test`, `/reports/markdown`, `/stats`, `/items/recent`, JWT + RBAC, `/auth/keys`.
