//! The evidence: an append-only list of records, one JSON object per line on disk. Every state a
//! step shows is derived from these and the current text ([`crate::graph`]); none is stored.
//!
//! An attempt is two records, [`Started`] and [`Finished`], so a host that dies mid-run leaves a
//! start with no finish, which the next open closes as [`Outcome::Interrupted`]. A retry is a new
//! attempt naming the one before it (`prev`); nothing is ever rewritten.

use serde::{Deserialize, Serialize};

/// Who did something. `local-user` unless an authenticated identity is known; an agent never
/// appears as the actor of a run or a verdict.
pub const LOCAL_USER: &str = "local-user";

/// What a prerequisite was when an attempt started: the cell and its proof token
/// ([`crate::graph::Proof`]). A different token later makes the attempt stale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proof {
    pub cell: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Started {
    /// The attempt id, unique within the document's evidence.
    pub attempt: u64,
    pub cell: String,
    /// The run number of this cell: 1 for its first attempt.
    pub run: u32,
    /// The attempt this one retries, when there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev: Option<u64>,
    pub by: String,
    pub at: String,
    pub shell: String,
    /// Where it ran, when not on the host (`ssh:<alias>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// The command after substitution, secrets as `•••`.
    pub command: String,
    pub hash: String,
    /// For a check: the step attempt it checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub of: Option<u64>,
    /// The prerequisites' proofs at start.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prereqs: Vec<Proof>,
    /// Started on a waiting step after the human confirmed skipping its prerequisites.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub forced: bool,
    /// The pending request this run answered, when it answered one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<u64>,
    /// The log, relative to the document's evidence directory.
    pub log: String,
}

/// How an attempt ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Outcome {
    Exit {
        code: i32,
    },
    /// Stopped by the human.
    Cancelled,
    TimedOut,
    /// The host went away while it ran.
    Interrupted,
    /// It never ran, or capture failed: the reason.
    Error {
        reason: String,
    },
}

impl Outcome {
    pub fn label(&self) -> String {
        match self {
            Outcome::Exit { code } => format!("exit {code}"),
            Outcome::Cancelled => "cancelled".into(),
            Outcome::TimedOut => "timed out".into(),
            Outcome::Interrupted => "interrupted".into(),
            Outcome::Error { reason } => format!("error: {reason}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finished {
    pub attempt: u64,
    pub at: String,
    pub outcome: Outcome,
    /// Exit 0 for a step; for a check exit 0, or its `expect=` matched.
    pub passed: bool,
    pub took_ms: u64,
    /// What the command printed, whether or not all of it was kept.
    pub lines: u64,
    pub bytes: u64,
    /// The log stopped at its cap.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
    /// The bounded head-and-tail excerpt ([`crate::excerpt`]), already redacted.
    #[serde(default)]
    pub excerpt: String,
}

/// A human's verdict on a manual check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    pub id: u64,
    pub cell: String,
    pub pass: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    pub by: String,
    pub at: String,
    /// The step attempt it judged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub of: Option<u64>,
    /// The checklist's hash when judged.
    pub hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionKind {
    /// Do not run this step; its dependents proceed.
    Skip,
    /// It failed; count it done anyway.
    Override,
    /// A human accepted this cell's `readonly` mark, so `gate: readonly-auto` may run it when an
    /// agent asks. Bound to the command hash: an edit needs a new acceptance.
    AcceptReadonly,
}

/// A human's skip or override, with why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub id: u64,
    pub cell: String,
    pub kind: DecisionKind,
    pub reason: String,
    pub by: String,
    pub at: String,
    /// The step's command hash when decided: an edit retires the decision.
    pub hash: String,
}

/// One line of the evidence file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "kebab-case")]
pub enum Record {
    Started(Started),
    Finished(Finished),
    Verdict(Verdict),
    Decision(Decision),
}

impl Record {
    /// The id this record introduces (an attempt's on its start).
    pub fn id(&self) -> Option<u64> {
        match self {
            Record::Started(s) => Some(s.attempt),
            Record::Finished(_) => None,
            Record::Verdict(v) => Some(v.id),
            Record::Decision(d) => Some(d.id),
        }
    }
}

/// One attempt, folded from its two records.
#[derive(Debug, Clone, PartialEq)]
pub struct Attempt {
    pub started: Started,
    pub finished: Option<Finished>,
}

impl Attempt {
    pub fn id(&self) -> u64 {
        self.started.attempt
    }
    pub fn passed(&self) -> bool {
        self.finished.as_ref().is_some_and(|f| f.passed)
    }
}

/// Every record of one document, in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Evidence {
    pub records: Vec<Record>,
}

impl Evidence {
    /// Read JSON lines. A line that does not parse is skipped and counted.
    pub fn from_jsonl(text: &str) -> (Evidence, usize) {
        let mut records = Vec::new();
        let mut bad = 0;
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            match serde_json::from_str(line) {
                Ok(r) => records.push(r),
                Err(_) => bad += 1,
            }
        }
        (Evidence { records }, bad)
    }

    pub fn line(record: &Record) -> String {
        let mut s = serde_json::to_string(record).unwrap_or_default();
        s.push('\n');
        s
    }

    /// The next free id.
    pub fn next_id(&self) -> u64 {
        self.records
            .iter()
            .filter_map(Record::id)
            .max()
            .unwrap_or(0)
            + 1
    }

    pub fn attempts(&self) -> Vec<Attempt> {
        let mut out: Vec<Attempt> = Vec::new();
        for r in &self.records {
            match r {
                Record::Started(s) => out.push(Attempt {
                    started: s.clone(),
                    finished: None,
                }),
                Record::Finished(f) => {
                    if let Some(a) = out.iter_mut().find(|a| a.id() == f.attempt) {
                        a.finished = Some(f.clone());
                    }
                }
                _ => {}
            }
        }
        out
    }

    pub fn attempt(&self, id: u64) -> Option<Attempt> {
        self.attempts().into_iter().find(|a| a.id() == id)
    }

    /// The attempts of one cell, oldest first.
    pub fn of_cell(&self, cell: &str) -> Vec<Attempt> {
        self.attempts()
            .into_iter()
            .filter(|a| a.started.cell == cell)
            .collect()
    }

    /// Attempts started and not finished.
    pub fn unfinished(&self) -> Vec<Started> {
        self.attempts()
            .into_iter()
            .filter(|a| a.finished.is_none())
            .map(|a| a.started)
            .collect()
    }

    pub fn verdicts(&self, cell: &str) -> Vec<&Verdict> {
        self.records
            .iter()
            .filter_map(|r| match r {
                Record::Verdict(v) if v.cell == cell => Some(v),
                _ => None,
            })
            .collect()
    }

    pub fn decisions(&self, cell: &str) -> Vec<&Decision> {
        self.records
            .iter()
            .filter_map(|r| match r {
                Record::Decision(d) if d.cell == cell => Some(d),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip_through_jsonl_and_attempts_fold() {
        let start = Record::Started(Started {
            attempt: 1,
            cell: "a".into(),
            run: 1,
            prev: None,
            by: LOCAL_USER.into(),
            at: "t".into(),
            shell: "sh".into(),
            target: None,
            command: "true".into(),
            hash: "h".into(),
            of: None,
            prereqs: vec![],
            forced: false,
            request: None,
            log: "logs/a-1.log".into(),
        });
        let end = Record::Finished(Finished {
            attempt: 1,
            at: "t2".into(),
            outcome: Outcome::Exit { code: 0 },
            passed: true,
            took_ms: 3,
            lines: 0,
            bytes: 0,
            truncated: false,
            excerpt: String::new(),
        });
        let text = Evidence::line(&start) + &Evidence::line(&end) + "garbage\n";
        let (ev, bad) = Evidence::from_jsonl(&text);
        assert_eq!(bad, 1);
        assert_eq!(ev.records, vec![start, end]);
        assert!(ev.attempts()[0].passed());
        assert_eq!(ev.next_id(), 2);
        assert!(ev.unfinished().is_empty());
    }
}
