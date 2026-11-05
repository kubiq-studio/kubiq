//! The onboarding's Default agent step, at the state level: offered at the end of the welcome
//! only while no default agent is configured, picking from what is installed, taking the picked
//! harness's catalogue and no other, and confirming with `SetDefaultAgent` when the run is passed.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{AppContext as _, Entity, TestAppContext};
use kubiq::app::{AppState, BusHub};
use kubiq::state::WindowRegistry;
use kubiq::state::onboarding::{FormKind, StepBody, WELCOME};
use kubiq_proto::bus::{self, FromClient, To};
use kubiq_proto::messages::{AgentTypeInfo, CatalogueModel, Message};
use kubiq_proto::settings::DefaultAgent;

const PATIENCE: Duration = Duration::from_millis(300);

struct Fixture {
    state: Entity<AppState>,
    host: bus::HostEnd,
}

impl Fixture {
    fn open(cx: &mut TestAppContext) -> Self {
        let (hub, host) = bus::hub();
        cx.update(|cx| {
            gpui_component::init(cx);
            kubiq::theme::set_mode(kubiq::app::boot_theme(), cx);
            BusHub::install(hub, cx);
            WindowRegistry::install(cx);
            kubiq::app::install_key_bindings(cx);
        });
        let held: Rc<RefCell<Option<Entity<AppState>>>> = Rc::default();
        let taken = held.clone();
        cx.add_window(move |window, cx| {
            let state = cx.new(|cx| AppState::for_project(None, 'A', window, cx));
            *taken.borrow_mut() = Some(state.clone());
            gpui_component::Root::new(state, window, cx)
        });
        cx.run_until_parked();
        let state = held
            .borrow_mut()
            .take()
            .expect("the window built its state");
        state.update(cx, |state, _| state.workbench.prefs_loaded = true);
        let fixture = Self { state, host };
        fixture.said();
        fixture
    }

    fn said(&self) -> Vec<Message> {
        let mut said = Vec::new();
        while let Ok(event) = self.host.recv_timeout(PATIENCE) {
            if let FromClient::Said { message, .. } = event {
                said.push(message);
            }
        }
        said
    }

    fn deliver(&self, message: Message, cx: &mut TestAppContext) {
        self.host.send(To::Everyone, message);
        cx.run_until_parked();
    }

    fn update<R>(
        &self,
        cx: &mut TestAppContext,
        f: impl FnOnce(&mut AppState, &mut gpui::Context<AppState>) -> R,
    ) -> R {
        let out = self.state.update(cx, f);
        cx.run_until_parked();
        out
    }

    /// Open the welcome and walk to its last step.
    fn to_last_step(&self, cx: &mut TestAppContext) {
        self.update(cx, |state, cx| state.open_onboarding(WELCOME, cx));
        loop {
            let last = self.state.read_with(cx, |state, _| {
                state.workbench.onboarding.as_ref().unwrap().is_last()
            });
            if last {
                break;
            }
            self.update(cx, |state, cx| state.onboarding_next(cx));
        }
    }
}

fn harness(id: &str, available: bool, chat: bool) -> AgentTypeInfo {
    AgentTypeInfo {
        id: id.to_string(),
        label: id.to_string(),
        command: id.to_string(),
        available,
        chat,
        acp: false,
        modes: Vec::new(),
        unattended_mode: None,
        keeps_sessions: true,
        quota: Default::default(),
        shares_home: false,
        steers: false,
        device_login: false,
    }
}

fn model(id: &str) -> CatalogueModel {
    CatalogueModel {
        id: id.to_string(),
        description: None,
        default: false,
        levels: Vec::new(),
        default_level: None,
    }
}

fn last_body(fixture: &Fixture, cx: &mut TestAppContext) -> StepBody {
    fixture.state.read_with(cx, |state, _| {
        state
            .workbench
            .onboarding
            .as_ref()
            .unwrap()
            .current()
            .body
            .clone()
    })
}

#[gpui::test]
fn the_welcome_ends_on_the_step_only_while_no_default_is_configured(cx: &mut TestAppContext) {
    let fixture = Fixture::open(cx);
    fixture.to_last_step(cx);
    assert_eq!(
        last_body(&fixture, cx),
        StepBody::Form(FormKind::DefaultAgent)
    );
    fixture.update(cx, |state, cx| state.skip_onboarding(cx));

    fixture.update(cx, |state, _| {
        state.workbench.settings.host.default_agent = Some(DefaultAgent {
            agent_type: "codex".to_string(),
            model: None,
        });
    });
    fixture.to_last_step(cx);
    assert_ne!(
        last_body(&fixture, cx),
        StepBody::Form(FormKind::DefaultAgent),
        "a configured default takes the step out"
    );
}

#[gpui::test]
fn the_step_picks_from_what_is_installed_and_confirms_on_finish(cx: &mut TestAppContext) {
    let fixture = Fixture::open(cx);
    fixture.update(cx, |state, _| {
        state.workbench.agent_types = vec![
            harness("absent", false, true),
            harness("pane-only", true, false),
            harness("claude-code", true, true),
            harness("codex", true, true),
        ];
    });
    fixture.to_last_step(cx);
    let said = fixture.said();
    assert!(said.iter().any(|it| matches!(it, Message::ListAgentTypes)));
    assert!(
        said.iter().any(|it| matches!(
            it,
            Message::ListHarnessCatalogue { agent_type, account: None } if agent_type == "claude-code"
        )),
        "the first installed conversational harness is preselected and asked about"
    );

    fixture.update(cx, |state, cx| state.agent_setup_pick_harness("codex", cx));
    // A late answer for the harness just left is dropped; the picked one's is taken.
    fixture.deliver(
        Message::HarnessCatalogue {
            agent_type: "claude-code".to_string(),
            account: None,
            models: vec![model("sonnet")],
            last_model: String::new(),
            last_thinking: String::new(),
        },
        cx,
    );
    fixture.state.read_with(cx, |state, _| {
        assert!(state.workbench.agent_setup.probing);
        assert!(state.workbench.agent_setup.models.is_empty());
    });
    fixture.deliver(
        Message::HarnessCatalogue {
            agent_type: "codex".to_string(),
            account: None,
            models: vec![model("gpt-5-codex"), model("gpt-5")],
            last_model: String::new(),
            last_thinking: String::new(),
        },
        cx,
    );
    fixture.state.read_with(cx, |state, _| {
        let setup = &state.workbench.agent_setup;
        assert!(!setup.probing);
        assert_eq!(setup.models.len(), 2);
        assert_eq!(setup.model, None, "the harness default until one is picked");
    });
    fixture.update(cx, |state, cx| {
        state.agent_setup_pick_model(Some("gpt-5-codex".to_string()), cx)
    });
    fixture.said();

    fixture.update(cx, |state, cx| state.onboarding_next(cx));
    let said = fixture.said();
    assert!(said.iter().any(|it| matches!(
        it,
        Message::SetDefaultAgent { agent_type, model: Some(model) }
            if agent_type == "codex" && model == "gpt-5-codex"
    )));
    fixture.state.read_with(cx, |state, _| {
        assert!(
            state.workbench.onboarding.is_none(),
            "Finish closed the run"
        );
    });
}

#[gpui::test]
fn skip_and_nothing_installed_send_no_default(cx: &mut TestAppContext) {
    let fixture = Fixture::open(cx);
    fixture.update(cx, |state, _| {
        state.workbench.agent_types = vec![harness("claude-code", true, true)];
    });
    fixture.to_last_step(cx);
    fixture.update(cx, |state, cx| state.skip_onboarding(cx));
    assert!(
        !fixture
            .said()
            .iter()
            .any(|it| matches!(it, Message::SetDefaultAgent { .. })),
        "Skip leaves the default unset"
    );

    fixture.update(cx, |state, _| {
        state.workbench.agent_types = vec![harness("claude-code", false, true)];
    });
    fixture.to_last_step(cx);
    fixture.state.read_with(cx, |state, _| {
        assert_eq!(state.workbench.agent_setup.harness, None);
    });
    fixture.update(cx, |state, cx| state.onboarding_next(cx));
    assert!(
        !fixture
            .said()
            .iter()
            .any(|it| matches!(it, Message::SetDefaultAgent { .. })),
        "with nothing installed, Finish only closes"
    );
}
