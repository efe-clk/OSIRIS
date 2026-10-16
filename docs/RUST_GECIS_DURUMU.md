# OSIRIS — Rust Geçişi Durumu

Plan: [RUST_GECIS_PLANI.md](RUST_GECIS_PLANI.md) · Envanter: [RUST_GECIS_ENVANTER.md](RUST_GECIS_ENVANTER.md)

| Faz | Durum | Dal |
|---|---|---|
| R0 — Envanter | ✅ Tamamlandı, karar bekliyor (K1–K9) | `rust/r0-envanter` |
| R1 — Doküman + güvenlik düzeltmeleri | ⏳ Başlamadı | — |
| R2–R7 | ⏳ Başlamadı | — |

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
- K1–K9 kararlarının kullanıcı tarafından onaylanması.
- E1'deki sorunun cevaplanması: canlı kurulumda `osiris:raw_items` kuyruğunu ne boşaltıyor?
- Onaydan sonra R0 PR'ı açılacak, ardından R1 başlayacak.
