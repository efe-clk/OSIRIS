//! Host DNS ve ham TCP erişimi (dns-whois, irc).

use async_trait::async_trait;
use std::time::Duration;

/// DNS sorgu hatası (dnspython istisnalarının karşılığı).
#[derive(Debug, Clone, thiserror::Error)]
pub enum DnsError {
    /// `dns.resolver.NoAnswer` — ad var, bu türde kayıt yok.
    #[error("The DNS response does not contain an answer to the question")]
    NoAnswer,
    /// `dns.resolver.NXDOMAIN`
    #[error("The DNS query name does not exist")]
    NxDomain,
    /// Diğer `dns.exception.DNSException` (zaman aşımı, SERVFAIL…).
    #[error("{0}")]
    Other(String),
}

/// Kayıtları dnspython `str(rdata)` biçiminde döndüren çözücü.
#[async_trait]
pub trait Dns: Send + Sync {
    /// `domain` mutlak sorgulanır (arama listesi yok). `nameservers` boşsa sistem ayarı.
    async fn resolve(
        &self,
        domain: &str,
        rtype: &str,
        nameservers: &[String],
        lifetime: Duration,
    ) -> Result<Vec<String>, DnsError>;
}

/// `recv` sonucu hatası.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RecvError {
    #[error("timed out")]
    Timeout,
    #[error("{0}")]
    Io(String),
}

/// Açık TCP bağlantısı (Python soket nesnesi karşılığı).
#[async_trait]
pub trait Connection: Send {
    async fn send_all(&mut self, data: &[u8]) -> Result<(), String>;
    /// En fazla `n` bayt; bağlantı kapandıysa boş vektör. `timeout` aşılırsa `Timeout`.
    async fn recv(&mut self, n: usize, timeout: Duration) -> Result<Vec<u8>, RecvError>;
}

/// `socket.create_connection((host, port), timeout)`
#[async_trait]
pub trait Tcp: Send + Sync {
    async fn connect(
        &self,
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> Result<Box<dyn Connection>, String>;
}

/// dnspython `ipv6.inet_ntoa` (en uzun sıfır dizisi `::`; `::a.b.c.d` / `::ffff:a.b.c.d`).
pub fn dnspython_ipv6(addr: std::net::Ipv6Addr) -> String {
    let chunks: Vec<String> = addr.segments().iter().map(|s| format!("{s:x}")).collect();
    let (mut best_start, mut best_len) = (0usize, 0usize);
    let mut start = 0usize;
    let mut last_zero = false;
    for (i, c) in chunks.iter().enumerate() {
        if c != "0" {
            if last_zero {
                if i - start > best_len {
                    best_start = start;
                    best_len = i - start;
                }
                last_zero = false;
            }
        } else if !last_zero {
            start = i;
            last_zero = true;
        }
    }
    if last_zero && 8 - start > best_len {
        best_start = start;
        best_len = 8 - start;
    }
    if best_len > 1 {
        if best_start == 0 && (best_len == 6 || (best_len == 5 && chunks[5] == "ffff")) {
            let o = addr.octets();
            let prefix = if best_len == 6 { "::" } else { "::ffff:" };
            return format!("{prefix}{}.{}.{}.{}", o[12], o[13], o[14], o[15]);
        }
        return format!(
            "{}::{}",
            chunks[..best_start].join(":"),
            chunks[best_start + best_len..].join(":")
        );
    }
    chunks.join(":")
}

/// dnspython `_escapify` + tırnak: TXT parçası.
pub fn dnspython_txt(parts: &[Vec<u8>]) -> String {
    parts
        .iter()
        .map(|p| {
            let mut s = String::from("\"");
            for &c in p {
                if c == b'"' || c == b'\\' {
                    s.push('\\');
                    s.push(c as char);
                } else if (0x20..0x7f).contains(&c) {
                    s.push(c as char);
                } else {
                    s.push_str(&format!("\\{c:03}"));
                }
            }
            s.push('"');
            s
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dnspython_bicimleri() {
        let f = |s: &str| dnspython_ipv6(s.parse().unwrap());
        assert_eq!(f("2606:2800:220:1:248:1893:25c8:1946"), "2606:2800:220:1:248:1893:25c8:1946");
        assert_eq!(f("2001:db8::1"), "2001:db8::1");
        assert_eq!(f("::1"), "::1");
        assert_eq!(f("::"), "::");
        assert_eq!(f("::ffff:1.2.3.4"), "::ffff:1.2.3.4");
        assert_eq!(f("1:0:0:2:0:0:0:3"), "1:0:0:2::3");
        assert_eq!(
            dnspython_txt(&[b"v=spf1 \"x\"".to_vec(), vec![0x01, b'a']]),
            r#""v=spf1 \"x\"" "\001a""#
        );
    }
}
