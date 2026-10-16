# OSIRIS — Rust Geçişi Durumu

Plan: [RUST_GECIS_PLANI.md](RUST_GECIS_PLANI.md) · Envanter: [RUST_GECIS_ENVANTER.md](RUST_GECIS_ENVANTER.md)

| Faz | Durum | Dal |
|---|---|---|
| R0 — Envanter | ✅ Tamamlandı, kararlar onaylandı | `master` |
| R1 — Doküman + güvenlik düzeltmeleri | ✅ Tamamlandı | `master` |
| R2 — Geçiş sözleşmesi | ⏳ Başlamadı | — |
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
