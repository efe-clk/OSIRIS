//! Prometheus ölçümleri (metin biçimi 0.0.4) — ayrı dinleyicide (`OSIRIS_METRICS_ADDR`)
//! sunulur; ana API'ye (ve Nginx `/api/` önekine) açılmaz.
//!
//! Etiketler yalnızca rota şablonu (`/items/{item_id}`), yöntem ve durum kodudur: yol
//! parametreleri ve sorgu dizgeleri (kişisel veri olabilir) ölçüme girmez.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const BUCKETS: [f64; 11] = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];

#[derive(Default)]
struct Histogram {
    counts: [u64; BUCKETS.len()],
    sum: f64,
    count: u64,
}

#[derive(Default)]
pub struct Metrics {
    requests: Mutex<BTreeMap<(String, String, u16), u64>>,
    durations: Mutex<BTreeMap<String, Histogram>>,
    pub rate_limited: AtomicU64,
    pub audit_failures: AtomicU64,
}

impl Metrics {
    pub fn observe(&self, method: &str, route: &str, status: u16, elapsed: Duration) {
        if let Ok(mut r) = self.requests.lock() {
            *r.entry((method.to_owned(), route.to_owned(), status)).or_default() += 1;
        }
        let secs = elapsed.as_secs_f64();
        if let Ok(mut d) = self.durations.lock() {
            let h = d.entry(route.to_owned()).or_default();
            for (i, b) in BUCKETS.iter().enumerate() {
                if secs <= *b {
                    h.counts[i] += 1;
                }
            }
            h.sum += secs;
            h.count += 1;
        }
    }

    pub fn render(&self) -> String {
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
        let mut out = String::new();
        let _ = writeln!(
            out,
            "# HELP osiris_build_info OSIRIS API sürümü\n# TYPE osiris_build_info gauge"
        );
        let _ = writeln!(out, "osiris_build_info{{version=\"{}\"}} 1", env!("CARGO_PKG_VERSION"));
        out.push_str("# HELP osiris_http_requests_total HTTP istekleri\n# TYPE osiris_http_requests_total counter\n");
        if let Ok(r) = self.requests.lock() {
            for ((m, route, s), n) in r.iter() {
                let _ = writeln!(
                    out,
                    "osiris_http_requests_total{{method=\"{}\",route=\"{}\",status=\"{s}\"}} {n}",
                    esc(m),
                    esc(route)
                );
            }
        }
        out.push_str(
            "# HELP osiris_http_request_duration_seconds İstek süresi\n# TYPE osiris_http_request_duration_seconds histogram\n",
        );
        if let Ok(d) = self.durations.lock() {
            for (route, h) in d.iter() {
                let route = esc(route);
                for (i, b) in BUCKETS.iter().enumerate() {
                    let _ = writeln!(
                        out,
                        "osiris_http_request_duration_seconds_bucket{{route=\"{route}\",le=\"{b}\"}} {}",
                        h.counts[i]
                    );
                }
                let _ = writeln!(
                    out,
                    "osiris_http_request_duration_seconds_bucket{{route=\"{route}\",le=\"+Inf\"}} {}",
                    h.count
                );
                let _ = writeln!(
                    out,
                    "osiris_http_request_duration_seconds_sum{{route=\"{route}\"}} {}",
                    h.sum
                );
                let _ = writeln!(
                    out,
                    "osiris_http_request_duration_seconds_count{{route=\"{route}\"}} {}",
                    h.count
                );
            }
        }
        for (name, help, v) in [
            (
                "osiris_api_rate_limited_total",
                "Kota aşımıyla reddedilen istekler",
                &self.rate_limited,
            ),
            ("osiris_audit_failures_total", "Yazılamayan denetim kayıtları", &self.audit_failures),
        ] {
            let _ = writeln!(
                out,
                "# HELP {name} {help}\n# TYPE {name} counter\n{name} {}",
                v.load(Ordering::Relaxed)
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn metin_bicimi() {
        let m = super::Metrics::default();
        m.observe("GET", "/items/{item_id}", 404, std::time::Duration::from_millis(3));
        m.rate_limited.fetch_add(2, std::sync::atomic::Ordering::Relaxed);
        let t = m.render();
        assert!(t.contains("osiris_http_requests_total{method=\"GET\",route=\"/items/{item_id}\",status=\"404\"} 1"), "{t}");
        assert!(t.contains(
            "osiris_http_request_duration_seconds_bucket{route=\"/items/{item_id}\",le=\"0.005\"} 1"
        ));
        assert!(t.contains("osiris_api_rate_limited_total 2"));
    }
}
