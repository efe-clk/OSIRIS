# OSIRIS — WASM Plugin Tasarım Notu (K6)

> Rust geçişi **R5** · Durum: **yalnızca tasarım** (uygulama sonraki bir faza bırakıldı) ·
> Plan: [RUST_GECIS_PLANI.md](RUST_GECIS_PLANI.md) · Sözleşme: [SOZLESME.md](SOZLESME.md)

## 1. Amaç ve kapsam

K6 kararı: plugin'ler bugün **gömülü** (`crates/osiris-plugins`, derleme zamanında bağlanır).
Üçüncü taraf ya da operatöre özel plugin'ler için ileride **WASM** (WebAssembly Component
Model) desteği eklenecek. Bu not, R5'te kurulan `Collector` trait'inin WASM'a nasıl
taşınacağını ve hangi güvenlik sınırlarının değişmeyeceğini sabitler.

Kapsam dışı: Python plugin'lerini çalıştırmak, plugin pazarı/imza altyapısı.

## 2. Değişmeyecek ilkeler (R5'ten devralınır)

1. **Plugin ağa doğrudan erişmez.** WASM modülüne soket, dosya sistemi, ortam değişkeni ya
   da saat verilmez (WASI yetenekleri kapalı). Tüm G/Ç host fonksiyonlarıyla yapılır.
2. **HTTP yalnızca host'un `fetch`'i** (`osiris_plugin::http::fetch`): her yönlendirmede SSRF
   denetimi, en fazla 3 yönlendirme, zaman aşımı, gövde sınırı, `.onion`/`.i2p` yalnızca
   manifestte izin verilen plugin'e. Proxy (Tor/I2P) operatörün belirlediği adrestir.
3. **Ham TCP yalnızca `safe_connect`** (çöz → denetle → doğrulanmış IP'ye bağlan).
4. **Sonuç sözleşmesi aynı:** `CollectionResult` / `CollectedItem` JSON'u
   (`schemas/collection_result.schema.json`); host her sonucu şemaya karşı doğrular,
   geçersiz sonuç = plugin hatası.
5. **Çökme izolasyonu:** WASM tuzağı (trap), yakıt (fuel) ya da bellek sınırı aşımı →
   `plugin hatası: …`; collector düşmez.

## 3. Arayüz (WIT taslağı)

```wit
package osiris:plugin@0.1.0;

interface host {
    record http-request {
        method: string, url: string,
        headers: list<tuple<string, string>>, params: list<tuple<string, string>>,
        json-body: option<string>, timeout-ms: u32, max-bytes: u32,
    }
    record http-response {
        status: u16, headers: list<tuple<string, string>>, body: list<u8>,
        final-url: string, encoding: option<string>,
    }
    variant http-error { unsafe-url(string), transport(string), too-many-redirects }
    fetch: func(req: http-request) -> result<http-response, http-error>;

    variant dns-error { no-answer, nx-domain, other(string) }
    dns-resolve: func(domain: string, rtype: string) -> result<list<string>, dns-error>;

    resource connection {
        send: func(data: list<u8>) -> result<_, string>;
        recv: func(max: u32, timeout-ms: u32) -> result<list<u8>, string>;
    }
    safe-connect: func(host: string, port: u16, timeout-ms: u32) -> result<connection, string>;

    log: func(level: u8, msg: string);
}

world collector {
    import host;
    /// manifest.json içeriği (host şemayla doğrular)
    export manifest: func() -> string;
    /// config: JSON nesnesi; dönüş: CollectionResult JSON'u
    export collect: func(config: string) -> string;
    export health-check: func(config: string) -> bool;
}
```

JSON dizeleri bilinçli seçildi: şema `schemas/` altında tek kaynaktır ve dil bağımsızdır.

## 4. Çalışma zamanı

- **wasmtime** (Component Model, WASI yetenekleri kapalı), modül başına ayrı `Store`.
  Sınırlar: yakıt (CPU), bellek (ör. 64 MB), toplam süre (manifestte `timeout_s`, üst sınır
  120 sn).
- Host fonksiyonları `osiris-plugin`'deki mevcut `Host`'a köprülenir: aynı `fetch`, `Dns`,
  `safe_connect`. Böylece fixture oynatıcısı (`replay`) WASM plugin'lerini de sınayabilir —
  her WASM plugin'i `fixtures/plugins/` biçiminde senaryo getirmek zorundadır.
- Plugin kaydı: `plugins/<id>/plugin.wasm` + `manifest.json`; manifestte `"runtime": "wasm"`
  ve `"capabilities"` (ör. `["http", "onion"]`). Host yalnızca bildirilen yetenekleri bağlar
  (bildirilmeyen `safe-connect` → bağlama hatası).
- Bütünlük: `plugin.wasm` için SHA-256 manifestte; isteğe bağlı imza (minisign) sonraki faz.

## 5. Geçiş yolu

1. `Collector` trait'ine `WasmCollector` uygulaması (host fonksiyonları + JSON köprüsü).
2. Bir gömülü plugin'in (ör. `blockchain`) WASM'a derlenmiş kopyasının aynı fixture'ları
   geçtiğini gösteren test.
3. `manifest.schema.json`'a `runtime`, `capabilities` doğrulaması (geriye uyumlu: alan yoksa
   `embedded`).
4. Operatör belgesi: plugin ekleme, yetenek onayı, kaldırma.

## 6. Açık sorular (uygulama fazında karar)

- Component Model mi, çekirdek modül + elle ABI mi? (Öneri: Component Model, WIT yukarıda.)
- Plugin başına eşzamanlılık (username-search gibi çok istekli plugin'lerde host tarafında
  tempo/semafor zorunlu tutulmalı).
- Yakıt/bellek sınırlarının manifestle mi, operatör ayarıyla mı belirleneceği.
