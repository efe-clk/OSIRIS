# OSIRIS — Rust Geçişi Durumu

Plan: [RUST_GECIS_PLANI.md](RUST_GECIS_PLANI.md) · Envanter: [RUST_GECIS_ENVANTER.md](RUST_GECIS_ENVANTER.md) · Sözleşme: [SOZLESME.md](SOZLESME.md)

| Faz | Durum | Dal |
|---|---|---|
| R0 — Envanter | ✅ Tamamlandı, kararlar onaylandı | `master` |
| R1 — Doküman + güvenlik düzeltmeleri | ✅ Tamamlandı | `master` |
| R2 — Geçiş sözleşmesi | ✅ Tamamlandı | `master` |
| R3–R7 | ⏳ Başlamadı | — |

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
4. Sözleşme testleri canlıya karşı güvenle çalıştırılabilir (`OSIRIS_CONTRACT_DESTRUCTIVE` verme): `OSIRIS_BASE_URL=https://<sunucu>:8443/api OSIRIS_CONTRACT_API_KEY=… OSIRIS_CONTRACT_VERIFY_TLS=0 pytest tests/contract`. Testler `contract-*` adlı kullanıcılar bırakır, anahtarları iptal edilmiş olur.

**Kalanlar**
- E1 (açık): canlıda `osiris:raw_items` kuyruğunu ne boşaltıyor?
