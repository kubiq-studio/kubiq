//! A project's knowledge-base source list, as one TOML file.
//!
//! Modelled on [`crate::store::file::FileTaskStore`]: one file per project, under the project's own
//! directory in the config root, `version` at the top for a migration to read. Deliberately
//! stateless where that store is not — nothing here calls in from a settings form except through
//! [`super::Kb`], so there is no in-memory copy to keep consistent, only a file to read and a file
//! to write.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use kubiq_proto::ids::ProjectId;
use kubiq_proto::kb::{KbOrigin, KbSource};

use crate::atomic::write_atomic;
use crate::store::project_dir::{relative_in, resolve_in};

/// The format this Kubiq writes and understands.
///
/// `2`: the field beneath `version` was renamed from `roots` to `sources` alongside the rest of
/// this family's rename — a `1` file no longer parses, since `#[serde(deny_unknown_fields)]` is
/// not set but the field this reads is now a different name, so [`load`] falls back to an empty
/// list for it, the same as any other unreadable file. That costs a re-typed URL, on this
/// module's own rule, and nothing more.
pub const KB_VERSION: u32 = 2;

/// The whole file, mirroring [`crate::store::file::TasksFile`]: `version` at the top, the rows
/// beneath it.
#[derive(Debug, Default, Serialize, Deserialize)]
struct KbFile {
    version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sources: Vec<KbSource>,
}

/// `kb.toml` in the project's data directory — the user's own list of roots, on the shared side,
/// so it follows a project-managed project into `.kubiq/` (`D173`). The clones it names are
/// derived and live under `local/` instead.
pub fn path(config_root: &Path, project: ProjectId) -> PathBuf {
    crate::store::project_dir::ProjectData::resolved(config_root, project).kb_sources()
}

/// A project's sources. A missing file is a project whose knowledge base was never configured,
/// which is an empty list rather than an error — [`crate::store::file::FileTaskStore::load`]'s own
/// convention, for the same reason.
///
/// A folder source saved relative to the project (see [`save`]) is resolved against
/// `project_root` here, so every reader sees an absolute path. `None` leaves it as written.
pub fn load(path: &Path, project_root: Option<&Path>) -> Vec<KbSource> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return Vec::new(),
    };

    match toml::from_str::<KbFile>(&raw) {
        Ok(file) => {
            let mut sources = file.sources;
            if let Some(root) = project_root {
                for source in &mut sources {
                    if let KbOrigin::Folder { path } = &mut source.origin {
                        *path = resolve_in(root, path);
                    }
                }
            }
            sources
        }
        Err(error) => {
            // Unlike the catalogue, a source list costs the user a re-typed URL to recreate, not a
            // lost project — so it is discarded rather than preserved, on the view-state family's
            // reasoning, and the settings form simply reopens empty.
            tracing::warn!(
                "discarding an unreadable knowledge-base source list at {}: {error}",
                path.display()
            );
            Vec::new()
        }
    }
}

/// Write a project's whole source list, atomically. A folder source inside `project_root` is
/// written relative to it, so a clone of a project-managed project resolves it in its own
/// checkout; one outside stays absolute.
pub fn save(path: &Path, project_root: Option<&Path>, sources: &[KbSource]) -> std::io::Result<()> {
    let mut sources = sources.to_vec();
    if let Some(root) = project_root {
        for source in &mut sources {
            if let KbOrigin::Folder { path } = &mut source.origin {
                *path = relative_in(root, path);
            }
        }
    }
    let file = KbFile {
        version: KB_VERSION,
        sources,
    };
    // Every field here is `KbSource`, which is always representable in TOML — this only fails if
    // that ever stops being true, which a test would catch long before a user's sources did.
    let body = toml::to_string_pretty(&file).expect("a knowledge-base source list serialises");
    write_atomic(path, body.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubiq_proto::ids::KbSourceId;

    fn folder(path: &str) -> KbSource {
        KbSource {
            id: KbSourceId::generate(),
            name: "docs".to_string(),
            origin: KbOrigin::Folder {
                path: path.to_string(),
            },
            filter: String::new(),
            access: Default::default(),
            protected: false,
        }
    }

    /// A folder inside the project is relative on disk and absolute once read; one outside is
    /// absolute both ways.
    #[test]
    fn an_in_project_folder_is_stored_relative_and_read_back_absolute() {
        let dir = tempfile::tempdir().expect("a data directory");
        let file = dir.path().join("kb.toml");
        let root = Path::new("/work/demo");
        let sources = vec![folder("/work/demo/docs"), folder("/elsewhere/notes")];

        save(&file, Some(root), &sources).expect("written");
        let raw = std::fs::read_to_string(&file).expect("the file");
        assert!(raw.contains("path = \"docs\""), "{raw}");
        assert!(raw.contains("path = \"/elsewhere/notes\""), "{raw}");

        assert_eq!(load(&file, Some(root)), sources);
    }
}
