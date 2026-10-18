# web-scraper plugin'i

Bir web sayfasını indirir ve metnini çıkarır; isteğe bağlı bir CSS seçiciyle yalnızca sayfanın
belirli bölümlerini alır. Ayrıntılı başvuru: [docs/PLUGINS.tr.md](../../docs/PLUGINS.tr.md#web-scraper).

*English: see [docs/PLUGINS.md](../../docs/PLUGINS.md#web-scraper).*

## Yapılandırma

| Alan | Tür | Zorunlu | Varsayılan | Açıklama |
|------|-----|---------|-----------|----------|
| `url` | dize | evet | — | Hedef sayfa |
| `css_selector` | dize | hayır | — | Yalnızca eşleşen öğelerin metni; her eşleşme ayrı öğe (en çok 20) |
| `depth` | tamsayı | hayır | 1 | Yalnızca öğe üst verisine (`metadata.depth`) yazılır |
| `follow_links` | mantıksal | hayır | false | Şu an etkisiz: bağlantı izleme uygulanmadı |

## Davranış

- `GET`, 30 sn zaman aşımı, yanıt en çok 2 MB; her yönlendirme adımında SSRF denetimi (en çok 3).
- Seçici verilmezse ya da geçersizse tüm sayfa tek öğe olur. Desteklenen seçiciler: tür, `#id`,
  `.sınıf`, öznitelik işleçleri, birleştiriciler, `:nth-*`, `:not`, `:is`.
- Öğe: `url` = sayfa adresi, `title` = sayfanın `<title>`'ı, `raw_content` = metin (≤ 50 000).
  Boş metinli eşleşmeler atlanır. HTML'den metne dönüşüm `content_hash` ile aynı kuralları izler
  ([docs/SOZLESME.md §1](../../docs/SOZLESME.md#1-content_hash-tekilleştirme-anahtarı)).

## Kullanım

```bash
# Deneme (kuyruğa yazmaz; sonuç JSON olarak basılır)
osiris-server collect --plugin web-scraper --config '{"url":"https://example.com","css_selector":"article"}'

# CLI (öğeler kuyruğa yazılır; OSIRIS_REDIS_URL)
osiris collect web-scraper --config '{"url":"https://example.com"}'

# API (öğeler kuyruğa yazılır)
curl -sk -X POST https://<sunucu>:8443/api/collect -H "X-API-Key: <anahtar>" \
  -H 'Content-Type: application/json' \
  -d '{"plugin_id":"web-scraper","config":{"url":"https://example.com"}}'
```

Kod: `crates/osiris-plugins/src/web_scraper.rs`; senaryolar `fixtures/plugins/web_scraper_*.json`.
