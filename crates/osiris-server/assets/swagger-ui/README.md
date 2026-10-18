# Swagger UI varlıkları

`GET /docs` sayfasının dosyaları; derleme zamanında `osiris-server` ikilisine gömülür
(`src/api/openapi.rs`). Dış CDN kullanılmaz: sayfa Nginx'in CSP'siyle (`script-src 'self'`)
çalışır ve tarayıcı üçüncü tarafa istek göndermez.

| Dosya | Kaynak |
|-------|--------|
| `swagger-ui-bundle.js`, `swagger-ui.css`, `LICENSE`, `NOTICE` | npm `swagger-ui-dist@5.33.0` (Apache-2.0), değiştirilmeden; paket bütünlüğü npm `dist.integrity` (sha512) ile doğrulandı |
| `init.js` | OSIRIS başlatıcısı (satır içi betik yerine) |

sha256:

```
62df541529080464a7660adc793eab7128c6193ce3be24ddc1e0e0a4a63edc2f  swagger-ui-bundle.js
1ac324f7dcd27e4b9386b4bd6421271ec147e922a22c05ba24b11515e9aa6321  swagger-ui.css
```

Güncelleme: yeni sürümün paketini (`https://registry.npmjs.org/swagger-ui-dist/<sürüm>`) indirip
`dist.integrity` ile doğrulayın, dört dosyayı değiştirin, bu tablodaki sürümü ve özetleri
güncelleyin.
