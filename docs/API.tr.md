> **English:** [API.md](API.md)

# OSIRIS REST API Başvurusu

> Sürüm 1.0.0-rc · Uygulama: `osiris-server api` (Rust, axum) · Bağlayıcı tanım: `tests/contract/`

Bu belge API'nin tüm uçlarını, istek/yanıt biçimlerini, sınırlarını ve hata kodlarını anlatır.
Örneklerdeki değerler uydurmadır (`example.com`, `203.0.113.0/24` belgeleme blokları).

## İçindekiler

1. [Temeller](#1-temeller)
2. [Roller ve yetki matrisi](#2-roller-ve-yetki-matrisi)
3. [Ortak kurallar](#3-ortak-kurallar)
4. [Sağlık ve kimlik](#4-sağlık-ve-kimlik)
5. [Plugin'ler ve toplama](#5-pluginler-ve-toplama)
6. [Arama](#6-arama)
7. [Öğeler ve varlıklar](#7-öğeler-ve-varlıklar)
8. [Kaynaklar](#8-kaynaklar)
9. [Graf](#9-graf)
10. [Kayıtlı sorgular ve uyarılar](#10-kayıtlı-sorgular-ve-uyarılar)
11. [Raporlar](#11-raporlar)
12. [Gizlilik ve saklama (KVKK)](#12-gizlilik-ve-saklama-kvkk)
13. [İstatistik](#13-istatistik)
14. [Hatalar](#14-hatalar)
15. [Hız sınırları ve ölçümler](#15-hız-sınırları-ve-ölçümler)
16. [Değişiklik geçmişi](#16-değişiklik-geçmişi)

---

## 1. Temeller

| | |
|---|---|
| Taban adres (Nginx üzerinden) | `https://<sunucu>:8443/api` — Nginx `/api` önekini sıyırır |
| Doğrudan (yalnızca sunucuda / SSH tüneliyle) | `http://127.0.0.1:8001` |
| İç Docker ağı (n8n, diğer servisler) | `http://osiris-api:8000` |
| OpenAPI 3 belgesi | `GET /openapi.json` |
| Swagger UI | `GET /docs` |
| Biçim | İstek ve yanıt gövdeleri JSON (`Content-Type: application/json`); graf dışa aktarımı XML |
| Zaman | UTC, ISO 8601: `2026-10-18T12:49:29.085604Z` (mikrosaniye sıfırsa kesirsiz) |
| Kimlikler | UUID dizgesi |

> **Swagger UI ve Nginx:** `/docs` sayfası Swagger UI dosyalarını `cdn.jsdelivr.net`'ten ve satır
> içi bir betikle yükler. Nginx'in Content-Security-Policy başlığı (`default-src 'self'`) buna
> izin vermediği için `https://<sunucu>:8443/api/docs` tarayıcıda boş açılır. Etkileşimli doküman
> için API'ye doğrudan erişin (`ssh -L 8001:127.0.0.1:8001 sunucu` → `http://127.0.0.1:8001/docs`)
> ya da `/api/openapi.json`'ı bir OpenAPI görüntüleyicisine yükleyin.

### 1.1 Kimlik doğrulama

Her istekte **bir** kimlik bilgisi gönderin:

```bash
# API anahtarı (operatör anahtarı ya da kullanıcı anahtarı)
curl -H "X-API-Key: <anahtar>" https://<sunucu>:8443/api/plugins

# Kısa ömürlü JWT
curl -H "Authorization: Bearer <jeton>" https://<sunucu>:8443/api/plugins
```

API kimliği şu sırayla çözer:

1. `X-API-Key` operatör anahtarına (`OSIRIS_API_KEY`) eşitse → kullanıcı `operator`, rol `admin`.
2. `Authorization: Bearer …` geçerli bir JWT ise → kullanıcı `api-client`, rol jetondaki rol.
3. `X-API-Key` iptal edilmemiş bir kullanıcı anahtarının özetine eşitse → anahtarın kullanıcısı
   ve onun rolü (anahtarın `last_used_at` alanı güncellenir).

Hiçbiri tutmazsa `401 {"detail":"Geçersiz kimlik bilgisi"}`.

- `OSIRIS_API_KEY` tanımsızsa API **başlamaz**. Tek istisna `OSIRIS_INSECURE_OPEN=1`'dir: o
  zaman kimlik denetimi yapılmaz ve her çağıran `anonymous`/`admin` sayılır (yalnızca yalıtılmış
  geliştirme ağı için).
- JWT'ler **yalnızca** `OSIRIS_JWT_SECRET` ile imzalanır ve doğrulanır (HS256, algoritma sabit).
  Sır tanımsızsa `POST /auth/token` `503` döner ve tüm Bearer jetonlar reddedilir; API anahtarları
  çalışmaya devam eder. Süre denetiminde 60 saniyelik saat kayması toleransı vardır.

## 2. Roller ve yetki matrisi

Roller birbirini kapsar: `viewer` < `analyst` < `admin`. Rolü yetmeyen çağrı
`403 {"detail":"Yetki yetersiz"}` alır.

| Uç | En düşük rol |
|----|--------------|
| `GET /health`, `GET /openapi.json`, `GET /docs` | kimliksiz |
| `POST /auth/token` | gövdede operatör anahtarı |
| `GET /auth/me` | viewer |
| `GET /plugins`, `GET /stats` | viewer |
| `GET /items/recent`, `GET /items/{id}`, `GET /entities/top` | viewer |
| `POST /search`, `POST /search/semantic`, `POST /search/entity` | viewer |
| `GET /graph`, `GET /graph/analysis`, `GET /graph/export` | viewer |
| `GET /sources`, `GET /sources/{id}`, `GET /saved-queries` | viewer |
| `POST /collect`, `POST /collect/batch` | analyst |
| `POST /sources`, `PATCH /sources/{id}`, `DELETE /sources/{id}`, `POST /sources/{id}/collect` | analyst |
| `POST /saved-queries`, `DELETE /saved-queries/{id}`, `POST /alerts/test` | analyst |
| `POST /graph/relation` | analyst |
| `POST /reports/markdown`, `POST /reports/generate` | analyst |
| `GET /auth/keys`, `POST /auth/keys`, `DELETE /auth/keys/{id}` | admin |
| `DELETE /items/{id}`, `POST /entities/erase`, `POST /admin/retention/run` | admin |

## 3. Ortak kurallar

**İşlem sırası.** Her uç sırasıyla: JSON gövdeyi ayrıştırır → kimliği ve rolü denetler → kotayı
düşer → alanları doğrular (`422`) → iş kurallarını uygular (`400/404/409/413/502/503`) → yanıt
verir. Denetim kaydı yanıttan önce, en iyi çabayla yazılır; yazılamazsa istek bozulmaz.

**Sayfalama.** Liste uçları `?limit=` ve `?offset=` alır:

| Uç | `limit` varsayılan | `limit` aralığı | Not |
|----|-------------------|-----------------|-----|
| `GET /sources`, `GET /saved-queries` | 100 | 1–200 (aralık dışı kırpılır) | `offset < 0` → `422` |
| `GET /items/recent` | 20 | 1–50 (kırpılır) | negatif `offset` 0 sayılır |
| `GET /entities/top` | 20 | 1–100 (kırpılır) | sayfalama yok |
| `POST /search*` | 20 | 1–100 | aralık dışı → `422` |

**Yol kimlikleri.** `{id}` yerine UUID biçiminde olmayan bir değer `404` döner (asla `500` değil).

**Esnek tür kabulü.** Tamsayı ve mantıksal alanlar dize olarak da gelebilir (n8n gibi araçlar
için): `"20"` → 20, `5.0` → 5; `"true"`, `"1"`, `"yes"`, `"on"` → true; `"false"`, `"0"`,
`"no"`, `"off"` → false.

**Boyut sınırları.** İstek gövdesi ≤ 5 MiB (Nginx'te de aynı). Her `config` nesnesi ≤ 50 anahtar,
serileştirilmiş hâli ≤ 20 000 karakter (aşarsa `413`). Alan uzunluk sınırları her uçta yazılıdır.

**Kısmi başarı.** `POST /collect/batch` her zaman `200` döner; işlerin sonucu tek tek raporlanır.

## 4. Sağlık ve kimlik

### `GET /health` — kimliksiz

```json
{"service": "osiris-api", "status": "ok"}
```

### `POST /auth/token` — JWT üretir

Gövde: `api_key` (zorunlu, operatör anahtarı, ≤ 500), `role` (`viewer` varsayılan; `viewer`,
`analyst`, `admin`).

```bash
curl -X POST https://<sunucu>:8443/api/auth/token -H 'Content-Type: application/json' \
  -d '{"api_key":"<OSIRIS_API_KEY>","role":"analyst"}'
```
```json
{"access_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9…", "token_type": "bearer", "expires_in": 3600}
```

Hatalar: `401` (anahtar yanlış), `400 Geçersiz rol`, `503` (`OSIRIS_JWT_SECRET` yok). Jeton
yalnızca operatör anahtarıyla üretilebilir; kullanıcı anahtarları jeton üretemez.

### `GET /auth/me` — çağıranın kimliği (viewer)

Yan etkisizdir (yalnızca kota sayılır). Web arayüzü giriş sırasında rolü buradan öğrenir.

```json
{"role": "analyst", "user": "ayse"}
```

### `POST /auth/keys` — kullanıcı anahtarı üretir (admin)

Gövde: `username` (zorunlu, 1–200), `role` (`viewer` varsayılan), `name` (≤ 200, ör. cihaz adı).

```json
{"id": "6f1c…", "api_key": "osiris_Xb3…", "role": "analyst"}
```

- Ham anahtar (`osiris_` + 43 karakter) **yalnızca bu yanıtta** döner; veritabanında SHA-256 özeti
  ve ilk 8 karakteri (`key_prefix`) saklanır.
- Kullanıcı yoksa oluşturulur. **Varsa rolü verilen rolle güncellenir**; bu, kullanıcının diğer
  anahtarlarını da etkiler.

### `GET /auth/keys` — anahtar listesi (admin)

En yeni 200 anahtar; ham değer ya da özet asla dönmez.

```json
[{"id": "6f1c…", "username": "ayse", "role": "analyst", "key_prefix": "osiris_X", "name": "dizüstü",
  "revoked": false, "created_at": "2026-10-18T09:12:00Z", "last_used_at": "2026-10-18T11:40:31.5Z"}]
```

### `DELETE /auth/keys/{id}` — anahtarı iptal eder (admin)

`{"status": "revoked"}`. Satır silinmez (`revoked: true`); bilinmeyen kimlik `404`.

## 5. Plugin'ler ve toplama

### `GET /plugins` — plugin envanteri (viewer)

Kimliğe göre sıralı; `config_schema` alanları manifest sırasındadır (arayüz formları bu sırayla
çizilir). Tüm plugin'lerin alanları: [PLUGINS.tr.md](PLUGINS.tr.md).

```json
[{"id": "rss", "name": "RSS/Atom Feed", "network_type": "rss", "description": "RSS/Atom feed okuyucu",
  "config_schema": {"feed_url": {"type": "string", "required": true},
                    "max_items": {"type": "integer", "default": 50}},
  "schedule_default": "*/15 * * * *"}]
```

### `POST /collect` — tek toplama (analyst)

Gövde: `plugin_id` (zorunlu, ≤ 64, küçük harf/rakam/`-`/`_`), `config` (nesne), `include_items`
(varsayılan `false`).

```bash
curl -X POST https://<sunucu>:8443/api/collect -H "X-API-Key: <a>" -H 'Content-Type: application/json' \
  -d '{"plugin_id":"rss","config":{"feed_url":"https://example.com/feed.xml"},"include_items":true}'
```
```json
{"items": 12, "metadata": {…},
 "results": [{"url": "https://example.com/haber/1", "title": "…", "snippet": "…",
              "published_at": "2026-10-18T07:30:00+00:00", "metadata": {…}, "tags": []}]}
```

- Öğeler kuyruğa yazılır; işçi onları birkaç saniye içinde veritabanına işler.
- `include_items: true` → `results` önizlemesi: en çok 1 000 öğe, `snippet` ham içeriğin ilk 500
  karakteri. Önizleme yalnızca yanıttadır; öğeler yine kuyruğa yazılır.
- Hatalar: `404 Plugin bulunamadı`, `413 config çok büyük`, `502 <plugin iletisi>` (plugin çalıştı
  ama başarısız oldu; ör. `Zorunlu alanlar eksik: feed_url`, `İç ağ/özel host engellendi`).

### `POST /collect/batch` — toplu toplama (analyst)

Gövde: `jobs` (1–10 iş, her biri `/collect` gövdesi gibi). İşler sırayla çalışır.

```json
{"jobs": [{"plugin_id": "rss", "ok": true, "items": 12, "error": null},
          {"plugin_id": "dns-whois", "ok": false, "items": 0, "error": "Domain bulunamadı"},
          {"plugin_id": "yok", "ok": false, "error": "Plugin bulunamadı"}],
 "ok": 1, "total": 3}
```

## 6. Arama

Üç arama ucunun ortak alanları: `limit` (1–100, varsayılan 20). Sonuçlarda `cleaned_content`
ilk 2 000 karakterdir.

### `POST /search` — tam metin (viewer)

Gövde: `query` (zorunlu, 1–500), `limit`. PostgreSQL `simple` sözlüğüyle `plainto_tsquery`;
`ts_rank`'e göre sıralı. Kök bulma yoktur: sorgudaki kelimeler metinde aynen geçmelidir
(büyük/küçük harf duyarsız).

```json
[{"id": "…", "title": "…", "url": "https://example.com/haber/1",
  "cleaned_content": "…", "collected_at": "2026-10-18T08:00:00Z"}]
```

### `POST /search/semantic` — anlamsal (viewer)

Gövde: `query` (zorunlu, 1–500), `limit`. Sorgu, işçinin kullandığı modelle 384 boyutlu vektöre
çevrilir ve `embedding_v2` ile kosinüs benzerliğine göre sıralanır. Sonuçlar `/search` ile aynı
alanlar + `similarity` (0–1, yüksek = daha yakın).

```json
[{"id": "…", "title": "…", "url": "…", "cleaned_content": "…", "collected_at": "…", "similarity": 0.83}]
```

- Model API sürecinde ilk aramada yüklenir; ilk istek birkaç saniye sürebilir.
- `OSIRIS_EMBEDDING_MODEL` boşsa ya da model dosyaları yoksa `503` döner; API'nin geri kalanı
  etkilenmez. Modelleri indirmek: `docker compose --profile models run --rm osiris-models`.
- Yalnızca vektörü olan öğeler aranır.

### `POST /search/entity` — varlık (viewer)

Gövde: `entity_type` (zorunlu), `value` (1–500; alt dize, büyük/küçük harf duyarsız), `limit`.

```bash
-d '{"entity_type":"email","value":"@example.com","limit":20}'
```

Sonuç alanları: `id`, `title`, `url`, `cleaned_content`. Geçersiz tip `400`.

Varlık türleri: `person`, `org`, `location`, `ip`, `domain`, `email`, `phone`,
`crypto_address`, `hash`, `username`, `cve`, `custom`. (İşleme hattı bugün `person`, `org`,
`location`, `ip`, `domain`, `email`, `phone`, `crypto_address`, `cve` üretir.)

## 7. Öğeler ve varlıklar

### `GET /items/recent` — son öğeler (viewer)

`?limit=` (1–50, varsayılan 20), `?offset=`. `collected_at`'e göre yeniden eskiye.

```json
[{"id": "…", "title": "…", "url": "…", "language": "tr", "snippet": "ilk 500 karakter…",
  "collected_at": "…", "tags": ["threat"]}]
```

### `GET /items/{id}` — öğe detayı (viewer)

```json
{"id": "…", "source_id": "…", "title": "…", "url": "…", "language": "en",
 "cleaned_content": "…", "collected_at": "…", "published_at": null,
 "content_hash": "9f86d0…", "metadata": {…}, "tags": ["breach"],
 "entities": [{"type": "email", "value": "info@example.com", "mention_count": 1}]}
```

`entities` anma sayısına göre en çok 200 varlık içerir. Bulunamazsa `404`.

### `GET /entities/top` — en çok anılan varlıklar (viewer)

`?entity_type=` (isteğe bağlı), `?limit=` (1–100, varsayılan 20).

```json
[{"type": "domain", "value": "example.com", "mentions": 12, "last_seen": "…"}]
```

## 8. Kaynaklar

### Kaynak nesnesi

| Alan | Tür | Kural |
|------|-----|-------|
| `name` | dize | zorunlu, 1–500 |
| `plugin_id` | dize | zorunlu, kayıtlı bir plugin (değilse `404`) |
| `url` | dize | ≤ 2048; boş → `null`. Çalıştırırken `config`'teki `url`/`feed_url`/`endpoint` boşsa onlara kopyalanır |
| `network_type` | dize | `www` (varsayılan), `tor`, `i2p`, `p2p`, `freenet`, `zeronet`, `irc`, `matrix`, `rss`, `api`, `blockchain`, `sdr`, `custom` (değilse `400`) |
| `schedule` | dize | boş ya da 5 alanlı cron (UTC); geçersizse `422` |
| `priority` | tamsayı | 1–10, varsayılan 5 |
| `enabled` | mantıksal | varsayılan `true` |
| `tags` | dize dizisi | ≤ 20 etiket, her biri ≤ 100 karakter |
| `config` | nesne | plugin ayarları; ≤ 50 anahtar, ≤ 20 KB |
| `retention_days` | tamsayı \| null | `null` → `OSIRIS_RETENTION_DAYS`; `0` → süreyle silinmez; 1–36 500 gün |

Okuma yanıtları ayrıca sağlık alanlarını taşır: `last_crawled_at`, `last_success_at`,
`failure_count` (ardışık başarısızlık), `avg_response_ms` (son çalışmanın süresi).

### `GET /sources` — liste (viewer)

Süzgeçler: `?enabled=true|false`, `?plugin_id=rss`, `?q=` (ad ya da URL alt dizesi; `%` ve `_`
düz karakter sayılır). Sayfalama: `limit` (1–200, varsayılan 100), `offset`. Ada göre sıralı.
Listede `config` yoktur.

### `POST /sources` — oluştur (analyst)

```bash
curl -X POST https://<sunucu>:8443/api/sources -H "X-API-Key: <a>" -H 'Content-Type: application/json' -d '{
  "name": "Örnek API", "plugin_id": "rest-api", "network_type": "api",
  "schedule": "0 * * * *", "retention_days": 30,
  "config": {"endpoint": "https://api.example.com/v1/olaylar", "json_path": "data.items",
             "headers": {"Authorization": "Bearer gizli-deger"}}}'
```
```json
{"id": "2b0e…"}
```

### `GET /sources/{id}` — detay (viewer)

Liste alanları + `created_at`, `updated_at` ve `config`. **Sırlar maskelenir:** anahtar adı
`api_key`, `access_token`, `token`, `password`, `passwd`, `secret`, `authorization`, `cookie`,
`credential` ya da `private_key` içeren (büyük/küçük harf duyarsız, `-`/`_` serbest) ve değeri
boş olmayan her alan, iç içe nesne ve dizilerde de (8 düzeye kadar), `"********"` olarak döner.

```json
{"id": "2b0e…", "name": "Örnek API", "plugin_id": "rest-api", "network_type": "api", "url": null,
 "schedule": "0 * * * *", "priority": 5, "enabled": true, "tags": [], "retention_days": 30,
 "last_crawled_at": null, "last_success_at": null, "failure_count": 0, "avg_response_ms": null,
 "created_at": "…", "updated_at": "…",
 "config": {"endpoint": "https://api.example.com/v1/olaylar", "json_path": "data.items",
            "headers": {"Authorization": "********"}}}
```

### `PATCH /sources/{id}` — kısmi güncelleme (analyst)

Yalnızca gövdede bulunan alanlar değişir: `name`, `url`, `schedule`, `priority`, `enabled`,
`tags`, `config`, `retention_days`.

- `url: null` / `schedule: null` → alan temizlenir; `retention_days: null` → sunucu varsayılanına döner.
- `name`, `priority`, `enabled` `null` olamaz (`422`).
- `config` **sığ birleşir**: gönderilen anahtarlar yazılır, değeri `null` olan anahtar silinir,
  gönderilmeyen anahtarlar korunur. Değeri `"********"` olan yerlerde (iç içe de) saklı eski değer
  korunur; eskisi yoksa o anahtar yazılmaz. Böylece `GET`'ten dönen maskeli `config`'i düzenleyip
  geri göndermek sırları bozmaz.

```bash
curl -X PATCH https://<sunucu>:8443/api/sources/<id> -H "X-API-Key: <a>" -H 'Content-Type: application/json' \
  -d '{"enabled": false, "schedule": "0 */6 * * *", "config": {"json_path": null}}'
```
```json
{"status": "updated"}
```

Hatalar: boş gövde `422 Güncellenecek alan yok`, bilinmeyen kaynak `404`, büyük `config` `413`.

### `DELETE /sources/{id}` — sil (analyst)

`{"status": "deleted"}`. Kaynağın öğeleri de silinir (veritabanında kaskad).

### `POST /sources/{id}/collect` — şimdi çalıştır (analyst)

Kaynağın `config`'i, `source_id` ve (gerekirse) `url` eklenerek çalıştırılır; sağlık alanları ve
`source_metrics` güncellenir. `?include_items=true` → `/collect`'teki gibi `results` önizlemesi.

Yanıt `/collect` ile aynıdır. Hatalar: `404` (kaynak ya da plugin yok), `409 Kaynak devre dışı`,
`502` (plugin başarısız).

## 9. Graf

Graf yönsüzdür; bir düğüm çifti arasında tek kenar olur (aynı çifte yeni `relation_type`
eklemek öncekinin yerine geçer); öz döngü yok sayılır. Düğüm kimlikleri varlık adlarıdır
(kırpılmış dize).

### `GET /graph` — düğümler ve kenarlar (viewer)

En çok 5 000 düğüm ve 20 000 kenar.

```json
{"nodes": [{"id": "example.com"}, {"id": "203.0.113.7"}],
 "edges": [{"source": "example.com", "target": "203.0.113.7", "relation_type": "resolves", "weight": 1.0}]}
```

### `POST /graph/relation` — ilişki ekle (analyst)

Gövde: `source`, `target` (zorunlu, 1–500), `relation_type` (≤ 100, varsayılan `related`).
Kenar bellekteki grafa ve `graph_edges` tablosuna yazılır (yeniden başlatmada korunur).

```json
{"status": "ok"}
```

Kırpınca boş kalan kimlik `400`.

### `GET /graph/analysis` — analiz (viewer)

`?top=` (1–200, varsayılan 25), `?node=` (isteğe bağlı).

```json
{"nodes": 42, "edges": 57,
 "centrality": [{"id": "example.com", "score": 0.121951, "degree": 5}],
 "communities": [{"size": 9, "members": ["203.0.113.7", "example.com", …]}],
 "neighbors": ["203.0.113.7"]}
```

- `centrality`: derece merkeziliği `derece / (n − 1)`, büyükten küçüğe ilk `top` düğüm (6 ondalık).
- `communities`: Clauset–Newman–Moore modülerlik toplulukları, en büyük 50, her biri en çok 200
  üye. Graf 5 000 düğümden büyükse `null`.
- `neighbors`: `node` verildiyse komşuları (en çok 500), verilmediyse `null`.

### `GET /graph/export` — dışa aktarım (viewer)

`?format=graphml` (varsayılan) ya da `gexf`. Yanıt `application/xml`, ayrıca
`Content-Disposition: attachment; filename="osiris-graf.<uzantı>"`. Başka biçim `400`.

```bash
curl -H "X-API-Key: <a>" "https://<sunucu>:8443/api/graph/export?format=gexf" -o osiris.gexf
```

## 10. Kayıtlı sorgular ve uyarılar

### `GET /saved-queries` — liste (viewer)

`?alert_enabled=true|false`, `limit` (1–200), `offset`. Ada göre sıralı.

```json
[{"id": "…", "name": "Kritik", "query_text": "fidye", "query_type": "fts",
  "alert_enabled": true, "last_triggered_at": null}]
```

### `POST /saved-queries` — oluştur (analyst)

Gövde: `name` (1–200), `query_text` (1–500), `query_type` (`fts` varsayılan; `fts`, `semantic`,
`regex`, `entity`, `graph` — başkası `400`), `alert_enabled` (varsayılan `false`). Yanıt `{"id": "…"}`.

### `DELETE /saved-queries/{id}` — sil (analyst)

`{"status": "deleted"}`; bilinmeyen kimlik `404`.

### `POST /alerts/test` — kuralları dene (analyst)

Gövde: `text` (zorunlu, 1–50 000), `title` (≤ 2000). Metin, `alert_enabled` olan en çok 100
kurala karşı eşleştirilir: kural metni (kırpılıp küçük harfe çevrilmiş) başlıkta ya da içerikte
alt dize olarak geçerse kural eşleşir. `query_type` eşleşmeyi değiştirmez. Kayıt oluşturulmaz,
bildirim gönderilmez; eşleşen kuralların `last_triggered_at` alanı güncellenir.

```json
[{"kind": "match", "query_id": "…", "query_name": "Kritik", "item_id": null, "matched": "fidye"}]
```

## 11. Raporlar

Raporlar yanıt gövdesinde döner; sunucuda dosya oluşturulmaz.

### `POST /reports/generate` — çok biçimli rapor (analyst)

| Alan | Kural |
|------|-------|
| `title` | zorunlu, 1–500 |
| `scope` | ≤ 1000 |
| `summary` | ≤ 20 000 |
| `findings` | ≤ 500 öğe: `{"title", "description"}` (başlık ≤ 500, açıklama ≤ 5000) |
| `sources` | ≤ 500 dize (her biri ≤ 1000) |
| `format` | `markdown` (varsayılan), `html`, `json`, `stix` |
| `entities` | ≤ 500 öğe: `{"type", "value"}` — JSON ve STIX göstergeleri |
| `identity_name` | STIX `identity` nesnesinin adı (varsayılan `OSIRIS`) |

```bash
curl -X POST https://<sunucu>:8443/api/reports/generate -H "X-API-Key: <a>" -H 'Content-Type: application/json' \
  -d '{"title":"Vaka 12","format":"stix","entities":[{"type":"ip","value":"203.0.113.7"},{"type":"cve","value":"CVE-2024-3094"}]}'
```
```json
{"format": "stix", "media_type": "application/stix+json", "filename": "Vaka-12.stix.json",
 "content": "{\n  \"type\": \"bundle\", …}"}
```

| `format` | `media_type` | Uzantı | İçerik |
|----------|-------------|--------|--------|
| `markdown` | `text/markdown` | `.md` | Başlık, oluşturulma zamanı, kapsam, özet, bulgular, kaynaklar; HTML karakterleri kaçışlı |
| `html` | `text/html` | `.html` | Aynı içerik, tek dosya, satır içi stil, kaçışlı |
| `json` | `application/json` | `.json` | `{title, generated_at, scope, summary, findings, sources, entities}` |
| `stix` | `application/stix+json` | `.stix.json` | STIX 2.1 `bundle`: bir `identity`; `cve` → `vulnerability`, `hash` → `file`, `ip`/`domain`/`email`/`crypto_address` → `indicator`; diğer türler atlanır |

`filename` başlıktan türetilir: Türkçe harfler ASCII'ye sadeleşir (`ı→i`, `ş→s`, …), diğer
karakterler `-` olur, en çok 60 karakter; boş kalırsa `osiris-rapor`.

### `POST /reports/markdown` — yalnızca Markdown (analyst)

Eski uç; `title`, `scope`, `summary`, `findings`, `sources` alır ve `{"markdown": "…"}` döner.

## 12. Gizlilik ve saklama (KVKK)

### `DELETE /items/{id}` — öğeyi sil (admin)

Öğeyi ve artık hiçbir öğeye bağlı olmayan varlıkları siler.

```json
{"status": "deleted", "items_deleted": 1, "entities_deleted": 2}
```

### `POST /entities/erase` — varlığı ve onu ananları sil (admin)

Gövde: `entity_type` (zorunlu), `value` (zorunlu, 1–500), `delete_items` (varsayılan `true`).

```bash
curl -X POST https://<sunucu>:8443/api/entities/erase -H "X-API-Key: <admin>" -H 'Content-Type: application/json' \
  -d '{"entity_type":"email","value":"ali@example.com","delete_items":true}'
```
```json
{"items_deleted": 4, "entities_deleted": 3, "edges_deleted": 1}
```

- Eşleşme tip + kırpılmış, büyük/küçük harf duyarsız değerdir.
- `delete_items: true` → varlığı anan tüm öğeler silinir; ardından yetim kalan varlıklar da silinir.
- Kaynak ya da hedef adı aynı değer olan graf kenarları silinir ve bellekteki graf yeniden yüklenir.
- Denetim kaydına değer değil `value_sha256` (küçük harfli değerin SHA-256'sı) yazılır.
- Geçersiz tip `400`, kırpınca boş kalan değer `422`.

### `POST /admin/retention/run` — saklama temizliğini şimdi çalıştır (admin)

```json
{"items_deleted": 120, "entities_deleted": 37, "default_days": 180}
```

Kurallar: kaynağın `retention_days`'i (boşsa `OSIRIS_RETENTION_DAYS`; `0` → silinmez); kaynaksız
öğelere varsayılan uygulanır. Normalde işçi bunu `OSIRIS_RETENTION_INTERVAL` saniyede bir yapar.

## 13. İstatistik

### `GET /stats` (viewer)

```json
{"sources": 4, "sources_enabled": 3, "items": 1280, "entities": 342, "edges": 57,
 "saved_queries": 3, "alerts": 2, "latest_item_at": "2026-10-18T11:59:02Z"}
```

`alerts`, `alert_enabled` olan kayıtlı sorgu sayısıdır.

## 14. Hatalar

Hata gövdesi her zaman `{"detail": …}` biçimindedir. İç ayrıntılar (SQL, sürücü iletisi, yığın
izi) gövdeye yazılmaz, loga yazılır.

| Kod | Anlam | Örnek `detail` |
|-----|-------|----------------|
| 400 | Geçersiz değer | `Geçersiz rol`, `Geçersiz network_type`, `Geçersiz entity_type`, `format graphml veya gexf olmalı` |
| 401 | Kimlik yok ya da geçersiz | `Geçersiz kimlik bilgisi` |
| 403 | Rol yetersiz | `Yetki yetersiz` |
| 404 | Kayıt yok, bozuk UUID ya da bilinmeyen yol | `Kaynak bulunamadı`, `Plugin bulunamadı`, `Not Found` |
| 405 | Yöntem desteklenmiyor | `Method Not Allowed` |
| 409 | Durum çatışması | `Kaynak devre dışı` |
| 413 | Çok büyük | `config çok büyük` ya da gövde > 5 MiB |
| 422 | Şema/alan ihlali | dizi: `[{"type": "…", "loc": ["body", "limit"], "msg": "…"}]` ya da tek ileti |
| 429 | Kota aşıldı | `İstek kotası aşıldı` |
| 500 | Sunucu/veritabanı hatası | `Kaynaklar okunamadı` (genel ileti) |
| 502 | Plugin çalıştı ama başarısız | Plugin'in hata iletisi |
| 503 | Özellik yapılandırılmamış | `Jeton üretimi kapalı (OSIRIS_JWT_SECRET yok)`, `Semantik arama yapılandırılmamış: …` |

## 15. Hız sınırları ve ölçümler

İki katman vardır:

| Katman | Anahtar | Sınır | Aşılınca |
|--------|---------|-------|----------|
| Nginx | İstemci IP'si | `/api/*` için 10 istek/sn, 20 taşma | `503` (Nginx) |
| API | Kimlik (`rol:kullanıcı`) | `OSIRIS_API_RATE_PER_MIN` istek/dk (varsayılan 600; `0` = kapalı) | `429 İstek kotası aşıldı` |

Tüm JWT'ler aynı kullanıcı adını (`api-client`) taşıdığından aynı roldeki JWT'ler tek bir kotayı
paylaşır. Büyük işlerde `/collect/batch` (≤ 10) kullanın ve istemcide tempo koyun.

**Ölçümler** ayrı dinleyicidedir (`OSIRIS_METRICS_ADDR`, Compose'da `osiris-api:9464/metrics`,
yalnızca iç ağ; `/api/` altında sunulmaz): `osiris_http_requests_total`,
`osiris_http_request_duration_seconds`, `osiris_api_rate_limited_total`,
`osiris_audit_failures_total`, `osiris_build_info`. Etiketler rota şablonudur; yol değerleri ve
sorgu dizgeleri yazılmaz.

## 16. Değişiklik geçmişi

- **Web arayüzü sürümü** — Yeni: `GET /auth/me`; `POST /search/semantic`; `GET` ve
  `PATCH /sources/{id}` (sırlar her derinlikte maskeli, maske güncellemede eski değeri korur,
  `config` birleştirme); kaynak `schedule` cron doğrulaması; `GET /graph/analysis`;
  `GET /graph/export`; `POST /reports/generate` (Markdown/HTML/JSON/STIX); `/collect` ve
  `/sources/{id}/collect` için `include_items` önizlemesi; CORS'ta `PATCH`.
- **1.0.0-rc (Rust R7)** — Python uygulaması kaldırıldı, sözleşme aynı. `osiris-server migrate`;
  migration 006 eski `items.embedding` sütununu siler; denetim zinciri yalnızca v2.
- **Rust R6** — API Rust'ta (axum) yeniden yazıldı, sözleşme değişmedi. Kimlik başına kota
  (`429`), `/metrics` (ayrı dinleyici), JSON log, `/openapi.json`. `/graph/relation`'da kırpınca
  boş kalan kimlik artık `500` yerine `400`.
- **Rust R2** — KVKK uçları (`DELETE /items/{id}`, `POST /entities/erase`,
  `POST /admin/retention/run`), kaynaklarda `retention_days`; `OSIRIS_API_KEY` olmadan başlamama;
  JWT sırrının API anahtarına düşmemesi; denetim zinciri v2 (RFC 8785).
- **0.2.0** — Listelerde sayfalama ve süzgeçler; `GET /items/{id}`, `GET /entities/top`,
  `POST /collect/batch`; UUID güvenli yollar; `q` süzgecinde LIKE joker kaçışı.
- **0.1.x** — Kaynak CRUD ve çalıştırma, kayıtlı sorgular, `/alerts/test`, `/reports/markdown`,
  `/stats`, `/items/recent`, JWT + RBAC, `/auth/keys`.
