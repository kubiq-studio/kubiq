//! The text layer: frontmatter, fences and their info-string attributes, as byte spans of the
//! source. Nothing here interprets a cell; [`crate::model`] does.
//!
//! The markdown engine (`kubiq-md`) keeps only the language word of a fence's info string, so the
//! runbook scans fences itself, line by line. The scan is deliberately simple: a fence opens on a
//! line of three or more backticks or tildes (up to three spaces of indent) and closes on a line of
//! the same character, at least as long, with nothing after it. An unclosed fence runs to the end.
//!
//! Every block keeps its byte span, so a rewrite replaces exactly the blocks it owns and leaves every
//! other byte of the file as it was.

use std::ops::Range;

/// One parsed document: where the frontmatter is, and every top-level block in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    /// The frontmatter's YAML text (without the `---` lines), and the span of the whole frontmatter
    /// including both delimiters and the newline after the closing one.
    pub front: Option<(String, Range<usize>)>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// Bytes of the source, from the first byte of the block to just after its last newline.
    pub span: Range<usize>,
    /// 1-based line of the block's first line.
    pub line: usize,
    pub kind: BlockKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind {
    /// Anything that is not a fence.
    Prose,
    Fence(Fence),
}

/// A fenced block.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fence {
    /// The first word of the info string (`sh`, `output`, `check`), lowercased.
    pub lang: String,
    /// The `{…}` attributes, when the info string has them.
    pub attrs: Option<Attrs>,
    /// The body between the fences, every line ending in `\n`.
    pub body: String,
    /// Whether a closing fence was found.
    pub closed: bool,
}

/// `{#id key=value key="quoted value" flag}`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Attrs {
    pub id: Option<String>,
    /// Bare words, in order (`readonly`, `manual`, `interactive`).
    pub flags: Vec<String>,
    /// `key=value` pairs, in order. A key given twice keeps both; [`Attrs::get`] reads the first.
    pub pairs: Vec<(String, String)>,
}

impl Attrs {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn has(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }

    /// The attributes back as an info-string group, `{#id flag key=value}`. Values with a space,
    /// a quote, a brace or nothing at all are quoted.
    pub fn emit(&self) -> String {
        let mut parts = Vec::new();
        if let Some(id) = &self.id {
            parts.push(format!("#{id}"));
        }
        for (k, v) in &self.pairs {
            parts.push(format!("{k}={}", quote(v)));
        }
        for flag in &self.flags {
            parts.push(flag.clone());
        }
        format!("{{{}}}", parts.join(" "))
    }
}

fn quote(value: &str) -> String {
    let plain = !value.is_empty()
        && !value
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | '{' | '}' | '\''));
    if plain {
        value.to_string()
    } else {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// Parse an attribute group. `text` is what sits between `{` and `}`.
pub fn parse_attrs(text: &str) -> Attrs {
    let mut attrs = Attrs::default();
    let mut chars = text.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() || c == '=' {
                break;
            }
            key.push(c);
            chars.next();
        }
        if chars.peek() == Some(&'=') {
            chars.next();
            let mut value = String::new();
            if let Some(&q) = chars.peek().filter(|c| **c == '"' || **c == '\'') {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '\\' {
                        if let Some(n) = chars.next() {
                            value.push(n);
                        }
                    } else if c == q {
                        break;
                    } else {
                        value.push(c);
                    }
                }
            } else {
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() {
                        break;
                    }
                    value.push(c);
                    chars.next();
                }
            }
            attrs.pairs.push((key, value));
        } else if let Some(id) = key.strip_prefix('#') {
            if attrs.id.is_none() && !id.is_empty() {
                attrs.id = Some(id.to_string());
            }
        } else if !key.is_empty() {
            attrs.flags.push(key);
        }
    }
    attrs
}

/// The fence marker of an opening line: its character and length, and the info string.
fn opening(line: &str) -> Option<(char, usize, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let ch = rest.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let len = rest.chars().take_while(|c| *c == ch).count();
    if len < 3 {
        return None;
    }
    let info = rest[len..].trim();
    if ch == '`' && info.contains('`') {
        return None;
    }
    Some((ch, len, info))
}

fn closing(line: &str, ch: char, len: usize) -> bool {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return false;
    }
    let rest = line[indent..].trim_end();
    rest.len() >= len && rest.chars().all(|c| c == ch)
}

/// Split an info string into its language word and its attribute group.
pub fn split_info(info: &str) -> (String, Option<Attrs>) {
    let (head, attrs) = match (info.find('{'), info.rfind('}')) {
        (Some(open), Some(close)) if close > open => {
            (&info[..open], Some(parse_attrs(&info[open + 1..close])))
        }
        _ => (info, None),
    };
    let lang = head.split_whitespace().next().unwrap_or("").to_lowercase();
    (lang, attrs)
}

/// Lines with their byte offsets, each including its trailing newline.
fn lines(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        out.push((at, line));
        at += line.len();
    }
    out
}

/// Scan `text` into frontmatter and blocks.
pub fn scan(text: &str) -> Source {
    let all = lines(text);
    let mut i = 0;
    let mut front = None;
    if all
        .first()
        .is_some_and(|(_, l)| l.trim_end_matches(['\r', '\n']) == "---")
        && let Some(end) = all
            .iter()
            .skip(1)
            .position(|(_, l)| matches!(l.trim_end_matches(['\r', '\n']), "---" | "..."))
    {
        let end = end + 1;
        let yaml: String = all[1..end].iter().map(|(_, l)| *l).collect();
        let stop = all[end].0 + all[end].1.len();
        front = Some((yaml, 0..stop));
        i = end + 1;
    }

    let mut blocks = Vec::new();
    let mut prose: Option<(usize, usize)> = None; // (start byte, start line index)
    let flush = |prose: &mut Option<(usize, usize)>, end: usize, blocks: &mut Vec<Block>| {
        if let Some((start, line)) = prose.take()
            && end > start
        {
            blocks.push(Block {
                span: start..end,
                line: line + 1,
                kind: BlockKind::Prose,
            });
        }
    };
    while i < all.len() {
        let (at, line) = all[i];
        let bare = line.trim_end_matches(['\r', '\n']);
        if let Some((ch, len, info)) = opening(bare) {
            flush(&mut prose, at, &mut blocks);
            let (lang, attrs) = split_info(info);
            let mut body = String::new();
            let mut j = i + 1;
            let mut closed = false;
            while j < all.len() {
                let inner = all[j].1.trim_end_matches(['\r', '\n']);
                if closing(inner, ch, len) {
                    closed = true;
                    break;
                }
                body.push_str(all[j].1);
                j += 1;
            }
            if !body.is_empty() && !body.ends_with('\n') {
                body.push('\n');
            }
            let end = if closed {
                all[j].0 + all[j].1.len()
            } else {
                text.len()
            };
            blocks.push(Block {
                span: at..end,
                line: i + 1,
                kind: BlockKind::Fence(Fence {
                    lang,
                    attrs,
                    body,
                    closed,
                }),
            });
            i = if closed { j + 1 } else { all.len() };
            continue;
        }
        if prose.is_none() {
            prose = Some((at, i));
        }
        i += 1;
    }
    flush(&mut prose, text.len(), &mut blocks);
    Source { front, blocks }
}

/// A fence as text: the opening line with its info string, the body, the closing line. Uses a
/// backtick run one longer than the longest run in the body, at least three.
pub fn emit_fence(lang: &str, attrs: Option<&Attrs>, body: &str) -> String {
    let longest = body
        .lines()
        .map(|l| {
            let t = l.trim_start();
            t.chars().take_while(|c| *c == '`').count()
        })
        .max()
        .unwrap_or(0);
    let marker = "`".repeat((longest + 1).max(3));
    let mut out = String::new();
    out.push_str(&marker);
    out.push_str(lang);
    if let Some(attrs) = attrs {
        out.push(' ');
        out.push_str(&attrs.emit());
    }
    out.push('\n');
    out.push_str(body);
    if !body.is_empty() && !body.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&marker);
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_parse_ids_flags_and_quoted_values() {
        let a = parse_attrs(r#"#rotate needs=lag,b confirm="This changes \"prod\"" readonly"#);
        assert_eq!(a.id.as_deref(), Some("rotate"));
        assert_eq!(a.get("needs"), Some("lag,b"));
        assert_eq!(a.get("confirm"), Some("This changes \"prod\""));
        assert!(a.has("readonly"));
        let again = parse_attrs(&a.emit()[1..a.emit().len() - 1]);
        assert_eq!(again, a);
    }

    #[test]
    fn empty_needs_is_an_empty_value() {
        let a = parse_attrs("#root needs=");
        assert_eq!(a.get("needs"), Some(""));
    }

    #[test]
    fn scan_finds_frontmatter_fences_and_prose_and_spans_cover_the_text() {
        let text = "---\nrunbook: x\n---\n\n# T\n\n```sh {#a}\necho hi\n```\n\ntext\n~~~~output {cell=a}\n```\n~~~~\n";
        let src = scan(text);
        assert_eq!(src.front.as_ref().unwrap().0, "runbook: x\n");
        let kinds: Vec<_> = src
            .blocks
            .iter()
            .map(|b| match &b.kind {
                BlockKind::Prose => "prose".to_string(),
                BlockKind::Fence(f) => f.lang.clone(),
            })
            .collect();
        assert_eq!(kinds, ["prose", "sh", "prose", "output"]);
        let BlockKind::Fence(out) = &src.blocks[3].kind else {
            panic!()
        };
        assert_eq!(out.body, "```\n");
        // Spans are contiguous after the frontmatter.
        let mut at = src.front.as_ref().unwrap().1.end;
        for b in &src.blocks {
            assert_eq!(b.span.start, at);
            at = b.span.end;
        }
        assert_eq!(at, text.len());
        assert_eq!(src.blocks[1].line, 7);
    }

    #[test]
    fn emit_fence_lengthens_the_marker_past_the_body() {
        let f = emit_fence("output", None, "```\ninner\n```\n");
        assert!(f.starts_with("````output\n"));
        let src = scan(&f);
        let BlockKind::Fence(fence) = &src.blocks[0].kind else {
            panic!()
        };
        assert_eq!(fence.body, "```\ninner\n```\n");
    }
}
