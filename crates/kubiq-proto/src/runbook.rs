//! The runbook family's records: what a window asks of a `*.runbook.md` the host runs, and what the
//! host tells every window about it.
//!
//! The model — a step's derived state, a cell's command and hash, a pending request, an attempt,
//! a provider's actions — is defined once, in `kubiq-runbook`, and re-exported here (the database
//! family's precedent). What is below is the contract's own: the requests and the events.
//!
//! **A runbook is addressed by its project and its project-relative path.** The document is the
//! project's file, which the editor tab reads and saves as any other; the host owns its evidence,
//! its runs and its gate. The window never starts a process and never reads a log itself.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use kubiq_runbook::graph::{CheckState, CheckView, Command, StepState, StepView};
pub use kubiq_runbook::lint::{Lint, Lints, Severity};
pub use kubiq_runbook::view::{
    ActionOutcome, ActionSpec, Approval, AttemptView, DocStatus, FieldSpec, OutputQuery,
    OutputSlice, Pending, PendingKind, RunOptions, RunReport, Snapshot, Tail,
};

/// What a window asks of one runbook.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RunbookRequest {
    /// Start following it: the host recovers interrupted attempts, projects the evidence into the
    /// file, and answers [`RunbookEvent::State`] to every window.
    Open,
    /// Run approved cells. Answered with [`RunbookEvent::Ran`] to the asker; the attempts then
    /// report to everyone.
    Run {
        approvals: Vec<Approval>,
        options: RunOptions,
    },
    /// Stop a running cell: its process group is ended.
    Stop { cell: String },
    /// Do not run this step; its dependents proceed. Recorded with the reason.
    Skip { cell: String, reason: String },
    /// Count this failed step as done. Recorded with the reason.
    Override { cell: String, reason: String },
    /// A human's verdict on a manual check.
    Verdict {
        cell: String,
        pass: bool,
        note: String,
    },
    /// Set an agent's request aside without running it.
    Dismiss { request: u64 },
    /// A human accepted this readonly cell's mark for its current text, so `gate: readonly-auto`
    /// may run it when an agent asks.
    /// `hash` is the command hash the person was shown; a different current one refuses it.
    AcceptReadonly {
        cell: String,
        reason: String,
        hash: String,
    },
    /// Bind an agent to this runbook (or unbind, `None`): the bound agent is queued the
    /// runbook's events — a run finished, a verdict, a skip, a request answered, an edit. Any
    /// agent may still reach the runbook through `kubiq-runbook`.
    Bind { agent: Option<crate::work::AgentId> },
    /// Read part of one attempt's retained log. Answered with [`RunbookEvent::Output`].
    ReadOutput {
        cell: String,
        attempt: u64,
        query: OutputQuery,
    },
    /// Run a provider's action. Answered with [`RunbookEvent::ActionDone`].
    Action {
        action: String,
        fields: BTreeMap<String, String>,
    },
}

/// What the host says about one runbook.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RunbookEvent {
    /// Its whole state, and the agent bound to it. To every window: a window not showing it
    /// ignores it.
    State {
        snapshot: Snapshot,
        bound: Option<crate::work::AgentId>,
    },
    /// A frame of a running cell's live tail. To every window.
    Tail(Tail),
    /// An attempt ended. To every window.
    Finished {
        cell: String,
        attempt: u64,
        label: String,
    },
    /// What a [`RunbookRequest::Run`] started and refused. To the asker.
    Ran(RunReport),
    /// A slice of a log, or why there is none. To the asker.
    Output {
        cell: String,
        attempt: u64,
        slice: Option<OutputSlice>,
        error: Option<String>,
    },
    /// What an action did. To the asker.
    ActionDone {
        action: String,
        outcome: Option<ActionOutcome>,
        error: Option<String>,
    },
    /// A request was refused: the sentence to show. To the asker.
    Refused { reason: String },
}
