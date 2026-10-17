//! Host HTTP erişimi — Python `security.fetch_url` + requests `Response` semantiği.
//!
//! Plugin'ler ağa doğrudan erişmez: [`fetch`] her adımda SSRF denetimi yapar
//! (`osiris_security::assert_safe_url`), yönlendirmeleri elle izler (en fazla
//! `max_redirects`, 303 → GET, 301/302 + POST → GET), gövdeyi `max_bytes` ile keser.
//! Tek istek [`Transport`] ile gider: gerçek ağ (`real::ReqwestTransport`) ya da
//! fixture oynatıcısı (`replay`).

use crate::Host;
use async_trait::async_trait;
use osiris_hash::Json;
use osiris_security::UrlPolicy;
use std::time::Duration;

/// Proxy (operatörün belirlediği Tor/I2P).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Proxy {
    /// `socks5h://host:port` — host adını proxy çözer (.onion).
    Socks5h { host: String, port: u16 },
    /// `http://host:port`
    Http { host: String, port: u16 },
}

impl Proxy {
    pub fn url(&self) -> String {
        let fmt = |scheme: &str, host: &str, port: u16| {
            let h = if host.contains(':') { format!("[{host}]") } else { host.to_owned() };
            format!("{scheme}://{h}:{port}")
        };
        match self {
            Proxy::Socks5h { host, port } => fmt("socks5h", host, *port),
            Proxy::Http { host, port } => fmt("http", host, *port),
        }
    }
}

/// Tek HTTP isteği (yönlendirme izlenmez).
#[derive(Debug, Clone)]
pub struct WireRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// requests `params=` (sorgu dizesine eklenir).
    pub params: Vec<(String, String)>,
    /// requests `json=`
    pub json: Option<serde_json::Value>,
    pub timeout: Duration,
    pub proxy: Option<Proxy>,
    /// Okunacak en fazla gövde baytı.
    pub max_bytes: usize,
}

/// Tek HTTP yanıtı.
#[derive(Debug, Clone, Default)]
pub struct WireResponse {
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// requests `Response.encoding` (başlıktan; fixture'da açık değer).
    pub encoding: Option<String>,
}

/// İstemci/ağ hatası (Python `requests.RequestException`).
#[derive(Debug, Clone, thiserror::Error)]
#[error("{kind}: {message}")]
pub struct TransportError {
    /// `ConnectionError`, `Timeout`, `SSLError`, …
    pub kind: String,
    pub message: String,
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, req: WireRequest) -> Result<WireResponse, TransportError>;
}

/// `fetch` hatası: Python'da `ValueError` (güvensiz URL, çok fazla yönlendirme, geçersiz
/// JSON) ya da `RequestException` (ağ, HTTP durum kodu).
#[derive(Debug, Clone, thiserror::Error)]
pub enum HttpError {
    #[error("{0}")]
    Value(String),
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error("{status} {class} Error: {reason} for url: {url}")]
    Status { status: u16, class: &'static str, reason: String, url: String },
}

impl HttpError {
    /// Python `requests.RequestException` mı (ValueError değil)?
    pub fn is_request_exception(&self) -> bool {
        !matches!(self, HttpError::Value(_))
    }

    /// `exc.response.status_code` (yalnızca durum hatasında).
    pub fn status(&self) -> Option<u16> {
        match self {
            HttpError::Status { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// `type(exc).__name__`
    pub fn type_name(&self) -> &str {
        match self {
            HttpError::Value(_) => "ValueError",
            HttpError::Transport(t) => &t.kind,
            HttpError::Status { .. } => "HTTPError",
        }
    }
}

/// `fetch_url` seçenekleri (varsayılanlar Python ile aynı).
#[derive(Debug, Clone)]
pub struct FetchOptions {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub params: Vec<(String, String)>,
    pub json: Option<serde_json::Value>,
    pub timeout: Duration,
    pub max_redirects: u32,
    pub max_bytes: usize,
    pub policy: UrlPolicy,
    pub proxy: Option<Proxy>,
}

impl FetchOptions {
    pub fn new(method: &str, url: &str) -> Self {
        Self {
            method: method.to_owned(),
            url: url.to_owned(),
            headers: Vec::new(),
            params: Vec::new(),
            json: None,
            timeout: Duration::from_secs(30),
            max_redirects: 3,
            max_bytes: 2_000_000,
            policy: UrlPolicy::default(),
            proxy: None,
        }
    }

    pub fn header(mut self, k: &str, v: &str) -> Self {
        self.headers.push((k.to_owned(), v.to_owned()));
        self
    }

    pub fn timeout_secs(mut self, s: u64) -> Self {
        self.timeout = Duration::from_secs(s);
        self
    }

    pub fn max_bytes(mut self, n: usize) -> Self {
        self.max_bytes = n;
        self
    }
}

const REDIRECTS: [u16; 5] = [301, 302, 303, 307, 308];

/// Python `fetch_url`.
pub async fn fetch(host: &Host, opts: FetchOptions) -> Result<Response, HttpError> {
    if opts.max_redirects > 10 {
        return Err(HttpError::Value("max_redirects 0-10 aralığında olmalı".into()));
    }
    let mut method = opts.method.clone();
    let mut current = host.assert_safe_url(&opts.url, opts.policy).await?;
    let mut hops = 0;
    loop {
        let req = WireRequest {
            method: method.clone(),
            url: current.clone(),
            headers: opts.headers.clone(),
            params: opts.params.clone(),
            json: opts.json.clone(),
            timeout: opts.timeout,
            proxy: opts.proxy.clone(),
            max_bytes: opts.max_bytes,
        };
        let resp = host.http.send(req).await?;
        let location = header(&resp.headers, "location").map(str::to_owned);
        if REDIRECTS.contains(&resp.status) && location.is_some() && hops < opts.max_redirects {
            hops += 1;
            let loc = location.unwrap_or_default();
            let nxt = osiris_security::urlsplit::urljoin(&current, osiris_hash::py_strip(&loc))
                .map_err(|e| HttpError::Value(e.0))?;
            current = host.assert_safe_url(&nxt, opts.policy).await?;
            if resp.status == 303
                || ((resp.status == 301 || resp.status == 302)
                    && method.eq_ignore_ascii_case("POST"))
            {
                method = "GET".into();
            }
            continue;
        }
        if REDIRECTS.contains(&resp.status) && location.is_some() {
            return Err(HttpError::Value("Çok fazla yönlendirme".into()));
        }
        let mut body = resp.body;
        body.truncate(opts.max_bytes);
        return Ok(Response {
            status: resp.status,
            reason: resp.reason,
            headers: resp.headers,
            body,
            encoding: resp.encoding,
            url: current,
        });
    }
}

/// Büyük/küçük harf duyarsız ilk başlık değeri.
pub fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

/// requests `get_encoding_from_headers`.
pub fn encoding_from_headers(headers: &[(String, String)]) -> Option<String> {
    let ct = header(headers, "content-type")?;
    let mut tokens = ct.split(';');
    let content_type = tokens.next().unwrap_or("").trim();
    let strip = |s: &str| s.trim_matches(|c| c == '"' || c == '\'' || c == ' ').to_owned();
    for param in tokens {
        let param = param.trim();
        if param.is_empty() {
            continue;
        }
        if let Some(idx) = param.find('=')
            && strip(&param[..idx]).to_lowercase() == "charset"
        {
            return Some(
                strip(&param[idx + 1..]).trim_matches(|c| c == '\'' || c == '"').to_owned(),
            );
        }
    }
    if content_type.contains("text") {
        return Some("ISO-8859-1".into());
    }
    if content_type.contains("application/json") {
        return Some("utf-8".into());
    }
    None
}

/// Python codec ile çözme (`errors="replace"`). Bilinmeyen etiket → UTF-8 (requests gibi);
/// kodlama yoksa (charset_normalizer tahmini) → geçerliyse UTF-8, değilse windows-1252.
pub fn decode(body: &[u8], encoding: Option<&str>) -> String {
    let utf8 = || String::from_utf8_lossy(body).into_owned();
    let Some(label) = encoding else {
        return match std::str::from_utf8(body) {
            Ok(s) => s.to_owned(),
            Err(_) => encoding_rs::WINDOWS_1252.decode_without_bom_handling(body).0.into_owned(),
        };
    };
    let l = label.trim().to_ascii_lowercase().replace('_', "-");
    if matches!(
        l.as_str(),
        "iso-8859-1" | "iso8859-1" | "latin-1" | "latin1" | "l1" | "8859" | "cp819" | "iso-ir-100"
    ) {
        return body.iter().map(|&b| char::from(b)).collect();
    }
    if matches!(l.as_str(), "utf-8" | "utf8" | "u8") {
        return utf8();
    }
    match encoding_rs::Encoding::for_label(l.as_bytes()) {
        Some(enc) => enc.decode_without_bom_handling(body).0.into_owned(),
        None => utf8(),
    }
}

/// Tamamlanmış yanıt (requests `Response` karşılığı).
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub encoding: Option<String>,
    /// Son URL (yönlendirmelerden sonra).
    pub url: String,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }

    /// `resp.text`
    pub fn text(&self) -> String {
        decode(&self.body, self.encoding.as_deref())
    }

    /// `resp.json()` — Python `json.loads` semantiği; hata `ValueError`.
    pub fn json(&self) -> Result<Json, HttpError> {
        Json::parse_py(&self.text()).map_err(|e| HttpError::Value(e.0))
    }

    /// `resp.raise_for_status()`
    pub fn raise_for_status(&self) -> Result<(), HttpError> {
        let class = match self.status {
            400..=499 => "Client",
            500..=599 => "Server",
            _ => return Ok(()),
        };
        Err(HttpError::Status {
            status: self.status,
            class,
            reason: self.reason.clone(),
            url: self.url.clone(),
        })
    }
}

/// Python `urllib.parse.quote_plus` / requests `params` kodlaması: harf, rakam ve `_.-~`
/// korunur, boşluk `+`, diğer UTF-8 baytları `%XX` (büyük harf).
pub fn quote_plus(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Python `urllib.parse.quote(s, safe="")`.
pub fn quote_all(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// requests `prepare_url` sorgu ekleme: `url?k=v&…` (mevcut sorgunun sonuna `&` ile).
pub fn url_with_params(url: &str, params: &[(String, String)]) -> String {
    if params.is_empty() {
        return url.to_owned();
    }
    let enc = params
        .iter()
        .map(|(k, v)| format!("{}={}", quote_plus(k), quote_plus(v)))
        .collect::<Vec<_>>()
        .join("&");
    let (base, frag) = match url.split_once('#') {
        Some((b, f)) => (b, Some(f)),
        None => (url, None),
    };
    let mut out =
        if base.contains('?') { format!("{base}&{enc}") } else { format!("{base}?{enc}") };
    if let Some(f) = frag {
        out.push('#');
        out.push_str(f);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(ct: &str) -> Vec<(String, String)> {
        vec![("Content-Type".into(), ct.into())]
    }

    #[test]
    fn requests_kodlama_kurallari() {
        assert_eq!(
            encoding_from_headers(&h("text/html; charset=\"UTF-8\"")).as_deref(),
            Some("UTF-8")
        );
        assert_eq!(encoding_from_headers(&h("text/html")).as_deref(), Some("ISO-8859-1"));
        assert_eq!(encoding_from_headers(&h("Text/HTML")), None);
        assert_eq!(encoding_from_headers(&h("application/json")).as_deref(), Some("utf-8"));
        assert_eq!(encoding_from_headers(&h("application/octet-stream")), None);
        assert_eq!(decode(&[0x80, 0xe9], Some("ISO-8859-1")), "\u{80}é");
        assert_eq!(decode("ş".as_bytes(), Some("utf-8")), "ş");
        assert_eq!(quote_plus("a b/ç~"), "a+b%2F%C3%A7~");
        assert_eq!(
            url_with_params("https://x/a?q=1#f", &[("k".into(), "v w".into())]),
            "https://x/a?q=1&k=v+w#f"
        );
    }
}
