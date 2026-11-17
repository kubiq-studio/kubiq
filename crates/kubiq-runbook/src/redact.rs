//! Masking known secret values in a byte stream, before anything is written, streamed or answered
//! (proposal §3.1, R6).
//!
//! The capture arrives in arbitrary chunks, so a secret can straddle two of them. [`Masker`] holds
//! back the longest tail of what it has seen that could still be the start of a secret, and
//! releases it once the next chunk proves it is not. Matching is on raw bytes, so ANSI sequences
//! around a secret do not hide it; a secret with escape sequences written *inside* it is not
//! recognised — the stated limit of masking known values.

use crate::params::MASK;

#[derive(Debug, Clone)]
pub struct Masker {
    secrets: Vec<Vec<u8>>,
    held: Vec<u8>,
}

impl Masker {
    /// Secrets shorter than 4 bytes are not masked: masking `a` would destroy the capture and
    /// protect nothing.
    pub fn new(secrets: &[String]) -> Masker {
        let mut secrets: Vec<Vec<u8>> = secrets
            .iter()
            .filter(|s| s.len() >= 4)
            .map(|s| s.as_bytes().to_vec())
            .collect();
        // Longest first, so a secret that contains another is masked whole.
        secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        secrets.dedup();
        Masker {
            secrets,
            held: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.secrets.is_empty()
    }

    /// Feed a chunk; answer what may be released now.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        if self.secrets.is_empty() {
            return chunk.to_vec();
        }
        let mut buf = std::mem::take(&mut self.held);
        buf.extend_from_slice(chunk);
        let (out, held) = self.replace(&buf, false);
        self.held = held;
        out
    }

    /// Release whatever is held: the stream ended, so it cannot become a secret.
    pub fn finish(&mut self) -> Vec<u8> {
        let buf = std::mem::take(&mut self.held);
        self.replace(&buf, true).0
    }

    /// Mask a whole text at once.
    pub fn mask_all(&self, text: &str) -> String {
        String::from_utf8_lossy(&self.replace(text.as_bytes(), true).0).into_owned()
    }

    /// Mask `buf`. Unless `last`, stop at the first position where the rest of the buffer could
    /// still grow into a secret (a longer one than any that already matches there), and answer
    /// the rest as held.
    fn replace(&self, buf: &[u8], last: bool) -> (Vec<u8>, Vec<u8>) {
        let mut out = Vec::with_capacity(buf.len());
        let mut i = 0;
        while i < buf.len() {
            let rest = &buf[i..];
            if !last
                && self
                    .secrets
                    .iter()
                    .any(|s| s.len() > rest.len() && s.starts_with(rest))
            {
                return (out, rest.to_vec());
            }
            // Longest first: a secret that contains another is masked whole.
            if let Some(s) = self.secrets.iter().find(|s| rest.starts_with(s)) {
                out.extend_from_slice(MASK.as_bytes());
                i += s.len();
            } else {
                out.push(buf[i]);
                i += 1;
            }
        }
        (out, Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(secrets: &[&str], text: &str, chunk: usize) -> String {
        let secrets: Vec<String> = secrets.iter().map(|s| s.to_string()).collect();
        let mut m = Masker::new(&secrets);
        let mut out = Vec::new();
        for part in text.as_bytes().chunks(chunk) {
            out.extend(m.push(part));
        }
        out.extend(m.finish());
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn a_secret_split_across_every_possible_boundary_is_masked() {
        let text = "user=admin pass=hunter2-xyz end; again hunter2-xyz\n";
        for chunk in 1..=text.len() {
            assert_eq!(
                stream(&["hunter2-xyz"], text, chunk),
                "user=admin pass=••• end; again •••\n",
                "chunk {chunk}"
            );
        }
    }

    #[test]
    fn ansi_around_a_secret_does_not_hide_it_and_near_misses_are_released() {
        let text = "\u{1b}[31mtoken-abcd\u{1b}[0m tok token-abc\n";
        assert_eq!(
            stream(&["token-abcd"], text, 3),
            "\u{1b}[31m•••\u{1b}[0m tok token-abc\n"
        );
    }

    #[test]
    fn overlapping_secrets_mask_the_longer_one_whole_and_tiny_ones_are_ignored() {
        assert_eq!(
            stream(&["abcd", "abcdefgh", "x"], "abcdefgh abcd x", 2),
            "••• ••• x"
        );
    }
}
