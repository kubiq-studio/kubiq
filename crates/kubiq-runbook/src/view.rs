//! The records a runbook's state travels in: what the viewer draws and what the agent reads. Plain
//! data; `kubiq-proto`'s runbook family re-exports them, so they are defined once.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::graph::{Command, States};
use crate::lint::Lints;

/// What an agent asked the human for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PendingKind {
    /// `request_run`: run these cells.
    Run,
    /// `request_check`: judge this manual check.
    Check,
}

/// A request waiting for the human (the gate, proposal §6). It runs nothing by itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub id: u64,
    pub kind: PendingKind,
    pub cells: Vec<String>,
    pub why: String,
    /// Who asked: the agent's name.
    pub by: String,
    pub at: String,
    /// The command hash of each cell when asked, so the bar can say a cell changed since.
    pub hashes: BTreeMap<String, String>,
}

/// One field of a provider action's form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FieldSpec {
    pub name: String,
    pub label: String,
    /// A closed set to pick from; empty is free text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// An action a provider offers on a document (Start run, Close run, Runs, …). The viewer draws
/// it as a toolbar button and, when it has fields, a small form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ActionSpec {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<FieldSpec>,
    /// Irreversible (closing a run): drawn in the danger colour and confirmed.
    #[serde(default)]
    pub danger: bool,
}

/// What a provider says about one document. The base's own answer for a document no provider
/// claims is the default: a scratch runbook, runnable, no actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DocStatus {
    /// `master`, `instance`, … — a chip in the toolbar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Why the document is read-only: it runs, records and is written nothing (a closed run).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<String>,
    /// Why the document runs and records nothing while staying editable (a master: edit it, start
    /// a run of it). `None` with `locked` `None` is runnable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ActionSpec>,
    /// Facts for the parameters panel (`master`, `master_rev`, `env = prod`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<(String, String)>,
}

impl DocStatus {
    /// Why nothing may run or be recorded here, if anything says so.
    pub fn no_runs(&self) -> Option<&String> {
        self.locked.as_ref().or(self.idle.as_ref())
    }
}

/// What an action did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ActionOutcome {
    pub message: String,
    /// A markdown report to show (the Runs list, a comparison, a promotion diff).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
    /// A project-relative path to open in the runbook viewer (the new instance).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<String>,
}

/// One attempt, summarised.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptView {
    pub attempt: u64,
    pub cell: String,
    pub run: u32,
    /// `running`, or the outcome's label.
    pub status: String,
    pub passed: bool,
    pub lines: u64,
    pub bytes: u64,
    pub truncated: bool,
    pub at: String,
    pub by: String,
}

/// A runbook's whole state, as the viewer and `read_runbook` get it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Snapshot {
    /// The content hash of the file on disk: `write_runbook`'s base revision.
    pub rev: String,
    pub states: States,
    /// Every cell's current command, by id.
    pub commands: BTreeMap<String, Command>,
    pub lints: Lints,
    pub pending: Vec<Pending>,
    pub status: DocStatus,
    /// The bound parameters, secrets as `•••`.
    pub params: Vec<(String, String)>,
    pub attempts: Vec<AttemptView>,
    /// `$RUNBOOK_ENV` as the next cell will load it, masked.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub env: String,
}

/// A frame of a running cell's live tail: the newest lines only, and how many were passed over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tail {
    pub cell: String,
    pub attempt: u64,
    pub lines: Vec<String>,
    /// Lines this frame did not carry because a newer one came in the same batch.
    pub skipped: u64,
}

/// How much of a log to read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum OutputQuery {
    Head {
        n: u64,
    },
    Tail {
        n: u64,
    },
    /// Lines `from..=to`, 1-based, as the excerpt numbers them.
    Range {
        from: u64,
        to: u64,
    },
    /// Lines matching `pattern` with `context` lines around each.
    Grep {
        pattern: String,
        context: u64,
    },
    /// The retained log from the start, up to the read cap (the full-output view).
    All,
}

/// A slice of one attempt's retained log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputSlice {
    pub cell: String,
    pub attempt: u64,
    /// `n | text` lines, real numbers.
    pub text: String,
    pub total_lines: u64,
    pub total_bytes: u64,
    /// The log itself stopped at its cap.
    pub truncated: bool,
    /// This answer stopped at the per-call cap.
    pub capped: bool,
}

/// One approved cell: the hash of the command the human saw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub cell: String,
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RunOptions {
    /// The human confirmed running a waiting step without its prerequisites.
    pub skip_prereqs: bool,
    /// The human gave the second confirmation `confirm=` asks for.
    pub confirmed: bool,
    /// The request this run answers.
    pub request: Option<u64>,
    /// Set by the worker alone when a policy, not a click, approved the run (`readonly-auto for
    /// <agent>`): recorded as the attempt's actor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    /// The person's defaults (the Cells settings): a cell's timeout when it names none, and the
    /// log cap in MiB. `None` is the runbook's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_timeout_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_cap_mib: Option<u32>,
}

/// What a run request did, per cell.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RunReport {
    pub started: Vec<(String, u64)>,
    pub refused: Vec<(String, String)>,
}
