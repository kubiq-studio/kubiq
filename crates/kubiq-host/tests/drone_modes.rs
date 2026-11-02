//! Drone modes (`D232`) against real sockets: a mode starts on an ephemeral loopback port, counts
//! the windows attached through it, stops and frees the port; a port already taken fails the mode
//! without failing anything else; and the coordinator mints and persists a missing token.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use kubiq_host::drone_modes::{DroneEndpoint, DroneModes, Launch, Registry, Running, Started};
use kubiq_proto::bus::{self, To};
use kubiq_proto::messages::Message;
use kubiq_proto::settings::{DroneMode, DroneModeKind, DroneModeState};

const PATIENCE: Duration = Duration::from_secs(5);

fn http(id: &str, port: u16, token: &str) -> DroneMode {
    DroneMode {
        id: id.to_string(),
        name: id.to_string(),
        enabled: true,
        kind: DroneModeKind::Http {
            bind: "127.0.0.1".to_string(),
            port,
            tls_cert: None,
            tls_key: None,
            tunnel_allow: Vec::new(),
            token: token.to_string(),
        },
    }
}

/// A service bound to a hub, and a window that hears what it says.
fn service(kinds: Registry) -> (DroneModes, bus::Client, bus::HostEnd) {
    let (hub, host) = bus::hub();
    let window = hub.connect();
    let mut drones = DroneModes::new(Some(hub), kinds);
    drones.bind(host.mailbox(To::Everyone));
    (drones, window, host)
}

/// The next status for `id` that satisfies `want`, skipping everything else.
fn wait_for(
    window: &bus::Client,
    id: &str,
    want: impl Fn(&DroneModeState) -> bool,
) -> DroneModeState {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match window.from_host().recv_timeout(left) {
            Ok(Message::DroneModeStatus { id: said, state }) if said == id && want(&state) => {
                return state;
            }
            Ok(_) => {}
            Err(error) => panic!("no matching status for {id}: {error}"),
        }
    }
}

fn serving_addr(state: &DroneModeState) -> String {
    match state {
        DroneModeState::Serving { addr, .. } => addr.clone(),
        other => panic!("not serving: {other:?}"),
    }
}

fn attach(addr: &str, token: &str) -> TcpStream {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.set_read_timeout(Some(PATIENCE)).unwrap();
    write!(
        stream,
        "GET /attach?token={token} HTTP/1.1\r\nHost: x\r\nUpgrade: kubiq\r\n\r\n"
    )
    .unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        assert_eq!(stream.read(&mut byte).unwrap(), 1, "closed mid-response");
        head.push(byte[0]);
    }
    assert!(
        head.starts_with(b"HTTP/1.1 101"),
        "{}",
        String::from_utf8_lossy(&head)
    );
    stream
}

#[test]
fn an_http_mode_serves_counts_and_stops() {
    let (mut drones, window, _host) = service(Registry::new());
    drones.reconcile(&[http("m", 0, "secret")]);

    let state = wait_for(&window, "m", |s| {
        matches!(s, DroneModeState::Serving { .. })
    });
    let addr = serving_addr(&state);
    let DroneModeState::Serving { connect_string, .. } = &state else {
        unreachable!()
    };
    assert!(connect_string.starts_with("http://127.0.0.1:"));
    assert!(connect_string.ends_with("?token=secret"));

    let mut attached = attach(&addr, "secret");
    wait_for(&window, "m", |s| {
        matches!(s, DroneModeState::Serving { clients: 1, .. })
    });

    // Disabling it stops it: the attached socket is closed and the port is free again.
    let mut off = http("m", 0, "secret");
    off.enabled = false;
    drones.reconcile(&[off]);
    wait_for(&window, "m", |s| *s == DroneModeState::Stopped);
    let mut buf = [0u8; 64];
    let deadline = Instant::now() + PATIENCE;
    loop {
        match attached.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => assert!(Instant::now() < deadline, "the session was never closed"),
        }
    }
    // `stop` returned with the accept loop ended: the port is free with no wait.
    TcpListener::bind(&addr).expect("the port is free once the mode reads Stopped");
}

#[test]
fn a_mode_restarts_on_its_own_fixed_port_at_once() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let (mut drones, window, _host) = service(Registry::new());
    drones.reconcile(&[http("m", port, "t")]);
    let addr = serving_addr(&wait_for(&window, "m", |s| {
        matches!(s, DroneModeState::Serving { .. })
    }));
    // A window attached, so the restart also has an accepted socket to close.
    let _attached = attach(&addr, "t");
    wait_for(&window, "m", |s| {
        matches!(s, DroneModeState::Serving { clients: 1, .. })
    });

    for _ in 0..3 {
        assert!(drones.restart("m"));
        let state = drones.state("m").unwrap();
        assert_eq!(serving_addr(&state), addr, "{state:?}");
    }
}

#[test]
fn a_port_in_use_fails_the_mode_and_nothing_else() {
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let (mut drones, window, _host) = service(Registry::new());
    drones.reconcile(&[http("busy", port, "t"), http("free", 0, "t")]);

    let failed = wait_for(&window, "busy", |s| {
        matches!(s, DroneModeState::Failed { .. })
    });
    let DroneModeState::Failed { reason } = failed else {
        unreachable!()
    };
    assert!(reason.contains(&port.to_string()), "{reason}");
    assert!(matches!(
        drones.state("free"),
        Some(DroneModeState::Serving { .. })
    ));

    // The retry fails the same way while the port is held, and serves once it is not.
    drop(taken);
    assert!(drones.restart("busy"));
    wait_for(&window, "busy", |s| {
        matches!(s, DroneModeState::Serving { .. })
    });
}

#[test]
fn a_mode_removed_from_the_list_is_said_stopped_and_forgotten() {
    let (mut drones, window, _host) = service(Registry::new());
    drones.reconcile(&[http("m", 0, "t")]);
    wait_for(&window, "m", |s| {
        matches!(s, DroneModeState::Serving { .. })
    });
    drones.reconcile(&[]);
    wait_for(&window, "m", |s| *s == DroneModeState::Stopped);
    assert!(drones.statuses().is_empty());
    assert!(!drones.restart("m"));
}

struct Fake;
struct FakeRunning;

impl Running for FakeRunning {
    fn stop(self: Box<Self>) {}
}

impl DroneEndpoint for Fake {
    fn kind(&self) -> &str {
        "fake"
    }
    fn start(&self, launch: Launch<'_>) -> Result<Started, String> {
        let channel = launch
            .config
            .get("channel")
            .and_then(|value| value.as_str())
            .ok_or("no channel")?;
        Ok(Started {
            addr: channel.to_string(),
            connect_string: String::new(),
            running: Box::new(FakeRunning),
        })
    }
}

#[test]
fn an_edition_kind_starts_through_the_registry_and_an_unknown_one_fails() {
    let mut kinds = Registry::new();
    kinds.register(Box::new(Fake));
    let (mut drones, window, _host) = service(kinds);
    let ext = |id: &str, kind: &str| {
        let mut config = serde_json::Map::new();
        config.insert("channel".to_string(), serde_json::json!("KUBIQ"));
        DroneMode {
            id: id.to_string(),
            name: id.to_string(),
            enabled: true,
            kind: DroneModeKind::Ext {
                kind: kind.to_string(),
                config,
            },
        }
    };
    drones.reconcile(&[ext("a", "fake"), ext("b", "rdp")]);
    let a = wait_for(&window, "a", |s| {
        matches!(s, DroneModeState::Serving { .. })
    });
    assert_eq!(serving_addr(&a), "KUBIQ");
    wait_for(&window, "b", |s| matches!(s, DroneModeState::Failed { .. }));
}

#[test]
fn the_coordinator_mints_persists_and_announces_a_missing_token() {
    use kubiq_host::config::{ConfigRoot, RootSource};
    use kubiq_host::coordinator;
    use kubiq_host::projects::Projects;
    use kubiq_host::settings::Settings;
    use kubiq_host::store::SettingsStore as _;
    use kubiq_host::store::file::FileSettingsStore;
    use kubiq_host::store::memory::{MemoryPreferenceStore, MemoryProjectStore, MemoryTaskStore};
    use kubiq_host::work::Work;
    use kubiq_proto::settings::{HostSettings, SettingsLayer};

    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_path_buf();
    let store = FileSettingsStore::new(path.clone());
    let seeded = HostSettings {
        drone_modes: vec![http("m", 0, "")],
        ..HostSettings::default()
    };
    store
        .set(
            SettingsLayer::Host,
            &serde_json::to_string(&seeded).unwrap(),
        )
        .unwrap();

    let (hub, host) = bus::hub();
    let config = ConfigRoot {
        path: path.clone(),
        source: RootSource::Flag,
    };
    let (projects, pending) = Projects::open(
        path.clone(),
        Box::new(MemoryProjectStore::new()),
        Box::new(MemoryPreferenceStore::new()),
    );
    coordinator::start(
        host,
        config,
        projects,
        Work::open(Box::new(MemoryTaskStore::new())),
        Settings::open(Box::new(FileSettingsStore::new(path.clone()))),
        kubiq_host::tasksrc::Registry::with_defaults(),
        kubiq_host::runners::Registry::with_defaults(),
        pending,
        DroneModes::new(Some(hub.clone()), Registry::new()),
    );
    let window = hub.connect();

    let state = wait_for(&window, "m", |s| {
        matches!(s, DroneModeState::Serving { .. })
    });
    let DroneModeState::Serving { connect_string, .. } = state else {
        unreachable!()
    };
    let token = connect_string.split("?token=").nth(1).unwrap().to_string();
    assert!(token.len() >= 40, "{token}");

    let on_disk = store.get(SettingsLayer::Host).unwrap().unwrap();
    let on_disk: HostSettings = serde_json::from_str(&on_disk).unwrap();
    assert_eq!(on_disk.drone_modes, vec![http("m", 0, &token)]);
    drop(root);
}
