//! The notebook family on the window's side: sending a request about a `.ipynb`, applying what
//! the host says about one, and the actions the viewer's keys and buttons run.

use gpui::{Context, Window};
use kubiq_proto::ids::NotebookRunId;
use kubiq_proto::messages::Message;
use kubiq_proto::notebook::{CellKind, CellOp, NotebookRequest};

use super::AppState;
use crate::state::notebook::{Key, notebooks};

impl AppState {
    /// Ask the host something about a notebook.
    pub fn notebook_send(&self, key: &Key, request: NotebookRequest) {
        self.bus.send(Message::Notebook {
            project_id: key.0,
            path: key.1.clone(),
            request,
        });
    }

    /// Run a cell; `advance` moves the selection to the next cell (adding one at the end).
    pub fn notebook_run(&mut self, key: &Key, cell: &str, cx: &mut Context<Self>) {
        let run = NotebookRunId::generate();
        self.notebook_send(key, NotebookRequest::Execute { run, cell: cell.to_string() });
        cx.notify();
    }

    pub fn notebook_edit(&self, key: &Key, ops: Vec<CellOp>, cx: &mut Context<Self>) {
        let rev = notebooks(cx).tabs.get(key).map_or(0, |t| t.rev);
        self.notebook_send(key, NotebookRequest::Edit { rev, ops });
    }

    /// Insert a cell of `kind` at `index` and select it.
    pub fn notebook_insert(&mut self, key: &Key, index: usize, kind: CellKind, source: String, cx: &mut Context<Self>) {
        self.notebook_edit(
            key,
            vec![CellOp::Insert { index, id: None, kind, source }],
            cx,
        );
        if let Some(tab) = notebooks(cx).tabs.get_mut(key) {
            tab.selected = index;
        }
        cx.notify();
    }

    /// The window was told something about a notebook. Answers with the message when it is not
    /// this family's.
    pub(super) fn receive_notebook(&mut self, message: Message, cx: &mut Context<Self>) -> Option<Message> {
        match message {
            Message::NotebookEvent { project_id, path, event } => {
                if let Some(tab) = notebooks(cx).tabs.get_mut(&(project_id, path)) {
                    tab.apply(*event);
                    cx.notify();
                }
                None
            }
            other => Some(other),
        }
    }

    /// The tab's ⌘S: the host serialises and writes the notebook.
    pub fn save_notebook(&mut self, key: &Key, _window: &mut Window, cx: &mut Context<Self>) {
        let strip_outputs = self.cells_settings().strip_outputs;
        self.notebook_send(key, NotebookRequest::Save { strip_outputs });
        cx.notify();
    }
}
