//! JSON minimal, suffisant pour le protocole LSP.
//!
//! Rava n'a aucune dépendance : le compilateur et ses outils se lisent en
//! entier. Le sous-ensemble du protocole utilisé ici tient dans ce fichier.

use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}

impl Json {
    pub fn obj(pairs: Vec<(&str, Json)>) -> Json {
        Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    pub fn str(s: impl Into<String>) -> Json {
        Json::Str(s.into())
    }

    pub fn int(n: i64) -> Json {
        Json::Num(n as f64)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(m) => m.get(key),
            _ => None,
        }
    }

    /// Descente dans un chemin de clés : `v.path(["params", "textDocument"])`.
    pub fn path<'a>(&'a self, keys: &[&str]) -> Option<&'a Json> {
        let mut cur = self;
        for k in keys {
            cur = cur.get(k)?;
        }
        Some(cur)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Json::Num(n) if *n >= 0.0 => Some(*n as u32),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(v) => Some(v),
            _ => None,
        }
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => {
                if n.fract() == 0.0 && n.abs() < 9e15 {
                    let _ = write!(out, "{}", *n as i64);
                } else {
                    let _ = write!(out, "{n}");
                }
            }
            Json::Str(s) => write_string(s, out),
            Json::Arr(v) => {
                out.push('[');
                for (i, e) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    e.write(out);
                }
                out.push(']');
            }
            Json::Obj(m) => {
                out.push('{');
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_string(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn parse(src: &str) -> Option<Json> {
    let mut p = P { b: src.as_bytes(), i: 0 };
    p.ws();
    let v = p.value()?;
    Some(v)
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> P<'a> {
    fn ws(&mut self) {
        while matches!(self.b.get(self.i), Some(c) if c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.b.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Option<Json> {
        self.ws();
        match *self.b.get(self.i)? {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => self.string().map(Json::Str),
            b't' => self.lit("true", Json::Bool(true)),
            b'f' => self.lit("false", Json::Bool(false)),
            b'n' => self.lit("null", Json::Null),
            _ => self.number(),
        }
    }

    fn lit(&mut self, word: &str, v: Json) -> Option<Json> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Some(v)
        } else {
            None
        }
    }

    fn object(&mut self) -> Option<Json> {
        self.i += 1;
        let mut m = BTreeMap::new();
        self.ws();
        if self.eat(b'}') {
            return Some(Json::Obj(m));
        }
        loop {
            self.ws();
            let k = self.string()?;
            self.ws();
            if !self.eat(b':') {
                return None;
            }
            let v = self.value()?;
            m.insert(k, v);
            self.ws();
            if self.eat(b',') {
                continue;
            }
            return if self.eat(b'}') { Some(Json::Obj(m)) } else { None };
        }
    }

    fn array(&mut self) -> Option<Json> {
        self.i += 1;
        let mut v = Vec::new();
        self.ws();
        if self.eat(b']') {
            return Some(Json::Arr(v));
        }
        loop {
            v.push(self.value()?);
            self.ws();
            if self.eat(b',') {
                continue;
            }
            return if self.eat(b']') { Some(Json::Arr(v)) } else { None };
        }
    }

    fn string(&mut self) -> Option<String> {
        if !self.eat(b'"') {
            return None;
        }
        let mut out = String::new();
        loop {
            let c = *self.b.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return Some(out),
                b'\\' => {
                    let e = *self.b.get(self.i)?;
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
                            let hex = std::str::from_utf8(self.b.get(self.i..self.i + 4)?).ok()?;
                            self.i += 4;
                            let mut cp = u32::from_str_radix(hex, 16).ok()?;
                            // Paire de substitution UTF-16.
                            if (0xD800..0xDC00).contains(&cp) && self.b.get(self.i) == Some(&b'\\')
                            {
                                let hex2 =
                                    std::str::from_utf8(self.b.get(self.i + 2..self.i + 6)?).ok()?;
                                if let Ok(lo) = u32::from_str_radix(hex2, 16) {
                                    if (0xDC00..0xE000).contains(&lo) {
                                        self.i += 6;
                                        cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                    }
                                }
                            }
                            out.push(char::from_u32(cp)?);
                        }
                        _ => return None,
                    }
                }
                _ => {
                    // Recolle les octets d'un caractère UTF-8 multi-octets.
                    let start = self.i - 1;
                    while matches!(self.b.get(self.i), Some(c) if c & 0xC0 == 0x80) {
                        self.i += 1;
                    }
                    out.push_str(std::str::from_utf8(&self.b[start..self.i]).ok()?);
                }
            }
        }
    }

    fn number(&mut self) -> Option<Json> {
        let start = self.i;
        if self.b.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        while matches!(self.b.get(self.i), Some(c) if c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-'))
        {
            self.i += 1;
        }
        std::str::from_utf8(&self.b[start..self.i]).ok()?.parse().ok().map(Json::Num)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour() {
        let src = r#"{"a":[1,2,{"b":"x\ny"}],"c":true,"d":null}"#;
        let v = parse(src).unwrap();
        assert_eq!(v.path(&["a"]).unwrap().as_arr().unwrap().len(), 3);
        assert_eq!(parse(&v.to_text()).unwrap(), v);
    }

    #[test]
    fn echappements_unicode() {
        let v = parse(r#"{"s":"café 🦀"}"#).unwrap();
        assert_eq!(v.get("s").unwrap().as_str().unwrap(), "café 🦀");
    }

    #[test]
    fn chaines_reechappees_correctement() {
        let v = Json::str("guillemet \" et \\ et \n");
        assert_eq!(parse(&Json::obj(vec![("k", v.clone())]).to_text()).unwrap().get("k"), Some(&v));
    }
}
