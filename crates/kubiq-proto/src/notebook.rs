//! The notebook family's records: what the interface asks of a `.ipynb` the host holds, and what the
//! host tells every window about it.
//!
//! The model — a cell, an output, a cell operation, who made a change, a kernel candidate — is
//! defined once, in `kubiq-notebook`, and re-exported here (the database family's precedent). What
//! is below is the contract's own: the requests, the events, a kernel's state, the agent-run policy.
//!
//! **A notebook is addressed by its project and its project-relative path.** The host keeps one
//! document and at most one kernel per path, so two tabs, two windows and an agent all work on the
//! same cells; the interface never holds the file or the process, only cell views.

use serde::{Deserialize, Serialize};

use crate::ids::NotebookRunId;
use crate::work::AgentId;

pub use kubiq_cells::{DocEvent, RunState, RunStatus};

pub use kubiq_notebook::env::{Candidate, Group, InterruptMode, KernelSpec};
pub use kubiq_notebook::{CellKind, CellOp, CellView, Origin, Output};

/// Which kernel to start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KernelChoice {
    /// A [`Candidate::id`] from the latest [`NotebookEvent::Kernels`].
    Candidate(String),
    /// An interpreter the user typed or picked by path; its kernelspec is synthesised.
    Interpreter(String),
}

/// What an agent's `run_cells` may do on this notebook. Kept per notebook in the project's `local/`
/// data; the harness's own permission mode never implies `Allow`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunPolicy {
    /// A run waits for a person's yes in the notebook's header.
    #[default]
    Ask,
    Allow,
    /// Read and edit only.
    Deny,
}

impl RunPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            RunPolicy::Ask => "ask",
            RunPolicy::Allow => "allow",
            RunPolicy::Deny => "deny",
        }
    }
}

/// Where a kernel stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KernelState {
    /// No kernel for this notebook.
    Off,
    Starting,
    Idle,
    Busy,
    /// It exited on its own or never came up: the exit status and the last lines it wrote.
    Dead { code: Option<i32>, tail: String },
}

/// An explicit environment action — only ever a click (nothing is installed without one).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnvAction {
    /// `uv add --dev ipykernel` in a uv project, else `<venv python> -m pip install ipykernel`.
    /// Never against an interpreter outside the project.
    InstallIpykernel { python: Option<String> },
    /// `uv venv` at the project root, then the install above.
    CreateVenv,
}

/// UI → host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NotebookRequest {
    /// Read the file (no size cap) and subscribe. Answered with [`NotebookEvent::Snapshot`]; a
    /// path that is not there starts an empty notebook, written on the first save.
    /// `default_policy` is the agent policy a notebook with none of its own starts with (the
    /// window's *Cells* preference).
    Open { default_policy: RunPolicy },
    /// This window no longer shows the notebook. The last view leaving shuts its kernel down.
    Close,
    /// Cell edits, as one step. `rev` is the revision the window drew them against.
    Edit { rev: u64, ops: Vec<CellOp> },
    /// Undo the latest notebook-level step (structural edits and agent edits; typing inside a cell
    /// is the cell editor's own history).
    Undo,
    /// Write the file; `strip_outputs` writes it without outputs and counts (the open document
    /// keeps them).
    Save { strip_outputs: bool },
    /// Find the kernels this notebook could run on. Answered with [`NotebookEvent::Kernels`].
    /// `run_project` is a person's consent to execute the project's own interpreters (and `uv`)
    /// to probe them; without it they are listed by path only. The consent lasts for the host's
    /// run, for the project.
    ResolveKernels { run_project: bool },
    StartKernel { choice: KernelChoice },
    /// Run one cell's current source.
    Execute { run: NotebookRunId, cell: String },
    Interrupt,
    /// Restart the kernel; `run_all` then runs every code cell top to bottom.
    Restart { run_all: bool },
    Shutdown,
    /// The answer to an [`NotebookEvent::InputRequest`].
    InputReply { run: NotebookRunId, value: String },
    SetPolicy { policy: RunPolicy },
    /// A person's answer to an agent's run waiting under `Ask`. `session` promotes the notebook
    /// to `Allow` until its kernel restarts.
    DecideRun { run: NotebookRunId, allow: bool, session: bool },
    Env { action: EnvAction },
    /// Bind the agent that receives this notebook's events (a run it asked for finished, a run
    /// approved, a question on a cell) through the doc queue; `None` unbinds. Any agent may still
    /// use `kubiq-notebook` — binding only decides who is told.
    Bind { agent: Option<AgentId> },
    /// A person's question or remark about a cell, for the bound agent.
    Ask { cell: String, text: String },
}

/// Host → UI. Every event but the snapshot and the kernel list goes to every window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NotebookEvent {
    /// The whole notebook, to the window that opened it.
    Snapshot {
        rev: u64,
        cells: Vec<CellView>,
        dirty: bool,
        can_undo: bool,
        kernel: KernelState,
        kernel_label: Option<String>,
        policy: RunPolicy,
        bound: Option<AgentId>,
    },
    /// Something changed. `order` is every cell id in order; `cells` are the ones whose content
    /// changed; `stale` names every stale cell.
    Changed {
        rev: u64,
        order: Vec<String>,
        cells: Vec<CellView>,
        stale: Vec<String>,
        origin: Origin,
        dirty: bool,
        can_undo: bool,
    },
    Saved { rev: u64 },
    /// The file changed on disk while the document held unsaved edits. Saving overwrites it.
    Conflict,
    /// A request could not be carried out: `<what failed>: <cause>`.
    Failed { error: String },
    Kernels { candidates: Vec<Candidate> },
    Kernel { state: KernelState, label: Option<String> },
    /// A run was queued or started on `cell`.
    Running { run: NotebookRunId, cell: String, origin: Origin },
    Done {
        run: NotebookRunId,
        cell: String,
        count: Option<i64>,
        status: RunStatus,
    },
    InputRequest {
        run: NotebookRunId,
        cell: String,
        prompt: String,
        password: bool,
    },
    /// An agent asked to run cells under `Ask`: the cells and their source, for the header's
    /// question. Answered with [`NotebookRequest::DecideRun`].
    RunRequested {
        run: NotebookRunId,
        agent: String,
        cells: Vec<(String, String)>,
    },
    /// The run question was answered (here or in another window).
    RunDecided { run: NotebookRunId },
    Policy { policy: RunPolicy },
    Bound { agent: Option<AgentId> },
    /// An [`EnvAction`] finished: whether it worked and what it printed.
    EnvDone { ok: bool, log: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(v: T) {
        let bytes = rmp_serde::to_vec_named(&v).unwrap();
        let back: T = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn the_family_round_trips() {
        let run = NotebookRunId::generate();
        round(NotebookRequest::Edit {
            rev: 3,
            ops: vec![CellOp::Insert {
                index: 0,
                id: None,
                kind: CellKind::Markdown,
                source: "# hi".into(),
            }],
        });
        round(NotebookRequest::StartKernel {
            choice: KernelChoice::Interpreter("/p/.venv/bin/python".into()),
        });
        round(NotebookEvent::Changed {
            rev: 4,
            order: vec!["a".into()],
            cells: vec![CellView {
                id: "a".into(),
                kind: CellKind::Code,
                source: "1".into(),
                execution_count: Some(1),
                outputs: vec![
                    Output::Stream {
                        name: "stdout".into(),
                        text: "x\n".into(),
                    },
                    Output::Data {
                        execute_result: true,
                        execution_count: Some(1),
                        bundle: vec![("text/plain".into(), "1".into())],
                    },
                ],
                stale: false,
            }],
            stale: vec![],
            origin: Origin::Agent("ag".into()),
            dirty: true,
            can_undo: true,
        });
        round(NotebookEvent::Kernel {
            state: KernelState::Dead {
                code: Some(1),
                tail: "boom".into(),
            },
            label: None,
        });
        round(NotebookEvent::RunRequested {
            run,
            agent: "a".into(),
            cells: vec![("c".into(), "print(1)".into())],
        });
    }
}
