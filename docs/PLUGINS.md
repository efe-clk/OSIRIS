> **Türkçe:** [PLUGINS.tr.md](PLUGINS.tr.md)

# OSIRIS Plugin Reference

OSIRIS's data sources are 11 plugins, all compiled into the `osiris-server` binary. This document
describes what each plugin does, its configuration fields, the items it produces and its limits.
The same schema is available on a running system via `GET /plugins` (the web UI draws its forms
from it). Plugin descriptions and error messages are in Turkish.

To write a new plugin: [CONTRIBUTING.md](CONTRIBUTING.md#4-adding-a-new-plugin).

## Common rules

- **Required fields.** If a field marked `required: true` in the manifest is empty (`null` or
  `""`), the plugin is not called at all; the result is the error `Zorunlu alanlar eksik: <fields>`
  ("required fields missing").
- **Network access.** Plugins do not open sockets; every HTTP, DNS and TCP request passes the
  host's SSRF check (private/internal addresses, unresolvable hosts and credentials in URLs are
  rejected; every redirect is re-checked, at most 3 redirects). Details:
  [ARCHITECTURE.md §6](ARCHITECTURE.md#6-plugin-host-model).
- **User-Agent.** `OSIRIS-OSINT/0.1 (+self-hosted)`.
- **Item fields.** Every item carries `url`, `title`, `raw_content`, `published_at`, `metadata`,
  `tags`. The pipeline cleans `raw_content`, deduplicates it and extracts its entities.
- **Failure vs crash.** A network or input error returns `success=false` with an error message;
  in the API this becomes `502`. Anything unexpected is reported as `plugin hatası: …` ("plugin
  error"); the service is unaffected.
- **`url` from the source.** When a plugin runs through a source and `config` has no `url`,
  `feed_url` or `endpoint`, the source's `url` is copied there.

## Summary

| Plugin | Network | Default cron | Collects | Items |
|--------|---------|--------------|----------|-------|
| [`rss`](#rss) | rss | `*/15 * * * *` | RSS/Atom entries | ≤ 200 (`max_items`) |
| [`web-scraper`](#web-scraper) | www | `0 */6 * * *` | Text of one web page (optionally via a CSS selector) | ≤ 20 |
| [`rest-api`](#rest-api) | api | `0 * * * *` | Records of a JSON API | ≤ 200 |
| [`shodan`](#shodan) | api | `0 */6 * * *` | Shodan host search results | ≤ 100 |
| [`dns-whois`](#dns-whois) | api | `0 */12 * * *` | DNS records + WHOIS text | ≤ 100 |
| [`blockchain`](#blockchain) | blockchain | `*/10 * * * *` | Address balance/transaction count or block number | 1 |
| [`username-search`](#username-search) | www | — | Sites where a username has a profile | number of profiles found |
| [`tor`](#tor) | tor | `0 */12 * * *` | Text of one `.onion` page | 1 |
| [`i2p`](#i2p) | i2p | `0 */12 * * *` | Text of one eepsite page | 1 |
| [`irc`](#irc) | irc | `*/5 * * * *` | Channel messages (20 s of listening) | ≤ 200 |
| [`matrix`](#matrix) | matrix | `*/5 * * * *` | Latest messages of a room | ≤ 100 |

---

## rss

Reads RSS 2.0 and Atom feeds.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `feed_url` | string | **required** | Feed address |
| `max_items` | integer | 50 | Maximum entries to take (1–200) |

- At most 2 MB of the response is read; 30 s timeout.
- Each entry is an item: `url` = entry link, `title`, `raw_content` = summary/content (≤ 50 000),
  `published_at` = publication date, `metadata.feed`.
- Unparseable feed: `Geçersiz feed` ("invalid feed").

```json
{"plugin_id": "rss", "config": {"feed_url": "https://example.com/feed.xml", "max_items": 20}}
```

## web-scraper

Downloads a web page and extracts its text.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `url` | string | **required** | Page address |
| `css_selector` | string | — | Text of matching elements only (each match a separate item, at most 20) |
| `depth` | integer | 1 | Only written to `metadata.depth`; links are not followed |
| `follow_links` | boolean | false | Currently has no effect (link following is not implemented) |

- Response at most 2 MB, 30 s timeout; item text ≤ 50 000 characters.
- Without a selector, or with an invalid one, the whole page becomes one item. Supported selectors:
  type, `#id`, `.class`, attribute operators, combinators, `:nth-*`, `:not`, `:is`.
- `title` = the page's `<title>`; matches with empty text are skipped.

```json
{"plugin_id": "web-scraper", "config": {"url": "https://example.com/announcements", "css_selector": "article"}}
```

## rest-api

Calls a JSON API and turns the returned records into items.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `endpoint` | string | **required** | API address |
| `method` | string | `GET` | `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, `OPTIONS` |
| `headers` | object | `{}` | Request headers (at most 20; key ≤ 200, value ≤ 2000 characters) |
| `params` | object | `{}` | Query parameters (same limits) |
| `json_path` | string | — | Dotted path to the record array, e.g. `data.items` (≤ 500) |

- 30 s timeout. A non-JSON response → `Geçersiz JSON yanıtı`; a `json_path` that is not found →
  `json_path geçersiz`.
- If the result is an array, each element is an item (at most 200); otherwise a single item.
- `raw_content` = the record's **canonical JSON** (sorted keys, ≤ 50 000); `title` = the record's
  `title` field (if any); `url` = `endpoint`.
- Secrets inside a stored source's `headers`/`params` (`Authorization`, `api_key` …) are masked on read.

```json
{"plugin_id": "rest-api", "config": {"endpoint": "https://api.example.com/v1/events",
  "json_path": "data.items", "headers": {"Authorization": "Bearer <token>"}}}
```

## shodan

Calls Shodan's host search API (`https://api.shodan.io/shodan/host/search`).

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `api_key` | string | **required** | Shodan API key |
| `query` | string | **required** | Shodan query (≤ 500), e.g. `port:22 country:TR` |

- The first 100 matches; each is an item: `url` = `http://<ip>:<port>`, `title` = product name,
  `raw_content` = the match's canonical JSON, `metadata` = `ip`, `port`.
- `401` → `Shodan: yetkisiz (API anahtarı geçersiz)` (unauthorised, invalid key), `429` →
  `Shodan: hız limiti aşıldı` (rate limited). The API key is never written to errors or metadata.

## dns-whois

Passive reconnaissance: DNS records and IANA WHOIS.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `domain` | string | **required** | Domain name |
| `record_types` | array | `["A","AAAA","MX","NS","TXT"]` | At most 10 types; allowed: `A`, `AAAA`, `MX`, `NS`, `TXT`, `CNAME`, `SOA`, `PTR`, `SRV` (others are skipped; `A` if none remain) |

- Up to 50 answers per record type, each answer an item (`metadata.record_type`); then the WHOIS
  text from `whois.iana.org:43` as a separate item (`record_type: "WHOIS"`, ≤ 20 000 characters).
  At most 100 items in total.
- Non-existent domain → `Domain bulunamadı`. Types without an answer are skipped silently.

## blockchain

Two modes: watch an address, or fetch the chain's latest block number.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `rpc_url` | string | **required** | EVM JSON-RPC address (Ethereum etc.). Not used for Bitcoin, but it is a required field, so fill it in |
| `address` | string | — | Address to watch (20–100 letters/digits) |
| `chain` | string | `ethereum` | `bitcoin` → Blockstream; any other value is treated as EVM |

- **With an address (EVM):** `eth_getBalance` + `eth_getTransactionCount`; item text
  `address=… balance_wei=… txcount=…`, tag `crypto`. No item if the address is not a 42-character
  `0x` address.
- **With an address (`chain: bitcoin`):** `https://blockstream.info/api/address/<address>`; item
  text `address=… balance_sat=… txcount=…`.
- **Without an address:** `eth_blockNumber`; item text `block_number=…`.
- `rpc_url` also passes the SSRF check; nodes on the internal network cannot be reached.

## username-search

Maigret-style username scan: probes profile URLs on sites and returns every site where a profile
was found as an item. The web UI's *Username scan* screen uses this plugin.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `username` | string | **required** | 2–64 characters; letters, digits, `_`, `.`, `-` (first character a letter, digit or `_`) |
| `full` | boolean | false | `false`: quick DB (59 sites); `true`: full DB (~2 500 sites, Maigret + WhatsMyName) |
| `sites` | array | — | Only these site names |
| `tags` | array | — | Only these categories: `dev`, `social`, `photo`, `video`, `music`, `gaming`, `forum`, `blog` |
| `max_sites` | integer | 200 | Maximum sites to probe (1–3 000) |
| `workers` | integer | 10 | Concurrent requests (1–20) |
| `timeout` | integer | 15 | Per-site timeout, s (5–30) |
| `rate_per_sec` | number | 10 | Total requests/s across all sites (0 = unlimited; at most 50) |
| `per_host_interval_ms` | integer | 1000 | Minimum gap between two requests to the same host (0–60 000) |
| `sites_db` | string | — | Custom site DB (Maigret `data.json` compatible); only the embedded DB names or `.json` files under `OSIRIS_USERNAME_DB_DIR` |

- Supported check types: `status_code`, `message` (`presenceStrs`/`absenceStrs`),
  `response_url`. Unknown types are skipped; an unreachable site does not stop the scan.
- Item: `url` = profile URL, `title` = `<user> @ <site>`, tags `username` + the site's categories,
  `metadata` = `site`, `username`, `check`. Result metadata: `checked` (probed), `found`,
  `unknown` (no profile found or could not be probed).
- Duration: never shorter than `max_sites / rate_per_sec` seconds (200 sites at 10 req/s → at least
  20 s). To stay within Nginx's 60 s timeout, lower `max_sites` for large scans.
- Details and updating the site DB: [plugins/username-search/README.md](../plugins/username-search/README.md) (Turkish).

## tor

Downloads a `.onion` page through Tor.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `url` | string | **required** | Address containing `.onion` |
| `socks5_host`, `socks5_port` | string, integer | — | Leave empty (see below) |

- Proxy selection: if `OSIRIS_TOR_SOCKS` is set (Compose: `tor:9050`) it is **always** used.
  Otherwise only a loopback proxy (`127.0.0.1`/`::1`, default `127.0.0.1:9050`) is accepted; any
  other host is rejected. The connection is SOCKS5h (DNS is also resolved inside Tor).
- 60 s timeout, response ≤ 2 MB; the whole page text is one item (≤ 50 000), `metadata.network: "tor"`.
- Requires the `darknet` profile: `docker compose --profile darknet up -d`.

## i2p

Downloads an eepsite through an I2P HTTP proxy.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `url` | string | **required** | `.i2p` address |
| `http_proxy_host`, `http_proxy_port` | string, integer | — | Leave empty |

- Proxy selection follows the same rule as `tor`: `OSIRIS_I2P_HTTP_PROXY` (Compose: `i2pd:4444`),
  otherwise loopback only (default port 4444). `.i2p` names are not looked up in DNS; the proxy
  resolves them.
- 60 s timeout, response ≤ 2 MB; one item, `metadata.network: "i2p"`.
- Joining the I2P network can take a while; wait a few minutes after i2pd starts.

## irc

Connects to an IRC server, joins channels and collects messages for 20 seconds.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `server` | string | **required** | Server name (a valid host name) |
| `channels` | array | **required** | At most 10 channels, e.g. `["#osint"]` |
| `port` | integer | 6667 | 1–65535 (plain TCP; no TLS) |
| `nickname` | string | `osiris-bot` | ≤ 64; sanitised against IRC command injection |

- After the SSRF check the connection goes to the verified IP (15 s). `PING`s are answered.
- Only `PRIVMSG` lines become items (raw line, ≤ 2000), `metadata` = `server`, `protocol`. At most
  200 items; listening ends early after 5 s of silence.

## matrix

Reads the latest messages of a Matrix room with the client-server API v3.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `homeserver` | string | **required** | E.g. `https://matrix.example.org` |
| `room_id` | string | **required** | E.g. `!abc123:example.org` (≤ 500) |
| `access_token` | string | — | Access token for private rooms (masked in stored sources) |

- `GET /_matrix/client/v3/rooms/<room>/messages?dir=b&limit=50`; of the first 100 events only
  `m.room.message` bodies become items (≤ 20 000), `metadata` = `room`, `sender`.
- `401`/`403` → `Matrix: yetkisiz` (unauthorised).
