"""Paylaşılan güvenlik yardımcıları.

SSRF koruması, URL/host doğrulama ve gizli veri maskeleme.
Tüm collector plugin'leri ve API bu modülü kullanmalıdır.
"""

from __future__ import annotations

import ipaddress
import os
import socket
from urllib.parse import urlparse

# RFC1918 + loopback + link-local + multicast + CGNAT + dokümantasyon blokları
_BLOCKED_NETWORKS = [
    ipaddress.ip_network("127.0.0.0/8"),
    ipaddress.ip_network("10.0.0.0/8"),
    ipaddress.ip_network("172.16.0.0/12"),
    ipaddress.ip_network("192.168.0.0/16"),
    ipaddress.ip_network("169.254.0.0/16"),  # link-local (cloud metadata dahil)
    ipaddress.ip_network("0.0.0.0/8"),
    ipaddress.ip_network("100.64.0.0/10"),  # CGNAT
    ipaddress.ip_network("192.0.2.0/24"),  # TEST-NET-1
    ipaddress.ip_network("198.51.100.0/24"),  # TEST-NET-2
    ipaddress.ip_network("203.0.113.0/24"),  # TEST-NET-3
    ipaddress.ip_network("224.0.0.0/4"),  # multicast
    ipaddress.ip_network("::1/128"),
    ipaddress.ip_network("fc00::/7"),  # unique local
    ipaddress.ip_network("fe80::/10"),  # link-local v6
]

_ALLOWED_SCHEMES = {"http", "https"}

# İzin verilen port aralığı (ayrıcalıklı portlara gidişi kısıtla)
_MAX_PORT = 65535


def _host_ips(host: str) -> list[str]:
    """Host adını IP listesine çözer (v4+v6). Çözülemezse boş liste döner."""
    try:
        infos = socket.getaddrinfo(host, None, family=socket.AF_UNSPEC,
                                   type=socket.SOCK_STREAM)
    except OSError:
        return []
    ips = []
    for info in infos:
        sockaddr = info[4]
        if sockaddr and sockaddr[0] not in ips:
            ips.append(sockaddr[0])
    return ips


def _ip_blocked(ip: ipaddress.IPv4Address | ipaddress.IPv6Address) -> bool:
    """IP engelli bloklardan birinde mi? IPv4-mapped IPv6 (::ffff:a.b.c.d) açılarak denetlenir."""
    if ip.is_unspecified:  # 0.0.0.0 / :: → yerel makineye gider
        return True
    if isinstance(ip, ipaddress.IPv6Address) and ip.ipv4_mapped is not None:
        ip = ip.ipv4_mapped
    return any(ip in net for net in _BLOCKED_NETWORKS)


def resolve_safe_ips(host: str) -> list[str]:
    """Host'u çözer ve yalnızca dış (izinli) IP'leri döner; aksi halde ValueError.

    Kayıtlardan biri bile iç ağdaysa host tümden reddedilir; çözülemeyen host da
    reddedilir (fail-closed). Ham soket açan plugin'ler bağlantıyı DÖNEN IP'lere
    yapmalıdır — host adıyla yeniden bağlanmak DNS rebinding'e açık kapı bırakır.
    """
    if not host:
        raise ValueError("Host boş")
    h = host.strip().lower().rstrip(".")
    if h in ("localhost",):
        raise ValueError(f"İç ağ/özel host engellendi: {h}")
    try:
        ips = [str(ipaddress.ip_address(h))]
    except ValueError:
        ips = _host_ips(h)
    if not ips:
        raise ValueError(f"Host çözülemedi: {h}")
    for ip_str in ips:
        try:
            ip = ipaddress.ip_address(ip_str.split("%", 1)[0])  # IPv6 kapsam kimliği
        except ValueError as exc:
            raise ValueError(f"Geçersiz adres: {ip_str}") from exc
        if _ip_blocked(ip):
            raise ValueError(f"İç ağ/özel host engellendi: {h}")
    return ips


def is_blocked_host(host: str) -> bool:
    """Özel/iç ağ host'u mu? Çözülemeyen host'lar da engellenir (fail-closed)."""
    try:
        resolve_safe_ips(host)
    except ValueError:
        return True
    return False


def safe_create_connection(host: str, port: int, timeout: float = 15) -> socket.socket:
    """SSRF denetimli TCP bağlantısı (IRC/ham soket plugin'leri için).

    Host bir kez çözülür, tüm kayıtlar denetlenir ve bağlantı doğrulanmış IP'ye
    açılır. İç ağ veya çözülemeyen host için ValueError, ağ hatası için OSError.
    """
    if not isinstance(port, int) or not 1 <= port <= _MAX_PORT:
        raise ValueError("Geçersiz port")
    last_exc: OSError | None = None
    for ip_str in resolve_safe_ips(host):
        try:
            return socket.create_connection((ip_str, port), timeout=timeout)
        except OSError as exc:
            last_exc = exc
    raise last_exc or OSError(f"Bağlanılamadı: {host}")


def _ambiguous_url_chars(url: str) -> bool:
    """İstemci (urllib3) ile ``urlparse``'ın farklı böldüğü karakterler (E14).

    Ters eğik çizgi urllib3'te yetki bölümünü bitirir, ``urlparse``'ta bitirmez:
    ``http://127.0.0.1\\x.saldirgan.example/`` denetimde saldırganın alan adı gibi
    görünür, istek ise 127.0.0.1'e gider. Kontrol karakterleri ``urlparse``'ta silinir,
    istemcide silinmez.
    """
    return any(c == "\\" or c < " " or c == "\x7f" for c in url)


def assert_safe_url(url: str, *, allow_onion: bool = False) -> str:
    """URL'yi SSRF'e karşı doğrular. Geçersizse ValueError yükseltir.

    - http/https zorunlu (onion için allow_onion ile .onion host'una izin)
    - kullanıcı bilgisi (user:pass@) yasak
    - ters eğik çizgi / kontrol karakteri yasak (ayrıştırıcı farkı, E14)
    - özel/iç IP ve localhost yasak
    """
    if not url or len(url) > 2048:
        raise ValueError("URL boş veya çok uzun")
    if _ambiguous_url_chars(url.strip()):
        raise ValueError("URL geçersiz karakter içeriyor")
    parsed = urlparse(url.strip())
    if parsed.scheme not in _ALLOWED_SCHEMES:
        raise ValueError(f"İzin verilmeyen şema: {parsed.scheme!r}")
    if parsed.username or parsed.password:
        raise ValueError("URL içinde kimlik bilgisi yasak")
    host = (parsed.hostname or "").lower()
    if not host:
        raise ValueError("URL host içermiyor")
    if host.endswith(".onion"):
        if not allow_onion:
            raise ValueError(".onion adresleri yalnızca Tor plugin'i ile kullanılabilir")
        return url
    if parsed.port is not None and not (1 <= parsed.port <= _MAX_PORT):
        raise ValueError("Geçersiz port")
    if is_blocked_host(host):
        raise ValueError(f"İç ağ/özel host engellendi: {host}")
    return url


def sanitize_irc_token(value: str, max_len: int = 64) -> str:
    """IRC nick/kanal enjeksiyonuna karşı temizler (CRLF + boşluk kontrolü)."""
    if not value:
        raise ValueError("Boş IRC değeri")
    if len(value) > max_len:
        raise ValueError("IRC değeri çok uzun")
    if any(c in value for c in ("\r", "\n", "\0", " ", ",", "*")):
        raise ValueError("IRC değerinde geçersiz karakter")
    if not value.startswith("#") and value.startswith(("#", "&")) is False and max_len == 64:
        # nick için ek kontrol: harf/rakam/-/_/[/]/{} ile başlamalı
        pass
    return value


def sanitize_channel(value: str) -> str:
    if not value or len(value) > 64:
        raise ValueError("Geçersiz kanal")
    if any(c in value for c in ("\r", "\n", "\0", " ", ",")):
        raise ValueError("Kanalda geçersiz karakter")
    if not value.startswith(("#", "&")):
        raise ValueError("Kanal # veya & ile başlamalı")
    return value


def sanitize_domain(domain: str) -> str:
    """DNS/WHOIS için domain doğrulama (CRLF enjeksiyonunu engeller)."""
    if not domain or len(domain) > 253:
        raise ValueError("Geçersiz domain")
    d = domain.strip().lower().rstrip(".")
    if any(c in d for c in ("\r", "\n", "\0", " ", "/", "@", ":")):
        raise ValueError("Domain geçersiz karakter içeriyor")
    if "." not in d and d != "localhost":
        raise ValueError("Domain TLD içermeli")
    labels = d.split(".")
    for label in labels:
        if not label or len(label) > 63:
            raise ValueError("Domain etiketi geçersiz")
        if label.startswith("-") or label.endswith("-"):
            raise ValueError("Domain etiketi tire ile başlayamaz/bitmez")
        if not all(c.isalnum() or c == "-" for c in label):
            raise ValueError("Domain etiketi geçersiz karakter içeriyor")
    return d


def sanitize_hostname(host: str, max_len: int = 253) -> str:
    if not host or len(host) > max_len:
        raise ValueError("Geçersiz host")
    h = host.strip().lower()
    if any(c in h for c in ("\r", "\n", "\0", " ", "/")):
        raise ValueError("Host geçersiz karakter içeriyor")
    return h


def safe_title(soup) -> str | None:
    """BeautifulSoup title çıkarımını güvenli yapar (None-string crash fix)."""
    try:
        if soup is None or soup.title is None:
            return None
        text = soup.title.get_text(" ", strip=True)
        return text[:500] if text else None
    except Exception:
        return None


def cap_list(value: list, limit: int) -> list:
    return value[:limit] if isinstance(value, list) else []


def mask_secret(value: str | None, keep: int = 4) -> str:
    if not value:
        return "***"
    s = str(value)
    if len(s) <= keep:
        return "***"
    return "*" * (len(s) - keep) + s[-keep:]


_REDIRECT_STATUSES = frozenset({301, 302, 303, 307, 308})


def fetch_url(session, method: str, url: str, *, allow_onion: bool = False,
              timeout: int = 30, max_redirects: int = 3,
              max_bytes: int = 2_000_000, **kwargs):
    """Yönlendirme-denetimli HTTP istemi (redirect-SSRF korumalı).

    `requests` otomatik yönlendirme takibini KAPATIR; her `Location`
    adımını `assert_safe_url` ile yeniden doğrular. Gövde `max_bytes`
    ile sınırlanır. `session` çağrıcının oturumudur (Tor/I2P proxy'leri
    korunur). Dönen: tam okunmuş `requests.Response` (raise_for_status/
    json()/text kullanılabilir).
    """
    from urllib.parse import urljoin

    if max_redirects < 0 or max_redirects > 10:
        raise ValueError("max_redirects 0-10 aralığında olmalı")
    current = assert_safe_url(url, allow_onion=allow_onion)
    hops = 0
    while True:
        resp = session.request(method, current, allow_redirects=False,
                               timeout=timeout, stream=True, **kwargs)
        location = resp.headers.get("Location") if resp is not None else None
        if resp.status_code in _REDIRECT_STATUSES and location and hops < max_redirects:
            hops += 1
            resp.close()
            nxt = urljoin(current, location.strip())
            current = assert_safe_url(nxt, allow_onion=allow_onion)
            # Tarayıcı kuralı: 303 her zaman GET'e döner; 301/302 POST'u GET yapar
            if resp.status_code == 303 or (
                resp.status_code in (301, 302) and method.upper() == "POST"
            ):
                method = "GET"
            continue
        if resp.status_code in _REDIRECT_STATUSES and location:
            resp.close()
            raise ValueError("Çok fazla yönlendirme")
        # Gövdeyi bütçeyle oku (stream) ve bağlantıyı bırak
        data = b""
        try:
            for chunk in resp.iter_content(65536):
                if not chunk:
                    continue
                data += chunk
                if len(data) >= max_bytes:
                    data = data[:max_bytes]
                    break
        finally:
            resp.close()
        try:
            resp._content = data  # noqa: SLF001 — Response'u tamamlama
        except AttributeError:
            pass
        resp.url = current
        return resp


_LOCAL_PROXY_HOSTS = frozenset({"127.0.0.1", "::1"})


def resolve_proxy(cfg_host: object, cfg_port: object, env_var: str,
                  default_port: int, label: str) -> tuple[str, int]:
    """Tor/I2P proxy adresini seçer; kullanıcı config'i rastgele host'a yönlendiremez.

    - Operatör ``env_var``'ı (``host:port``) tanımladıysa HER ZAMAN o kullanılır
      (Docker'da proxy ayrı bir konteynerdedir). Config yalnızca boş, localhost
      veya aynı host olabilir; başka bir host → ValueError.
    - Tanımlı değilse eski davranış: host yalnızca localhost, port config'ten.
    """
    env = os.getenv(env_var, "").strip()
    host_in = "" if cfg_host in (None, "") else str(cfg_host).strip()
    if env:
        env_host, sep, env_port_s = env.rpartition(":")
        try:
            env_port = int(env_port_s)
        except ValueError as exc:
            raise ValueError(f"{env_var} 'host:port' biçiminde olmalı") from exc
        if not sep or not env_host or not 1 <= env_port <= _MAX_PORT:
            raise ValueError(f"{env_var} 'host:port' biçiminde olmalı")
        env_host = env_host.strip("[]")
        if host_in and host_in not in _LOCAL_PROXY_HOSTS and host_in != env_host:
            raise ValueError(f"{label} proxy host'u yalnızca operatörün belirlediği "
                             f"{env_var} olabilir")
        return env_host, env_port
    host = host_in or "127.0.0.1"
    if host not in _LOCAL_PROXY_HOSTS:
        raise ValueError(f"{label} proxy host yalnızca localhost olabilir")
    try:
        port = int(cfg_port) if cfg_port not in (None, "") else default_port
    except (TypeError, ValueError) as exc:
        raise ValueError(f"Geçersiz {label} proxy portu") from exc
    if not 1 <= port <= _MAX_PORT:
        raise ValueError(f"Geçersiz {label} proxy portu")
    return host, port
