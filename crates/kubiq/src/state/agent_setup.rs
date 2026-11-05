//! The onboarding's **Default agent** step: which installed harness, and which of its models, the
//! definitions a fresh install starts with run on.
//!
//! Data and small mutators only. The harness list is `WorkbenchState::agent_types` (the host's
//! `ListAgentTypes` answer, filtered to what can start here), the model list is the host's
//! `HarnessCatalogue` for the picked harness, and confirming sends `SetDefaultAgent` —
//! `app/agent_setup.rs` does all three. "Configured" is `HostSettings::default_agent` being set.

use kubiq_proto::messages::{AgentTypeInfo, CatalogueModel};

/// The step's picks while it is on screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentSetup {
    /// The picked harness id. `None` until the harness list arrives, and for good when it lists
    /// nothing installed.
    pub harness: Option<String>,
    /// The picked model id; `None` is the harness's own default.
    pub model: Option<String>,
    /// The picked harness's models, as its catalogue answered.
    pub models: Vec<CatalogueModel>,
    /// Waiting on the picked harness's catalogue.
    pub probing: bool,
}

impl AgentSetup {
    /// The harnesses the step offers: installed (or given a command) and able to hold a
    /// conversation, since the seeded definitions start conversations.
    pub fn installed(types: &[AgentTypeInfo]) -> impl Iterator<Item = &AgentTypeInfo> {
        types.iter().filter(|it| it.available && it.chat)
    }

    /// Pick `harness`. `true` when it changed — its models are then unknown and the caller asks.
    pub fn pick_harness(&mut self, harness: &str) -> bool {
        if self.harness.as_deref() == Some(harness) {
            return false;
        }
        self.harness = Some(harness.to_string());
        self.model = None;
        self.models.clear();
        self.probing = true;
        true
    }

    /// Pick the first installed harness when none is picked yet, or the picked one is gone.
    /// `true` when that changed the pick.
    pub fn preselect(&mut self, types: &[AgentTypeInfo]) -> bool {
        let still = self
            .harness
            .as_deref()
            .is_some_and(|id| Self::installed(types).any(|it| it.id == id));
        if still {
            return false;
        }
        match Self::installed(types).next() {
            Some(first) => {
                let id = first.id.clone();
                self.pick_harness(&id)
            }
            None => {
                let changed = self.harness.is_some();
                *self = Self::default();
                changed
            }
        }
    }

    /// Pick a model, or `None` for the harness's own default.
    pub fn pick_model(&mut self, model: Option<String>) {
        self.model = model;
    }

    /// A catalogue arrived. Dropped unless it is the picked harness's, asked with no account —
    /// a slow answer for a harness the user has since left must not overwrite the current one.
    /// The pick stays on the harness default unless it was already a model the list carries.
    pub fn apply_catalogue(
        &mut self,
        agent_type: &str,
        account: Option<&str>,
        models: &[CatalogueModel],
    ) {
        if account.is_some() || self.harness.as_deref() != Some(agent_type) {
            return;
        }
        self.models = models.to_vec();
        self.probing = false;
        if self
            .model
            .as_ref()
            .is_some_and(|id| !self.models.iter().any(|it| it.id == *id))
        {
            self.model = None;
        }
    }

    /// What confirming sends: the harness and the model, or nothing when no harness is picked.
    pub fn choice(&self) -> Option<(String, Option<String>)> {
        self.harness
            .clone()
            .map(|harness| (harness, self.model.clone()))
    }
}
