//! JSON minimal : lecture complète et écriture des chaînes, de quoi parler
//! aux API HTTP sans dépendance.

use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}

impl Json {
    pub(crate) fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(m) => m.get(key),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn as_arr(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(v) => Some(v),
            _ => None,
        }
    }

    pub(crate) fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }
}

/// Lit un document JSON complet.
pub(crate) fn parse(s: &str) -> Option<Json> {
    let mut p = Parser {
        s: s.as_bytes(),
        i: 0,
    };
    let v = p.value()?;
    p.ws();
    (p.i == p.s.len()).then_some(v)
}

/// Chaîne JSON entre guillemets, échappée.
pub(crate) fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
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
    out
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, lit: &str) -> Option<()> {
        self.s[self.i..]
            .starts_with(lit.as_bytes())
            .then(|| self.i += lit.len())
    }

    fn value(&mut self) -> Option<Json> {
        self.ws();
        match *self.s.get(self.i)? {
            b'{' => {
                self.i += 1;
                let mut m = BTreeMap::new();
                self.ws();
                if self.eat("}").is_some() {
                    return Some(Json::Obj(m));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    self.ws();
                    self.eat(":")?;
                    let v = self.value()?;
                    m.insert(k, v);
                    self.ws();
                    if self.eat(",").is_none() {
                        self.eat("}")?;
                        return Some(Json::Obj(m));
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.eat("]").is_some() {
                    return Some(Json::Arr(v));
                }
                loop {
                    v.push(self.value()?);
                    self.ws();
                    if self.eat(",").is_none() {
                        self.eat("]")?;
                        return Some(Json::Arr(v));
                    }
                }
            }
            b'"' => self.string().map(Json::Str),
            b't' => self.eat("true").map(|_| Json::Bool(true)),
            b'f' => self.eat("false").map(|_| Json::Bool(false)),
            b'n' => self.eat("null").map(|_| Json::Null),
            _ => {
                let start = self.i;
                while self.i < self.s.len() && b"+-.eE0123456789".contains(&self.s[self.i]) {
                    self.i += 1;
                }
                std::str::from_utf8(&self.s[start..self.i])
                    .ok()?
                    .parse()
                    .ok()
                    .map(Json::Num)
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let h = std::str::from_utf8(self.s.get(self.i..self.i + 4)?).ok()?;
        self.i += 4;
        u32::from_str_radix(h, 16).ok()
    }

    fn string(&mut self) -> Option<String> {
        self.eat("\"")?;
        let mut out = Vec::new();
        loop {
            let b = *self.s.get(self.i)?;
            self.i += 1;
            match b {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let e = *self.s.get(self.i)?;
                    self.i += 1;
                    let c = match e {
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp) {
                                // Paire de substitution UTF-16.
                                self.eat("\\u")?;
                                let lo = self.hex4()?.checked_sub(0xDC00)?;
                                cp = 0x10000 + ((cp - 0xD800) << 10) + lo;
                            }
                            char::from_u32(cp)?
                        }
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        c => c as char,
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                b => out.push(b),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let v = parse(r#" {"a": [1, -2.5e1, true, null], "b": "x\"yé😀", "c": {}} "#).unwrap();
        assert_eq!(
            v.get("a").unwrap().as_arr().unwrap()[1].as_f64(),
            Some(-25.0)
        );
        assert_eq!(v.get("b").unwrap().as_str(), Some("x\"yé😀"));
        assert_eq!(v.get("c"), Some(&Json::Obj(BTreeMap::new())));
        assert!(parse("[1,]").is_none());
        assert!(parse("{} x").is_none());
        let q = quote("a\"b\\c\nd\u{1}é");
        assert_eq!(q, r#""a\"b\\c\nd\u0001é""#);
        assert_eq!(parse(&q).unwrap().as_str(), Some("a\"b\\c\nd\u{1}é"));
    }
}
