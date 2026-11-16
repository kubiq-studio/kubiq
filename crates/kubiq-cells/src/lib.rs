//! The shared layer of Kubiq's cell documents — the notebook (`kubiq-notebook`, a `.ipynb` on a
//! persistent kernel) and the runbook (`kubiq-runbook`, markdown fences run as fresh processes).
//!
//! The two keep opposite state models on purpose (a kernel's hidden state against a runbook's
//! evidence log), so what is shared is the vocabulary and the decisions both make the same way,
//! not a common document type:
//!
//! - [`Origin`] — who made a change or asked for a run: a person, an agent, the disk.
//! - [`RunStatus`] / [`RunState`] — how one run ended, and what a cell's row shows.
//! - [`RunMarks`] — staleness: a cell whose source moved on since it ran, or that ran before a
//!   cell above it ran again. A kind that derives its state (the runbook's needs graph) keeps its
//!   own and may ignore this.
//! - [`CellDocument`] — the read side every kind answers the same way: which cells, in order, with
//!   what source. What an agent's `read` and a view's row list start from.
//! - [`RunGate`] — whether a run an agent asked for runs now, waits for a person, or is refused.
//!   One component, one decision shape; the policy behind it is the kind's.
//! - [`DocEvent`] — something that happened on a document, for the agent bound to it: delivered
//!   through the host's doc queue with the annotated documents' comments.
//!
//! - [`process`] (feature `process`, the one part with I/O) — a cell's process as a group,
//!   interrupted and killed as one on Unix and Windows (Job object, `JPY_INTERRUPT_EVENT`).
//!
//! The host's half of the shared layer is `kubiq-host/src/cells.rs` (the executor trait); the
//! interface's is `crates/kubiq/src/ui/cells/` (the cell rows, outputs, the zoom dialog).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[cfg(feature = "process")]
pub mod process;

/// Who made a change, or asked for a run.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Origin {
    Human,
    /// An agent, by the id the host knows it by (its conversation's `AgentId`, as text).
    Agent(String),
    /// The file changed on disk and was re-read.
    Disk,
}

impl Origin {
    /// `human`, `agent:<id>`, `disk` — how a tool result and a log line say it.
    pub fn label(&self) -> String {
        match self {
            Origin::Human => "human".into(),
            Origin::Agent(id) => format!("agent:{id}"),
            Origin::Disk => "disk".into(),
        }
    }
}

/// How one run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    Ok,
    Error,
    /// Interrupted, stopped, or the executor went away under it.
    Aborted,
}

/// What a cell's row shows about running it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunState {
    /// Never ran here.
    Idle,
    Queued,
    Running,
    /// Waiting for a person's yes (a gated agent run).
    Waiting,
    Ok,
    Error,
    Aborted,
    /// Ran, but what it ran no longer holds (see [`RunMarks`]).
    Stale,
}

impl From<RunStatus> for RunState {
    fn from(status: RunStatus) -> Self {
        match status {
            RunStatus::Ok => RunState::Ok,
            RunStatus::Error => RunState::Error,
            RunStatus::Aborted => RunState::Aborted,
        }
    }
}

/// What each run left behind: the source it ran, and when, in this document's own sequence.
#[derive(Debug, Clone, Default)]
pub struct RunMarks {
    marks: HashMap<String, (String, u64)>,
    seq: u64,
}

impl RunMarks {
    /// `cell` started running `source` now.
    pub fn started(&mut self, cell: &str, source: &str) {
        self.seq += 1;
        self.marks.insert(cell.to_string(), (source.to_string(), self.seq));
    }

    pub fn forget(&mut self, cell: &str) {
        self.marks.remove(cell);
    }

    /// Keep only the cells `keep` answers yes for (a reload matched by id).
    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.marks.retain(|id, _| keep(id));
    }

    /// Whether the cell at `at` in `cells` (id, current source, in order) is stale: its source
    /// changed since it ran, or a cell above it ran after it. A cell that never ran is not.
    pub fn stale(&self, cells: &[(&str, &str)], at: usize) -> bool {
        let Some((id, source)) = cells.get(at) else {
            return false;
        };
        let Some((ran, seq)) = self.marks.get(*id) else {
            return false;
        };
        ran != source
            || cells[..at]
                .iter()
                .filter_map(|(above, _)| self.marks.get(*above))
                .any(|(_, s)| s > seq)
    }
}

/// The read side every cell document answers.
pub trait CellDocument {
    /// Cell ids, in document order.
    fn cell_ids(&self) -> Vec<String>;
    /// A cell's source, as it reads now.
    fn source(&self, id: &str) -> Option<String>;
    /// A short word for the cell's kind (`code`, `markdown`, `step`, `check`, …).
    fn kind_label(&self, id: &str) -> Option<&'static str>;
}

/// A run an agent asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateRequest {
    pub agent: String,
    pub cells: Vec<String>,
}

/// What a [`RunGate`] decides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GateDecision {
    Run,
    /// Waits for a person's yes; the reason is what the question says.
    Ask { reason: String },
    Refuse { reason: String },
}

/// Whether an agent's run goes ahead. The notebook's is its `ask`/`allow`/`deny` policy; the
/// runbook's binds an approval to the command it would run.
pub trait RunGate {
    fn decide(&self, request: &GateRequest) -> GateDecision;
}

/// Something that happened on a document, for the agent bound to it. Composed when it happens
/// (unlike a comment, an event has nothing to re-read later) and delivered by the host's doc
/// queue with whatever else waits for that agent, one prompt per turn boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocEvent {
    /// The document, project-relative.
    pub doc: String,
    /// Which server answers it (`kubiq-notebook`, `kubiq-runbook`), for the prompt's pointer.
    pub server: String,
    /// One line or a short paragraph: what happened.
    pub text: String,
}

/// The prompt for a batch of events, as the doc queue delivers it.
pub fn compose_events(events: &[DocEvent]) -> String {
    let mut text = String::new();
    for e in events {
        text.push_str(&format!("\n- `{}` ({}): {}", e.doc, e.server, e.text));
    }
    if text.is_empty() {
        return text;
    }
    format!("Events on documents bound to you:{text}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staleness_by_source_and_order() {
        let mut m = RunMarks::default();
        m.started("a", "1");
        m.started("b", "2");
        let cells = [("a", "1"), ("b", "2")];
        assert!(!m.stale(&cells, 0) && !m.stale(&cells, 1));
        m.started("a", "1");
        assert!(m.stale(&cells, 1), "a ran again after b");
        assert!(m.stale(&[("a", "9"), ("b", "2")], 0), "a's source moved on");
        assert!(!m.stale(&[("c", "x")], 0), "never ran");
    }

    #[test]
    fn events_compose() {
        let e = DocEvent {
            doc: "a.ipynb".into(),
            server: "kubiq-notebook".into(),
            text: "cell x finished: ok".into(),
        };
        assert!(compose_events(&[e]).contains("`a.ipynb` (kubiq-notebook): cell x finished"));
        assert!(compose_events(&[]).is_empty());
    }
}
