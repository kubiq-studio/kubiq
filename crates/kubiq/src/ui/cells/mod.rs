//! The interface's half of the shared cell layer (`kubiq_cells`): the pieces every cell document's
//! viewer draws with — the notebook's (`ui::notebook`) and the runbook's.
//!
//! - [`zoom`] — the read-only zoom dialog: a text body in a read-only editor (selection, copy,
//!   search, line numbers) or an image fitted to the window.
//! - [`output`] — a selectable, copyable text block with its zoom button; images decoded once.
//! - [`agent`] — the bound-agent control: who hears this document's events.
//! - [`keys`] — the one key context (`CellsView`) and its actions, Jupyter's keymap.
//! - [`run_chip`] — a run state's colour and label.

pub mod agent;
pub mod keys;
pub mod output;
pub mod settings;
pub mod zoom;

use gpui::Rgba;
use kubiq_cells::RunState;

use crate::theme;

/// The colour and the word a row shows for a run state. Status by colour, never by wording alone.
pub fn run_chip(state: RunState) -> (Rgba, &'static str) {
    match state {
        RunState::Idle => (theme::text_faint(), "idle"),
        RunState::Queued => (theme::info(), "queued"),
        RunState::Running => (theme::info(), "running"),
        RunState::Waiting => (theme::warning(), "waiting for you"),
        RunState::Ok => (theme::success(), "ok"),
        RunState::Error => (theme::danger(), "error"),
        RunState::Aborted => (theme::warning(), "stopped"),
        RunState::Stale => (theme::warning(), "stale"),
    }
}
