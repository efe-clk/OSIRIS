//! IP adresi ayrıştırma/biçimleme (Python `ipaddress` 3.11 ile aynı) ve engelli bloklar.

use std::net::{Ipv4Addr, Ipv6Addr};

/// Python `ipaddress.ip_address(s)` sonucu (IPv6 kapsam kimliği korunur).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PyIp {
    V4(Ipv4Addr),
    V6(Ipv6Addr, Option<String>),
}

impl PyIp {
    /// Python `ipaddress.ip_address(s)`; geçersizse None.
    pub fn parse(s: &str) -> Option<PyIp> {
        if let Some(v4) = parse_v4(s) {
            return Some(PyIp::V4(v4));
        }
        let (addr, scope) = match s.split_once('%') {
            None => (s, None),
            Some((a, z)) if !z.is_empty() && !z.contains('%') => (a, Some(z.to_owned())),
            Some(_) => return None,
        };
        parse_v6(addr).map(|v6| PyIp::V6(v6, scope))
    }

    /// Python `str(ip)`.
    pub fn to_py_string(&self) -> String {
        match self {
            PyIp::V4(v4) => v4.to_string(),
            PyIp::V6(v6, scope) => {
                // 3.11.16: IPv4-mapped "::ffff:a.b.c.d" biçiminde (_ipv4_mapped_ipv6_to_str)
                let mut s = match ipv4_mapped(v6) {
                    Some(v4) => format!("::ffff:{v4}"),
                    None => compress_v6(v6),
                };
                if let Some(z) = scope {
                    s.push('%');
                    s.push_str(z);
                }
                s
            }
        }
    }

    /// `security._ip_blocked`: belirtilmemiş adres, IPv4-mapped IPv6 açılarak engelli bloklar.
    pub fn is_blocked(&self) -> bool {
        match self {
            PyIp::V4(v4) => v4_blocked(*v4),
            PyIp::V6(v6, _) => {
                if v6.is_unspecified() {
                    return true;
                }
                if let Some(v4) = ipv4_mapped(v6) {
                    return v4_blocked(v4);
                }
                v6_blocked(*v6)
            }
        }
    }
}

/// IPv4 dörtlü: ASCII rakam, 1–3 hane, baştaki sıfır yasak ("0" hariç), ≤ 255.
fn parse_v4(s: &str) -> Option<Ipv4Addr> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut octets = [0u8; 4];
    for (o, p) in octets.iter_mut().zip(&parts) {
        if p.is_empty() || p.len() > 3 || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if p.len() > 1 && p.starts_with('0') {
            return None;
        }
        *o = p.parse::<u16>().ok().filter(|v| *v <= 255)? as u8;
    }
    Some(Ipv4Addr::from(octets))
}

/// Python `IPv6Address._ip_int_from_string` kuralları.
fn parse_v6(s: &str) -> Option<Ipv6Addr> {
    if s.is_empty() || s.chars().count() > 45 {
        return None;
    }
    let mut parts: Vec<String> = s.split(':').map(str::to_owned).collect();
    if parts.len() < 3 {
        return None;
    }
    if parts.last().is_some_and(|p| p.contains('.')) {
        let v4 = parse_v4(&parts.pop()?)?;
        let n = u32::from(v4);
        parts.push(format!("{:x}", n >> 16));
        parts.push(format!("{:x}", n & 0xffff));
    }
    if parts.len() > 9 {
        return None;
    }
    // '::' en fazla bir kez (iç boş parça)
    let inner_empty: Vec<usize> = (1..parts.len() - 1).filter(|&i| parts[i].is_empty()).collect();
    let (hi, lo, skipped);
    if let Some(&skip) = inner_empty.first() {
        if inner_empty.len() > 1 {
            return None;
        }
        let mut h = skip;
        let mut l = parts.len() - skip - 1;
        if parts[0].is_empty() {
            h -= 1;
            if h != 0 {
                return None; // ':' ile başlayamaz ('::' değilse)
            }
        }
        if parts[parts.len() - 1].is_empty() {
            l -= 1;
            if l != 0 {
                return None;
            }
        }
        let sk = 8i32 - (h + l) as i32;
        if sk < 1 {
            return None;
        }
        hi = h;
        lo = l;
        skipped = sk as usize;
    } else {
        if parts.len() != 8 || parts[0].is_empty() || parts[7].is_empty() {
            return None;
        }
        hi = 8;
        lo = 0;
        skipped = 0;
    }
    let mut segs = [0u16; 8];
    let mut idx = 0;
    for p in &parts[..hi] {
        segs[idx] = parse_hextet(p)?;
        idx += 1;
    }
    idx += skipped;
    for p in &parts[parts.len() - lo..] {
        segs[idx] = parse_hextet(p)?;
        idx += 1;
    }
    Some(Ipv6Addr::from(segs))
}

fn parse_hextet(p: &str) -> Option<u16> {
    if p.is_empty() || p.len() > 4 || !p.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u16::from_str_radix(p, 16).ok()
}

/// Python 3.11 `_BaseV6._string_from_ip_int` + `_compress_hextets`.
fn compress_v6(v6: &Ipv6Addr) -> String {
    let mut hextets: Vec<String> = v6.segments().iter().map(|h| format!("{h:x}")).collect();
    let (mut best_start, mut best_len) = (0usize, 0usize);
    let (mut cur_start, mut cur_len) = (None::<usize>, 0usize);
    for (i, h) in hextets.iter().enumerate() {
        if h == "0" {
            cur_len += 1;
            let start = *cur_start.get_or_insert(i);
            if cur_len > best_len {
                best_len = cur_len;
                best_start = start;
            }
        } else {
            cur_len = 0;
            cur_start = None;
        }
    }
    if best_len > 1 {
        let best_end = best_start + best_len;
        if best_end == hextets.len() {
            hextets.push(String::new());
        }
        hextets.splice(best_start..best_end, [String::new()]);
        if best_start == 0 {
            hextets.insert(0, String::new());
        }
    }
    hextets.join(":")
}

/// `IPv6Address.ipv4_mapped` — yalnızca `::ffff:a.b.c.d`.
fn ipv4_mapped(v6: &Ipv6Addr) -> Option<Ipv4Addr> {
    let s = v6.segments();
    if s[..5].iter().all(|&x| x == 0) && s[5] == 0xffff {
        Some(Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8))
    } else {
        None
    }
}

/// RFC1918 + loopback + link-local (bulut metadata dahil) + CGNAT + dokümantasyon + multicast.
const BLOCKED_V4: &[([u8; 4], u8)] = &[
    ([127, 0, 0, 0], 8),
    ([10, 0, 0, 0], 8),
    ([172, 16, 0, 0], 12),
    ([192, 168, 0, 0], 16),
    ([169, 254, 0, 0], 16),
    ([0, 0, 0, 0], 8),
    ([100, 64, 0, 0], 10),
    ([192, 0, 2, 0], 24),
    ([198, 51, 100, 0], 24),
    ([203, 0, 113, 0], 24),
    ([224, 0, 0, 0], 4),
];

/// `::1/128`, `fc00::/7` (unique local), `fe80::/10` (link-local).
const BLOCKED_V6: &[(u128, u8)] = &[(1, 128), (0xfc00 << 112, 7), (0xfe80 << 112, 10)];

fn v4_blocked(ip: Ipv4Addr) -> bool {
    let n = u32::from(ip);
    BLOCKED_V4.iter().any(|&(net, len)| {
        let mask = if len == 0 { 0 } else { u32::MAX << (32 - len) };
        n & mask == u32::from(Ipv4Addr::from(net)) & mask
    })
}

fn v6_blocked(ip: Ipv6Addr) -> bool {
    let n = u128::from(ip);
    BLOCKED_V6.iter().any(|&(net, len)| {
        let mask = if len == 0 { 0 } else { u128::MAX << (128 - len) };
        n & mask == net & mask
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_str_bicimi() {
        let cases = [
            ("::ffff:127.0.0.1", "::ffff:127.0.0.1"),
            ("::FFFF:808:808", "::ffff:8.8.8.8"),
            ("2001:DB8:0:0:1:0:0:1", "2001:db8::1:0:0:1"),
            ("1:0:1:1:1:1:1:1", "1:0:1:1:1:1:1:1"),
            ("::", "::"),
            ("1::", "1::"),
            ("fe80::1%eth0", "fe80::1%eth0"),
            ("::1:2:3:4:5:6:7", "0:1:2:3:4:5:6:7"),
        ];
        for (inp, want) in cases {
            assert_eq!(PyIp::parse(inp).unwrap().to_py_string(), want, "{inp}");
        }
    }

    #[test]
    fn python_gecersiz() {
        for s in [
            "01.2.3.4",
            "1.2.3",
            "256.1.1.1",
            "1:2:3:4:5:6:7::8",
            "1:2:3:4:5:6:7:8:9",
            "fe80::1%",
            ":1::2",
            "１.2.3.4",
        ] {
            assert!(PyIp::parse(s).is_none(), "{s}");
        }
    }

    #[test]
    fn engelli() {
        for s in [
            "127.0.0.1",
            "10.1.2.3",
            "::1",
            "::",
            "0.0.0.0",
            "::ffff:169.254.169.254",
            "fd00::5",
            "fe80::1",
        ] {
            assert!(PyIp::parse(s).unwrap().is_blocked(), "{s}");
        }
        for s in ["93.184.216.34", "2606:4700::1111", "::ffff:8.8.8.8"] {
            assert!(!PyIp::parse(s).unwrap().is_blocked(), "{s}");
        }
    }
}
