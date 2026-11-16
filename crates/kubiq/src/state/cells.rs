//! The cell documents' preferences — the *Cells* section of the application settings
//! (`ui::cells::settings`), kept in the interface's settings blob (`UiSettings::cells`) like
//! Archify's *Diagrams*. What the host needs of them travels with the request that uses it (a
//! notebook's default policy with `Open`, stripping outputs with `Save`).

use kubiq_proto::notebook::RunPolicy;
use serde::{Deserialize, Serialize};

/// Every field has a default, so a blob written before a field existed still reads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CellsSettings {
    /// The agent policy a notebook starts with until it is given its own.
    pub notebook_policy: RunPolicy,
    /// Write notebooks without their outputs and execution counts (git-friendly).
    pub strip_outputs: bool,
    /// Lines of a text output drawn in a cell; the rest is behind the zoom.
    pub output_lines: usize,
    /// The runbook's rows (default gate, log cap, default shell and timeout) — filled by the
    /// runbook's own section rows; opaque here.
    pub runbook: RunbookCells,
}

/// The runbook's preferences. The runbook owns their meaning.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunbookCells {
    /// `every-cell` (the default) or `readonly-auto`.
    pub gate: Option<String>,
    /// A run's log cap, in MiB; `None` is the runbook's own default.
    pub log_cap_mib: Option<u32>,
    /// The shell a bare fence runs under; `None` is the host's default.
    pub shell: Option<String>,
    /// A cell's timeout, in seconds; `None` is none.
    pub timeout_secs: Option<u64>,
}

impl Default for CellsSettings {
    fn default() -> Self {
        CellsSettings {
            notebook_policy: RunPolicy::Ask,
            strip_outputs: false,
            output_lines: 400,
            runbook: RunbookCells::default(),
        }
    }
}

/// The output caps a user can pick.
pub const OUTPUT_LINES: [usize; 4] = [100, 400, 2_000, 10_000];
