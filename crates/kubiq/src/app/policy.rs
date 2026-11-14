//! The policy manager's mutators: what each gesture on the Policies section, the policy editor,
//! the import question and the clone prompt sends, and what the host's answers do to them.
//!
//! **Every write is a message and the answer is the list.** The section redraws from the
//! `Policies` the host broadcasts after a change, and an editor that was waiting on a save closes
//! when it arrives. A refusal (`PolicyError`) leaves whatever is up in place with the host's
//! sentence on it.

use super::*;
use crate::state::policy::{PolicyCloneAs, PolicyEditor, PolicyFill, PolicyImport, Section, Tick};
use kubiq_proto::policy::PolicyInfo;

impl AppState {
    /// Ask for the list. The answer is a `Policies`, to everyone.
    pub fn ask_policies(&mut self) {
        self.bus.send(Message::ListPolicies);
    }

    /// The section asks on arrival, so a change made in another window shows up here.
    pub(crate) fn on_show_policies(&mut self, _cx: &mut Context<Self>) {
        self.ask_policies();
    }

    pub fn dismiss_policy_error(&mut self, cx: &mut Context<Self>) {
        self.workbench.policy.error = None;
        cx.notify();
    }

    // ── The editor ──────────────────────────────────────────────────

    /// Raise the editor on an empty local policy.
    pub fn open_policy_editor_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.workbench.policy.error = None;
        self.workbench.policy.editor = Some(PolicyEditor::blank(window, cx));
        cx.notify();
    }

    /// Ask the host for a policy; the editor opens when `PolicyBody` arrives.
    pub fn open_policy_editor(&mut self, name: String, cx: &mut Context<Self>) {
        self.workbench.policy.error = None;
        self.bus.send(Message::ReadPolicy { name });
        cx.notify();
    }

    pub fn close_policy_editor(&mut self, cx: &mut Context<Self>) {
        self.workbench.policy.editor = None;
        self.workbench.policy.fill = None;
        self.workbench.policy.error = None;
        cx.notify();
    }

    /// Build the editor over a `PolicyBody` the wire left waiting. Run once a frame, where there
    /// is a window to make the boxes with.
    pub(super) fn sync_policy_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(fill) = self.workbench.policy.fill.take() else {
            return;
        };
        self.workbench.policy.editor = Some(PolicyEditor::from_body(&fill, window, cx));
        cx.notify();
    }

    pub fn save_policy_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.workbench.policy.editor.as_ref() else {
            return;
        };
        if !editor.ready(cx) {
            return;
        }
        let name = editor.name_text(cx);
        // The host overwrites a local policy of the same name when `replaces` is `None`, and a
        // rename onto another name would do the same: refuse both here.
        let renamed = editor.replaces.as_deref() != Some(name.as_str());
        if renamed
            && self
                .workbench
                .policy
                .policies
                .iter()
                .any(|p| p.name == name)
        {
            self.workbench.policy.error =
                Some(format!("A policy named \"{name}\" already exists."));
            cx.notify();
            return;
        }
        let message = Message::SavePolicy {
            name,
            replaces: editor.replaces.clone(),
            description: editor.description.read(cx).value().trim().to_string(),
            doc: editor.to_doc(cx),
        };
        if let Some(editor) = self.workbench.policy.editor.as_mut() {
            editor.saving = true;
        }
        self.workbench.policy.error = None;
        self.bus.send(message);
        cx.notify();
    }

    fn with_editor(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut PolicyEditor)) {
        if let Some(editor) = self.workbench.policy.editor.as_mut() {
            edit(editor);
        }
        cx.notify();
    }

    pub fn policy_tick(&mut self, tick: Tick, value: String, cx: &mut Context<Self>) {
        self.with_editor(cx, |editor| editor.toggle(tick, &value));
    }

    pub fn policy_toggle_section(&mut self, section: Section, cx: &mut Context<Self>) {
        self.with_editor(cx, |editor| editor.toggle_section(section));
    }

    pub fn policy_toggle_home(&mut self, scratch: bool, cx: &mut Context<Self>) {
        self.with_editor(cx, |editor| match scratch {
            true => editor.home_scratch = !editor.home_scratch,
            false => editor.home_enabled = !editor.home_enabled,
        });
    }

    pub fn policy_add_path(
        &mut self,
        rule: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = self.workbench.policy.editor.as_mut() {
            editor.add_path(rule, window, cx);
        }
        cx.notify();
    }

    pub fn policy_remove_path(
        &mut self,
        rule: Option<usize>,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        self.with_editor(cx, |editor| editor.remove_path(rule, index));
    }

    pub fn policy_cycle_path(
        &mut self,
        rule: Option<usize>,
        index: usize,
        matching: bool,
        cx: &mut Context<Self>,
    ) {
        self.with_editor(cx, |editor| editor.cycle_path(rule, index, matching));
    }

    pub fn policy_add_env(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.workbench.policy.editor.as_mut() {
            editor.add_env(window, cx);
        }
        cx.notify();
    }

    pub fn policy_remove_env(&mut self, index: usize, cx: &mut Context<Self>) {
        self.with_editor(cx, |editor| editor.remove_env(index));
    }

    pub fn policy_add_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.workbench.policy.editor.as_mut() {
            editor.add_rule(window, cx);
        }
        cx.notify();
    }

    pub fn policy_remove_rule(&mut self, index: usize, cx: &mut Context<Self>) {
        self.with_editor(cx, |editor| editor.remove_rule(index));
    }

    // ── The list's own verbs ────────────────────────────────────────

    pub fn set_policy_default(&mut self, name: String, default: bool, cx: &mut Context<Self>) {
        self.workbench.policy.error = None;
        self.bus.send(Message::SetPolicyDefault { name, default });
        cx.notify();
    }

    pub fn ask_delete_policy(&mut self, name: String, cx: &mut Context<Self>) {
        self.workbench.policy.confirm_delete = Some(name);
        cx.notify();
    }

    pub fn cancel_delete_policy(&mut self, cx: &mut Context<Self>) {
        self.workbench.policy.confirm_delete = None;
        cx.notify();
    }

    pub fn confirm_delete_policy(&mut self, cx: &mut Context<Self>) {
        if let Some(name) = self.workbench.policy.confirm_delete.take() {
            self.workbench.policy.error = None;
            self.bus.send(Message::DeletePolicy { name });
        }
        cx.notify();
    }

    // ── Clone ───────────────────────────────────────────────────────

    /// Ask what to call the copy of `source`, pre-filled with a free name.
    pub fn open_policy_clone(
        &mut self,
        source: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let policy = &self.workbench.policy;
        let taken = |name: &str| policy.find(name).is_some();
        let mut suggestion = format!("{source}-copy");
        let mut n = 2;
        while taken(&suggestion) {
            suggestion = format!("{source}-copy-{n}");
            n += 1;
        }
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("my-policy"));
        name.update(cx, |state, cx| {
            state.set_value(&suggestion, window, cx);
            state.focus(window, cx);
        });
        self.workbench.policy.error = None;
        self.workbench.policy.clone_as = Some(PolicyCloneAs { source, name });
        cx.notify();
    }

    pub fn close_policy_clone(&mut self, cx: &mut Context<Self>) {
        self.workbench.policy.clone_as = None;
        cx.notify();
    }

    pub fn run_policy_clone(&mut self, cx: &mut Context<Self>) {
        let Some(ask) = self.workbench.policy.clone_as.as_ref() else {
            return;
        };
        let new_name = ask.name.read(cx).value().trim().to_string();
        if new_name.is_empty() {
            return;
        }
        let name = ask.source.clone();
        self.workbench.policy.clone_as = None;
        // Cloning from the read-only view lands on the copy, which is the one to work on.
        self.workbench.policy.editor = None;
        self.bus.send(Message::ClonePolicy { name, new_name });
        cx.notify();
    }

    // ── Import ──────────────────────────────────────────────────────

    pub fn open_policy_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let import = PolicyImport::new(window, cx);
        import.url.update(cx, |state, cx| state.focus(window, cx));
        self.workbench.policy.error = None;
        self.workbench.policy.import = Some(import);
        cx.notify();
    }

    pub fn close_policy_import(&mut self, cx: &mut Context<Self>) {
        self.workbench.policy.import = None;
        self.workbench.policy.error = None;
        cx.notify();
    }

    pub fn run_policy_import(&mut self, cx: &mut Context<Self>) {
        let Some(import) = self.workbench.policy.import.as_mut() else {
            return;
        };
        let url = import.url.read(cx).value().trim().to_string();
        if url.is_empty() || import.busy {
            return;
        }
        let git_ref = Some(import.git_ref.read(cx).value().trim().to_string())
            .filter(|git_ref| !git_ref.is_empty());
        import.busy = true;
        import.result = None;
        self.workbench.policy.error = None;
        self.bus.send(Message::ImportPolicies { url, git_ref });
        cx.notify();
    }

    // ── What the host says ──────────────────────────────────────────

    /// The list, replaced whole. An editor waiting on a save is done once it arrives.
    pub(super) fn apply_policies(&mut self, policies: Vec<PolicyInfo>, cx: &mut Context<Self>) {
        let state = &mut self.workbench.policy;
        state.policies = policies;
        if state.editor.as_ref().is_some_and(|editor| editor.saving) {
            state.editor = None;
        }
        cx.notify();
    }

    pub(super) fn apply_policy_body(
        &mut self,
        info: PolicyInfo,
        doc: kubiq_proto::policy::PolicyDoc,
        toml: String,
        cx: &mut Context<Self>,
    ) {
        self.workbench.policy.fill = Some(PolicyFill { info, doc, toml });
        cx.notify();
    }

    pub(super) fn apply_policy_imported(&mut self, names: Vec<String>, cx: &mut Context<Self>) {
        if let Some(import) = self.workbench.policy.import.as_mut() {
            import.busy = false;
            import.result = Some(names);
        }
        cx.notify();
    }

    pub(super) fn apply_policy_error(&mut self, error: String, cx: &mut Context<Self>) {
        let state = &mut self.workbench.policy;
        if let Some(editor) = state.editor.as_mut() {
            editor.saving = false;
        }
        if let Some(import) = state.import.as_mut() {
            import.busy = false;
        }
        state.error = Some(error);
        cx.notify();
    }

    // ── On the New agent form ───────────────────────────────────────

    /// Tick or untick one policy on the open form. The list stays down, like the MCP one.
    pub fn toggle_new_agent_policy(&mut self, name: String, cx: &mut Context<Self>) {
        // A default is applied whatever the form says, so its row is not the form's to untick.
        let default = self
            .workbench
            .policy
            .find(&name)
            .is_some_and(|policy| policy.default);
        if let Some(form) = self.new_agent_form_mut().filter(|_| !default) {
            form.toggle_policy(&name);
        }
        cx.notify();
    }
}
