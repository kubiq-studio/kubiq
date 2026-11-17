//! The runbook engine: a markdown document whose fenced cells a human runs, one decision at a time,
//! while an agent writes and reads it.
//!
//! A leaf like `kubiq-md` and `kubiq-archify`: no GPUI and no Kubiq crate. Everything is pure
//! except [`service`] (feature `service`), the worker the host runs: files, processes, logs.
//!
//! - [`format`] — frontmatter and fences with their `{#id key=value}` attributes, as byte spans
//! - [`model`] — the cells and the frontmatter settings
//! - [`params`] — `{{name}}` substitution and the command hash an approval binds
//! - [`lint`] — validation, and which cells an error keeps from running
//! - [`graph`] — the step graph and every derived state (never stored)
//! - [`evidence`] — the append-only records the states are derived from
//! - [`project`] — writing the evidence into the document as `output` blocks
//! - [`excerpt`] — the bounded head-and-tail excerpt
//! - [`redact`] — masking known secrets across chunk boundaries
//! - [`gate`] — the shared run gate for an agent's request (`every-cell`, `readonly-auto`)
//! - [`shell`] — the interpreter and invocation of a cell by the target's OS
//! - [`view`] — the records the viewer and the agent read

pub mod evidence;
pub mod excerpt;
pub mod format;
pub mod gate;
pub mod graph;
pub mod lint;
pub mod model;
pub mod params;
pub mod project;
pub mod redact;
#[cfg(feature = "service")]
pub mod service;
pub mod shell;
pub mod view;

/// The suffix a runbook's file name ends in.
pub const SUFFIX: &str = ".runbook.md";

impl kubiq_cells::CellDocument for model::Runbook {
    fn cell_ids(&self) -> Vec<String> {
        self.cells.iter().map(|c| c.id.clone()).collect()
    }
    fn source(&self, id: &str) -> Option<String> {
        self.cell(id).map(|c| c.body.clone())
    }
    fn kind_label(&self, id: &str) -> Option<&'static str> {
        self.cell(id).map(|c| match c.kind {
            model::CellKind::Step => "step",
            model::CellKind::Check => "check",
            model::CellKind::Manual => "manual check",
        })
    }
}

/// A new runbook's text: what *New ▸ Runbook* writes. The one cell is a bare fence, so it runs
/// under the creating host's default interpreter (`sh` on Unix, PowerShell on Windows), and its
/// command means the same in both.
pub fn template() -> String {
    "---\nrunbook: untitled\ngate: every-cell\n---\n\n# Untitled runbook\n\nWhat this procedure is for, what must hold before it starts, and how to roll it back.\n\n```console {#first}\necho hello\n```\n".to_string()
}

/// Whether `path` names a runbook.
pub fn is_runbook(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(SUFFIX)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_template_is_a_clean_runbook_with_one_ready_cell() {
        let text = super::template();
        let book = super::model::Runbook::parse(&text);
        assert_eq!(book.cells.len(), 1);
        assert_eq!(book.cells[0].id, "first");
        assert!(super::lint::lint(&book, None).lints.is_empty());
    }
}
