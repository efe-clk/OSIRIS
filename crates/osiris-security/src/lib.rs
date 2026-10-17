//! OSIRIS güvenlik çekirdeği — `osiris/security.py` (SSRF + sanitizer) ile birebir.
//!
//! - SSRF: şema allowlist (http/https), URL içi kimlik yasağı, özel/loopback/link-local/
//!   metadata IP engeli (IPv4-mapped IPv6 açılarak), **fail-closed DNS** (çözülemeyen ya
//!   da kayıtlarından biri iç ağda olan host reddedilir).
//! - DNS çözümü [`Resolver`] ile enjekte edilir: Python köprüsü `security._host_ips`'i
//!   geçirir (testler onu taklit eder), Rust servisleri [`SystemResolver`] kullanır.
//!   Bağlantı DÖNEN IP'lere yapılmalıdır (DNS rebinding).
//! - Hata iletileri Python ile aynıdır (Türkçe, kullanıcıya gösterilir).

pub mod ip;
pub mod urlsplit;

use std::sync::LazyLock;

pub use ip::PyIp;

/// Denetim hatası (Python'da `ValueError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityError(pub String);

impl std::fmt::Display for SecurityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SecurityError {}

fn err(msg: impl Into<String>) -> SecurityError {
    SecurityError(msg.into())
}

/// Host adını IP dizelerine çözer (v4 + v6). Çözülemezse boş liste.
pub trait Resolver {
    type Error;
    fn resolve(&self, host: &str) -> Result<Vec<String>, Self::Error>;
}

/// Sistem çözücüsü (`getaddrinfo`); hata = boş liste (Python `_host_ips` gibi).
pub struct SystemResolver;

impl Resolver for SystemResolver {
    type Error = std::convert::Infallible;

    fn resolve(&self, host: &str) -> Result<Vec<String>, Self::Error> {
        use std::net::ToSocketAddrs;
        let mut out: Vec<String> = Vec::new();
        if let Ok(addrs) = (host, 0u16).to_socket_addrs() {
            for a in addrs {
                let s = match a {
                    std::net::SocketAddr::V4(v4) => v4.ip().to_string(),
                    std::net::SocketAddr::V6(v6) if v6.scope_id() != 0 => {
                        format!("{}%{}", v6.ip(), v6.scope_id())
                    }
                    std::net::SocketAddr::V6(v6) => v6.ip().to_string(),
                };
                if !out.contains(&s) {
                    out.push(s);
                }
            }
        }
        Ok(out)
    }
}

/// Çözücü hatası ya da denetim hatası.
#[derive(Debug)]
pub enum CheckError<E> {
    Denied(SecurityError),
    Resolver(E),
}

impl<E> From<SecurityError> for CheckError<E> {
    fn from(e: SecurityError) -> Self {
        CheckError::Denied(e)
    }
}

/// `resolve_safe_ips`: host'u çözer, yalnızca dış IP'leri döner; aksi halde hata.
pub fn resolve_safe_ips<R: Resolver>(
    host: &str,
    resolver: &R,
) -> Result<Vec<String>, CheckError<R::Error>> {
    if host.is_empty() {
        return Err(err("Host boş").into());
    }
    let h = py_strip(host).to_lowercase();
    let h = h.trim_end_matches('.');
    if h == "localhost" {
        return Err(err(format!("İç ağ/özel host engellendi: {h}")).into());
    }
    let ips = match PyIp::parse(h) {
        Some(ip) => vec![ip.to_py_string()],
        None => resolver.resolve(h).map_err(CheckError::Resolver)?,
    };
    if ips.is_empty() {
        return Err(err(format!("Host çözülemedi: {h}")).into());
    }
    for ip_str in &ips {
        let addr = ip_str.split_once('%').map_or(ip_str.as_str(), |(a, _)| a);
        let Some(ip) = PyIp::parse(addr) else {
            return Err(err(format!("Geçersiz adres: {ip_str}")).into());
        };
        if ip.is_blocked() {
            return Err(err(format!("İç ağ/özel host engellendi: {h}")).into());
        }
    }
    Ok(ips)
}

/// `is_blocked_host`: çözülemeyen host'lar da engellidir (fail-closed).
pub fn is_blocked_host<R: Resolver>(host: &str, resolver: &R) -> Result<bool, R::Error> {
    match resolve_safe_ips(host, resolver) {
        Ok(_) => Ok(false),
        Err(CheckError::Denied(_)) => Ok(true),
        Err(CheckError::Resolver(e)) => Err(e),
    }
}

pub const MAX_URL_CHARS: usize = 2048;
pub const MAX_PORT: u32 = 65535;

/// İstemci (urllib3) ile `urlsplit` farklı bölebileceği karakterler: ters eğik çizgi
/// urllib3'te yetki bölümünü bitirir, `urlsplit`'te bitirmez (E14); kontrol karakterleri
/// `urlsplit`'te silinir, istemcide silinmez.
fn has_ambiguous_chars(url: &str) -> bool {
    url.chars().any(|c| c == '\\' || c < ' ' || c == '\u{7f}')
}

/// `assert_safe_url`: geçerliyse özgün URL'yi döner.
pub fn assert_safe_url<R: Resolver>(
    url: &str,
    allow_onion: bool,
    resolver: &R,
) -> Result<String, CheckError<R::Error>> {
    if url.is_empty() || url.chars().count() > MAX_URL_CHARS {
        return Err(err("URL boş veya çok uzun").into());
    }
    let stripped = py_strip(url);
    if has_ambiguous_chars(stripped) {
        return Err(err("URL geçersiz karakter içeriyor").into());
    }
    let parsed = urlsplit::urlsplit(stripped)?;
    if parsed.scheme != "http" && parsed.scheme != "https" {
        return Err(err(format!("İzin verilmeyen şema: {}", py_repr(&parsed.scheme))).into());
    }
    let truthy = |v: Option<&str>| v.is_some_and(|s| !s.is_empty());
    if truthy(parsed.username()) || truthy(parsed.password()) {
        return Err(err("URL içinde kimlik bilgisi yasak").into());
    }
    let host = parsed.hostname().unwrap_or_default().to_lowercase();
    if host.is_empty() {
        return Err(err("URL host içermiyor").into());
    }
    if host.ends_with(".onion") {
        if !allow_onion {
            return Err(err(".onion adresleri yalnızca Tor plugin'i ile kullanılabilir").into());
        }
        return Ok(url.to_owned());
    }
    if let Some(port) = parsed.port()?
        && !(1..=MAX_PORT).contains(&port)
    {
        return Err(err("Geçersiz port").into());
    }
    if is_blocked_host(&host, resolver).map_err(CheckError::Resolver)? {
        return Err(err(format!("İç ağ/özel host engellendi: {host}")).into());
    }
    Ok(url.to_owned())
}

// ------------------------------------------------------------------------- sanitizer'lar

/// Python `repr(str)` — hata iletileri için. Yazdırılabilir karakterler olduğu gibi kalır;
/// tırnak seçimi ve `\\`, `\n`, `\r`, `\t`, C0/DEL kaçışları Python ile aynıdır.
pub fn py_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python `str.isspace()`.
pub fn py_is_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}'
            | '\u{1c}'..='\u{20}'
            | '\u{85}'
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

/// Python `str.strip()`.
pub fn py_strip(s: &str) -> &str {
    s.trim_matches(py_is_space)
}

/// Python `str.isalnum()` = `[\p{L}\p{N}]` (her karakter için).
fn py_isalnum_or_dash(label: &str) -> bool {
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^[\p{L}\p{N}-]*$").expect("etiket deseni"));
    RE.is_match(label)
}

/// IRC nick/kanal enjeksiyonuna karşı (CRLF, boşluk, ayraçlar).
pub fn sanitize_irc_token(value: &str, max_len: usize) -> Result<&str, SecurityError> {
    if value.is_empty() {
        return Err(err("Boş IRC değeri"));
    }
    if value.chars().count() > max_len {
        return Err(err("IRC değeri çok uzun"));
    }
    if value.contains(['\r', '\n', '\0', ' ', ',', '*']) {
        return Err(err("IRC değerinde geçersiz karakter"));
    }
    Ok(value)
}

pub fn sanitize_channel(value: &str) -> Result<&str, SecurityError> {
    if value.is_empty() || value.chars().count() > 64 {
        return Err(err("Geçersiz kanal"));
    }
    if value.contains(['\r', '\n', '\0', ' ', ',']) {
        return Err(err("Kanalda geçersiz karakter"));
    }
    if !value.starts_with(['#', '&']) {
        return Err(err("Kanal # veya & ile başlamalı"));
    }
    Ok(value)
}

/// DNS/WHOIS için alan adı doğrulama; küçük harfli, sondaki noktasız biçimi döner.
pub fn sanitize_domain(domain: &str) -> Result<String, SecurityError> {
    if domain.is_empty() || domain.chars().count() > 253 {
        return Err(err("Geçersiz domain"));
    }
    let d = py_strip(domain).to_lowercase();
    let d = d.trim_end_matches('.');
    if d.contains(['\r', '\n', '\0', ' ', '/', '@', ':']) {
        return Err(err("Domain geçersiz karakter içeriyor"));
    }
    if !d.contains('.') && d != "localhost" {
        return Err(err("Domain TLD içermeli"));
    }
    for label in d.split('.') {
        if label.is_empty() || label.chars().count() > 63 {
            return Err(err("Domain etiketi geçersiz"));
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(err("Domain etiketi tire ile başlayamaz/bitmez"));
        }
        if !py_isalnum_or_dash(label) {
            return Err(err("Domain etiketi geçersiz karakter içeriyor"));
        }
    }
    Ok(d.to_owned())
}

pub fn sanitize_hostname(host: &str, max_len: usize) -> Result<String, SecurityError> {
    if host.is_empty() || host.chars().count() > max_len {
        return Err(err("Geçersiz host"));
    }
    let h = py_strip(host).to_lowercase();
    if h.contains(['\r', '\n', '\0', ' ', '/']) {
        return Err(err("Host geçersiz karakter içeriyor"));
    }
    Ok(h)
}

/// Sırları maskeler: son `keep` karakter görünür, kısa/boş değer `***`.
pub fn mask_secret(value: Option<&str>, keep: usize) -> String {
    let Some(s) = value.filter(|s| !s.is_empty()) else { return "***".to_owned() };
    let n = s.chars().count();
    if n <= keep {
        return "***".to_owned();
    }
    // Python s[-0:] tüm dizedir
    let tail: String = if keep == 0 { s.to_owned() } else { s.chars().skip(n - keep).collect() };
    format!("{}{tail}", "*".repeat(n - keep))
}
