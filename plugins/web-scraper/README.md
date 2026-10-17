# Web Scraper Plugin

Genel amaçlı web sayfası toplama plugin'i.

## Yapılandırma

| Alan | Tip | Zorunlu | Varsayılan | Açıklama |
|------|-----|---------|-----------|----------|
| `url` | string | evet | — | Hedef URL |
| `depth` | integer | hayır | 1 | Tarama derinliği |
| `follow_links` | boolean | hayır | false | Bağlantıları takip et |
| `css_selector` | string | hayır | — | Belirli bir CSS seçicisini çek |

## Kullanım

```bash
# Kuru çalıştırma (kuyruğa yazmaz; sonuç JSON olarak basılır)
osiris-server collect --plugin web-scraper --config '{"url": "https://example.com", "css_selector": "article"}'

# CLI (öğeler kuyruğa yazılır; OSIRIS_REDIS_URL)
osiris collect web-scraper --config '{"url": "https://example.com"}'

# API (öğeler kuyruğa yazılır)
curl -sk -X POST https://<sunucu>:8443/api/collect -H "X-API-Key: $OSIRIS_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"plugin_id": "web-scraper", "config": {"url": "https://example.com"}}'
```

Kod: `crates/osiris-plugins/src/web_scraper.rs`; senaryolar `fixtures/plugins/web_scraper_*.json`.
