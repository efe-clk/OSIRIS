//! CSS seçici — soupsieve (`soup.select`) semantiğinin desteklenen alt kümesi.
//!
//! Destek: tür / `*`, `#id`, `.class`, `[a]`, `[a=v]`, `~=`, `|=`, `^=`, `$=`, `*=`, `!=`
//! (+ ` i` / ` s` bayrağı), birleştiriciler (boşluk, `>`, `+`, `~`), seçici listesi (`,`),
//! `:first-child`, `:last-child`, `:only-child`, `:first-of-type`, `:last-of-type`,
//! `:only-of-type`, `:empty`, `:root`, `:nth-child()`, `:nth-last-child()`, `:nth-of-type()`,
//! `:nth-last-of-type()` (`an+b`, `odd`, `even`), `:not()`, `:is()`, `:where()`.
//! Diğer her şey [`SelectError::Unsupported`] (çağıran Python'daki hata dalı gibi tüm
//! belgeye düşer). HTML belgesinde tür ve öznitelik adları büyük/küçük harf duyarsızdır;
//! `class` gibi çok değerli öznitelikler Python boşluğuna göre bölünür (bs4).

use super::{Document, Node, ROOT, py_is_space};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectError {
    Invalid(String),
    Unsupported(String),
}

#[derive(Debug, Clone)]
enum AttrOp {
    Exists,
    Eq,
    Word,
    Dash,
    Prefix,
    Suffix,
    Contains,
    NotEq,
}

#[derive(Debug, Clone)]
enum Simple {
    Universal,
    Type(String),
    Id(String),
    Class(String),
    Attr { name: String, op: AttrOp, value: String, ci: bool },
    Nth { a: i64, b: i64, last: bool, of_type: bool },
    Empty,
    Root,
    Not(Vec<Complex>),
    Is(Vec<Complex>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Comb {
    Descendant,
    Child,
    Adjacent,
    Sibling,
}

/// Sağdan sola eşleşen bileşik dizisi: (bileşik, kendisinden önceki birleştirici).
#[derive(Debug, Clone)]
struct Complex(Vec<(Vec<Simple>, Option<Comb>)>);

struct P<'a> {
    c: Vec<char>,
    i: usize,
    src: &'a str,
}

impl P<'_> {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn ws(&mut self) -> bool {
        let st = self.i;
        while self.peek().is_some_and(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')) {
            self.i += 1;
        }
        self.i > st
    }

    fn invalid(&self, why: &str) -> SelectError {
        SelectError::Invalid(format!("{why}: {:?} @{}", self.src, self.i))
    }

    fn ident(&mut self) -> Result<String, SelectError> {
        let mut out = String::new();
        let start = self.i;
        while let Some(c) = self.peek() {
            if c == '\\' {
                self.i += 1;
                let Some(e) = self.peek() else { return Err(self.invalid("kaçış")) };
                if e.is_ascii_hexdigit() {
                    let mut hex = String::new();
                    while hex.len() < 6 && self.peek().is_some_and(|x| x.is_ascii_hexdigit()) {
                        hex.push(self.peek().unwrap_or('0'));
                        self.i += 1;
                    }
                    if self.peek() == Some(' ') {
                        self.i += 1;
                    }
                    out.push(
                        u32::from_str_radix(&hex, 16)
                            .ok()
                            .and_then(char::from_u32)
                            .unwrap_or('\u{fffd}'),
                    );
                } else {
                    out.push(e);
                    self.i += 1;
                }
            } else if c.is_alphanumeric() || c == '-' || c == '_' || (c as u32) >= 0x80 {
                out.push(c);
                self.i += 1;
            } else {
                break;
            }
        }
        let first_ok = self.c.get(start).is_some_and(|&f| !f.is_ascii_digit())
            && !(out.starts_with('-') && out.chars().nth(1).is_some_and(|x| x.is_ascii_digit()));
        if out.is_empty() || !first_ok {
            return Err(self.invalid("tanımlayıcı"));
        }
        Ok(out)
    }

    fn string(&mut self) -> Result<String, SelectError> {
        let q = self.peek().ok_or_else(|| self.invalid("dize"))?;
        self.i += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err(self.invalid("kapanmayan dize")),
                Some(c) if c == q => {
                    self.i += 1;
                    return Ok(out);
                }
                Some('\\') => {
                    self.i += 1;
                    if let Some(e) = self.peek() {
                        out.push(e);
                        self.i += 1;
                    }
                }
                Some(c) => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
    }

    fn list(&mut self) -> Result<Vec<Complex>, SelectError> {
        let mut out = Vec::new();
        loop {
            self.ws();
            out.push(self.complex()?);
            self.ws();
            match self.peek() {
                Some(',') => self.i += 1,
                _ => return Ok(out),
            }
        }
    }

    fn complex(&mut self) -> Result<Complex, SelectError> {
        let mut parts: Vec<(Vec<Simple>, Option<Comb>)> = Vec::new();
        let mut comb: Option<Comb> = None;
        loop {
            let compound = self.compound()?;
            if compound.is_empty() {
                return Err(self.invalid("boş seçici"));
            }
            parts.push((compound, comb));
            let had_ws = self.ws();
            comb = match self.peek() {
                Some('>') => Some(Comb::Child),
                Some('+') => Some(Comb::Adjacent),
                Some('~') => Some(Comb::Sibling),
                Some(',') | Some(')') | None => break,
                Some(_) if had_ws => {
                    comb = Some(Comb::Descendant);
                    continue;
                }
                Some(_) => return Err(self.invalid("beklenmeyen karakter")),
            };
            self.i += 1;
            self.ws();
        }
        Ok(Complex(parts))
    }

    fn nth(&mut self) -> Result<(i64, i64), SelectError> {
        let mut arg = String::new();
        while let Some(c) = self.peek() {
            if c == ')' {
                break;
            }
            arg.push(c);
            self.i += 1;
        }
        let a = arg.trim().to_ascii_lowercase().replace(' ', "");
        let parse =
            |s: &str| s.parse::<i64>().map_err(|_| SelectError::Invalid(format!("nth: {arg}")));
        Ok(match a.as_str() {
            "odd" => (2, 1),
            "even" => (2, 0),
            s if s.contains('n') => {
                let (an, b) = s.split_once('n').unwrap_or((s, ""));
                let a = match an {
                    "" | "+" => 1,
                    "-" => -1,
                    x => parse(x)?,
                };
                (a, if b.is_empty() { 0 } else { parse(b)? })
            }
            s => (0, parse(s)?),
        })
    }

    fn compound(&mut self) -> Result<Vec<Simple>, SelectError> {
        let mut out = Vec::new();
        if self.peek() == Some('*') {
            self.i += 1;
            out.push(Simple::Universal);
        } else if self
            .peek()
            .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '\\' || c == '-')
        {
            out.push(Simple::Type(self.ident()?.to_lowercase()));
        }
        loop {
            match self.peek() {
                Some('#') => {
                    self.i += 1;
                    out.push(Simple::Id(self.ident()?));
                }
                Some('.') => {
                    self.i += 1;
                    out.push(Simple::Class(self.ident()?));
                }
                Some('[') => {
                    self.i += 1;
                    self.ws();
                    let name = self.ident()?.to_lowercase();
                    self.ws();
                    let op = match self.peek() {
                        Some(']') => AttrOp::Exists,
                        Some('=') => AttrOp::Eq,
                        Some(c @ ('~' | '|' | '^' | '$' | '*' | '!'))
                            if self.c.get(self.i + 1) == Some(&'=') =>
                        {
                            self.i += 1;
                            match c {
                                '~' => AttrOp::Word,
                                '|' => AttrOp::Dash,
                                '^' => AttrOp::Prefix,
                                '$' => AttrOp::Suffix,
                                '*' => AttrOp::Contains,
                                _ => AttrOp::NotEq,
                            }
                        }
                        _ => return Err(self.invalid("öznitelik işleci")),
                    };
                    let (mut value, mut ci) = (String::new(), false);
                    if !matches!(op, AttrOp::Exists) {
                        self.i += 1;
                        self.ws();
                        value = if matches!(self.peek(), Some('"' | '\'')) {
                            self.string()?
                        } else {
                            self.ident()?
                        };
                        self.ws();
                        if let Some(f) = self.peek().filter(|f| matches!(f, 'i' | 'I' | 's' | 'S'))
                        {
                            ci = matches!(f, 'i' | 'I');
                            self.i += 1;
                            self.ws();
                        }
                    }
                    if self.peek() != Some(']') {
                        return Err(self.invalid("]"));
                    }
                    self.i += 1;
                    out.push(Simple::Attr { name, op, value, ci });
                }
                Some(':') => {
                    self.i += 1;
                    if self.peek() == Some(':') {
                        return Err(SelectError::Unsupported("sözde öğe".into()));
                    }
                    let name = self.ident()?.to_ascii_lowercase();
                    let functional = self.peek() == Some('(');
                    if functional {
                        self.i += 1;
                        self.ws();
                    }
                    let s = match (name.as_str(), functional) {
                        ("first-child", false) => {
                            Simple::Nth { a: 0, b: 1, last: false, of_type: false }
                        }
                        ("last-child", false) => {
                            Simple::Nth { a: 0, b: 1, last: true, of_type: false }
                        }
                        ("first-of-type", false) => {
                            Simple::Nth { a: 0, b: 1, last: false, of_type: true }
                        }
                        ("last-of-type", false) => {
                            Simple::Nth { a: 0, b: 1, last: true, of_type: true }
                        }
                        ("only-child", false) => Simple::Is(vec![Complex(vec![(
                            vec![
                                Simple::Nth { a: 0, b: 1, last: false, of_type: false },
                                Simple::Nth { a: 0, b: 1, last: true, of_type: false },
                            ],
                            None,
                        )])]),
                        ("only-of-type", false) => Simple::Is(vec![Complex(vec![(
                            vec![
                                Simple::Nth { a: 0, b: 1, last: false, of_type: true },
                                Simple::Nth { a: 0, b: 1, last: true, of_type: true },
                            ],
                            None,
                        )])]),
                        ("empty", false) => Simple::Empty,
                        ("root", false) => Simple::Root,
                        (
                            "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type",
                            true,
                        ) => {
                            let (a, b) = self.nth()?;
                            Simple::Nth {
                                a,
                                b,
                                last: name.contains("last"),
                                of_type: name.contains("type"),
                            }
                        }
                        ("not", true) => Simple::Not(self.list()?),
                        ("is" | "matches" | "any" | "where", true) => Simple::Is(self.list()?),
                        _ => return Err(SelectError::Unsupported(format!(":{name}"))),
                    };
                    if functional {
                        self.ws();
                        if self.peek() != Some(')') {
                            return Err(self.invalid(")"));
                        }
                        self.i += 1;
                    }
                    out.push(s);
                }
                _ => return Ok(out),
            }
        }
    }
}

/// bs4 `DEFAULT_CDATA_LIST_ATTRIBUTES` (çok değerli öznitelikler).
fn is_list_attr(tag: &str, attr: &str) -> bool {
    matches!(attr, "class" | "accesskey" | "dropzone")
        || matches!(
            (tag, attr),
            ("a" | "link", "rel" | "rev")
                | ("td" | "th", "headers")
                | ("form", "accept-charset")
                | ("object", "archive")
                | ("area", "rel")
                | ("icon", "sizes")
                | ("iframe", "sandbox")
                | ("output", "for")
        )
}

fn tokens(v: &str) -> Vec<&str> {
    v.split(py_is_space).filter(|t| !t.is_empty()).collect()
}

impl Document {
    fn element_siblings(&self, id: usize) -> Vec<usize> {
        match self.parent(id) {
            Some(p) => self.children(p).iter().copied().filter(|&c| self.is_element(c)).collect(),
            None => vec![id],
        }
    }

    fn attr_value(&self, id: usize, name: &str) -> Option<String> {
        let tag = self.name(id)?;
        let Node::Element { attrs, .. } = &self.nodes[id] else { return None };
        let (_, v) = attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case(name))?;
        Some(if is_list_attr(tag, name) { tokens(v).join(" ") } else { v.clone() })
    }

    fn matches_simple(&self, id: usize, s: &Simple) -> bool {
        match s {
            Simple::Universal => true,
            Simple::Type(t) => self.name(id) == Some(t.as_str()),
            Simple::Id(v) => self.attr_value(id, "id").as_deref() == Some(v.as_str()),
            Simple::Class(c) => {
                self.attr_value(id, "class").is_some_and(|v| tokens(&v).contains(&c.as_str()))
            }
            Simple::Attr { name, op, value, ci } => {
                let Some(actual) = self.attr_value(id, name) else {
                    return matches!(op, AttrOp::NotEq);
                };
                let (a, v) = if *ci {
                    (actual.to_lowercase(), value.to_lowercase())
                } else {
                    (actual, value.clone())
                };
                match op {
                    AttrOp::Exists => true,
                    AttrOp::Eq => a == v,
                    AttrOp::NotEq => a != v,
                    AttrOp::Word => !v.is_empty() && tokens(&a).contains(&v.as_str()),
                    AttrOp::Dash => a == v || a.starts_with(&format!("{v}-")),
                    AttrOp::Prefix => !v.is_empty() && a.starts_with(&v),
                    AttrOp::Suffix => !v.is_empty() && a.ends_with(&v),
                    AttrOp::Contains => !v.is_empty() && a.contains(&v),
                }
            }
            Simple::Nth { a, b, last, of_type } => {
                let mut sibs = self.element_siblings(id);
                if *of_type {
                    let n = self.name(id);
                    sibs.retain(|&x| self.name(x) == n);
                }
                if *last {
                    sibs.reverse();
                }
                let Some(pos) = sibs.iter().position(|&x| x == id) else { return false };
                let k = pos as i64 + 1;
                if *a == 0 { k == *b } else { (k - b) % a == 0 && (k - b) / a >= 0 }
            }
            // soupsieve match_empty: öğe çocuk yok; içerik dizesi (yorum/CData/doctype/
            // bildirim/PI hariç) yalnızca [ \t\r\n\f] içerebilir
            Simple::Empty => self.children(id).iter().all(|&c| match &self.nodes[c] {
                Node::Element { .. } => false,
                Node::Text { kind, text, .. } => {
                    !matches!(kind, super::Kind::Text | super::Kind::Contained(_))
                        || text.chars().all(|ch| matches!(ch, ' ' | '\t' | '\r' | '\n' | '\u{c}'))
                }
            }),
            Simple::Root => self.parent(id) == Some(ROOT),
            Simple::Not(list) => !list.iter().any(|c| self.matches_complex(id, c)),
            Simple::Is(list) => list.iter().any(|c| self.matches_complex(id, c)),
        }
    }

    fn matches_compound(&self, id: usize, comp: &[Simple]) -> bool {
        self.is_element(id) && id != ROOT && comp.iter().all(|s| self.matches_simple(id, s))
    }

    fn matches_from(&self, id: usize, parts: &[(Vec<Simple>, Option<Comb>)]) -> bool {
        let Some(((comp, comb), rest)) = parts.split_last() else { return true };
        if !self.matches_compound(id, comp) {
            return false;
        }
        let Some(comb) = comb else { return rest.is_empty() };
        match comb {
            Comb::Child => self.parent(id).is_some_and(|p| self.matches_from(p, rest)),
            Comb::Descendant => {
                let mut cur = self.parent(id);
                while let Some(p) = cur {
                    if self.matches_from(p, rest) {
                        return true;
                    }
                    cur = self.parent(p);
                }
                false
            }
            // soupsieve 3.0.3: `~` önbellekli eşleyicisi belge kökünü üst saymaz
            // (get_parent(no_doc=True) → None) — üst düzey öğelerde `~` hiç eşleşmez; `+` eşleşir.
            Comb::Sibling if self.parent(id) == Some(ROOT) => false,
            Comb::Adjacent | Comb::Sibling => {
                let sibs = self.element_siblings(id);
                let pos = sibs.iter().position(|&x| x == id).unwrap_or(0);
                let before = &sibs[..pos];
                if *comb == Comb::Adjacent {
                    before.last().is_some_and(|&s| self.matches_from(s, rest))
                } else {
                    before.iter().any(|&s| self.matches_from(s, rest))
                }
            }
        }
    }

    fn matches_complex(&self, id: usize, c: &Complex) -> bool {
        self.matches_from(id, &c.0)
    }

    /// `soup.select(selector)` — belge sırasıyla eşleşen öğeler.
    pub fn select(&self, selector: &str) -> Result<Vec<usize>, SelectError> {
        let mut p = P { c: selector.chars().collect(), i: 0, src: selector };
        let list = p.list()?;
        p.ws();
        if p.i != p.c.len() {
            return Err(p.invalid("fazla karakter"));
        }
        Ok(self
            .descendants(ROOT)
            .into_iter()
            .filter(|&n| self.is_element(n) && list.iter().any(|c| self.matches_complex(n, c)))
            .collect())
    }
}
