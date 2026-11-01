//! The trust prompt (`D230`): `UntrustedProjectSettings` is kept per project and drawn as a strip,
//! approving sends `TrustProjectItems` with what the strip lists, and an empty list clears it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use chrono::Utc;
use gpui::{AppContext as _, Entity, TestAppContext, WindowHandle};
use gpui_component::Root;
use kubiq::app::{AppState, BusHub};
use kubiq::state::WindowRegistry;
use kubiq_proto::bus::{self, FromClient, To};
use kubiq_proto::ids::ProjectId;
use kubiq_proto::messages::Message;
use kubiq_proto::projects::{
    ProjectHealth, ProjectRecord, ProjectSnapshot, TrustItem, TrustKind, TrustReason,
    UntrustedItem,
};

const PATIENCE: Duration = Duration::from_millis(500);

struct Fixture {
    state: Entity<AppState>,
    window: WindowHandle<Root>,
    host: bus::HostEnd,
    project: ProjectId,
}

impl Fixture {
    fn open(cx: &mut TestAppContext) -> Self {
        let snapshot = a_project();
        let project = snapshot.record.id;
        let (hub, host) = bus::hub();

        cx.update(|cx| {
            gpui_component::init(cx);
            kubiq::theme::set_mode(kubiq::app::boot_theme(), cx);
            BusHub::install(hub, cx);
            WindowRegistry::install(cx);
            cx.global_mut::<WindowRegistry>().apply(snapshot);
        });

        let held: Rc<RefCell<Option<Entity<AppState>>>> = Rc::default();
        let taken = held.clone();
        let window = cx.add_window(move |window, cx| {
            let state = cx.new(|cx| AppState::for_project(Some(project), 'A', window, cx));
            *taken.borrow_mut() = Some(state.clone());
            Root::new(state, window, cx)
        });
        cx.run_until_parked();

        let state = held
            .borrow_mut()
            .take()
            .expect("the window built its state");
        Self {
            state,
            window,
            host,
            project,
        }
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

    fn with<R>(
        &self,
        cx: &mut TestAppContext,
        f: impl FnOnce(&mut AppState, &mut gpui::Window, &mut gpui::Context<AppState>) -> R,
    ) -> R {
        let out = self
            .window
            .update(cx, |_, window, cx| {
                self.state.update(cx, |state, cx| f(state, window, cx))
            })
            .expect("the window is open");
        cx.run_until_parked();
        out
    }
}

fn a_project() -> ProjectSnapshot {
    ProjectSnapshot {
        record: ProjectRecord {
            id: ProjectId::generate(),
            name: "kubiq".to_string(),
            path: "/tmp/kubiq".to_string(),
            colour: 0,
            custom_colour: None,
            storage: Default::default(),
            temporary: false,
            created_at: Utc::now(),
            last_opened_at: None,
            search_excludes: Vec::new(),
            index: None,
            mission_term: None,
            tools: Vec::new(),
            managed_repos: Vec::new(),
            lanes: Vec::new(),
            runs_on: None,
            initials: String::new(),
            definitions_use_global: true,
            definitions_allowed: Vec::new(),
        },
        health: ProjectHealth::Ok,
        open_panes: 0,
        ephemeral: false,
        workarea: "/tmp/kubiq-workarea".to_string(),
    }
}


fn item(kind: TrustKind, id: &str, reason: TrustReason) -> UntrustedItem {
    UntrustedItem {
        kind,
        id: id.to_string(),
        reason,
        fingerprint: format!("print-of-{id}"),
    }
}

#[gpui::test]
fn untrusted_settings_are_listed_approved_and_cleared(cx: &mut TestAppContext) {
    let fixture = Fixture::open(cx);
    let items = vec![
        item(TrustKind::Definition, "reviewer", TrustReason::Changed),
        item(TrustKind::Mcp, "linter", TrustReason::New),
    ];
    fixture.deliver(
        Message::UntrustedProjectSettings {
            project_id: fixture.project,
            items: items.clone(),
        },
        cx,
    );
    fixture.state.read_with(cx, |state, cx| {
        let open = state.open_project(cx).expect("the project is open");
        assert_eq!(open.untrusted, items);
    });
    fixture.said();

    // Approve all names exactly what the prompt lists, with the fingerprints it was shown.
    let project = fixture.project;
    fixture.with(cx, |state, _, cx| state.trust_project_items(project, None, cx));
    let said = fixture.said();
    assert!(said.iter().any(|message| matches!(
        message,
        Message::TrustProjectItems { project_id, items: sent }
            if *project_id == fixture.project
                && *sent == vec![
                    TrustItem {
                        kind: TrustKind::Definition,
                        id: "reviewer".to_string(),
                        fingerprint: "print-of-reviewer".to_string(),
                    },
                    TrustItem {
                        kind: TrustKind::Mcp,
                        id: "linter".to_string(),
                        fingerprint: "print-of-linter".to_string(),
                    },
                ]
    )), "{said:?}");

    // One item, approved alone.
    fixture.with(cx, |state, _, cx| {
        state.trust_project_items(
            project,
            Some(vec![TrustItem {
                kind: TrustKind::Mcp,
                id: "linter".to_string(),
                fingerprint: "print-of-linter".to_string(),
            }]),
            cx,
        )
    });
    let said = fixture.said();
    assert!(said.iter().any(|message| matches!(
        message,
        Message::TrustProjectItems { items: sent, .. } if sent.len() == 1 && sent[0].id == "linter"
    )));

    // The host's answer with nothing left clears the prompt, and approving then sends nothing.
    fixture.deliver(
        Message::UntrustedProjectSettings {
            project_id: fixture.project,
            items: Vec::new(),
        },
        cx,
    );
    fixture.state.read_with(cx, |state, cx| {
        assert!(state.open_project(cx).expect("open").untrusted.is_empty());
    });
    fixture.with(cx, |state, _, cx| state.trust_project_items(project, None, cx));
    assert!(fixture.said().is_empty());
}
