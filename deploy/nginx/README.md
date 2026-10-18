# OSIRIS Nginx

`nginx.conf`, Compose'daki `nginx` servisinin (`nginx:1.31-alpine`) yapılandırmasıdır: TLS'i
sonlandırır, istekleri servislere yönlendirir, güvenlik başlıklarını ekler ve IP başına hız
sınırı uygular.

*English summary: TLS termination, routing (`/api/`, `/ui/`, `/n8n/`), security headers and
per-IP rate limiting. TLS listens on 8443; generate a self-signed certificate with
`make-certs.sh`, or mount a real one and map `443:8443`.*

## Dinleyiciler ve yönlendirme

| Port | Ne |
|------|----|
| `80` | Düz HTTP — aynı içerik (LAN ya da ilk kurulum için) |
| `8443` | TLS 1.2/1.3 — üretim erişimi |

| Yol | Hedef | Not |
|-----|-------|-----|
| `/api/` | `osiris-api:8000` | `/api` öneki sıyrılır (`/api/search` → `/search`); 10 istek/sn/IP, 20 taşma; `proxy_read_timeout 60s` |
| `/ui/` | `osiris-web-ui:3000` | `/` → `/ui/` (302), `/ui` → `/ui/` (301) |
| `/n8n/` | `n8n:5678` | |

Gövde tavanı `client_max_body_size 5m`'dir (API'nin sınırıyla aynı).

## Güvenlik başlıkları

`X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`,
`Strict-Transport-Security: max-age=31536000; includeSubDomains` (yalnızca HTTPS yanıtlarında
etkilidir) ve:

```
Content-Security-Policy: default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline';
                         object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'
```

`style-src 'unsafe-inline'` web arayüzünün satır içi `style` öznitelikleri (graf renkleri,
çubuklar) içindir; betikler yalnızca aynı kökenden yüklenir. API'nin `/api/docs` (Swagger UI)
sayfası da bu politikaya uyar: dosyaları API ikilisine gömülüdür ve aynı kökenden gelir
(bkz. [docs/API.tr.md](../../docs/API.tr.md#1-temeller)).

## Sertifika

İlk kurulumda öz-imzalı bir sertifika üretin; sertifika olmadan Nginx açılmaz:

```bash
./deploy/nginx/make-certs.sh
# deploy/nginx/certs/osiris.local.crt ve .key — CN=osiris.local,
# SAN: osiris.local, localhost, 127.0.0.1; 825 gün; anahtar 600 izinli
```

Betik var olan sertifikanın üzerine yazmaz. Sertifika dosyaları git dışıdır (`*.crt`, `*.key`
`.gitignore`'dadır). Yenilemek için dosyaları silip betiği yeniden çalıştırın, sonra:

```bash
docker compose up -d --force-recreate --no-deps nginx
```

## Gerçek alan adı ve 443

TLS varsayılan olarak `8443`'tedir; amaç `443`'ü kullanan başka bir ters proxy ile (ör. aynı
sunucudaki başka bir site) çakışmamaktır. Kendi alan adınızla `443`'te yayın yapmak için:

1. Sertifikayı (ör. Let's Encrypt) `deploy/nginx/certs/osiris.local.crt` ve `.key` adlarıyla
   koyun ya da `nginx.conf`'taki `ssl_certificate` yollarını değiştirin.
2. `nginx.conf`'ta `server_name _;` satırını alan adınızla değiştirin.
3. `docker-compose.yml`'de `"8443:8443"` port eşlemesini `"443:8443"` yapın; `443`'te başka bir
   servis olmadığından emin olun.
4. `docker compose up -d --force-recreate --no-deps nginx`.

Önde başka bir ters proxy (Traefik, Caddy, Cloudflare Tunnel) varsa TLS'i orada sonlandırıp bu
Nginx'e `:80`'den gelmek de mümkündür. Bu durumda o katman da `/api/` için hız sınırı ve gövde
tavanı uygulamalıdır.

## API yeniden oluşturulduktan sonra

Nginx upstream adreslerini açılışta çözer. `osiris-api` ya da `osiris-web-ui` konteyneri yeniden
oluşturulduğunda IP'si değişebilir ve Nginx `502` döner. Çözüm:

```bash
docker compose restart nginx
```
