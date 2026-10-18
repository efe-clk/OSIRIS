# OSIRIS — Rust Geçişi Durumu

Plan: [RUST_GECIS_PLANI.md](RUST_GECIS_PLANI.md) · Envanter: [RUST_GECIS_ENVANTER.md](RUST_GECIS_ENVANTER.md) · Sözleşme: [SOZLESME.md](SOZLESME.md)

> **Tarihsel kayıt.** Bu belge, plan ve envanter Python → Rust geçişinin (R0–R7, 2026-10-16/18)
> aşama aşama kaydıdır ve yazıldıkları anı yansıtır; artık var olmayan Python dosyalarına ve
> komutlarına başvurabilirler. Sistemin **bugünkü** hâli için:
> [TEKNIK_DOKUMAN.tr.md](TEKNIK_DOKUMAN.tr.md) (kurulum ve işletim), [ARCHITECTURE.tr.md](ARCHITECTURE.tr.md),
> [API.tr.md](API.tr.md), [PLUGINS.tr.md](PLUGINS.tr.md). Geçişten sonra eklenen web arayüzü uçları
> (`/auth/me`, semantik arama, kaynak detayı/güncelleme, graf analizi/dışa aktarım, çok biçimli
> rapor) API belgesinin değişiklik geçmişindedir.

| Faz | Durum | Dal |
|---|---|---|
| R0 — Envanter | ✅ Tamamlandı, kararlar onaylandı | `master` |
| R1 — Doküman + güvenlik düzeltmeleri | ✅ Tamamlandı | `master` |
| R2 — Geçiş sözleşmesi | ✅ Tamamlandı | `master` |
| R3 — Geçiş dostu altyapı | ✅ Tamamlandı | `master` |
| R4 — Rust iskeleti + PyO3 köprüsü | ✅ Tamamlandı | `master` |
| R5 — Veri katmanı Rust'ta | ✅ Tamamlandı (canlı geçiş operatörde) | `master` |
| R6 — API, CLI ve analiz katmanı Rust'ta | ✅ Tamamlandı (canlı geçiş operatörde) | `master` |
| R7 — Temizlik ve sadeleştirme | ✅ Tamamlandı — sürüm 1.0.0-rc (canlı yükseltme operatörde) | `master` |

## Kararlar (kullanıcı onayı, 2026-10-16)

- **K1–K9:** hepsi önerildiği gibi (C++, MinIO ve FreshRSS kaldırılacak; ONNX NER; fastembed 384 boyut; gömülü trait + ileride WASM; 180 gün saklama, kaynak başına; Qt/Tauri donduruldu; anahtarsız API başlamaz).
- **content_hash:** bugünkü gibi kalacak (NFC yok, küçük harf yok). R2 sözleşmesi mevcut davranışı birebir tanımlayacak, hash sürümü artırılmayacak.
- **IRC SSRF (E5):** R1'e alındı.
- **İş akışı sapması:** Kullanıcının isteğiyle faz dalı ve PR açılmıyor, commit'ler doğrudan `master`'a push ediliyor (plandaki Bölüm 0 kural 2'nin yerine geçer).

## R0 (2026-10-16)

**Yapılanlar**
- Plan `docs/RUST_GECIS_PLANI.md` olarak repoya eklendi.
- `docs/RUST_GECIS_ENVANTER.md` hazırlandı: R0'daki 7 doğrulama sorusunun cevabı, modül, API uçları, plugin ve DB envanteri, test ve kapsam raporu.
- Uygulama kodu değişmedi. Testler 154/154 geçti, kapsam %85, ruff temiz.

**Plandan sapmalar ve plana etkiler**
- `content_hash` için "küçük harf?" sorusunun cevabı **hayır**. NFC de uygulanmıyor. R2 sözleşmesi bugünkü davranışı (NFC'siz) mı koruyacak, yoksa NFC eklenip hash sürümü mü artırılacak, karar verilmeli. NFC eklemek, aynı içeriğin mevcut hash'lerle eşleşmemesine yol açar.
- STIX çıktısı bugün deterministik değil (`uuid4` bundle kimliği, `now()` zaman damgaları). R2'deki "byte byte" fixture için önce bunların enjekte edilebilir yapılması gerekiyor.
- `items.source_id` her zaman NULL. K7'deki "kaynak başına saklama" bu düzeltilmeden çalışamaz. R2'ye bir ön madde olarak eklenmeli.
- Compose'da pipeline işçisi ve zamanlayıcı yok. Cron zamanlaması koda bağlı değil (envanter E1, E2).
- IRC plugin'inde SSRF açığı var (E5). R1 kapsamına eklenmesi önerilir.
- K8'in (Qt/Tauri) pratik etkisi yok, repoda bu istemciler bulunmuyor.

**Kalanlar**
- E1'deki sorunun cevaplanması: canlı kurulumda `osiris:raw_items` kuyruğunu ne boşaltıyor? (açık)

## R1 (2026-10-16)

**Yapılanlar**
- Dokümanlar (iki dil): `aşJRımlı` yazım hatası, `OSIRIS_Technical_Document.md` referansı, diyagramdaki `[P2P]` kutusu (`[IRC]` oldu, README dahil), "zamanlama C++ core'da" ifadesi (APScheduler), §5.8'deki eksik uç tablosu (API.md bağlantısı kaldı), API taban URL'si `https://<sunucu>:8443/api`, TR dokümana `stop_scheduler`, CONTRIBUTING'e `ruff`.
- **K9:** `OSIRIS_API_KEY` yoksa API lifespan'da başlamayı reddediyor. Açık mod yalnızca `OSIRIS_INSECURE_OPEN=1` ile açılıyor. Başlatma denetimi atlanırsa uçlar 503 dönüyor (fail-closed).
- **JWT:** API anahtarına düşme yedeği kaldırıldı. `OSIRIS_JWT_SECRET` yoksa `/auth/token` 503 dönüyor (önceden 404) ve API anahtarıyla imzalanmış jetonlar reddediliyor. compose artık `OSIRIS_JWT_SECRET` ve `OSIRIS_INSECURE_OPEN` değerlerini `osiris-api`'ye geçiriyor.
- **SSRF:** SDK'ya `resolve_safe_ips` ve `safe_create_connection` eklendi, IRC bunları kullanıyor. Ek bulgu: `is_blocked_host` IPv4-mapped IPv6 (`::ffff:127.0.0.1`) ve `::` adreslerini engellemiyordu, yani `http://[::ffff:127.0.0.1]/` bütün HTTP plugin'lerinde SSRF korumasından geçiyordu. Bu da kapatıldı.
- Testler 154 → 186, hepsi geçiyor. Kapsam %85,9, ruff temiz. Yeni testlerin eski kodda kırıldığı doğrulandı.

**Sapmalar**
- JWT için plandaki "503 veya başlatma reddi" seçeneklerinden **503** seçildi. JWT isteğe bağlı bir özellik, API anahtarları çalışmaya devam ediyor.
- IPv4-mapped IPv6 açığı planda yoktu, R1 kapsamına eklendi.

**⚠ Canlı kurulum için dağıtım notu**
- Canlı `.env` dosyasında `OSIRIS_API_KEY` boşsa, yeni imajla `osiris-api` **başlamaz**. Dağıtımdan önce bir anahtar atanmalı (ya da bilerek `OSIRIS_INSECURE_OPEN=1` verilmeli).
- Önceki compose `OSIRIS_JWT_SECRET` değerini konteynere geçirmiyordu, yani canlıdaki JWT'ler API anahtarıyla imzalanıyordu. Yeni sürümde `.env` içine ayrı bir `OSIRIS_JWT_SECRET` eklenmezse `/auth/token` 503 döner ve eski jetonlar geçersiz olur. Web UI ve n8n `X-API-Key` kullandığı için bundan etkilenmez.

## R2 (2026-10-16)

**Yapılanlar**
- **Ön madde — `items.source_id`:** collector kaynak kimliğini kuyruk mesajına koyuyor, pipeline öğeye yazıyor. Kaynak bu arada silindiyse foreign key hatası yerine NULL yazılıyor.
- **`docs/SOZLESME.md`:** content_hash (bugünkü davranış: NFC yok; Python `\s` kümesi açıkça listelendi, U+001C–001F Rust'tan farklı), audit v1/v2, STIX UUID'leri, plugin, kuyruk, KVKK, API, regex ve graf sözleşmeleri.
- **`schemas/`:** manifest, CollectedItem, CollectionResult ve kuyruk mesajı için JSON Schema. 11 manifest, fixture sonuçları ve gerçek kuyruk mesajı bu şemalara karşı doğrulanıyor.
- **Audit `chain_version=2`:** RFC 8785 (JCS) kullanılıyor (`rfc8785` 0.1.4). Migration 003 eklendi, karışık v1/v2 zincir doğrulanıyor. jsonb'nin sayıları normalize etmesi doğrulamayı kırmıyor (gerçek PG testinde kanıtlandı).
- **STIX:** saat ve bundle kimliği dışarıdan verilebilir, böylece bayt bayt fixture mümkün.
- **`fixtures/`:** hash (23), extract (10), audit (3 zincir), stix (2), graph (7) ve 11 plugin için 23 senaryo. `tests/golden` altında harness, `generate.py --check` ve testler var.
- **`tests/contract/`:** kara kutu API paketi (144 test). Gerçek bir API'ye karşı hepsi geçti. Kasıtlı olarak anahtarsız açık modda çalıştırılan API'de 60 test kırıldı, yani paket gerçekten ayırt ediyor.
- **`tests/integration/`:** gerçek PostgreSQL testleri (yalnızca `*_test` DB'de çalışıyor). CI'a PostgreSQL ve Redis servisli bir `integration` işi eklendi. Bu iş migration'ları iki kez uygulayıp entegrasyon ve sözleşme testlerini koşuyor.
- **K7 KVKK:**
  - `sources.retention_days` (migration 004) ve `OSIRIS_RETENTION_DAYS` eklendi.
  - Temizlik pipeline'da çalışıyor: `--purge` ve döngü içinde periyodik olarak.
  - Yeni admin uçları: `DELETE /items/{id}`, `POST /entities/erase`, `POST /admin/retention/run`.
  - Hiçbir öğeye bağlı kalmayan varlıklar da siliniyor. Audit kaydına silinen değer değil, yalnızca SHA-256 özeti yazılıyor.
- **username-search temposu:** `rate_per_sec` (varsayılan 10/sn) ve `per_host_interval_ms` (varsayılan 1000) eklendi.
- Testler 186 → 274 birim testi + 6 entegrasyon + 144 sözleşme testi. Ruff temiz, kapsam eşiği sağlanıyor.

**Sapmalar ve ek bulgular**
- **Migration numaraları:** plan `003_retention.sql` diyordu. Audit sürümü 003, saklama 004 oldu. R3'teki embedding migration'ı artık **005**.
- **Golden'ların ortaya çıkardığı iki varlık çıkarma hatası düzeltildi:** Domain regex büyük harfli domain'leri hiç bulmuyordu. ETH adresinin içindeki rakamlar telefon numarası sayılıyordu. Bunlar `content_hash`'i etkilemiyor.
- **E13 (yeni güvenlik bulgusu):** username-search'teki `sites_db`, sunucuda herhangi bir dosya yolunu kabul ediyordu (analyst rolüyle rastgele JSON okuma). Artık yalnızca plugin dizini veya `OSIRIS_USERNAME_DB_DIR` altındaki dosyalar kabul ediliyor.
- KVKK uçları ve saklama modülü SDK'ya (`osiris/retention.py`) kondu. API imajı `osiris-pipeline`'ı (spaCy, pandas) kurmuyor.
- **R5 için karar maddesi:** `rest-api` ve `shodan`, `raw_content` alanına Python repr çıktısı yazıyor (SOZLESME §4). Rust bunu ya birebir taklit edecek ya da JSON'a geçilecek. JSON'a geçmek bu iki plugin'in yeni öğelerinin hash'ini değiştirir.
- **Öneri:** `beautifulsoup4` sürümü sabitlenmeli (`>=4.12,<5`). bs4'ün davranışı content_hash'in parçası.

**⚠ Canlı kurulum için dağıtım adımları (R2)**
1. Migration'ları sırayla uygula (ikisi de yalnızca ekleme yapıyor ve tekrar çalıştırılabilir):
   `docker exec -i osiris-postgresql psql -U osiris -d osiris -v ON_ERROR_STOP=1 < db/migrations/003_audit_chain_version.sql`, ardından aynısını `004_retention.sql` için.
   Sırası önemli değil: Migration'lar uygulanmadan API güncellenirse audit v1'e düşer, `/sources` da `retention_days` olmadan çalışır.
2. `.env` içinde `OSIRIS_RETENTION_DAYS` değerini gözden geçir. Varsayılan **180 gün**. Temizlik ilk kez çalıştığında 180 günden eski öğeler **kalıcı olarak silinir**. İstemiyorsan önce `0` yap.
3. Temizliği çalıştıran bir pipeline servisi compose'da yok (E1). İki yol var: `osiris-pipeline --loop` çalıştırmak ya da n8n/cron ile `POST /api/admin/retention/run` uç noktasını tetiklemek.
4. Sözleşme testleri canlıya karşı güvenle çalıştırılabilir (`OSIRIS_CONTRACT_DESTRUCTIVE` verme): `OSIRIS_BASE_URL=https://<sunucu>:8443/api OSIRIS_CONTRACT_API_KEY=… OSIRIS_CONTRACT_VERIFY_TLS=0 pytest tests/contract`. Testler `contract-*` adlı kullanıcılar bırakır, anahtarları iptal edilmiş olur. *(R7 notu: Nginx IP hız sınırı nedeniyle paketi doğrudan API portuna — `http://127.0.0.1:8001` — koşun.)*

**Kalanlar**
- E1 → R3'te kapandı (kullanıcı: kuyruğu boşaltan bir şey yok; `worker` profilli servis eklendi).

## R3 (2026-10-16)

**Kararlar:** K4 modeli olarak kullanıcının seçtiği Davlan mBERT-hrl ONNX (int8, AFL-3.0, repoda dağıtılmıyor) kullanıldı. E1'e verilen cevap "kuyruğu boşaltan bir şey yok / bilmiyorum" oldu; bu yüzden yalnızca açıkça başlatılınca çalışan `worker` profilli bir servis eklendi.

**Yapılanlar**
- **K2/K3:** MinIO ve FreshRSS compose, nginx `/rss/`, `.env.example` ve dokümanlardan (iki dil) kaldırıldı. `nginx -t` doğrulandı.
- **Tor/I2P (E6):**
  - `darknet` profilli `tor` (alpine + tor, yalnızca istemci) ve `i2pd` servisleri eklendi. Host'a port yayınlanmıyor.
  - Plugin'ler artık `OSIRIS_TOR_SOCKS` / `OSIRIS_I2P_HTTP_PROXY` ile belirlenen operatör proxy'sini kullanıyor; kullanıcı config'i başka bir host seçemiyor.
  - Gerçek bir tor konteyneri üzerinden TorCollector DuckDuckGo'nun .onion adresini çekebildi.
- **Redis Streams:**
  - Kuyruk `osiris:stream:raw_items`, consumer group `osiris-pipeline`. Akış: önce eski LIST boşaltılıyor, sonra `XAUTOCLAIM`, sonra `XREADGROUP`.
  - Başarılı kayıtlar `XACK` + `XDEL` ile siliniyor. Bozuk kayıtlar ve 5 teslimi aşanlar ölü-mektup akışına gidiyor.
  - Teslim garantisi artık **en az bir kez** (önceden en fazla bir kezdi). Gerçek Redis'te 7 entegrasyon senaryosu geçti.
- **K5 embedding:**
  - fastembed + `paraphrase-multilingual-MiniLM-L12-v2` (384 boyut, Apache-2.0).
  - Migration **005** `embedding_v2` ve `embedding_model` sütunlarını ekliyor. `--reembed` eski öğeleri dolduruyor.
  - Gerçek PG testinde Türkçe sorgu İngilizce ransomware öğesini ilk sırada buldu.
  - Rust karşılığı fastembed-rs `ParaphraseMLMiniLML12V2Q`; aynı dosyayı kullandığı crate kaynağında doğrulandı.
- **K4 NER:**
  - spaCy kaldırıldı (zaten hiç çalışmıyordu, E3). Yerine onnxruntime + tokenizers ile ONNX NER geldi.
  - Birleştirme kuralları sözleşmede (§11) ve modelsiz testlerle sabitlendi. 9 NER fixture'ı eklendi.
- **Modeller:** `models/download.py` sabit HF revizyonlarından indirip sha256 / git-blob ile doğruluyor. Modeller git'e girmiyor.
- **Pipeline işçisi (E1):**
  - `Dockerfile.pipeline` ve compose'da `osiris-pipeline` (profil `worker`) eklendi.
  - Duman testi: konteyner akıştan okudu, vektör + NER + regex varlıklarıyla DB'ye yazdı.
- **CI:** model önbelleği, model golden testleri ve `OSIRIS_REQUIRE_MODELS` korumasıyla (model yoksa test sessizce atlanmıyor).
- **Testler:** 348 birim + 15 entegrasyon (PG, Redis, model) + 144 sözleşme. Modelsiz koşulda kapsam %81,8.

**Sapmalar**
- Embedding modeli olarak plandaki örnek multilingual-e5-small yerine **MiniLM-L12-v2** seçildi (ikisi de 384 boyutlu). Gerekçe: e5-small Python fastembed'de hazır desteklenmiyor, MiniLM hem Python'da hem Rust'ta hazır destekleniyor ve e5'in `query:` / `passage:` önek tuzağını taşımıyor.
- Embedding varsayılan olarak **isteğe bağlı** kalmaya devam ediyor (`OSIRIS_EMBEDDING_MODEL`); `worker` servisi bunu varsayılan olarak açıyor.
- Stream anahtarı yeni (`osiris:stream:raw_items`), çünkü aynı anahtar LIST'ten Stream'e çevrilemiyor (WRONGTYPE). Eski LIST uyumluluk okuyucusuyla boşaltılıyor.
- fastembed `>=0.8.1,<0.10` aralığına sabitlendi. 0.9.0 iki haftadan yeni olduğu için testler 0.8.1 ile yapıldı.
- Duman testi bir hata yakaladı: İndirilen modeller 0600 izinliydi ve konteyner kullanıcısı okuyamıyordu. Düzeltildi.

**⚠ Canlı kurulum için dağıtım adımları (R3)**
1. `db/migrations/005_embedding.sql` dosyasını uygula (yalnızca ekleme yapıyor, tekrar çalıştırılabilir).
2. API ve işçiyi **birlikte** güncelle. Yeni collector Stream'e yazıyor; eski LIST'te bekleyen mesajları işçinin uyumluluk okuyucusu boşaltıyor.
3. Modelleri indir: `python models/download.py` (~420 MB).
4. İşçiyi başlatmadan **önce** `OSIRIS_RETENTION_DAYS` değerini gözden geçir. İşçi ilk turda saklama temizliğini çalıştırır.
   Ardından: `docker compose --profile worker up -d --build`. Eski öğeleri vektörlemek istersen: `docker compose exec osiris-pipeline osiris-pipeline --reembed`.
5. MinIO ve FreshRSS konteynerlerini kaldır: `docker compose up -d --remove-orphans`. Birimler (`miniodata`, `freshrssdata`) silinmez. FreshRSS'te akış listesi varsa önce OPML olarak dışa aktar.
6. Tor/I2P isteniyorsa: `docker compose --profile darknet up -d --build`.

## R4 (2026-10-17)

**Yapılanlar**
- **Cargo workspace** (repo kökü, `crates/*`), `rust-toolchain.toml` → Rust **1.99.1**, `rustfmt.toml`,
  `deny.toml`. Sürümler crates.io / PyPI'dan kontrol edildi: pyo3 0.30.1, maturin 1.15.0,
  regex 1.13.1, fancy-regex 0.19.2, sha2 0.11.1, ryu 1.0.23, ryu-js 1.0.3, serde_json 1.0.154,
  unicode-normalization 0.1.26.
- **`osiris-hash`:** `content_hash` normalizasyonu. bs4 4.15 + CPython 3.11.16 `html.parser`
  `get_text(" ", strip=True)` davranışının birebir portu (iki geçişli `feed`/`close`,
  geri izlemeli regex uç noktaları, dize kapları, `already_closed_empty_element`). Entity /
  Windows-1252 / boş öğe tabloları bs4'ten üretiliyor (`tools/gen_html_tables.py`, CI `--check`).
  Ayrıca RFC 8785 (JCS), Python `json.dumps` v1 biçimi (`float.__repr__` dahil), audit zinciri ve
  doğrulama.
- **`osiris-extract`:** regex varlık çıkarma. Python `re` farkları kapatıldı (`\b` → Python kelime
  sınıfı, `IGNORECASE` ile `[a-z]`'ye İ/ı/ſ/K eklendi, Python `\s`, Python `ipaddress` IPv4 kuralları).
- **`osiris-security`:** SSRF denetimi (CPython `urlsplit` portu, Python `ipaddress` ayrıştırma ve
  `str()` biçimi, engelli bloklar, fail-closed DNS, enjekte edilebilir `Resolver`) ve sanitizer'lar.
- **`osiris-py` (PyO3 + maturin, modül `osiris_native`):** Python tarafında `osiris/_native.py`
  arka ucu seçiyor (`OSIRIS_NATIVE=rust|python`, varsayılan `rust`). Rust seçilip modül
  yüklenemezse süreç açık bir hatayla duruyor, Python'a sessizce düşmüyor.
  Rust'ı çağıran yerler: `security.assert_safe_url/resolve_safe_ips/sanitize_*/mask_secret`,
  `audit.canonical_payload/chain_hash/verify_records`, `ProcessingPipeline.clean`,
  `extract_entities` (regex kısmı) ve `content_hash`. Python uygulamaları `_py_*` adıyla duruyor.
- **Fixture'lar:** Rust, `fixtures/hash`, `fixtures/extract` ve `fixtures/audit` dosyalarının
  hepsini geçiyor. Yeni `fixtures/security/cases.json` (83 durum, hata iletileri dahil)
  eklendi; Python ve Rust ikisi de geçiyor.
- **Eşdeğerlik testleri (`tests/golden/test_native_parity.py`):** HTML, regex, audit, URL, host
  ve sanitizer alanlarında Python ↔ Rust karşılaştırması yapılıyor; CI'da tohumlu 3000'er girdiyle
  koşuyor. Yerelde 200 000'er token düzeyinde ve 600 000 karakter düzeyinde rastgele girdi
  denendi, **hiç fark çıkmadı**. Kötü niyetli 200 000 karakterlik girdilerde Rust doğrusal
  çalışıyor (ör. `<title>&&&…`: Rust 7 ms, Python 121 ms).
- **K1:** `osiris-core/` (C++) ve CMake silindi, CI'daki `cpp-build` işi kaldırıldı. Dokümanlar iki
  dilde güncellendi (README, ARCHITECTURE, TECHNICAL_DOCUMENT/TEKNIK_DOKUMAN, CONTRIBUTING).
  Eskimiş `osiris:raw_items` kuyruk adı da düzeltildi.
- **CI:** yeni `rust` işi (`cargo fmt --check`, `clippy --all-targets -D warnings`,
  `cargo test --locked`) ve `cargo-deny` işi eklendi. Python işleri `osiris-py`'yi derleyip
  testleri **iki yolda** koşuyor (varsayılan Rust + kapsam, sonra `OSIRIS_NATIVE=python`).
  Entegrasyon ve sözleşme işi de Rust yolunu kullanıyor.
- **Docker:** `Dockerfile.api` ve `Dockerfile.pipeline` artık çok aşamalı:
  `rust:1.99.1-slim-trixie` aşamasında `maturin build --locked` ile abi3 tekerleği derleniyor.
  `.dockerignore`'a `target/` ve `models/` eklendi (modeller 414 MB'ı build bağlamına
  taşıyordu).
- **Testler:** Rust 17 test (fixture'lar dahil). Python birim testleri Rust yolunda 447, Python
  yolunda 446 + 1 atlama. Entegrasyon 15/15 (iki yolda). Model golden 20/20. Sözleşme 144/144
  (API Rust yolunda). Modelsiz kapsam %81. `cargo deny`: advisories, bans, licenses ve sources
  temiz.

**Yeni güvenlik bulgusu — E14 (düzeltildi, iki yolda)**
- `assert_safe_url`, `http://127.0.0.1\x.saldirgan.example/` adresini kabul ediyordu. `urlparse`
  host'u `127.0.0.1\x.saldirgan.example` olarak görüyordu ve saldırganın joker DNS kaydı genel bir
  IP dönerse denetim geçiyordu. requests/urllib3 ise ters eğik çizgiyi yetki bölümünün sonu
  sayıp bağlantıyı **127.0.0.1**'e açıyordu (doğrulandı: urllib3 2.8.0, requests 2.34.2).
- Düzeltme: kırpılmış URL'de ters eğik çizgi, C0 kontrol karakteri veya DEL varsa red
  (`URL geçersiz karakter içeriyor`). Regresyon testleri `test_security.py` dosyasında ve
  `fixtures/security` içinde.

**Plandan sapmalar**
- **Python uygulamaları henüz silinmedi.** Plan "testler iki yolu da koşar, sonra Python
  uygulaması silinir" diyor. `_py_*` fonksiyonları şimdilik iki iş görüyor: eşdeğerlik
  testlerinin kâhini, ve canlı sistemde Rust tekerleği derlenemezse `OSIRIS_NATIVE=python` ile
  kullanılabilecek bir kaçış yolu. Silinmeleri R7'ye bırakıldı; istenirse şimdi silinebilir.
- `security.py` dosyasının hepsi taşınmadı. HTTP, soket ve bs4'e bağlı olan `fetch_url`,
  `safe_create_connection`, `resolve_proxy`, `safe_title` ve `cap_list` Python'da kaldı (R5'te
  host HTTP istemcisi bunların yerini alacak). Bu fonksiyonlar denetim için Rust'ı çağırıyor.
- Plandaki kütüphane tablosunda `serde_json_canonicalizer` geçiyordu; onun yerine JCS `ryu-js` ile
  elle yazıldı. Gerekçe: Python'un sınırsız tamsayıları (`BigInt`), v1 biçimiyle ortak değer türü
  ve tam denetim. Fixture'lar ve bulanık testlerle doğrulandı.
- URL bölme için WHATWG (`url` crate) bilerek kullanılmadı, CPython `urlsplit` port edildi.
  Denetimin, isteği gönderen istemciyle aynı host'u görmesi gerekiyor (E14 bunun nedenini
  gösteriyor).
- `osiris-py` crate'inin `cargo test`'i yok (`--exclude`); köprü pytest ile sınanıyor.

**⚠ Canlı kurulum için dağıtım adımları (R4)**
1. Migration yok. `content_hash` değişmedi (fixture'lar ve bulanık testlerle doğrulandı), yeniden
   hash'lemeye gerek yok.
2. İmajları yeniden derle: `docker compose --profile api --profile worker build`. İlk derlemede
   Rust çekirdeği derlendiği için 2–4 dakika fazladan sürer.
3. Docker dışında (venv'de) çalışan bir servis varsa önce Rust araç zincirini kur, sonra
   `pip install ./crates/osiris-py`. Kurulmazsa servisler açık bir hatayla başlamaz. Geçici
   çözüm: `OSIRIS_NATIVE=python`.
4. **Davranış değişikliği (E14):** URL'sinde `\` ya da kontrol karakteri bulunan kaynaklar artık
   reddediliyor. Var mı diye bakmak için (test DB'sinde denendi):
   `SELECT id, name FROM sources WHERE url ~ '[\\[:cntrl:]]' OR metadata::text LIKE '%\\%';`
   (`metadata` JSON metninde `\n`/`\t` kaçışları da eşleşir; satırları gözle kontrol et).

**Sıradaki:** R5 — veri katmanı Rust'ta (`osiris-types`, `osiris-db`, `osiris-plugin`, 11 plugin,
collector, pipeline). Açık karar maddesi: rest-api ve shodan plugin'lerinin `raw_content`'i
(Python `str(dict)` taklidi mi, JSON'a geçiş mi; JSON'a geçiş hash'i değiştirir).

## R5 (2026-10-17)

**Kararlar (kullanıcı, 2026-10-17)**
- rest-api / shodan `raw_content`: **JSON'a geçildi** (kanonik JSON; bu iki plugin'in yeni
  öğelerinin `content_hash`'i değişti).
- Dil tespiti: **lingua**, sınırlı dil kümesi (16 dil).
- Zamanlama (E2): **varsayılan kapalı**, `OSIRIS_SCHEDULER=on` ile açılır.
- Geçiş: **Rust varsayılan, Python yedek profilde** (`worker-python`); önce gölge mod.

**Yapılanlar**
- **Crate'ler:** `osiris-types` (schemas/ ile birebir, schemars karşılaştırması),
  `osiris-plugin` (Collector trait, SSRF zorunlu `fetch`, DNS, `safe_connect`, fixture
  oynatıcısı), `osiris-feed` (feedparser kuralları), `osiris-plugins` (11 plugin),
  `osiris-db` (sqlx, Python ile aynı SQL), `osiris-pipeline` (Stream işçisi, ONNX NER, fastembed,
  lingua, gölge karşılaştırma), `osiris-server` (ikili: `pipeline`, `shadow`, `collector`,
  `collect`, `purge`).
- **Fixture'lar:** 23 plugin senaryosu, 9 NER, 6 embedding (kosinüs ≥ 0.999), hash/extract/
  audit/security Rust'ta geçiyor.
- **Eşdeğerlik testleri (yeni):** bs4 ağacı (60k), soupsieve seçicileri (20k belge × 31
  seçici), `html.unescape` (60k), `urljoin` (20k), Python `json.loads` + kanonik kayıt (200k),
  RSS/Atom ↔ feedparser (10k; `content_hash` düzeyinde). Fark yok.
- **İşçi eşdeğerliği (`tests/integration/test_rust_pipeline_parity.py`):** aynı 31 mesaj
  Python ve Rust işçisine ayrı veritabanlarında işletildi: 27 öğe (metin, hash, URL, başlık,
  tarih, metadata, kaynak, etiket, NER dahil varlıklar, embedding) birebir; ölü-mektup 4 = 4;
  `osiris-server shadow` Python'un yazdığı veritabanında temiz. Dil uyumu 14/27 (beklenen).
- **Canlı denemeler:** rss (3 gerçek feed), web-scraper (example.com, python.org + seçici),
  dns-whois (example.com, 9 kayıt), username-search (GitHub/GitLab/Reddit) Python ile aynı;
  Tor plugin'i gerçek Tor üzerinden DuckDuckGo .onion'u çekti; 4 SSRF vektörü (metadata IP'si,
  iç ağa yönlendirme, 127.0.0.1'e çözülen ad, ters eğik çizgi) reddedildi; zamanlayıcı
  kapalı/açık davranışı ve kaynak sağlığı + `source_id` bağlantısı doğrulandı.
- **Dağıtım:** `Dockerfile.server` (çok aşamalı, debian trixie-slim, `nobody`); compose
  `worker` profili artık Rust işçisi + collector; Python işçisi `worker-python` profilinde.
- **CI:** entegrasyon işi `osiris-server`'ı derler, Rust model fixture'larını zorunlu koşar ve
  işçi eşdeğerlik testini çalıştırır; `gen_feed_tables --check`; cargo-deny allowlist'e
  ISC, BSD-3-Clause, MIT-0, BSL-1.0, CDLA-Permissive-2.0 (GPL-3.0 uyumlu) eklendi.
- **K6:** [WASM plugin tasarım notu](WASM_PLUGIN_TASARIMI.md) (yalnızca tasarım).

**Bulunan ve düzeltilen hatalar**
- **E15 (güvenlik/işlev):** i2p plugin'i `.i2p` adlarını SSRF denetiminde DNS'e soruyordu →
  canlıda hiç çalışamıyordu. `allow_i2p` eklendi (iki yolda).
- **gzip:** Rust HTTP istemcisinde sıkıştırma çözme kapalıydı; canlı karşılaştırma yakaladı.
- **Embedding bağlamı:** fastembed-rs 512 token kullanıyordu, Python 128; uzun metinde
  kosinüs 0,64'tü — Python kuralı uygulandı.
- **Cron haftanın günü:** Python APScheduler `day_of_week`'i 0 = Pazartesi sayardı (hiç
  çalışmadığı için etkisizdi); Rust standart cron (0/7 = Pazar) kullanır.

**Plandan sapmalar**
- **RSS eşdeğerliği `content_hash` düzeyinde:** feedparser'ın sgmllib tabanlı temizleyicisi
  bayt bayt taşınmadı; girdiler, link ve tarih birebir, temizlenmiş metin aynı, ham HTML ve
  HTML'li başlıkların kaçış biçimi farklı olabilir.
- **Gölge mod tasarımı:** plandaki "aynı Stream'den yazmadan oku" yerine Rust, Python'un
  **yazdığı öğeleri** DB'den okuyup yeniden hesaplar. Gerekçe: Python işçisi işlediği girdiyi
  `XDEL` ile siler; ikinci bir tüketici grubu girdileri kaçırırdı.
- **Python collector/pipeline kodu silinmedi:** Python işçisi `worker-python` profilinde geri
  dönüş yolu olarak duruyor; API'nin süreç içi `/sources/{id}/collect` toplaması R6'ya kadar
  Python'da (API R6'da taşınır). Silme R7'de.
- **ort yalnızca RC sürümünde:** `ort =2.0.0-rc.13` (fastembed 7.1.1'in sabitlediği sürüm;
  kararlı 2.x yok).
- `paste` bakım dışı uyarısı (tokenizers zinciri) cargo-deny'da gerekçeli istisna.
- Eşlenmemiş UTF-16 vekili: Python dizede taşır (kuyruğa yazarken hata), Rust U+FFFD yazar.

**⚠ Canlı kurulum için geçiş adımları (R5)**
1. Migration yok. Modeller kurulu olmalı: `python models/download.py`.
2. İmajları derle: `docker compose --profile worker --profile worker-python build`
   (Rust derlemesi 10–15 dakika sürebilir).
3. **Gölge aşaması** (önerilen, birkaç saat/gün): Python işçisi çalışırken Rust'ı yalnızca
   karşılaştırıcı olarak başlat:
   `docker compose --profile worker-python up -d` ve
   `OSIRIS_PIPELINE_SHADOW=1 docker compose --profile worker up -d osiris-pipeline`.
   Rapor: `docker compose logs -f osiris-pipeline` (`gölge: uyuşmazlık yok` beklenir) ya da tek
   seferlik `docker compose run --rm osiris-pipeline shadow --minutes 1440`.
4. **Geçiş:** `docker compose stop osiris-pipeline-python` → `.env`'de
   `OSIRIS_PIPELINE_SHADOW=0` → `docker compose --profile worker up -d`. Bekleyen mesajlar
   aynı tüketici grubunda kaldığı için kaybolmaz.
5. **Zamanlama** isteniyorsa önce kaynakların `schedule` değerlerini gözden geçir (artık
   gerçekten çalışacaklar), sonra `.env`'de `OSIRIS_SCHEDULER=on` ve
   `docker compose --profile worker up -d osiris-collector`.
6. **Geri dönüş:** `docker compose stop osiris-pipeline osiris-collector` →
   `docker compose --profile worker-python up -d`.
7. rest-api/shodan kaynakları varsa: geçişten sonraki ilk toplamada aynı kayıtlar yeni
   (JSON) `content_hash` ile bir kez daha saklanır; saklama süresiyle temizlenir.

**Sıradaki:** R6 (aşağıda).

## R6 (2026-10-17)

**Karar (E11, kullanıcı onayı):** API'ye açık olmayan fonksiyonlar (`semantic_search`, `centrality`,
`communities`, STIX/HTML/CSV/JSON raporları, `check_anomaly`, GraphML/GEXF) **yalnızca kütüphane**
olarak Rust'a taşındı; yeni uç eklenmedi. API, Python ile birebir aynı 26 uçtan oluşur.

**Yeni crate'ler**
- `osiris-queue` (kuyruk üreticisi: `XADD`), `osiris-collector` (R5'teki yönetici `osiris-server`'dan
  kütüphaneye taşındı; API, CLI ve collector servisi paylaşır).
- `osiris-graph`: networkx `Graph` semantiği (yönsüz, çift başına tek kenar, ekleme sırası), derece
  merkeziliği, **CNM topluluk tespiti birebir port** (networkx 3.6.1 `MappedQueue` yerine aynı tam
  sıralamalı küme), `/graph` gövdesi, GraphML (bayt bayt), GEXF. Fixture 15 durum: karate kulübü ve
  60/120/200 düğümlü tohumlu rastgele graflar dahil.
- `osiris-report`: Markdown/HTML (Jinja2 autoescape eşdeğeri), JSON, CSV (formül koruması, `\r\n`,
  QUOTE_MINIMAL; dizin kilidi + dışarı çıkan sembolik bağ reddi), STIX 2.1 (bayt bayt). Yeni fixture
  `fixtures/report`.
- `osiris-alert`: eşleşme, sessize alma, z-skor anomali, Redis `PUBLISH`; webhook, Telegram, e-posta
  (SMTP, `lettre`) işleyicileri (`OSIRIS_ALERT_*`; Python'daki gibi hiçbir servise bağlı değil).
  Yeni fixture `fixtures/alert`.
- `osiris-query`: tam metin, pgvector semantik, varlık araması (Python SQL'i).
- `osiris-cli` (ikili `osiris`): `status`, `plugins`, `collect`; çıkış kodları 0/1/2. Python CLI'ın
  betiği `osiris-py` oldu.
- `osiris-db` ekleri: denetim zinciri yazımı (`append_audit`: danışma kilidi, v1/v2, `_normalize`
  kuralları), graf kenarları, tembel havuz, yetim varlık temizliği, `sql_ts!`.

**API (`osiris-server api`, axum)**
- Uçlar, durum kodları, hata iletileri, sayfalama sınırları, kontrol sırası (bozuk JSON → 422
  kimlikten önce; alan doğrulaması kimlikten sonra), kimlik sırası (operatör anahtarı → Bearer JWT →
  DB anahtarı), RBAC, CORS izin listesi, `{"detail": …}` gövdeleri Python ile aynı. Pydantic "lax"
  dönüşümleri (`"20"` → 20, `"yes"` → true) korunur.
- Yeni: kimlik başına kota (`governor`, `OSIRIS_API_RATE_PER_MIN`, varsayılan 600/dk, 429), gövde
  tavanı 5 MiB (413), Prometheus `/metrics` ayrı dinleyicide (`OSIRIS_METRICS_ADDR`; etiketler rota
  şablonu), yapılandırılmış JSON log (`OSIRIS_LOG_FORMAT=json`), OpenAPI (`/openapi.json`, utoipa) +
  `/docs`, SIGTERM ile düzgün kapanış, `osiris-server health` (konteyner sağlık denetimi).
- Zaman damgaları FastAPI/pydantic biçiminde yazılır (`2026-10-17T16:39:00.779110Z`; mikrosaniye
  sıfırsa kesirsiz) — PostgreSQL `to_json` hem `+00:00` yazar hem sondaki sıfırları atar; `sql_ts!`
  bunu SQL'de düzeltir.
- Denetim kayıtları Rust'ta yazılır; Python `verify_chain` zinciri doğruladı (21/21, v2).

**Doğrulama**
- `tests/contract/` **değiştirilmeden** Rust API'ye karşı 144/144 (yıkıcı testler dahil); Python
  API'ye karşı da yeşil kalıyor.
- Yeni `tests/parity/` (R6–R7 arası): aynı 71 köşe durum isteği + veri dolu 13 okuma ucunun gövdesi
  iki API'de birebir aynı; JWT iki yönde geçerli (Python'un jetonu Rust'ta, Rust'ınki Python'da).
- Rust: fixture testleri (graph, report, stix, alert), API birim testleri (DB'siz: kimlik matrisi,
  doğrulama sırası, 404/405, 413, 429, CORS, başlatma denetimi), CLI çıkış kodları.
- **n8n:** iş akışı n8n 1.114.4'te içe aktarılıp çalıştırıldı → Rust API: `/collect` 200 (gerçek
  RSS, 20 öğe kuyruğa yazıldı), `/search` 200.
- Web UI: kullandığı tüm uçlar sözleşme + eşdeğerlik testlerinde; gövde biçimleri aynı (kod
  değişikliği gerekmedi).

**Bulgular**
- **E16 (işlev):** `deploy/n8n/osiris-workflow.json` hiçbir API'de çalışmıyordu: `config` JSON dizesi
  olarak gidiyordu (422), arama gövdesindeki `{{ $json.query }}` başında `=` olmadığı için düz metin
  gidiyordu, ifadedeki `}}` ifadeyi erken kapatıyordu ve zamanlayıcının çıktısında `query`/`feed_url`
  yoktu. İş akışı `Zamanlayıcı → Ayarlar → Topla → Ara` olarak yeniden yazıldı (JSON gövde, 15 dk);
  compose'a `N8N_BLOCK_ENV_ACCESS_IN_NODE=false` eklendi.
- **R5 gizli hatası:** collector servisi Redis'e `MultiplexedConnection` ile bağlanıyordu; Redis
  yeniden başlarsa kuyruğa yazma kalıcı olarak başarısız olurdu. Artık tembel, kendini yenileyen
  `ConnectionManager` (API ve CLI de aynı).
- Manifest `config_schema` sırası Rust'ta alfabetikti (`BTreeMap`): zorunlu alan hata iletisinde ve
  `/plugins` gövdesinde alan sırası Python'dan farklıydı (arayüz form sırası). `IndexMap` ile manifest
  sırası korunuyor.

**Plandan / Python'dan sapmalar**
- `/graph/relation` yalnızca eklenen kenarı kalıcılaştırır. Python her çağrıda bellekteki **tüm**
  kenarları upsert edip hepsinin `evidence_count`'unu artırıyordu.
- Graf DB'den yüklenemezse sonraki istekte yeniden denenir (Python başarısızlığı da "yüklendi" sayıp
  süreç yeniden başlayana kadar boş graf gösteriyordu).
- `/graph/relation`: kırpınca boş kalan kimlik 400 (Python: düz metin 500); düğüm sınırı 503.
- 422 listelerinde kayıt biçimi pydantic'e benzer ama ileti metinleri birebir değil (sözleşme yalnızca
  `detail` varlığını ve iç ayrıntı sızmamasını ister).
- Bazı gövdelerde (`/graph`, `/collect` metadata, `/alerts/test`) nesne anahtarları alfabetik sıralı;
  liste uçları sütun sırasını korur. İstemciler anahtar sırasına bağlı değil.
- `/docs` OpenAPI'yi göreli yoldan yükler (Nginx `/api/docs` altında da çalışır; FastAPI'de kırıktı).
  Swagger UI dosyaları FastAPI'deki gibi jsDelivr'dan gelir (Nginx CSP'si ikisinde de engeller).
- Python `str.isprintable` (iç içe değerlerin `repr`'inde) yaklaşık taşındı; yalnızca rapor/CSV
  hücrelerinde iç içe liste/sözlük olduğunda ve nadir Unicode biçim karakterlerinde fark edebilir.
- `osiris plugins --plugins-dir` yok sayılır (plugin'ler ikiliye gömülü).
- Python API ve CLI silinmedi (R7). Python API `api-python` profilinde geri dönüş yolu.

**⚠ Canlı kurulum için geçiş adımları (R6)**
1. Migration yok. İsteğe bağlı `.env`: `OSIRIS_API_RATE_PER_MIN` (varsayılan 600, 0 = kapalı).
2. İmaj: `docker compose --profile api build osiris-api` (Rust derlemesi 10–15 dk; `osiris-server`
   imajını R5 işçisiyle paylaşır).
3. Geçiş: `docker compose --profile api up -d osiris-api` (aynı konteyner adı, Rust imajıyla yeniden
   oluşturulur) → `docker compose restart nginx` (Nginx upstream IP'sini başlangıçta çözer).
4. Doğrula: `curl -sk https://localhost:8443/api/health`; isteğe bağlı sözleşme testleri
   (yıkıcı OLMAYAN): `OSIRIS_BASE_URL=http://127.0.0.1:8001 OSIRIS_CONTRACT_API_KEY=… pytest -q
   tests/contract` (Nginx üzerinden değil — IP hız sınırına takılır). Loglar JSON: `docker compose logs -f osiris-api`.
5. n8n: `docker compose up -d n8n` (yeni ortam değişkeni), iş akışını `deploy/n8n/` dosyasından yeniden
   içe aktarın ve **Ayarlar** düğümünde `feed_url`/`query` değerlerini girin.
6. **Geri dönüş:** `docker compose stop osiris-api && docker compose --profile api-python up -d --build
   osiris-api-python && docker compose restart nginx` (Python API `osiris-api` adını devralır; ikisini
   aynı anda çalıştırmayın).

**Sıradaki:** R7 (aşağıda).

## R7 (2026-10-17)

**Kararlar (kullanıcı onayı)**
- Eski `items.embedding VECTOR(1536)` sütunu **silindi** (migration 006).
- Denetim zinciri v1 desteği **tamamen kaldırıldı**: eski v1 kayıtlar tabloda kalır, artık
  doğrulanmaz ("Bilinmeyen chain_version: 1").
- Python geri dönüş yolları (`worker-python`, `api-python`, gölge karşılaştırma) **silindi**
  (kullanıcı: canlı R5/R6 geçişleri yapıldı).

**Yapılanlar**
- **Python uygulama kodu silindi** (~11 200 satır): `osiris-*` paketleri, `osiris-sdk`, plugin
  `collector.py`'leri ve `requirements.txt`'leri, `crates/osiris-py` (PyO3 köprüsü), Python
  golden/entegrasyon/eşdeğerlik testleri, `Dockerfile.api` / `Dockerfile.pipeline`, kök
  `pyproject.toml`, `models/download.py`. Repoda kalan Python: `tests/contract/` (planın
  öngördüğü kara kutu paketi) ve geliştirme araçları (`tools/gen_*.py` — bs4/feedparser'dan
  Rust tablosu üretir; `plugins/username-search/tools/build_full_db.py` — site DB'si). Bunlar
  uygulama kodu değildir; sürümleri `tools/requirements.txt`'de sabit.
- **Rust'a taşınanlar:** model indirici (`osiris-server models`, sabit revizyon + sha256/git-sha1),
  denetim zinciri doğrulayıcı (`osiris_db::verify_chain`, `osiris-server audit-verify`),
  yeniden vektörleştirme (`osiris_pipeline::reembed`), Python entegrasyon testleri
  (`crates/osiris-server/tests/integration.rs`, 13 test: denetim, saklama, CHECK, depolama, 7 kuyruk
  senaryosu, embedding + semantik arama).
- **Migration 006** + `osiris-server migrate` (migration'lar ikiliye gömülü, tekrar
  çalıştırılabilir; boş, ikinci kez ve eski şemalı DB'de sınandı). 001'den eski sütun/indeks
  çıkarıldı (aksi hâlde tekrar çalıştırmada silinen sütuna indeks kurmaya çalışıyordu).
- **Sadeleştirme:** migration 003/004/005 öncesi uyumluluk dalları (v1 yazma, `retention_days`
  yokken NULL/503, `embedding_v2` sütun denetimi) ve gölge mod kaldırıldı; semantik arama yalnızca
  384 boyut.
- **Tek imaj:** `deploy/docker/Dockerfile` → `osiris:1.0.0-rc`, `gcr.io/distroless/cc-debian13:nonroot`
  (kabuk yok, root değil; ONNX Runtime statik bağlı — glibc + libstdc++ yeterli). 332 MB → 252 MB.
  Compose: `osiris-api`, `osiris-pipeline`, `osiris-collector` aynı imaj + ortak ayar çapası;
  `osiris-models` (tek seferlik model indirme, host kullanıcısıyla); `worker`/`full` profilleri.
- **CI:** Python test işi kalktı; entegrasyon işi Rust (migrate iki yol × 2, entegrasyon + model
  testleri, Rust API'ye karşı sözleşme, `audit-verify`); yeni imaj derleme + duman testi işi; araç
  tabloları ve ruff ayrı işte.
- **Dokümanlar iki dilde yeniden yazıldı:** README, TEKNIK_DOKUMAN / TECHNICAL_DOCUMENT (kurulum,
  yapılandırma, işletim, yükseltme, yedekleme, sorun giderme), ARCHITECTURE, CONTRIBUTING (Rust
  geliştirme ortamı, test katmanları, plugin yazma rehberi), API, SOZLESME (doğrulama tablosu,
  v1 kaldırımı), models/README.
- **Sürüm 1.0.0-rc** (Cargo çalışma alanı → OpenAPI ve `/metrics`, web UI, imaj etiketleri).

**Sözleşme değişiklikleri**
- `fixtures/audit`: iki v1 durumu çıkarıldı (v2 durumu aynen).
- Semantik arama 384 dışı boyutta `Invalid` döner (API'de uç yok).

**Taze kurulum doğrulaması (kabul ölçütü)** — temiz klon, boş Docker birimleri, README "Quick
Start" adımları birebir (`.env` → `make-certs.sh` → `--profile models run osiris-models` →
`--profile full up -d --build`):
- Modeller sıfırdan indi ve özetleri doğrulandı; 8 servis ayakta, `osiris-api` *healthy*;
  `osiris-server 1.0.0-rc`, OpenAPI sürümü 1.0.0-rc (24 yol), `/ui/` 200.
- Sözleşme testleri doğrudan API'ye (`127.0.0.1:8001`) karşı **144/144** (yıkıcılar dahil);
  Nginx üzerinden tek tek istekler 200. *Not:* paketi Nginx'ten koşmak IP başına hız sınırına
  (10 istek/sn) takılır — bu bilinçli korumadır, paket doğrudan porta koşulmalıdır.
- `migrate` çalışan DB'de tekrar temiz; `audit-verify` → `ok` (25 kayıt).
- Gerçek RSS (hnrss, 15 öğe) → kuyruk → işçi: 15 öğe, 15 vektör, NER + regex varlıkları; tam metin
  arama ve `/entities/top` sonuç döndü.

**⚠ Canlı kurulum için yükseltme adımları (R7)**
1. **Yedek alın** (migration 006 veri siler): `docker compose exec -T postgresql pg_dump -U osiris
   -Fc osiris > osiris-oncesi-r7.dump`.
2. İsteğe bağlı: eski sütundaki vektörlerin yerine yenisi üretilsin diye önce
   `docker compose run --rm osiris-pipeline pipeline --reembed` (R6 imajıyla).
3. `git pull && docker compose --profile full build` (yeni imaj adı `osiris:1.0.0-rc`).
4. `docker compose run --rm osiris-api migrate`.
5. Eski Python konteynerleri kaldıysa: `docker compose --profile full up -d --remove-orphans`;
   ardından `docker compose restart nginx`.
6. Doğrula: `curl -sk https://localhost:8443/api/health`,
   `docker compose run --rm osiris-api audit-verify` (migration 003 öncesi v1 kayıtlar varsa
   `ok: false, Bilinmeyen chain_version: 1` beklenir — kullanıcı kararı).
7. Modeller daha önce `python models/download.py` ile indirildiyse aynı dosyalar geçerlidir;
   değilse `docker compose --profile models run --rm osiris-models`.

## Geçiş sonrası: web arayüzü (2026-10-18)

**Karar (E11 güncellendi, kullanıcı isteği):** web arayüzünde tüm özelliklerin kullanılabilmesi için
kütüphanede hazır olan işlevler uç olarak açıldı: `POST /search/semantic`, `GET /graph/analysis`
(merkezilik, CNM toplulukları ≤ 5000 düğüm, komşular), `GET /graph/export` (GraphML/GEXF),
`POST /reports/generate` (Markdown/HTML/JSON/STIX). Ayrıca `GET`/`PATCH /sources/{id}` (sırlar
`********`, config sığ birleştirme) ve `/collect` + `/sources/{id}/collect` için `include_items`
önizlemesi. OpenAPI 24 → 28 yol; CORS'a `PATCH` eklendi. Mevcut 26 ucun sözleşmesi değişmedi.

- **Semantik arama:** sorgu vektörünü API üretir (`osiris_pipeline::embed`, ilk aramada yüklenir).
  compose'da `osiris-api` artık `./models`'i salt okunur bağlar ve `OSIRIS_EMBEDDING_MODEL` alır;
  bellek tavanı 512m → 1g. Model yoksa uç `503`, API'nin geri kalanı etkilenmez.
- **Dağıtım:** Nginx CSP'si arayüz için `img-src data: blob:` ve `style-src 'unsafe-inline'`
  (React `style` öznitelikleri) aldı; `script-src 'self'` kaldı. `/` → `/ui/` yönlendirmesi.
  UI imajı kendi nginx yapılandırmasıyla (özetli varlıklar 1 yıl önbellek, `index.html` no-cache,
  proxy arkasında gzip) ve sağlık denetimiyle; nginx sürümü sabit (1.31-alpine).
- **Testler:** Rust birim testlerine 3 yeni test (doğrulama sırası, RBAC, graf, rapor); sözleşme
  paketi 144 → 182, gerçek PostgreSQL + Redis'e karşı Rust API'de 182/182. Arayüz gerçek API'ye
  karşı tarayıcıda uçtan uca, hem doğrudan hem dış nginx → UI nginx zinciri üzerinden denendi.
- **Not:** `/api/docs` (Swagger UI) betiklerini jsDelivr'dan yükler; Nginx CSP'si
  (`default-src 'self'`) bunu Nginx üzerinden engeller. Doğrudan API portunda (`127.0.0.1:8001/docs`)
  çalışır; `/api/openapi.json` her iki yoldan erişilebilir.

**Rust geçişi tamamlandı.** Sonraki işler (geçiş dışı): uyarıların işçide yayımlanması, 2FA,
sır kasası, PDF rapor, WASM plugin'leri, :443 geçişi.
