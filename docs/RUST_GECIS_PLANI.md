# OSIRIS — Rust Geçiş Planı (Claude Code için)

> Bu dosyayı repoya `docs/RUST_GECIS_PLANI.md` olarak koy ve Claude Code'a bu dosyayı izlemesini söyle.
> Plan, `docs/` incelemesi ve repo klasör yapısı üzerine kuruldu. Kodun ayrıntıları ilk fazda (R0) Claude Code tarafından doğrulanacak.

---

## 0. Claude Code için çalışma kuralları

Bu bölüm Claude Code'a yöneliktir. Her oturumda önce burayı oku.

1. **Fazları sırayla uygula.** Bir faz bitip kabul kriterleri sağlanmadan sonrakine geçme.
2. **Her faz ayrı dalda yapılır** (`rust/r0-envanter`, `rust/r1-duzeltmeler` …). Faz sonunda PR aç, `master`'a doğrudan push etme.
3. **Commit mesajları Türkçe** ve `Rust R<n>: ...` önekli (örn. `Rust R2: audit zinciri JCS kanonik JSON'a geçti`). Her mantıksal değişiklik ayrı commit.
4. **Her commit öncesi:** Python tarafında `pytest -q` yeşil + `ruff check .` temiz; Rust tarafında `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` yeşil.
5. **Mevcut sistemi bozma.** Canlı bir self-host kurulumu var. Geçiş boyunca DB şeması geriye uyumlu kalmalı, yeni migration'lar yalnızca ekleme yapmalı (sütun silme/yeniden adlandırma ancak R7'de).
6. **Sır commitleme.** `.env`, `*.key`, `*.crt` ignore'da kalır.
7. **Karar noktalarında dur ve sor.** Bölüm 1'deki tabloda "Karar" yazan bir maddeye geldiğinde, kullanıcı cevap vermediyse önerilen seçeneği uygulama; seçenekleri özetle ve sor.
8. **Crate/paket sürümlerini tahmin etme.** Eklerken crates.io / PyPI'daki güncel kararlı sürümü kontrol et.
9. Her faz sonunda `docs/RUST_GECIS_DURUMU.md` dosyasını güncelle: ne yapıldı, ne kaldı, sapmalar.

---

## 1. Karar noktaları (başlamadan önce kullanıcı onaylamalı)

| # | Konu | Önerilen | Alternatif | Etki |
|---|------|----------|-----------|------|
| K1 | C++ çekirdek (`osiris-core`) | Kaldır; işlevini Rust servisi devralır | Rust'a birebir taşı | R4'te silinir |
| K2 | MinIO | Kullanılmıyorsa compose'dan çıkar | İleride ham HTML/ek dosya arşivi için tut | R3 |
| K3 | FreshRSS | RSS plugin'i yeterliyse çıkar | Tut, pipeline'a bağla | R3 |
| K4 | NER modeli | ONNX formatlı çok dilli transformer NER (HF) | spaCy'de kal, NER'i ayrı Python servisi yap | R3 |
| K5 | Embedding modeli | `fastembed` uyumlu çok dilli küçük model (örn. multilingual-e5-small, 384 boyut) | Daha büyük model (768) | R3 + migration |
| K6 | Dış plugin yükleme | Trait ile derlemeye gömülü + ileride WASM (`extism`) | Yalnızca gömülü | R5 |
| K7 | Veri saklama süresi (KVKK) | `items` için varsayılan 180 gün, kaynak başına ayarlanabilir | Süresiz + manuel silme | R2 |
| K8 | Qt / Tauri istemcileri | Geçiş bitene kadar dondur | Paralel geliştir | — |
| K9 | API anahtarı yoksa davranış | Başlatmayı reddet; `OSIRIS_INSECURE_OPEN=1` ile açıkça izin | Mevcut "açık mod" | R1 |

---

## 2. Hedef mimari

```
osiris/                      (Cargo workspace, repo kökünde)
├── Cargo.toml               [workspace]
├── crates/
│   ├── osiris-types/        modeller (Source, Item, Entity…), serde, JSON Schema
│   ├── osiris-security/     SSRF denetimi, sanitizer'lar, URL doğrulama
│   ├── osiris-hash/         content_hash normalizasyonu, JCS kanonik JSON, audit zinciri
│   ├── osiris-extract/      regex varlık çıkarma (+ ONNX NER, feature: "ner")
│   ├── osiris-db/           sqlx sorguları, migration çalıştırıcı
│   ├── osiris-plugin/       Collector trait'i, plugin kaydı, HTTP host fonksiyonu
│   ├── osiris-plugins/      11 collector (her biri modül/feature)
│   ├── osiris-pipeline/     temizle → dil → çıkar → sınıflandır → embed → tekilleştir → depola
│   ├── osiris-query/        FTS, semantik, varlık araması
│   ├── osiris-graph/        petgraph/rustworkx-core, kalıcılık
│   ├── osiris-alert/        kayıtlı sorgular, z-skor anomali
│   ├── osiris-report/       minijinja şablonları, CSV, STIX 2.1
│   ├── osiris-py/           PyO3 köprüsü (geçiş dönemi; R7'de silinir)
│   └── osiris-server/       axum API + scheduler + pipeline worker (tek binary)
├── osiris-cli-rs/           clap tabanlı CLI (R6'da `osiris` adını devralır)
├── osiris-web-ui/           React — değişmez
├── fixtures/                diller arası ortak altın test verisi
└── tests/contract/          kara kutu API testleri (pytest + httpx)
```

**Hedef çalışma şekli:** tek `osiris-server` binary'si (API + zamanlayıcı + pipeline işçisi, alt komutlarla ayrı süreç olarak da çalışabilir), PostgreSQL + pgvector, Redis, Nginx. Python yok.

### Kütüphane eşlemesi

| İşlev | Python (bugün) | Rust (hedef) |
|------|---------------|--------------|
| REST API | FastAPI + Pydantic | `axum` + `serde` + `validator`, OpenAPI: `utoipa` |
| CLI | Click + Rich | `clap` + `comfy-table`/`indicatif` |
| Async runtime | — | `tokio` |
| Cron | APScheduler | `tokio-cron-scheduler` |
| PostgreSQL | psycopg | `sqlx` (compile-time kontrollü sorgular) |
| pgvector | pgvector-python | `pgvector` crate (sqlx özelliği) |
| Redis kuyruk | redis-py (LIST) | `redis` crate, **Streams + consumer group** |
| HTTP / Tor SOCKS5 | requests | `reqwest` (`socks` özelliği) |
| RSS/Atom | feedparser | `feed-rs` |
| DNS | dnspython | `hickory-resolver` |
| HTML | beautifulsoup4 | `scraper` |
| Dil tespiti | langdetect | `lingua` (doğruluk) veya `whatlang` (hız) |
| NER | spaCy | `ort` + `tokenizers` (ONNX) |
| Embedding | sentence-transformers | `fastembed` (Rust) |
| Graf | networkx | `petgraph` + `rustworkx-core` |
| Şablon | Jinja2 | `minijinja` |
| JWT | stdlib HS256 | `jsonwebtoken` |
| Hash | hashlib | `sha2` |
| Kanonik JSON | — | `serde_json_canonicalizer` (RFC 8785) |
| UUIDv5 (STIX) | uuid | `uuid` (`v5` özelliği) |
| Log / metrik | logging | `tracing` + `tracing-subscriber` + `metrics-exporter-prometheus` |
| Python köprüsü | — | `pyo3` + `maturin` |

---

## 3. Fazlar

### R0 — Envanter ve doğrulama (kod değişikliği yok)

**Amaç:** Planın varsayımlarını gerçek kodla doğrulamak.

Görevler:
- [ ] Her Python modülünün public fonksiyonlarını, plugin'lerin `config_schema`'larını ve API uçlarını çıkar → `docs/RUST_GECIS_ENVANTER.md`.
- [ ] Şunları **kodda** doğrula ve sonucu yaz:
  - `osiris:raw_items` Redis LIST mi, Stream mi?
  - `content_hash` tam olarak hangi normalizasyondan sonra hangi alanlardan hesaplanıyor?
  - Audit zincirinde hash girdisi nasıl serileştiriliyor (anahtar sırası, `separators`, float)?
  - STIX deterministik UUID'leri hangi sürüm/namespace/girdiyle üretiliyor?
  - Topluluk tespiti algoritması (Louvain? greedy modularity?) ve seed.
  - MinIO'yu kullanan kod var mı? FreshRSS'e bağlı kod var mı? C++ core'u çağıran bir şey var mı?
  - Embedding modeli ve gerçek boyutu; `VECTOR(1536)` ile uyuşuyor mu?
- [ ] Mevcut test sayısı ve kapsam raporu.

**Kabul:** Envanter dosyası var; K1–K3 kararları için gereken bilgiler (kullanım var/yok) netleşti. Kullanıcıya özet sunuldu.

---

### R1 — Doküman ve hızlı güvenlik düzeltmeleri (Python)

Görevler:
- [ ] `docs/TEKNIK_DOKUMAN.tr.md` §2.7 yazım hatası: "zaman aşJRımlı" → "zaman aşımlı".
- [ ] Var olmayan `docs/OSIRIS_Technical_Document.md` referanslarını iki dilde kaldır.
- [ ] Mimari diyagramındaki `[P2P]` kutusunu kaldır (planlı, gerçekleşmedi).
- [ ] "Zamanlama C++ core'da" ifadesini düzelt (gerçek: APScheduler, Python).
- [ ] §5.8'deki eksik uç tablosunu kaldırıp yalnızca `API.md` bağlantısı bırak.
- [ ] `API.md`/`API.tr.md` üretim taban URL'si → `https://<sunucu>:8443/api`.
- [ ] Türkçe dokümana `stop_scheduler` adını ekle.
- [ ] `CONTRIBUTING` kurulum adımlarına `pip install ruff` ekle.
- [ ] **K9:** `OSIRIS_API_KEY` yoksa API başlamasın; yalnızca `OSIRIS_INSECURE_OPEN=1` ile açık mod. Test ekle.
- [ ] `OSIRIS_JWT_SECRET` yoksa API anahtarına düşme yedeğini kaldır; JWT uçları 503 dönsün veya başlatma reddedilsin. Test ekle.

**Kabul:** İki dilde dokümanlar tutarlı; yeni güvenlik testleri yeşil; `.env.example` ve dokümanlar yeni davranışı anlatıyor.

---

### R2 — Geçiş sözleşmesi (dilden bağımsız tanımlar, Python'da uygulanır)

Bu faz geçişin en önemli kısmı: buradan sonra Python ve Rust aynı çıktıyı üretmek zorunda.

Görevler:
- [ ] `docs/SOZLESME.md` yaz:
  - **content_hash:** Unicode NFC → HTML soyma → boşluk daraltma → trim → (küçük harf? R0 sonucuna göre) → UTF-8 → SHA-256 hex. Adımlar kesin ve sıralı.
  - **Audit zinciri:** kayıt **RFC 8785 (JCS)** ile kanonikleştirilir; `hash = sha256(prev_hash || jcs(kayıt))`. Eski kayıtlar için `chain_version` alanı; doğrulayıcı iki sürümü de tanır.
  - **STIX UUID:** UUIDv5, namespace ve girdi formatı sabit.
  - **Plugin sözleşmesi:** `manifest.json`, config ve `CollectionResult` için JSON Schema dosyaları → `schemas/`.
  - **Kuyruk mesajı:** `osiris:raw_items` mesaj formatının JSON Schema'sı.
- [ ] Python'da audit zincirini JCS'ye geçir (`chain_version=2`), eski zincir doğrulanmaya devam etsin.
- [ ] `fixtures/` oluştur (altın veriler):
  - `fixtures/hash/` — girdi metin → beklenen `content_hash`
  - `fixtures/extract/` — girdi metin → beklenen varlık listesi
  - `fixtures/audit/` — kayıt dizisi → beklenen hash zinciri
  - `fixtures/stix/` — rapor girdisi → beklenen STIX paketi (byte byte)
  - `fixtures/graph/` — küçük graflar → beklenen merkezilik/topluluk çıktıları
  - `fixtures/plugins/` — plugin başına kaydedilmiş HTTP yanıtı → beklenen `CollectionResult`
- [ ] Python testleri bu fixture'ları okuyarak doğrulasın.
- [ ] **Kara kutu API testleri:** `tests/contract/` altında, `OSIRIS_BASE_URL` ortam değişkeniyle çalışan servise gerçek HTTP atan pytest paketi. Auth matrisi (401/403/200), tüm uçlar, hata kodları (400/404/413/422/502), sayfalama sınırları.
- [ ] **K7 (KVKK):** `items` için saklama süresi (kaynak başına `retention_days`, varsayılan ayarlanabilir), periyodik temizlik görevi, `DELETE /items/{id}` ve varlık bazlı silme ucu (admin). Migration `003_retention.sql`.
- [ ] username-search: site başına istek temposu (rate limit) ayarı.

**Kabul:** `SOZLESME.md` + `schemas/` + `fixtures/` var; Python tüm fixture testlerini geçiyor; contract testleri canlı/dev API'ye karşı yeşil.

---

### R3 — Geçiş dostu altyapı değişiklikleri (Python)

Görevler:
- [ ] **K4:** spaCy'yi kaldır; ONNX NER modelini `onnxruntime` + `tokenizers` ile çalıştır. Model dosyası ve tokenizer `models/` altında (git LFS veya indirme betiği; repoya ikili dosya gömme). Regex çıkarıcı aynen kalır; NER yalnızca person/org/location.
- [ ] **K5:** sentence-transformers → `fastembed`. Migration `004_embedding.sql`: yeni boyutta `embedding_v2` sütunu + `embedding_model` metin sütunu + yeni HNSW indeksi. Arka plan betiği eski öğeleri yeniden embed eder; bitince sorgular `embedding_v2`'ye geçer.
- [ ] **Redis Streams:** `osiris:raw_items` → Stream + consumer group (`XREADGROUP`/`XACK`, bekleyenleri `XAUTOCLAIM` ile geri alma). Geçiş sırasında eski LIST'i boşaltan uyumluluk okuyucusu.
- [ ] **K2/K3:** R0 sonucuna ve kullanıcı kararına göre MinIO/FreshRSS'i compose'dan çıkar, dokümanları güncelle.
- [ ] Tor/I2P proxy'lerinin compose'da nasıl çalıştığını netleştir (servis ekle veya host-ağı gereksinimini dokümante et).

**Kabul:** Tüm testler + fixture'lar + contract testleri yeşil; NER/embedding çıktıları yeni fixture'larla sabitlendi.

---

### R4 — Rust iskeleti ve PyO3 köprüsü

Görevler:
- [ ] Repo köküne Cargo workspace kur (Bölüm 2 yapısı). `rust-toolchain.toml` ile kararlı sürümü sabitle.
- [ ] CI (`.github/workflows/`): `cargo fmt --check`, `clippy -D warnings`, `cargo test`, `cargo deny` (lisans/güvenlik). Python işleri aynen kalır.
- [ ] Önce **saf ve güvenlik kritik** crate'leri yaz:
  - `osiris-security` — SSRF denetimi (özel/loopback/link-local/metadata IP, URL içi kimlik, şema allowlist, fail-closed DNS), sanitizer'lar.
  - `osiris-hash` — `content_hash`, JCS, audit zinciri doğrulama.
  - `osiris-extract` — regex varlık çıkarma (IP doğrulama, CVE, kripto adresleri vb.).
- [ ] Bu crate'ler `fixtures/` altındaki altın verilerin hepsini geçmeli.
- [ ] `osiris-py` (PyO3 + maturin): bu üç crate'i Python'a aç. Python SDK'daki karşılık gelen fonksiyonlar Rust uygulamasını çağırsın (özellik bayrağıyla; varsayılan Rust, hata durumunda Python yedeğine düşmez — testler iki yolu da koşar, sonra Python uygulaması silinir).
- [ ] **K1:** C++ core'u kaldır (`osiris-core/`, CMake, CI adımı, dokümanlar).

**Kabul:** Python sistemi artık güvenlik/hash/regex için Rust kodunu kullanıyor; tüm Python testleri + contract testleri yeşil; C++ yok.

---

### R5 — Veri katmanı Rust'ta (collector + plugin'ler + pipeline)

Görevler:
- [ ] `osiris-types`: modeller, `schemas/` JSON Schema'larıyla uyumlu (`schemars` ile şema üretip karşılaştıran test).
- [ ] `osiris-db`: sqlx ile sorgular; mevcut migration'ları değiştirmeden kullan.
- [ ] `osiris-plugin`: `Collector` trait'i (`collect`, `health_check`, `manifest`). Plugin'ler **ağa doğrudan erişmez**; HTTP yalnızca host'un sağladığı istemciden geçer ve SSRF denetimi orada zorunlu. Zaman aşımı ve yanıt boyutu sınırı istemcide.
- [ ] `osiris-plugins`: 11 plugin'i taşı, sıra: `rss` → `web-scraper` → `dns-whois` → `rest-api` → `shodan` → `blockchain` → `username-search` → `tor` → `i2p` → `matrix` → `irc`. Her biri `fixtures/plugins/` testlerini geçmeli. `manifest.json` dosyaları aynen kalır (derleme zamanında gömülür).
- [ ] Collector servisi: `tokio-cron-scheduler`, kaynak sağlık yazımı (`sources`, `source_metrics`), crash izolasyonu (`catch_unwind` + görev başına `tokio::spawn`), Redis Stream'e yazma.
- [ ] `osiris-pipeline`: Stream'den oku → temizle → dil → regex + ONNX NER (`ort`) → sınıflandır → `fastembed` → tekilleştir → depola → `XACK`.
- [ ] **Paralel çalışma:** Rust collector/pipeline, Python'unkiyle aynı DB ve Stream'i kullanır. Önce gölge modda (yazmadan, çıktıyı karşılaştırarak) çalıştır, sonra Python bileşenlerini kapat.
- [ ] **K6:** WASM plugin desteği bu fazda yalnızca tasarım notu olarak `docs/`'a yazılır; uygulama sonraya.

**Kabul:** Rust collector + pipeline üretimde Python'unkilerin yerini aldı; çıktılar fixture ve gölge karşılaştırmasıyla eşleşti; Python collector/pipeline devre dışı.

---

### R6 — API, CLI ve analiz katmanı Rust'ta

Görevler:
- [ ] `osiris-query`, `osiris-graph` (merkezilik, topluluk — R2 fixture'larıyla aynı sonuç), `osiris-alert` (z-skor, sessize alma, e-posta/Telegram/webhook işleyicileri), `osiris-report` (Markdown/HTML/JSON/CSV/STIX; CSV formül koruması, dizin kilidi).
- [ ] `osiris-server` (axum): tüm uçlar, auth (operatör anahtarı, DB anahtarları SHA-256, JWT HS256 sabit alg), RBAC, CORS allowlist, istek sınırları (≤50 anahtar, ≤20 KB, sorgu ≤500 karakter), hata gövdelerinde iç detay yok, best-effort audit, OpenAPI (`utoipa`) + `/docs`.
- [ ] **API anahtarı başına kota** (`governor` crate), Nginx IP sınırına ek.
- [ ] `/metrics` (Prometheus) ve yapılandırılmış JSON log (`tracing`).
- [ ] `tests/contract/` paketi Rust API'ye karşı **değiştirilmeden** yeşil olmalı.
- [ ] `osiris-cli-rs` (clap): `status`, `plugins`, `collect`; aynı çıkış kodları (0/1/2).
- [ ] Nginx'te `/api/*` hedefini Rust servisine çevir; Python API'yi kapat.

**Kabul:** Contract testleri Rust API'ye karşı yeşil; web UI değişiklik gerektirmeden çalışıyor; N8N iş akışı çalışıyor.

---

### R7 — Temizlik ve sadeleştirme

Görevler:
- [ ] Python modüllerini ve `osiris-py` köprüsünü sil; `pyproject.toml`'ı kaldır (contract testleri için küçük bir `tests/contract/requirements.txt` kalır).
- [ ] Eski `embedding` sütununu ve audit `chain_version=1` desteğini (kullanıcı onayıyla) kaldıran migration.
- [ ] Docker: çok aşamalı build, `distroless`/`scratch` tabanlı tek imaj; compose sadeleşir.
- [ ] Dokümanları (iki dil) Rust mimarisine göre yeniden yaz: TEKNIK_DOKUMAN, ARCHITECTURE, API, CONTRIBUTING (Rust geliştirme ortamı, plugin yazma rehberi).
- [ ] Sürüm: `1.0.0-rc`.

**Kabul:** Repoda Python uygulama kodu yok; tüm Rust testleri + contract testleri yeşil; taze kurulum dokümandan adım adım çalışıyor.

---

## 4. Riskler ve önlemler

| Risk | Önlem |
|------|-------|
| Audit zinciri diller arasında kırılır | R2'de JCS + `chain_version`; fixture'larla doğrulama |
| `content_hash` farkı → aynı içerik iki kez | R2'de kesin tanım; R5'te gölge mod karşılaştırması |
| Embedding'ler uyuşmaz → semantik arama bozulur | Aynı ONNX modeli iki dilde (`fastembed`); `embedding_model` sütunu |
| NER sonuçları değişir | R3'te modeli değiştir ve yeni fixture'larla sabitle, R5'te aynı model |
| Canlı sistem kesintisi | Ekleyici migration'lar, gölge mod, bileşen bileşen geçiş, Nginx ile anahtarlama |
| Kapsam büyür, iş bitmez | K8 (istemcileri dondur); her faz kendi başına değer üretir, R4'te durulsa bile kazanç kalır |
| Hukuki risk (KVKK, site şartları) | R2'de saklama/silme ve tempo ayarı |

---

## 5. Claude Code'a ilk mesaj (örnek)

```
docs/RUST_GECIS_PLANI.md dosyasını oku. Bölüm 0'daki kurallara uy.
Önce Bölüm 1'deki karar noktalarını bana özetle ve hangilerini onayladığımı sor.
Ardından R0 fazını uygula: kod değiştirme, yalnızca envanter dosyasını çıkar
ve R0'daki doğrulama sorularının cevaplarını raporla. R0 bitince dur.
```
