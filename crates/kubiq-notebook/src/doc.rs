//! The live notebook: cell operations, the notebook-level undo stack, who changed what, and which
//! cells are stale.
//!
//! **Two undo histories, on purpose.** A person's typing inside a cell is that cell editor's own
//! history, so a human [`CellOp::Source`] takes no entry here. Everything else — inserting,
//! deleting, moving, a kind change, clearing outputs, and *every* agent edit including a source
//! replacement — is one entry on [`Notebook::undo`]'s stack, tagged with its [`Origin`].
//!
//! **Staleness is a mark, not an enforcement.** A cell is stale when its source changed since its
//! outputs were produced, or when it ran before a cell above it was run again. Only runs this
//! document saw count: a notebook opened from disk has no stale cells until something runs.

use serde::{Deserialize, Serialize};

use crate::nbformat::{Cell, CellKind, NotebookFile, Output, StoredOutput, mint_id};

pub use kubiq_cells::Origin;
use kubiq_cells::{CellDocument, RunMarks};

/// One edit to the notebook. Cells are named by id, never by index, so a concurrent edit cannot
/// shift the target; only `Insert` and `Move` take a position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CellOp {
    /// Insert a cell at `index` (clamped). `id: None` mints one.
    Insert {
        index: usize,
        id: Option<String>,
        kind: CellKind,
        source: String,
    },
    Delete {
        id: String,
    },
    Move {
        id: String,
        to: usize,
    },
    Source {
        id: String,
        source: String,
    },
    Kind {
        id: String,
        kind: CellKind,
    },
    /// Clear one cell's outputs and count, or every cell's (`None`).
    ClearOutputs {
        id: Option<String>,
    },
}

/// The inverse of an applied op.
#[derive(Debug, Clone)]
enum Inverse {
    Remove(String),
    Restore { index: usize, cell: Box<Cell> },
    MoveTo { id: String, to: usize },
    Replace { cell: Box<Cell> },
}

struct Entry {
    origin: Origin,
    inverse: Vec<Inverse>,
}

/// What an apply changed, for the event every view receives.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Change {
    /// Ids of cells whose content changed (and must be re-sent).
    pub cells: Vec<String>,
    /// The order changed (insert, delete, move).
    pub order: bool,
}

impl Change {
    fn touch(&mut self, id: &str) {
        if !self.cells.iter().any(|c| c == id) {
            self.cells.push(id.to_string());
        }
    }
}

const UNDO_CAP: usize = 200;

pub struct Notebook {
    pub file: NotebookFile,
    /// Bumped by every change.
    pub rev: u64,
    /// The `rev` that was last read from or written to disk.
    pub saved_rev: u64,
    /// Edited since it was read: minted ids are written on save.
    edited: bool,
    undo: Vec<Entry>,
    runs: RunMarks,
    seed: u64,
    minted: u64,
}

impl Notebook {
    /// Parse a file. `seed` drives id minting (the host draws it at random).
    pub fn parse(text: &str, seed: u64) -> Result<Notebook, crate::nbformat::FormatError> {
        let mut n = 0;
        let file = NotebookFile::parse(text, &mut || {
            n += 1;
            mint_id(seed, n)
        })?;
        Ok(Notebook::from_file(file, seed, n))
    }

    pub fn new_empty(seed: u64) -> Notebook {
        Notebook::from_file(NotebookFile::empty(mint_id(seed, 1)), seed, 1)
    }

    fn from_file(file: NotebookFile, seed: u64, minted: u64) -> Notebook {
        Notebook {
            file,
            rev: 0,
            saved_rev: 0,
            edited: false,
            undo: Vec::new(),
            runs: RunMarks::default(),
            seed,
            minted,
        }
    }

    pub fn dirty(&self) -> bool {
        self.rev != self.saved_rev
    }

    pub fn serialize(&self) -> String {
        self.file.serialize(self.edited)
    }

    /// Of the `(cell, source)` pairs a person was shown, the cells whose source is no longer that
    /// (or that are gone) — what an approval must not run.
    pub fn changed_since(&self, shown: &[(String, String)]) -> Vec<String> {
        shown
            .iter()
            .filter(|(id, source)| self.cell(id).is_none_or(|c| &c.source != source))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// The file's text without outputs or execution counts — the document itself keeps them.
    pub fn serialize_without_outputs(&self) -> String {
        let mut file = self.file.clone();
        for cell in &mut file.cells {
            cell.outputs.clear();
            cell.execution_count = None;
        }
        file.serialize(true)
    }

    pub fn mark_saved(&mut self) {
        self.saved_rev = self.rev;
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.file.cells.iter().position(|c| c.id == id)
    }

    pub fn cell(&self, id: &str) -> Option<&Cell> {
        self.file.cells.iter().find(|c| c.id == id)
    }

    pub fn cell_mut(&mut self, id: &str) -> Option<&mut Cell> {
        self.file.cells.iter_mut().find(|c| c.id == id)
    }

    pub fn order(&self) -> Vec<String> {
        self.file.cells.iter().map(|c| c.id.clone()).collect()
    }

    fn mint(&mut self) -> String {
        loop {
            self.minted += 1;
            let id = mint_id(self.seed, self.minted);
            if self.index_of(&id).is_none() {
                return id;
            }
        }
    }

    /// Apply `ops` as one step. All or nothing: an op naming an unknown cell refuses the batch.
    pub fn apply(&mut self, ops: Vec<CellOp>, origin: Origin) -> Result<Change, String> {
        for op in &ops {
            let named = match op {
                CellOp::Delete { id }
                | CellOp::Move { id, .. }
                | CellOp::Source { id, .. }
                | CellOp::Kind { id, .. }
                | CellOp::ClearOutputs { id: Some(id) } => Some(id),
                _ => None,
            };
            if let Some(id) = named
                && self.index_of(id).is_none()
                && !ops.iter().any(|o| matches!(o, CellOp::Insert { id: Some(new), .. } if new == id))
            {
                return Err(format!("no cell with id {id}"));
            }
        }
        let mut change = Change::default();
        let mut inverse = Vec::new();
        let mut undoable = false;
        for op in ops {
            match op {
                CellOp::Insert {
                    index,
                    id,
                    kind,
                    source,
                } => {
                    let id = match id {
                        Some(id) if self.index_of(&id).is_none() => id,
                        _ => self.mint(),
                    };
                    let at = index.min(self.file.cells.len());
                    self.file.cells.insert(at, Cell::new(id.clone(), kind, source));
                    inverse.push(Inverse::Remove(id.clone()));
                    change.touch(&id);
                    change.order = true;
                    undoable = true;
                }
                CellOp::Delete { id } => {
                    if let Some(at) = self.index_of(&id) {
                        let cell = self.file.cells.remove(at);
                        inverse.push(Inverse::Restore {
                            index: at,
                            cell: Box::new(cell),
                        });
                        change.order = true;
                        undoable = true;
                    }
                }
                CellOp::Move { id, to } => {
                    if let Some(at) = self.index_of(&id) {
                        let cell = self.file.cells.remove(at);
                        let to = to.min(self.file.cells.len());
                        self.file.cells.insert(to, cell);
                        inverse.push(Inverse::MoveTo { id, to: at });
                        change.order = true;
                        undoable = true;
                    }
                }
                CellOp::Source { id, source } => {
                    if let Some(cell) = self.cell_mut(&id)
                        && cell.source != source
                    {
                        let before = cell.clone();
                        cell.source = source;
                        inverse.push(Inverse::Replace {
                            cell: Box::new(before),
                        });
                        change.touch(&id);
                        undoable |= origin != Origin::Human;
                    }
                }
                CellOp::Kind { id, kind } => {
                    if let Some(cell) = self.cell_mut(&id)
                        && cell.kind != kind
                    {
                        let before = cell.clone();
                        cell.kind = kind;
                        if kind != CellKind::Code {
                            cell.outputs.clear();
                            cell.execution_count = None;
                        }
                        inverse.push(Inverse::Replace {
                            cell: Box::new(before),
                        });
                        change.touch(&id);
                        undoable = true;
                    }
                }
                CellOp::ClearOutputs { id } => {
                    let ids: Vec<String> = match id {
                        Some(id) => vec![id],
                        None => self.order(),
                    };
                    for id in ids {
                        if let Some(cell) = self.cell_mut(&id)
                            && (!cell.outputs.is_empty() || cell.execution_count.is_some())
                        {
                            let before = cell.clone();
                            cell.outputs.clear();
                            cell.execution_count = None;
                            inverse.push(Inverse::Replace {
                                cell: Box::new(before),
                            });
                            self.runs.forget(&id);
                            change.touch(&id);
                            undoable = true;
                        }
                    }
                }
            }
        }
        if !change.cells.is_empty() || change.order {
            self.rev += 1;
            self.edited = true;
        }
        if undoable && !inverse.is_empty() {
            self.undo.push(Entry { origin, inverse });
            if self.undo.len() > UNDO_CAP {
                self.undo.remove(0);
            }
        }
        Ok(change)
    }

    /// Undo the latest notebook-level step. Returns what changed and whose step it was, or `None`
    /// when there is nothing to undo.
    pub fn undo(&mut self) -> Option<(Change, Origin)> {
        let entry = self.undo.pop()?;
        let mut change = Change::default();
        for inverse in entry.inverse.into_iter().rev() {
            match inverse {
                Inverse::Remove(id) => {
                    if let Some(at) = self.index_of(&id) {
                        self.file.cells.remove(at);
                        change.order = true;
                    }
                }
                Inverse::Restore { index, cell } => {
                    change.touch(&cell.id);
                    let at = index.min(self.file.cells.len());
                    self.file.cells.insert(at, *cell);
                    change.order = true;
                }
                Inverse::MoveTo { id, to } => {
                    if let Some(at) = self.index_of(&id) {
                        let cell = self.file.cells.remove(at);
                        self.file.cells.insert(to.min(self.file.cells.len()), cell);
                        change.order = true;
                    }
                }
                Inverse::Replace { cell } => {
                    change.touch(&cell.id);
                    if let Some(slot) = self.cell_mut(&cell.id) {
                        *slot = *cell;
                    }
                }
            }
        }
        self.rev += 1;
        Some((change, entry.origin))
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    // ── runs ────────────────────────────────────────────────────────────────────────────────

    /// A run of `id` started with `source`: its old outputs go, and it is marked as run now.
    pub fn run_started(&mut self, id: &str, source: &str) -> bool {
        let Some(cell) = self.cell_mut(id) else {
            return false;
        };
        cell.outputs.clear();
        self.runs.started(id, source);
        self.rev += 1;
        true
    }

    /// An output for the run of `id` arrived.
    pub fn push_output(&mut self, id: &str, output: StoredOutput) {
        if let Some(cell) = self.cell_mut(id) {
            crate::nbformat::push_output(&mut cell.outputs, output);
            self.rev += 1;
        }
    }

    /// `update_display_data`: replace every output carrying `display_id`. Returns the cells touched.
    pub fn update_display(&mut self, display_id: &str, output: StoredOutput) -> Vec<String> {
        let mut touched = Vec::new();
        for cell in &mut self.file.cells {
            for slot in &mut cell.outputs {
                if slot.display_id.as_deref() == Some(display_id) {
                    // An update keeps the slot's kind (`execute_result` stays one).
                    let mut raw = output.raw.clone();
                    if let Some(kind) = slot.raw.get("output_type").cloned() {
                        raw.set("output_type", kind);
                    }
                    if let Some(count) = slot.raw.get("execution_count").cloned() {
                        raw.set("execution_count", count);
                    }
                    slot.raw = raw;
                    if !touched.contains(&cell.id) {
                        touched.push(cell.id.clone());
                    }
                }
            }
        }
        if !touched.is_empty() {
            self.rev += 1;
        }
        touched
    }

    pub fn clear_outputs(&mut self, id: &str) {
        if let Some(cell) = self.cell_mut(id) {
            cell.outputs.clear();
            self.rev += 1;
        }
    }

    pub fn set_count(&mut self, id: &str, count: Option<i64>) {
        if let Some(cell) = self.cell_mut(id) {
            cell.execution_count = count;
            self.rev += 1;
        }
    }

    /// Whether `id` is stale: its source moved on since it ran, or a cell above ran after it.
    pub fn stale(&self, id: &str) -> bool {
        let Some(at) = self.index_of(id) else {
            return false;
        };
        let cells: Vec<(&str, &str)> = self
            .file
            .cells
            .iter()
            .map(|c| (c.id.as_str(), c.source.as_str()))
            .collect();
        self.runs.stale(&cells, at)
    }

    /// The file changed on disk: take the new one, matching cells by id. A run's marks survive
    /// for a cell whose id is still there. Returns everything as changed.
    pub fn reload(&mut self, file: NotebookFile) -> Change {
        self.runs.retain(|id| file.cells.iter().any(|c| c.id == id));
        self.file = file;
        self.rev += 1;
        self.saved_rev = self.rev;
        self.edited = false;
        Change {
            cells: self.order(),
            order: true,
        }
    }

    /// The view of one cell.
    pub fn view(&self, id: &str) -> Option<CellView> {
        let cell = self.cell(id)?;
        Some(CellView {
            id: cell.id.clone(),
            kind: cell.kind,
            source: cell.source.clone(),
            execution_count: cell.execution_count,
            outputs: cell.outputs.iter().map(StoredOutput::view).collect(),
            stale: self.stale(id),
        })
    }

    pub fn views(&self) -> Vec<CellView> {
        self.file
            .cells
            .iter()
            .filter_map(|c| self.view(&c.id))
            .collect()
    }
}

/// One cell, as the interface and an agent see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellView {
    pub id: String,
    pub kind: CellKind,
    pub source: String,
    pub execution_count: Option<i64>,
    pub outputs: Vec<Output>,
    pub stale: bool,
}

impl CellDocument for Notebook {
    fn cell_ids(&self) -> Vec<String> {
        self.order()
    }

    fn source(&self, id: &str) -> Option<String> {
        self.cell(id).map(|c| c.source.clone())
    }

    fn kind_label(&self, id: &str) -> Option<&'static str> {
        self.cell(id).map(|c| c.kind.as_str())
    }
}
