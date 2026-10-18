# OSIRIS Web UI

OSIRIS REST API'sinin (`osiris-server api`) tarayıcı arayüzü. React 18 + TypeScript + Tailwind;
ek grafik kütüphanesi yoktur (ilişki grafı canvas üzerinde bağımlılıksız kuvvet yerleşimiyle
çizilir). Arayüz yalnızca REST API'yi kullanır; uç başvurusu [docs/API.tr.md](../docs/API.tr.md).

*English summary: browser UI for the OSIRIS REST API — 11 screens, role-aware menus, demo mode
(`VITE_DEMO=1`). Served under `/ui/` by the deployment Nginx. The commands below are the same.*

## Giriş ve oturum

- Adres: `https://<sunucu>:8443/ui/` (kök `/` buraya yönlenir).
- Giriş ekranında kimlik türünü seçin: **API anahtarı** (operatör anahtarı ya da *Erişim ve
  gizlilik* ekranında üretilen kullanıcı anahtarı) veya **JWT**.
- Arayüz rolü `GET /auth/me` ile öğrenir (yan etkisiz). Menü ve düğmeler role göre kilitlenir;
  asıl yetki denetimi sunucudadır.
- Kimlik bilgisi yalnızca o sekmede (`sessionStorage`) tutulur; sekme kapanınca silinir. Oturum
  sırasında `401` alınırsa giriş ekranına dönülür. Rapor taslağı tarayıcıda (`localStorage`) kalır.

## Ekranlar

| Ekran | Ne yapar | Kullandığı uçlar | En düşük rol |
|---|---|---|---|
| Panel | Sayaçlar, kaynak sağlığı, son öğeler, öne çıkan varlıklar, hızlı arama | `/stats`, `/sources`, `/items/recent`, `/entities/top` | viewer |
| Kullanıcı adı tarama | Maigret tarzı: hızlı (59 site) ya da tam (~2 500 site), kategori süzgeci, tempo ayarları; CSV/JSON indir, grafa bağla, rapora ekle | `POST /collect` (`username-search`, `include_items`), `/graph/relation` | analyst |
| Toplama | Plugin seç → manifest şemasından üretilen form → çalıştır ve önizle; toplu iş (≤ 10); kaynak olarak kaydet | `/plugins`, `/collect`, `/collect/batch`, `POST /sources` | analyst |
| Arama | Tam metin (vurgulu), semantik (benzerlik yüzdesi), varlık; CSV indir, uyarı kuralı olarak kaydet | `/search`, `/search/semantic`, `/search/entity`, `/saved-queries` | viewer |
| Öğeler | Sayfalı liste, süzgeç; detay çekmecesi (içerik, varlıklar, üst veri, KVKK silme) | `/items/recent`, `/items/{id}`, `DELETE /items/{id}` | viewer |
| Varlıklar | Türe göre en çok anılanlar; öğelere/grafa git, rapora gösterge ekle, KVKK silme | `/entities/top`, `/entities/erase` | viewer |
| İlişki grafı | Kuvvet yerleşimi, topluluk renkleri, merkezilik, komşular, ilişki ekleme, GraphML/GEXF | `/graph`, `/graph/analysis`, `/graph/export`, `/graph/relation` | viewer |
| Kaynaklar | Oluştur, düzenle (sırlar maskeli), aç/kapat, şimdi çalıştır, sil; sağlık ve hazır cron ayarları | `/sources`, `GET/PATCH/DELETE /sources/{id}`, `/sources/{id}/collect` | viewer (okuma) / analyst |
| Uyarı kuralları | Kayıtlı sorgular, uyarı aç/kapat, bir metni kurallara karşı dene | `/saved-queries`, `/alerts/test` | viewer / analyst |
| Raporlar | "Rapora ekle" ile biriken bulgular, kaynaklar ve göstergeler; Markdown/HTML/JSON/STIX 2.1 üret, önizle, indir | `/reports/generate` | analyst |
| Erişim ve gizlilik | Kullanıcı anahtarları (oluştur/iptal), kısa ömürlü JWT, KVKK varlık silme, saklama temizliği | `/auth/keys`, `/auth/token`, `/entities/erase`, `/admin/retention/run` | admin |

## Sunucu tarafı gereksinimler

- Toplanan öğelerin görünmesi için işleme hattı çalışmalıdır (Compose `worker` ya da `full` profili).
- Semantik arama için modeller indirilmiş olmalıdır
  (`docker compose --profile models run --rm osiris-models`); model yoksa API `503` döner, arayüz
  bunu açıklayıp tam metin aramaya yönlendirir.
- Tor/I2P plugin'leri `darknet` profilindeki proxy'lerle çalışır.
- Uzun kullanıcı adı taramaları Nginx'in 60 sn zaman aşımına takılabilir; `max_sites`'ı düşürün.

## Dağıtım

`docker compose --profile full up -d --build` arayüz imajını (`osiris-web-ui:1.0.0-rc`) derler.
İmaj iki aşamalıdır: Node 20 ile `npm ci && npm run build`, ardından `nginx:1.31-alpine` derlenmiş
dosyaları `nginx.conf` ile 3000 portunda sunar:

- içerik özetli `assets/` bir yıl önbelleklenir; `index.html` her yüklemede doğrulanır (yeni sürüm
  hemen görünür);
- uygulama `HashRouter` kullanır, sunucu tarafı yönlendirme gerekmez;
- dış Nginx `/ui/` önekini sıyırıp bu servise iletir (`base: "./"` sayesinde göreli yollar çalışır);
- dış Nginx'in CSP'si satır içi `style` özniteliklerine ve `data:` görsellerine izin verir; satır
  içi betik yoktur.

## Geliştirme

```bash
npm ci
npm run dev        # http://localhost:3000; /api/* → http://127.0.0.1:8001 (önek sıyrılır)
npm run lint       # ESLint 10 (typescript-eslint + react-hooks)
npm run build      # tsc + vite → dist/
npm run preview    # derlenmiş paketi yerelde sun
```

Geliştirme sunucusu `/api` isteklerini, üretimdeki Nginx gibi önekini sıyırarak
`127.0.0.1:8001`'e (Compose'un doğrudan API portu) iletir. API'yi yerelde çalıştırmak için:
[docs/CONTRIBUTING.tr.md §2.2](../docs/CONTRIBUTING.tr.md#22-apiyi-yerelde-çalıştırmak).

Lint kuralları `eslint.config.js`'tedir. Klasik kanca kuralları (`rules-of-hooks`,
`exhaustive-deps`) hata düzeyindedir. eslint-plugin-react-hooks 7'nin React Compiler kuralları
açılmamıştır, çünkü arayüz Compiler kullanmaz ve graf tuvali bilinçli olarak ref ve yerinde
değişimle çalışır.

### Kod yapısı

| Dosya | Görev |
|-------|-------|
| `src/api.ts` | API istemcisi, türler, oturum (`sessionStorage`), rol tespiti (`/auth/me`) |
| `src/auth.tsx` | Giriş durumu, `401`'de oturumu kapatma |
| `src/App.tsx` | Yerleşim, menü, rotalar (`HashRouter`) |
| `src/pages/*.tsx` | Ekranlar |
| `src/schemaForm.tsx` | Plugin `config_schema`'sından form üretimi |
| `src/reportDraft.tsx` | "Rapora ekle" taslağı (`localStorage`) |
| `src/itemDrawer.tsx` | Öğe detay çekmecesi |
| `src/ui.tsx`, `src/util.ts` | Ortak bileşenler; biçimlendirme, CSV (formül enjeksiyonuna karşı korumalı) |
| `src/demo.ts`, `src/demo*.json` | Demo modunun sahte API'si ve örnek veriler |

## Demo modu

Sunucu olmadan arayüzü göstermek için:

```bash
VITE_DEMO=1 npm run dev           # ya da: VITE_DEMO=1 npm run build:demo → dist-demo/
```

Demo modunda `src/demo.ts`, `/api/*` isteklerini tarayıcıda örnek verilerle (example.* alan
adları, belgeleme IP blokları) yanıtlar ve oturumu yönetici olarak açar. Kod dinamik `import` ile
yüklenir; `VITE_DEMO` olmadan derlenen üretim paketine girmez (CI bunu denetler).
