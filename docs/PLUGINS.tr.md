> **English:** [PLUGINS.md](PLUGINS.md)

# OSIRIS Plugin Başvurusu

OSIRIS'in veri kaynakları 11 plugin'dir; hepsi `osiris-server` ikilisine gömülüdür. Bu belge her
plugin'in ne yaptığını, yapılandırma alanlarını, ürettiği öğeleri ve sınırlarını anlatır. Aynı
şema canlı sistemde `GET /plugins` ile okunur (web arayüzündeki formlar oradan çizilir).

Yeni plugin yazmak için: [CONTRIBUTING.tr.md](CONTRIBUTING.tr.md#4-yeni-plugin-ekleme).

## Ortak kurallar

- **Zorunlu alanlar.** Manifest'te `required: true` olan bir alan boşsa (`null` ya da `""`)
  plugin hiç çağrılmaz; sonuç `Zorunlu alanlar eksik: <alanlar>` hatasıdır.
- **Ağ erişimi.** Plugin'ler soket açmaz; tüm HTTP, DNS ve TCP istekleri host'un SSRF
  denetiminden geçer (özel/iç adresler, çözülemeyen host'lar, URL'de kimlik bilgisi reddedilir;
  her yönlendirme yeniden denetlenir, en çok 3 yönlendirme). Ayrıntı:
  [ARCHITECTURE.tr.md §6](ARCHITECTURE.tr.md#6-plugin-host-modeli).
- **User-Agent.** `OSIRIS-OSINT/0.1 (+self-hosted)`.
- **Öğe alanları.** Her öğe `url`, `title`, `raw_content`, `published_at`, `metadata`, `tags`
  taşır. İşleme hattı `raw_content`'i temizler, tekilleştirir ve varlıklarını çıkarır.
- **Hata ile çökme.** Ağ ya da girdi hatası `success=false` ve bir hata iletisiyle döner; API'de
  `502` olur. Beklenmeyen bir durum `plugin hatası: …` olarak raporlanır; servis etkilenmez.
- **Kaynakta `url`.** Plugin bir kaynak üzerinden çalıştırılıyorsa ve `config`'te `url`,
  `feed_url` ya da `endpoint` boşsa kaynağın `url` alanı oraya kopyalanır.

## Özet

| Plugin | Ağ | Varsayılan cron | Ne toplar | Öğe sayısı |
|--------|----|-----------------|-----------|-----------|
| [`rss`](#rss) | rss | `*/15 * * * *` | RSS/Atom girdileri | ≤ 200 (`max_items`) |
| [`web-scraper`](#web-scraper) | www | `0 */6 * * *` | Bir web sayfasının metni (isteğe bağlı CSS seçiciyle) | ≤ 20 |
| [`rest-api`](#rest-api) | api | `0 * * * *` | Bir JSON API'nin kayıtları | ≤ 200 |
| [`shodan`](#shodan) | api | `0 */6 * * *` | Shodan host araması sonuçları | ≤ 100 |
| [`dns-whois`](#dns-whois) | api | `0 */12 * * *` | DNS kayıtları + WHOIS metni | ≤ 100 |
| [`blockchain`](#blockchain) | blockchain | `*/10 * * * *` | Adres bakiyesi/işlem sayısı ya da blok numarası | 1 |
| [`username-search`](#username-search) | www | — | Bir kullanıcı adının profil bulunduğu siteler | bulunan profil sayısı |
| [`tor`](#tor) | tor | `0 */12 * * *` | Bir `.onion` sayfasının metni | 1 |
| [`i2p`](#i2p) | i2p | `0 */12 * * *` | Bir eepsite sayfasının metni | 1 |
| [`irc`](#irc) | irc | `*/5 * * * *` | Kanallardaki mesajlar (20 sn dinleme) | ≤ 200 |
| [`matrix`](#matrix) | matrix | `*/5 * * * *` | Bir odanın son mesajları | ≤ 100 |

---

## rss

RSS 2.0 ve Atom akışlarını okur.

| Alan | Tür | Varsayılan | Açıklama |
|------|-----|-----------|----------|
| `feed_url` | dize | **zorunlu** | Akış adresi |
| `max_items` | tamsayı | 50 | Alınacak en çok girdi (1–200) |

- Yanıt en çok 2 MB okunur, zaman aşımı 30 sn.
- Her girdi bir öğe: `url` = girdi bağlantısı, `title`, `raw_content` = özet/içerik (≤ 50 000),
  `published_at` = yayımlanma tarihi, `metadata.feed`.
- Ayrıştırılamayan akış: `Geçersiz feed`.

```json
{"plugin_id": "rss", "config": {"feed_url": "https://example.com/feed.xml", "max_items": 20}}
```

## web-scraper

Bir web sayfasını indirip metnini çıkarır.

| Alan | Tür | Varsayılan | Açıklama |
|------|-----|-----------|----------|
| `url` | dize | **zorunlu** | Sayfa adresi |
| `css_selector` | dize | — | Yalnızca eşleşen öğelerin metni (her eşleşme ayrı öğe, en çok 20) |
| `depth` | tamsayı | 1 | Yalnızca `metadata.depth`'e yazılır; bağlantı izlenmez |
| `follow_links` | mantıksal | false | Şu an etkisiz (bağlantı izleme uygulanmadı) |

- Yanıt en çok 2 MB, zaman aşımı 30 sn; öğe metni ≤ 50 000 karakter.
- Seçici verilmezse ya da geçersizse tüm sayfa tek öğe olur. Desteklenen seçiciler: tür, `#id`,
  `.sınıf`, öznitelik işleçleri, birleştiriciler, `:nth-*`, `:not`, `:is`.
- `title` = sayfanın `<title>`'ı; boş metinli eşleşmeler atlanır.

```json
{"plugin_id": "web-scraper", "config": {"url": "https://example.com/duyurular", "css_selector": "article"}}
```

## rest-api

Bir JSON API'yi çağırır ve dönen kayıtları öğeye çevirir.

| Alan | Tür | Varsayılan | Açıklama |
|------|-----|-----------|----------|
| `endpoint` | dize | **zorunlu** | API adresi |
| `method` | dize | `GET` | `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, `OPTIONS` |
| `headers` | nesne | `{}` | İstek başlıkları (en çok 20; anahtar ≤ 200, değer ≤ 2000 karakter) |
| `params` | nesne | `{}` | Sorgu parametreleri (aynı sınırlar) |
| `json_path` | dize | — | Kayıt dizisine giden noktalı yol, ör. `data.items` (≤ 500) |

- Zaman aşımı 30 sn. Yanıt JSON değilse `Geçersiz JSON yanıtı`; `json_path` bulunamazsa
  `json_path geçersiz`.
- Sonuç dizi ise her eleman bir öğe (en çok 200), değilse tek öğe.
- `raw_content` = kaydın **kanonik JSON**'u (anahtarlar sıralı, ≤ 50 000); `title` = kaydın
  `title` alanı (varsa); `url` = `endpoint`.
- Kaynakta saklanan `headers`/`params` içindeki sırlar (`Authorization`, `api_key` …) okumada
  maskelenir.

```json
{"plugin_id": "rest-api", "config": {"endpoint": "https://api.example.com/v1/olaylar",
  "json_path": "data.items", "headers": {"Authorization": "Bearer <jeton>"}}}
```

## shodan

Shodan'ın host arama API'sini (`https://api.shodan.io/shodan/host/search`) çağırır.

| Alan | Tür | Varsayılan | Açıklama |
|------|-----|-----------|----------|
| `api_key` | dize | **zorunlu** | Shodan API anahtarı |
| `query` | dize | **zorunlu** | Shodan sorgusu (≤ 500), ör. `port:22 country:TR` |

- İlk 100 eşleşme; her biri bir öğe: `url` = `http://<ip>:<port>`, `title` = ürün adı,
  `raw_content` = eşleşmenin kanonik JSON'u, `metadata` = `ip`, `port`.
- `401` → `Shodan: yetkisiz (API anahtarı geçersiz)`, `429` → `Shodan: hız limiti aşıldı`.
  API anahtarı hata iletilerine ve üst veriye yazılmaz.

## dns-whois

Pasif keşif: DNS kayıtları ve IANA WHOIS.

| Alan | Tür | Varsayılan | Açıklama |
|------|-----|-----------|----------|
| `domain` | dize | **zorunlu** | Alan adı |
| `record_types` | dizi | `["A","AAAA","MX","NS","TXT"]` | En çok 10 tür; izinli: `A`, `AAAA`, `MX`, `NS`, `TXT`, `CNAME`, `SOA`, `PTR`, `SRV` (diğerleri atlanır; hiçbiri kalmazsa `A`) |

- Her kayıt türü için en çok 50 yanıt, her yanıt bir öğe (`metadata.record_type`); ardından
  `whois.iana.org:43`'ten gelen WHOIS metni ayrı bir öğe (`record_type: "WHOIS"`, ≤ 20 000
  karakter). Toplam en çok 100 öğe.
- Alan adı yoksa `Domain bulunamadı`. Yanıtı olmayan türler sessizce atlanır.

## blockchain

İki kullanım biçimi vardır: bir adresi izlemek ya da zincirin son blok numarasını almak.

| Alan | Tür | Varsayılan | Açıklama |
|------|-----|-----------|----------|
| `rpc_url` | dize | **zorunlu** | EVM JSON-RPC adresi (Ethereum vb.). Bitcoin'de kullanılmaz, ama zorunlu alan olduğu için doldurulmalıdır |
| `address` | dize | — | İzlenecek adres (20–100 harf/rakam) |
| `chain` | dize | `ethereum` | `bitcoin` → Blockstream; diğer her değer EVM sayılır |

- **Adres verildiyse (EVM):** `eth_getBalance` + `eth_getTransactionCount`; öğe metni
  `address=… balance_wei=… txcount=…`, etiket `crypto`. Adres `0x` ile başlayan 42 karakter değilse
  öğe üretilmez.
- **Adres verildiyse (`chain: bitcoin`):** `https://blockstream.info/api/address/<adres>`;
  öğe metni `address=… balance_sat=… txcount=…`.
- **Adres yoksa:** `eth_blockNumber`; öğe metni `block_number=…`.
- `rpc_url` de SSRF denetiminden geçer; iç ağdaki bir düğüme bağlanılamaz.

## username-search

Maigret tarzı kullanıcı adı taraması: profil adreslerini sitelerde yoklar ve bulunan her profili
bir öğe olarak döndürür. Web arayüzündeki *Kullanıcı adı tarama* ekranı bu plugin'i kullanır.

| Alan | Tür | Varsayılan | Açıklama |
|------|-----|-----------|----------|
| `username` | dize | **zorunlu** | 2–64 karakter; harf, rakam, `_`, `.`, `-` (ilk karakter harf, rakam ya da `_`) |
| `full` | mantıksal | false | `false`: hızlı DB (59 site); `true`: tam DB (~2 500 site, Maigret + WhatsMyName) |
| `sites` | dizi | — | Yalnızca bu site adları |
| `tags` | dizi | — | Yalnızca bu kategoriler: `dev`, `social`, `photo`, `video`, `music`, `gaming`, `forum`, `blog` |
| `max_sites` | tamsayı | 200 | Yoklanacak en çok site (1–3 000) |
| `workers` | tamsayı | 10 | Eşzamanlı istek (1–20) |
| `timeout` | tamsayı | 15 | Site başına zaman aşımı, sn (5–30) |
| `rate_per_sec` | sayı | 10 | Tüm siteler için toplam istek/sn (0 = sınırsız; en çok 50) |
| `per_host_interval_ms` | tamsayı | 1000 | Aynı host'a iki istek arası en kısa süre (0–60 000) |
| `sites_db` | dize | — | Özel site DB'si (Maigret `data.json` uyumlu); yalnızca gömülü DB adları ya da `OSIRIS_USERNAME_DB_DIR` altındaki `.json` dosyaları |

- Desteklenen denetim türleri: `status_code`, `message` (`presenceStrs`/`absenceStrs`),
  `response_url`. Bilinmeyen türler atlanır; erişilemeyen bir site taramayı durdurmaz.
- Öğe: `url` = profil adresi, `title` = `<kullanıcı> @ <site>`, etiketler `username` + sitenin
  kategorileri, `metadata` = `site`, `username`, `check`. Sonucun üst verisi: `checked`
  (yoklanan), `found` (bulunan), `unknown` (profil bulunamayan ya da yoklanamayan).
- Süre: `max_sites / rate_per_sec` saniyeden kısa sürmez (200 site, 10 istek/sn → en az 20 sn).
  Nginx'in 60 sn zaman aşımını aşmamak için büyük taramalarda `max_sites`'ı düşürün.
- Ayrıntı ve site DB'sini güncelleme: [plugins/username-search/README.md](../plugins/username-search/README.md).

## tor

Bir `.onion` sayfasını Tor üzerinden indirir.

| Alan | Tür | Varsayılan | Açıklama |
|------|-----|-----------|----------|
| `url` | dize | **zorunlu** | `.onion` içeren adres |
| `socks5_host`, `socks5_port` | dize, tamsayı | — | Boş bırakın (aşağıya bakın) |

- Proxy seçimi: `OSIRIS_TOR_SOCKS` tanımlıysa (Compose: `tor:9050`) **her zaman** o kullanılır.
  Tanımsızsa yalnızca loopback bir proxy (`127.0.0.1`/`::1`, varsayılan `127.0.0.1:9050`) kabul
  edilir; başka bir host verilirse toplama reddedilir. Bağlantı SOCKS5h'dir (DNS de Tor içinde
  çözülür).
- Zaman aşımı 60 sn, yanıt ≤ 2 MB; tüm sayfa metni tek öğe (≤ 50 000), `metadata.network: "tor"`.
- Çalışması için `darknet` profili açık olmalı: `docker compose --profile darknet up -d`.

## i2p

Bir eepsite'ı I2P HTTP proxy'si üzerinden indirir.

| Alan | Tür | Varsayılan | Açıklama |
|------|-----|-----------|----------|
| `url` | dize | **zorunlu** | `.i2p` adresi |
| `http_proxy_host`, `http_proxy_port` | dize, tamsayı | — | Boş bırakın |

- Proxy seçimi `tor` ile aynı kuraldır: `OSIRIS_I2P_HTTP_PROXY` (Compose: `i2pd:4444`), yoksa
  yalnızca loopback (varsayılan port 4444). `.i2p` adları DNS'e sorulmaz, proxy çözer.
- Zaman aşımı 60 sn, yanıt ≤ 2 MB; tek öğe, `metadata.network: "i2p"`.
- I2P ağına ilk bağlanma uzun sürebilir; i2pd açıldıktan sonra birkaç dakika bekleyin.

## irc

Bir IRC sunucusuna bağlanır, kanallara katılır ve 20 saniye boyunca mesajları toplar.

| Alan | Tür | Varsayılan | Açıklama |
|------|-----|-----------|----------|
| `server` | dize | **zorunlu** | Sunucu adı (geçerli host adı) |
| `channels` | dizi | **zorunlu** | En çok 10 kanal, ör. `["#osint"]` |
| `port` | tamsayı | 6667 | 1–65535 (düz TCP; TLS yok) |
| `nickname` | dize | `osiris-bot` | ≤ 64; IRC komut enjeksiyonuna karşı temizlenir |

- Bağlantı SSRF denetiminden sonra doğrulanmış IP'ye kurulur (15 sn). `PING`'ler yanıtlanır.
- Yalnızca `PRIVMSG` satırları öğe olur (ham satır, ≤ 2000), `metadata` = `server`, `protocol`.
  En çok 200 öğe; 5 sn sessizlikte dinleme erken biter.

## matrix

Bir Matrix odasının son mesajlarını client-server API v3 ile okur.

| Alan | Tür | Varsayılan | Açıklama |
|------|-----|-----------|----------|
| `homeserver` | dize | **zorunlu** | Ör. `https://matrix.example.org` |
| `room_id` | dize | **zorunlu** | Ör. `!abc123:example.org` (≤ 500) |
| `access_token` | dize | — | Kapalı odalar için erişim jetonu (kaynakta maskelenir) |

- `GET /_matrix/client/v3/rooms/<oda>/messages?dir=b&limit=50`; yanıttaki ilk 100 olaydan
  yalnızca `m.room.message` gövdeleri öğe olur (≤ 20 000), `metadata` = `room`, `sender`.
- `401`/`403` → `Matrix: yetkisiz`.
