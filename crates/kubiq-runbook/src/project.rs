//! Writing the evidence into the document: the host-owned `output` blocks under each cell.
//!
//! The evidence is the truth and the blocks are its projection, so projecting is idempotent and
//! total: every `output` block of a cell that has evidence is removed and regenerated, wherever it
//! was and whoever wrote it, and every other byte of the file stays as it was. A cell with no
//! evidence keeps whatever `output` blocks it has — a runbook copied in from elsewhere keeps its
//! record. This is also how an agent's or an editor's write is merged with the host's: whatever
//! they did to an `output` block, the next projection puts the evidence back.

use std::collections::{BTreeSet, HashMap};

use crate::evidence::{Attempt, DecisionKind, Evidence, Outcome};
use crate::excerpt;
use crate::format::{self, Attrs, BlockKind};
use crate::model::{OUTPUT, Runbook};

/// The `cell=` an output block names.
fn cell_of(attrs: Option<&Attrs>) -> Option<&str> {
    attrs.and_then(|a| a.get("cell"))
}

fn took(ms: u64) -> String {
    if ms < 10_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}s", ms / 1000)
    }
}

fn outcome_attr(attrs: &mut Attrs, outcome: &Outcome) {
    match outcome {
        Outcome::Exit { code } => attrs.pairs.push(("exit".into(), code.to_string())),
        Outcome::Cancelled => attrs.pairs.push(("outcome".into(), "cancelled".into())),
        Outcome::TimedOut => attrs.pairs.push(("outcome".into(), "timed-out".into())),
        Outcome::Interrupted => attrs.pairs.push(("outcome".into(), "interrupted".into())),
        Outcome::Error { .. } => attrs.pairs.push(("outcome".into(), "error".into())),
    }
}

fn attempt_block(a: &Attempt, collapsed: bool) -> String {
    let f = a.finished.as_ref().expect("a finished attempt");
    let s = &a.started;
    let mut attrs = Attrs::default();
    let mut push = |k: &str, v: String| attrs.pairs.push((k.into(), v));
    push("cell", s.cell.clone());
    push("run", s.run.to_string());
    push("attempt", s.attempt.to_string());
    outcome_attr(&mut attrs, &f.outcome);
    let mut push = |k: &str, v: String| attrs.pairs.push((k.into(), v));
    if !collapsed {
        push("at", f.at.clone());
        push("by", s.by.clone());
        push("took", took(f.took_ms));
        push("hash", s.hash.clone());
        push("lines", f.lines.to_string());
    }
    push("bytes", f.bytes.to_string());
    push("log", s.log.clone());
    if f.truncated {
        attrs.flags.push("truncated".into());
    }
    if s.forced {
        attrs.flags.push("forced".into());
    }
    if collapsed {
        attrs.flags.push("collapsed".into());
    }
    let body = if collapsed {
        format!(
            "run {} · {} · {} · {}\n",
            s.run,
            f.outcome.label(),
            excerpt::size(f.bytes),
            s.log
        )
    } else {
        let mut body = f.excerpt.clone();
        if let Outcome::Error { reason } = &f.outcome {
            body.push_str(&format!("[runbook: {reason}]\n"));
        }
        if f.truncated {
            body.push_str("[runbook: the log reached its cap; later output was discarded]\n");
        }
        body
    };
    format::emit_fence(OUTPUT, Some(&attrs), &body)
}

/// The generated blocks of every cell that has evidence.
fn generated(evidence: &Evidence) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let mut cells: BTreeSet<String> = BTreeSet::new();
    let attempts = evidence.attempts();
    for a in &attempts {
        cells.insert(a.started.cell.clone());
    }
    for r in &evidence.records {
        match r {
            crate::evidence::Record::Verdict(v) => {
                cells.insert(v.cell.clone());
            }
            crate::evidence::Record::Decision(d) => {
                cells.insert(d.cell.clone());
            }
            _ => {}
        }
    }
    for cell in cells {
        let mut blocks = Vec::new();
        let finished: Vec<&Attempt> = attempts
            .iter()
            .filter(|a| a.started.cell == cell && a.finished.is_some())
            .collect();
        if let Some((last, earlier)) = finished.split_last() {
            for a in earlier {
                blocks.push(attempt_block(a, true));
            }
            blocks.push(attempt_block(last, false));
        }
        for v in evidence.verdicts(&cell) {
            let mut attrs = Attrs::default();
            attrs.pairs.push(("cell".into(), cell.clone()));
            attrs.pairs.push((
                "verdict".into(),
                if v.pass { "pass" } else { "fail" }.into(),
            ));
            if let Some(of) = v.of {
                attrs.pairs.push(("of".into(), of.to_string()));
            }
            attrs.pairs.push(("at".into(), v.at.clone()));
            attrs.pairs.push(("by".into(), v.by.clone()));
            let body = if v.note.is_empty() {
                String::new()
            } else {
                format!("{}\n", v.note.trim_end())
            };
            blocks.push(format::emit_fence(OUTPUT, Some(&attrs), &body));
        }
        for d in evidence.decisions(&cell) {
            let mut attrs = Attrs::default();
            attrs.pairs.push(("cell".into(), cell.clone()));
            let kind = match d.kind {
                DecisionKind::Skip => "skip",
                DecisionKind::Override => "override",
                DecisionKind::AcceptReadonly => "accept-readonly",
            };
            attrs.pairs.push(("decision".into(), kind.into()));
            attrs.pairs.push(("at".into(), d.at.clone()));
            attrs.pairs.push(("by".into(), d.by.clone()));
            blocks.push(format::emit_fence(
                OUTPUT,
                Some(&attrs),
                &format!("{}\n", d.reason.trim_end()),
            ));
        }
        out.insert(cell, blocks);
    }
    out
}

/// Project `evidence` into `text`.
pub fn project(text: &str, evidence: &Evidence) -> String {
    let mut generated = generated(evidence);
    let book = Runbook::parse(text);
    let src = &book.source;
    let cell_blocks: HashMap<usize, &str> = book
        .cells
        .iter()
        .map(|c| (c.block, c.id.as_str()))
        .collect();

    // Which blocks go: host-owned outputs of cells with evidence, and the blank separator before
    // each.
    let owned = |i: usize| -> bool {
        match &src.blocks[i].kind {
            BlockKind::Fence(f) if f.lang == OUTPUT => {
                cell_of(f.attrs.as_ref()).is_some_and(|c| generated.contains_key(c))
            }
            _ => false,
        }
    };
    let mut drop = vec![false; src.blocks.len()];
    for i in 0..src.blocks.len() {
        if owned(i) {
            drop[i] = true;
            if i > 0
                && matches!(src.blocks[i - 1].kind, BlockKind::Prose)
                && text[src.blocks[i - 1].span.clone()].trim().is_empty()
            {
                drop[i - 1] = true;
            }
        }
    }

    let mut out = String::with_capacity(text.len());
    let start = src.front.as_ref().map(|(_, s)| s.end).unwrap_or(0);
    out.push_str(&text[..start]);
    for (i, block) in src.blocks.iter().enumerate() {
        if !drop[i] {
            let piece = &text[block.span.clone()];
            out.push_str(piece);
            if !piece.ends_with('\n') && cell_blocks.contains_key(&i) {
                out.push('\n');
            }
        }
        if let Some(cell) = cell_blocks.get(&i)
            && let Some(blocks) = generated.remove(*cell)
        {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            for b in blocks {
                out.push('\n');
                out.push_str(&b);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::{Decision, Finished, LOCAL_USER, Record, Started, Verdict};

    fn attempt(ev: &mut Evidence, cell: &str, code: i32, excerpt: &str) -> u64 {
        let id = ev.next_id();
        ev.records.push(Record::Started(Started {
            attempt: id,
            cell: cell.into(),
            run: ev.of_cell(cell).len() as u32 + 1,
            prev: None,
            by: LOCAL_USER.into(),
            at: "2026-11-16T10:00:00Z".into(),
            shell: "sh".into(),
            target: None,
            command: "x".into(),
            hash: "abc".into(),
            of: None,
            prereqs: vec![],
            forced: false,
            request: None,
            log: format!("logs/{cell}-{id}.log"),
        }));
        ev.records.push(Record::Finished(Finished {
            attempt: id,
            at: "2026-11-16T10:00:01Z".into(),
            outcome: Outcome::Exit { code },
            passed: code == 0,
            took_ms: 400,
            lines: 1,
            bytes: 6,
            truncated: false,
            excerpt: excerpt.into(),
        }));
        id
    }

    const DOC: &str = "---\nrunbook: x\n---\n\n# Title\n\nProse.\n\n```sh {#a}\necho hi\n```\n\nMiddle prose.\n\n```check {#a-ok check-of=a manual}\n- [ ] fine\n```\n\nEnd.\n";

    #[test]
    fn projection_writes_blocks_under_their_cells_and_preserves_everything_else() {
        let mut ev = Evidence::default();
        let first = attempt(&mut ev, "a", 1, "1 | boom\n");
        attempt(&mut ev, "a", 0, "1 | hi\n");
        ev.records.push(Record::Verdict(Verdict {
            id: ev.next_id(),
            cell: "a-ok".into(),
            pass: true,
            note: "dashboard clean".into(),
            by: LOCAL_USER.into(),
            at: "t".into(),
            of: Some(first + 1),
            hash: "h".into(),
        }));
        let once = project(DOC, &ev);
        assert!(once.starts_with("---\nrunbook: x\n---\n\n# Title\n\nProse.\n\n```sh {#a}\necho hi\n```\n\n```output {cell=a run=1 attempt=1 exit=1 bytes=6 log=logs/a-1.log collapsed}\nrun 1 · exit 1 · 6 B · logs/a-1.log\n```\n"), "{once}");
        assert!(once.contains("```output {cell=a run=2 attempt=2 exit=0 at=2026-11-16T10:00:01Z by=local-user took=0.4s hash=abc lines=1 bytes=6 log=logs/a-2.log}\n1 | hi\n```\n\nMiddle prose."));
        assert!(once.contains("```output {cell=a-ok verdict=pass of=2 at=t by=local-user}\ndashboard clean\n```\n\nEnd.\n"));
        // Idempotent.
        assert_eq!(project(&once, &ev), once);
        // An agent's edit that mangled an output block is put back; its prose edit is kept.
        let mangled = once
            .replace("1 | hi", "1 | forged")
            .replace("Middle prose.", "Middle prose, edited.");
        let again = project(&mangled, &ev);
        assert!(again.contains("1 | hi"));
        assert!(!again.contains("forged"));
        assert!(again.contains("Middle prose, edited."));
        // No evidence, no change.
        assert_eq!(project(DOC, &Evidence::default()), DOC);
    }

    #[test]
    fn foreign_outputs_of_cells_without_evidence_are_kept_and_decisions_are_written() {
        let doc = "```sh {#a}\ntrue\n```\n\n```output {cell=a run=1 exit=0}\nold\n```\n```sh {#b}\ntrue\n```\n";
        let mut ev = Evidence::default();
        ev.records.push(Record::Decision(Decision {
            id: 1,
            cell: "b".into(),
            kind: DecisionKind::Skip,
            reason: "not needed in staging".into(),
            by: LOCAL_USER.into(),
            at: "t".into(),
            hash: "h".into(),
        }));
        let out = project(doc, &ev);
        assert!(out.contains("```output {cell=a run=1 exit=0}\nold\n```\n"));
        assert!(out.ends_with("```sh {#b}\ntrue\n```\n\n```output {cell=b decision=skip at=t by=local-user}\nnot needed in staging\n```\n"), "{out}");
        assert_eq!(project(&out, &ev), out);
    }
}
