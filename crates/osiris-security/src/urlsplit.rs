//! CPython 3.11.16 `urllib.parse.urlsplit` + `SplitResult.hostname/port/username/password`.
//!
//! WHATWG (`url` crate) bilerek kullanılmaz: Python tarafındaki HTTP istemcisi (requests/
//! urllib3) URL'yi RFC 3986 tarzı böler; denetim ile bağlantı aynı host'u görmelidir.
//! Ayrıştırıcı farkı riskli girdiler (ters eğik çizgi, kontrol karakteri) [`crate`]
//! düzeyinde ayrıca reddedilir.

use crate::SecurityError;
use crate::ip::PyIp;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitResult {
    pub scheme: String,
    pub netloc: String,
    pub path: String,
    pub query: String,
    pub fragment: String,
}

const SCHEME_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+-.";

fn err(msg: impl Into<String>) -> SecurityError {
    SecurityError(msg.into())
}

/// `urlsplit(url)` (scheme='', allow_fragments=True).
pub fn urlsplit(url: &str) -> Result<SplitResult, SecurityError> {
    // _WHATWG_C0_CONTROL_OR_SPACE baştan; \t \r \n her yerden silinir
    let url: String = url
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    let mut rest: &str = &url;
    let mut scheme = String::new();
    if let Some(i) = rest.find(':') {
        let first = rest.chars().next();
        if i > 0
            && first.is_some_and(|c| c.is_ascii_alphabetic())
            && rest[..i].chars().all(|c| SCHEME_CHARS.contains(c))
        {
            scheme = rest[..i].to_ascii_lowercase();
            rest = &rest[i + 1..];
        }
    }
    let mut netloc = String::new();
    if rest.starts_with("//") {
        let after = &rest[2..];
        let delim = after.find(['/', '?', '#']).unwrap_or(after.len());
        netloc = after[..delim].to_owned();
        rest = &after[delim..];
        let (open, close) = (netloc.contains('['), netloc.contains(']'));
        if open != close {
            return Err(err("Invalid IPv6 URL"));
        }
        if open && close {
            check_bracketed_netloc(&netloc)?;
        }
    }
    let (rest, fragment) = match rest.split_once('#') {
        Some((a, b)) => (a, b.to_owned()),
        None => (rest, String::new()),
    };
    let (path, query) = match rest.split_once('?') {
        Some((a, b)) => (a.to_owned(), b.to_owned()),
        None => (rest.to_owned(), String::new()),
    };
    check_netloc(&netloc)?;
    Ok(SplitResult { scheme, netloc, path, query, fragment })
}

fn check_bracketed_netloc(netloc: &str) -> Result<(), SecurityError> {
    let hostname_and_port = netloc.rsplit_once('@').map_or(netloc, |(_, h)| h);
    let hostname = match hostname_and_port.split_once('[') {
        Some((before, bracketed)) => {
            if !before.is_empty() {
                return Err(err("Invalid IPv6 URL"));
            }
            let (hostname, port) = bracketed.split_once(']').unwrap_or((bracketed, ""));
            if !port.is_empty() && !port.starts_with(':') {
                return Err(err("Invalid IPv6 URL"));
            }
            hostname
        }
        None => hostname_and_port.split_once(':').map_or(hostname_and_port, |(h, _)| h),
    };
    if let Some(rest) = hostname.strip_prefix('v') {
        // \Av[a-fA-F0-9]+\..+\Z
        let ok = rest.split_once('.').is_some_and(|(hex, tail)| {
            !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()) && !tail.is_empty()
        });
        if !ok {
            return Err(err("IPvFuture address is invalid"));
        }
        return Ok(());
    }
    match PyIp::parse(hostname) {
        Some(PyIp::V6(..)) => Ok(()),
        Some(PyIp::V4(_)) => Err(err("An IPv4 address cannot be in brackets")),
        None => Err(err(format!(
            "{} does not appear to be an IPv4 or IPv6 address",
            crate::py_repr(hostname)
        ))),
    }
}

/// `_checknetloc`: NFKC altında '/?#@:' üreten karakterler (ör. U+2100 "a/c") reddedilir.
fn check_netloc(netloc: &str) -> Result<(), SecurityError> {
    if netloc.is_empty() || netloc.is_ascii() {
        return Ok(());
    }
    let n: String = netloc.chars().filter(|c| !matches!(c, '@' | ':' | '#' | '?')).collect();
    let normalized: String = n.nfkc().collect();
    if n == normalized {
        return Ok(());
    }
    if normalized.contains(['/', '?', '#', '@', ':']) {
        return Err(err(format!(
            "netloc {} contains invalid characters under NFKC normalization",
            crate::py_repr(netloc)
        )));
    }
    Ok(())
}

impl SplitResult {
    fn userinfo(&self) -> (Option<&str>, Option<&str>) {
        match self.netloc.rsplit_once('@') {
            Some((info, _)) => match info.split_once(':') {
                Some((u, p)) => (Some(u), Some(p)),
                None => (Some(info), None),
            },
            None => (None, None),
        }
    }

    pub fn username(&self) -> Option<&str> {
        self.userinfo().0
    }

    pub fn password(&self) -> Option<&str> {
        self.userinfo().1
    }

    fn hostinfo(&self) -> (&str, Option<&str>) {
        let hostinfo = self.netloc.rsplit_once('@').map_or(self.netloc.as_str(), |(_, h)| h);
        let (hostname, port) = match hostinfo.split_once('[') {
            Some((_, bracketed)) => {
                let (h, after) = bracketed.split_once(']').unwrap_or((bracketed, ""));
                (h, after.split_once(':').map_or("", |(_, p)| p))
            }
            None => hostinfo.split_once(':').unwrap_or((hostinfo, "")),
        };
        (hostname, if port.is_empty() { None } else { Some(port) })
    }

    /// `hostname`: boşsa None; kapsam kimliğinden (`%`) önceki kısım küçük harfe iner.
    pub fn hostname(&self) -> Option<String> {
        let (h, _) = self.hostinfo();
        if h.is_empty() {
            return None;
        }
        Some(match h.split_once('%') {
            Some((a, z)) => format!("{}%{z}", a.to_lowercase()),
            None => h.to_lowercase(),
        })
    }

    /// `port`: yoksa None; ASCII rakam değilse ya da 0–65535 dışındaysa hata.
    pub fn port(&self) -> Result<Option<u32>, SecurityError> {
        let Some(p) = self.hostinfo().1 else { return Ok(None) };
        if !p.bytes().all(|b| b.is_ascii_digit()) {
            return Err(err(format!(
                "Port could not be cast to integer value as {}",
                crate::py_repr(p)
            )));
        }
        let trimmed = p.trim_start_matches('0');
        if trimmed.len() > 5 {
            return Err(err("Port out of range 0-65535"));
        }
        let v: u32 = if trimmed.is_empty() { 0 } else { trimmed.parse().unwrap_or(u32::MAX) };
        if v > 65535 {
            return Err(err("Port out of range 0-65535"));
        }
        Ok(Some(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_ile_ayni() {
        let r = urlsplit("HTTP://User:Pw@Example.COM:8080/a/b?q=1#f").unwrap();
        assert_eq!(r.scheme, "http");
        assert_eq!(r.netloc, "User:Pw@Example.COM:8080");
        assert_eq!(r.path, "/a/b");
        assert_eq!(r.query, "q=1");
        assert_eq!(r.fragment, "f");
        assert_eq!(r.username(), Some("User"));
        assert_eq!(r.password(), Some("Pw"));
        assert_eq!(r.hostname().as_deref(), Some("example.com"));
        assert_eq!(r.port().unwrap(), Some(8080));
        assert_eq!(urlsplit("http://[::1]:80/").unwrap().hostname().as_deref(), Some("::1"));
        assert!(urlsplit("http://[127.0.0.1]/").is_err());
        assert!(urlsplit("http://[::1/").is_err());
        assert!(urlsplit("http://a\u{2100}b/").is_err());
        assert!(urlsplit("http://x:99999/").unwrap().port().is_err());
    }
}
