//! A JSON value that round-trips a notebook byte for byte.
//!
//! `serde_json` cannot: it forgets key order and re-prints numbers (`1e-05` comes back as `1e-5`),
//! and turning on its `preserve_order` / `arbitrary_precision` features would change it for the
//! whole build. So the notebook keeps its own tree: objects are ordered pairs, numbers are the
//! source text, and [`write`] prints the way the writer of the file did — Jupyter's
//! `json.dumps(indent=1, sort_keys=True, ensure_ascii=False)` and the VS Code / Colab variants all
//! reduce to an indent width and a key order, which the tree already carries.

use std::fmt::Write as _;

/// A JSON value with its key order and its number text kept.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// The number as written in the source; never re-formatted.
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    /// Pairs in source order.
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Num(n) => n.parse().ok(),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }

    /// A string, or a list of strings joined — nbformat's "multiline string".
    pub fn as_multiline(&self) -> Option<String> {
        match self {
            Json::Str(s) => Some(s.clone()),
            Json::Arr(items) => {
                let mut out = String::new();
                for item in items {
                    out.push_str(item.as_str()?);
                }
                Some(out)
            }
            _ => None,
        }
    }

    /// Set `key`, keeping its position when present. A new key goes where sorted order puts it
    /// when the object's keys are sorted (Jupyter's files), at the end otherwise (Colab's).
    pub fn set(&mut self, key: &str, value: Json) {
        let Json::Obj(pairs) = self else { return };
        if let Some(slot) = pairs.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value;
            return;
        }
        let sorted = pairs.windows(2).all(|w| w[0].0 <= w[1].0);
        let at = if sorted {
            pairs.partition_point(|(k, _)| k.as_str() < key)
        } else {
            pairs.len()
        };
        pairs.insert(at, (key.to_string(), value));
    }

    pub fn remove(&mut self, key: &str) {
        if let Json::Obj(pairs) = self {
            pairs.retain(|(k, _)| k != key);
        }
    }

    /// An object from pairs, sorted by key — the shape of anything Kubiq creates.
    pub fn sorted(mut pairs: Vec<(String, Json)>) -> Json {
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        Json::Obj(pairs)
    }

    pub fn str(s: impl Into<String>) -> Json {
        Json::Str(s.into())
    }

    /// nbformat's multiline string as Jupyter writes it: one entry per line, each keeping its
    /// `\n`. An empty string is an empty list.
    pub fn lines(s: &str) -> Json {
        Json::Arr(split_lines(s).into_iter().map(Json::str).collect())
    }
}

/// Split after every `\n`, keeping it. `""` is no lines.
pub fn split_lines(s: &str) -> Vec<String> {
    s.split_inclusive('\n').map(str::to_string).collect()
}

/// How a file was printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    /// Spaces per level; `None` is compact (no whitespace at all).
    pub indent: Option<usize>,
    /// The file ends with a newline.
    pub trailing_newline: bool,
}

impl Default for Style {
    /// Jupyter's own: one space, a trailing newline.
    fn default() -> Self {
        Style {
            indent: Some(1),
            trailing_newline: true,
        }
    }
}

impl Style {
    /// Read the style off the text: the indent of the first indented line, and the last byte.
    pub fn detect(text: &str) -> Style {
        let indent = text.lines().nth(1).map(|line| line.len() - line.trim_start_matches(' ').len());
        Style {
            indent: match indent {
                Some(n) if text.trim_start().starts_with('{') && text.contains('\n') => Some(n),
                _ => None,
            },
            trailing_newline: text.ends_with('\n'),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub at: usize,
    pub what: &'static str,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at byte {}", self.what, self.at)
    }
}

impl std::error::Error for ParseError {}

pub fn parse(text: &str) -> Result<Json, ParseError> {
    let mut p = Parser {
        s: text.as_bytes(),
        text,
        i: 0,
    };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.err("trailing characters"));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    text: &'a str,
    i: usize,
}

const MAX_DEPTH: usize = 256;

impl Parser<'_> {
    fn err(&self, what: &'static str) -> ParseError {
        ParseError { at: self.i, what }
    }

    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }

    fn eat(&mut self, b: u8) -> bool {
        if self.s.get(self.i) == Some(&b) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn lit(&mut self, word: &str, v: Json) -> Result<Json, ParseError> {
        if self.s[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(self.err("unexpected token"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, ParseError> {
        if depth > MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        match self.s.get(self.i) {
            None => Err(self.err("unexpected end")),
            Some(b'n') => self.lit("null", Json::Null),
            Some(b't') => self.lit("true", Json::Bool(true)),
            Some(b'f') => self.lit("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::Str),
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.eat(b']') {
                    return Ok(Json::Arr(items));
                }
                loop {
                    self.ws();
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    if self.eat(b',') {
                        continue;
                    }
                    if self.eat(b']') {
                        return Ok(Json::Arr(items));
                    }
                    return Err(self.err("expected , or ]"));
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut pairs = Vec::new();
                self.ws();
                if self.eat(b'}') {
                    return Ok(Json::Obj(pairs));
                }
                loop {
                    self.ws();
                    if self.s.get(self.i) != Some(&b'"') {
                        return Err(self.err("expected a key"));
                    }
                    let key = self.string()?;
                    self.ws();
                    if !self.eat(b':') {
                        return Err(self.err("expected :"));
                    }
                    self.ws();
                    let v = self.value(depth + 1)?;
                    pairs.push((key, v));
                    self.ws();
                    if self.eat(b',') {
                        continue;
                    }
                    if self.eat(b'}') {
                        return Ok(Json::Obj(pairs));
                    }
                    return Err(self.err("expected , or }"));
                }
            }
            Some(b'-' | b'0'..=b'9') => {
                let start = self.i;
                self.i += 1;
                while self.i < self.s.len()
                    && matches!(self.s[self.i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                {
                    self.i += 1;
                }
                Ok(Json::Num(self.text[start..self.i].to_string()))
            }
            // Python's json writes these for non-finite floats; keep them as written.
            Some(b'N') => self.lit("NaN", Json::Num("NaN".into())),
            Some(b'I') => self.lit("Infinity", Json::Num("Infinity".into())),
            Some(_) => Err(self.err("unexpected character")),
        }
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        let h = self
            .text
            .get(self.i..self.i + 4)
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .ok_or_else(|| self.err("bad \\u escape"))?;
        self.i += 4;
        Ok(h)
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.i += 1; // the opening quote
        let mut out = String::new();
        loop {
            let start = self.i;
            while self.i < self.s.len() && self.s[self.i] != b'"' && self.s[self.i] != b'\\' {
                self.i += 1;
            }
            out.push_str(&self.text[start..self.i]);
            match self.s.get(self.i) {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                _ => {
                    self.i += 1;
                    let c = *self.s.get(self.i).ok_or_else(|| self.err("bad escape"))?;
                    self.i += 1;
                    match c {
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
                            let code = if (0xD800..0xDC00).contains(&hi)
                                && self.s[self.i..].starts_with(b"\\u")
                            {
                                self.i += 2;
                                let lo = self.hex4()?;
                                0x10000 + ((hi - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3FF)
                            } else {
                                hi
                            };
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        _ => return Err(self.err("bad escape")),
                    }
                }
            }
        }
    }
}

/// Print `value` in `style`. Escaping is Python's `ensure_ascii=False` (and `JSON.stringify`'s):
/// `"` `\` and the C0 controls only, `\n` `\r` `\t` `\b` `\f` by name, the rest as `\u00xx`.
pub fn write(value: &Json, style: Style) -> String {
    let mut out = String::new();
    put(&mut out, value, style.indent, 0);
    if style.trailing_newline {
        out.push('\n');
    }
    out
}

fn newline(out: &mut String, indent: Option<usize>, depth: usize) {
    if let Some(n) = indent {
        out.push('\n');
        out.extend(std::iter::repeat_n(' ', n * depth));
    }
}

fn put(out: &mut String, value: &Json, indent: Option<usize>, depth: usize) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(n) => out.push_str(n),
        Json::Str(s) => put_str(out, s),
        Json::Arr(items) if items.is_empty() => out.push_str("[]"),
        Json::Obj(pairs) if pairs.is_empty() => out.push_str("{}"),
        Json::Arr(items) => {
            out.push('[');
            for (n, item) in items.iter().enumerate() {
                if n > 0 {
                    out.push(',');
                }
                newline(out, indent, depth + 1);
                put(out, item, indent, depth + 1);
            }
            newline(out, indent, depth);
            out.push(']');
        }
        Json::Obj(pairs) => {
            out.push('{');
            for (n, (k, v)) in pairs.iter().enumerate() {
                if n > 0 {
                    out.push(',');
                }
                newline(out, indent, depth + 1);
                put_str(out, k);
                out.push(':');
                if indent.is_some() {
                    out.push(' ');
                }
                put(out, v, indent, depth + 1);
            }
            newline(out, indent, depth);
            out.push('}');
        }
    }
}

fn put_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_order_survive() {
        let text = "{\n \"b\": 1e-05,\n \"a\": [\n  1.50,\n  \"x\\u0001é\"\n ],\n \"c\": {}\n}\n";
        let v = parse(text).unwrap();
        assert_eq!(write(&v, Style::detect(text)), text);
    }

    #[test]
    fn set_inserts_sorted_or_appends() {
        let mut sorted = parse(r#"{"a":1,"c":2}"#).unwrap();
        sorted.set("b", Json::Null);
        assert_eq!(write(&sorted, Style { indent: None, trailing_newline: false }), r#"{"a":1,"b":null,"c":2}"#);
        let mut unsorted = parse(r#"{"c":1,"a":2}"#).unwrap();
        unsorted.set("b", Json::Null);
        assert_eq!(write(&unsorted, Style { indent: None, trailing_newline: false }), r#"{"c":1,"a":2,"b":null}"#);
    }

    #[test]
    fn surrogate_pairs_decode() {
        assert_eq!(parse(r#""😀""#).unwrap(), Json::str("😀"));
    }

    #[test]
    fn lines_keep_newlines() {
        assert_eq!(split_lines("a\nb"), vec!["a\n", "b"]);
        assert!(split_lines("").is_empty());
    }
}
