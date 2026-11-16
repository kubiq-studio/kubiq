//! The runbook viewer's interface state: one `Global`, keyed by (project, path), never a field on
//! `AppState`.
//!
//! **The document is the tab's buffer**: the viewer parses it to draw prose and cells (pure,
//! `kubiq_runbook`), and the editor saves it like any markdown file. What only the host knows —
//! each step's derived state, the commands after substitution and their hashes, pending agent
//! requests, the bound agent — arrives as a [`Snapshot`]; live tails arrive as frames into a
//! bounded ring per cell.

use std::collections::{BTreeMap, HashMap, VecDeque};

use gpui::{App, Entity, Global};
use gpui_component::input::InputState;
use kubiq_proto::ids::ProjectId;
use kubiq_proto::runbook::{ActionSpec, Approval, RunOptions, Snapshot, Tail};
use kubiq_proto::work::AgentId;

/// A runbook: its project and its project-relative path.
pub type Key = (ProjectId, String);

/// Lines a live tail keeps.
pub const TAIL_LINES: usize = 15;

/// One running cell's live tail: the newest lines and how many were passed over.
#[derive(Default, Clone)]
pub struct Ring {
    pub attempt: u64,
    pub lines: VecDeque<String>,
    pub skipped: u64,
}

impl Ring {
    pub fn push(&mut self, tail: Tail) {
        if tail.attempt != self.attempt {
            *self = Ring {
                attempt: tail.attempt,
                ..Ring::default()
            };
        }
        self.skipped += tail.skipped;
        for line in tail.lines {
            self.lines.push_back(line);
            if self.lines.len() > TAIL_LINES {
                self.lines.pop_front();
                self.skipped += 1;
            }
        }
    }
}

/// What a reason or note field is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    Skip,
    Override,
    Pass,
    Fail,
    AcceptReadonly,
}

/// A run that needs a second yes before it goes: the cells, the options it will carry, and what
/// the confirmation says.
#[derive(Clone)]
pub struct Confirm {
    pub approvals: Vec<Approval>,
    pub options: RunOptions,
    pub message: String,
}

#[derive(Default)]
pub struct Tab {
    /// `Open` has been sent.
    pub opened: bool,
    pub snapshot: Option<Snapshot>,
    pub bound: Option<AgentId>,
    pub agents_open: bool,
    pub tails: HashMap<String, Ring>,
    /// The selected cell, for the keys.
    pub selected: Option<String>,
    /// The open reason/note field: the cell, what it is for, the field.
    pub ask: Option<(String, Ask, Entity<InputState>)>,
    pub confirm: Option<Confirm>,
    /// The open action form: the action and its values.
    pub form: Option<(ActionSpec, BTreeMap<String, String>)>,
    pub form_inputs: HashMap<String, Entity<InputState>>,
    /// The last notice (a refusal, a finished run) and whether it is an error.
    pub notice: Option<(String, bool)>,
    /// A provider's report (Runs, Compare, Promote, History), shown until closed.
    pub report: Option<(String, String)>,
    pub focus: Option<gpui::FocusHandle>,
    /// The first key of `I I`.
    pub chord: Option<char>,
}

impl Tab {
    pub fn cell_state(&self, cell: &str) -> Option<kubiq_proto::runbook::StepState> {
        self.snapshot
            .as_ref()
            .and_then(|s| s.states.step(cell))
            .map(|s| s.state)
    }
}

#[derive(Default)]
pub struct Runbooks {
    pub tabs: HashMap<Key, Tab>,
}

impl Global for Runbooks {}

pub fn runbooks(cx: &mut App) -> &mut Runbooks {
    cx.default_global::<Runbooks>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_keeps_the_newest_lines_and_counts_the_rest() {
        let mut ring = Ring::default();
        ring.push(Tail {
            cell: "a".into(),
            attempt: 1,
            lines: (1..=10).map(|n| n.to_string()).collect(),
            skipped: 3,
        });
        ring.push(Tail {
            cell: "a".into(),
            attempt: 1,
            lines: (11..=20).map(|n| n.to_string()).collect(),
            skipped: 0,
        });
        assert_eq!(ring.lines.len(), TAIL_LINES);
        assert_eq!(ring.lines.back().unwrap(), "20");
        assert_eq!(ring.skipped, 3 + 5);
        // A new attempt starts a new ring.
        ring.push(Tail {
            cell: "a".into(),
            attempt: 2,
            lines: vec!["x".into()],
            skipped: 0,
        });
        assert_eq!(ring.lines.len(), 1);
        assert_eq!(ring.skipped, 0);
    }
}
