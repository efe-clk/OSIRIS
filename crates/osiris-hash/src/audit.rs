//! Denetim zinciri (docs/SOZLESME.md §2) — Python `osiris/audit.py` ile aynı.
//!
//! `hash = sha256(utf8(prev + "|" + payload))`, ilk kayıtta `prev = "GENESIS"`.
//! Kayıt nesnesi tam olarak `{user_id, action, resource, detail}`; `detail` boş/yanlış
//! değerliyse `{}`. v1 = Python `json.dumps`, v2 = RFC 8785 (`_jsonb_numbers` sonrası).

use crate::canonical::{CanonError, jcs, jsonb_numbers, py_dumps};
use crate::json::Json;
use crate::sha256_hex;

pub const GENESIS: &str = "GENESIS";
pub const CURRENT_CHAIN_VERSION: i64 = 2;

fn record(
    user_id: Option<&str>,
    action: &str,
    resource: Option<&str>,
    detail: Option<&Json>,
) -> Json {
    let opt = |v: Option<&str>| v.map_or(Json::Null, |s| Json::Str(s.to_owned()));
    let detail = match detail {
        Some(d) if !d.is_falsy() => d.clone(),
        _ => Json::Object(Vec::new()),
    };
    Json::Object(vec![
        ("user_id".to_owned(), opt(user_id)),
        ("action".to_owned(), Json::Str(action.to_owned())),
        ("resource".to_owned(), opt(resource)),
        ("detail".to_owned(), detail),
    ])
}

/// Kaydın hash'e giren kanonik dizgesi (sürüme göre).
pub fn canonical_payload(
    user_id: Option<&str>,
    action: &str,
    resource: Option<&str>,
    detail: Option<&Json>,
    chain_version: i64,
) -> Result<String, CanonError> {
    let rec = record(user_id, action, resource, detail);
    match chain_version {
        1 => Ok(py_dumps(&rec)),
        2 => jcs(&jsonb_numbers(&rec)?),
        v => Err(CanonError(format!("Bilinmeyen chain_version: {v}"))),
    }
}

/// `sha256(prev | payload)` — `prev` boş/None ise GENESIS.
pub fn chain_hash(prev: Option<&str>, payload: &str) -> String {
    let anchor = match prev {
        Some(p) if !p.is_empty() => p,
        _ => GENESIS,
    };
    sha256_hex(format!("{anchor}|{payload}").as_bytes())
}

/// `audit_logs` satırı (doğrulama için).
#[derive(Debug, Clone)]
pub struct AuditRow {
    pub user_id: Option<String>,
    pub action: String,
    pub resource: Option<String>,
    pub detail: Option<Json>,
    pub prev_hash: Option<String>,
    pub hash: Option<String>,
    /// `None` → 1 (migration 003 öncesi satırlar).
    pub chain_version: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verification {
    pub ok: bool,
    pub checked: usize,
    pub error: Option<String>,
}

/// Artan id sıralı kayıtları doğrular; ilk satırın beyan ettiği öncül kabul edilir.
pub fn verify_records(rows: &[AuditRow]) -> Verification {
    let Some(first) = rows.first() else {
        return Verification { ok: true, checked: 0, error: None };
    };
    let fail = |checked, msg: String| Verification { ok: false, checked, error: Some(msg) };
    let mut prev: Option<String> = first.prev_hash.clone();
    let mut checked = 0;
    for row in rows {
        if row.prev_hash != prev {
            return fail(checked, "prev_hash uyuşmazlığı".to_owned());
        }
        let payload = match canonical_payload(
            row.user_id.as_deref(),
            &row.action,
            row.resource.as_deref(),
            row.detail.as_ref(),
            row.chain_version.unwrap_or(1),
        ) {
            Ok(p) => p,
            Err(e) => return fail(checked, format!("kanonikleştirme: {e}")),
        };
        let digest = chain_hash(prev.as_deref(), &payload);
        if row.hash.as_deref() != Some(digest.as_str()) {
            return fail(checked, "hash uyuşmazlığı".to_owned());
        }
        prev = Some(digest);
        checked += 1;
    }
    Verification { ok: true, checked, error: None }
}
