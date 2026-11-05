//! A project file opened as a SQLite database from the IDE explorer: which names qualify, the
//! header that proves it, and the steps from a verified file to its connection in the DB tree.
//!
//! **Pure.** The window reads the header through the file family (`ReadProjectFile` with
//! `max_bytes` at the header's length — the UI reads no disk) and drives [`DbState::sqlite_step`]
//! as the header and the connection list arrive.

use std::path::{Path, PathBuf};

use kubiq_proto::db::{ConnectionConfig, DbKind};
use kubiq_proto::ids::DbConnId;

use super::DbState;

/// The first 16 bytes of every SQLite 3 database file.
pub const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

/// Whether a file's name says SQLite: `.db`, or an extension starting `sqlite` (`.sqlite`,
/// `.sqlite3`, `.sqlitedb`), case-insensitive. A name only — the header decides.
pub fn is_sqlite_name(path: &str) -> bool {
    let leaf = path.rsplit('/').next().unwrap_or(path);
    let Some((stem, ext)) = leaf.rsplit_once('.') else {
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    !stem.is_empty() && (ext == "db" || ext.starts_with("sqlite"))
}

/// Whether a file's first bytes are the SQLite 3 header.
pub fn has_sqlite_header(bytes: &[u8]) -> bool {
    bytes.starts_with(SQLITE_HEADER)
}

/// A file on its way to the DB tree, from the gesture until its connection is revealed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqliteOpen {
    /// The project-relative path the explorer named.
    pub rel_path: String,
    /// The same file as the host lists a SQLite connection's path: absolute, under the project.
    pub abs_path: String,
    pub stage: SqliteStage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqliteStage {
    /// The header read is on the wire.
    Probing,
    /// The header is SQLite; waiting for the connection list.
    Verified,
    /// A new connection was saved; waiting for the list that holds it.
    Saving,
}

/// What the window does next for a [`SqliteOpen`].
#[derive(Clone, Debug, PartialEq)]
pub enum SqliteStep {
    /// Nothing yet: the header or the list is still on its way.
    Wait,
    /// The connection exists: reveal it.
    Reveal(DbConnId),
    /// No connection names the file: save this one.
    Create(Box<ConnectionConfig>),
    /// The list came back after the save without the file in it.
    Missing(String),
}

impl SqliteOpen {
    pub fn new(rel_path: String, abs_path: String) -> Self {
        Self {
            rel_path,
            abs_path,
            stage: SqliteStage::Probing,
        }
    }
}

impl DbState {
    /// The SQLite connection whose file is this one. The host lists a path inside the project as
    /// absolute (`store::absolutise`); a relative one is compared as written, for a project with
    /// no root.
    pub fn sqlite_connection(&self, rel_path: &str, abs_path: &str) -> Option<DbConnId> {
        self.connections
            .iter()
            .find(|conn| {
                conn.config.kind == DbKind::Sqlite
                    && conn.config.path.as_deref().is_some_and(|path| {
                        path == Path::new(abs_path) || path == Path::new(rel_path)
                    })
            })
            .map(|conn| conn.id)
    }

    /// The next step for the file being opened as SQLite, advancing its stage. Ends the open on
    /// `Reveal` and `Missing`.
    pub fn sqlite_step(&mut self) -> SqliteStep {
        let Some(open) = &self.sqlite_open else {
            return SqliteStep::Wait;
        };
        if open.stage == SqliteStage::Probing || !self.loaded {
            return SqliteStep::Wait;
        }
        if let Some(conn) = self.sqlite_connection(&open.rel_path, &open.abs_path) {
            self.sqlite_open = None;
            return SqliteStep::Reveal(conn);
        }
        match open.stage {
            SqliteStage::Saving => {
                let rel_path = open.rel_path.clone();
                self.sqlite_open = None;
                SqliteStep::Missing(rel_path)
            }
            _ => {
                let mut config = ConnectionConfig::new(DbKind::Sqlite);
                config.name = open
                    .rel_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&open.rel_path)
                    .to_string();
                config.path = Some(PathBuf::from(&open.abs_path));
                if let Some(open) = self.sqlite_open.as_mut() {
                    open.stage = SqliteStage::Saving;
                }
                SqliteStep::Create(Box::new(config))
            }
        }
    }
}
