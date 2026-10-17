//! Diller arası JSON değeri: Python `json`/psycopg nesnelerinin kayıpsız karşılığı.
//!
//! `serde_json::Value` yerine kendi türümüz: Python tamsayıları sınırsızdır (`BigInt`) ve
//! iki kanonik biçim (v1 = Python `json.dumps`, v2 = RFC 8785) sayıları farklı yazar.

/// JSON değeri. Nesne anahtarları benzersizdir (Python `dict`); sıra kanonikleştirmede
/// yeniden belirlenir.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    /// `i64` dışındaki Python tamsayısı, işaretli ondalık dize (`"-123…"`).
    BigInt(String),
    Float(f64),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Python doğruluk değeri (`detail or {}` için).
    pub fn is_falsy(&self) -> bool {
        match self {
            Json::Null => true,
            Json::Bool(b) => !b,
            Json::Int(i) => *i == 0,
            Json::BigInt(_) => false,
            Json::Float(f) => *f == 0.0,
            Json::Str(s) => s.is_empty(),
            Json::Array(a) => a.is_empty(),
            Json::Object(o) => o.is_empty(),
        }
    }

    /// JSON metnini ayrıştırır (Python `json.loads` gibi: tamsayı literali tamsayı,
    /// kesirli/üslü literal float olur).
    pub fn parse(text: &str) -> Result<Json, serde_json::Error> {
        let v: serde_json::Value = serde_json::from_str(text)?;
        Ok(Json::from(v))
    }
}

impl From<serde_json::Value> for Json {
    fn from(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Null => Json::Null,
            serde_json::Value::Bool(b) => Json::Bool(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Json::Int(i)
                } else if let Some(u) = n.as_u64() {
                    Json::BigInt(u.to_string())
                } else {
                    Json::Float(n.as_f64().unwrap_or(f64::NAN))
                }
            }
            serde_json::Value::String(s) => Json::Str(s),
            serde_json::Value::Array(a) => Json::Array(a.into_iter().map(Json::from).collect()),
            serde_json::Value::Object(o) => {
                Json::Object(o.into_iter().map(|(k, v)| (k, Json::from(v))).collect())
            }
        }
    }
}

/// JSON dize kaçışı — Python `json.dumps(ensure_ascii=False)` ve RFC 8785 aynıdır:
/// `"` `\\` `\b` `\f` `\n` `\r` `\t` kısa biçim, diğer U+0000–U+001F `\u00xx` (küçük harf).
pub(crate) fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Python `json.loads` uyumlu ayrıştırma hatası.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyJsonError(pub String);

impl std::fmt::Display for PyJsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PyJsonError {}

/// Python'un özyineleme sınırına yakın iç içe geçme tavanı.
const MAX_PY_JSON_DEPTH: usize = 1000;

impl Json {
    /// Python `json.loads(text)` ile aynı sonuç: tamsayılar tam (`BigInt`), `NaN`,
    /// `Infinity`, `-Infinity` kabul edilir, tekrarlanan anahtarda **ilk konum korunur, son
    /// değer kazanır** (dict ataması), dizelerde kontrol karakteri reddedilir (strict).
    /// Eşlenmemiş vekil (`\ud800`) Rust dizesine sığmadığından U+FFFD olur.
    pub fn parse_py(text: &str) -> Result<Json, PyJsonError> {
        let mut p = PyParser { s: text.as_bytes(), i: 0, text };
        p.ws();
        let v = p.value(0)?;
        p.ws();
        if p.i != p.s.len() {
            return Err(p.err("Extra data"));
        }
        Ok(v)
    }
}

struct PyParser<'a> {
    s: &'a [u8],
    i: usize,
    text: &'a str,
}

impl PyParser<'_> {
    fn err(&self, msg: &str) -> PyJsonError {
        PyJsonError(format!(
            "{msg}: char {}",
            self.text[..self.i.min(self.text.len())].chars().count()
        ))
    }

    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn eat(&mut self, lit: &str) -> bool {
        if self.s[self.i..].starts_with(lit.as_bytes()) {
            self.i += lit.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, PyJsonError> {
        if depth > MAX_PY_JSON_DEPTH {
            return Err(self.err("maximum recursion depth exceeded"));
        }
        let Some(&c) = self.s.get(self.i) else { return Err(self.err("Expecting value")) };
        match c {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => Ok(Json::Str(self.string()?)),
            b'n' if self.eat("null") => Ok(Json::Null),
            b't' if self.eat("true") => Ok(Json::Bool(true)),
            b'f' if self.eat("false") => Ok(Json::Bool(false)),
            b'N' if self.eat("NaN") => Ok(Json::Float(f64::NAN)),
            b'I' if self.eat("Infinity") => Ok(Json::Float(f64::INFINITY)),
            b'-' if self.s[self.i..].starts_with(b"-Infinity") => {
                self.i += 9;
                Ok(Json::Float(f64::NEG_INFINITY))
            }
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(self.err("Expecting value")),
        }
    }

    fn number(&mut self) -> Result<Json, PyJsonError> {
        let start = self.i;
        let digits = |p: &mut Self| {
            let st = p.i;
            while p.i < p.s.len() && p.s[p.i].is_ascii_digit() {
                p.i += 1;
            }
            p.i - st
        };
        if self.s[self.i] == b'-' {
            self.i += 1;
        }
        // (0|[1-9]\d*)
        match self.s.get(self.i) {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                digits(self);
            }
            _ => {
                self.i = start;
                return Err(self.err("Expecting value"));
            }
        }
        let int_end = self.i;
        let mut is_float = false;
        if self.s.get(self.i) == Some(&b'.') {
            let save = self.i;
            self.i += 1;
            if digits(self) == 0 {
                self.i = save; // "1." → tamsayı 1, ardından fazla veri
            } else {
                is_float = true;
            }
        }
        if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
            let save = self.i;
            self.i += 1;
            if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if digits(self) == 0 {
                self.i = save;
            } else {
                is_float = true;
            }
        }
        let lit = &self.text[start..self.i];
        if is_float {
            return Ok(Json::Float(lit.parse::<f64>().unwrap_or(f64::NAN)));
        }
        let lit = &self.text[start..int_end];
        Ok(match lit.parse::<i64>() {
            Ok(v) => Json::Int(v),
            Err(_) => Json::BigInt(lit.to_owned()),
        })
    }

    fn hex4(&mut self) -> Result<u32, PyJsonError> {
        let h = self.s.get(self.i..self.i + 4).ok_or_else(|| self.err("Invalid \\uXXXX escape"))?;
        let h = std::str::from_utf8(h).map_err(|_| self.err("Invalid \\uXXXX escape"))?;
        if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(self.err("Invalid \\uXXXX escape"));
        }
        self.i += 4;
        Ok(u32::from_str_radix(h, 16).unwrap_or(0xfffd))
    }

    fn string(&mut self) -> Result<String, PyJsonError> {
        self.i += 1; // "
        let mut out = String::new();
        loop {
            let start = self.i;
            while self.i < self.s.len()
                && !matches!(self.s[self.i], b'"' | b'\\')
                && self.s[self.i] >= 0x20
            {
                self.i += 1;
            }
            out.push_str(&self.text[start..self.i]);
            match self.s.get(self.i) {
                None => return Err(self.err("Unterminated string starting at")),
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.i += 1;
                    let Some(&e) = self.s.get(self.i) else {
                        return Err(self.err("Unterminated string starting at"));
                    };
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let cp = if (0xd800..0xdc00).contains(&hi)
                                && self.s[self.i..].starts_with(b"\\u")
                            {
                                let save = self.i;
                                self.i += 2;
                                let lo = self.hex4()?;
                                if (0xdc00..0xe000).contains(&lo) {
                                    0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00)
                                } else {
                                    self.i = save;
                                    hi
                                }
                            } else {
                                hi
                            };
                            out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                        }
                        _ => {
                            self.i -= 1;
                            return Err(self.err("Invalid \\escape"));
                        }
                    }
                }
                Some(_) => return Err(self.err("Invalid control character at")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, PyJsonError> {
        self.i += 1;
        let mut items = Vec::new();
        self.ws();
        if self.s.get(self.i) == Some(&b']') {
            self.i += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.ws();
            items.push(self.value(depth + 1)?);
            self.ws();
            match self.s.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.err("Expecting ',' delimiter")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, PyJsonError> {
        self.i += 1;
        let mut entries: Vec<(String, Json)> = Vec::new();
        let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        self.ws();
        if self.s.get(self.i) == Some(&b'}') {
            self.i += 1;
            return Ok(Json::Object(entries));
        }
        loop {
            self.ws();
            if self.s.get(self.i) != Some(&b'"') {
                return Err(self.err("Expecting property name enclosed in double quotes"));
            }
            let key = self.string()?;
            self.ws();
            if self.s.get(self.i) != Some(&b':') {
                return Err(self.err("Expecting ':' delimiter"));
            }
            self.i += 1;
            self.ws();
            let v = self.value(depth + 1)?;
            match index.get(&key) {
                Some(&pos) => entries[pos].1 = v,
                None => {
                    index.insert(key.clone(), entries.len());
                    entries.push((key, v));
                }
            }
            self.ws();
            match self.s.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Json::Object(entries));
                }
                _ => return Err(self.err("Expecting ',' delimiter")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_json_loads_gibi() {
        let v = Json::parse_py(
            r#" {"a": 1, "b": [NaN, -Infinity, 1e400, 123456789012345678901234], "a": 2} "#,
        )
        .unwrap();
        let Json::Object(e) = v else { panic!() };
        assert_eq!(e[0], ("a".into(), Json::Int(2)));
        let Json::Array(b) = &e[1].1 else { panic!() };
        assert!(matches!(b[0], Json::Float(f) if f.is_nan()));
        assert_eq!(b[1], Json::Float(f64::NEG_INFINITY));
        assert_eq!(b[2], Json::Float(f64::INFINITY));
        assert_eq!(b[3], Json::BigInt("123456789012345678901234".into()));
        assert_eq!(Json::parse_py(r#""😀ç""#).unwrap(), Json::Str("😀ç".into()));
        for bad in ["", "[1,]", "{\"a\" 1}", "\"\u{1}\"", "01", "1 2", "[1.]", "tru"] {
            assert!(Json::parse_py(bad).is_err(), "{bad:?}");
        }
    }
}
