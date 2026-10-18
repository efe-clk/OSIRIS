# OSIRIS — Davranış Sözleşmesi

> Sürüm 1.0.0-rc · İlgili: [ARCHITECTURE.tr.md](ARCHITECTURE.tr.md), [API.tr.md](API.tr.md),
> Rust geçiş kaydı [RUST_GECIS_DURUMU.md](RUST_GECIS_DURUMU.md)

Bu belge OSIRIS'in sürümler arasında **değişmemesi gereken** davranışlarını tanımlar:
tekilleştirme özeti, denetim zinciri, kuyruk, varlık çıkarma, SSRF kuralları, rapor ve graf
çıktıları gibi. Bunlardan biri değişirse eski veriler yeniden işlendiğinde farklı sonuç verir,
eski kayıtlar doğrulanamaz ya da dış sistemler (STIX tüketicileri, n8n) bozulur.

## Bağlayıcı olan veridir

Belgedeki anlatım yol göstericidir; **bağlayıcı olan testlerdeki veridir**:

| Ne | Nerede | Doğrulama |
|----|--------|-----------|
| Altın veriler | `fixtures/` (dilden bağımsız JSON) | `cargo test --workspace` (crate'lerin `tests/fixtures.rs` dosyaları) |
| Yapısal sözleşmeler | `schemas/*.schema.json` (JSON Schema 2020-12) | `crates/osiris-types/tests/schemas.rs` |
| API davranışı | `tests/contract/` (çalışan servise gerçek HTTP) | CI `integration` işi |
| Veritabanı ve kuyruk | `crates/osiris-server/tests/integration.rs` (gerçek PostgreSQL + Redis) | CI `integration` işi |
| Modeller (NER, embedding) | `fixtures/ner`, `fixtures/embedding` | `crates/osiris-pipeline/tests/models.rs` |

**Köken.** Beklenen değerler Rust geçişi (R2–R6) sırasında özgün Python uygulamasından üretildi
ve Rust ile birebir eşleştirildi. Python uygulaması R7'de kaldırıldı (git geçmişinde `1a4525f`
öncesi). Bu yüzden bazı kurallar "Python'daki gibi" tanımlıdır (ör. boşluk kümesi Python `re`
`\s`'idir). Bu tanımlar artık Rust'ta dondurulmuştur; Python'un sonraki sürümleri onları
değiştirmez.

**Değişiklik kuralı.** Bir fixture'ın `expected` değerini değiştirmek ya da `tests/contract/`'taki
bir beklentiyi gevşetmek **sözleşme değişikliğidir**: bu belge aynı commit'te güncellenir ve
gerekçe commit iletisine yazılır.

---

## 1. `content_hash` (tekilleştirme anahtarı)

Uygulama: `crates/osiris-hash`. Fixture: `fixtures/hash/cases.json`.

Karar (2026-10-16): Unicode normalizasyonu (NFC) yok, küçük harfe çevirme yok, hash sürümü yok.

Girdi kuyruk mesajındaki `item.raw_content` dizesidir. Adımlar **bu sırayla** uygulanır:

1. **Kes:** ilk 200 000 Unicode kod noktası (bayt ya da UTF-16 birimi değil).
2. **HTML'den metne:** BeautifulSoup 4.15.0 `html.parser` + `get_text(" ", strip=True)` davranışı:
   - her metin düğümü ayrı ayrı kırpılır, boş düğümler atılır, kalanlar **tek boşlukla** birleşir
     (`a<br>b<b>c</b>d` → `a b c d`; `Ka<b>lın</b>` → `Ka lın` — satır içi etiket de boşluk ekler);
   - `<script>` / `<style>` içeriği atılır; HTML yorumları atılır, yerlerine ayraç boşluğu kalır
     (`a<!--x-->b` → `a b`);
   - öznitelik değerleri metne girmez; `CDATA` içeriği metindir;
   - varlık (entity) çözümü `html.parser` kurallarıyladır (`&amp b` → `& b`, `&foo;` → `&foo`);
   - bozuk HTML toleransı (`<p>açık <b>kalın`, `a < b > c`, `<<>>x<>`) fixture'da sabittir;
   - ayrıştırıcı hata verirse ham metin kullanılır (ör. 4 300 haneyi aşan ondalık sayısal referans).
3. **Boşluk daralt:** aşağıdaki kümeden bir ya da daha fazla karakter → tek `U+0020`. Küme Python
   `re` `\s` (Unicode) ile aynıdır ve Rust `char::is_whitespace`'ten **farklıdır**:
   `U+0009–000D, U+001C–001F, U+0020, U+0085, U+00A0, U+1680, U+2000–200A, U+2028, U+2029, U+202F,
   U+205F, U+3000`. `U+001C–001F` dahildir; `U+200B` ve `U+FEFF` boşluk değildir, korunur.
4. **Kırp** (aynı küme), sonra yeniden ilk 200 000 kod noktası.
5. `hex(sha256(utf8(sonuç)))` — küçük harfli 64 karakter.

Notlar:

- Özete yalnızca temizlenmiş metin girer; URL, başlık ya da kaynak girmez. Tekilleştirme
  **global**dir (`items.content_hash UNIQUE`): aynı metin farklı kaynaktan gelse de bir kez saklanır.
- Birleşik `é` (U+00E9) ile ayrışık `e` + U+0301 farklı özet üretir (bilinçli: NFC yok).
- HTML ayrıştırıcı `crates/osiris-hash/src/html/`, bs4 4.15.0 + CPython 3.11.16 `html.parser`'ın
  birebir portudur. Varlık, Windows-1252, boş öğe ve dize kabı tabloları
  `tools/gen_html_tables.py` ile bs4'ten üretilir (CI `--check`).

## 2. Denetim zinciri

Uygulama: `crates/osiris-hash/src/audit.rs` (özet), `crates/osiris-db` (yazma/doğrulama).
Fixture: `fixtures/audit/cases.json`. Migration 003, 006.

**Kayıt** — özete giren nesne tam olarak dört anahtardır:
`{"user_id": str, "action": str, "resource": str, "detail": object}`. `id`, `ts`, `prev_hash` girmez.

**Normalizasyon** (yazmadan önce; saklanan değer = özetlenen değer):

- `user_id` / `resource`: `null` → `""`, sonra ilk 200 / 500 kod noktası.
- `detail`: JSON'a gidip döner. `null` → `{}`; nesne değilse `{"value": …}`; NaN/Infinity ya da
  serileşemeyen değer → `{"_unserializable": true}`; `|n| > 2^53 − 1` tamsayılar ondalık dizeye.

**Kanonik dizge** — `chain_version` sütununa göre:

| Sürüm | Serileştirme |
|-------|--------------|
| 1 | Desteklenmiyor (R7 kararı, 2026-10-17). Migration 003 öncesi kayıtlar tabloda kalır ama doğrulanmaz; `Bilinmeyen chain_version: 1` raporlanır. |
| **2** | **RFC 8785 (JCS)**. Doğrulamada güvenli aralık dışı tamsayılar önce float'a çevrilir (jsonb büyük float'ı tamsayı döndürür: `1e+16` → `10000000000000000`; JCS çıktısı aynıdır). |

**Özet:** `hash = hex(sha256(utf8(prev + "|" + kanonik)))`. İlk kayıtta `prev = "GENESIS"` ve
`prev_hash` sütunu `NULL`'dur. Zincir sırası `id` artan sıradır; yazıcılar
`pg_advisory_xact_lock(hashtext('osiris_audit_chain'))` ile sıralanır. Yazıcı yalnızca v2 yazar.
Doğrulama: `osiris-server audit-verify --limit N`.

## 3. STIX 2.1 çıktısı

Uygulama: `crates/osiris-report` (`stix`). Fixture: `fixtures/stix/cases.json` — **bayt bayt**
(çıktı 2 boşluk girintili, ASCII olmayan karakterler kaçışsız JSON).

| Nesne | Kimlik |
|-------|--------|
| `bundle` | Verilen `bundle_id`; verilmezse rastgele UUIDv4 (sözleşme dışı) |
| `identity` | `identity--` + UUIDv5(`NAMESPACE_DNS`, `"osiris.local"`) |
| `vulnerability` (cve), `file` (hash) | `<tip>--` + UUIDv5(`NAMESPACE_URL`, `"{etype}:{value.lower()}"`) |
| `indicator` (ip, domain, email, crypto_address) | `indicator--` + UUIDv5(`NAMESPACE_URL`, pattern dizgesi) |

- Eşleme: `ip` → `ipv4-addr:value` (değer `:` içeriyorsa `ipv6-addr`), `domain` → `domain-name:value`,
  `email` → `email-addr:value`, `crypto_address` → `cryptocurrency-wallet:value`, `hash` → `file`
  (`hashes.MD5`), `cve` → `vulnerability` (`name`). Diğer türler atlanır.
- Zaman damgaları UTC `%Y-%m-%dT%H:%M:%S.000Z`. Değer temizliği: CR/LF → boşluk, ilk 500 kod
  noktası. Pattern kaçışı: `\` → `\\`, `'` → `\'`.
- Bilinen sapma: STIX'in SCO için önerdiği namespace (`00abedb4-…`) kullanılmıyor; değiştirmek
  sözleşme değişikliğidir.

## 4. Plugin sözleşmesi

Uygulama: `crates/osiris-plugin` (host), `crates/osiris-plugins` (11 plugin). Fixture:
`fixtures/plugins/*.json` (11 plugin, 23 senaryo).

- **Manifest:** `schemas/manifest.schema.json`. `config_schema` alan biçimi
  `{type, required?, default?, description?}`. `required: true` → değer `null` ya da `""` ise
  görev çalıştırılmaz.
- **Sonuç:** `schemas/collection_result.schema.json` + `schemas/collected_item.schema.json`.
  `success=false` ⇒ `items` boş, `error` dolu. **`error` metni sözleşme dışıdır**; yalnızca varlığı
  bağlayıcıdır.
- **Senaryo dosyası:** `config`, kayıtlı `http` alışverişleri (method, url, params, json → status,
  headers, body), `dns`, `sockets`, beklenen `expected` ve `expected_network` (tüm HTTP kayıtları
  tüketilmeli; açılan soket adresleri raporlanır). DNS her host'u `93.184.216.34`'e çözer.
- **HTTP (host sağlar):** yalnızca `http`/`https`; URL'de kimlik bilgisi yok; her yönlendirme
  adımında SSRF denetimi; en çok 3 yönlendirme; 303 → GET, 301/302 + POST → GET; gövde
  `max_bytes` ile kesilir; gzip/deflate çözülür. Yanıt metni requests kurallarıyla çözülür:
  başlıktaki charset; `text/*` → ISO-8859-1; `application/json` → UTF-8; aksi hâlde UTF-8 ya da
  windows-1252.
- **Ham TCP:** yalnızca çöz → denetle → **doğrulanmış IP'ye** bağlan.
- **Yapılandırma dönüşümleri** Python semantiğindedir (`int()`, `float()`, `str()`, `bool()`);
  beklenmeyen girdi `plugin hatası` olur.
- **HTML plugin'leri** (web-scraper, tor, i2p): §1'deki ayrıştırıcı + soupsieve 3.0.3 seçicilerinin
  alt kümesi (tür, `#id`, `.class`, öznitelik işleçleri, birleştiriciler, `:nth-*`, `:not`,
  `:is`); desteklenmeyen seçici → tüm belge. Üst düzeyde `~` eşleşmez, `:empty` CDATA'yı saymaz.
- **rss:** feedparser alan kuralları (`crates/osiris-feed`, tablolar `tools/gen_feed_tables.py`).
  Eşdeğerlik `content_hash` düzeyindedir: girdi sayısı, `link`, `published` birebir; temizlenmiş
  `raw_content` aynı; HTML içeren başlıklarda yalnızca kaçış biçimi (`&amp;`/`&`) farklı olabilir.
- **rest-api ve shodan `raw_content`'i** (R5 kararı, 2026-10-17): kaydın **kanonik JSON**'u
  (`osiris_hash::canonical::record_json`) — RFC 8785 anahtar sırası (UTF-16) ve dize/sayı biçimi;
  tamsayılar her büyüklükte tam ondalık; `NaN`/`±Infinity` → `null`; kayıt bir dizeyse olduğu gibi.
  Yanıt gövdesi Python `json.loads` semantiğiyle okunur (tekrarlanan anahtarda ilk konum + son
  değer; `NaN`/`Infinity` kabul). Eşlenmemiş UTF-16 vekili U+FFFD olur.
- **username-search:** site DB'leri gömülüdür; `sites_db` yalnızca gömülü DB adlarını ya da
  `OSIRIS_USERNAME_DB_DIR` altındaki `.json` dosyalarını gösterebilir.
- **`.i2p`:** I2P plugin'i `allow_i2p` ile çağrılır; `.i2p` host'u DNS'e sorulmaz (proxy çözer).

## 5. Kuyruk (Redis Streams)

Uygulama: `crates/osiris-queue` (üretici), `crates/osiris-pipeline/src/stream.rs` (tüketici).
Doğrulama: `crates/osiris-server/tests/integration.rs`.

**Mesaj** — `schemas/queue_message.schema.json`, UTF-8 JSON, en çok 512 000 bayt:
`{"plugin_id", "source_id" (küçük harf UUID | null), "item": CollectedItem, "collected_at": epoch_sn}`.
`source_id` alanı olmayan mesajlar `null` sayılır. İşçi yalnızca geçerli UUID'yi kabul eder;
kaynak silinmişse öğe `source_id = NULL` ile yazılır.

| Öğe | Değer |
|-----|-------|
| Akış | `osiris:stream:raw_items`; tek alanlı girdi: `data` = mesaj JSON'u |
| Üretici | `XADD … MAXLEN ~ OSIRIS_STREAM_MAXLEN (200 000) * data <json>` |
| Tüketici grubu | `osiris-pipeline` (`XGROUP CREATE … 0 MKSTREAM`); tüketici adı `OSIRIS_PIPELINE_CONSUMER` ya da `host-pid` |
| Tur sırası | (1) eski LIST `osiris:raw_items` → `LPOP` ile boşalt; (2) `XAUTOCLAIM` en az 300 sn boşta; (3) `XREADGROUP >` |
| Başarı | `XACK` + `XDEL` (akış yalnızca işlenmemiş/bekleyen girdileri tutar) |
| Bozuk mesaj | ölü-mektup akışı `osiris:stream:raw_items:dead` (`reason=invalid: …`), sonra `XACK` + `XDEL` |
| Depolama hatası | girdi bekleyen listede kalır, süre dolunca yeniden alınır; teslim sayısı > 5 → ölü-mektup (`reason=max_deliveries`) |
| Eski LIST'te depolama hatası | mesaj akışa aktarılır (kaybolmaz) |

Teslim garantisi **en az bir kez**dir. Aynı mesajın iki kez işlenmesi zararsızdır: `content_hash`
UNIQUE (`ON CONFLICT DO NOTHING`) ve varlık bağları upsert'tir. Ölü-mektup girdisi orijinal
alanları + `source_id` (asıl girdi kimliği) + `reason`'u taşır.

**Zamanlama:** standart 5 alanlı cron (UTC; haftanın günü 0 ve 7 = Pazar), kaynak başına aynı anda
tek çalışma, varsayılan kapalı (`OSIRIS_SCHEDULER=on`).

## 6. Veri saklama ve silme (KVKK)

Uygulama: `crates/osiris-db` (`purge_expired`, `purge_orphan_entities`), API uçları. Migration 004.

- `sources.retention_days`: `NULL` → varsayılan; `0` → süreyle silinmez; `N` (1–36 500) →
  `collected_at`'i `N` günden eski öğeler silinir. Kaynaksız öğelere varsayılan uygulanır.
- Varsayılan: `OSIRIS_RETENTION_DAYS` (180; `0` = varsayılan kapalı, yalnızca pozitif kaynak
  süreleri uygulanır).
- Silinen öğelerin varlık bağları kaskadla gider; **hiçbir öğeye bağlı kalmayan varlıklar da
  silinir**.
- Uçlar (admin): `DELETE /items/{id}`, `POST /entities/erase {entity_type, value, delete_items}`
  (tip + kırpılmış, büyük/küçük harf duyarsız değer; aynı adlı graf kenarları da silinir),
  `POST /admin/retention/run`. İşçi `--loop` içinde `OSIRIS_RETENTION_INTERVAL` saniyede bir;
  `osiris-server purge` tek seferlik.
- **Denetim kaydına silinen kişisel veri yazılmaz**; yalnızca `value_sha256` (küçük harfli değerin
  özeti) yazılır.

## 7. REST API

Bağlayıcı tanım `tests/contract/` paketidir; ayrıntılı başvuru [API.tr.md](API.tr.md).

- Zaman damgaları UTC, `YYYY-MM-DDTHH:MM:SS[.ffffff]Z` (mikrosaniye sıfırsa kesirsiz, değilse 6 hane).
- Kimlik sırası: operatör `X-API-Key` → `Bearer` JWT (HS256 sabit; sır yalnızca
  `OSIRIS_JWT_SECRET`) → veritabanı anahtarı (SHA-256 özeti). Roller `viewer` < `analyst` < `admin`.
- Anahtarsız başlatma reddedilir (`OSIRIS_INSECURE_OPEN=1` hariç).
- Hata gövdesi `{"detail": …}`, iç ayrıntı içermez; kodlar 400/401/403/404/405/409/413/422/429/500/502/503.
- Liste uçlarının sayfa sınırları, bozuk UUID'nin `404` olması, alan uzunluk sınırları ve kaynak
  sırlarının maskelenmesi (her derinlikte; `********` güncellemede eski değeri korur) paketle
  sabittir.

## 8. Varlık çıkarma (regex)

Uygulama: `crates/osiris-extract`. Fixture: `fixtures/extract/cases.json` (NER kapalıyken).
Desenler Python `re` Unicode modundaki anlamlarıyla uygulanır:

- Kelime sınırı (`\b`) için kelime karakteri `[\p{L}\p{N}_]`'dir (işaretler, Pc, ZWJ/ZWNJ değil).
- `IGNORECASE` ile `[a-z]` **İ ı ſ K** (U+0130, U+0131, U+017F, U+212A) karakterlerini de eşler
  (`BİLGİ.COM` → `bi̇lgi̇.com`).
- `\s` §1'deki kümedir; `\d` Unicode rakamlarıdır.

| Sıra | Tip | Desen | Ek kural |
|------|-----|-------|----------|
| 1 | email | `[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}` | — |
| 2 | ip | `\b(?:\d{1,3}\.){3}\d{1,3}\b` | geçerli bir IP adresi olmalı |
| 3 | domain | `\b(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,}\b` (IGNORECASE) | başlangıcı bir e-posta aralığındaysa atlanır; değer küçük harfe iner |
| 4 | phone | `\+?\d[\d\s\-().]{7,}\d` | IP / CVE / kripto aralığıyla çakışırsa atlanır; rakam sayısı 7–15 |
| 5 | crypto_address | `\b(bc1\|[13])[a-zA-HJ-NP-Z0-9]{25,39}\b\|\b0x[a-fA-F0-9]{40}\b` | — |
| 6 | cve | `\bCVE-\d{4}-\d{4,7}\b` (IGNORECASE) | değer büyük harfe çıkar |

Değer: kırp + ilk 500 kod noktası. Tekilleştirme `(tip, değer.lower())` ile yapılır, **ilk görülen**
biçim kalır. Çıktı sırası yukarıdaki tip sırası, tip içinde metindeki sıradır. Çakışma denetimi
aralığı `[s, e)`'dir: `s ≤ başlangıç < e` ya da `s < bitiş ≤ e`.

## 9. Graf analizi

Uygulama: `crates/osiris-graph`. Fixture: `fixtures/graph/cases.json`.

- Graf **yönsüzdür**, düğüm çifti başına tek kenar vardır (aynı çifte farklı `relation_type`
  eklemek öncekinin yerine geçer), öz döngü yok sayılır.
- Merkezilik: derece merkeziliği `derece / (n − 1)`; tek düğümlü grafta `1.0` (networkx tanımı).
  Fixture 12 ondalığa yuvarlar.
- Topluluk: Clauset–Newman–Moore açgözlü modülerlik (networkx 3.6.1 `greedy_modularity_communities`,
  ağırlıksız; adım adım taşındı). **Topluluk ve üye sırası sözleşme dışıdır.** Kenarsız graf →
  her düğüm kendi topluluğu.
- `/graph` gövdesi: düğümler ekleme sırasıyla `{"id", …nitelikler}`, kenarlar
  `{"source", "target", "relation_type", "weight"}`. GraphML çıktısı networkx `write_graphml` ile
  **bayt bayt** aynıdır.

## 10. Embedding

Uygulama: `crates/osiris-pipeline/src/embed.rs`. Fixture: `fixtures/embedding/cases.json`.
Migration 005 (`items.embedding_v2 VECTOR(384)`, `items.embedding_model`); eski 1536 boyutlu
sütun migration 006 ile kaldırıldı.

- Model `sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2`; ONNX dosyası
  `Qdrant/paraphrase-multilingual-MiniLM-L12-v2-onnx-Q` `model_optimized.onnx` (sabit revizyon ve
  sha256: `crates/osiris-pipeline/src/models.rs`).
- Girdi `cleaned_content`'in ilk 8 000 kod noktasıdır; bağlam **128 token** (tokenizer
  yapılandırmasındaki `model_max_length` 512 ile `max_length` 128'in küçüğü). Mean pooling,
  ardından **L2 normalize**; sıfır vektör → vektör yok.
- Karşılaştırma bayt bayt değildir: her fixture vektörüyle kosinüs ≥ 0,999 (ONNX çalışma zamanı ve
  CPU farkı).
- `embedding_model` vektörü üreten modeli kaydeder; farklı modellerin vektörleri karıştırılmaz.
  Vektörlenemeyen (boş) öğe `"<model>#skip"` ile işaretlenir.
- Semantik arama: 384 boyutlu sorgu vektörü, `embedding_v2 <=> sorgu` (kosinüs mesafesi),
  `similarity = 1 − mesafe`.

## 11. NER ve dil tespiti

Uygulama: `crates/osiris-pipeline/src/ner.rs`, `lang.rs`. Fixture: `fixtures/ner/cases.json`
(model gerekir).

- Model `onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX` `onnx/model_int8.onnx` (Davlan
  mBERT-hrl, AFL-3.0 — depoda dağıtılmaz). CPU, `intra_op_num_threads = 1`.
- Kurallar: ilk 10 000 kod noktası → özel tokensız tokenize → 510 token'lık pencereler
  (`[CLS]…[SEP]`) → argmax → kelime etiketi = ilk alt-token → BIO birleştirme → uçlardan boşluk ve
  Unicode `P*` noktalama atılır → 2 kod noktasından kısa olanlar atılır. `PER` → `person`,
  `ORG` → `org`, `LOC` → `location`; `DATE` atılır.
- **Ofsetler Unicode kod noktasıdır** (tokenizer'ın bayt ofsetleri dilimlemeden önce çevrilir;
  `emoji_ofset` durumu bunu sınar).
- NER varlıkları regex varlıklarından **sonra** eklenir ve aynı `(tip, değer.lower())`
  tekilleştirmesine girer. Model yoksa ya da `OSIRIS_NER=off` ise yalnızca regex çalışır.
- Aynı model ve aynı CPU sınıfında birebir eşitlik beklenir; int8 çekirdeklerinde CPU'ya bağlı
  nadir argmax farkları için fixture'a tolerans eklemek sözleşme değişikliğidir.
- **Dil tespiti** lingua ile 16 dilde yapılır (Arapça, Çince, Felemenkçe, İngilizce, Fransızca,
  Almanca, İtalyanca, Japonca, Korece, Farsça, Lehçe, Portekizce, Rusça, İspanyolca, Türkçe,
  Ukraynaca; metnin ilk 500 karakteri). Kod ISO 639-1'dir (Çince `zh-cn`), belirsizse `unknown`.
  `language` alanı sözleşme dışıdır ve özete girmez.

## 12. SSRF denetimi ve temizleyiciler

Uygulama: `crates/osiris-security`. Fixture: `fixtures/security/cases.json` (dosyadaki sabit
`dns` haritasıyla; haritada olmayan host çözülemez). **Hata iletileri sözleşmenin parçasıdır**
(kullanıcıya ve plugin sonucuna yansır).

- URL: boş ya da 2 048 kod noktasından uzun → red. Kırpılmış URL'de **ters eğik çizgi, C0 kontrol
  karakteri ya da DEL** → `URL geçersiz karakter içeriyor` (E14: `http://127.0.0.1\x.saldirgan.example/`
  denetimde saldırganın alan adı, bağlantıda 127.0.0.1 olurdu). Bölme CPython 3.11 `urlsplit`
  kurallarıyladır (WHATWG değil).
- Şema yalnızca `http`/`https`; kullanıcı bilgisi (boş olmayan kullanıcı ya da parola) yasak;
  `.onion` yalnızca `allow_onion`, `.i2p` yalnızca `allow_i2p` ile (DNS'e gidilmez); port 1–65535.
- Host: kırp → küçük harf → sondaki noktalar atılır; `localhost` red; IP literali Python
  `ipaddress` kurallarıyla (baştaki sıfırlı IPv4 literal sayılmaz → DNS'e gider), değilse DNS.
  Kayıt yoksa red; **herhangi bir** kayıt engelli bloktaysa red (fail-closed).
- Engelli bloklar: RFC 1918, loopback, link-local/metadata, CGNAT, TEST-NET, multicast,
  `0.0.0.0`, `::`, `::1`, `fc00::/7`, `fe80::/10`. IPv4-mapped IPv6 açılarak denetlenir.
- Temizleyiciler (`sanitize_domain`, `sanitize_hostname`, `sanitize_irc_token`,
  `sanitize_channel`, `mask_secret`): uzunluklar kod noktasıdır; "alfanümerik" = `[\p{L}\p{N}]`.

## 13. Rapor ve uyarı

- **Rapor** (`crates/osiris-report`, `fixtures/report/cases.json`): Markdown/HTML çıktısı, kaçışlı
  (`autoescape`) Jinja2 şablonlarının çıktısıyla bayt bayt aynıdır; kaçış `& < > " '` →
  `&amp; &lt; &gt; &#34; &#39;`. Alan sınırları (kod noktası): başlık 500, kapsam 1 000, özet
  20 000, bulgu ≤ 500 (başlık 500, açıklama 5 000), kaynak ≤ 500 × 1 000. `generated_at` UTC ISO
  8601. CSV: excel lehçesi (`\r\n`, gerektiğinde tırnak), `= + - @ \t \r` ile başlayan dize
  hücresinin başına `'`; başlık ilk satırın anahtarları (≤ 50), ≤ 10 000 satır; dosya yalnızca
  `output_dir` altına yazılır.
- **Uyarı eşleşmesi** (`crates/osiris-alert`, `fixtures/alert/cases.json`): içerik (ilk 50 000) ve
  başlık (ilk 2 000) kesilip küçük harfe çevrilir; kural metni kırpılıp küçük harfe çevrilir ve
  500'e kesilir; alt dize eşleşmesi; öğe başına ≤ 100 kural. Anomali: popülasyon standart sapması,
  `|z| > eşik`; sapma 0 iken farklı değer → `z_score = null`; ısınma `min_samples`; yeni değer
  tabana önce katılmaz. Ayar sınırları: pencere 10–10 000, eşik 0,5–10, ısınma 5–1 000.
