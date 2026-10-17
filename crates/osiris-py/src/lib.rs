//! `osiris_native` — Rust çekirdeğin Python yüzü (R4 geçiş köprüsü).
//!
//! Python tarafı `osiris/_native.py` üzerinden seçer (`OSIRIS_NATIVE=rust|python`,
//! varsayılan rust). Bu modül iş mantığı içermez: dönüşüm + hata eşlemesi yapar.
//! Tüm doğrulama hataları `ValueError` olarak yükselir (Python uygulamasıyla aynı).

use osiris_hash::Json;
use osiris_hash::audit::{self, AuditRow};
use osiris_security::{CheckError, Resolver};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};

/// İç içe JSON derinlik sınırı (Python'da RecursionError'dan önce).
const MAX_JSON_DEPTH: usize = 512;

// ------------------------------------------------------------------------ hash / html

/// `ProcessingPipeline.clean` (bs4 html.parser birebir). GIL bırakılır.
#[pyfunction]
fn clean_text(py: Python<'_>, text: String) -> String {
    py.detach(|| osiris_hash::clean(&text))
}

/// `hex(sha256(utf8(cleaned)))`
#[pyfunction]
fn content_hash(cleaned: String) -> String {
    osiris_hash::content_hash(&cleaned)
}

/// Regex varlıkları: `[{"type": …, "value": …}]` (NER hariç).
#[pyfunction]
fn extract_entities<'py>(py: Python<'py>, text: String) -> PyResult<Bound<'py, PyList>> {
    let ents = py.detach(|| osiris_extract::extract_entities(&text));
    let out = PyList::empty(py);
    for e in ents {
        let d = PyDict::new(py);
        d.set_item("type", e.etype)?;
        d.set_item("value", e.value)?;
        out.append(d)?;
    }
    Ok(out)
}

// -------------------------------------------------------------------------------- audit

fn to_json(obj: &Bound<'_, PyAny>, depth: usize) -> PyResult<Json> {
    if depth > MAX_JSON_DEPTH {
        return Err(PyValueError::new_err("JSON çok derin"));
    }
    if obj.is_none() {
        return Ok(Json::Null);
    }
    if obj.is_instance_of::<PyBool>() {
        return Ok(Json::Bool(obj.extract::<bool>()?));
    }
    if obj.is_instance_of::<PyInt>() {
        return Ok(match obj.extract::<i64>() {
            Ok(i) => Json::Int(i),
            Err(_) => Json::BigInt(obj.str()?.to_string()),
        });
    }
    if obj.is_instance_of::<PyFloat>() {
        return Ok(Json::Float(obj.extract::<f64>()?));
    }
    if obj.is_instance_of::<PyString>() {
        return Ok(Json::Str(obj.extract::<String>()?));
    }
    if obj.is_instance_of::<PyList>() || obj.is_instance_of::<PyTuple>() {
        let mut items = Vec::new();
        for item in obj.try_iter()? {
            items.push(to_json(&item?, depth + 1)?);
        }
        return Ok(Json::Array(items));
    }
    if let Ok(dict) = obj.cast::<PyDict>() {
        let mut entries = Vec::with_capacity(dict.len());
        for (k, v) in dict.iter() {
            if !k.is_instance_of::<PyString>() {
                return Err(PyTypeError::new_err("object keys must be strings"));
            }
            entries.push((k.extract::<String>()?, to_json(&v, depth + 1)?));
        }
        return Ok(Json::Object(entries));
    }
    Err(PyTypeError::new_err(format!("unsupported type: {}", obj.get_type().name()?)))
}

fn value_err(e: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// `audit.canonical_payload` (v1: Python json.dumps, v2: RFC 8785).
#[pyfunction]
#[pyo3(signature = (user_id, action, resource, detail, chain_version))]
fn audit_canonical_payload(
    user_id: Option<String>,
    action: String,
    resource: Option<String>,
    detail: &Bound<'_, PyAny>,
    chain_version: i64,
) -> PyResult<String> {
    let detail = to_json(detail, 0)?;
    audit::canonical_payload(
        user_id.as_deref(),
        &action,
        resource.as_deref(),
        Some(&detail),
        chain_version,
    )
    .map_err(value_err)
}

/// `audit.chain_hash`
#[pyfunction]
#[pyo3(signature = (prev, payload))]
fn audit_chain_hash(prev: Option<String>, payload: String) -> String {
    audit::chain_hash(prev.as_deref(), &payload)
}

fn row_from_tuple(row: &Bound<'_, PyAny>) -> PyResult<AuditRow> {
    let len = row.len()?;
    let get = |i: usize| row.get_item(i);
    let version = if len > 7 { get(7)?.extract::<Option<i64>>()? } else { None };
    let detail = get(4)?;
    Ok(AuditRow {
        user_id: get(1)?.extract()?,
        action: get(2)?.extract()?,
        resource: get(3)?.extract()?,
        detail: if detail.is_none() { None } else { Some(to_json(&detail, 0)?) },
        prev_hash: get(5)?.extract()?,
        hash: get(6)?.extract()?,
        chain_version: version,
    })
}

/// `audit.verify_records(rows)` — satır: `(id, user_id, action, resource, detail,
/// prev_hash, hash[, chain_version])`, `detail` ayrıştırılmış JSON (dize değil).
#[pyfunction]
fn audit_verify_records<'py>(
    py: Python<'py>,
    rows: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let mut parsed: Vec<AuditRow> = Vec::new();
    let mut convert_error: Option<String> = None;
    for row in rows.try_iter()? {
        match row_from_tuple(&row?) {
            Ok(r) => parsed.push(r),
            Err(e) => {
                convert_error = Some(e.value(py).to_string());
                break;
            }
        }
    }
    let mut result = audit::verify_records(&parsed);
    if let (true, Some(msg)) = (result.ok, convert_error) {
        // Python sırası: prev_hash karşılaştırması kanonikleştirmeden önce gelir — ama
        // dönüştürülemeyen satırın prev_hash'i de okunamamış olabilir; kanonikleştirme
        // hatası olarak raporlanır.
        result = audit::Verification {
            ok: false,
            checked: result.checked,
            error: Some(format!("kanonikleştirme: {msg}")),
        };
    }
    let d = PyDict::new(py);
    d.set_item("ok", result.ok)?;
    d.set_item("checked", result.checked)?;
    if let Some(e) = result.error {
        d.set_item("error", e)?;
    }
    Ok(d)
}

// ----------------------------------------------------------------------------- security

/// Python çağrılabiliri (`security._host_ips`) ile DNS — testler onu taklit eder.
struct PyResolver<'a, 'py>(&'a Bound<'py, PyAny>);

impl Resolver for PyResolver<'_, '_> {
    type Error = PyErr;

    fn resolve(&self, host: &str) -> Result<Vec<String>, PyErr> {
        self.0.call1((host,))?.extract::<Vec<String>>()
    }
}

fn check_err(e: CheckError<PyErr>) -> PyErr {
    match e {
        CheckError::Denied(d) => PyValueError::new_err(d.0),
        CheckError::Resolver(e) => e,
    }
}

/// `security.resolve_safe_ips(host)`; `resolver(host) -> list[str]`.
#[pyfunction]
fn resolve_safe_ips(host: Option<String>, resolver: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    osiris_security::resolve_safe_ips(host.as_deref().unwrap_or(""), &PyResolver(resolver))
        .map_err(check_err)
}

/// `security.assert_safe_url(url, allow_onion=...)`; geçerliyse özgün URL.
#[pyfunction]
#[pyo3(signature = (url, allow_onion, resolver))]
fn assert_safe_url(
    url: Option<String>,
    allow_onion: bool,
    resolver: &Bound<'_, PyAny>,
) -> PyResult<String> {
    osiris_security::assert_safe_url(
        url.as_deref().unwrap_or(""),
        allow_onion,
        &PyResolver(resolver),
    )
    .map_err(check_err)
}

#[pyfunction]
#[pyo3(signature = (value, max_len=64))]
fn sanitize_irc_token(value: Option<String>, max_len: usize) -> PyResult<String> {
    osiris_security::sanitize_irc_token(value.as_deref().unwrap_or(""), max_len)
        .map(str::to_owned)
        .map_err(value_err)
}

#[pyfunction]
fn sanitize_channel(value: Option<String>) -> PyResult<String> {
    osiris_security::sanitize_channel(value.as_deref().unwrap_or(""))
        .map(str::to_owned)
        .map_err(value_err)
}

#[pyfunction]
fn sanitize_domain(domain: Option<String>) -> PyResult<String> {
    osiris_security::sanitize_domain(domain.as_deref().unwrap_or("")).map_err(value_err)
}

#[pyfunction]
#[pyo3(signature = (host, max_len=253))]
fn sanitize_hostname(host: Option<String>, max_len: usize) -> PyResult<String> {
    osiris_security::sanitize_hostname(host.as_deref().unwrap_or(""), max_len).map_err(value_err)
}

#[pyfunction]
#[pyo3(signature = (value, keep=4))]
fn mask_secret(value: Option<String>, keep: usize) -> String {
    osiris_security::mask_secret(value.as_deref(), keep)
}

#[pymodule]
fn osiris_native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_function(wrap_pyfunction!(clean_text, m)?)?;
    m.add_function(wrap_pyfunction!(content_hash, m)?)?;
    m.add_function(wrap_pyfunction!(extract_entities, m)?)?;
    m.add_function(wrap_pyfunction!(audit_canonical_payload, m)?)?;
    m.add_function(wrap_pyfunction!(audit_chain_hash, m)?)?;
    m.add_function(wrap_pyfunction!(audit_verify_records, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_safe_ips, m)?)?;
    m.add_function(wrap_pyfunction!(assert_safe_url, m)?)?;
    m.add_function(wrap_pyfunction!(sanitize_irc_token, m)?)?;
    m.add_function(wrap_pyfunction!(sanitize_channel, m)?)?;
    m.add_function(wrap_pyfunction!(sanitize_domain, m)?)?;
    m.add_function(wrap_pyfunction!(sanitize_hostname, m)?)?;
    m.add_function(wrap_pyfunction!(mask_secret, m)?)?;
    Ok(())
}
