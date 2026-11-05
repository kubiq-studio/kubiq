//! The onboarding's **Default agent** step as the window acts on it: whether it is due, the picks,
//! and the confirm.
//!
//! The picks are `crate::state::agent_setup`; the step is drawn by `ui::onboarding::agent_setup`.
//! Nothing here knows a harness: the list is the host's `AgentTypes`, the models its
//! `HarnessCatalogue`, and the confirm is `SetDefaultAgent`, which records the default and
//! re-points the seeded definitions host-side.

use gpui::Context;
use kubiq_proto::messages::Message;

use super::AppState;
use crate::state::onboarding::{FormKind, StepBody};

impl AppState {
    /// Whether the default agent is configured: the host's settings name one. The onboarding step
    /// is offered only while this is false.
    pub fn default_agent_configured(&self) -> bool {
        self.workbench.settings.host.default_agent.is_some()
    }

    /// Whether the onboarding run is on the Default agent step.
    pub fn on_agent_setup_step(&self) -> bool {
        self.workbench
            .onboarding
            .as_ref()
            .is_some_and(|run| matches!(run.current().body, StepBody::Form(FormKind::DefaultAgent)))
    }

    /// Arriving at the step: ask for the harness list again (an install since launch should be
    /// offered), and pick from what is already known.
    pub(super) fn enter_agent_setup(&mut self, cx: &mut Context<Self>) {
        self.bus.send(Message::ListAgentTypes);
        self.agent_setup_types_changed(cx);
    }

    /// The harness list moved: keep the pick if it is still installed, else take the first.
    pub(super) fn agent_setup_types_changed(&mut self, cx: &mut Context<Self>) {
        if !self.on_agent_setup_step() {
            return;
        }
        if self
            .workbench
            .agent_setup
            .preselect(&self.workbench.agent_types)
        {
            self.probe_agent_setup();
        }
        cx.notify();
    }

    /// The user picked a harness.
    pub fn agent_setup_pick_harness(&mut self, harness: &str, cx: &mut Context<Self>) {
        if self.workbench.agent_setup.pick_harness(harness) {
            self.probe_agent_setup();
        }
        cx.notify();
    }

    /// The user picked a model, or the harness's default.
    pub fn agent_setup_pick_model(&mut self, model: Option<String>, cx: &mut Context<Self>) {
        self.workbench.agent_setup.pick_model(model);
        cx.notify();
    }

    /// Passing the step: send the pick, when there is one. Nothing installed sends nothing.
    pub(super) fn commit_agent_setup(&mut self) {
        if let Some((agent_type, model)) = self.workbench.agent_setup.choice() {
            self.bus
                .send(Message::SetDefaultAgent { agent_type, model });
        }
        self.workbench.agent_setup = Default::default();
    }

    fn probe_agent_setup(&mut self) {
        if let Some(agent_type) = self.workbench.agent_setup.harness.clone() {
            self.bus.send(Message::ListHarnessCatalogue {
                agent_type,
                account: None,
            });
        }
    }
}
