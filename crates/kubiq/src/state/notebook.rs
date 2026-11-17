//! The notebook viewer's interface state: one `Global`, keyed by (project, path), never a field on
//! `AppState`.
//!
//! The document is the host's (`kubiq-host/src/notebook/`): this is a mirror of its cell views,
//! fed by `NotebookEvent`s, plus what only the window has — each code cell's editor, the cell in
//! focus, the open stdin prompt, the latest kernel list. A tab's own buffer is never read: the
//! host reads the file with no size cap, so a notebook full of plots opens whatever its size.

use std::collections::HashMap;

use gpui::{App, Entity, Global, Subscription};
use gpui_component::input::{EditorState, InputState};
use kubiq_cells::{RunState, RunStatus};
use kubiq_proto::ids::{NotebookRunId, ProjectId};
use kubiq_proto::notebook::{Candidate, CellView, KernelState, NotebookEvent, RunPolicy};
use kubiq_proto::work::AgentId;

/// Command mode (keys act on cells) or edit mode (keys type into the focused cell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Command,
    Edit,
}

/// One notebook as this window shows it.
pub struct Tab {
    /// `Open` has been sent.
    pub opened: bool,
    /// The snapshot has arrived.
    pub loaded: bool,
    pub rev: u64,
    pub cells: Vec<CellView>,
    pub dirty: bool,
    pub can_undo: bool,
    pub kernel: KernelState,
    pub kernel_label: Option<String>,
    pub candidates: Vec<Candidate>,
    pub policy: RunPolicy,
    pub bound: Option<AgentId>,
    /// Per cell: the latest run's state and its origin's label (`agent:<id>` for an agent's).
    pub runs: HashMap<String, (RunState, Option<String>)>,
    pub run_ids: HashMap<NotebookRunId, String>,
    /// Per code / raw cell: its editor and the subscription feeding edits to the host.
    pub editors: HashMap<String, (Entity<EditorState>, Subscription)>,
    /// A source the host sent for a cell whose editor must be updated when a `Window` is at hand.
    pub incoming: HashMap<String, String>,
    pub selected: usize,
    pub mode: Mode,
    /// Collapsed outputs, by cell.
    pub collapsed: std::collections::HashSet<String>,
    /// The open stdin prompt: run, cell, prompt text, whether it is a password.
    pub prompt: Option<(NotebookRunId, String, String, bool)>,
    /// The prompt's field, built at the first draw with a `Window`.
    pub prompt_field: Option<Entity<InputState>>,
    /// Agent runs waiting for a yes: run id, agent, (cell, source).
    pub questions: Vec<(NotebookRunId, String, Vec<(String, String)>)>,
    /// The last error or install log, shown in the header's banner.
    pub notice: Option<String>,
    pub conflict: bool,
    /// The cell whose "ask the agent" field is open, and its field.
    pub ask: Option<(String, Entity<InputState>)>,
    /// A cell cut or copied, for paste: kind and source.
    pub clipboard: Option<(kubiq_proto::notebook::CellKind, String)>,
    /// The first key of a two-key chord (`D D`, `I I`, `0 0`).
    pub chord: Option<char>,
    /// The kernel list is open in the header.
    pub picker: bool,
    /// The bound-agent list is open in the header.
    pub agents_open: bool,
    /// Markdown cells shown as source (edited) rather than rendered.
    pub editing_md: std::collections::HashSet<String>,
    /// The list's focus, for command mode's keys.
    pub focus: Option<gpui::FocusHandle>,
}

impl Default for Tab {
    fn default() -> Self {
        Tab {
            opened: false,
            loaded: false,
            rev: 0,
            cells: Vec::new(),
            dirty: false,
            can_undo: false,
            kernel: KernelState::Off,
            kernel_label: None,
            candidates: Vec::new(),
            policy: RunPolicy::Ask,
            bound: None,
            runs: HashMap::new(),
            run_ids: HashMap::new(),
            editors: HashMap::new(),
            incoming: HashMap::new(),
            selected: 0,
            mode: Mode::Command,
            collapsed: Default::default(),
            prompt: None,
            prompt_field: None,
            questions: Vec::new(),
            notice: None,
            conflict: false,
            ask: None,
            clipboard: None,
            chord: None,
            picker: false,
            agents_open: false,
            editing_md: Default::default(),
            focus: None,
        }
    }
}

pub type Key = (ProjectId, String);

#[derive(Default)]
pub struct Notebooks {
    pub tabs: HashMap<Key, Tab>,
}

impl Global for Notebooks {}

pub fn notebooks(cx: &mut App) -> &mut Notebooks {
    cx.default_global::<Notebooks>()
}

impl Tab {
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.cells.iter().position(|c| c.id == id)
    }

    pub fn selected_id(&self) -> Option<String> {
        self.cells.get(self.selected).map(|c| c.id.clone())
    }

    /// Apply one host event. Returns whether anything changed.
    pub fn apply(&mut self, event: NotebookEvent) {
        match event {
            NotebookEvent::Snapshot {
                rev,
                cells,
                dirty,
                can_undo,
                kernel,
                kernel_label,
                policy,
                bound,
            } => {
                for cell in &cells {
                    self.incoming.insert(cell.id.clone(), cell.source.clone());
                }
                self.rev = rev;
                self.cells = cells;
                self.dirty = dirty;
                self.can_undo = can_undo;
                self.kernel = kernel;
                self.kernel_label = kernel_label;
                self.policy = policy;
                self.bound = bound;
                self.loaded = true;
            }
            NotebookEvent::Changed {
                rev,
                order,
                cells,
                stale,
                origin,
                dirty,
                can_undo,
            } => {
                let mut by_id: HashMap<String, CellView> =
                    self.cells.drain(..).map(|c| (c.id.clone(), c)).collect();
                for cell in cells {
                    self.incoming.insert(cell.id.clone(), cell.source.clone());
                    if let kubiq_cells::Origin::Agent(agent) = &origin {
                        self.runs
                            .entry(cell.id.clone())
                            .and_modify(|r| r.1 = Some(format!("agent:{agent}")))
                            .or_insert((RunState::Idle, Some(format!("agent:{agent}"))));
                    }
                    by_id.insert(cell.id.clone(), cell);
                }
                self.cells = order.iter().filter_map(|id| by_id.remove(id)).collect();
                for cell in &mut self.cells {
                    cell.stale = stale.contains(&cell.id);
                }
                self.editors.retain(|id, _| order.contains(id));
                self.rev = rev;
                self.dirty = dirty;
                self.can_undo = can_undo;
                self.selected = self.selected.min(self.cells.len().saturating_sub(1));
            }
            NotebookEvent::Saved { .. } => {
                self.dirty = false;
                self.conflict = false;
            }
            NotebookEvent::Conflict => self.conflict = true,
            NotebookEvent::Failed { error } => self.notice = Some(error),
            NotebookEvent::Kernels { candidates } => self.candidates = candidates,
            NotebookEvent::Kernel { state, label } => {
                if let KernelState::Dead { tail, .. } = &state
                    && !tail.is_empty()
                {
                    self.notice = Some(format!("Kernel stopped: {}", tail.trim()));
                }
                self.kernel = state;
                if label.is_some() {
                    self.kernel_label = label;
                }
            }
            NotebookEvent::Running { run, cell, origin } => {
                let who = match origin {
                    kubiq_cells::Origin::Agent(a) => Some(format!("agent:{a}")),
                    _ => None,
                };
                self.run_ids.insert(run, cell.clone());
                self.runs.insert(cell, (RunState::Queued, who));
            }
            NotebookEvent::Done { run, cell, count, status } => {
                self.run_ids.remove(&run);
                let who = self.runs.get(&cell).and_then(|r| r.1.clone());
                self.runs.insert(cell.clone(), (RunState::from(status), who));
                if let Some(c) = self.cells.iter_mut().find(|c| c.id == cell)
                    && status != RunStatus::Aborted
                {
                    c.execution_count = count;
                }
                if self.prompt.as_ref().is_some_and(|i| i.0 == run) {
                    self.prompt = None;
                    self.prompt_field = None;
                }
            }
            NotebookEvent::InputRequest { run, cell, prompt, password } => {
                self.prompt = Some((run, cell, prompt, password));
                self.prompt_field = None;
            }
            NotebookEvent::RunRequested { run, agent, cells } => {
                for (cell, _) in &cells {
                    self.runs.insert(cell.clone(), (RunState::Waiting, Some(format!("agent:{agent}"))));
                }
                self.questions.push((run, agent, cells));
            }
            NotebookEvent::RunDecided { run } => {
                if let Some(at) = self.questions.iter().position(|q| q.0 == run) {
                    let (_, _, cells) = self.questions.remove(at);
                    for (cell, _) in cells {
                        if self.runs.get(&cell).is_some_and(|r| r.0 == RunState::Waiting) {
                            self.runs.remove(&cell);
                        }
                    }
                }
            }
            NotebookEvent::Policy { policy } => self.policy = policy,
            NotebookEvent::Bound { agent } => self.bound = agent,
            NotebookEvent::EnvDone { ok, log } => {
                self.notice = Some(if ok {
                    "Installed. Pick the kernel again.".to_string()
                } else {
                    format!("Install failed:\n{}", log.trim())
                });
                self.candidates.clear();
            }
        }
    }

    /// What the row of `cell` shows about running it.
    pub fn run_state(&self, cell: &CellView) -> RunState {
        match self.runs.get(&cell.id).map(|r| r.0) {
            Some(RunState::Queued) if self.run_ids.values().any(|c| c == &cell.id) => RunState::Queued,
            Some(state) if state != RunState::Ok => state,
            _ if cell.stale => RunState::Stale,
            Some(state) => state,
            None => RunState::Idle,
        }
    }
}
