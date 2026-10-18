# username-search plugin'i

Maigret tarzı kullanıcı adı taraması: bir kullanıcı adının profil adresini site veritabanındaki
her sitede yoklar ve profil bulunan her siteyi bir öğe olarak döndürür. Tüm alanlar, sınırlar ve
çıktı biçimi: [docs/PLUGINS.tr.md](../../docs/PLUGINS.tr.md#username-search).

*English: see [docs/PLUGINS.md](../../docs/PLUGINS.md#username-search).*

## Kullanım

```bash
# CLI (öğeler kuyruğa yazılır)
osiris collect username-search --config '{"username":"ornek","tags":["dev"],"max_sites":50}'

# Tam DB (~2 500 site)
osiris collect username-search --config '{"username":"ornek","full":true,"max_sites":300}'

# API — önizlemeyle
curl -sk -X POST https://<sunucu>:8443/api/collect -H "X-API-Key: <anahtar>" \
  -H 'Content-Type: application/json' \
  -d '{"plugin_id":"username-search","config":{"username":"ornek"},"include_items":true}'
```

Web arayüzündeki *Kullanıcı adı tarama* ekranı aynı plugin'i kullanır; sonuçları CSV/JSON olarak
indirme, grafa bağlama ve rapora ekleme imkânı verir.

## Site veritabanları

| Dosya | Site | Kaynak |
|-------|------|--------|
| `sites.json` | 59 | Elle seçilmiş hızlı liste (varsayılan) |
| `sites_full.json` | ~2 500 | [Maigret](https://github.com/soxoj/maigret) (~1 980 doğrudan site) + [WhatsMyName](https://github.com/WebBreacher/WhatsMyName) (~535 benzersiz); arama motoru gerektiren ve devre dışı siteler elenmiş |

İki dosya da **derleme zamanında ikiliye gömülür**; değiştirdiğinizde imajı yeniden derleyin
(`docker compose --profile full build`).

Tam DB'yi güncellemek için (geliştirme aracı, yalnızca Python standart kütüphanesi):

```bash
python3 plugins/username-search/tools/build_full_db.py            # → sites_full.json
python3 plugins/username-search/tools/build_full_db.py --help     # kaynak ve çıktı seçenekleri
```

## Özel site DB'si

`sites_db` ayarıyla Maigret `data.json` uyumlu bir dosya verilebilir (sarmalayıcı nesne,
`presenseStrs` yazım hatası ve dize-liste alanları tolere edilir). Güvenlik nedeniyle değer
yalnızca gömülü DB adlarını ya da operatörün `OSIRIS_USERNAME_DB_DIR` ile izin verdiği dizindeki
bir `.json` dosyasını gösterebilir; başka yollar reddedilir.

Desteklenen `checkType` değerleri: `status_code`, `message` (`presenceStrs` / `absenceStrs`),
`response_url`. Bilinmeyen türler atlanır.

## Davranış notları

- Tüm istekler host'un SSRF denetimli HTTP yeteneğinden geçer (yönlendirme denetimi, gövde
  tavanı); iç ağdaki adresler yoklanmaz.
- Erişilemeyen bir site "bilinmiyor" sayılır ve atlanır; tek bir site hatası taramayı durdurmaz.
- Kibarlık ayarları: varsayılan 10 eşzamanlı istek, site başına 15 sn zaman aşımı, toplam
  10 istek/sn (`rate_per_sec`, en çok 50, `0` = sınırsız) ve aynı host'a iki istek arasında en az
  1 000 ms (`per_host_interval_ms`). Site kullanım koşullarına ve KVKK'ya uygun bir tempo seçin.
- Süre en az `max_sites / rate_per_sec` saniyedir (200 site → ≥ 20 sn). Eşzamanlı `/collect`
  çağrısında Nginx'in 60 sn `proxy_read_timeout` sınırını gözetin.
