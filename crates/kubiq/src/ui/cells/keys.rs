//! The cell documents' keymap: one key context, Jupyter's keys, shared by every kind.
//!
//! Two modes, as in JupyterLab and VS Code. **Command mode** — the cell list has focus, no cell
//! editor does — and letters act on cells. **Edit mode** — a cell's editor has focus — and only the
//! run keys and Escape are the list's; everything else types. The list element carries
//! [`CONTEXT`] with `mode=command` or `mode=edit`, so a letter binding names `mode == command` and
//! can never reach a key an editor is typing.
//!
//! Each kind answers the actions it has with `.on_action` on its own list element: the runbook
//! maps the run keys to its gate, `0 0` is the notebook's alone.

use gpui::{KeyBinding, actions};

/// The key context a cell list carries.
pub const CONTEXT: &str = "CellsView";

actions!(
    cells,
    [
        /// Shift-Enter: run the cell, select the next (adding one at the end).
        RunAdvance,
        /// Ctrl/Cmd-Enter: run the cell, stay.
        RunInPlace,
        /// Alt-Enter: run the cell, insert one below.
        RunInsert,
        /// Escape: command mode.
        CommandMode,
        /// Enter: edit the selected cell.
        EditMode,
        InsertAbove,
        InsertBelow,
        /// The first `D` of `D D`.
        DeleteChord,
        ToMarkdown,
        ToCode,
        ToRaw,
        SelectNext,
        SelectPrev,
        CopyCell,
        CutCell,
        PasteCell,
        /// Undo the last notebook-level step.
        UndoCells,
        /// `I I`.
        InterruptChord,
        /// `0 0`.
        RestartChord,
        ToggleOutput,
        /// F: open the selected cell's output (or its source) in the zoom dialog.
        ZoomCell,
        SaveCells,
    ]
);

pub fn key_bindings() -> Vec<KeyBinding> {
    let cmd = "CellsView && mode == command";
    let any = Some(CONTEXT);
    // The run keys and Escape win over the editor's own: registered after `gpui_component::init`,
    // at the editor's depth (`CellsView > Input`).
    let edit = Some("CellsView > Input");
    let mut bindings = vec![
        KeyBinding::new("shift-enter", RunAdvance, any),
        KeyBinding::new("shift-enter", RunAdvance, edit),
        KeyBinding::new("secondary-enter", RunInPlace, any),
        KeyBinding::new("secondary-enter", RunInPlace, edit),
        KeyBinding::new("alt-enter", RunInsert, any),
        KeyBinding::new("alt-enter", RunInsert, edit),
        KeyBinding::new("escape", CommandMode, edit),
        KeyBinding::new("secondary-s", SaveCells, any),
    ];
    let c = Some(cmd);
    bindings.extend([
        KeyBinding::new("enter", EditMode, c),
        KeyBinding::new("a", InsertAbove, c),
        KeyBinding::new("b", InsertBelow, c),
        KeyBinding::new("d", DeleteChord, c),
        KeyBinding::new("m", ToMarkdown, c),
        KeyBinding::new("y", ToCode, c),
        KeyBinding::new("r", ToRaw, c),
        KeyBinding::new("j", SelectNext, c),
        KeyBinding::new("down", SelectNext, c),
        KeyBinding::new("k", SelectPrev, c),
        KeyBinding::new("up", SelectPrev, c),
        KeyBinding::new("c", CopyCell, c),
        KeyBinding::new("x", CutCell, c),
        KeyBinding::new("v", PasteCell, c),
        KeyBinding::new("z", UndoCells, c),
        KeyBinding::new("i", InterruptChord, c),
        KeyBinding::new("0", RestartChord, c),
        KeyBinding::new("o", ToggleOutput, c),
        KeyBinding::new("f", ZoomCell, c),
    ]);
    bindings
}
