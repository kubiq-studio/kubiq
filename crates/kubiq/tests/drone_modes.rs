//! The "Configure drone mode" dialog (`D232`): the host's `DroneModeStatus` lands in state and
//! drives the titlebar's badge, and Save writes the edited list through `SetSettings`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use chrono::Utc;
use gpui::{AppContext as _, Entity, TestAppContext, WindowHandle};
use gpui_component::Root;
use kubiq::app::{AppState, BusHub};
use kubiq::ext::SlotId;
use kubiq::ext::drone::{DroneField, DroneFieldInput, DroneKindSpec};
use kubiq::state::WindowRegistry;
use kubiq::state::drone::{BindChoice, EditBody, HttpForm, ext_config, field_text};
use kubiq_proto::bus::{self, FromClient, To};
use kubiq_proto::ids::ProjectId;
use kubiq_proto::messages::Message;
use kubiq_proto::projects::{ProjectHealth, ProjectRecord, ProjectSnapshot};
use kubiq_proto::settings::{
    DroneMode, DroneModeKind, DroneModeState, HostSettings, SettingsLayer,
};

const PATIENCE: Duration = Duration::from_millis(300);

struct Fixture {
    state: Entity<AppState>,
    window: WindowHandle<Root>,
    host: bus::HostEnd,
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
        let fixture = Self {
            state,
            window,
            host,
        };
        // Remote features are experimental: without the switch the dialog never opens.
        fixture.with(cx, |state, _, _| {
            state.workbench.settings.ui.experimental = true;
        });
        let _ = fixture.said();
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

    fn serving(&self, cx: &mut TestAppContext) -> bool {
        self.state
            .read_with(cx, |state, _| state.workbench.drone_modes.any_serving())
    }

    /// The host settings of the one `SetSettings` the window said, if it said one.
    fn written(&self) -> Option<HostSettings> {
        self.said().into_iter().find_map(|message| match message {
            Message::SetSettings {
                layer: SettingsLayer::Host,
                value,
            } => serde_json::from_str(&value).ok(),
            _ => None,
        })
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
            harness_agents: Default::default(),
        },
        health: ProjectHealth::Ok,
        open_panes: 0,
        ephemeral: false,
        workarea: "/tmp/kubiq-workarea".to_string(),
    }
}

fn status(id: &str, state: DroneModeState) -> Message {
    Message::DroneModeStatus {
        id: id.to_string(),
        state,
    }
}

fn serving() -> DroneModeState {
    DroneModeState::Serving {
        addr: "127.0.0.1:7420".to_string(),
        connect_string: "http://127.0.0.1:7420?token=t".to_string(),
        clients: 2,
    }
}

#[gpui::test]
fn status_lands_in_state_and_the_badge_follows_serving(cx: &mut TestAppContext) {
    let fixture = Fixture::open(cx);
    assert!(
        !fixture.serving(cx),
        "nothing serves before the host says so"
    );

    fixture.deliver(status("a", DroneModeState::Starting), cx);
    assert!(!fixture.serving(cx), "starting is not serving");

    fixture.deliver(status("a", serving()), cx);
    assert!(fixture.serving(cx));
    fixture.state.read_with(cx, |state, _| {
        assert_eq!(
            state.workbench.drone_modes.status.get("a"),
            Some(&serving())
        );
    });

    // A second mode failing does not take the first's badge away.
    fixture.deliver(
        status(
            "b",
            DroneModeState::Failed {
                reason: "port in use".to_string(),
            },
        ),
        cx,
    );
    assert!(fixture.serving(cx));

    fixture.deliver(status("a", DroneModeState::Stopped), cx);
    assert!(!fixture.serving(cx), "no mode serves, no badge");
}

#[gpui::test]
fn saving_the_form_writes_the_edited_modes(cx: &mut TestAppContext) {
    let fixture = Fixture::open(cx);
    fixture.with(cx, |state, window, cx| {
        state.open_drone_modes(cx);
        state.begin_add_drone_mode(None, window, cx);
        state.set_drone_bind(BindChoice::Lan, cx);
        let edit = state.workbench.drone_modes.edit.as_ref().expect("a form");
        edit.name
            .update(cx, |input, cx| input.set_value("Office", window, cx));
        let EditBody::Http { port, .. } = &edit.body else {
            panic!("an http form");
        };
        port.update(cx, |input, cx| input.set_value("7500", window, cx));
    });
    fixture.state.read_with(cx, |state, cx| {
        assert!(
            state.drone_form_exposed_without_tls(cx),
            "the network without TLS is warned about"
        );
    });

    fixture.with(cx, |state, _, cx| state.save_drone_edit(cx));
    let sent = fixture.written().expect("Save wrote the host layer");
    assert_eq!(sent.drone_modes.len(), 1);
    let mode = &sent.drone_modes[0];
    assert_eq!(mode.name, "Office");
    assert!(mode.enabled, "a new mode starts enabled");
    assert!(matches!(
        &mode.kind,
        DroneModeKind::Http { bind, port: 7500, token, .. } if bind == "0.0.0.0" && token.is_empty()
    ));
    fixture.state.read_with(cx, |state, _| {
        assert!(
            state.workbench.drone_modes.edit.is_none(),
            "Save returns to the list"
        );
    });

    // The switch on the list writes the flipped flag.
    let id = mode.id.clone();
    fixture.with(cx, |state, _, cx| state.toggle_drone_mode(&id, cx));
    let sent = fixture.written().expect("the toggle wrote the host layer");
    assert!(!sent.drone_modes[0].enabled);
}

#[gpui::test]
fn a_refused_form_writes_nothing(cx: &mut TestAppContext) {
    let fixture = Fixture::open(cx);
    fixture.with(cx, |state, window, cx| {
        state.open_drone_modes(cx);
        state.begin_add_drone_mode(None, window, cx);
        let edit = state.workbench.drone_modes.edit.as_ref().expect("a form");
        let EditBody::Http { port, .. } = &edit.body else {
            panic!("an http form");
        };
        port.update(cx, |input, cx| input.set_value("not a port", window, cx));
        state.save_drone_edit(cx);
    });
    assert!(fixture.written().is_none());
    fixture.state.read_with(cx, |state, _| {
        let edit = state
            .workbench
            .drone_modes
            .edit
            .as_ref()
            .expect("the form stays");
        assert!(edit.error.is_some());
    });
}

#[gpui::test]
fn edit_and_remove_round_trip_the_saved_list(cx: &mut TestAppContext) {
    let fixture = Fixture::open(cx);
    fixture.with(cx, |state, _, _| {
        state.workbench.settings.host.drone_modes = vec![DroneMode {
            id: "m1".to_string(),
            name: "Home".to_string(),
            enabled: true,
            kind: DroneModeKind::Http {
                bind: "127.0.0.1".to_string(),
                port: 7420,
                tls_cert: None,
                tls_key: None,
                tunnel_allow: Vec::new(),
                token: "keep-me".to_string(),
            },
        }];
    });
    fixture.with(cx, |state, window, cx| {
        state.open_drone_modes(cx);
        state.begin_edit_drone_mode("m1", window, cx);
        let edit = state.workbench.drone_modes.edit.as_ref().expect("a form");
        edit.name
            .update(cx, |input, cx| input.set_value("Home 2", window, cx));
        state.save_drone_edit(cx);
    });
    let sent = fixture.written().expect("Save wrote");
    assert_eq!(
        sent.drone_modes.len(),
        1,
        "an edit replaces, never duplicates"
    );
    assert_eq!(sent.drone_modes[0].name, "Home 2");
    assert!(matches!(
        &sent.drone_modes[0].kind,
        DroneModeKind::Http { token, .. } if token == "keep-me"
    ));

    fixture.with(cx, |state, _, cx| state.remove_drone_mode("m1", cx));
    assert!(
        fixture
            .written()
            .expect("Remove wrote")
            .drone_modes
            .is_empty()
    );
}

#[test]
fn the_http_form_validates() {
    let form = HttpForm {
        bind: BindChoice::Custom,
        bind_custom: "not an ip".to_string(),
        port: String::new(),
        tls_cert: String::new(),
        tls_key: String::new(),
        tunnel_allow: "a, b  c".to_string(),
    };
    assert!(form.to_kind(String::new()).is_err(), "a bind must be an IP");

    let form = HttpForm {
        bind_custom: "10.0.0.5".to_string(),
        ..form
    };
    let Ok(DroneModeKind::Http {
        bind,
        port,
        tunnel_allow,
        ..
    }) = form.to_kind(String::new())
    else {
        panic!("a valid form");
    };
    assert_eq!((bind.as_str(), port), ("10.0.0.5", 7420));
    assert_eq!(tunnel_allow, ["a", "b", "c"]);

    let half = HttpForm {
        tls_cert: "/c.pem".to_string(),
        ..form
    };
    assert!(half.to_kind(String::new()).is_err(), "TLS needs both paths");
}

const FIELDS: &[DroneField] = &[
    DroneField {
        key: "port",
        label: "Port",
        note: "",
        input: DroneFieldInput::Number {
            placeholder: "3389",
        },
    },
    DroneField {
        key: "pipe",
        label: "Pipe",
        note: "",
        input: DroneFieldInput::Text { placeholder: "" },
    },
    DroneField {
        key: "allow",
        label: "Allow",
        note: "",
        input: DroneFieldInput::Lines { placeholder: "" },
    },
];

#[test]
fn an_edition_form_writes_its_configuration() {
    let spec = DroneKindSpec {
        id: SlotId::new("test.drone.x"),
        kind: "x",
        label: "X",
        description: "",
        default_config: Default::default,
        fields: FIELDS,
    };
    let mut base = serde_json::Map::new();
    base.insert("keep".to_string(), true.into());
    let typed = HashMap::from([
        ("port", "3390".to_string()),
        ("pipe", " ".to_string()),
        ("allow", "a:1\n\n b:2 \n".to_string()),
    ]);
    let config = ext_config(&spec, &base, &typed).expect("a valid form");
    assert_eq!(config.get("port"), Some(&serde_json::json!(3390)));
    assert!(config.get("pipe").is_none(), "an empty box removes its key");
    assert_eq!(
        config.get("allow"),
        Some(&serde_json::json!(["a:1", "b:2"]))
    );
    let typed = HashMap::from([("allow", "".to_string())]);
    let mut had = base.clone();
    had.insert("allow".to_string(), serde_json::json!(["x"]));
    assert!(
        ext_config(&spec, &had, &typed)
            .unwrap()
            .get("allow")
            .is_none()
    );
    assert_eq!(field_text(&config, "allow"), "a:1\nb:2");
    assert_eq!(
        config.get("keep"),
        Some(&serde_json::json!(true)),
        "unnamed keys survive"
    );

    let typed = HashMap::from([("port", "x".to_string())]);
    assert!(ext_config(&spec, &base, &typed).is_err());
}

#[gpui::test]
fn a_remote_hosts_status_is_not_this_windows(cx: &mut TestAppContext) {
    use kubiq::app::{Bus, HostRef};
    use kubiq_proto::settings::RemoteScheme;

    let (local, _end) = bus::detached();
    let mut bus = Bus::new(local);
    let (remote, _) = bus.register_remote(
        bus::detached().0,
        "far".to_string(),
        String::new(),
        "far".to_string(),
        RemoteScheme::Http,
    );

    let fixture = Fixture::open(cx);
    fixture.with(cx, |state, _, cx| {
        let rest = state.receive_drone(HostRef::Remote(remote), status("far", serving()), cx);
        assert!(rest.is_none(), "the message is this family's either way");
    });
    assert!(
        !fixture.serving(cx),
        "a remote's serving does not light the badge"
    );
    fixture.state.read_with(cx, |state, _| {
        assert!(state.workbench.drone_modes.status.is_empty());
    });
    fixture.with(cx, |state, _, cx| {
        state.receive_drone(HostRef::Local, status("here", serving()), cx);
    });
    assert!(fixture.serving(cx));
}

#[gpui::test]
fn an_enabled_mode_keeps_its_way_in_when_the_tier_is_lowered(cx: &mut TestAppContext) {
    let fixture = Fixture::open(cx);
    let a_mode = |enabled| DroneMode {
        id: "m".to_string(),
        name: "m".to_string(),
        enabled,
        kind: DroneModeKind::Ext {
            kind: "x".to_string(),
            config: Default::default(),
        },
    };
    fixture.with(cx, |state, _, _| {
        state.workbench.settings.ui.experimental = false;
        state.workbench.settings.host.drone_modes = vec![a_mode(false)];
    });
    fixture.with(cx, |state, _, cx| {
        assert!(
            !state.drone_modes_reachable(),
            "nothing enabled, tier off: hidden"
        );
        state.open_drone_modes(cx);
        assert!(!state.workbench.drone_modes.open);
        state.workbench.settings.host.drone_modes = vec![a_mode(true)];
        assert!(
            state.drone_modes_reachable(),
            "an enabled mode is never hidden"
        );
        state.open_drone_modes(cx);
        assert!(state.workbench.drone_modes.open, "and its dialog opens");
    });
}
