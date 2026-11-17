//! What an output becomes on screen or in an agent's read: terminal text cleaned up, the richest
//! MIME type chosen, images decoded. Pure functions both halves call.

use crate::nbformat::Output;

/// Text as a terminal would leave it: ANSI escape sequences removed, a `\r` returning to the start
/// of the line (a progress bar's redraw), a backspace erasing one character.
pub fn terminal_text(raw: &str) -> String {
    let mut lines: Vec<String> = vec![String::new()];
    let mut col = 0usize;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => {
                // CSI: ESC [ params final(0x40..0x7e); OSC: ESC ] … BEL or ESC \; else one char.
                match chars.peek() {
                    Some('[') => {
                        chars.next();
                        for c in chars.by_ref() {
                            if ('\u{40}'..='\u{7e}').contains(&c) {
                                break;
                            }
                        }
                    }
                    Some(']') => {
                        chars.next();
                        while let Some(c) = chars.next() {
                            if c == '\u{7}' {
                                break;
                            }
                            if c == '\u{1b}' {
                                chars.next();
                                break;
                            }
                        }
                    }
                    Some(_) => {
                        chars.next();
                    }
                    None => {}
                }
            }
            '\n' => {
                lines.push(String::new());
                col = 0;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    continue;
                }
                col = 0;
            }
            '\u{8}' => col = col.saturating_sub(1),
            c => {
                let line = lines.last_mut().expect("one line at least");
                let mut cs: Vec<char> = line.chars().collect();
                if col < cs.len() {
                    cs[col] = c;
                } else {
                    cs.push(c);
                }
                col += 1;
                *line = cs.into_iter().collect();
            }
        }
    }
    lines.join("\n")
}

/// The MIME types the native renderer draws, richest first.
pub const NATIVE: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/svg+xml",
    "text/markdown",
    "application/json",
    "text/plain",
];

/// The MIME type to draw from a bundle: the first of [`NATIVE`] it carries. `text/html` is drawn
/// only as its `text/plain` fallback (a pandas table carries one); a bundle with nothing native
/// returns `None` and is drawn as a placeholder naming its types.
pub fn richest(bundle: &[(String, String)]) -> Option<(&str, &str)> {
    NATIVE.iter().find_map(|want| {
        bundle
            .iter()
            .find(|(mime, _)| mime == want)
            .map(|(m, v)| (m.as_str(), v.as_str()))
    })
}

/// Standard base64 (whitespace ignored), as image outputs carry it.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    }
    let bytes: Vec<u8> = text
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for (n, b) in chunk.iter().enumerate() {
            acc |= val(*b)? << (18 - 6 * n);
        }
        let take = match chunk.len() {
            4 => 3,
            3 => 2,
            2 => 1,
            _ => return None,
        };
        out.extend_from_slice(&acc.to_be_bytes()[1..1 + take]);
    }
    Some(out)
}

/// Head and tail of a long text, with what was cut said in the middle.
pub fn trim(text: &str, head: usize, tail: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= head + tail + 1 {
        return text.to_string();
    }
    let cut = lines.len() - head - tail;
    let mut out = lines[..head].join("\n");
    out.push_str(&format!("\n… {cut} lines cut …\n"));
    out.push_str(&lines[lines.len() - tail..].join("\n"));
    out
}

/// One output as an agent reads it: plain text, ANSI removed and trimmed, images and other rich
/// types named rather than inlined.
pub fn agent_text(output: &Output) -> String {
    const HEAD: usize = 40;
    const TAIL: usize = 20;
    match output {
        Output::Stream { name, text } => format!("[{name}]\n{}", trim(&terminal_text(text), HEAD, TAIL)),
        Output::Error {
            ename,
            evalue,
            traceback,
        } => format!(
            "[error] {ename}: {evalue}\n{}",
            trim(&terminal_text(&traceback.join("\n")), HEAD, TAIL)
        ),
        Output::Data { bundle, .. } => {
            let plain = bundle.iter().find(|(m, _)| m == "text/plain").map(|(_, v)| v);
            let others: Vec<&str> = bundle
                .iter()
                .map(|(m, _)| m.as_str())
                .filter(|m| *m != "text/plain")
                .collect();
            let mut out = String::new();
            if !others.is_empty() {
                out.push_str(&format!("[{}]", others.join(", ")));
            }
            if let Some(plain) = plain {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&trim(&terminal_text(plain), HEAD, TAIL));
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carriage_return_and_ansi() {
        assert_eq!(terminal_text("10%\r50%\r100%\n"), "100%\n");
        assert_eq!(terminal_text("\u{1b}[31mred\u{1b}[0m ok"), "red ok");
        assert_eq!(terminal_text("ab\u{8}c"), "ac");
        assert_eq!(terminal_text("a\r\nb"), "a\nb");
    }

    #[test]
    fn base64_round() {
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64_decode("aGVs\nbG8h").unwrap(), b"hello!");
    }

    #[test]
    fn html_falls_back_to_plain() {
        let bundle = vec![
            ("text/html".to_string(), "<table/>".to_string()),
            ("text/plain".to_string(), "df".to_string()),
        ];
        assert_eq!(richest(&bundle), Some(("text/plain", "df")));
        let none = vec![("application/vnd.plotly+json".to_string(), "{}".to_string())];
        assert_eq!(richest(&none), None);
    }
}
