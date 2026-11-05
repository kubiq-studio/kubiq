//! Which project-layer settings this machine trusts (`D230`).
//!
//! A project's own agent definitions and its catalog layer's MCP servers live in the project's
//! data directory — `.kubiq/` for a project-managed project, inside the folder an agent works in
//! and can write. A definition carries `extra_rw`, `mcps` and an instructions file, a catalog MCP
//! entry names a command to run; an agent that edits either widens its own next launch. So each
//! one is fingerprinted — SHA-256 over its parsed record serialized canonically, and for a
//! definition the content of its instructions file too — in a **per-machine** store under the
//! config root, `<config root>/projects/<id>/local/trust.toml`, never in `.kubiq/`, and a launch
//! only uses what matches.
//!
//! - **Kubiq's own writes record the fingerprint in the same operation**, of the record Kubiq
//!   serialized rather than of a re-read file: a definition saved, renamed or deleted through
//!   [`crate::agent::Agents`], an MCP entry saved or removed through [`crate::catalog::Catalog`]. A
//!   clone is trusted only when its source is; repointing an `extends` keeps the record's standing.
//!   Moves that do not change content — the storage-mode change, the startup layout migration —
//!   need nothing: the store is keyed by id and sits on the config-root side those moves never
//!   touch.
//! - **At launch** every definition and catalog entry the run can reach is read **once** into a
//!   [`SnapshotProfiles`] / [`SnapshotRegistry`]; the check runs on that snapshot and the run is
//!   composed from it, so a file swapped after the check is never used. A project definition
//!   anywhere in the run's `extends` chain that does not match refuses the run with [`Refused`]; a
//!   project catalog MCP that does not match is left out of the snapshot and reported.
//! - **The user approves** what they were shown: [`approve`] trusts an item only if its content
//!   still has the fingerprint the prompt carried.
//! - **Upgrade:** the first start after the upgrade seeds every project's missing store from what
//!   is on disk and writes a root-level marker ([`seed_on_upgrade`], logged). From then on a
//!   missing store is an empty one — nothing trusted — whatever the reason it is missing.
//!
//! The global (config-root) layer is not checked: it is outside every project folder. Skills are
//! not covered.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use agent_manager::profile::{Profile, ProfileStore};
use agent_manager::registry::{McpEntry, Registry, SkillEntry};
use agent_manager::source::Source;
use kubiq_proto::ids::ProjectId;
use kubiq_proto::messages::AgentDefinition;
use kubiq_proto::projects::{TrustItem, TrustKind, TrustReason, UntrustedItem};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::atomic::write_atomic;
use crate::store::project_dir::ProjectData;

/// The store's file name, under the project's config-root `local/`.
const FILE: &str = "trust.toml";

/// The root-level file that says the upgrade seeding has happened. Once it exists, a project
/// with no store trusts nothing.
pub const SEEDED_MARKER: &str = "trust-seeded";

const VERSION: u32 = 1;

/// Every read-modify-write of a store holds this: the coordinator saves definitions on its own
/// thread, catalog edits can run on another.
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct TrustFile {
    #[serde(default)]
    version: u32,
    /// Definition id → fingerprint.
    #[serde(default)]
    definitions: BTreeMap<String, String>,
    /// Catalog MCP id → fingerprint.
    #[serde(default)]
    mcps: BTreeMap<String, String>,
}

impl TrustFile {
    fn map(&mut self, kind: TrustKind) -> &mut BTreeMap<String, String> {
        match kind {
            TrustKind::Definition => &mut self.definitions,
            TrustKind::Mcp => &mut self.mcps,
        }
    }

    fn get(&self, kind: TrustKind, id: &str) -> Option<&String> {
        match kind {
            TrustKind::Definition => self.definitions.get(id),
            TrustKind::Mcp => self.mcps.get(id),
        }
    }
}

/// Where a project's trust store is: under the config root whatever the storage mode.
pub fn path(root: &Path, project: ProjectId) -> PathBuf {
    ProjectData::under_config(root, project)
        .local()
        .join(FILE)
}

// ── fingerprints ────────────────────────────────────────────────────────────────────────────

fn hex(hasher: Sha256) -> String {
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A definition's fingerprint: the whole record (`extra_rw`, `mcps`, the instructions path …) and,
/// when it names an instructions file, that file's content as given — `None` for one that could
/// not be read, which is a state of its own.
///
/// Struct fields serialize in declaration order and every map in these records is a `BTreeMap`,
/// so the bytes are canonical for a given record.
pub fn definition_fingerprint(record: &Profile, instructions: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(record).unwrap_or_default());
    if record.defaults.instructions.is_some() {
        match instructions {
            Some(text) => {
                hasher.update(b"\0instructions\0");
                hasher.update(text.as_bytes());
            }
            None => hasher.update(b"\0instructions-unreadable\0"),
        }
    }
    hex(hasher)
}

/// A catalog MCP entry's fingerprint — command, args, env, url, headers and how it is exposed.
pub fn mcp_fingerprint(entry: &McpEntry) -> String {
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(entry).unwrap_or_default());
    hex(hasher)
}

/// The instructions file a record names, read now. `None` when it names none or it cannot be read.
pub fn read_instructions(record: &Profile) -> Option<String> {
    let path = record.defaults.instructions.as_ref()?;
    std::fs::read_to_string(path).ok()
}

/// [`definition_fingerprint`] with the instructions read now.
fn definition_print(record: &Profile) -> String {
    definition_fingerprint(record, read_instructions(record).as_deref())
}

// ── reading the project layer, one record at a time ────────────────────────────────────────

/// A record as the library's loader leaves it: the directory name for an empty id, the legacy
/// role keys folded into `tags` (`agent_manager::profile::Profile::fold_legacy_roles`).
fn normalized(mut record: Profile, name: &str) -> Profile {
    if record.id.is_empty() {
        record.id = name.to_string();
    }
    let legacy = [
        (record.legacy_coordinator.take(), "coordinator"),
        (record.legacy_worker.take(), "worker"),
    ];
    if record.tags.is_empty() {
        for (flag, tag) in legacy {
            if flag == Some(true) {
                record.tags.push(tag.to_string());
            }
        }
    }
    record
}

/// One definition, `<dir>/<id>/profile.toml`, read alone — a malformed sibling cannot hide it.
pub fn read_definition(dir: &Path, id: &str) -> Option<Profile> {
    let text = std::fs::read_to_string(dir.join(id).join("profile.toml")).ok()?;
    let record = toml::from_str::<Profile>(&text).ok()?;
    Some(normalized(record, id))
}

/// Every definition under `dir`, each read alone. A record that does not parse, or whose id is not
/// its directory's name (what Kubiq always writes), is left out and logged: two directories
/// claiming one id must not let one stand in for the other.
pub fn read_definitions(dir: &Path) -> Vec<Profile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut records = Vec::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !entry.path().join("profile.toml").is_file() {
            continue;
        }
        match read_definition(dir, &name) {
            Some(record) if record.id == name => records.push(record),
            Some(record) => tracing::warn!(
                "{}: the record says '{}', not its directory's name, so it is ignored",
                entry.path().display(),
                record.id
            ),
            None => tracing::warn!("{}: the definition does not parse", entry.path().display()),
        }
    }
    records.sort_by(|a, b| a.id.cmp(&b.id));
    records
}

/// The record as reading back what Kubiq just serialized would give it — a round trip through
/// TOML in memory, not a re-read of the file.
pub fn definition_as_written(record: &Profile) -> Option<Profile> {
    let text = toml::to_string_pretty(record).ok()?;
    let back = toml::from_str::<Profile>(&text).ok()?;
    Some(normalized(back, &record.id))
}

/// The entry as reading back what Kubiq just serialized would give it — a JSON round trip in
/// memory, with the id the file name would give it.
pub fn mcp_as_written(entry: &McpEntry) -> Option<McpEntry> {
    let text = serde_json::to_string(entry).ok()?;
    let mut back = serde_json::from_str::<McpEntry>(&text).ok()?;
    back.def.id = back.id.clone();
    Some(back)
}

fn definitions_dir(root: &Path, project: ProjectId) -> PathBuf {
    ProjectData::resolved(root, project).definitions()
}

fn project_mcps(root: &Path, project: ProjectId) -> Vec<McpEntry> {
    crate::catalog::Catalog::new(root)
        .layer(Some(project))
        .mcps()
        .unwrap_or_else(|error| {
            tracing::warn!("trust: the catalog of project {project} could not be read: {error:#}");
            Vec::new()
        })
}

// ── the store ──────────────────────────────────────────────────────────────────────────────

/// The store as it is on disk. Absent or unreadable is an empty store — nothing trusted — so a
/// failure never widens anything.
fn load(root: &Path, project: ProjectId) -> TrustFile {
    let path = path(root, project);
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|error| {
            tracing::warn!("trust: {} is unreadable, trusting nothing: {error}", path.display());
            TrustFile::default()
        }),
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("trust: {} could not be read: {error}", path.display());
            }
            TrustFile::default()
        }
    }
}

fn store(root: &Path, project: ProjectId, file: &TrustFile) -> std::io::Result<()> {
    let path = path(root, project);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = file.clone();
    file.version = VERSION;
    let text = toml::to_string(&file).map_err(std::io::Error::other)?;
    write_atomic(&path, text.as_bytes())
}

fn edit(root: &Path, project: ProjectId, change: impl FnOnce(&mut TrustFile)) {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut file = load(root, project);
    change(&mut file);
    if let Err(error) = store(root, project, &file) {
        tracing::warn!("trust: the store of project {project} could not be written: {error}");
    }
}

/// The upgrade into `D230`, run once at startup over every project in the catalogue: a project
/// with no store trusts its settings as they stand, then the root-level [`SEEDED_MARKER`] is
/// written and this never seeds again. A project whose data directory is not there right now is
/// not seeded — it starts trusting nothing, like any project after the marker.
pub fn seed_on_upgrade(root: &Path, projects: impl IntoIterator<Item = ProjectId>) {
    let marker = root.join(SEEDED_MARKER);
    if marker.exists() {
        return;
    }
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for project in projects {
        if path(root, project).exists() || !ProjectData::resolved(root, project).dir().is_dir() {
            continue;
        }
        let mut file = TrustFile::default();
        for record in read_definitions(&definitions_dir(root, project)) {
            file.definitions
                .insert(record.id.clone(), definition_print(&record));
        }
        for entry in project_mcps(root, project) {
            file.mcps.insert(entry.id.clone(), mcp_fingerprint(&entry));
        }
        match store(root, project, &file) {
            Ok(()) => tracing::info!(
                %project,
                definitions = file.definitions.len(),
                mcps = file.mcps.len(),
                "trust: upgrade — this project's settings are trusted as they stand (first use)"
            ),
            Err(error) => tracing::warn!("trust: seeding project {project} failed: {error}"),
        }
    }
    if let Err(error) = write_atomic(&marker, b"1\n") {
        tracing::warn!("trust: {} could not be written: {error}", marker.display());
    }
}

/// A development setup can put the config root inside a project's folder, where an agent working
/// in that project can write the trust store itself. Said in the log, nothing more.
pub fn warn_if_root_inside(root: &Path, project_path: &Path) {
    let (Ok(root), Ok(project_path)) = (root.canonicalize(), project_path.canonicalize()) else {
        return;
    };
    if root.starts_with(&project_path) {
        tracing::warn!(
            "the config root {} is inside the project folder {}: an agent working there can \
             write the trust store (D230)",
            root.display(),
            project_path.display()
        );
    }
}

/// Record one item as Kubiq just wrote it: `Some` fingerprint trusts it, `None` forgets it.
pub fn record(root: &Path, project: ProjectId, kind: TrustKind, id: &str, value: Option<String>) {
    edit(root, project, |file| match value {
        Some(value) => {
            file.map(kind).insert(id.to_string(), value);
        }
        None => {
            file.map(kind).remove(id);
        }
    });
}

/// Trust the definition Kubiq just serialized: the in-memory record round-tripped, plus its
/// instructions file as it reads now.
pub fn record_definition(root: &Path, project: ProjectId, written: &Profile) {
    let value = definition_as_written(written).map(|it| definition_print(&it));
    record(root, project, TrustKind::Definition, &written.id, value);
}

/// Trust the catalog MCP entry Kubiq just serialized.
pub fn record_mcp(root: &Path, project: ProjectId, entry: &McpEntry) {
    let value = mcp_as_written(entry).map(|it| mcp_fingerprint(&it));
    record(root, project, TrustKind::Mcp, &entry.id, value);
}

/// An item moved from `from` to `to` unchanged: its standing goes with it, trusted or not.
pub fn rename(root: &Path, project: ProjectId, kind: TrustKind, from: &str, to: &str) {
    edit(root, project, |file| {
        let map = file.map(kind);
        match map.remove(from) {
            Some(value) => {
                map.insert(to.to_string(), value);
            }
            None => {
                map.remove(to);
            }
        }
    });
}

/// What one project's store says, read once — a launch asks it several questions.
pub struct Trusted(TrustFile);

impl Trusted {
    pub fn of(root: &Path, project: ProjectId) -> Self {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        Self(load(root, project))
    }

    /// `None` when `fingerprint` is what this machine trusts for that item.
    pub fn reason(&self, kind: TrustKind, id: &str, fingerprint: &str) -> Option<TrustReason> {
        match self.0.get(kind, id) {
            Some(trusted) if trusted == fingerprint => None,
            Some(_) => Some(TrustReason::Changed),
            None => Some(TrustReason::New),
        }
    }

    /// The item, if it is not trusted as `fingerprint`.
    pub fn check(&self, kind: TrustKind, id: &str, fingerprint: String) -> Option<UntrustedItem> {
        self.reason(kind, id, &fingerprint)
            .map(|reason| UntrustedItem {
                kind,
                id: id.to_string(),
                reason,
                fingerprint,
            })
    }
}

/// Whether a project definition, as given, is trusted here.
pub fn definition_trusted(root: &Path, project: ProjectId, record: &Profile) -> bool {
    Trusted::of(root, project)
        .reason(TrustKind::Definition, &record.id, &definition_print(record))
        .is_none()
}

/// A project's harness agents (`D233`) under its folder `local`, as the definition records a
/// launch adds to the project's snapshot: the ones no stored definition of `stored` already
/// names, since the stored one is what that id means. Empty when the project lists none.
pub fn local_records(local: Option<&Path>, stored: &[Profile]) -> Vec<Profile> {
    let Some(folder) = local else {
        return Vec::new();
    };
    crate::harness_agents::records(folder)
        .into_iter()
        .filter(|it| !stored.iter().any(|own| own.id == it.id))
        .collect()
}

/// Trust the harness agent Kubiq just wrote from `definition` (`D233`): the record it serialized,
/// not a re-read of the files.
pub fn record_local(root: &Path, project: ProjectId, definition: &AgentDefinition) {
    let written = crate::harness_agents::written_record(definition);
    let value = Some(definition_fingerprint(&written, None));
    record(root, project, TrustKind::Definition, &written.id, value);
}

/// Trust every harness agent under `local` as the files hold it now: what a project that syncs
/// its agents (`D233`) says about them, so a file edited outside Kubiq, or one a sync wrote, is
/// not asked about. Stored definitions are untouched.
pub fn trust_local(root: &Path, project: ProjectId, local: &Path) {
    let records = local_records(Some(local), &read_definitions(&definitions_dir(root, project)));
    if records.is_empty() {
        return;
    }
    let trusted = Trusted::of(root, project);
    let changed: Vec<(String, String)> = records
        .iter()
        .filter_map(|record| {
            let print = definition_print(record);
            trusted
                .reason(TrustKind::Definition, &record.id, &print)
                .map(|_| (record.id.clone(), print))
        })
        .collect();
    if changed.is_empty() {
        return;
    }
    edit(root, project, |file| {
        for (id, print) in changed {
            file.map(TrustKind::Definition).insert(id, print);
        }
    });
}

/// Every project-layer setting of `project` this machine does not trust, definitions first — the
/// project's whole list, which is what the window is always sent. `local` is the project's folder
/// when it lists its harness agents (`D233`): they are definitions it runs, so they are checked
/// like its stored ones.
pub fn untrusted(root: &Path, project: ProjectId, local: Option<&Path>) -> Vec<UntrustedItem> {
    let trusted = Trusted::of(root, project);
    let stored = read_definitions(&definitions_dir(root, project));
    let local = local_records(local, &stored);
    let mut items: Vec<UntrustedItem> = stored
        .iter()
        .chain(&local)
        .filter_map(|record| {
            trusted.check(TrustKind::Definition, &record.id, definition_print(record))
        })
        .collect();
    items.extend(
        project_mcps(root, project)
            .iter()
            .filter_map(|entry| trusted.check(TrustKind::Mcp, &entry.id, mcp_fingerprint(entry))),
    );
    items
}

/// The user approved `items` as they were shown. Each is trusted only if its content on disk still
/// has the fingerprint the prompt carried; one edited since stays untrusted and is reported again.
pub fn approve(root: &Path, project: ProjectId, items: &[TrustItem], local: Option<&Path>) {
    let dir = definitions_dir(root, project);
    let mcps = project_mcps(root, project);
    let local = local_records(local, &read_definitions(&dir));
    edit(root, project, |file| {
        for item in items {
            let current = match item.kind {
                TrustKind::Definition => read_definition(&dir, &item.id)
                    .filter(|record| record.id == item.id)
                    .or_else(|| local.iter().find(|it| it.id == item.id).cloned())
                    .map(|record| definition_print(&record)),
                TrustKind::Mcp => mcps
                    .iter()
                    .find(|it| it.id == item.id)
                    .map(mcp_fingerprint),
            };
            if current.as_deref() == Some(item.fingerprint.as_str()) {
                file.map(item.kind)
                    .insert(item.id.clone(), item.fingerprint.clone());
            }
        }
    });
}

/// A launch refused because a project definition it would run is not trusted here. Carried as
/// the launch's `anyhow` error, so the coordinator can tell it apart and send the project's
/// [`kubiq_proto::messages::Message::UntrustedProjectSettings`].
#[derive(Debug)]
pub struct Refused {
    pub project: ProjectId,
    pub items: Vec<UntrustedItem>,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<String> = self.items.iter().map(|it| format!("'{}'", it.id)).collect();
        write!(
            f,
            "the project's agent definition {} was changed outside Kubiq, or is new on this \
             machine, so it is not trusted here — review and approve it in the project's settings \
             before running it",
            names.join(", ")
        )
    }
}

impl std::error::Error for Refused {}

// ── the launch snapshot ─────────────────────────────────────────────────────────────────────

/// Definitions read once, answered from memory. The config-overlay bases (a captured login) are
/// not definition content and are still answered by the real store.
pub struct SnapshotProfiles<'a> {
    records: Vec<Profile>,
    real: &'a dyn ProfileStore,
}

impl<'a> SnapshotProfiles<'a> {
    pub fn new(records: Vec<Profile>, real: &'a dyn ProfileStore) -> Self {
        Self { records, real }
    }

    pub fn records(&self) -> &[Profile] {
        &self.records
    }
}

impl ProfileStore for SnapshotProfiles<'_> {
    fn profiles(&self) -> anyhow::Result<Vec<Profile>> {
        Ok(self.records.clone())
    }

    fn profile(&self, id: &str) -> anyhow::Result<Option<Profile>> {
        Ok(self.records.iter().find(|it| it.id == id).cloned())
    }

    fn base_source(&self, id: &str, harness: &str) -> Option<Source> {
        self.real.base_source(id, harness)
    }

    fn put_base(&self, id: &str, harness: &str, from: &Path) -> anyhow::Result<()> {
        self.real.put_base(id, harness, from)
    }
}

/// One catalog layer read once, answered from memory.
pub struct SnapshotRegistry {
    pub skills: Vec<SkillEntry>,
    pub mcps: Vec<McpEntry>,
}

impl SnapshotRegistry {
    pub fn of(layer: &dyn Registry) -> anyhow::Result<Self> {
        Ok(Self {
            skills: layer.skills()?,
            mcps: layer.mcps()?,
        })
    }

    pub fn empty() -> Self {
        Self {
            skills: Vec::new(),
            mcps: Vec::new(),
        }
    }
}

impl Registry for SnapshotRegistry {
    fn skills(&self) -> anyhow::Result<Vec<SkillEntry>> {
        Ok(self.skills.clone())
    }

    fn mcps(&self) -> anyhow::Result<Vec<McpEntry>> {
        Ok(self.mcps.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::agent::{Agents, ConverseOptions};
    use kubiq_proto::catalog::{CatalogMcp, McpKind};
    use kubiq_proto::ids::PaneId;

    fn entry(id: &str, command: &str) -> McpEntry {
        McpEntry {
            id: id.to_string(),
            def: agent_manager::config::McpServer {
                id: id.to_string(),
                transport: agent_manager::config::McpTransport::Stdio,
                command: Some(command.to_string()),
                args: Vec::new(),
                env: BTreeMap::new(),
                url: None,
                headers: BTreeMap::new(),
            },
            expose: agent_manager::registry::McpExpose::Tools,
            summary: None,
            description: None,
        }
    }

    fn agents(root: &Path) -> Agents {
        let mut agents = Agents::new(root, false);
        agents.set_commands(BTreeMap::from([(
            "claude-code".to_string(),
            "echo".to_string(),
        )]));
        agents
    }

    fn definition(id: &str, project: Option<ProjectId>) -> AgentDefinition {
        AgentDefinition {
            id: id.to_string(),
            description: None,
            agent_type: "claude-code".to_string(),
            account: None,
            model: None,
            mode: None,
            thinking: None,
            max_subagents: None,
            prompt: None,
            mcps: Vec::new(),
            skills: Vec::new(),
            mission_assistant: None,
            tags: Vec::new(),
            disabled: false,
            project,
            grants: Vec::new(),
            local: None,
        }
    }

    fn run(
        agents: &Agents,
        cwd: &Path,
        project: ProjectId,
        definition: Option<&str>,
        mcps: &[&str],
    ) -> anyhow::Result<crate::agent::Composed> {
        agents.compose(
            PaneId::generate(),
            "claude-code",
            cwd,
            Vec::new(),
            ConverseOptions {
                definition: definition.map(str::to_string),
                project: Some(project),
                mcps: mcps.iter().map(|it| it.to_string()).collect(),
                ..Default::default()
            },
        )
    }

    fn refused_items(result: anyhow::Result<crate::agent::Composed>) -> Vec<(String, TrustReason)> {
        match result {
            Ok(_) => panic!("the run should have been refused"),
            Err(error) => error
                .downcast_ref::<Refused>()
                .expect("refused by the trust check")
                .items
                .iter()
                .map(|it| (it.id.clone(), it.reason))
                .collect(),
        }
    }

    fn listed(root: &Path, project: ProjectId) -> Vec<(TrustKind, String, TrustReason)> {
        untrusted(root, project, None)
            .into_iter()
            .map(|it| (it.kind, it.id, it.reason))
            .collect()
    }

    /// What the prompt would send back for every item it lists.
    fn as_shown(root: &Path, project: ProjectId) -> Vec<TrustItem> {
        untrusted(root, project, None)
            .into_iter()
            .map(|it| TrustItem {
                kind: it.kind,
                id: it.id,
                fingerprint: it.fingerprint,
            })
            .collect()
    }

    fn record_file(root: &Path, project: ProjectId, id: &str) -> PathBuf {
        definitions_dir(root, project)
            .join(id)
            .join("profile.toml")
    }

    fn prepend(file: &Path, line: &str) {
        let text = std::fs::read_to_string(file).unwrap();
        std::fs::write(file, format!("{line}\n{text}")).unwrap();
    }

    #[test]
    fn a_fingerprint_follows_the_content() {
        let a = entry("x", "run");
        let mut b = entry("x", "run");
        assert_eq!(mcp_fingerprint(&a), mcp_fingerprint(&b));
        b.def.command = Some("other".into());
        assert_ne!(mcp_fingerprint(&a), mcp_fingerprint(&b));
    }

    /// Kubiq's own save trusts; an edit outside Kubiq refuses the run; the user's approval of
    /// what they were shown trusts it again.
    #[test]
    fn a_saved_definition_runs_an_edited_one_is_refused_and_approval_restores_it() {
        let root = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let agents = agents(root.path());
        let project = ProjectId::generate();

        agents
            .save_definition(definition("reviewer", Some(project)))
            .unwrap();
        assert!(untrusted(root.path(), project, None).is_empty());
        run(&agents, cwd.path(), project, Some("reviewer"), &[]).expect("a trusted run");

        // An agent widens its own grant behind Kubiq's back.
        prepend(&record_file(root.path(), project, "reviewer"), "extra_rw = [\"/\"]");
        assert_eq!(
            refused_items(run(&agents, cwd.path(), project, Some("reviewer"), &[])),
            vec![("reviewer".to_string(), TrustReason::Changed)]
        );
        assert_eq!(
            listed(root.path(), project),
            vec![(TrustKind::Definition, "reviewer".into(), TrustReason::Changed)]
        );

        approve(root.path(), project, &as_shown(root.path(), project), None);
        assert!(untrusted(root.path(), project, None).is_empty());
        run(&agents, cwd.path(), project, Some("reviewer"), &[]).expect("approved");
    }

    /// Approval is of what was shown: content edited after the prompt was drawn stays untrusted.
    #[test]
    fn an_approval_of_stale_content_trusts_nothing() {
        let root = tempfile::tempdir().unwrap();
        let agents = agents(root.path());
        let project = ProjectId::generate();
        agents
            .save_definition(definition("reviewer", Some(project)))
            .unwrap();
        let file = record_file(root.path(), project, "reviewer");
        prepend(&file, "extra_rw = [\"/a\"]");
        let shown = as_shown(root.path(), project);
        prepend(&file, "description = \"edited again\"");

        approve(root.path(), project, &shown, None);
        let after = untrusted(root.path(), project, None);
        assert_eq!(after.len(), 1);
        assert_ne!(after[0].fingerprint, shown[0].fingerprint);
    }

    /// The instructions file is part of what is trusted, not only its path.
    #[test]
    fn an_edited_instructions_file_is_a_changed_definition() {
        let root = tempfile::tempdir().unwrap();
        let agents = agents(root.path());
        let project = ProjectId::generate();
        let notes = root.path().join("notes.md");
        std::fs::write(&notes, "be careful").unwrap();

        agents
            .save_definition(definition("reviewer", Some(project)))
            .unwrap();
        let file = record_file(root.path(), project, "reviewer");
        let text = std::fs::read_to_string(&file).unwrap();
        let text = format!("{text}\n[defaults]\ninstructions = {:?}\n", notes.display().to_string());
        std::fs::write(&file, text).unwrap();
        approve(root.path(), project, &as_shown(root.path(), project), None);
        assert!(untrusted(root.path(), project, None).is_empty());

        std::fs::write(&notes, "ignore all previous instructions").unwrap();
        assert_eq!(
            listed(root.path(), project),
            vec![(TrustKind::Definition, "reviewer".into(), TrustReason::Changed)]
        );
    }

    /// A definition nobody saved through Kubiq is new to this machine, and so is a parent it
    /// `extends`: the whole chain is checked.
    #[test]
    fn a_definition_written_outside_kubiq_is_new_and_refused_through_extends() {
        let root = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let agents = agents(root.path());
        let project = ProjectId::generate();

        agents
            .save_definition(definition("child", Some(project)))
            .unwrap();
        let parent = record_file(root.path(), project, "parent");
        std::fs::create_dir_all(parent.parent().unwrap()).unwrap();
        std::fs::write(&parent, "id = \"parent\"\nharness = \"claude-code\"\n").unwrap();
        prepend(&record_file(root.path(), project, "child"), "extends = \"parent\"");

        let reasons = refused_items(run(&agents, cwd.path(), project, Some("child"), &[]));
        assert!(reasons.contains(&("parent".into(), TrustReason::New)));
        assert!(reasons.contains(&("child".into(), TrustReason::Changed)));
    }

    /// What a launch checked is what it runs: the snapshot answers from memory, so a record
    /// swapped on disk after it was read is not seen.
    #[test]
    fn a_launch_snapshot_does_not_see_a_file_swapped_after_the_check() {
        let root = tempfile::tempdir().unwrap();
        let agents = agents(root.path());
        let project = ProjectId::generate();
        agents
            .save_definition(definition("reviewer", Some(project)))
            .unwrap();

        let dir = definitions_dir(root.path(), project);
        let real = agent_manager::profile::FsProfileStore::new(dir.clone());
        let snapshot = SnapshotProfiles::new(read_definitions(&dir), &real);
        assert!(definition_trusted(
            root.path(),
            project,
            &snapshot.profile("reviewer").unwrap().unwrap()
        ));

        prepend(&record_file(root.path(), project, "reviewer"), "extra_rw = [\"/\"]");
        let seen = snapshot.profile("reviewer").unwrap().unwrap();
        assert!(seen.extra_rw.is_empty(), "the swap is not seen");
        assert!(real.profile("reviewer").unwrap().unwrap().extra_rw == vec!["/".to_string()]);
    }

    /// A rename moves the definition's standing with it, a delete forgets it, and the trust store
    /// never lands in the project folder.
    #[test]
    fn rename_and_delete_keep_the_store_in_step() {
        let root = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let agents = agents(root.path());
        let project = ProjectId::generate();

        agents
            .save_definition(definition("old", Some(project)))
            .unwrap();
        agents
            .save_definition_replacing(definition("new", Some(project)), Some("old"), None)
            .unwrap();
        assert!(untrusted(root.path(), project, None).is_empty());
        run(&agents, cwd.path(), project, Some("new"), &[]).expect("renamed, still trusted");
        let store = std::fs::read_to_string(path(root.path(), project)).unwrap();
        assert!(store.contains("new") && !store.contains("old"));

        agents.delete_definition("new", Some(project)).unwrap();
        let store = std::fs::read_to_string(path(root.path(), project)).unwrap();
        assert!(!store.contains("new"), "a deleted definition is forgotten");
        assert!(path(root.path(), project).starts_with(root.path().join("projects")));
    }

    /// A clone is trusted only when its source is: copying an edited record must not launder it.
    #[test]
    fn a_clone_of_an_untrusted_definition_is_untrusted() {
        let root = tempfile::tempdir().unwrap();
        let agents = agents(root.path());
        let project = ProjectId::generate();
        agents
            .save_definition(definition("good", Some(project)))
            .unwrap();
        agents.clone_definition("good", "good2", Some(project)).unwrap();
        assert!(untrusted(root.path(), project, None).is_empty());

        prepend(&record_file(root.path(), project, "good"), "extra_rw = [\"/\"]");
        agents.clone_definition("good", "copy", Some(project)).unwrap();
        let ids: Vec<String> = listed(root.path(), project)
            .into_iter()
            .map(|(_, id, _)| id)
            .collect();
        assert_eq!(ids, vec!["copy".to_string(), "good".to_string()]);

        // A global source is trusted, so its clone into the project is too.
        agents.save_definition(definition("global", None)).unwrap();
        agents.clone_definition("global", "local", Some(project)).unwrap();
        assert!(!listed(root.path(), project).iter().any(|(_, id, _)| id == "local"));
    }

    /// A malformed sibling does not make a save forget what it just wrote.
    #[test]
    fn a_malformed_sibling_does_not_untrust_a_save() {
        let root = tempfile::tempdir().unwrap();
        let agents = agents(root.path());
        let project = ProjectId::generate();
        let broken = record_file(root.path(), project, "broken");
        std::fs::create_dir_all(broken.parent().unwrap()).unwrap();
        std::fs::write(&broken, "this is = not [toml").unwrap();

        agents
            .save_definition(definition("fine", Some(project)))
            .unwrap();
        let store = std::fs::read_to_string(path(root.path(), project)).unwrap();
        assert!(store.contains("fine"));
        assert!(untrusted(root.path(), project, None).is_empty());
    }

    /// The storage-mode move takes the definitions into `.kubiq/` unchanged and leaves the store
    /// where it is, so nothing has to be approved again.
    #[test]
    fn moving_a_project_into_its_folder_keeps_its_definitions_trusted() {
        use crate::store::project_dir::change_mode;
        use kubiq_proto::projects::{ProjectRecord, StorageMode};

        let root = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let agents = agents(root.path());
        let project = ProjectId::generate();
        agents
            .save_definition(definition("mine", Some(project)))
            .unwrap();

        let record = ProjectRecord {
            id: project,
            name: "demo".into(),
            path: folder.path().to_string_lossy().into_owned(),
            colour: 3,
            custom_colour: None,
            temporary: false,
            created_at: chrono::Utc::now(),
            last_opened_at: None,
            search_excludes: vec![],
            index: None,
            mission_term: None,
            managed_repos: vec![],
            tools: vec![],
            lanes: vec![],
            runs_on: None,
            initials: String::new(),
            definitions_use_global: true,
            definitions_allowed: Vec::new(),
            harness_agents: Default::default(),
            storage: StorageMode::KubiqManaged,
        };
        change_mode(root.path(), &record, StorageMode::ProjectManaged).unwrap();
        assert!(record_file(root.path(), project, "mine").starts_with(folder.path()));
        assert!(untrusted(root.path(), project, None).is_empty());
        run(&agents, folder.path(), project, Some("mine"), &[]).expect("still trusted");
    }

    /// Upgrade: the first start seeds every project with no store and writes the marker; after
    /// it, a missing store — a project added later, a deleted file — trusts nothing.
    #[test]
    fn the_upgrade_seeds_once_and_a_missing_store_afterwards_trusts_nothing() {
        let root = tempfile::tempdir().unwrap();
        let before = ProjectId::generate();
        let after = ProjectId::generate();
        for project in [before, after] {
            let file = record_file(root.path(), project, "cloned");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "id = \"cloned\"\nharness = \"claude-code\"\n").unwrap();
        }

        seed_on_upgrade(root.path(), [before]);
        assert!(root.path().join(SEEDED_MARKER).is_file());
        assert!(untrusted(root.path(), before, None).is_empty());

        seed_on_upgrade(root.path(), [before, after]);
        assert_eq!(
            listed(root.path(), after),
            vec![(TrustKind::Definition, "cloned".into(), TrustReason::New)]
        );
        std::fs::remove_file(path(root.path(), before)).unwrap();
        seed_on_upgrade(root.path(), [before]);
        assert_eq!(listed(root.path(), before).len(), 1, "a deleted store is not re-seeded");
    }

    /// A catalog MCP saved through Kubiq is given to the run; edited outside, it is omitted and
    /// reported rather than refusing the run — and not reported as a stale reference.
    #[test]
    fn an_edited_catalog_mcp_is_omitted_from_the_run_and_reported() {
        let root = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let agents = agents(root.path());
        let project = ProjectId::generate();
        let catalog = crate::catalog::Catalog::new(root.path());
        catalog
            .save_mcp(
                Some(project),
                &CatalogMcp {
                    id: "things".into(),
                    kind: McpKind::Stdio,
                    command: Some("npx".into()),
                    args: vec!["thing".into()],
                    env: BTreeMap::new(),
                    url: None,
                    headers: BTreeMap::new(),
                    description: None,
                },
                None,
            )
            .unwrap();

        let composed = run(&agents, cwd.path(), project, None, &["things"]).unwrap();
        assert!(composed.untrusted.is_empty());

        let file = ProjectData::resolved(root.path(), project)
            .catalog()
            .join("mcp")
            .join("things.json");
        let text = std::fs::read_to_string(&file).unwrap();
        std::fs::write(&file, text.replace("npx", "/bin/sh")).unwrap();

        let composed = run(&agents, cwd.path(), project, None, &["things"]).unwrap();
        let omitted: Vec<(TrustKind, String, TrustReason)> = composed
            .untrusted
            .iter()
            .map(|it| (it.kind, it.id.clone(), it.reason))
            .collect();
        assert_eq!(omitted, vec![(TrustKind::Mcp, "things".into(), TrustReason::Changed)]);
        assert!(
            !composed.problems.iter().any(|it| it.contains("things")),
            "{:?}",
            composed.problems
        );

        approve(root.path(), project, &as_shown(root.path(), project), None);
        let composed = run(&agents, cwd.path(), project, None, &["things"]).unwrap();
        assert!(composed.untrusted.is_empty());

        catalog.remove_mcp(Some(project), "things").unwrap();
        let store = std::fs::read_to_string(path(root.path(), project)).unwrap();
        assert!(!store.contains("things"));
    }

    /// A project's harness agent (`D233`) is one of its definitions to a launch: written outside
    /// Kubiq it is new here and refused, approved it runs — composed from its file, its servers
    /// reaching `resolve` — and edited again it is refused again. Kubiq's own save trusts it.
    #[test]
    fn a_harness_agent_runs_only_as_trusted() {
        let root = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let agents = agents(root.path());
        let project = ProjectId::generate();
        let local = Some(folder.path());
        // The folder is the library's to name, even in a test.
        let dir = folder
            .path()
            .join(agent_manager::harness_agents::home_folder(Some("claude-code")).dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("helper.md");
        std::fs::write(
            &file,
            "---\nname: helper\ndescription: Helps\nkubiq-mcps: ghost\n---\nHelp.\n",
        )
        .unwrap();
        let start = |local: Option<&Path>| {
            agents.compose(
                PaneId::generate(),
                "claude-code",
                folder.path(),
                Vec::new(),
                ConverseOptions {
                    definition: Some("helper".to_string()),
                    project: Some(project),
                    local_agents: local.map(Path::to_path_buf),
                    ..Default::default()
                },
            )
        };

        assert_eq!(
            refused_items(start(local)),
            vec![("helper".to_string(), TrustReason::New)]
        );
        let shown: Vec<TrustItem> = untrusted(root.path(), project, local)
            .into_iter()
            .map(|it| TrustItem {
                kind: it.kind,
                id: it.id,
                fingerprint: it.fingerprint,
            })
            .collect();
        assert_eq!(shown.len(), 1, "the window is told about it");
        assert!(
            untrusted(root.path(), project, None).is_empty(),
            "a project that does not list its harness agents runs none of them"
        );
        approve(root.path(), project, &shown, local);
        let composed = start(local).expect("approved, it runs");
        assert!(
            composed.problems.iter().any(|it| it.contains("ghost")),
            "its own servers reached the run: {:?}",
            composed.problems
        );

        // An agent edits the file behind Kubiq's back.
        std::fs::write(
            &file,
            "---\nname: helper\ndescription: Helps\nkubiq-mode: bypassPermissions\n---\nHelp.\n",
        )
        .unwrap();
        assert_eq!(
            refused_items(start(local)),
            vec![("helper".to_string(), TrustReason::Changed)]
        );

        // Kubiq's own save of it trusts what it wrote.
        let mut saved = definition("helper", Some(project));
        saved.local = Some(kubiq_proto::messages::LocalAgent::default());
        crate::harness_agents::save(folder.path(), &saved, Some("helper"), false).unwrap();
        record_local(root.path(), project, &saved);
        assert!(untrusted(root.path(), project, local).is_empty());
        start(local).expect("saved through Kubiq, it runs");
    }
}
