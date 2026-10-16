# OSIRIS — Rust Geçişi R0 Envanteri

> Faz: **R0** (yalnızca okuma, uygulama kodu değişmedi) · Tarih: 2026-10-16
> Taban commit: `6cb08c8` (`master`) · Plan: [RUST_GECIS_PLANI.md](RUST_GECIS_PLANI.md)
> Her iddianın yanında kanıt olarak `dosya:satır` verilmiştir.

---

## 1. Doğrulama soruları — cevaplar

### 1.1 `osiris:raw_items` LIST mi, Stream mi?

**Redis LIST.** Üretici `RPUSH` (`osiris-collector/osiris_collector/manager.py:157`), tüketici `LPOP` (`osiris-pipeline/osiris_pipeline/pipeline.py:247`). FIFO, consumer group ya da ACK yok.

- **Teslim garantisi: en fazla bir kez.** `LPOP` öğeyi hemen siler. `store()` hata verirse kayıt loglanır ve kaybolur (`pipeline.py:258-262`). Bozuk JSON da sessizce atlanır.
- **Mesaj biçimi** (`manager.py:142-150`):
  ```json
  {"plugin_id": "rss", "item": <CollectedItem.model_dump()>, "collected_at": 1760000000.123}
  ```
  `item` = `{url, title, raw_content, published_at (str|null), metadata{}, tags[]}`. Mesaj başına üst sınır 512 000 bayt (UTF-8).
- **⚠ `source_id` mesajda yok.** `collect_source` kimliği plugin config'ine koyar (`osiris-api/osiris_api/main.py:778`), ama `CollectedItem` bu alanı taşımaz. Pipeline `item.get("source_id")` okur ve sonuç `NULL` olur (`pipeline.py:346`). **Yani `items.source_id` pratikte hep NULL.** Bu durum K7'deki "kaynak başına saklama süresi" fikrini doğrudan etkiler (bkz. §2 E4).

### 1.2 `content_hash` nasıl hesaplanıyor?

Yalnızca **temizlenmiş içerikten** hesaplanır. URL, başlık ve kaynak hash'e girmez (`pipeline.py:100-103`, `119-129`):

1. `raw_content` → `str()`, ilk **200 000 karakter**
2. `BeautifulSoup(text, "html.parser").get_text(" ", strip=True)`: HTML soyulur, metin düğümleri tek boşlukla birleştirilir, her düğüm trim edilir
3. `re.sub(r"\s+", " ", …)`: Python `\s` (Unicode boşlukları dahil) tek boşluğa iner
4. `.strip()`, sonra yine ilk 200 000 karakter
5. `sha256(cleaned.encode("utf-8")).hexdigest()`: küçük harfli 64 karakter hex

- **Unicode NFC yok** ve **küçük harfe çevirme yok**. Ampirik doğrulama: `"Café"` (U+00E9) ile `"Café"` aynı görünür ama farklı hash üretir.
- Uzunluk sınırı Python **kod noktası** sayısıyla uygulanır. Rust'ta `char` sayısıyla birebir eşleşmeli, bayt sayısıyla değil.
- BeautifulSoup `html.parser` davranışını (`<br>` → boşluk, entity çözümü, bozuk HTML toleransı) `scraper`/html5ever ile birebir taklit etmek **riskli**. R2'deki fixture'lar bu kenar durumlarını mutlaka içermeli.
- **Tekilleştirme global:** `items.content_hash UNIQUE`, `ON CONFLICT DO NOTHING` (`db/migrations/001_init.sql:71`, `pipeline.py:369,393`). İki kaynaktaki aynı metin tek öğeye iner.

### 1.3 Audit zinciri serileştirmesi

Kaynak: `osiris-sdk/osiris/audit.py:29-42` ve `:79-80`.

- **Kayıt:** `{"user_id", "action", "resource", "detail"}`. `id`, `ts` ve `prev_hash` **kayda girmez**.
- **Normalizasyon:** `user_id` ve `resource` için `None` → `""`, kırpma 200 ve 500 karakter. `detail` önce JSON'a gidip geri döner; dict değilse `{"value": …}` olur (`:45-53`).
- **Serileştirme:** `json.dumps(..., ensure_ascii=False, sort_keys=True, separators=(",", ":"), default=str)`
- **Hash:** `sha256(f"{prev}|{payload}".encode())`. Ayraç `|`, ilk kayıtta `prev = "GENESIS"`. DB'de ilk kaydın `prev_hash` değeri `NULL`.
- **Eşzamanlılık:** `pg_advisory_xact_lock(hashtext('osiris_audit_chain'))`.

**JCS (RFC 8785) ile farklar** (`chain_version=2` gerekçesi):

| Konu | Mevcut (Python) | JCS |
|---|---|---|
| Float | `1.0` → `1.0`, `1e16` → `1e+16` | `1`, `10000000000000000` (ECMAScript sayı biçimi) |
| Anahtar sırası | Unicode kod noktası | UTF-16 kod birimi (yalnızca BMP dışı karakterlerde fark eder) |
| Önek | `prev + "|" + payload` | plan: `prev_hash ‖ jcs(kayıt)` (ayraç ve GENESIS de sözleşmede sabitlenmeli) |

Ek risk: `detail` PostgreSQL `jsonb` üzerinden döndüğü için sayılar normalize edilir. Örneğin Python'un `1e+16` çıktısı jsonb'den `10000000000000000` olarak geri gelir ve **doğrulama kırılır**. Bugünkü audit `detail` değerleri yalnızca tamsayı ve dize içerdiği için sorun pratikte henüz görülmüyor.

### 1.4 STIX UUID'leri

Kaynak: `osiris-report/osiris_report/generator.py:134-209`.

| Nesne | Üretim | Deterministik mi? |
|---|---|---|
| `bundle` | `uuid4()` | **Hayır** (rastgele) |
| `identity` | `uuid5(NAMESPACE_DNS, "osiris.local")` | Evet |
| `vulnerability` / `file` | `uuid5(NAMESPACE_URL, f"{etype}:{value.lower()}")` | Evet |
| `indicator` | `uuid5(NAMESPACE_URL, pattern)` | Evet |
| `created` / `modified` / `valid_from` | `datetime.now(UTC)` | **Hayır** |

- Plandaki "byte byte STIX fixture'ı" bugün **mümkün değil**. Bundle kimliği ve zaman damgaları enjekte edilebilir hale gelmeli (R2).
- `NAMESPACE_URL` kullanılıyor. STIX 2.1'in SCO kimlikleri için önerdiği namespace (`00abedb4-aa42-466c-9c01-fed23315a9b7`) kullanılmıyor.
- `cryptocurrency-wallet` STIX 2.1 çekirdeğinde bir SCO tipi değil, bir uzantı tipi.
- `generate_stix` herhangi bir API ucundan çağrılmıyor. Yalnızca testlerde kullanılıyor.

### 1.5 Topluluk tespiti

- `nx.community.greedy_modularity_communities(G)`: Clauset–Newman–Moore açgözlü modülerlik (`osiris-graph/osiris_graph/engine.py:76-83`). **Louvain değil.** Seed parametresi yok; algoritma deterministik, ama eşitlik durumunda düğüm sırası sonucu etkiler.
- Merkezilik: `nx.degree_centrality` (`:70-74`).
- Graf `nx.Graph`, yani **yönsüz ve tek kenarlı**. Aynı düğüm çifti için farklı `relation_type` eklemek önceki kenarı ezer. DB'de ise PK `(source, target, relation_type)` olduğu için birden fazla kenar tutulabilir. Bellek ve DB modelleri tutarsız.
- `centrality()` ve `communities()` hiçbir API ucundan çağrılmıyor (yalnızca testlerde). `/graph` yalnızca düğüm ve kenar listesi döner.
- `rustworkx-core` greedy modularity (CNM) sunmuyor. Birebir sonuç gerekiyorsa algoritmayı elle taşımak gerekir (R6).

### 1.6 MinIO / FreshRSS / C++ core kullanımı

| Bileşen | Koddan kullanım | Nerede geçiyor |
|---|---|---|
| **MinIO** | **Yok.** Python/TS içinde `minio`, `boto` veya `s3` importu ya da çağrısı yok | `docker-compose.yml` servisi, `.env.example`, README, TEKNIK_DOKUMAN.tr.md:163, 379, 524 |
| **FreshRSS** | **Yok.** API çağrısı yok, pipeline'a bağlı değil | compose servisi, `deploy/nginx/nginx.conf` `/rss/` proxy'si, dokümanda "bağımsız okuyucu, pipeline entegrasyonu planlı" (TEKNIK_DOKUMAN.tr.md:481-483) |
| **C++ `osiris-core`** | **Yok.** Hiçbir Python, TS veya Docker dosyası çağırmıyor | Yalnızca CI `cpp-build` adımı (`.github/workflows/ci.yml:84-93`). `start_api_server` boş bir iskelet (`osiris-core/src/core.cpp:107-110`) |

**Sonuç:** K1, K2 ve K3 için önerilen "kaldır" seçenekleri hiçbir kod yolunu bozmaz. Etkilenen yalnızca compose, nginx, CI ve dokümanlar olur.

### 1.7 Embedding modeli ve boyut

- Model **varsayılan olarak yok.** `OSIRIS_EMBEDDING_MODEL` ortam değişkeni compose'da ve `.env.example` içinde tanımlı değil. `sentence-transformers` isteğe bağlı bir ekstra (`osiris-pipeline/pyproject.toml`).
- Sütun `VECTOR(1536)` + HNSW cosine (`001_init.sql:72,78`). Yaygın sentence-transformers modelleri 384 veya 768 boyutlu. Boyut uyuşmazsa INSERT SAVEPOINT ile geri alınır ve öğe **vektörsüz** yazılır (`pipeline.py:357-386`).
- **Pratikte üretimde hiç embedding yazılmıyor** (model tanımsız). `QueryEngine.semantic_search` var, ama onu çağıran **API ucu yok**.
- Sonuç: K5 migration'ı (`embedding_v2`) için yeniden embed edilecek eski veri büyük olasılıkla **yok**, geçiş ucuz olur.

---

## 2. Ek bulgular (planı etkileyenler)

| # | Bulgu | Kanıt | Etkilenen faz |
|---|---|---|---|
| E1 | **Compose'da pipeline işçisi ve collector zamanlayıcısı servisi yok.** API, collector'ı süreç içinde çalıştırıp kuyruğa yazıyor. `osiris-pipeline` elle çalıştırılmazsa kuyruk boşalmıyor | `docker-compose.yml`, `deploy/docker/Dockerfile.api` (pipeline kurulmuyor) | R3/R5. Canlı sistemde kuyruğun nasıl boşaltıldığı kullanıcıya sorulmalı |
| E2 | **Cron zamanlaması bağlı değil.** `CollectorManager.schedule()`'ı çağıran kod yok. `sources.schedule` sütunu hiç okunmuyor. `osiris-collector` boş bir zamanlayıcı başlatıyor | `grep '\.schedule('`: yalnızca tanım | R5 (Rust collector'da zamanlama ilk kez gerçekten çalışacak) |
| E3 | **spaCy NER pratikte kapalı.** `xx_ent_wiki_sm` kurulu değil, `spacy.blank("xx")` hiç varlık üretmiyor. Model kurulsa bile etiketler `PER/LOC/ORG/MISC` → küçük harf → yalnızca `org` geçerli tipe eşleşiyor; `per` ve `loc` `custom`'a düşüyor | `pipeline.py:71-84, 183-192` | K4 (ONNX'e geçiş bugünkü davranışı bozmaz, aksine ilk kez çalışır hale getirir) |
| E4 | `items.source_id` hep NULL (bkz. §1.1) | `manager.py:142-150`, `pipeline.py:346` | K7 (kaynak başına saklama için önce bunun düzeltilmesi gerekir) |
| E5 | IRC plugin'i kullanıcının verdiği `server`'a ham soket açıyor ve **SSRF denetiminden geçmiyor**. `sanitize_hostname` yalnızca karakter denetliyor, iç IP'ye bağlanmak mümkün. WHOIS sabit host'a (`whois.iana.org:43`) bağlanıyor, risk taşımıyor | `plugins/irc/collector.py:38,95` | R1'e eklenebilir; R5'te "plugin ağa doğrudan erişmez" kuralı bunu çözer |
| E6 | Tor ve I2P proxy host'u yalnızca `127.0.0.1`/`::1` kabul ediyor. Compose'da tor/i2p servisi yok. Konteyner içinde localhost'ta proxy olmadığı için bu plugin'ler Docker'da çalışamaz | `plugins/tor/collector.py:27`, `plugins/i2p/collector.py` | R3 maddesi doğrulandı |
| E7 | `/auth/token`, anahtar yoksa **404** dönüyor (plan 503 diyor). `JWT_SECRET` boşsa `API_KEY`'e düşüyor. Anahtar yoksa tüm uçlar `anonymous/admin` | `main.py:47, 166, 266` | R1/K9 |
| E8 | username-search'te tempo sınırı yok. Yalnızca `workers` (1-20 thread) ve `timeout` ayarlanabiliyor | `plugins/username-search/collector.py:167-177` | R2 |
| E9 | Qt ve Tauri istemcileri repoda **yok** (yalnızca dokümanda "planlı") | README, ARCHITECTURE | K8'in pratik etkisi yok |
| E10 | R1 doküman maddeleri doğrulandı: `aşJRımlı` (TEKNIK_DOKUMAN.tr.md:85), `OSIRIS_Technical_Document.md` referansı (:9), `[P2P]` (:117), "zamanlama C++ core'da" (:133), API taban URL'si `http://<sunucu>/api` (API.tr.md:6) | — | R1 |
| E11 | Bazı fonksiyonlar API'ye açık değil: `semantic_search`, `centrality`, `communities`, `generate_stix/html/csv/json`, `AlertManager.check_anomaly`, `export_graphml/gexf`. R6'da "tüm uçlar" taşınırken bunlar uç olarak eklenecek mi, yoksa yalnızca kütüphane olarak mı kalacak, karar verilmeli | §3.2 | R6 |
| E12 | `pandas` pipeline bağımlılığı ama hiçbir yerde import edilmiyor | `osiris-pipeline/pyproject.toml` | R3 temizliği |

---

## 3. Modül envanteri

### 3.1 Python paketleri — public yüzey

| Paket | Modül | Public fonksiyon/sınıf | Rust hedefi |
|---|---|---|---|
| `osiris-sdk` | `osiris/models.py` | `Source`, `Entity`, `Item` (pydantic) | `osiris-types` |
| | `osiris/plugin.py` | `CollectedItem`, `CollectionResult`, `BaseCollector.{collect, health_check, validate_config}` | `osiris-plugin` |
| | `osiris/security.py` | `is_blocked_host`, `assert_safe_url`, `fetch_url`, `sanitize_irc_token`, `sanitize_channel`, `sanitize_domain`, `sanitize_hostname`, `safe_title`, `cap_list`, `mask_secret` | `osiris-security` |
| | `osiris/audit.py` | `append_audit`, `verify_chain` | `osiris-hash` + `osiris-db` |
| `osiris-collector` | `manager.py` | `CollectorManager.{load_plugins, run_collection, get_manifest, schedule, start, shutdown}` | `osiris-plugin` + `osiris-server` |
| | `cli.py` | `main` (`--list`, `--run`, `--config`) | `osiris-cli-rs` |
| `osiris-pipeline` | `pipeline.py` | `ProcessingPipeline.{process_one, clean, detect_language, extract_entities, classify, generate_embedding, run, store}` | `osiris-pipeline`, `osiris-extract`, `osiris-hash` |
| | `cli.py` | `main` (`--once`, `--loop`, `--batch`, `--interval`) | `osiris-server` alt komutu |
| `osiris-query` | `engine.py` | `QueryEngine.{fulltext_search, semantic_search, entity_search}` | `osiris-query` |
| `osiris-graph` | `engine.py` | `GraphEngine.{add_relation, add_entity, centrality, communities, neighbors, to_json, export_graphml, export_gexf, save_to_db, load_from_db}` | `osiris-graph` |
| `osiris-alert` | `manager.py` | `AlertManager.{register_handler, mute, unmute, check_item, record_metric, check_anomaly}` (Redis pub/sub `osiris:alerts`) | `osiris-alert` |
| `osiris-report` | `generator.py` | `ReportGenerator.{generate_markdown, generate_json, generate_html, generate_stix, generate_csv, save}` | `osiris-report` |
| `osiris-api` | `auth.py` | `role_satisfies`, `hash_api_key`, `create_token`, `verify_token`, `verify_token_with_role` | `osiris-server` |
| | `main.py` | 25 uç (§3.2) + `resolve_identity`, `require_{viewer,analyst,admin}` | `osiris-server` |
| `osiris-cli` | `cli.py` | `status`, `plugins`, `collect` (çıkış kodları 0/1/2) | `osiris-cli-rs` |

Pipeline sabitleri: varlık tipleri `person, org, location, ip, domain, email, phone, crypto_address, hash, username, cve, custom`. Konu anahtar kelimeleri `threat / breach / vulnerability / phishing`, alt dize eşleşmesiyle (`pipeline.py:42-49`). Dil tespiti `langdetect.detect(text[:500])`, **seed sabitlenmemiş**, yani deterministik değil. Fixture için `DetectorFactory.seed` gerekli.

### 3.2 API uçları (FastAPI 0.2.0, nginx `/api/` önekini sıyırır)

| Yöntem | Yol | Rol | Not |
|---|---|---|---|
| GET | `/health` | — | |
| POST | `/auth/token` | operatör anahtarı (gövdede) | anahtar yoksa 404 |
| POST | `/auth/keys` | admin | ham anahtar bir kez döner |
| GET | `/auth/keys` | admin | ≤200 |
| DELETE | `/auth/keys/{key_id}` | admin | iptal (silmez) |
| GET | `/plugins` | viewer | manifest + config_schema |
| GET | `/stats` | viewer | |
| GET | `/items/recent` | viewer | limit 1-50 |
| GET | `/items/{item_id}` | viewer | +varlıklar ≤200 |
| GET | `/entities/top` | viewer | limit 1-100 |
| POST | `/collect/batch` | analyst | ≤10 iş |
| POST | `/collect` | analyst | 404 / 413 / 502 |
| POST | `/search` | viewer | FTS, sorgu ≤500 |
| POST | `/search/entity` | viewer | |
| GET | `/graph` | viewer | ≤5000 düğüm / 20000 kenar |
| POST | `/graph/relation` | analyst | |
| GET / POST | `/sources` | viewer / analyst | sayfa 1-200 |
| DELETE | `/sources/{source_id}` | analyst | |
| POST | `/sources/{source_id}/collect` | analyst | 409 (devre dışı) |
| GET / POST | `/saved-queries` | viewer / analyst | |
| DELETE | `/saved-queries/{query_id}` | analyst | |
| POST | `/alerts/test` | analyst | |
| POST | `/reports/markdown` | analyst | |

Auth sırası: operatör `X-API-Key` (sabit sürede karşılaştırma) → `Bearer` JWT (HS256 sabit, `role` claim'i; yoksa viewer sayılır) → DB anahtarı (SHA-256). Roller `viewer(1) < analyst(2) < admin(3)`. Dış istemciler: web UI (`osiris-web-ui/src/api.ts`, yalnızca bu uçlar) ve n8n (`/search`, `/collect`).

### 3.3 Plugin'ler (11) — `config_schema`

| Plugin | network_type | Zorunlu | İsteğe bağlı (varsayılan) | Ağ erişimi | Bağımlılık |
|---|---|---|---|---|---|
| `rss` | rss | `feed_url` | `max_items` (50) | `fetch_url` | feedparser, requests |
| `web-scraper` | www | `url` | `depth` (1), `follow_links` (false), `css_selector` | `fetch_url` | requests, bs4 |
| `dns-whois` | api | `domain` | `record_types` (A, AAAA, MX, NS, TXT) | dnspython + ham soket (`whois.iana.org:43`) | dnspython |
| `rest-api` | api | `endpoint` | `method` (GET), `headers` {}, `params` {}, `json_path` | `fetch_url` | requests |
| `shodan` | api | `api_key`, `query` | — | `fetch_url` | requests |
| `blockchain` | blockchain | `rpc_url` | `address`, `chain` (ethereum) | `fetch_url` + `assert_safe_url` | requests |
| `username-search` | www | `username` | `sites`, `tags`, `max_sites` (200), `workers` (10), `timeout` (15), `full` (false), `sites_db` | `fetch_url`, ThreadPool | requests (+ 2514 sitelik `sites_full.json`) |
| `tor` | tor | `url` | `socks5_host` (127.0.0.1), `socks5_port` (9050) | `fetch_url` (`allow_onion`), socks5h | requests[socks], bs4 |
| `i2p` | i2p | `url` | `http_proxy_host` (127.0.0.1), `http_proxy_port` (4444) | `fetch_url` | requests, bs4 |
| `matrix` | matrix | `homeserver`, `room_id` | `access_token` | `fetch_url` | requests |
| `irc` | irc | `server`, `channels` | `port` (6667), `nickname` (osiris-bot) | **ham soket, SSRF yok** | — |

Yükleme: `plugins/*/manifest.json` + `collector.py`, `importlib` ile dinamik yükleniyor. `BaseCollector` alt sınıfı aranıyor, id regex'i `^[a-z0-9][a-z0-9-]{0,63}$`.

### 3.4 Veritabanı (migration 001–002)

Tablolar: `sources`, `items` (`content_hash CHAR(64) UNIQUE`, `embedding VECTOR(1536)`), `entities` (`UNIQUE(type, value)`), `item_entities`, `entity_relations` (**kullanılmıyor**; graf `graph_edges`'e yazıyor), `saved_queries`, `audit_logs`, `source_metrics` (TimescaleDB varsa hypertable oluyor), `users`, `api_keys`, `graph_edges`. Enum'lar: `network_type`, `entity_type`, `query_type`, `user_role`.

### 3.5 Altyapı

compose: postgresql (pgvector/pg16), redis 7.4, minio, n8n 1.114.4, freshrss 1.24.0, osiris-api (profil `api`/`full`), osiris-web-ui (profil `ui`/`full`), nginx 1.31 (80 + 8443 TLS). CI işleri: `python-tests` (kapsam ≥%80), `lint` (ruff + doküman bağlantı kontrolü), `security-audit` (pip-audit, `|| true`), `cpp-build`, `web-ui-build`.

---

## 4. Testler ve kapsam

Ölçüm: Python 3.11.16, temiz venv, `pytest -q` + `ruff check` (CI ile aynı hedefler).

- **154 test, hepsi geçti** (4,7 sn). `ruff check`: temiz.
- **Toplam kapsam %85** (1861 satır, 272'si kapsanmıyor). CI eşiği %80.

| Modül | Kapsam | | Modül | Kapsam |
|---|---|---|---|---|
| `osiris/security.py` | %93 | | `osiris_api/main.py` | %79 |
| `osiris/audit.py` | %84 | | `osiris_api/auth.py` | %92 |
| `osiris_pipeline/pipeline.py` | %82 | | `osiris_collector/manager.py` | %93 |
| `osiris_pipeline/cli.py` | **%0** | | `osiris_collector/cli.py` | %78 |
| `osiris_graph/engine.py` | %96 | | `osiris_query/engine.py` | %98 |
| `osiris_alert/manager.py` | %96 | | `osiris_report/generator.py` | %94 |

- **Plugin'lerin 10'u test edilmiyor.** Yalnızca `username-search`'ün testi var ve plugin kodu kapsam ölçümüne dahil değil. R2'deki `fixtures/plugins/` maddesi bu boşluğu kapatır.
- Gerçek bir DB, Redis veya HTTP'ye karşı entegrasyon testi yok. Hepsi mock. R2'deki `tests/contract/` ilk kara kutu katmanı olacak.
