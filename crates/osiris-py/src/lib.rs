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

// ------------------------------------------------ HTML ağacı (eşdeğerlik testleri, R5)

fn kind_name(k: &osiris_hash::html::Kind) -> &'static str {
    use osiris_hash::html::Kind;
    match k {
        Kind::Text => "NavigableString",
        Kind::Contained(c) => match c.as_str() {
            "script" => "Script",
            "style" => "Stylesheet",
            "template" => "TemplateString",
            "rt" => "RubyTextString",
            "rp" => "RubyParenthesisString",
            _ => "NavigableString",
        },
        Kind::Comment => "Comment",
        Kind::CData => "CData",
        Kind::Doctype => "Doctype",
        Kind::Declaration => "Declaration",
        Kind::Pi => "ProcessingInstruction",
    }
}

fn tree_node<'py>(
    py: Python<'py>,
    doc: &osiris_hash::html::Document,
    id: usize,
) -> PyResult<Bound<'py, PyAny>> {
    use osiris_hash::html::Node;
    Ok(match &doc.nodes[id] {
        Node::Element { name, attrs, children, .. } => {
            let kids = PyList::empty(py);
            for &c in children {
                kids.append(tree_node(py, doc, c)?)?;
            }
            let a: Vec<(String, String)> = attrs.clone();
            (name.clone(), a, kids).into_pyobject(py)?.into_any()
        }
        Node::Text { kind, text, .. } => {
            ("#text", kind_name(kind), text.clone()).into_pyobject(py)?.into_any()
        }
    })
}

/// bs4 ağacı: `(ad, [(anahtar, değer)], [çocuklar])` / `("#text", sınıf, metin)`.
#[pyfunction]
fn html_tree<'py>(py: Python<'py>, markup: String) -> PyResult<Bound<'py, PyAny>> {
    let doc = osiris_hash::html::parse(&markup).map_err(value_err)?;
    tree_node(py, &doc, osiris_hash::html::ROOT)
}

/// `soup.select(sel)` → `[(ad, get_text(" ", strip=True))]`; desteklenmeyen seçici
/// `NotImplementedError`, geçersiz seçici `ValueError`.
#[pyfunction]
fn html_select(markup: String, selector: String) -> PyResult<Vec<(String, String)>> {
    use osiris_hash::html::select::SelectError;
    let doc = osiris_hash::html::parse(&markup).map_err(value_err)?;
    let ids = doc.select(&selector).map_err(|e| match e {
        SelectError::Invalid(m) => PyValueError::new_err(m),
        SelectError::Unsupported(m) => pyo3::exceptions::PyNotImplementedError::new_err(m),
    })?;
    Ok(ids.into_iter().map(|i| (doc.name(i).unwrap_or("").to_owned(), doc.text(i))).collect())
}

/// `security.safe_title(BeautifulSoup(markup, "html.parser"))`
#[pyfunction]
fn html_title(markup: String) -> PyResult<Option<String>> {
    Ok(osiris_hash::html::parse(&markup).map_err(value_err)?.safe_title())
}

/// (link, title, summary, published)
type FeedEntry = (Option<String>, Option<String>, Option<String>, Option<String>);

/// RSS/Atom (eşdeğerlik testleri): `[(link, title, summary, published)]`, bozo.
#[pyfunction]
fn feed_parse(data: Vec<u8>) -> (Vec<FeedEntry>, bool) {
    let f = osiris_feed::parse(&data);
    (f.entries.into_iter().map(|e| (e.link, e.title, e.summary, e.published)).collect(), f.bozo)
}

/// Python `html.unescape`
#[pyfunction]
fn html_unescape(s: String) -> PyResult<String> {
    osiris_hash::html::py_unescape(&s).map_err(value_err)
}

/// CPython `urllib.parse.urljoin`
#[pyfunction]
fn urljoin(base: String, url: String) -> PyResult<String> {
    osiris_security::urlsplit::urljoin(&base, &url).map_err(value_err)
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

/// `records.record_json` — plugin `raw_content` kanonik JSON'u (SOZLESME §4).
#[pyfunction]
fn record_json(value: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(osiris_hash::canonical::record_json(&to_json(value, 0)?))
}

/// Python `json.loads` uyumlu ayrıştırma + kanonik kayıt metni (eşdeğerlik testleri için).
#[pyfunction]
fn record_json_from_text(text: String) -> PyResult<String> {
    let v = Json::parse_py(&text).map_err(value_err)?;
    Ok(osiris_hash::canonical::record_json(&v))
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
#[pyo3(signature = (url, allow_onion, allow_i2p, resolver))]
fn assert_safe_url(
    url: Option<String>,
    allow_onion: bool,
    allow_i2p: bool,
    resolver: &Bound<'_, PyAny>,
) -> PyResult<String> {
    let policy = osiris_security::UrlPolicy { allow_onion, allow_i2p };
    osiris_security::assert_safe_url(url.as_deref().unwrap_or(""), policy, &PyResolver(resolver))
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
    m.add_function(wrap_pyfunction!(html_tree, m)?)?;
    m.add_function(wrap_pyfunction!(html_select, m)?)?;
    m.add_function(wrap_pyfunction!(html_title, m)?)?;
    m.add_function(wrap_pyfunction!(html_unescape, m)?)?;
    m.add_function(wrap_pyfunction!(feed_parse, m)?)?;
    m.add_function(wrap_pyfunction!(urljoin, m)?)?;
    m.add_function(wrap_pyfunction!(record_json, m)?)?;
    m.add_function(wrap_pyfunction!(record_json_from_text, m)?)?;
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
