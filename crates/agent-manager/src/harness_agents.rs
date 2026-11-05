//! A project's **harness agent files** — the subagent definitions each harness reads from its own
//! folder inside the project (`.claude/agents/<name>.md`, `.codex/agents/<name>.toml`, …) and the
//! cross-harness `.agents/agents/` folder.
//!
//! This module is the one place that knows which folders exist, which harness reads which, and
//! each folder's file format ([`FOLDERS`]). An embedder never names one of those paths: it asks
//! [`list`], [`save`], [`delete`] and [`sync`] about a project root.
//!
//! **A file is never rewritten blind.** It is parsed into its own keys and its body; a write
//! changes only the four portable fields — the name, the description, the instructions (the
//! markdown body, or Codex's `developer_instructions`) and the embedder's own `kubiq-*` keys — and
//! keeps every other key, the harness's own and any unknown one, as it was. A file whose portable
//! fields already agree is not written at all. YAML comments and quoting style inside a
//! frontmatter block are not kept on a rewrite; keys and values are.
//!
//! **One agent across folders** is recognised by its `kubiq-id` key, or — before it has one — by
//! its name (the frontmatter `name`, else the file stem); [`sync`] assigns the id the first time
//! it groups a file. When copies disagree the newest wins: the one with the latest
//! `kubiq-updated` timestamp, or its file modification time when it carries none, ties broken by
//! modification time.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

/// The prefix of every key the embedder owns in a harness agent file.
pub const KEY_PREFIX: &str = "kubiq-";
/// The key carrying the identity that ties one agent's copies together across folders.
pub const ID_KEY: &str = "kubiq-id";
/// The key carrying when the agent's content was last written by an embedder, RFC 3339 in UTC.
pub const UPDATED_KEY: &str = "kubiq-updated";

/// How a folder's files are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentFormat {
    /// Markdown with a YAML frontmatter block; the body is the instructions.
    Markdown,
    /// Codex's TOML agent file; `developer_instructions` is the instructions.
    CodexToml,
}

/// One folder a harness reads agent files from, relative to the project root.
#[derive(Debug, PartialEq, Eq)]
pub struct AgentFolder {
    /// The folder, `/`-separated, relative to the project root.
    pub dir: &'static str,
    /// The file name suffix an agent file carries in it.
    pub suffix: &'static str,
    /// How a file in it is written.
    pub format: AgentFormat,
    /// The harness ids that read it. Empty for the cross-harness folder.
    pub harnesses: &'static [&'static str],
    /// Whether the agent's name is written as a `name` key. opencode names an agent by its file
    /// alone, so a copy written there carries no `name`.
    pub name_key: bool,
    /// Whether [`sync`] writes into this folder when only its parent exists. `false` for a
    /// parent that is not the harness's own (`.github` holds far more than Copilot's agents).
    pub parent_marks: bool,
}

/// The cross-harness folder: where an agent goes when no harness folder is asked for.
pub const NEUTRAL_DIR: &str = ".agents/agents";

/// Every folder this library knows, the cross-harness one first. The only list of them.
pub const FOLDERS: &[AgentFolder] = &[
    AgentFolder {
        dir: NEUTRAL_DIR,
        suffix: ".md",
        format: AgentFormat::Markdown,
        harnesses: &[],
        name_key: true,
        parent_marks: true,
    },
    AgentFolder {
        dir: ".claude/agents",
        suffix: ".md",
        format: AgentFormat::Markdown,
        harnesses: &["claude-code", "claude-code-acp", "codebuddy"],
        name_key: true,
        parent_marks: true,
    },
    AgentFolder {
        dir: ".codex/agents",
        suffix: ".toml",
        format: AgentFormat::CodexToml,
        harnesses: &["codex", "codex-acp"],
        name_key: true,
        parent_marks: true,
    },
    AgentFolder {
        dir: ".gemini/agents",
        suffix: ".md",
        format: AgentFormat::Markdown,
        harnesses: &["gemini"],
        name_key: true,
        parent_marks: true,
    },
    AgentFolder {
        dir: ".opencode/agents",
        suffix: ".md",
        format: AgentFormat::Markdown,
        harnesses: &["opencode"],
        name_key: false,
        parent_marks: true,
    },
    AgentFolder {
        dir: ".github/agents",
        suffix: ".agent.md",
        format: AgentFormat::Markdown,
        harnesses: &["copilot"],
        name_key: true,
        parent_marks: false,
    },
];

/// The folder `dir` names, if it is one of [`FOLDERS`].
pub fn folder(dir: &str) -> Option<&'static AgentFolder> {
    FOLDERS.iter().find(|it| it.dir == dir)
}

/// The folder an agent for `harness` is written to when only one is: that harness's own, else
/// the cross-harness one.
pub fn home_folder(harness: Option<&str>) -> &'static AgentFolder {
    harness
        .and_then(|id| FOLDERS.iter().find(|it| it.harnesses.contains(&id)))
        .unwrap_or(&FOLDERS[0])
}

/// The folder's primary harness, when it belongs to one.
pub fn folder_harness(folder: &AgentFolder) -> Option<&'static str> {
    folder.harnesses.first().copied()
}

/// What an agent is, independent of the folder it is written in: the four fields [`sync`]
/// carries between copies.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentContent {
    /// The agent's name.
    pub name: String,
    /// What it is for.
    pub description: Option<String>,
    /// Its instructions — the body.
    pub instructions: String,
    /// The embedder's own keys, without [`KEY_PREFIX`] and without the id and timestamp keys.
    pub kubiq: BTreeMap<String, String>,
}

impl AgentContent {
    fn same_as(&self, other: &AgentContent) -> bool {
        self.name == other.name
            && self
                .description
                .as_deref()
                .map(str::trim)
                .filter(|it| !it.is_empty())
                == other
                    .description
                    .as_deref()
                    .map(str::trim)
                    .filter(|it| !it.is_empty())
            && self.instructions.trim() == other.instructions.trim()
            && self.kubiq == other.kubiq
    }
}

/// The parsed keys of one file, in the file's own order where the format keeps one.
#[derive(Clone, Debug)]
enum Keys {
    Yaml(serde_yaml::Mapping),
    Toml(toml::Table),
}

/// One agent file, parsed.
#[derive(Clone, Debug)]
pub struct AgentFile {
    /// The folder it was found in.
    pub folder: &'static AgentFolder,
    /// Where it is.
    pub path: PathBuf,
    /// Its modification time, when the file system says.
    pub modified: Option<SystemTime>,
    keys: Keys,
    /// The markdown body, as written. Empty for TOML.
    body: String,
}

fn yaml_text(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(it) => Some(it.clone()),
        serde_yaml::Value::Number(it) => Some(it.to_string()),
        serde_yaml::Value::Bool(it) => Some(it.to_string()),
        serde_yaml::Value::Sequence(items) => Some(
            items
                .iter()
                .filter_map(yaml_text)
                .collect::<Vec<_>>()
                .join(", "),
        ),
        _ => None,
    }
}

fn toml_text(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(it) => Some(it.clone()),
        toml::Value::Integer(it) => Some(it.to_string()),
        toml::Value::Float(it) => Some(it.to_string()),
        toml::Value::Boolean(it) => Some(it.to_string()),
        toml::Value::Array(items) => Some(
            items
                .iter()
                .filter_map(toml_text)
                .collect::<Vec<_>>()
                .join(", "),
        ),
        _ => None,
    }
}

impl AgentFile {
    /// Parse `text` as a file of `folder` found at `path`.
    pub fn parse(folder: &'static AgentFolder, path: PathBuf, text: &str) -> Result<Self> {
        let (keys, body) = match folder.format {
            AgentFormat::Markdown => {
                let (front, body) = split_frontmatter(text);
                let keys = match front {
                    Some(front) if !front.trim().is_empty() => serde_yaml::from_str(front)
                        .with_context(|| {
                            format!("reading the frontmatter of {}", path.display())
                        })?,
                    _ => serde_yaml::Mapping::new(),
                };
                (Keys::Yaml(keys), body.to_string())
            }
            AgentFormat::CodexToml => (
                Keys::Toml(
                    toml::from_str(text).with_context(|| format!("reading {}", path.display()))?,
                ),
                String::new(),
            ),
        };
        Ok(Self {
            folder,
            path,
            modified: None,
            keys,
            body,
        })
    }

    /// Read and parse the file at `path`.
    pub fn read(folder: &'static AgentFolder, path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut file = Self::parse(folder, path.to_path_buf(), &text)?;
        file.modified = std::fs::metadata(path).and_then(|it| it.modified()).ok();
        Ok(file)
    }

    /// A new, empty file of `folder` at `path`.
    pub fn empty(folder: &'static AgentFolder, path: PathBuf) -> Self {
        Self {
            folder,
            path,
            modified: None,
            keys: match folder.format {
                AgentFormat::Markdown => Keys::Yaml(serde_yaml::Mapping::new()),
                AgentFormat::CodexToml => Keys::Toml(toml::Table::new()),
            },
            body: String::new(),
        }
    }

    /// The text this file is written as.
    pub fn render(&self) -> Result<String> {
        match &self.keys {
            Keys::Yaml(keys) => {
                let front = if keys.is_empty() {
                    String::new()
                } else {
                    serde_yaml::to_string(keys).context("writing the frontmatter")?
                };
                Ok(format!("---\n{front}---\n{}", self.body))
            }
            Keys::Toml(keys) => toml::to_string(keys).context("writing the agent file"),
        }
    }

    /// Write the file where [`Self::path`] says, creating its folder.
    pub fn write(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&self.path, self.render()?)
            .with_context(|| format!("writing {}", self.path.display()))
    }

    /// A string-valued key, lists joined by `, `.
    pub fn get(&self, key: &str) -> Option<String> {
        match &self.keys {
            Keys::Yaml(keys) => keys.get(key).and_then(yaml_text),
            Keys::Toml(keys) => keys.get(key).and_then(toml_text),
        }
    }

    /// Set a key to a string, or remove it on `None`. An existing key keeps its place.
    pub fn set(&mut self, key: &str, value: Option<&str>) {
        match (&mut self.keys, value) {
            (Keys::Yaml(keys), Some(value)) => {
                keys.insert(key.into(), value.into());
            }
            (Keys::Yaml(keys), None) => {
                keys.remove(key);
            }
            (Keys::Toml(keys), Some(value)) => {
                keys.insert(key.to_string(), value.into());
            }
            (Keys::Toml(keys), None) => {
                keys.remove(key);
            }
        }
    }

    /// Every key name, in the file's order.
    pub fn keys(&self) -> Vec<String> {
        match &self.keys {
            Keys::Yaml(keys) => keys
                .keys()
                .filter_map(|it| it.as_str().map(str::to_string))
                .collect(),
            Keys::Toml(keys) => keys.keys().cloned().collect(),
        }
    }

    /// The file name without the folder's suffix.
    pub fn stem(&self) -> String {
        let name = self
            .path
            .file_name()
            .map(|it| it.to_string_lossy().into_owned())
            .unwrap_or_default();
        name.strip_suffix(self.folder.suffix)
            .unwrap_or(&name)
            .to_string()
    }

    /// The agent's name: its `name` key, else the file stem.
    pub fn name(&self) -> String {
        self.get("name")
            .map(|it| it.trim().to_string())
            .filter(|it| !it.is_empty())
            .unwrap_or_else(|| self.stem())
    }

    /// Its `kubiq-id`, when it has one.
    pub fn id(&self) -> Option<String> {
        self.get(ID_KEY)
            .map(|it| it.trim().to_string())
            .filter(|it| !it.is_empty())
    }

    /// Its instructions: the markdown body, or Codex's `developer_instructions`.
    pub fn instructions(&self) -> String {
        match self.folder.format {
            AgentFormat::Markdown => self.body.trim().to_string(),
            AgentFormat::CodexToml => self
                .get("developer_instructions")
                .unwrap_or_default()
                .trim()
                .to_string(),
        }
    }

    /// The portable fields of this file.
    pub fn content(&self) -> AgentContent {
        let kubiq = self
            .keys()
            .into_iter()
            .filter(|key| key != ID_KEY && key != UPDATED_KEY)
            .filter_map(|key| {
                let short = key.strip_prefix(KEY_PREFIX)?.to_string();
                Some((short, self.get(&key)?))
            })
            .collect();
        AgentContent {
            name: self.name(),
            description: self
                .get("description")
                .map(|it| it.trim().to_string())
                .filter(|it| !it.is_empty()),
            instructions: self.instructions(),
            kubiq,
        }
    }

    /// Write `content` into this file's keys and body, leaving every other key as it is. The body
    /// is replaced only when the instructions actually differ, so its own spacing survives.
    pub fn apply(&mut self, content: &AgentContent) {
        if self.folder.name_key || self.get("name").is_some() {
            self.set("name", Some(&content.name));
        }
        self.set("description", content.description.as_deref());
        if self.instructions() != content.instructions.trim() {
            match self.folder.format {
                AgentFormat::Markdown => {
                    self.body = if content.instructions.trim().is_empty() {
                        String::new()
                    } else {
                        format!("\n{}\n", content.instructions.trim())
                    };
                }
                AgentFormat::CodexToml => {
                    self.set("developer_instructions", Some(content.instructions.trim()))
                }
            }
        }
        // Embedder keys this content no longer carries go; the id and the stamp are set apart.
        for key in self.keys() {
            if let Some(short) = key.strip_prefix(KEY_PREFIX)
                && key != ID_KEY
                && key != UPDATED_KEY
                && !content.kubiq.contains_key(short)
            {
                self.set(&key, None);
            }
        }
        for (short, value) in &content.kubiq {
            self.set(&format!("{KEY_PREFIX}{short}"), Some(value));
        }
    }

    /// When its content was last written by an embedder, from `kubiq-updated`.
    pub fn updated(&self) -> Option<SystemTime> {
        self.get(UPDATED_KEY).and_then(|it| parse_rfc3339(&it))
    }

    /// What "newest" is ordered by: `kubiq-updated`, else the modification time; then the
    /// modification time.
    fn freshness(&self) -> (SystemTime, SystemTime) {
        let modified = self.modified.unwrap_or(UNIX_EPOCH);
        (self.updated().unwrap_or(modified), modified)
    }
}

/// Split a markdown text into its frontmatter block and its body. No block is `(None, text)`.
fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let rest = match text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    {
        Some(rest) => rest,
        None => return (None, text),
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return (Some(&rest[..offset]), &rest[offset + line.len()..]);
        }
        offset += line.len();
    }
    (None, text)
}

/// One agent as the project holds it: its content, from the newest copy, and every copy.
#[derive(Clone, Debug)]
pub struct LocalAgent {
    /// The `kubiq-id` its copies share, or the one [`sync`] would assign.
    pub id: String,
    /// The newest copy's content.
    pub content: AgentContent,
    /// Every copy, the newest first.
    pub files: Vec<AgentFile>,
}

impl LocalAgent {
    /// The folders it has a copy in, in [`FOLDERS`] order.
    pub fn folders(&self) -> Vec<&'static str> {
        FOLDERS
            .iter()
            .filter(|folder| self.files.iter().any(|it| it.folder.dir == folder.dir))
            .map(|folder| folder.dir)
            .collect()
    }

    /// When its newest copy was written, as [`AgentFile::freshness`] reads it.
    fn stamp(&self) -> SystemTime {
        self.files
            .first()
            .map(|it| it.freshness().0)
            .unwrap_or(UNIX_EPOCH)
    }
}

/// The id an agent named `name` is given when it has none: its name, lowercased, every run of
/// other characters a single `-`. Deterministic, so two folders holding the same agent assign
/// the same id without having seen each other.
pub fn id_for(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Every agent file under `root`'s folders. A file that cannot be parsed is skipped and logged.
pub fn scan(root: &Path) -> Vec<AgentFile> {
    let mut files = Vec::new();
    for folder in FOLDERS {
        let dir = root.join(folder.dir);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|it| it.ok().map(|it| it.path()))
            .filter(|path| path.is_file())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(folder.suffix))
            })
            .collect();
        paths.sort();
        for path in paths {
            match AgentFile::read(folder, &path) {
                Ok(file) => files.push(file),
                Err(error) => tracing::warn!("a harness agent file was skipped: {error:#}"),
            }
        }
    }
    files
}

/// Group files into agents: the same `kubiq-id`, or — for a file without one — the same name as
/// an agent already grouped, else the same name as other files without one.
pub fn group(files: Vec<AgentFile>) -> Vec<LocalAgent> {
    let (with_id, without): (Vec<AgentFile>, Vec<AgentFile>) =
        files.into_iter().partition(|it| it.id().is_some());
    let mut groups: Vec<(String, Vec<AgentFile>)> = Vec::new();
    for file in with_id {
        let id = file.id().unwrap_or_default();
        match groups.iter_mut().find(|(it, _)| *it == id) {
            Some((_, members)) => members.push(file),
            None => groups.push((id, vec![file])),
        }
    }
    for file in without {
        let name = file.name();
        let stem = file.stem();
        let joined = groups.iter_mut().find(|(_, members)| {
            members
                .iter()
                .any(|it| it.name() == name || it.stem() == stem)
        });
        match joined {
            Some((_, members)) => members.push(file),
            None => groups.push((id_for(&name), vec![file])),
        }
    }
    let mut agents: Vec<LocalAgent> = Vec::new();
    for (id, mut files) in groups {
        files.sort_by_key(|it| std::cmp::Reverse(it.freshness()));
        let content = files[0].content();
        // Two agents of one name are one definition to the embedder: the first stays.
        if agents.iter().any(|it| it.content.name == content.name) {
            tracing::warn!(
                "Agent definition skipped: duplicate name {} in {}",
                content.name,
                files[0].path.display()
            );
            continue;
        }
        agents.push(LocalAgent { id, content, files });
    }
    agents
}

/// Every agent under `root`, one per identity, the newest copy's content on each.
pub fn list(root: &Path) -> Vec<LocalAgent> {
    group(scan(root))
}

/// The folders [`sync`] keeps aligned under `root`: the cross-harness one always, and each
/// harness folder that exists — or whose parent does, where the parent is the harness's own.
pub fn sync_folders(root: &Path) -> Vec<&'static AgentFolder> {
    FOLDERS
        .iter()
        .filter(|folder| {
            let dir = root.join(folder.dir);
            folder.dir == NEUTRAL_DIR
                || dir.is_dir()
                || (folder.parent_marks && dir.parent().is_some_and(Path::is_dir))
        })
        .collect()
}

fn file_name(folder: &AgentFolder, name: &str) -> String {
    let stem: String = name
        .chars()
        .map(|ch| {
            if ch == '/' || ch == '\\' || ch.is_control() {
                '-'
            } else {
                ch
            }
        })
        .collect();
    format!("{}{}", stem.trim(), folder.suffix)
}

/// Write `content` into `file`, with the agent's id and `stamp`, if anything it holds differs.
/// Answers whether it was written.
fn put(file: &mut AgentFile, id: &str, content: &AgentContent, stamp: SystemTime) -> Result<bool> {
    let same = file.content().same_as(content) && file.id().as_deref() == Some(id);
    if same && file.path.exists() {
        return Ok(false);
    }
    file.apply(content);
    file.set(ID_KEY, Some(id));
    file.set(UPDATED_KEY, Some(&format_rfc3339(stamp)));
    file.write()?;
    Ok(true)
}

/// What a [`sync`] changed.
#[derive(Clone, Debug, Default)]
pub struct SyncReport {
    /// Every file written or created.
    pub written: Vec<PathBuf>,
}

/// Align every agent under `root` across [`sync_folders`]: each copy gets the agent's id, a copy
/// whose content differs from the newest one is rewritten to it, and a folder missing a copy
/// gets one. Nothing else is touched, and a copy already aligned is not written.
pub fn sync(root: &Path) -> Result<SyncReport> {
    let targets = sync_folders(root);
    let mut report = SyncReport::default();
    for agent in list(root) {
        let stamp = agent.stamp();
        let newest_name = agent.content.name.clone();
        for mut file in agent.files.clone() {
            match put(&mut file, &agent.id, &agent.content, stamp) {
                Ok(true) => report.written.push(file.path.clone()),
                Ok(false) => {}
                Err(error) => tracing::warn!("Agent file not aligned: {error:#}"),
            }
        }
        for folder in &targets {
            if agent.files.iter().any(|it| it.folder.dir == folder.dir) {
                continue;
            }
            let path = root.join(folder.dir).join(file_name(folder, &newest_name));
            if path.exists() {
                // A file of that name that is a different agent: never overwritten.
                tracing::warn!(
                    "{} is another agent: '{newest_name}' was not copied there",
                    path.display()
                );
                continue;
            }
            let mut file = AgentFile::empty(folder, path);
            match put(&mut file, &agent.id, &agent.content, stamp) {
                Ok(_) => report.written.push(file.path.clone()),
                Err(error) => tracing::warn!("Agent file not aligned: {error:#}"),
            }
        }
    }
    Ok(report)
}

/// Write an agent under `root`, as the embedder's form saved it.
///
/// `previous` is the name it was opened under, for a rename. An agent already held under that
/// name (or under `content.name`) has every copy rewritten in place — and renamed on disk when
/// the name changed; a new one is written to `folders`. Then, when `sync_all` is set, [`sync`]
/// carries it to every other folder. The write is stamped now, so it is the newest copy.
pub fn save(
    root: &Path,
    content: &AgentContent,
    previous: Option<&str>,
    folders: &[&'static AgentFolder],
    sync_all: bool,
) -> Result<()> {
    if content.name.trim().is_empty() {
        bail!("an agent needs a name");
    }
    let agents = list(root);
    let looked_for = previous.unwrap_or(&content.name);
    let existing = agents.iter().find(|it| it.content.name == looked_for);
    if previous.is_some_and(|old| old != content.name)
        && agents.iter().any(|it| {
            it.content.name == content.name
                && Some(it.id.as_str()) != existing.map(|e| e.id.as_str())
        })
    {
        bail!("an agent called '{}' already exists", content.name);
    }
    let now = SystemTime::now();
    match existing {
        Some(agent) => {
            for mut file in agent.files.clone() {
                let old_path = file.path.clone();
                // A file named after the agent follows a rename; one named otherwise keeps its name.
                if looked_for != content.name && file.stem() == looked_for {
                    let renamed = old_path
                        .parent()
                        .map(|dir| dir.join(file_name(file.folder, &content.name)))
                        .unwrap_or_else(|| old_path.clone());
                    if renamed.exists() {
                        bail!("{} already exists", renamed.display());
                    }
                    file.path = renamed;
                }
                put(&mut file, &agent.id, content, now)?;
                if file.path != old_path {
                    std::fs::remove_file(&old_path)
                        .with_context(|| format!("removing {}", old_path.display()))?;
                }
            }
        }
        None => {
            let id = id_for(&content.name);
            for folder in folders {
                let path = root.join(folder.dir).join(file_name(folder, &content.name));
                if path.exists() {
                    bail!("{} already exists", path.display());
                }
                put(&mut AgentFile::empty(folder, path), &id, content, now)?;
            }
        }
    }
    if sync_all {
        sync(root)?;
    }
    Ok(())
}

/// Remove every copy of the agent called `name` under `root`. Refused when there is none.
pub fn delete(root: &Path, name: &str) -> Result<()> {
    let Some(agent) = list(root).into_iter().find(|it| it.content.name == name) else {
        bail!("there is no harness agent called '{name}'");
    };
    for file in &agent.files {
        std::fs::remove_file(&file.path)
            .with_context(|| format!("removing {}", file.path.display()))?;
    }
    Ok(())
}

// ── RFC 3339 in UTC, millisecond precision, without a date crate ─────────────────────────

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `time` as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub fn format_rfc3339(time: SystemTime) -> String {
    let millis = time
        .duration_since(UNIX_EPOCH)
        .map(|it| it.as_millis() as i64)
        .unwrap_or(0);
    let (secs, ms) = (millis.div_euclid(1000), millis.rem_euclid(1000));
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{ms:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Parse `YYYY-MM-DDTHH:MM:SS[.fff]Z` (or a `+00:00` offset). Anything else is `None`.
pub fn parse_rfc3339(text: &str) -> Option<SystemTime> {
    let text = text.trim();
    let text = text
        .strip_suffix('Z')
        .or_else(|| text.strip_suffix("+00:00"))?;
    let (date, time) = text.split_once(['T', ' '])?;
    let mut date = date.split('-').map(|it| it.parse::<i64>().ok());
    let (y, m, d) = (date.next()??, date.next()??, date.next()??);
    let (clock, fraction) = time.split_once('.').unwrap_or((time, ""));
    let mut clock = clock.split(':').map(|it| it.parse::<i64>().ok());
    let (hh, mm, ss) = (clock.next()??, clock.next()??, clock.next()??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    let ms: i64 = if fraction.is_empty() {
        0
    } else {
        format!("{:0<3}", &fraction[..fraction.len().min(3)])
            .parse()
            .ok()?
    };
    let secs = days_from_civil(y, m, d) * 86_400 + hh * 3600 + mm * 60 + ss;
    let millis = u64::try_from(secs * 1000 + ms).ok()?;
    Some(UNIX_EPOCH + Duration::from_millis(millis))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude() -> &'static AgentFolder {
        folder(".claude/agents").unwrap()
    }

    fn write(path: &Path, text: &str, modified: SystemTime) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    }

    const REVIEWER: &str = "---\nname: reviewer\ndescription: Reviews code\ntools: Read, Grep\nmodel: sonnet\ncolor: blue\nx-custom:\n  nested: [1, 2]\n---\n\nYou review code.\n";

    #[test]
    fn a_rewrite_keeps_the_harness_keys_and_unknown_ones() {
        let mut file = AgentFile::parse(claude(), PathBuf::from("reviewer.md"), REVIEWER).unwrap();
        let mut content = file.content();
        content.description = Some("Reviews code carefully".into());
        content.kubiq.insert("harness".into(), "claude-code".into());
        file.apply(&content);
        file.set(ID_KEY, Some("reviewer"));
        let text = file.render().unwrap();
        let back = AgentFile::parse(claude(), PathBuf::from("reviewer.md"), &text).unwrap();
        assert_eq!(
            back.keys(),
            vec![
                "name",
                "description",
                "tools",
                "model",
                "color",
                "x-custom",
                "kubiq-harness",
                "kubiq-id"
            ]
        );
        assert_eq!(back.get("tools").as_deref(), Some("Read, Grep"));
        assert_eq!(back.get("model").as_deref(), Some("sonnet"));
        assert!(text.contains("nested:"), "{text}");
        assert_eq!(back.instructions(), "You review code.");
        assert_eq!(back.content().kubiq.get("harness").unwrap(), "claude-code");
        // The body was not touched: its blank line survives.
        assert!(text.ends_with("---\n\nYou review code.\n"), "{text}");
    }

    #[test]
    fn a_codex_file_round_trips_its_own_keys() {
        let codex = folder(".codex/agents").unwrap();
        let text = "name = \"reviewer\"\ndescription = \"Reviews\"\ndeveloper_instructions = \"Review.\"\nmodel = \"gpt-5\"\nsandbox_mode = \"read-only\"\n";
        let mut file = AgentFile::parse(codex, PathBuf::from("reviewer.toml"), text).unwrap();
        assert_eq!(file.instructions(), "Review.");
        let mut content = file.content();
        content.instructions = "Review twice.".into();
        file.apply(&content);
        let back = AgentFile::parse(
            codex,
            PathBuf::from("reviewer.toml"),
            &file.render().unwrap(),
        )
        .unwrap();
        assert_eq!(back.get("sandbox_mode").as_deref(), Some("read-only"));
        assert_eq!(back.get("model").as_deref(), Some("gpt-5"));
        assert_eq!(back.instructions(), "Review twice.");
    }

    #[test]
    fn copies_with_one_kubiq_id_are_one_agent_whatever_their_names() {
        let dir = tempfile::tempdir().unwrap();
        let t = UNIX_EPOCH + Duration::from_secs(1_000_000);
        write(
            &dir.path().join(".claude/agents/a.md"),
            "---\nname: alpha\nkubiq-id: x1\n---\nOne\n",
            t,
        );
        write(
            &dir.path().join(".agents/agents/b.md"),
            "---\nname: beta\nkubiq-id: x1\n---\nTwo\n",
            t + Duration::from_secs(5),
        );
        write(
            &dir.path().join(".gemini/agents/alpha.md"),
            "---\nname: alpha\n---\nThree\n",
            t,
        );
        let agents = list(dir.path());
        assert_eq!(agents.len(), 1, "{agents:?}");
        assert_eq!(agents[0].id, "x1");
        assert_eq!(agents[0].files.len(), 3);
        assert_eq!(agents[0].content.name, "beta");
    }

    #[test]
    fn sync_assigns_an_id_and_the_newest_copy_wins() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let t = UNIX_EPOCH + Duration::from_secs(2_000_000);
        write(&root.join(".claude/agents/reviewer.md"), REVIEWER, t);
        write(
            &root.join(".agents/agents/reviewer.md"),
            "---\nname: reviewer\ndescription: Old\nkeep-me: yes\n---\nOld body\n",
            t - Duration::from_secs(60),
        );
        let report = sync(root).unwrap();
        assert_eq!(report.written.len(), 2, "{report:?}");
        let neutral =
            AgentFile::read(&FOLDERS[0], &root.join(".agents/agents/reviewer.md")).unwrap();
        assert_eq!(neutral.id().as_deref(), Some("reviewer"));
        assert_eq!(neutral.instructions(), "You review code.");
        assert_eq!(neutral.get("description").as_deref(), Some("Reviews code"));
        // Its own key is kept; the Claude copy's own keys are not carried over.
        assert_eq!(neutral.get("keep-me").as_deref(), Some("yes"));
        assert_eq!(neutral.get("tools"), None);
        let claude_copy =
            AgentFile::read(claude(), &root.join(".claude/agents/reviewer.md")).unwrap();
        assert_eq!(claude_copy.get("tools").as_deref(), Some("Read, Grep"));
        assert_eq!(claude_copy.updated(), Some(t));
        // A second pass finds everything aligned.
        assert!(sync(root).unwrap().written.is_empty());
    }

    #[test]
    fn a_later_kubiq_updated_outranks_a_later_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let t = UNIX_EPOCH + Duration::from_secs(3_000_000);
        let stamp = format_rfc3339(t + Duration::from_secs(3600));
        write(
            &root.join(".claude/agents/r.md"),
            &format!("---\nname: r\nkubiq-id: r\nkubiq-updated: {stamp}\n---\nFrom Kubiq\n"),
            t,
        );
        write(
            &root.join(".agents/agents/r.md"),
            "---\nname: r\nkubiq-id: r\nkubiq-updated: 1970-02-04T00:00:00Z\n---\nStale\n",
            t + Duration::from_secs(60),
        );
        sync(root).unwrap();
        let neutral = AgentFile::read(&FOLDERS[0], &root.join(".agents/agents/r.md")).unwrap();
        assert_eq!(neutral.instructions(), "From Kubiq");
    }

    #[test]
    fn sync_creates_copies_only_where_a_harness_lives() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".codex")).unwrap();
        std::fs::create_dir_all(root.join(".github")).unwrap();
        write(
            &root.join(".claude/agents/r.md"),
            "---\nname: r\ndescription: d\n---\nBody\n",
            SystemTime::now(),
        );
        sync(root).unwrap();
        assert!(root.join(".agents/agents/r.md").is_file());
        let codex = AgentFile::read(
            folder(".codex/agents").unwrap(),
            &root.join(".codex/agents/r.toml"),
        )
        .unwrap();
        assert_eq!(codex.instructions(), "Body");
        assert!(!root.join(".github/agents").exists());
        assert!(!root.join(".gemini").exists());
    }

    #[test]
    fn save_renames_every_copy_and_delete_removes_them() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let content = AgentContent {
            name: "writer".into(),
            description: Some("Writes".into()),
            instructions: "Write.".into(),
            kubiq: BTreeMap::from([("harness".into(), "claude-code".into())]),
        };
        save(
            root,
            &content,
            None,
            &[home_folder(Some("claude-code"))],
            false,
        )
        .unwrap();
        assert!(root.join(".claude/agents/writer.md").is_file());
        assert!(!root.join(".agents/agents/writer.md").exists());
        let renamed = AgentContent {
            name: "author".into(),
            ..content
        };
        save(root, &renamed, Some("writer"), &[], true).unwrap();
        assert!(!root.join(".claude/agents/writer.md").exists());
        assert!(root.join(".claude/agents/author.md").is_file());
        assert!(root.join(".agents/agents/author.md").is_file());
        let agents = list(root);
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id, "writer", "the id survives a rename");
        delete(root, "author").unwrap();
        assert!(list(root).is_empty());
    }

    #[test]
    fn a_duplicate_name_and_an_unparseable_file_leave_the_rest_listed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let now = SystemTime::now();
        let agent =
            |id: &str, name: &str| format!("---\nname: {name}\nkubiq-id: {id}\n---\n\nbody {id}\n");
        write(
            &root.join(".claude/agents/a.md"),
            &agent("one", "twin"),
            now,
        );
        // Another agent, another id, the same name: the first stays.
        write(
            &root.join(".claude/agents/b.md"),
            &agent("two", "twin"),
            now,
        );
        write(
            &root.join(".claude/agents/c.md"),
            &agent("three", "solo"),
            now,
        );
        // A file that cannot be read as text at all.
        std::fs::write(root.join(".claude/agents/d.md"), [0xff, 0xfe, 0x00]).unwrap();
        let agents = list(root);
        let names: Vec<&str> = agents.iter().map(|it| it.content.name.as_str()).collect();
        assert_eq!(names, ["twin", "solo"]);
        assert_eq!(agents[0].id, "one");
        // Housekeeping neither fails nor drops anything.
        sync(root).unwrap();
        assert_eq!(list(root).len(), 2);
    }

    #[test]
    fn rfc3339_round_trips() {
        let t = UNIX_EPOCH + Duration::from_millis(1_762_336_800_123);
        let text = format_rfc3339(t);
        assert_eq!(text, "2025-11-05T10:00:00.123Z");
        assert_eq!(parse_rfc3339(&text), Some(t));
        assert_eq!(
            parse_rfc3339("2025-11-05T10:00:00Z"),
            Some(UNIX_EPOCH + Duration::from_secs(1_762_336_800))
        );
        assert_eq!(parse_rfc3339("yesterday"), None);
    }
}
