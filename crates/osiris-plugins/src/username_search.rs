//! `plugins/username-search` — Maigret uyumlu profil taraması (tempo sınırlı, eşzamanlı).
//!
//! Site DB'leri derleme zamanında gömülür (`sites.json`, `sites_full.json`). Kullanıcı
//! `sites_db` değeri yalnızca plugin dizinindeki bu dosyaları ya da `OSIRIS_USERNAME_DB_DIR`
//! altındaki `.json` dosyalarını gösterebilir (E13). Fark: Python'daki plugin dizinine giden
//! **mutlak** yol Rust'ta kabul edilmez (dizin ikili dosyada yoktur).

use crate::common::{UA, meta, py_max, py_min};
use async_trait::async_trait;
use osiris_plugin::http::quote_all;
use osiris_plugin::{Collector, FetchOptions, Host, PluginCrash, fetch};
use osiris_types::pyval::{PyConvError, py_float, py_int, py_str, trunc, truthy};
use osiris_types::{CollectedItem, CollectionResult, Config, Manifest};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore};
use tokio::time::Instant;

static SITES: &str = include_str!("../../../plugins/username-search/sites.json");
static SITES_FULL: &str = include_str!("../../../plugins/username-search/sites_full.json");
static MANIFEST_JSON: &str = include_str!("../../../plugins/username-search/manifest.json");
static USERNAME: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\A[A-Za-z0-9_][A-Za-z0-9_.-]{1,63}\n?\z").expect("kullanıcı adı deseni")
});

/// Plugin dizinindeki (gömülü) JSON dosyaları.
fn embedded(name: &str) -> Option<&'static str> {
    match name {
        "sites.json" => Some(SITES),
        "sites_full.json" => Some(SITES_FULL),
        "manifest.json" => Some(MANIFEST_JSON),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct Rule {
    url: String,
    check: String,
    presence: Vec<String>,
    absence: Vec<String>,
    tags: Vec<String>,
}

/// Göreli yolu sözcüksel olarak normalleştirir (`..` köke çıkamaz).
fn normalize(rel: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in rel.components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

enum DbSource {
    Embedded(&'static str),
    File(PathBuf),
}

/// `resolve_sites_db`: güvenilmeyen değeri izinli dizinlere kilitler.
fn resolve_sites_db(value: &str, extra: Option<&Path>) -> Result<DbSource, String> {
    if value.is_empty() || value.chars().count() > 1024 || value.contains('\0') {
        return Err("sites_db geçersiz".into());
    }
    let raw = Path::new(value);
    let not_found = || {
        "sites_db izinli dizinlerde bulunamadı (plugin dizini veya OSIRIS_USERNAME_DB_DIR)"
            .to_owned()
    };
    let extra_canon = extra.and_then(|d| d.canonicalize().ok());
    let check_file = |p: PathBuf| -> Option<DbSource> {
        let cand = p.canonicalize().ok()?;
        let dir = extra_canon.as_ref()?;
        (cand.extension().is_some_and(|e| e == "json") && cand.starts_with(dir) && cand.is_file())
            .then_some(DbSource::File(cand))
    };
    if raw.is_absolute() {
        return check_file(raw.to_path_buf()).ok_or_else(not_found);
    }
    if let Some(n) = normalize(raw)
        && let Some(s) = n.to_str().and_then(embedded)
    {
        return Ok(DbSource::Embedded(s));
    }
    if let Some(d) = extra
        && let Some(src) = check_file(d.join(raw))
    {
        return Ok(src);
    }
    Err(not_found())
}

/// Python `ast.literal_eval` alt kümesi: dizeler, sayılar, True/False/None içeren liste.
fn literal_list(s: &str) -> Option<Vec<String>> {
    let t = s.trim();
    let inner = t.strip_prefix('[')?.strip_suffix(']')?;
    let mut out = Vec::new();
    let c: Vec<char> = inner.chars().collect();
    let mut i = 0;
    loop {
        while i < c.len() && c[i].is_whitespace() {
            i += 1;
        }
        if i >= c.len() {
            return Some(out);
        }
        if c[i] == '\'' || c[i] == '"' {
            let q = c[i];
            i += 1;
            let mut v = String::new();
            while i < c.len() && c[i] != q {
                if c[i] == '\\' && i + 1 < c.len() {
                    i += 1;
                }
                v.push(c[i]);
                i += 1;
            }
            if i >= c.len() {
                return None;
            }
            i += 1;
            out.push(v);
        } else {
            let st = i;
            while i < c.len() && c[i] != ',' {
                i += 1;
            }
            let tok = c[st..i].iter().collect::<String>().trim().to_owned();
            match tok.as_str() {
                "True" | "False" | "None" => out.push(tok),
                t if t.parse::<f64>().is_ok() => out.push(t.to_owned()),
                _ => return None,
            }
        }
        while i < c.len() && c[i].is_whitespace() {
            i += 1;
        }
        if i < c.len() {
            if c[i] != ',' {
                return None;
            }
            i += 1;
        }
    }
}

/// `_as_list`: Maigret'in dize-veya-liste alanları.
fn as_list(v: Option<&Value>) -> Vec<String> {
    match v {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a.iter().map(py_str).take(10).collect(),
        Some(Value::String(s)) => {
            let st = s.trim_matches(osiris_hash::py_is_space);
            if st.starts_with('[')
                && let Some(l) = literal_list(st)
            {
                return l.into_iter().take(10).collect();
            }
            if st.is_empty() { Vec::new() } else { vec![st.to_owned()] }
        }
        Some(other) => vec![py_str(other)],
    }
}

fn load_sites(text: &str) -> Result<BTreeMap<String, Rule>, String> {
    let mut data: Value =
        serde_json::from_str(text).map_err(|e| format!("Site DB okunamadı: {e}"))?;
    if let Some(Value::Object(sites)) = data.get("sites") {
        data = Value::Object(sites.clone());
    }
    let Value::Object(map) = data else { return Err("Site DB biçimi geçersiz".into()) };
    let mut sites = BTreeMap::new();
    for (name, rule) in map {
        let Value::Object(r) = &rule else { continue };
        if name.starts_with('_') || r.get("disabled").is_some_and(truthy) {
            continue;
        }
        let url = [r.get("urlProbe"), r.get("url")]
            .into_iter()
            .flatten()
            .find(|v| truthy(v))
            .map(py_str)
            .unwrap_or_default();
        if !url.contains("{username}") {
            continue;
        }
        let check = r.get("checkType").map_or("status_code".into(), py_str);
        if !matches!(check.as_str(), "status_code" | "message" | "response_url") {
            continue;
        }
        let mut presence = as_list(r.get("presenceStrs"));
        presence.extend(as_list(r.get("presenseStrs")));
        presence.truncate(10);
        let mut absence = as_list(r.get("absenceStrs"));
        absence.truncate(10);
        let mut tags = as_list(r.get("tags"));
        tags.truncate(10);
        sites.insert(name.clone(), Rule { url, check, presence, absence, tags });
    }
    Ok(sites)
}

/// İstek temposu: genel hız + host başına aralık (iş parçacığı güvenli).
struct Throttle {
    gap: Duration,
    host_gap: Duration,
    state: Mutex<(Instant, HashMap<String, Instant>)>,
}

impl Throttle {
    fn new(rate: f64, per_host: f64) -> Self {
        let gap = if rate > 0.0 { Duration::from_secs_f64(1.0 / rate) } else { Duration::ZERO };
        let start = Instant::now();
        Self {
            gap,
            host_gap: Duration::from_secs_f64(per_host.max(0.0)),
            state: Mutex::new((start, HashMap::new())),
        }
    }

    async fn wait(&self, host: &str) {
        let start = {
            let mut st = self.state.lock().await;
            let now = Instant::now();
            let start = now.max(st.0).max(*st.1.get(host).unwrap_or(&now));
            st.0 = start + self.gap;
            st.1.insert(host.to_owned(), start + self.host_gap);
            start
        };
        tokio::time::sleep_until(start).await;
    }
}

async fn check_one(
    host: &Host,
    site: &str,
    rule: &Rule,
    username: &str,
    timeout: u64,
    throttle: &Throttle,
) -> Option<CollectedItem> {
    let profile_url = rule.url.replace("{username}", &quote_all(username));
    let host_key = osiris_security::urlsplit::urlsplit(&profile_url)
        .ok()
        .and_then(|r| r.hostname())
        .unwrap_or_default()
        .to_lowercase();
    throttle.wait(&host_key).await;
    let opts = FetchOptions::new("GET", &profile_url)
        .timeout_secs(timeout)
        .max_bytes(500_000)
        .header("User-Agent", UA);
    let resp = fetch(host, opts).await.ok()?;
    let found = match rule.check.as_str() {
        "status_code" => {
            if resp.status != 200 {
                return None;
            }
            let body = trunc(&resp.text(), 50_000);
            !rule.absence.iter().any(|a| body.contains(a.as_str()))
        }
        "message" => {
            let body = trunc(&resp.text(), 50_000);
            rule.presence.iter().any(|p| body.contains(p.as_str()))
        }
        "response_url" => resp.url.to_lowercase().contains(&username.to_lowercase()),
        _ => false,
    };
    if !found {
        return None;
    }
    let mut tags = vec!["username".to_owned()];
    tags.extend(rule.tags.iter().take(5).cloned());
    Some(CollectedItem {
        url: Some(trunc(&profile_url, 2048)),
        title: Some(trunc(&format!("{username} @ {site}"), 500)),
        raw_content: trunc(&format!("Profil bulundu: {site} — {profile_url}"), 2000),
        published_at: None,
        metadata: meta(json!({ "site": site, "username": username, "check": rule.check })),
        tags,
    })
}

pub struct UsernameSearch(pub Manifest);

#[async_trait]
impl Collector for UsernameSearch {
    fn manifest(&self) -> &Manifest {
        &self.0
    }

    async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash> {
        let username = match cfg.get("username") {
            Some(Value::String(u)) if !u.is_empty() && USERNAME.is_match(u) => u.clone(),
            _ => return Ok(CollectionResult::fail("Geçersiz username")),
        };
        let db_text = match cfg.get("sites_db").filter(|v| truthy(v)) {
            Some(v) => {
                let Value::String(s) = v else {
                    return Ok(CollectionResult::fail("sites_db geçersiz"));
                };
                match resolve_sites_db(s, host.env.username_db_dir.as_deref()) {
                    Ok(DbSource::Embedded(t)) => t.to_owned(),
                    Ok(DbSource::File(p)) => match std::fs::read_to_string(&p) {
                        Ok(t) => t,
                        Err(e) => {
                            return Ok(CollectionResult::fail(format!("Site DB okunamadı: {e}")));
                        }
                    },
                    Err(e) => return Ok(CollectionResult::fail(e)),
                }
            }
            None if cfg.get("full").is_some_and(truthy) => SITES_FULL.to_owned(),
            None => SITES.to_owned(),
        };
        let mut sites = match load_sites(&db_text) {
            Ok(s) => s,
            Err(e) => return Ok(CollectionResult::fail(e)),
        };
        if let Some(Value::Array(only)) = cfg.get("sites")
            && !only.is_empty()
        {
            let wanted: HashSet<String> = only.iter().map(py_str).collect();
            sites.retain(|k, _| wanted.contains(k));
        }
        if let Some(Value::Array(tags)) = cfg.get("tags")
            && !tags.is_empty()
        {
            let wanted: HashSet<String> = tags.iter().map(py_str).collect();
            sites.retain(|_, r| r.tags.iter().any(|t| wanted.contains(t)));
        }
        let int_or = |key: &str, default: i64, lo: i64, hi: i64| -> Result<i64, PluginCrash> {
            match cfg.get(key).map_or(Ok(default), py_int) {
                Ok(v) => Ok(v.clamp(lo, hi)),
                Err(PyConvError::Invalid) => Ok(default),
                Err(PyConvError::Overflow) => Err(PluginCrash("OverflowError".into())),
            }
        };
        let max_sites = int_or("max_sites", 200, 1, 3000)? as usize;
        let workers = int_or("workers", 10, 1, 20)? as usize;
        let timeout = int_or("timeout", 15, 5, 30)? as u64;
        let rate = match cfg.get("rate_per_sec").map_or(Ok(10.0), py_float) {
            Ok(r) if r <= 0.0 => 0.0,
            Ok(r) => py_max(0.1, py_min(r, 50.0)),
            Err(_) => 10.0,
        };
        let host_ms = int_or("per_host_interval_ms", 1000, 0, 60_000)?;
        let throttle = Arc::new(Throttle::new(rate, host_ms as f64 / 1000.0));
        let names: Vec<String> = sites.keys().take(max_sites).cloned().collect();
        let sem = Arc::new(Semaphore::new(workers));
        let mut tasks = Vec::new();
        for name in &names {
            let (host, rule, user, thr, sem, name) = (
                host.clone(),
                sites[name].clone(),
                username.clone(),
                throttle.clone(),
                sem.clone(),
                name.clone(),
            );
            tasks.push(tokio::spawn(async move {
                let _permit = sem.acquire_owned().await.ok()?;
                check_one(&host, &name, &rule, &user, timeout, &thr).await
            }));
        }
        let mut items = Vec::new();
        let mut unknown = 0;
        for t in tasks {
            match t.await {
                Ok(Some(it)) => items.push(it),
                _ => unknown += 1,
            }
        }
        items.sort_by(|a, b| {
            let s = |i: &CollectedItem| {
                i.metadata.get("site").and_then(Value::as_str).unwrap_or("").to_owned()
            };
            s(a).cmp(&s(b))
        });
        let found = items.len();
        let metadata: Map<String, Value> = meta(
            json!({ "username": username, "checked": names.len(), "found": found, "unknown": unknown }),
        );
        Ok(CollectionResult::ok(items, metadata))
    }

    async fn health_check(&self, _host: &Host, _cfg: &Config) -> bool {
        load_sites(SITES).is_ok_and(|s| !s.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gomulu_db_ve_yol_kilidi() {
        assert!(load_sites(SITES).unwrap().len() > 10);
        assert!(load_sites(SITES_FULL).unwrap().len() > 1000);
        assert!(matches!(resolve_sites_db("sites.json", None), Ok(DbSource::Embedded(_))));
        assert!(matches!(
            resolve_sites_db("./x/../sites_full.json", None),
            Ok(DbSource::Embedded(_))
        ));
        assert!(resolve_sites_db("/etc/passwd", None).is_err());
        assert!(resolve_sites_db("../../../etc/passwd", None).is_err());
        assert_eq!(
            literal_list("['a', \"b\", 1, None]"),
            Some(vec!["a".into(), "b".into(), "1".into(), "None".into()])
        );
    }
}
