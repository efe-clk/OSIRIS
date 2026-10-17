//! RSS/Atom ayrıştırma — feedparser 6.0 alan kurallarıyla (rss plugin'inin kullandığı alanlar).
//!
//! Kullanılan alanlar: `link`, `title`, `summary`, `published`. Kurallar (feedparser
//! `namespaces/_base.py`, `mixin.py`):
//! - Öğe adları küçük harfe iner; bilinen ad alanları feedparser önekleriyle eşlenir
//!   (`content:encoded`, `dc:*`, `dcterms:*`; Atom ve RSS ad alanları öneksiz).
//! - `summary`: ilk `description`/`summary` öğesi atanır; içerik (`content`,
//!   `content:encoded`) yalnızca özet yoksa kopyalanır (`_save`). İçerik daha önce geldiyse
//!   sonraki description özeti ezer.
//! - `link`: RSS metni atanır; Atom `<link href>` yalnızca `rel=alternate` (varsayılan) ve
//!   HTML türündeyse; `guid`/`id` (isPermaLink != false) link yoksa link olur.
//! - `published`: `pubDate`, `published`, `issued`, `dcterms:issued` (son değer).
//! - Değerler kırpılır; HTML türündeki içerik (`text/html`, `application/xhtml+xml`;
//!   RSS'te HTML'e benzeyen düz metin dahil) temizlenir; UTF-8 bozulma düzeltmesi ve
//!   cp1252 çevirisi uygulanır.
//!
//! **Sözleşme sınırı:** temizleyici feedparser'ın sgmllib tabanlı çıktısını bayt bayt üretmez;
//! eşdeğerlik `clean(raw_content)` (yani `content_hash`) düzeyinde tanımlıdır
//! (docs/SOZLESME.md §4). Bozuk XML'de feedparser'ın gevşek ayrıştırıcısı yerine hataya
//! kadar okunan girdiler kullanılır.

#[rustfmt::skip]
mod feed_tables; // @generated — tools/gen_feed_tables.py

use feed_tables as t;
use osiris_hash::html::{self, Document, Kind, Node, ROOT};
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entry {
    pub link: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub published: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Feed {
    pub entries: Vec<Entry>,
    pub bozo: bool,
}

const HTML_TYPES: [&str; 2] = ["text/html", "application/xhtml+xml"];

fn map_content_type(t: &str) -> String {
    let t = t.to_lowercase();
    match t.as_str() {
        "text" | "plain" => "text/plain".into(),
        "html" => "text/html".into(),
        "xhtml" => "application/xhtml+xml".into(),
        _ => t,
    }
}

fn ns_prefix(uri: &str) -> Option<&'static str> {
    let u = uri.to_lowercase();
    Some(match u.as_str() {
        "http://www.w3.org/2005/atom"
        | "http://purl.org/atom/ns#"
        | "http://purl.org/rss/1.0/"
        | "http://my.netscape.com/rdf/simple/0.9/"
        | "" => "",
        "http://purl.org/rss/1.0/modules/content/" => "content",
        "http://purl.org/dc/elements/1.1/" => "dc",
        "http://purl.org/dc/terms/" => "dcterms",
        "http://www.w3.org/1999/xhtml" => "xhtml",
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#" => "rdf",
        _ if u.contains("backend.userland.com/rss") => "",
        _ => return None,
    })
}

/// Bayt dizisini feedparser gibi çözer: BOM, XML bildirimi, UTF-8, sonra windows-1252.
fn decode(bytes: &[u8]) -> (String, bool, bool) {
    if let Some(rest) = bytes.strip_prefix(b"\xef\xbb\xbf") {
        return (String::from_utf8_lossy(rest).into_owned(), true, false);
    }
    if bytes.starts_with(b"\xff\xfe") || bytes.starts_with(b"\xfe\xff") {
        let (enc, _) = encoding_rs::Encoding::for_bom(bytes).unwrap_or((encoding_rs::UTF_16LE, 2));
        return (enc.decode_with_bom_removal(bytes).0.into_owned(), false, false);
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_lowercase();
    let declared = head
        .strip_prefix("<?xml")
        .and_then(|h| h.split("?>").next())
        .and_then(|d| d.split("encoding").nth(1))
        .and_then(|e| e.split(['"', '\'']).nth(1))
        .map(str::to_owned);
    match declared.as_deref() {
        Some(label) if !matches!(label, "utf-8" | "utf8") => {
            if let Some(enc) = encoding_rs::Encoding::for_label(label.as_bytes()) {
                let (s, _, bad) = enc.decode(bytes);
                return (s.into_owned(), false, bad);
            }
            (String::from_utf8_lossy(bytes).into_owned(), true, true)
        }
        _ => match std::str::from_utf8(bytes) {
            Ok(s) => (s.to_owned(), true, false),
            Err(_) => (
                encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes).0.into_owned(),
                false,
                true,
            ),
        },
    }
}

/// Temizleyici metin çıktısı: sgmllib çıplak `&`/`<` karakterlerini olduğu gibi yazar; yalnızca
/// yeniden ayrıştırıldığında işaretlemeye ya da referansa dönüşecek olanlar kaçırılır
/// (`<` + harf/`/`/`!`/`?`, `&` + harf/`#`).
fn text_escape(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &ch) in c.iter().enumerate() {
        let next = c.get(i + 1).copied();
        match ch {
            '<' if next
                .is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, '/' | '!' | '?')) =>
            {
                out.push_str("&lt;")
            }
            '&' if next.is_some_and(|n| n.is_ascii_alphabetic() || n == '#') => {
                out.push_str("&amp;")
            }
            ch => out.push(ch),
        }
    }
    out
}

/// `xml.sax.saxutils.escape`
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn looks_like_html(s: &str) -> bool {
    static CLOSE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"</(\w+)>").expect("re"));
    static ENT: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"&#?\w+;").expect("re"));
    static TAG: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"</?(\w+)").expect("re"));
    static NAMED: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"&(\w+);").expect("re"));
    if !(CLOSE.is_match(s) || ENT.is_match(s)) {
        return false;
    }
    if TAG.captures_iter(s).any(|c| !t::ACCEPTABLE_ELEMENTS.contains(&c[1].to_lowercase().as_str()))
    {
        return false;
    }
    !NAMED.captures_iter(s).any(|c| !t::ENTITYDEFS.contains(&&c[1]))
}

/// `pop()` sonrası işleme: kırp, HTML ise temizle, UTF-8 bozulma düzeltmesi, cp1252.
fn finish(raw: &str, ctype: &str, atom: bool, utf8: bool, dangerous: bool) -> String {
    let mut out = raw.trim_matches(osiris_hash::py_is_space).to_owned();
    let mut ctype = ctype.to_owned();
    if !atom && ctype == "text/plain" && looks_like_html(&out) {
        ctype = "text/html".into();
    }
    if dangerous && HTML_TYPES.contains(&ctype.as_str()) {
        out = sanitize_html(&out);
    }
    if utf8 && out.chars().all(|c| (c as u32) <= 0xff) {
        let bytes: Vec<u8> = out.chars().map(|c| c as u8).collect();
        if let Ok(fixed) = String::from_utf8(bytes) {
            out = fixed;
        }
    }
    out.chars()
        .map(|c| match t::CP1252.iter().find(|(k, _)| *k == c as u32) {
            Some((_, v)) => v.to_string(),
            None => c.to_string(),
        })
        .collect()
}

/// feedparser `_HTMLSanitizer` yaklaşımı (bs4 ağacı üzerinde): izinli öğeler ve
/// öznitelikler korunur, `script`/`applet`/`style` içeriğiyle atılır, diğer öğelerin
/// yalnızca içeriği kalır, yorumlar korunur.
pub fn sanitize_html(markup: &str) -> String {
    let Ok(doc) = html::parse(markup) else { return xml_escape(markup) };
    let mut out = String::new();
    for &c in doc.children(ROOT) {
        write_node(&doc, c, &mut out);
    }
    out.trim_matches(osiris_hash::py_is_space).replace("\r\n", "\n")
}

fn safe_uri(v: &str) -> String {
    match osiris_security::urlsplit::urlsplit(v) {
        Ok(r) if r.scheme.is_empty() || t::ACCEPTABLE_URI_SCHEMES.contains(&r.scheme.as_str()) => {
            v.to_owned()
        }
        _ => String::new(),
    }
}

fn attr_escape(v: &str) -> String {
    static BARE: std::sync::LazyLock<fancy_regex::Regex> = std::sync::LazyLock::new(|| {
        fancy_regex::Regex::new(r"&(?!#\d+;|#x[0-9a-fA-F]+;|\w+;)").expect("re")
    });
    let v = v.replace('>', "&gt;").replace('<', "&lt;").replace('"', "&quot;");
    BARE.replace_all(&v, "&amp;").into_owned()
}

fn write_node(doc: &Document, id: usize, out: &mut String) {
    match &doc.nodes[id] {
        Node::Text { kind, text, .. } => match kind {
            Kind::Text | Kind::CData => out.push_str(&text_escape(text)),
            Kind::Comment => {
                out.push_str("<!--");
                out.push_str(text);
                out.push_str("-->");
            }
            // kap içi metin yalnızca kabı izinliyse gelir (ör. <rt>)
            Kind::Contained(c) if !t::UNACCEPTABLE_WITH_END_TAG.contains(&c.as_str()) => {
                out.push_str(&text_escape(text));
            }
            _ => {}
        },
        Node::Element { name, attrs, children, .. } => {
            if t::UNACCEPTABLE_WITH_END_TAG.contains(&name.as_str()) {
                return;
            }
            let ok = t::ACCEPTABLE_ELEMENTS.contains(&name.as_str());
            if ok {
                let mut norm: Vec<(String, String)> = Vec::new();
                for (k, v) in attrs {
                    let k = k.to_lowercase();
                    let v = if k == "rel" || k == "type" { v.to_lowercase() } else { v.clone() };
                    match norm.iter_mut().find(|(k2, _)| *k2 == k) {
                        Some(slot) => slot.1 = v,
                        None => norm.push((k, v)),
                    }
                }
                norm.sort();
                out.push('<');
                out.push_str(name);
                for (k, v) in norm {
                    if k == "style" || !t::ACCEPTABLE_ATTRIBUTES.contains(&k.as_str()) {
                        continue;
                    }
                    let v = if k == "href" { safe_uri(&v) } else { v };
                    out.push_str(&format!(" {k}=\"{}\"", attr_escape(&v)));
                }
                if t::ELEMENTS_NO_END_TAG.contains(&name.as_str()) {
                    out.push_str(" />");
                    return;
                }
                out.push('>');
            }
            for &c in children {
                write_node(doc, c, out);
            }
            if ok {
                out.push_str("</");
                out.push_str(name);
                out.push('>');
            }
        }
    }
}

/// Yakalanan alan (feedparser `push_content`).
struct Capture {
    field: &'static str,
    ctype: String,
    depth: usize,
    text: String,
    /// İçerik türünde (description/summary/content/title) gömülü öğeler markup olur.
    is_content: bool,
    /// description/summary özet yerine içerik olarak işleniyor (`_summaryKey = 'content'`).
    as_content: bool,
}

#[derive(Default)]
struct EntryState {
    e: Entry,
    has_content: bool,
    title_done: bool,
}

/// Bayt dizisini ayrıştırır.
pub fn parse(bytes: &[u8]) -> Feed {
    let (text, utf8, mut bozo) = decode(bytes);
    let mut reader = quick_xml::NsReader::from_str(&text);
    {
        let cfg = reader.config_mut();
        cfg.check_end_names = false;
        // feedparser bozuk XML'de gevşek ayrıştırıcıya geçer; çıplak '&' ve eşleşmeyen
        // kapanışlar hatası yerine okumaya devam edilir
        cfg.allow_dangling_amp = true;
        cfg.allow_unmatched_ends = true;
        cfg.expand_empty_elements = true;
        cfg.trim_text(false);
    }
    let mut feed = Feed::default();
    let mut atom = false;
    let mut depth = 0usize;
    let mut entry: Option<(EntryState, usize)> = None;
    let mut source_depth: Option<usize> = None;
    let mut cap: Option<Capture> = None;

    let ename = |reader: &quick_xml::NsReader<&[u8]>, name: quick_xml::name::QName<'_>| -> String {
        let (ns, local) = reader.resolver().resolve_element(name);
        let local = AsRef::<str>::as_ref(&local).to_lowercase();
        let qprefix = name.prefix().map(|p| AsRef::<str>::as_ref(&p).to_lowercase());
        let prefix = match ns {
            ResolveResult::Bound(n) => {
                ns_prefix(AsRef::<str>::as_ref(&n)).map(str::to_owned).or(qprefix)
            }
            _ => qprefix.filter(|p| p != "xml").or(Some(String::new())),
        };
        match prefix.as_deref() {
            Some(p) if !p.is_empty() => format!("{p}:{local}"),
            _ => local,
        }
    };

    loop {
        let ev = match reader.read_event() {
            Ok(ev) => ev,
            Err(_) => {
                bozo = true;
                break;
            }
        };
        match ev {
            Event::Eof => break,
            Event::Start(s) => {
                depth += 1;
                let name = ename(&reader, s.name());
                let attrs: Vec<(String, String)> = s
                    .attributes()
                    .flatten()
                    .map(|a| {
                        let k = AsRef::<str>::as_ref(&a.key.local_name()).to_lowercase();
                        let v = a
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .map(|c| c.into_owned())
                            .unwrap_or_else(|_| a.value.to_string());
                        (k, v)
                    })
                    .collect();
                let attr = |k: &str| attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
                if depth == 1 {
                    let ns = s.name().prefix().is_none()
                        && attrs
                            .iter()
                            .any(|(k, v)| k == "xmlns" && v.to_lowercase().contains("atom"));
                    atom = name == "feed" || ns;
                }
                // içerik yakalanırken gömülü öğe → markup
                if let Some(c) = cap.as_mut()
                    && c.is_content
                {
                    if !c.ctype.ends_with("xml") {
                        if name == "div" || name == "xhtml:div" {
                            continue;
                        }
                        c.ctype = "application/xhtml+xml".into();
                    }
                    let tag = name.split_once(':').map_or(name.as_str(), |(_, l)| l);
                    let sa: String = attrs
                        .iter()
                        .filter(|(k, _)| k != "xmlns")
                        .map(|(k, v)| format!(" {k}=\"{}\"", xml_escape(v).replace('"', "&quot;")))
                        .collect();
                    c.text.push_str(&format!("<{tag}{sa}>"));
                    continue;
                }
                match name.as_str() {
                    "item" | "entry" => entry = Some((EntryState::default(), depth)),
                    "source" if entry.is_some() => source_depth = Some(depth),
                    _ if entry.is_none() || source_depth.is_some() => {}
                    "title" => {
                        let ctype =
                            map_content_type(&attr("type").unwrap_or_else(|| "text/plain".into()));
                        cap = Some(Capture {
                            field: "title",
                            ctype,
                            depth,
                            text: String::new(),
                            is_content: true,
                            as_content: false,
                        });
                    }
                    "description" | "summary" | "dcterms:abstract" => {
                        let default =
                            if name == "description" { "text/html" } else { "text/plain" };
                        let st = &entry
                            .as_ref()
                            .map(|e| (e.0.e.summary.is_some(), e.0.has_content))
                            .unwrap_or((false, false));
                        let as_content = st.0 && !st.1;
                        let ctype =
                            map_content_type(&attr("type").unwrap_or_else(|| default.into()));
                        cap = Some(Capture {
                            field: "summary",
                            ctype,
                            depth,
                            text: String::new(),
                            is_content: true,
                            as_content,
                        });
                    }
                    "content" | "content:encoded" | "fullitem" | "body" | "xhtml:body" => {
                        if let Some(e) = entry.as_mut() {
                            e.0.has_content = true;
                        }
                        let default = match name.as_str() {
                            "content" => "text/plain",
                            "body" | "xhtml:body" => "application/xhtml+xml",
                            _ => "text/html",
                        };
                        let ctype =
                            map_content_type(&attr("type").unwrap_or_else(|| default.into()));
                        cap = Some(Capture {
                            field: "content",
                            ctype,
                            depth,
                            text: String::new(),
                            is_content: true,
                            as_content: false,
                        });
                    }
                    "link" => match attr("href") {
                        Some(href) => {
                            let rel = attr("rel").unwrap_or_else(|| "alternate".into());
                            let ty = attr("type").unwrap_or_else(|| {
                                if rel == "self" {
                                    "application/atom+xml".into()
                                } else {
                                    "text/html".into()
                                }
                            });
                            if rel == "alternate"
                                && HTML_TYPES.contains(&map_content_type(&ty).as_str())
                                && let Some(e) = entry.as_mut()
                            {
                                e.0.e.link = Some(href);
                            }
                        }
                        None => {
                            cap = Some(Capture {
                                field: "link",
                                ctype: "text/plain".into(),
                                depth,
                                text: String::new(),
                                is_content: false,
                                as_content: false,
                            });
                        }
                    },
                    "guid" | "id" => {
                        let perma = attr("ispermalink").unwrap_or_else(|| "true".into()) == "true";
                        let field = if perma { "guid_link" } else { "guid" };
                        cap = Some(Capture {
                            field,
                            ctype: "text/plain".into(),
                            depth,
                            text: String::new(),
                            is_content: false,
                            as_content: false,
                        });
                    }
                    "pubdate" | "published" | "issued" | "dcterms:issued" => {
                        cap = Some(Capture {
                            field: "published",
                            ctype: "text/plain".into(),
                            depth,
                            text: String::new(),
                            is_content: false,
                            as_content: false,
                        });
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                let name = ename(&reader, e.name());
                if let Some(c) = cap.as_mut()
                    && c.is_content
                    && depth > c.depth
                {
                    if !c.ctype.ends_with("xml") {
                        if name == "div" || name == "xhtml:div" {
                            depth -= 1;
                            continue;
                        }
                        c.ctype = "application/xhtml+xml".into();
                    }
                    let tag = name.split_once(':').map_or(name.as_str(), |(_, l)| l);
                    c.text.push_str(&format!("</{tag}>"));
                    depth = depth.saturating_sub(1);
                    continue;
                }
                if let Some(c) = cap.take_if(|c| c.depth == depth)
                    && let Some((st, _)) = entry.as_mut()
                {
                    let mut raw = c.text;
                    if atom && c.ctype == "application/xhtml+xml" {
                        raw = strip_div(&raw);
                    }
                    let dangerous = matches!(c.field, "title" | "summary" | "content");
                    let value = finish(&raw, &c.ctype, atom, utf8, dangerous);
                    match c.field {
                        "title" => {
                            if !st.title_done && !value.is_empty() {
                                st.e.title = Some(value);
                                st.title_done = true;
                            }
                        }
                        "summary" if c.as_content => {
                            if st.e.summary.is_none() && is_copyable(&c.ctype) {
                                st.e.summary = Some(value);
                            }
                        }
                        "summary" => st.e.summary = Some(value),
                        "content" => {
                            if st.e.summary.is_none() && is_copyable(&c.ctype) {
                                st.e.summary = Some(value);
                            }
                        }
                        "link" => st.e.link = Some(value),
                        "guid_link" => {
                            if st.e.link.is_none() {
                                st.e.link = Some(value);
                            }
                        }
                        "published" => st.e.published = Some(value),
                        _ => {}
                    }
                }
                if source_depth == Some(depth) {
                    source_depth = None;
                }
                if let Some((st, d)) = entry.take() {
                    if d == depth && (name == "item" || name == "entry") {
                        feed.entries.push(st.e);
                    } else {
                        entry = Some((st, d));
                    }
                }
                depth = depth.saturating_sub(1);
            }
            Event::Text(t) => {
                if let Some(c) = cap.as_mut() {
                    let s = AsRef::<str>::as_ref(&t).to_owned();
                    push_text(c, &s);
                }
            }
            Event::CData(t) => {
                if let Some(c) = cap.as_mut() {
                    let s = AsRef::<str>::as_ref(&t).to_owned();
                    push_text(c, &s);
                }
            }
            Event::GeneralRef(r) => {
                if let Some(c) = cap.as_mut() {
                    let name = AsRef::<str>::as_ref(&r).to_owned();
                    let s = match name.as_str() {
                        "lt" => "<".to_owned(),
                        "gt" => ">".to_owned(),
                        "amp" => "&".to_owned(),
                        "apos" => "'".to_owned(),
                        "quot" => "\"".to_owned(),
                        _ => html::py_unescape(&format!("&{name};")).unwrap_or_default(),
                    };
                    push_text(c, &s);
                }
            }
            _ => {}
        }
    }
    feed.bozo = bozo;
    feed
}

fn is_copyable(ctype: &str) -> bool {
    ctype == "text/plain" || HTML_TYPES.contains(&ctype)
}

fn push_text(c: &mut Capture, s: &str) {
    if c.ctype == "application/xhtml+xml" {
        c.text.push_str(&xml_escape(s));
    } else {
        c.text.push_str(s);
    }
}

/// Atom 1.0 xhtml: tümü tek `<div>` altındaysa sarmalayıcı atılır.
fn strip_div(raw: &str) -> String {
    let t = raw.trim_matches(osiris_hash::py_is_space);
    if (t.starts_with("<div>") || t.starts_with("<div ")) && t.ends_with("</div>") {
        let inner_start = t.find('>').map_or(0, |i| i + 1);
        return t[inner_start..t.len() - 6].to_owned();
    }
    raw.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rss_ve_atom_temel() {
        let rss = br#"<?xml version="1.0"?><rss version="2.0"><channel><title>K</title>
<item><title>A &amp; B</title><link> https://x.example/1 </link><description>&lt;p&gt;Merhaba &lt;script&gt;x()&lt;/script&gt;&lt;b&gt;d&#252;nya&lt;/b&gt;&lt;/p&gt;</description><pubDate>Mon, 05 Jan 2026 10:00:00 GMT</pubDate></item>
<item><guid>https://x.example/2</guid><content:encoded xmlns:content="http://purl.org/rss/1.0/modules/content/"><![CDATA[<i>c</i>]]></content:encoded></item>
</channel></rss>"#;
        let f = parse(rss);
        assert_eq!(f.entries.len(), 2);
        assert_eq!(f.entries[0].title.as_deref(), Some("A & B"));
        assert_eq!(f.entries[0].link.as_deref(), Some("https://x.example/1"));
        assert_eq!(f.entries[0].summary.as_deref(), Some("<p>Merhaba <b>dünya</b></p>"));
        assert_eq!(f.entries[0].published.as_deref(), Some("Mon, 05 Jan 2026 10:00:00 GMT"));
        assert_eq!(f.entries[1].link.as_deref(), Some("https://x.example/2"));
        assert_eq!(f.entries[1].summary.as_deref(), Some("<i>c</i>"));
        let atom = br#"<feed xmlns="http://www.w3.org/2005/Atom"><entry><title type="html">&lt;b&gt;T&lt;/b&gt;</title>
<link rel="self" href="https://s/"/><link href="https://a/"/><id>urn:x</id><published>2026-01-01T00:00:00Z</published>
<content type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml"><p>x</p></div></content></entry></feed>"#;
        let f = parse(atom);
        assert_eq!(f.entries[0].title.as_deref(), Some("<b>T</b>"));
        assert_eq!(f.entries[0].link.as_deref(), Some("https://a/"));
        assert_eq!(f.entries[0].summary.as_deref(), Some("<p>x</p>"));
    }
}
