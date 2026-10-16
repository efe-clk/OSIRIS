# Username Search Plugin'i

Maigret tarzı kullanıcı adı taraması: `sites.json` DB'sindeki profil
URL'lerini yoklar, bulunan profilleri öğe olarak döndürür.

## Kullanım

```bash
osiris collect username-search --config '{"username":"ornek","tags":["dev"],"max_sites":50}'
```

## Binlerce site (Maigret + WhatsMyName birleşik DB)

`sites_full.json` (**~2500 site**) repoda gömülü gelir — ek indirme gerekmez:

```bash
osiris collect username-search --config '{"username":"ornek","full":true,"max_sites":300}'
```

Kaynaklar: [Maigret](https://github.com/soxoj/maigret) (~1980 doğrudan site)
+ [WhatsMyName](https://github.com/WebBreacher/WhatsMyName) (~535 benzersiz).
Motor-tabanlı (arama motoru gerektiren) ve devre-dışı siteler elenir.

DB'yi güncellemek için:

```bash
python3 plugins/username-search/tools/build_full_db.py
```

Özel/ham Maigret `data.json` da verilebilir (`sites_db` — sarmalayıcı,
`presenseStrs` yazım hatası ve dize-liste alanları tolere edilir). Güvenlik
nedeniyle `sites_db` yalnızca plugin dizinindeki ya da operatörün
`OSIRIS_USERNAME_DB_DIR` ile izin verdiği dizindeki bir `.json` dosyasını
gösterebilir; başka yollar reddedilir.

Desteklenen `checkType` değerleri: `status_code`, `message`
(`presenceStrs`/`absenceStrs`), `response_url`. Bilinmeyen türler atlanır.

## Notlar

- Tüm istekler `osiris.security.fetch_url` üzerinden gider (SSRF korumalı,
  yönlendirme-denetimli, gövde tavanlı).
- Erişilemeyen siteler "bilinmiyor" sayılıp atlanır; tek site hatası
  taramayı durdurmaz.
- Kibarlık: varsayılan 10 paralel işçi, site başına 15 sn zaman aşımı.
- Tempo (KVKK / site kullanım şartları): `rate_per_sec` tüm siteler için
  toplam istek/sn üst sınırıdır (varsayılan 10, en fazla 50, `0` = sınırsız);
  `per_host_interval_ms` aynı host'a ardışık iki istek arasındaki en kısa
  süredir (varsayılan 1000). Örn. 200 site ≈ en az 20 sn sürer — senkron
  `/collect` çağrısında Nginx'in 60 sn `proxy_read_timeout` sınırını gözetin.
