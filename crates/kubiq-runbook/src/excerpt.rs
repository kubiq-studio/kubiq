//! The bounded excerpt a finished attempt leaves in the document (proposal §3.1): the first and last
//! lines, with their real line numbers, a summary of what was left out, and a byte cap.
//!
//! [`Collector`] is fed the redacted capture as it streams, so nothing ever re-reads the log: it
//! keeps the head, a ring of the tail and the counts.

use std::collections::VecDeque;

/// Lines kept from the start and from the end.
pub const HEAD: usize = 20;
pub const TAIL: usize = 20;
/// The excerpt's byte cap.
pub const MAX_BYTES: usize = 8 * 1024;
/// A line longer than this is clipped in the excerpt (the log keeps it whole).
pub const MAX_LINE: usize = 400;

#[derive(Debug, Default)]
pub struct Collector {
    head: Vec<(u64, String)>,
    tail: VecDeque<(u64, String)>,
    partial: Vec<u8>,
    lines: u64,
    bytes: u64,
}

impl Collector {
    pub fn new() -> Collector {
        Collector::default()
    }

    pub fn push(&mut self, chunk: &[u8]) {
        self.bytes += chunk.len() as u64;
        for &b in chunk {
            if b == b'\n' {
                let line = std::mem::take(&mut self.partial);
                self.line(line);
            } else if self.partial.len() <= MAX_LINE * 4 {
                self.partial.push(b);
            }
        }
    }

    fn line(&mut self, bytes: Vec<u8>) {
        self.lines += 1;
        let text = clip(&strip_ansi(&String::from_utf8_lossy(&bytes)));
        if self.head.len() < HEAD {
            self.head.push((self.lines, text));
        } else {
            self.tail.push_back((self.lines, text));
            if self.tail.len() > TAIL {
                self.tail.pop_front();
            }
        }
    }

    /// Close the last line and answer (lines, bytes, excerpt).
    pub fn finish(mut self) -> (u64, u64, String) {
        if !self.partial.is_empty() {
            let line = std::mem::take(&mut self.partial);
            self.line(line);
        }
        let excerpt = render(
            &self.head,
            self.tail.make_contiguous(),
            self.lines,
            self.bytes,
        );
        (self.lines, self.bytes, excerpt)
    }
}

fn clip(line: &str) -> String {
    if line.chars().count() <= MAX_LINE {
        return line.to_string();
    }
    let kept: String = line.chars().take(MAX_LINE).collect();
    format!("{kept} …[clipped]")
}

/// Remove ANSI escape sequences (CSI and OSC) for the text excerpt. The log keeps them.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            if c != '\r' {
                out.push(c);
            }
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                for n in chars.by_ref() {
                    if ('@'..='~').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                while let Some(n) = chars.next() {
                    if n == '\u{7}' {
                        break;
                    }
                    if n == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {
                chars.next();
            }
        }
    }
    out
}

/// `6.1 MB`, `2.0 KB`, `512 B`.
pub fn size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// `48 213`: thousands separated by a space, as the proposal writes counts.
pub fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

fn render(head: &[(u64, String)], tail: &[(u64, String)], lines: u64, bytes: u64) -> String {
    let width = lines.max(1).to_string().len();
    let numbered = |(n, t): &(u64, String)| format!("{n:>width$} | {t}\n");
    let mut out = String::new();
    for l in head {
        out.push_str(&numbered(l));
    }
    let shown = (head.len() + tail.len()) as u64;
    if lines > shown {
        let elided = lines - shown;
        out.push_str(&format!(
            "… {} lines / {} elided — open full output\n",
            count(elided),
            size(bytes)
        ));
    }
    for l in tail {
        out.push_str(&numbered(l));
    }
    if out.len() > MAX_BYTES {
        // Keep the summary and as much of each end as fits, cutting on line boundaries.
        let mut cut = String::new();
        let half = MAX_BYTES / 2;
        for l in out.lines() {
            if cut.len() + l.len() + 1 > half {
                break;
            }
            cut.push_str(l);
            cut.push('\n');
        }
        let mut end: Vec<&str> = Vec::new();
        let mut used = 0;
        for l in out.lines().rev() {
            if used + l.len() + 1 > half - 80 {
                break;
            }
            used += l.len() + 1;
            end.push(l);
        }
        cut.push_str(&format!(
            "… excerpt capped at {} — {} lines / {} in the log — open full output\n",
            size(MAX_BYTES as u64),
            count(lines),
            size(bytes)
        ));
        for l in end.into_iter().rev() {
            cut.push_str(l);
            cut.push('\n');
        }
        return cut;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(text: &str, chunk: usize) -> (u64, u64, String) {
        let mut c = Collector::new();
        for part in text.as_bytes().chunks(chunk.max(1)) {
            c.push(part);
        }
        c.finish()
    }

    #[test]
    fn a_short_output_is_kept_whole_with_line_numbers() {
        let (lines, bytes, ex) = collect("one\ntwo\nthree", 2);
        assert_eq!((lines, bytes), (3, 13));
        assert_eq!(ex, "1 | one\n2 | two\n3 | three\n");
    }

    #[test]
    fn a_long_output_keeps_head_and_tail_with_real_numbers_and_a_summary() {
        let text: String = (1..=48213).map(|n| format!("line {n}\n")).collect();
        let (lines, bytes, ex) = collect(&text, 4096);
        assert_eq!(lines, 48213);
        assert_eq!(bytes, text.len() as u64);
        let rows: Vec<&str> = ex.lines().collect();
        assert_eq!(rows.len(), HEAD + 1 + TAIL);
        assert_eq!(rows[0], "    1 | line 1");
        assert!(
            rows[HEAD].starts_with("… 48 173 lines / "),
            "{}",
            rows[HEAD]
        );
        assert!(rows[HEAD].ends_with("elided — open full output"));
        assert_eq!(rows[HEAD + 1], "48194 | line 48194");
        assert_eq!(*rows.last().unwrap(), "48213 | line 48213");
    }

    #[test]
    fn huge_lines_are_clipped_and_the_excerpt_is_capped() {
        let text: String = (0..60).map(|_| "x".repeat(5000) + "\n").collect();
        let (_, _, ex) = collect(&text, 999);
        assert!(ex.len() <= MAX_BYTES, "{}", ex.len());
        assert!(ex.contains("[clipped]"));
        assert!(ex.contains("excerpt capped"));
    }

    #[test]
    fn ansi_is_stripped_from_the_excerpt() {
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m ok\r"), "red ok");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}x"), "x");
    }

    #[test]
    fn counts_and_sizes_read_like_the_proposal() {
        assert_eq!(count(48213), "48 213");
        assert_eq!(count(7), "7");
        assert_eq!(size(6_400_000), "6.1 MB");
    }
}
