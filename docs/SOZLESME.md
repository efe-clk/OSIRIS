# OSIRIS — Diller Arası Geçiş Sözleşmesi

> Rust geçişi **R2** · Plan: [RUST_GECIS_PLANI.md](RUST_GECIS_PLANI.md) · Durum: [RUST_GECIS_DURUMU.md](RUST_GECIS_DURUMU.md)
>
> Bu belge, Python ve Rust uygulamalarının **aynı girdiye aynı çıktıyı** vermesi gereken her
> noktayı tanımlar. Bağlayıcı olan metin değil **veridir**:
>
> | Ne | Nerede | Python doğrulaması |
> |---|---|---|
> | Altın veriler | `fixtures/` (dilden bağımsız JSON) | `tests/golden/test_golden.py` |
> | Yapısal sözleşmeler | `schemas/*.schema.json` (JSON Schema 2020-12) | `tests/golden/test_schemas.py` |
> | API davranışı | `tests/contract/` (çalışan servise gerçek HTTP) | CI `integration` işi |
> | DB davranışı | `tests/integration/` (gerçek PostgreSQL) | CI `integration` işi |
>
> **Değişiklik kuralı:** `python tests/golden/generate.py --check` bir fark gösteriyorsa bu bir
> *sözleşme değişikliğidir*. Bu belge güncellenir, fark commit'te gözden geçirilir; Rust tarafı
> aynı commit'te (ya da açık bir sürüm artışıyla) uyarlanır.

---

## 1. `content_hash` (tekilleştirme anahtarı)

Karar (2026-10-16): **bugünkü davranış aynen korunur** — NFC yok, küçük harfe çevirme yok, hash
sürümü artırılmaz. Fixture: `fixtures/hash/cases.json` (23 durum).

Girdi: kuyruk mesajındaki `item.raw_content` (dize). Adımlar **bu sırayla**:

1. **Kes:** ilk 200 000 **Unicode kod noktası** (bayt değil, UTF-16 birimi değil).
2. **HTML soy:** Python `BeautifulSoup(text, "html.parser").get_text(" ", strip=True)` ile birebir:
   - her metin düğümü ayrı ayrı trim edilir, boş düğümler atılır, kalanlar **tek boşlukla** birleşir
     (`a<br>b<b>c</b>d` → `a b c d`; `Ka<b>lın</b>` → `Ka lın` — satır içi etiket de boşluk ekler)
   - `<script>` / `<style>` içeriği **atılır**; HTML yorumları atılır (yerine ayraç boşluğu kalır: `a<!--x-->b` → `a b`)
   - öznitelik değerleri metne girmez; `CDATA` içeriği metindir
   - entity çözümü `html.parser` kurallarıyla (`&amp b` → `& b`, `&foo;` → `&foo`) — kenar durumlar fixture'da
   - bozuk HTML toleransı (`<p>açık <b>kalın`, `a < b > c`, `<<>>x<>`) fixture'da sabit
   - Ayrıştırıcı istisna atarsa ham metin kullanılır.
3. **Boşluk daralt:** aşağıdaki kümeden bir veya daha fazla karakter → tek `U+0020`.
   Küme Python `re` `\s` (Unicode) ile aynıdır ve **Rust `char::is_whitespace`'ten farklıdır**:
   `U+0009–000D, U+001C–001F, U+0020, U+0085, U+00A0, U+1680, U+2000–200A, U+2028, U+2029, U+202F, U+205F, U+3000`.
   U+001C–U+001F Python'da boşluktur, Unicode `White_Space` değildir — Rust bunları **eklemelidir**.
   U+200B (sıfır genişlik), U+FEFF boşluk **değildir**, korunur.
4. **Trim** (aynı küme), sonra tekrar ilk 200 000 kod noktası.
5. `hex(sha256(utf8(sonuç)))` — küçük harfli 64 karakter.

Notlar:
- Hash'e yalnızca temizlenmiş metin girer; URL/başlık/kaynak girmez. Tekilleştirme **global**dir
  (`items.content_hash UNIQUE`).
- `Café` (U+00E9) ve `Café` farklı hash üretir (bilinçli: NFC yok).
- Referans sürümler: `beautifulsoup4 4.15.0`. bs4 sürüm yükseltmesi bir sözleşme değişikliği
  olabilir → yükseltmede `generate.py --check` zorunlu.

## 2. Denetim (audit) zinciri

Kaynak: `osiris-sdk/osiris/audit.py`. Fixture: `fixtures/audit/cases.json`. Migration 003.

**Kayıt** (hash'e giren nesne) — tam olarak dört anahtar:
`{"user_id": str, "action": str, "resource": str, "detail": object}`. `id`, `ts`, `prev_hash` girmez.

**Normalizasyon (yazmadan önce, saklanan değer = hash'lenen değer):**
`user_id`/`resource`: `null` → `""`, sonra ilk 200 / 500 kod noktası. `detail`: JSON'a gidip döner
(`null` → `{}`; nesne değilse `{"value": …}`; NaN/Infinity veya serileşemeyen → `{"_unserializable": true}`);
`|n| > 2^53 − 1` tamsayılar **ondalık dizeye** çevrilir.

**Kanonik dizge** — `chain_version` sütununa göre:

| Sürüm | Serileştirme |
|---|---|
| 1 | Python `json.dumps(kayıt, ensure_ascii=False, sort_keys=True, separators=(",", ":"), default=str)` — yalnızca doğrulanır, yeni yazılmaz |
| **2** | **RFC 8785 (JCS)**. Doğrulamada güvenli aralık dışı tamsayılar önce float'a çevrilir (jsonb büyük float'ı tamsayı döndürür: `1e+16` → `10000000000000000`; JCS çıktısı aynıdır) |

**Hash:** `hash = hex(sha256(utf8(prev + "|" + kanonik)))`; ilk kayıtta `prev = "GENESIS"`, DB'de
ilk kaydın `prev_hash` sütunu `NULL`. Zincir sırası `id` artan; yazarlar
`pg_advisory_xact_lock(hashtext('osiris_audit_chain'))` ile sıralanır.

Geçiş: migration 003 öncesi kayıtlar `chain_version = 1` (varsayılan). Sütun yoksa yazıcı v1 yazar
(kayıt kaybolmaz). Doğrulayıcı kayıt başına sürümü okur; karışık zincir geçerlidir.

## 3. STIX 2.1 çıktısı

Kaynak: `ReportGenerator.generate_stix(identity_name, title, entities, *, now, bundle_id)`.
Fixture: `fixtures/stix/cases.json` — **bayt bayt** (çıktı `json.dumps(…, ensure_ascii=False, indent=2)`).

| Nesne | Kimlik |
|---|---|
| bundle | `bundle_id` parametresi (verilmezse uuid4 — sözleşme dışı) |
| identity | `identity--` + UUIDv5(`NAMESPACE_DNS`, `"osiris.local"`) |
| vulnerability / file | `<tip>--` + UUIDv5(`NAMESPACE_URL`, `"{etype}:{value.lower()}"`) |
| indicator | `indicator--` + UUIDv5(`NAMESPACE_URL`, pattern dizgesi) |

Zaman damgaları: `now` → UTC `%Y-%m-%dT%H:%M:%S.000Z`. Varlık eşlemesi, değer temizliği (CR/LF →
boşluk, 500 kod noktası), pattern kaçışı (`\` → `\\`, `'` → `\'`), IPv6 tespiti (`:` içeriyorsa) ve
atlanan tipler fixture'dadır. Bilinen sapma: STIX'in SCO için önerdiği namespace
(`00abedb4-…`) kullanılmıyor; değiştirmek sözleşme değişikliğidir.

## 4. Plugin sözleşmesi

- **Manifest:** `schemas/manifest.schema.json`. `config_schema` alan biçimi: `{type, required?, default?, description?}`.
  `required: true` → değer `null` veya `""` ise görev çalıştırılmaz.
- **Sonuç:** `schemas/collection_result.schema.json` + `schemas/collected_item.schema.json`.
  `success=false` ⇒ `items` boş, `error` dolu. **`error` metni sözleşme dışıdır** (yalnızca varlığı bağlayıcı).
- **Davranış:** `fixtures/plugins/*.json` — 11 plugin, 23 senaryo. Her dosya: `config`, kayıtlı
  `http` değiş tokuşları (method, url, params, json → status, headers, body), `dns`, `sockets` ve
  beklenen `expected` + `expected_network` (tüm HTTP kayıtları tüketilmeli; açılan soket adresleri).
  Fixture'larda DNS her host'u `93.184.216.34`'e çözer.
- **Ağ erişimi (host sağlar):** HTTP yalnızca `fetch_url` semantiğiyle: http/https, URL'de kimlik yok,
  her yönlendirme adımında SSRF denetimi, en fazla 3 yönlendirme, 303 → GET, 301/302 + POST → GET,
  gövde `max_bytes` ile kesilir. SSRF: engelli bloklar (RFC1918, loopback, link-local/metadata, CGNAT,
  TEST-NET, multicast, `::1`, `fc00::/7`, `fe80::/10`, `0.0.0.0`, `::`); **IPv4-mapped IPv6 açılarak**
  denetlenir; DNS kayıtlarından **biri bile** engelliyse host reddedilir; çözülemeyen host reddedilir.
  Ham TCP yalnızca `safe_create_connection` ile (çöz → denetle → **doğrulanmış IP'ye** bağlan).
- **Bilinen biçim borcu (R5 kararı):** `rest-api` ve `shodan` `raw_content` alanına Python `str(dict)`
  (repr: `{'a': 1, 'b': None, 'c': True}`) yazar. Rust bunu ya birebir üretir ya da iki plugin JSON'a
  geçer — ikincisi bu plugin'lerin yeni öğelerinin `content_hash`'ini değiştirir (bilinçli karar gerekir).

## 5. Kuyruk mesajı (`osiris:raw_items`)

`schemas/queue_message.schema.json`. Redis LIST (`RPUSH` / `LPOP`), UTF-8 JSON, en fazla 512 000 bayt:
`{"plugin_id", "source_id" (UUID küçük harf | null), "item": CollectedItem, "collected_at": epoch_sn}`.
`source_id` R2'de eklendi; alanı olmayan eski mesajlar `null` sayılır. Pipeline yalnızca geçerli
UUID'yi kabul eder; kaynak silinmişse öğe `source_id = NULL` ile yazılır. Bugünkü teslim garantisi
**en fazla bir kez**; R3'te Streams + consumer group'a geçer (bu bölüm güncellenecek).

## 6. Veri saklama ve silme (KVKK)

Migration 004, `osiris/retention.py`. Doğrulama: `tests/integration`.

- `sources.retention_days`: `NULL` → varsayılan; `0` → süreyle silinmez; `N` (1-36500) → `collected_at`'i
  `N` günden eski öğeler silinir. Kaynaksız öğelere varsayılan uygulanır.
- Varsayılan: `OSIRIS_RETENTION_DAYS` (180; `0` = varsayılan kapalı, yalnızca pozitif kaynak süreleri).
- Silinen öğelerin varlık bağları kaskadla gider; **hiçbir öğeye bağlı kalmayan varlıklar da silinir**.
- Uçlar (admin): `DELETE /items/{id}`, `POST /entities/erase {entity_type, value, delete_items}`
  (tip + büyük/küçük harf duyarsız değer; graf kenarlarındaki aynı ad da silinir),
  `POST /admin/retention/run`. Pipeline: `--purge`, `--loop` içinde `OSIRIS_RETENTION_INTERVAL` sn.
- **Denetim kaydına silinen kişisel veri yazılmaz**, yalnızca `value_sha256` (küçük harfli değerin özeti).

## 7. REST API

Bağlayıcı tanım `tests/contract/` paketidir (ayrıntılı başvuru: [API.md](API.md)). Paket R6'da Rust
API'sine **değiştirilmeden** koşmalıdır. Özet: kimlik sırası operatör `X-API-Key` → `Bearer` JWT
(HS256 sabit; sır yalnızca `OSIRIS_JWT_SECRET`) → DB anahtarı (SHA-256); roller viewer < analyst < admin;
anahtarsız başlatma reddedilir (`OSIRIS_INSECURE_OPEN=1` hariç); hata gövdesi `{"detail": …}` ve iç
ayrıntı içermez; kodlar 400/401/403/404/409/413/422/500/502/503.

## 8. Varlık çıkarma (regex)

Kaynak: `ProcessingPipeline.extract_entities`. Fixture: `fixtures/extract/cases.json` (NER kapalı; NER
R3'te ayrı fixture'larla sabitlenir). Python `re` Unicode modunda: `\d` ve `\b` Unicode'dur.

| Sıra | Tip | Desen | Ek kural |
|---|---|---|---|
| 1 | email | `[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}` | — |
| 2 | ip | `\b(?:\d{1,3}\.){3}\d{1,3}\b` | `ipaddress.ip_address` geçmeli |
| 3 | domain | `\b(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,}\b` (IGNORECASE) | başlangıcı bir e-posta aralığındaysa atlanır; değer küçük harfe iner |
| 4 | phone | `\+?\d[\d\s\-().]{7,}\d` | IP / CVE / kripto aralığıyla çakışırsa atlanır; rakam sayısı 7-15 |
| 5 | crypto_address | `\b(bc1\|[13])[a-zA-HJ-NP-Z0-9]{25,39}\b\|\b0x[a-fA-F0-9]{40}\b` | — |
| 6 | cve | `\bCVE-\d{4}-\d{4,7}\b` (IGNORECASE) | değer büyük harfe çıkar |

Değer: trim + ilk 500 kod noktası. Tekilleştirme `(tip, değer.lower())` ile, **ilk görülen** biçim
kalır. Çıktı sırası yukarıdaki tip sırası, tip içinde metindeki sıra. Çakışma denetimi: aralık
`[s, e)`; `s ≤ başlangıç < e` veya `s < bitiş ≤ e`.

## 9. Graf analizi

Fixture: `fixtures/graph/cases.json`. Graf **yönsüz, çift başına tek kenar** (aynı çifte farklı
`relation_type` eklemek öncekini ezer), öz döngü yok sayılır.

- Merkezilik: derece merkeziliği `derece / (n − 1)`; tek düğümlü grafta `1.0` (networkx tanımı).
  Fixture 12 ondalığa yuvarlar.
- Topluluk: Clauset–Newman–Moore açgözlü modülerlik (networkx `greedy_modularity_communities`,
  ağırlıksız). **Topluluk ve üye sırası sözleşme dışıdır** (fixture sıralı listeler karşılaştırır).
  Kenarsız graf → her düğüm kendi topluluğu.
