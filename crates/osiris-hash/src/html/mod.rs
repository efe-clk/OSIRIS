//! `BeautifulSoup(text, "html.parser").get_text(" ", strip=True)` — birebir port.
//!
//! Referans: CPython 3.11.16 `html/parser.py` (`convert_charrefs=False`, bs4'ün kullandığı
//! kip) + beautifulsoup4 4.15.0 (`BeautifulSoupHTMLParser`, `BeautifulSoup.endData`,
//! `Tag._all_strings`). Sözleşme: docs/SOZLESME.md §1, `fixtures/hash/cases.json`.
//!
//! Python'daki yapı korunmuştur ki satır satır karşılaştırılabilsin:
//! - [`Parser::goahead`] ↔ `HTMLParser.goahead` (bs4 tek `feed()` + `close()` yapar:
//!   önce `goahead(0)`, kalan tampon için `goahead(1)` — iki geçiş davranışı etkiler).
//! - `scan_*` fonksiyonları modüldeki düzenli ifadelerin (tagfind_tolerant,
//!   attrfind_tolerant, locatetagend, charref, entityref) geri izlemeli eşleşme sonucunu
//!   elle hesaplar; Python `re` ile aynı uç noktayı döndürmeleri `test_native_parity.py`
//!   bulanık testleriyle denetlenir.
//! - [`Soup`] ↔ bs4 ağaç kurucusu: yalnızca metin çıktısını etkileyen durum tutulur
//!   (açık etiket yığını, dize kabı yığını, `already_closed_empty_element`).
//!
//! Konumlar Python gibi Unicode kod noktası dizinidir (`Vec<char>`).

pub mod select;
#[rustfmt::skip]
mod tables; // @generated — tools/gen_html_tables.py

use std::collections::HashMap;

/// Python `int()` ondalık dize sınırı (`sys.int_info.default_max_str_digits`).
/// bs4 daha uzun bir `&#…;` referansında `ValueError` ile çöker; çağıran ham metne düşer.
const PY_INT_MAX_STR_DIGITS: usize = 4300;

/// Ayrıştırıcı bs4'ün istisna fırlatacağı bir girdiyle karşılaştı (Python'da
/// `ProcessingPipeline.clean` bu durumda HTML soymadan ham metni kullanır).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlError(pub &'static str);

impl std::fmt::Display for HtmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for HtmlError {}

/// Python `str.isspace()` / `re` `\s` (Unicode) kümesi — `char::is_whitespace`'ten farklıdır
/// (U+001C–U+001F dahil). Bkz. SOZLESME §1 adım 3.
pub fn py_is_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}'
            | '\u{1c}'..='\u{20}'
            | '\u{85}'
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

/// Python `str.strip()` (argümansız).
pub fn py_strip(s: &str) -> &str {
    s.trim_matches(py_is_space)
}

/// Metin düğümlerini sırayla, kırpılmış ve boş olmayanları `" "` ile birleştirerek döndürür.
pub fn get_text(markup: &str) -> Result<String, HtmlError> {
    Ok(parse(markup)?.text(ROOT))
}

/// `BeautifulSoup(markup, "html.parser")` ağacı.
pub fn parse(markup: &str) -> Result<Document, HtmlError> {
    let mut parser = Parser::new(markup.chars().collect());
    // HTMLParser.feed(markup): eşik 1 olduğundan boş olmayan girdi hemen goahead(0) görür
    if !parser.raw.is_empty() {
        parser.goahead(false)?;
    }
    // HTMLParser.close(): kalan tampon için goahead(1)
    parser.goahead(true)?;
    // BeautifulSoup._feed: son endData + tüm etiketleri kapat (metin üretmez)
    parser.soup.end_data(Kind::Text);
    Ok(Document { nodes: parser.soup.nodes })
}

/// Kök düğüm (`[document]`).
pub const ROOT: usize = 0;

/// Ağaç düğümü.
#[derive(Debug, Clone)]
pub enum Node {
    Element {
        name: String,
        attrs: Vec<(String, String)>,
        children: Vec<usize>,
        parent: Option<usize>,
    },
    Text {
        kind: Kind,
        text: String,
        parent: usize,
    },
}

/// bs4 ağacı (yalnızca okuma).
#[derive(Debug, Clone)]
pub struct Document {
    pub nodes: Vec<Node>,
}

impl Document {
    /// Öğe adı (metin düğümünde None).
    pub fn name(&self, id: usize) -> Option<&str> {
        match &self.nodes[id] {
            Node::Element { name, .. } => Some(name),
            Node::Text { .. } => None,
        }
    }

    pub fn attr(&self, id: usize, key: &str) -> Option<&str> {
        match &self.nodes[id] {
            Node::Element { attrs, .. } => {
                attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
            }
            Node::Text { .. } => None,
        }
    }

    pub fn children(&self, id: usize) -> &[usize] {
        match &self.nodes[id] {
            Node::Element { children, .. } => children,
            Node::Text { .. } => &[],
        }
    }

    pub fn parent(&self, id: usize) -> Option<usize> {
        match &self.nodes[id] {
            Node::Element { parent, .. } => *parent,
            Node::Text { parent, .. } => Some(*parent),
        }
    }

    /// Belge sırasıyla alt öğeler (kendisi hariç).
    pub fn descendants(&self, id: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack: Vec<usize> = self.children(id).iter().rev().copied().collect();
        while let Some(n) = stack.pop() {
            out.push(n);
            stack.extend(self.children(n).iter().rev());
        }
        out
    }

    /// `tag.get_text(" ", strip=True)`: `interesting_string_types` kuralı — dize kabı
    /// adındaki öğe (script, style, template, rt, rp) yalnızca kendi türündeki dizeleri,
    /// diğerleri düz metin + CData'yı verir.
    pub fn text(&self, id: usize) -> String {
        let container =
            self.name(id).filter(|n| tables::STRING_CONTAINERS.contains(n)).map(str::to_owned);
        let mut out = String::new();
        for n in self.descendants(id) {
            let Node::Text { kind, text, .. } = &self.nodes[n] else { continue };
            let wanted = match (&container, kind) {
                (Some(c), Kind::Contained(k)) => k == c,
                (Some(_), _) => false,
                (None, Kind::Text | Kind::CData) => true,
                (None, _) => false,
            };
            if !wanted {
                continue;
            }
            let stripped = py_strip(text);
            if stripped.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(stripped);
        }
        out
    }

    /// `soup.find(name)` — belge sırasıyla ilk öğe.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.descendants(ROOT).into_iter().find(|&n| self.name(n) == Some(name))
    }

    /// `security.safe_title(soup)`: `<title>` metni (ilk 500 kod noktası) ya da None.
    pub fn safe_title(&self) -> Option<String> {
        let t = self.find("title")?;
        let text = self.text(t);
        if text.is_empty() { None } else { Some(text.chars().take(500).collect()) }
    }

    /// Öğe mi?
    pub fn is_element(&self, id: usize) -> bool {
        matches!(self.nodes[id], Node::Element { .. })
    }
}

// ----------------------------------------------------------------------------- bs4 ağacı

/// `endData(containerClass)` ile oluşan dize sınıfı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// NavigableString (dize kabı dışında)
    Text,
    /// Kap içindeki düz metin: Script/Stylesheet/TemplateString/RubyText… (kap adı)
    Contained(String),
    Comment,
    CData,
    Doctype,
    Declaration,
    Pi,
}

struct Soup {
    current: Vec<String>,
    nodes: Vec<Node>,
    /// Açık öğelerin düğüm dizinleri (kök dahil).
    stack: Vec<usize>,
    open_count: HashMap<String, usize>,
    /// `string_container_stack` (düğüm dizinleri).
    containers: Vec<usize>,
}

impl Default for Soup {
    fn default() -> Self {
        Self {
            current: Vec::new(),
            nodes: vec![Node::Element {
                name: "[document]".into(),
                attrs: Vec::new(),
                children: Vec::new(),
                parent: None,
            }],
            stack: vec![ROOT],
            open_count: HashMap::new(),
            containers: Vec::new(),
        }
    }
}

impl Soup {
    fn handle_data(&mut self, data: String) {
        self.current.push(data);
    }

    fn top(&self) -> usize {
        *self.stack.last().unwrap_or(&ROOT)
    }

    fn append(&mut self, node: Node) -> usize {
        let id = self.nodes.len();
        let parent = self.top();
        self.nodes.push(node);
        if let Node::Element { children, .. } = &mut self.nodes[parent] {
            children.push(id);
        }
        id
    }

    fn end_data(&mut self, kind: Kind) {
        if self.current.is_empty() {
            return;
        }
        let data: String = self.current.concat();
        self.current.clear();
        // Yalnızca ASCII boşluktan oluşan dize " " / "\n" olur — kırpınca zaten boşalır.
        // string_container(): sınıf açıkça verilmediyse (NavigableString) en üstteki kap belirler.
        let kind = match kind {
            Kind::Text => match self.containers.last().and_then(|&c| self.name_of(c)) {
                Some(c) => Kind::Contained(c),
                None => Kind::Text,
            },
            other => other,
        };
        let parent = self.top();
        self.append(Node::Text { kind, text: data, parent });
    }

    fn name_of(&self, id: usize) -> Option<String> {
        match &self.nodes[id] {
            Node::Element { name, .. } => Some(name.clone()),
            Node::Text { .. } => None,
        }
    }

    fn push_tag(&mut self, name: &str, attrs: Vec<(String, String)>) {
        let parent = Some(self.top());
        let id = self.append(Node::Element {
            name: name.to_owned(),
            attrs,
            children: Vec::new(),
            parent,
        });
        self.stack.push(id);
        *self.open_count.entry(name.to_owned()).or_insert(0) += 1;
        if tables::STRING_CONTAINERS.contains(&name) {
            self.containers.push(id);
        }
    }

    fn pop_tag(&mut self) -> Option<String> {
        if self.stack.len() <= 1 {
            return None;
        }
        let id = self.stack.pop()?;
        let name = self.name_of(id)?;
        if let Some(c) = self.open_count.get_mut(&name) {
            *c -= 1;
        }
        if self.containers.last() == Some(&id) {
            self.containers.pop();
        }
        Some(name)
    }

    /// `_popToTag(name)`: o adla açık etiket yoksa hiçbir şey yapmaz.
    fn pop_to_tag(&mut self, name: &str) {
        if self.open_count.get(name).copied().unwrap_or(0) == 0 {
            return;
        }
        while let Some(top) = self.pop_tag() {
            if top == name {
                break;
            }
        }
    }

    fn handle_starttag(&mut self, name: &str, attrs: Vec<(String, String)>) {
        self.end_data(Kind::Text);
        self.push_tag(name, attrs);
    }

    fn handle_endtag(&mut self, name: &str) {
        self.end_data(Kind::Text);
        self.pop_to_tag(name);
    }
}

// ------------------------------------------------------------------------ html.parser

#[derive(Clone, PartialEq, Eq)]
enum Interesting {
    /// `[&<]`
    Normal,
    /// `</elem(?=[\t\n\r\f />])` (IGNORECASE|ASCII)
    Cdata(Vec<char>),
    /// `&|</elem(?=[\t\n\r\f />])` — RCDATA (textarea, title) ve convert_charrefs=False
    Rcdata(Vec<char>),
    /// `\Z`
    Plaintext,
}

const CDATA_CONTENT_ELEMENTS: &[&str] =
    &["script", "style", "xmp", "iframe", "noembed", "noframes"];
const RCDATA_CONTENT_ELEMENTS: &[&str] = &["textarea", "title"];

struct Parser {
    raw: Vec<char>,
    interesting: Interesting,
    soup: Soup,
    already_closed_empty_element: Vec<String>,
}

/// `[\t\n\r\f ]` — html.parser'ın boşluk sınıfı (\v yok).
fn is_ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{c}' | ' ')
}

fn is_lookbehind(c: char) -> bool {
    matches!(c, '\'' | '"' | '\t' | '\n' | '\r' | '\u{c}' | ' ' | '/')
}

fn starts_with(c: &[char], i: usize, pat: &str) -> bool {
    (i..).zip(pat.chars()).all(|(k, p)| c.get(k) == Some(&p))
}

fn find(c: &[char], ch: char, from: usize) -> Option<usize> {
    c.get(from..)?.iter().position(|&x| x == ch).map(|p| p + from)
}

fn find_str(c: &[char], pat: &str, from: usize) -> Option<usize> {
    (from..c.len()).find(|&k| starts_with(c, k, pat))
}

fn s(c: &[char]) -> String {
    c.iter().collect()
}

/// Python dilimleme: `rawdata[a:b]` (sınır dışı uçlar kırpılır, a > b → boş).
fn slice(c: &[char], a: usize, b: usize) -> String {
    let b = b.min(c.len());
    if a >= b { String::new() } else { s(&c[a..b]) }
}

fn starttagopen(c: &[char], i: usize) -> bool {
    c.get(i) == Some(&'<') && c.get(i + 1).is_some_and(|x| x.is_ascii_alphabetic())
}

fn endtagopen(c: &[char], i: usize) -> bool {
    starts_with(c, i, "</") && c.get(i + 2).is_some_and(|x| x.is_ascii_alphabetic())
}

/// Ad `[a-zA-Z][^\t\n\r\f />]*`; `p` harfi gösterir. Adın bitişi.
fn scan_tag_name(c: &[char], p: usize) -> usize {
    let mut k = p + 1;
    while k < c.len() && !is_ws(c[k]) && c[k] != '/' && c[k] != '>' {
        k += 1;
    }
    k
}

/// `(?:[\t\n\r\f ]|/(?!>))*`
fn scan_tolerant_sep(c: &[char], mut k: usize) -> usize {
    while k < c.len() && (is_ws(c[k]) || (c[k] == '/' && c.get(k + 1) != Some(&'>'))) {
        k += 1;
    }
    k
}

/// `[\t\n\r\f /]*`
fn scan_sep(c: &[char], mut k: usize) -> usize {
    while k < c.len() && (is_ws(c[k]) || c[k] == '/') {
        k += 1;
    }
    k
}

/// Öznitelik adı `(?<=['"\t\n\r\f /])[^\t\n\r\f />][^\t\n\r\f /=>]*`; bitiş ya da None.
fn scan_attr_name(c: &[char], k: usize) -> Option<usize> {
    if k == 0 || k >= c.len() || !is_lookbehind(c[k - 1]) {
        return None;
    }
    if is_ws(c[k]) || c[k] == '/' || c[k] == '>' {
        return None;
    }
    let mut m = k + 1;
    while m < c.len() && !is_ws(c[m]) && !matches!(c[m], '/' | '=' | '>') {
        m += 1;
    }
    Some(m)
}

/// İsteğe bağlı değer grubu:
/// `[\t\n\r\f ]*=[\t\n\r\f ]*('[^']*'|"[^"]*"|(?!['"])[^>\t\n\r\f ]*)` — geri izleme dahil.
/// (grup sonu, değer aralığı) döner.
fn scan_attr_value(c: &[char], m: usize) -> Option<(usize, (usize, usize))> {
    let n = c.len();
    let mut q = m;
    while q < n && is_ws(c[q]) {
        q += 1;
    }
    if q >= n || c[q] != '=' {
        return None;
    }
    let mut r = q + 1;
    while r < n && is_ws(c[r]) {
        r += 1;
    }
    if r < n && (c[r] == '\'' || c[r] == '"') {
        if let Some(close) = find(c, c[r], r + 1) {
            return Some((close + 1, (r, close + 1)));
        }
        // Kapanmayan tırnak: '=' sonrası boşluk bir geri alınır, çıplak değer boş eşleşir
        return if r > q + 1 { Some((r - 1, (r - 1, r - 1))) } else { None };
    }
    let mut e = r;
    while e < n && c[e] != '>' && !is_ws(c[e]) {
        e += 1;
    }
    Some((e, (r, e)))
}

/// `tagfind_tolerant.match(rawdata, p)` → (küçük harfli ad, bitiş).
fn scan_tagfind(c: &[char], p: usize) -> (String, usize) {
    let name_end = scan_tag_name(c, p);
    (s(&c[p..name_end]).to_lowercase(), scan_tolerant_sep(c, name_end))
}

/// `attrfind_tolerant.match(rawdata, k)` → (son, ad aralığı, değer aralığı).
#[allow(clippy::type_complexity)]
fn scan_attrfind(c: &[char], k: usize) -> Option<(usize, (usize, usize), Option<(usize, usize)>)> {
    let m = scan_attr_name(c, k)?;
    let (end, value) = match scan_attr_value(c, m) {
        Some((e, v)) => (e, Some(v)),
        None => (m, None),
    };
    Some((scan_tolerant_sep(c, end), (k, m), value))
}

/// `locatetagend.match(rawdata, p).end()`; `p` bir harfi gösterir.
fn scan_locatetagend(c: &[char], p: usize) -> usize {
    let mut k = scan_sep(c, scan_tag_name(c, p));
    while let Some(m) = scan_attr_name(c, k) {
        let m = scan_attr_value(c, m).map_or(m, |(e, _)| e);
        k = scan_sep(c, m);
    }
    if c.get(k) == Some(&'>') {
        k += 1;
    }
    k
}

/// `charref.match(rawdata, i)` → eşleşme sonu. `&#(?:[0-9]+|[xX][0-9a-fA-F]+)[^0-9a-fA-F]`
fn scan_charref(c: &[char], i: usize) -> Option<usize> {
    let n = c.len();
    let p = i + 2;
    let mut q = p;
    while q < n && c[q].is_ascii_digit() {
        q += 1;
    }
    if q > p {
        // geri izlenen konumlar rakamdır (onaltılık) → sonlandırıcı yalnızca q olabilir
        return (q < n && !c[q].is_ascii_hexdigit()).then_some(q + 1);
    }
    if p < n && (c[p] == 'x' || c[p] == 'X') {
        let mut q = p + 1;
        while q < n && c[q].is_ascii_hexdigit() {
            q += 1;
        }
        if q > p + 1 && q < n && !c[q].is_ascii_hexdigit() {
            return Some(q + 1);
        }
    }
    None
}

/// `entityref.match(rawdata, i)` → (ad, eşleşme sonu). `&([a-zA-Z][-.a-zA-Z0-9]*)[^a-zA-Z0-9]`
fn scan_entityref(c: &[char], i: usize) -> Option<(String, usize)> {
    let n = c.len();
    let p = i + 1;
    if p >= n || !c[p].is_ascii_alphabetic() {
        return None;
    }
    let mut q = p + 1;
    while q < n && (c[q].is_ascii_alphanumeric() || c[q] == '-' || c[q] == '.') {
        q += 1;
    }
    if q < n {
        return Some((s(&c[p..q]), q + 1));
    }
    // Metin sonu: geri izleme son '-' / '.' karakterini sonlandırıcı yapar
    (p + 1..q).rev().find(|&t| c[t] == '-' || c[t] == '.').map(|t| (s(&c[p..t]), t + 1))
}

impl Parser {
    fn new(raw: Vec<char>) -> Self {
        Self {
            raw,
            interesting: Interesting::Normal,
            soup: Soup::default(),
            already_closed_empty_element: Vec::new(),
        }
    }

    // ---- BeautifulSoupHTMLParser olay işleyicileri

    fn handle_starttag(
        &mut self,
        tag: &str,
        attrs: Vec<(String, String)>,
        handle_empty_element: bool,
    ) {
        self.soup.handle_starttag(tag, attrs);
        if handle_empty_element && tables::EMPTY_ELEMENT_TAGS.contains(&tag) {
            self.handle_endtag(tag, false);
            self.already_closed_empty_element.push(tag.to_owned());
        }
    }

    fn handle_startendtag(&mut self, tag: &str, attrs: Vec<(String, String)>) {
        self.handle_starttag(tag, attrs, false);
        self.handle_endtag(tag, false);
    }

    fn handle_endtag(&mut self, tag: &str, check_already_closed: bool) {
        if check_already_closed
            && let Some(pos) = self.already_closed_empty_element.iter().position(|t| t == tag)
        {
            self.already_closed_empty_element.remove(pos);
            return;
        }
        self.soup.handle_endtag(tag);
    }

    fn handle_data(&mut self, data: String) {
        self.soup.handle_data(data);
    }

    fn handle_charref(&mut self, name: &str) -> Result<(), HtmlError> {
        // `name` regex'ten gelir: yalnızca rakam ya da x/X + onaltılık; int() hep başarılı,
        // ondalık 4300 haneyi aşınca CPython ValueError verir (bs4 bunu yakalayamaz).
        let (digits, radix) = match name.strip_prefix(['x', 'X']) {
            Some(hex) => (hex, 16),
            None => (name, 10),
        };
        if radix == 10 && digits.len() > PY_INT_MAX_STR_DIGITS {
            return Err(HtmlError("sayısal karakter referansı çok uzun"));
        }
        let trimmed = digits.trim_start_matches('0');
        // 0x10FFFF'ten büyük her değer aynı sonucu verir; taşmayı önlemek için kırp
        let value = if trimmed.len() > 8 {
            u32::MAX
        } else {
            u32::from_str_radix(if trimmed.is_empty() { "0" } else { trimmed }, radix)
                .unwrap_or(u32::MAX)
        };
        let ch = numeric_character_reference(value);
        self.handle_data(ch);
        // extra_data "" — bs4 yine de handle_data("") çağırır (current_data boş kalmaz)
        self.handle_data(String::new());
        Ok(())
    }

    fn handle_entityref(&mut self, name: &str) {
        let data = match tables::ENTITIES.binary_search_by(|(k, _)| (*k).cmp(name)) {
            Ok(idx) => tables::ENTITIES[idx].1.to_owned(),
            Err(_) => format!("&{name}"),
        };
        self.handle_data(data);
    }

    fn handle_special(&mut self, data: String, kind: Kind) {
        self.soup.end_data(Kind::Text);
        self.soup.handle_data(data);
        self.soup.end_data(kind);
    }

    fn handle_comment(&mut self, data: String) {
        self.handle_special(data, Kind::Comment);
    }

    fn handle_decl(&mut self, decl: String) {
        // decl[len("DOCTYPE "):]
        let rest: String = decl.chars().skip(8).collect();
        self.handle_special(rest, Kind::Doctype);
    }

    fn unknown_decl(&mut self, data: String) {
        if data.to_uppercase().starts_with("CDATA[") {
            let rest: String = data.chars().skip(6).collect();
            self.handle_special(rest, Kind::CData);
        } else {
            self.handle_special(data, Kind::Declaration);
        }
    }

    fn handle_pi(&mut self, data: String) {
        self.handle_special(data, Kind::Pi);
    }

    // ---- HTMLParser

    fn set_cdata_mode(&mut self, elem: &str, escapable: bool) {
        let e: Vec<char> = elem.to_lowercase().chars().collect();
        self.interesting = if elem == "plaintext" {
            Interesting::Plaintext
        } else if escapable {
            Interesting::Rcdata(e)
        } else {
            Interesting::Cdata(e)
        };
    }

    fn clear_cdata_mode(&mut self) {
        self.interesting = Interesting::Normal;
    }

    fn in_cdata(&self) -> bool {
        self.interesting != Interesting::Normal
    }

    /// `</elem(?=[\t\n\r\f />])` k konumunda mı (ASCII büyük/küçük harf duyarsız)?
    fn at_cdata_end(c: &[char], k: usize, elem: &[char]) -> bool {
        if !starts_with(c, k, "</") {
            return false;
        }
        let mut p = k + 2;
        for &e in elem {
            match c.get(p) {
                Some(x) if x.eq_ignore_ascii_case(&e) => p += 1,
                _ => return false,
            }
        }
        c.get(p).is_some_and(|&x| is_ws(x) || x == '/' || x == '>')
    }

    /// `self.interesting.search(rawdata, i)` → eşleşme başlangıcı.
    fn search_interesting(&self, i: usize) -> Option<usize> {
        let c = &self.raw;
        match &self.interesting {
            Interesting::Normal => (i..c.len()).find(|&k| c[k] == '<' || c[k] == '&'),
            Interesting::Cdata(e) => (i..c.len()).find(|&k| Self::at_cdata_end(c, k, e)),
            Interesting::Rcdata(e) => {
                (i..c.len()).find(|&k| c[k] == '&' || Self::at_cdata_end(c, k, e))
            }
            Interesting::Plaintext => Some(c.len().max(i)),
        }
    }

    fn goahead(&mut self, end: bool) -> Result<(), HtmlError> {
        let mut i = 0usize;
        let n = self.raw.len();
        while i < n {
            let j = match self.search_interesting(i) {
                Some(j) => j,
                None if self.in_cdata() => break,
                None => n,
            };
            if i < j {
                let data = s(&self.raw[i..j]);
                self.handle_data(data);
            }
            i = j;
            if i == n {
                break;
            }
            let c = &self.raw;
            if c[i] == '<' {
                let mut k: isize = if starttagopen(c, i) {
                    self.parse_starttag(i)?
                } else if starts_with(c, i, "</") {
                    self.parse_endtag(i)
                } else if starts_with(c, i, "<!--") {
                    self.parse_comment(i)
                } else if starts_with(c, i, "<?") {
                    self.parse_pi(i)
                } else if starts_with(c, i, "<!") {
                    self.parse_html_declaration(i)
                } else if i + 1 < n || end {
                    self.handle_data("<".to_owned());
                    (i + 1) as isize
                } else {
                    break;
                };
                if k < 0 {
                    if !end {
                        break;
                    }
                    let c = &self.raw;
                    if starttagopen(c, i) {
                        // tamamlanmamış başlangıç etiketi: kalan her şey atılır
                    } else if starts_with(c, i, "</") {
                        if i + 2 == n {
                            self.handle_data("</".to_owned());
                        } else if endtagopen(c, i) {
                            // tamamlanmamış bitiş etiketi: atılır
                        } else {
                            let data = slice(c, i + 2, n);
                            self.handle_comment(data);
                        }
                    } else if starts_with(c, i, "<!--") {
                        let tail = &c[(i + 4).min(n)..];
                        let mut j = n;
                        for suffix in ["--!", "--", "-"] {
                            let sfx: Vec<char> = suffix.chars().collect();
                            if tail.ends_with(&sfx) {
                                j -= sfx.len();
                                break;
                            }
                        }
                        let data = slice(c, i + 4, j);
                        self.handle_comment(data);
                    } else if starts_with(c, i, "<![CDATA[") {
                        let data = slice(c, i + 3, n);
                        self.unknown_decl(data);
                    } else if slice(c, i, i + 9).to_lowercase() == "<!doctype" {
                        let data = slice(c, i + 2, n);
                        self.handle_decl(data);
                    } else if starts_with(c, i, "<!") {
                        let data = slice(c, i + 2, n);
                        self.handle_comment(data);
                    } else if starts_with(c, i, "<?") {
                        let data = slice(c, i + 2, n);
                        self.handle_pi(data);
                    } else {
                        unreachable!("html.parser: we should not get here!");
                    }
                    k = n as isize;
                }
                i = k as usize;
            } else if starts_with(c, i, "&#") {
                if let Some(mend) = scan_charref(c, i) {
                    let name = s(&c[i + 2..mend - 1]);
                    let k = if c[mend - 1] == ';' { mend } else { mend - 1 };
                    self.handle_charref(&name)?;
                    i = k;
                    continue;
                }
                if c[i..].contains(&';') {
                    // "&#" tüketilir, kalan sonraki geçişe bırakılır
                    self.handle_data("&#".to_owned());
                    i += 2;
                }
                break;
            } else if c[i] == '&' {
                if let Some((name, mend)) = scan_entityref(c, i) {
                    let k = if c[mend - 1] == ';' { mend } else { mend - 1 };
                    self.handle_entityref(&name);
                    i = k;
                    continue;
                }
                // incomplete = '&[a-zA-Z#]'
                if c.get(i + 1).is_some_and(|x| x.is_ascii_alphabetic() || *x == '#') {
                    if end && i + 2 == n {
                        i += 1;
                    }
                    break;
                } else if i + 1 < n {
                    self.handle_data("&".to_owned());
                    i += 1;
                } else {
                    break;
                }
            } else {
                unreachable!("html.parser: interesting.search() lied");
            }
        }
        if end && i < n {
            let data = s(&self.raw[i..n]);
            self.handle_data(data);
            i = n;
        }
        self.raw.drain(..i.min(self.raw.len()));
        Ok(())
    }

    fn parse_html_declaration(&mut self, i: usize) -> isize {
        let c = &self.raw;
        if starts_with(c, i, "<!--") {
            return self.parse_comment(i);
        }
        if starts_with(c, i, "<![CDATA[") {
            let Some(j) = find_str(c, "]]>", i + 9) else { return -1 };
            let data = slice(c, i + 3, j);
            self.unknown_decl(data);
            return (j + 3) as isize;
        }
        if slice(c, i, i + 9).to_lowercase() == "<!doctype" {
            let Some(gt) = find(c, '>', i + 9) else { return -1 };
            let data = slice(c, i + 2, gt);
            self.handle_decl(data);
            return (gt + 1) as isize;
        }
        if starts_with(c, i, "<![") {
            let Some(j) = find(c, '>', i + 3) else { return -1 };
            if c[j - 1] == ']' {
                let data = slice(c, i + 3, j - 1);
                self.unknown_decl(data);
            } else {
                let data = slice(c, i + 2, j);
                self.handle_comment(data);
            }
            return (j + 1) as isize;
        }
        self.parse_bogus_comment(i)
    }

    /// `--!?>` araması, yoksa `-?>` (i+4'te) eşleşmesi.
    fn parse_comment(&mut self, i: usize) -> isize {
        let c = &self.raw;
        let n = c.len();
        let close = (i + 4..n).find_map(|s0| {
            if c.get(s0) == Some(&'-') && c.get(s0 + 1) == Some(&'-') {
                match (c.get(s0 + 2), c.get(s0 + 3)) {
                    (Some('>'), _) => Some((s0, s0 + 3)),
                    (Some('!'), Some('>')) => Some((s0, s0 + 4)),
                    _ => None,
                }
            } else {
                None
            }
        });
        let close = close.or_else(|| {
            let p = i + 4;
            match (c.get(p), c.get(p + 1)) {
                (Some('-'), Some('>')) => Some((p, p + 2)),
                (Some('>'), _) => Some((p, p + 1)),
                _ => None,
            }
        });
        let Some((start, endpos)) = close else { return -1 };
        let data = slice(c, i + 4, start);
        self.handle_comment(data);
        endpos as isize
    }

    fn parse_bogus_comment(&mut self, i: usize) -> isize {
        let c = &self.raw;
        let Some(pos) = find(c, '>', i + 2) else { return -1 };
        let data = slice(c, i + 2, pos);
        self.handle_comment(data);
        (pos + 1) as isize
    }

    fn parse_pi(&mut self, i: usize) -> isize {
        let c = &self.raw;
        let Some(j) = find(c, '>', i + 2) else { return -1 };
        let data = slice(c, i + 2, j);
        self.handle_pi(data);
        (j + 1) as isize
    }

    fn parse_starttag(&mut self, i: usize) -> Result<isize, HtmlError> {
        let c = &self.raw;
        let endpos = scan_locatetagend(c, i + 1);
        if c[endpos - 1] != '>' {
            return Ok(-1);
        }
        let (tag, mut k) = scan_tagfind(c, i + 1);
        let mut attrs: Vec<(String, Option<String>)> = Vec::new();
        while k < endpos {
            let Some((m, (ns, ne), value)) = scan_attrfind(c, k) else { break };
            let value = match value {
                None => None,
                Some((vs, ve)) => {
                    let mut v: Vec<char> = c[vs..ve].to_vec();
                    if v.len() >= 2 && (v[0] == '\'' || v[0] == '"') && v[v.len() - 1] == v[0] {
                        v = v[1..v.len() - 1].to_vec();
                    } else if v.len() == 1 && (v[0] == '\'' || v[0] == '"') {
                        v.clear();
                    }
                    let raw: String = v.iter().collect();
                    Some(if raw.is_empty() { raw } else { py_unescape(&raw)? })
                }
            };
            attrs.push((s(&c[ns..ne]).to_lowercase(), value));
            k = m;
        }
        let rest = slice(c, k, endpos);
        let end = py_strip(&rest);
        if end != ">" && end != "/>" {
            let data = slice(c, i, endpos);
            self.handle_data(data);
            return Ok(endpos as isize);
        }
        // bs4: None → "", tekrarlanan anahtar REPLACE (ilk konum, son değer)
        let mut dict: Vec<(String, String)> = Vec::new();
        for (key, value) in attrs {
            let value = value.unwrap_or_default();
            match dict.iter_mut().find(|(k2, _)| *k2 == key) {
                Some(slot) => slot.1 = value,
                None => dict.push((key, value)),
            }
        }
        if end.ends_with("/>") {
            self.handle_startendtag(&tag, dict);
        } else {
            self.handle_starttag(&tag, dict, true);
            if CDATA_CONTENT_ELEMENTS.contains(&tag.as_str()) || tag == "plaintext" {
                self.set_cdata_mode(&tag, false);
            } else if RCDATA_CONTENT_ELEMENTS.contains(&tag.as_str()) {
                self.set_cdata_mode(&tag, true);
            }
        }
        Ok(endpos as isize)
    }

    fn parse_endtag(&mut self, i: usize) -> isize {
        let c = &self.raw;
        if find(c, '>', i + 2).is_none() {
            return -1;
        }
        if !endtagopen(c, i) {
            if c.get(i + 2) == Some(&'>') {
                return (i + 3) as isize; // "</>" yok sayılır
            }
            return self.parse_bogus_comment(i);
        }
        let j = scan_locatetagend(c, i + 2);
        if c[j - 1] != '>' {
            return -1;
        }
        let (tag, _) = scan_tagfind(c, i + 2);
        self.handle_endtag(&tag, true);
        self.clear_cdata_mode();
        j as isize
    }
}

/// Python `html.unescape` (öznitelik değerleri; HTML5 adlandırılmış referans kuralları).
/// `_charref = &(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\f <&#;]{1,32};?)`
pub fn py_unescape(s: &str) -> Result<String, HtmlError> {
    if !s.contains('&') {
        return Ok(s.to_owned());
    }
    let c: Vec<char> = s.chars().collect();
    let n = c.len();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < n {
        if c[i] != '&' {
            out.push(c[i]);
            i += 1;
            continue;
        }
        let p = i + 1;
        // sayısal
        if p < n && c[p] == '#' {
            let (hex, start) = if p + 1 < n && (c[p + 1] == 'x' || c[p + 1] == 'X') {
                (true, p + 2)
            } else {
                (false, p + 1)
            };
            let mut q = start;
            while q < n && (if hex { c[q].is_ascii_hexdigit() } else { c[q].is_ascii_digit() }) {
                q += 1;
            }
            if q > start {
                let digits: String = c[start..q].iter().collect();
                if !hex && digits.len() > PY_INT_MAX_STR_DIGITS {
                    return Err(HtmlError("sayısal karakter referansı çok uzun"));
                }
                let trimmed = digits.trim_start_matches('0');
                let num = if trimmed.len() > 8 {
                    u32::MAX
                } else {
                    u32::from_str_radix(
                        if trimmed.is_empty() { "0" } else { trimmed },
                        if hex { 16 } else { 10 },
                    )
                    .unwrap_or(u32::MAX)
                };
                let end = if q < n && c[q] == ';' { q + 1 } else { q };
                if let Some((_, v)) = tables::INVALID_CHARREFS.iter().find(|(k, _)| *k == num) {
                    out.push_str(v);
                } else if (0xd800..=0xdfff).contains(&num) || num > 0x10ffff {
                    out.push('\u{fffd}');
                } else if tables::INVALID_CODEPOINTS.contains(&num) {
                } else if let Some(ch) = char::from_u32(num) {
                    out.push(ch);
                }
                i = end;
                continue;
            }
            out.push('&');
            i += 1;
            continue;
        }
        // adlandırılmış: [^\t\n\f <&#;]{1,32};?
        let mut q = p;
        while q < n
            && q - p < 32
            && !matches!(c[q], '\t' | '\n' | '\u{c}' | ' ' | '<' | '&' | '#' | ';')
        {
            q += 1;
        }
        if q == p {
            out.push('&');
            i += 1;
            continue;
        }
        let end = if q < n && c[q] == ';' { q + 1 } else { q };
        let name: String = c[p..end].iter().collect();
        let lookup = |k: &str| {
            tables::HTML5.binary_search_by(|(e, _)| (*e).cmp(k)).ok().map(|i| tables::HTML5[i].1)
        };
        if let Some(v) = lookup(&name) {
            out.push_str(v);
        } else {
            let chars: Vec<char> = name.chars().collect();
            let mut done = false;
            for x in (2..chars.len()).rev() {
                let pre: String = chars[..x].iter().collect();
                if let Some(v) = lookup(&pre) {
                    out.push_str(v);
                    out.extend(&chars[x..]);
                    done = true;
                    break;
                }
            }
            if !done {
                out.push('&');
                out.push_str(&name);
            }
        }
        i = end;
    }
    Ok(out)
}

/// bs4 `UnicodeDammit.numeric_character_reference` (HTML "numeric character reference end state").
fn numeric_character_reference(numeric: u32) -> String {
    const REPLACEMENT: &str = "\u{fffd}";
    if numeric == 0 || numeric > 0x10ffff || (0xd800..=0xdfff).contains(&numeric) {
        return REPLACEMENT.to_owned();
    }
    if (0xfdd0..=0xfdef).contains(&numeric) || tables::NONCHARACTERS.contains(&numeric) {
        return char::from_u32(numeric).map_or_else(|| REPLACEMENT.to_owned(), String::from);
    }
    if (0x80..=0x9f).contains(&numeric)
        && let Some((_, v)) = tables::WINDOWS_1252.iter().find(|(k, _)| *k == numeric)
    {
        return (*v).to_owned();
    }
    char::from_u32(numeric).map_or_else(|| REPLACEMENT.to_owned(), String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> String {
        get_text(s).unwrap()
    }

    #[test]
    fn temel() {
        assert_eq!(t("a<br>b<b>c</b>d"), "a b c d");
        assert_eq!(t("<p>x</p><script>var a = 1;</script><style>p{}</style>"), "x");
        assert_eq!(t("a &amp b &foo; c"), "a & b &foo c");
        assert_eq!(t("<<>>x<>"), "<<>>x<>");
    }

    #[test]
    fn tamamlanmamis_baslangic_etiketi_kalan_metni_atar() {
        assert_eq!(t("x <a b='y>metin"), "x");
    }

    #[test]
    fn cok_uzun_ondalik_referans_hata() {
        let s = format!("&#{};", "1".repeat(4301));
        assert!(get_text(&s).is_err());
        let s = format!("&#{};", "1".repeat(4300));
        assert!(get_text(&s).is_ok());
    }

    #[test]
    fn sablon_ve_ruby_metni_atilir() {
        assert_eq!(t("a<template><p>gizli</p></template>b"), "a b");
        assert_eq!(t("<ruby>漢<rt>kan</rt></ruby>"), "漢");
    }
}
