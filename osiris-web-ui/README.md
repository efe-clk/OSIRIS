# OSIRIS Web UI

OSIRIS REST API'sinin (`osiris-server api`, Rust) tarayıcı arayüzü. React 18 + TypeScript + Tailwind, ek grafik
kütüphanesi yok (graf canvas üzerinde bağımlılıksız kuvvet yerleşimiyle çizilir).
Uç başvurusu: [docs/API.tr.md](../docs/API.tr.md).

## Ekranlar

| Ekran | Ne yapar | Kullandığı uçlar | En düşük rol |
|---|---|---|---|
| Panel | Sayaçlar, kaynak sağlığı, son öğeler, öne çıkan varlıklar, hızlı arama | `/stats`, `/sources`, `/items/recent`, `/entities/top` | viewer |
| Kullanıcı adı tarama | Maigret tarzı: hızlı (59 site) ya da tam (2500+ site), kategori süzgeci, tempo ayarları; CSV/JSON indir, grafa bağla, rapora ekle | `POST /collect` (`username-search`, `include_items`), `/graph/relation` | analyst |
| Toplama | Plugin seç → manifest şemasından üretilen form → çalıştır ve önizle; toplu iş (≤10); kaynak olarak kaydet | `/plugins`, `/collect`, `/collect/batch`, `POST /sources` | analyst |
| Arama | Tam metin (vurgulu), semantik (benzerlik yüzdesi), varlık; CSV indir, uyarı olarak kaydet | `/search`, `/search/semantic`, `/search/entity`, `/saved-queries` | viewer |
| Öğeler | Sayfalı liste, süzgeç; detay çekmecesi (içerik, varlıklar, üst veri, KVKK silme) | `/items/recent`, `/items/{id}`, `DELETE /items/{id}` | viewer |
| Varlıklar | Türe göre en çok anılanlar; öğelere/grafa git, rapora gösterge ekle, KVKK silme | `/entities/top`, `/entities/erase` | viewer |
| İlişki grafı | Kuvvet yerleşimi, topluluk renkleri, merkezilik, komşular, ilişki ekleme, GraphML/GEXF | `/graph`, `/graph/analysis`, `/graph/export`, `/graph/relation` | viewer |
| Kaynaklar | Oluştur, düzenle (sırlar maskeli), aç/kapat, şimdi çalıştır, sil; sağlık ve cron hazır ayarları | `/sources`, `GET/PATCH/DELETE /sources/{id}`, `/sources/{id}/collect` | viewer (okuma) / analyst |
| Uyarı kuralları | Kayıtlı sorgular, uyarı açık/kapalı, metni kurallara karşı dene | `/saved-queries`, `/alerts/test` | viewer / analyst |
| Raporlar | “Rapora ekle” ile biriken bulgular, kaynaklar ve göstergeler; Markdown/HTML/JSON/STIX 2.1 üret, önizle, indir | `/reports/generate` | analyst |
| Erişim ve gizlilik | Kullanıcı anahtarları (oluştur/iptal), kısa ömürlü JWT, KVKK varlık silme, saklama temizliği | `/auth/keys`, `/auth/token`, `/entities/erase`, `/admin/retention/run` | admin |

Rol giriş sırasında yan etkisiz yoklamalarla öğrenilir (`api.ts` → `resolveRole`); menü ve
düğmeler role göre kilitlenir, asıl yetki denetimi sunucudadır.

## Dağıtım

`docker compose --profile full up -d --build` → `https://<sunucu>:8443/ui/` (kök `/` buraya yönlenir;
operatör anahtarı ya da Erişim ekranından üretilen kullanıcı anahtarıyla giriş). İmaj, derlenmiş
dosyaları `nginx.conf` ile 3000'de sunar: özetli `assets/` bir yıl önbelleklenir, `index.html` her
yüklemede doğrulanır. Dış Nginx'in CSP'si satır içi `style` özniteliklerine ve `data:` görsellerine
izin verir; satır içi betik yoktur.

## Geliştirme

```bash
npm ci
npm run dev          # http://localhost:3000, /api → http://localhost:8001 (vite proxy)
npm run build        # tsc + vite → dist/ (nginx /ui/ altında sunulur, base "./")
```

Kimlik bilgisi yalnızca sekmede (`sessionStorage`) tutulur; rapor taslağı tarayıcıda
(`localStorage`) kalır.

## Demo modu

Sunucu olmadan arayüzü göstermek için:

```bash
VITE_DEMO=1 npm run dev          # ya da: VITE_DEMO=1 npm run build:demo → dist-demo/
```

Demo modunda `src/demo.ts` `/api/*` isteklerini tarayıcıda örnek verilerle (example.* alan
adları, belgeleme IP blokları) yanıtlar ve oturumu yönetici olarak açar. Kod dinamik import
ile yüklenir; `VITE_DEMO` olmadan derlenen üretim paketine girmez (CI bunu denetler).

## Sunucu tarafı gereksinimler

- Öğelerin görünmesi için pipeline işçisi çalışmalı (compose `worker` profili).
- Semantik arama için modeller indirilmiş olmalı (`docker compose --profile models run --rm
  osiris-models`); compose `osiris-api`'ye `./models`'i ve `OSIRIS_EMBEDDING_MODEL`'i verir. Model
  yoksa API `503` döner, arayüz bunu açıklayan bir uyarı gösterip tam metin aramaya yönlendirir.
- Tor/I2P plugin'leri `darknet` profilindeki proxy'lerle çalışır.
