//! A project file opened as SQLite from the IDE explorer: the names that qualify, the header read
//! that proves it, the mode switched on, the connection made once and revealed.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use chrono::Utc;
use gpui::{AppContext as _, Entity, TestAppContext, WindowHandle};
use gpui_component::Root;
use kubiq::app::{AppState, BusHub};
use kubiq::state::db::open_file::{SQLITE_HEADER, has_sqlite_header, is_sqlite_name};
use kubiq::state::explorer::{ExplorerAction, menu_entries};
use kubiq::state::{FileDialog, RailMode, WindowRegistry};
use kubiq_proto::bus::{self, FromClient, To};
use kubiq_proto::db::{ConnectionConfig, DbConnection, DbKeystore, DbKind, PasswordState};
use kubiq_proto::files::FileContents;
use kubiq_proto::ids::{DbConnId, ProjectId};
use kubiq_proto::messages::Message;
use kubiq_proto::projects::{ProjectHealth, ProjectRecord, ProjectSnapshot};

const ROOT: &str = "/tmp/kubiq";
const FILE: &str = "data/app.db";

struct Fixture {
    state: Entity<AppState>,
    _window: WindowHandle<Root>,
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
        let fixture = Self {
            state,
            _window: window,
            host,
            project,
        };
        fixture.said();
        fixture
    }

    fn deliver(&self, message: Message, cx: &mut TestAppContext) {
        self.host.send(To::Everyone, message);
        cx.run_until_parked();
    }

    fn said(&self) -> Vec<Message> {
        let mut said = Vec::new();
        while let Ok(event) = self.host.recv_timeout(Duration::from_millis(300)) {
            if let FromClient::Said { message, .. } = event {
                said.push(message);
            }
        }
        said
    }

    fn header(&self, bytes: &[u8], len: u64, cx: &mut TestAppContext) {
        self.deliver(
            Message::ProjectFileContents {
                project_id: self.project,
                rel_path: FILE.to_string(),
                contents: FileContents {
                    bytes: bytes.to_vec(),
                    len,
                    truncated: len > bytes.len() as u64,
                    is_binary: false,
                    version: None,
                },
            },
            cx,
        );
    }

    fn list(&self, connections: Vec<DbConnection>, cx: &mut TestAppContext) {
        self.deliver(
            Message::DbConnectionsListed {
                project_id: self.project,
                connections,
                keystore: DbKeystore::Ready,
            },
            cx,
        );
    }
}

fn a_project() -> ProjectSnapshot {
    ProjectSnapshot {
        record: ProjectRecord {
            id: ProjectId::generate(),
            name: "kubiq".to_string(),
            path: ROOT.to_string(),
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

fn sqlite_conn(id: DbConnId, path: &str) -> DbConnection {
    let mut config = ConnectionConfig::new(DbKind::Sqlite);
    config.name = "app.db".to_string();
    config.path = Some(PathBuf::from(path));
    DbConnection {
        id,
        config,
        password: PasswordState::default(),
        agent: Default::default(),
        ssh: None,
    }
}

fn abs_file() -> String {
    std::path::Path::new(ROOT)
        .join(FILE)
        .to_string_lossy()
        .into_owned()
}

#[test]
fn sqlite_names_and_the_header() {
    for name in [
        "a.db",
        "x/b.DB",
        "c.sqlite",
        "d.sqlite3",
        "e.sqlitedb",
        "f.SQLite",
    ] {
        assert!(is_sqlite_name(name), "{name}");
    }
    for name in ["a.rs", "db", ".db", "x/db.txt", "a.dbx", "sqlite"] {
        assert!(!is_sqlite_name(name), "{name}");
    }
    assert!(has_sqlite_header(SQLITE_HEADER));
    assert!(!has_sqlite_header(b"SQLite format 2\0"));
    assert!(!has_sqlite_header(b"SQLite"));
}

#[test]
fn the_menu_offers_open_as_sqlite_only_for_a_sqlite_name() {
    let offers = |path: &str| {
        menu_entries(Some(path), false, true, false, false)
            .iter()
            .any(|entry| entry.action == ExplorerAction::OpenAsSqlite)
    };
    assert!(offers("data/app.sqlite3"));
    assert!(!offers("src/main.rs"));
    assert_eq!(ExplorerAction::OpenAsSqlite.label(), "Open as SQLite");
}

#[gpui::test]
fn a_double_click_reads_the_header_and_creates_then_reveals_the_connection(
    cx: &mut TestAppContext,
) {
    let fx = Fixture::open(cx);
    fx.state.update(cx, |state, cx| {
        assert!(!state.mode_enabled(RailMode::DB, cx), "DB is opt-in");
        state.double_click_explorer_row(FILE.to_string(), cx);
    });
    let said = fx.said();
    assert!(
        said.iter().any(|m| matches!(
            m,
            Message::ReadProjectFile { rel_path, max_bytes: Some(16), .. } if rel_path == FILE
        )),
        "the header is read, 16 bytes: {said:?}"
    );

    let mut header = SQLITE_HEADER.to_vec();
    header.truncate(16);
    fx.header(&header, 4096, cx);
    fx.state.update(cx, |state, cx| {
        assert!(
            state.mode_enabled(RailMode::DB, cx),
            "the mode is switched on"
        );
        assert_eq!(state.workbench.rail_mode, RailMode::DB);
        assert!(state.workbench.file_dialog.is_none());
    });
    fx.said();

    // No connection names the file: one is saved, named after it, with its absolute path.
    fx.list(Vec::new(), cx);
    let said = fx.said();
    let saved = said.iter().find_map(|m| match m {
        Message::SaveDbConnection {
            id: None, config, ..
        } => Some(config.clone()),
        _ => None,
    });
    let config = saved.expect("a connection is saved");
    assert_eq!(config.kind, DbKind::Sqlite);
    assert_eq!(config.name, "app.db");
    assert_eq!(config.path, Some(PathBuf::from(abs_file())));

    // The list holding it: it is selected and dialled.
    let conn = DbConnId::generate();
    fx.list(vec![sqlite_conn(conn, &abs_file())], cx);
    fx.state.update(cx, |state, cx| {
        let db = state.db(cx).expect("db");
        assert_eq!(db.tree.selected.as_deref(), Some(conn.to_string().as_str()));
        assert!(db.tree.root(conn).is_some_and(|root| root.expanded));
        assert!(db.sqlite_open.is_none());
    });
    assert!(
        fx.said()
            .iter()
            .any(|m| matches!(m, Message::DbTree { conn: c, .. } if *c == conn)),
        "the connection is dialled"
    );
}

#[gpui::test]
fn an_existing_connection_is_revealed_not_duplicated(cx: &mut TestAppContext) {
    let fx = Fixture::open(cx);
    let conn = DbConnId::generate();
    // Already listed, as the host lists a file inside the project: absolute.
    fx.list(vec![sqlite_conn(conn, &abs_file())], cx);
    fx.state.update(cx, |state, cx| {
        state.open_as_sqlite(FILE.to_string(), cx);
    });
    fx.said();
    fx.header(SQLITE_HEADER, 16, cx);
    let said = fx.said();
    assert!(
        !said
            .iter()
            .any(|m| matches!(m, Message::SaveDbConnection { .. })),
        "nothing is saved: {said:?}"
    );
    fx.state.update(cx, |state, cx| {
        assert_eq!(state.workbench.rail_mode, RailMode::DB);
        let db = state.db(cx).expect("db");
        assert_eq!(db.tree.selected.as_deref(), Some(conn.to_string().as_str()));
    });
}

#[gpui::test]
fn a_file_without_the_header_raises_the_error(cx: &mut TestAppContext) {
    let fx = Fixture::open(cx);
    fx.state.update(cx, |state, cx| {
        state.open_as_sqlite(FILE.to_string(), cx);
    });
    fx.said();
    fx.header(b"not a database!!", 300, cx);
    fx.state.update(cx, |state, cx| {
        match &state.workbench.file_dialog {
            Some(FileDialog::Failed { message, .. }) => assert_eq!(
                message,
                "Not a SQLite database: data/app.db has no SQLite header"
            ),
            other => panic!("no error dialog: {other:?}"),
        }
        assert_ne!(state.workbench.rail_mode, RailMode::DB);
        assert!(!state.mode_enabled(RailMode::DB, cx));
        assert!(state.db(cx).expect("db").sqlite_open.is_none());
    });
}

#[gpui::test]
fn a_double_click_on_another_file_still_opens_it(cx: &mut TestAppContext) {
    let fx = Fixture::open(cx);
    fx.state.update(cx, |state, cx| {
        state.double_click_explorer_row("src/main.rs".to_string(), cx);
    });
    let said = fx.said();
    assert!(
        said.iter().any(|m| matches!(
            m,
            Message::ReadProjectFile { rel_path, max_bytes: Some(n), .. }
                if rel_path == "src/main.rs" && *n > 16
        )),
        "{said:?}"
    );
    fx.state.update(cx, |state, cx| {
        assert!(state.db(cx).expect("db").sqlite_open.is_none());
    });
}
