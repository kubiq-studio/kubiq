//! The policy manager's state: the host's list of sandbox policies, and the three overlays that
//! work on it — the editor, the import question and the clone-as prompt.
//!
//! **A policy is edited as a [`PolicyDoc`]** (`kubiq_proto::policy`), never as TOML: the editor
//! holds one box per typed field and [`PolicyEditor::to_doc`] reads them back, so the interface
//! parses nothing. The boxes are `InputState` entities because the widget's state *is* the model;
//! they need a window to be made, which is why an answer from the host waits in
//! [`PolicyState::fill`] for the frame that has one.
//!
//! Enum-valued fields (`ro`, `subpath`, `mach-lookup`) are the strings isol8 writes, so the lists
//! below are the vocabulary the editor offers and the host validates.

use gpui::{App, AppContext as _, Entity, Window};
use gpui_component::input::{InputState, TextareaState};
use kubiq_proto::policy::{
    PolicyDoc, PolicyFilter, PolicyHome, PolicyInfo, PolicyOrigin, PolicyPath, PolicyRule,
};

/// What a path grant may do, in the order the cycle button walks them.
pub const ACCESS: [&str; 4] = ["ro", "rw", "none", "metadata"];
/// How a grant's path is matched.
pub const MATCHING: [&str; 4] = ["subpath", "literal", "prefix", "regex"];
/// The operating systems a filter names.
pub const OSES: [&str; 3] = ["macos", "linux", "windows"];
/// The architectures a filter names.
pub const ARCHES: [&str; 2] = ["x86_64", "aarch64"];
/// isol8's macOS capabilities.
pub const MACOS_CAPABILITIES: [&str; 14] = [
    "mach-lookup",
    "mach-register",
    "iokit-open",
    "sysctl-read",
    "process-exec",
    "process-fork",
    "process-info",
    "signal",
    "pseudo-tty",
    "user-preference-read",
    "user-preference-write",
    "ipc-posix-shm",
    "sysv-sem",
    "pasteboard",
];
/// isol8's Windows capabilities.
pub const WINDOWS_CAPABILITIES: [&str; 12] = [
    "internet-client",
    "internet-client-server",
    "private-network-client-server",
    "documents-library",
    "pictures-library",
    "videos-library",
    "music-library",
    "enterprise-authentication",
    "shared-user-certificates",
    "removable-storage",
    "appointments",
    "contacts",
];

/// The list, and everything open over it.
#[derive(Default)]
pub struct PolicyState {
    /// Every policy the host resolves, from `Message::Policies`; empty until it answers.
    pub policies: Vec<PolicyInfo>,
    /// The host's last refusal (`Message::PolicyError`), shown where the user is working: in the
    /// editor or the import question while one is up, on the section otherwise.
    pub error: Option<String>,
    pub editor: Option<PolicyEditor>,
    /// A `PolicyBody` the next frame builds an editor from.
    pub fill: Option<PolicyFill>,
    pub import: Option<PolicyImport>,
    pub clone_as: Option<PolicyCloneAs>,
    /// The policy a delete is awaiting a yes on.
    pub confirm_delete: Option<String>,
}

impl PolicyState {
    /// The row named `name`.
    pub fn find(&self, name: &str) -> Option<&PolicyInfo> {
        self.policies.iter().find(|policy| policy.name == name)
    }

    /// The names of the defaults, which every isolated run gets whatever it picks.
    pub fn defaults(&self) -> impl Iterator<Item = &str> {
        self.policies
            .iter()
            .filter(|policy| policy.default)
            .map(|policy| policy.name.as_str())
    }
}

/// What the host said a policy holds, waiting for a window.
pub struct PolicyFill {
    pub info: PolicyInfo,
    pub doc: PolicyDoc,
    pub toml: String,
}

/// The import question: a repository, and what came of it.
pub struct PolicyImport {
    pub url: Entity<InputState>,
    pub git_ref: Entity<InputState>,
    /// Asked and not answered yet.
    pub busy: bool,
    /// The layers the last import wrote.
    pub result: Option<Vec<String>>,
}

impl PolicyImport {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            url: cx.new(|cx| {
                InputState::new(window, cx).placeholder("https://github.com/org/policies.git")
            }),
            git_ref: cx
                .new(|cx| InputState::new(window, cx).placeholder("Branch or tag (optional)")),
            busy: false,
            result: None,
        }
    }
}

/// The "clone as" prompt: which policy, and the name it is copied to.
pub struct PolicyCloneAs {
    pub source: String,
    pub name: Entity<InputState>,
}

/// Which advanced section of the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Home,
    Rewrite,
    Rules,
    Macos,
    Windows,
}

/// A list of ticks on the editor, for [`PolicyEditor::toggle`]. `Option<usize>` names a
/// conditional rule; `None` is the layer's own filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    Requires,
    MacosCapability,
    WindowsCapability,
    Os(Option<usize>),
    Arch(Option<usize>),
}

/// One grant row.
pub struct PathRow {
    pub path: Entity<InputState>,
    pub access: String,
    pub matching: String,
}

impl PathRow {
    fn new(grant: &PolicyPath, window: &mut Window, cx: &mut App) -> Self {
        let path =
            cx.new(|cx| InputState::new(window, cx).placeholder("~/.cargo, /usr/lib\u{2026}"));
        path.update(cx, |state, cx| state.set_value(&grant.path, window, cx));
        Self {
            path,
            access: pick(&ACCESS, &grant.access, "ro"),
            matching: pick(&MATCHING, &grant.matching, "subpath"),
        }
    }

    fn to_path(&self, cx: &App) -> PolicyPath {
        PolicyPath {
            path: self.path.read(cx).value().trim().to_string(),
            access: self.access.clone(),
            matching: self.matching.clone(),
        }
    }
}

/// An `os`/`arch`/`executables` gate: ticks for the first two, a line per executable.
pub struct FilterRows {
    pub os: Vec<String>,
    pub arch: Vec<String>,
    pub executables: Entity<TextareaState>,
}

impl FilterRows {
    fn new(filter: &PolicyFilter, window: &mut Window, cx: &mut App) -> Self {
        Self {
            os: filter.os.clone(),
            arch: filter.arch.clone(),
            executables: lines(window, cx, "claude\ncodex", &filter.executables),
        }
    }

    fn to_filter(&self, cx: &App) -> PolicyFilter {
        PolicyFilter {
            os: self.os.clone(),
            arch: self.arch.clone(),
            executables: read_lines(&self.executables, cx),
        }
    }
}

/// One conditional block.
pub struct RuleRow {
    pub filter: FilterRows,
    pub paths: Vec<PathRow>,
}

/// One environment default.
pub struct EnvRow {
    pub key: Entity<InputState>,
    pub value: Entity<InputState>,
}

/// The policy editor: a new policy, a local one being changed, or a read-only look at any other.
pub struct PolicyEditor {
    /// The name it was opened under, when it exists: a save under another name is a rename.
    pub replaces: Option<String>,
    /// Origin of what is open. Anything but local is shown, not edited — [`Self::readonly`].
    pub origin: PolicyOrigin,
    pub name: Entity<InputState>,
    pub description: Entity<InputState>,
    pub requires: Vec<String>,
    pub filter: FilterRows,
    pub paths: Vec<PathRow>,
    pub env: Vec<EnvRow>,
    pub home_enabled: bool,
    pub home_scratch: bool,
    pub home_path: Entity<InputState>,
    pub home_seed: Entity<TextareaState>,
    pub ensure_args: Entity<TextareaState>,
    pub rules: Vec<RuleRow>,
    pub macos_capabilities: Vec<String>,
    pub macos_raw: Entity<TextareaState>,
    pub windows_capabilities: Vec<String>,
    /// The advanced sections drawn open. Empty at first: they are collapsed by default.
    pub open: Vec<Section>,
    /// What the host wrote the policy as, for the read-only view.
    pub toml: String,
    /// A save is in flight.
    pub saving: bool,
}

impl PolicyEditor {
    /// An empty local policy.
    pub fn blank(window: &mut Window, cx: &mut App) -> Self {
        let mut editor = Self::from_doc(
            "",
            "",
            &PolicyDoc::default(),
            PolicyOrigin::Local,
            None,
            String::new(),
            window,
            cx,
        );
        editor.paths.push(PathRow::new(&blank_path(), window, cx));
        editor
    }

    /// The editor over what the host sent.
    pub fn from_body(fill: &PolicyFill, window: &mut Window, cx: &mut App) -> Self {
        let editable = fill.info.origin.editable();
        Self::from_doc(
            &fill.info.name,
            &fill.info.description,
            &fill.doc,
            fill.info.origin.clone(),
            editable.then(|| fill.info.name.clone()),
            fill.toml.clone(),
            window,
            cx,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn from_doc(
        name: &str,
        description: &str,
        doc: &PolicyDoc,
        origin: PolicyOrigin,
        replaces: Option<String>,
        toml: String,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let line = |window: &mut Window, cx: &mut App, hint: &'static str, text: &str| {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(hint));
            input.update(cx, |state, cx| state.set_value(text, window, cx));
            input
        };
        let home = doc.home_replace.clone().unwrap_or_default();
        Self {
            replaces,
            origin,
            name: line(window, cx, "toolchains/rust", name),
            description: line(window, cx, "What it is for", description),
            requires: doc.requires.clone(),
            filter: FilterRows::new(&doc.filter.clone().unwrap_or_default(), window, cx),
            paths: doc
                .paths
                .iter()
                .map(|grant| PathRow::new(grant, window, cx))
                .collect(),
            env: doc
                .env
                .iter()
                .map(|(key, value)| EnvRow {
                    key: line(window, cx, "NAME", key),
                    value: line(window, cx, "value", value),
                })
                .collect(),
            home_enabled: home.enabled,
            home_scratch: home.auto_scratch,
            home_path: line(
                window,
                cx,
                "Replacement home (optional)",
                home.path.as_deref().unwrap_or(""),
            ),
            home_seed: lines(window, cx, ".gitconfig\n.ssh/known_hosts", &home.seed),
            ensure_args: lines(window, cx, "--no-update-check", &doc.ensure_args),
            rules: doc
                .rules
                .iter()
                .map(|rule| RuleRow {
                    filter: FilterRows::new(&rule.filter, window, cx),
                    paths: rule
                        .paths
                        .iter()
                        .map(|grant| PathRow::new(grant, window, cx))
                        .collect(),
                })
                .collect(),
            macos_capabilities: doc.macos_capabilities.clone(),
            macos_raw: {
                let raw = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .placeholder("(allow \u{2026})")
                        .auto_grow(2, 8)
                });
                raw.update(cx, |state, cx| state.set_value(&doc.macos_raw, window, cx));
                raw
            },
            windows_capabilities: doc.windows_capabilities.clone(),
            open: Vec::new(),
            toml,
            saving: false,
        }
    }

    /// Whether the open policy may be saved over: only a local one, or a new one.
    pub fn readonly(&self) -> bool {
        !self.origin.editable()
    }

    /// Tick or untick `value` on one of the lists. Ticked order is kept.
    pub fn toggle(&mut self, tick: Tick, value: &str) {
        let list = match tick {
            Tick::Requires => &mut self.requires,
            Tick::MacosCapability => &mut self.macos_capabilities,
            Tick::WindowsCapability => &mut self.windows_capabilities,
            Tick::Os(rule) => match self.filter_mut(rule) {
                Some(filter) => &mut filter.os,
                None => return,
            },
            Tick::Arch(rule) => match self.filter_mut(rule) {
                Some(filter) => &mut filter.arch,
                None => return,
            },
        };
        match list.iter().position(|it| it == value) {
            Some(at) => {
                list.remove(at);
            }
            None => list.push(value.to_string()),
        }
    }

    fn filter_mut(&mut self, rule: Option<usize>) -> Option<&mut FilterRows> {
        match rule {
            None => Some(&mut self.filter),
            Some(at) => self.rules.get_mut(at).map(|rule| &mut rule.filter),
        }
    }

    fn paths_mut(&mut self, rule: Option<usize>) -> Option<&mut Vec<PathRow>> {
        match rule {
            None => Some(&mut self.paths),
            Some(at) => self.rules.get_mut(at).map(|rule| &mut rule.paths),
        }
    }

    /// Whether an advanced section is drawn open.
    pub fn is_open(&self, section: Section) -> bool {
        self.open.contains(&section)
    }

    pub fn toggle_section(&mut self, section: Section) {
        match self.open.iter().position(|it| *it == section) {
            Some(at) => {
                self.open.remove(at);
            }
            None => self.open.push(section),
        }
    }

    pub fn add_path(&mut self, rule: Option<usize>, window: &mut Window, cx: &mut App) {
        let row = PathRow::new(&blank_path(), window, cx);
        if let Some(paths) = self.paths_mut(rule) {
            paths.push(row);
        }
    }

    pub fn remove_path(&mut self, rule: Option<usize>, index: usize) {
        if let Some(paths) = self.paths_mut(rule)
            && index < paths.len()
        {
            paths.remove(index);
        }
    }

    /// Step a grant's access to the next of [`ACCESS`] (`matching: false`) or its match to the
    /// next of [`MATCHING`].
    pub fn cycle_path(&mut self, rule: Option<usize>, index: usize, matching: bool) {
        let Some(row) = self.paths_mut(rule).and_then(|paths| paths.get_mut(index)) else {
            return;
        };
        let (value, all) = match matching {
            true => (&mut row.matching, &MATCHING),
            false => (&mut row.access, &ACCESS),
        };
        let at = all.iter().position(|it| *it == value).unwrap_or(0);
        *value = all[(at + 1) % all.len()].to_string();
    }

    pub fn add_env(&mut self, window: &mut Window, cx: &mut App) {
        self.env.push(EnvRow {
            key: cx.new(|cx| InputState::new(window, cx).placeholder("NAME")),
            value: cx.new(|cx| InputState::new(window, cx).placeholder("value")),
        });
    }

    pub fn remove_env(&mut self, index: usize) {
        if index < self.env.len() {
            self.env.remove(index);
        }
    }

    pub fn add_rule(&mut self, window: &mut Window, cx: &mut App) {
        self.rules.push(RuleRow {
            filter: FilterRows::new(&PolicyFilter::default(), window, cx),
            paths: vec![PathRow::new(&blank_path(), window, cx)],
        });
    }

    pub fn remove_rule(&mut self, index: usize) {
        if index < self.rules.len() {
            self.rules.remove(index);
        }
    }

    /// The layer name the name box holds.
    pub fn name_text(&self, cx: &App) -> String {
        self.name.read(cx).value().trim().to_string()
    }

    /// Whether Save has what it needs: a name, and nothing in flight.
    pub fn ready(&self, cx: &App) -> bool {
        !self.readonly() && !self.saving && !self.name_text(cx).is_empty()
    }

    /// What the boxes say, as the structured layer. Empty rows are dropped; a filter or a home
    /// that says nothing is `None`, the shape the host writes no table for.
    pub fn to_doc(&self, cx: &App) -> PolicyDoc {
        let read_paths = |paths: &[PathRow]| -> Vec<PolicyPath> {
            paths
                .iter()
                .map(|row| row.to_path(cx))
                .filter(|grant| !grant.path.is_empty())
                .collect()
        };
        let filter = self.filter.to_filter(cx);
        let home = PolicyHome {
            enabled: self.home_enabled,
            auto_scratch: self.home_scratch,
            path: Some(self.home_path.read(cx).value().trim().to_string())
                .filter(|path| !path.is_empty()),
            seed: read_lines(&self.home_seed, cx),
        };
        PolicyDoc {
            requires: self.requires.clone(),
            filter: (filter != PolicyFilter::default()).then_some(filter),
            paths: read_paths(&self.paths),
            env: self
                .env
                .iter()
                .map(|row| {
                    (
                        row.key.read(cx).value().trim().to_string(),
                        row.value.read(cx).value().to_string(),
                    )
                })
                .filter(|(key, _)| !key.is_empty())
                .collect(),
            home_replace: (home != PolicyHome::default()).then_some(home),
            ensure_args: read_lines(&self.ensure_args, cx),
            rules: self
                .rules
                .iter()
                .map(|rule| PolicyRule {
                    filter: rule.filter.to_filter(cx),
                    paths: read_paths(&rule.paths),
                })
                .collect(),
            macos_capabilities: self.macos_capabilities.clone(),
            macos_raw: self.macos_raw.read(cx).value().to_string(),
            windows_capabilities: self.windows_capabilities.clone(),
        }
    }
}

fn blank_path() -> PolicyPath {
    PolicyPath {
        path: String::new(),
        access: "ro".to_string(),
        matching: "subpath".to_string(),
    }
}

/// `value` when it is one of `all`, else `fallback` — a layer written by hand may say something the
/// editor's cycle does not know, and the row should still draw.
fn pick(all: &[&str], value: &str, fallback: &str) -> String {
    if all.contains(&value) {
        value
    } else {
        fallback
    }
    .to_string()
}

fn lines(
    window: &mut Window,
    cx: &mut App,
    hint: &'static str,
    items: &[String],
) -> Entity<TextareaState> {
    let area = cx.new(|cx| {
        TextareaState::new(window, cx)
            .placeholder(hint)
            .auto_grow(2, 5)
    });
    area.update(cx, |state, cx| {
        state.set_value(items.join("\n"), window, cx)
    });
    area
}

fn read_lines(area: &Entity<TextareaState>, cx: &App) -> Vec<String> {
    area.read(cx)
        .value()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_the_cycle_does_not_know_falls_back() {
        assert_eq!(pick(&ACCESS, "rw", "ro"), "rw");
        assert_eq!(pick(&ACCESS, "weird", "ro"), "ro");
        assert_eq!(pick(&MATCHING, "", "subpath"), "subpath");
    }

    #[test]
    fn only_a_local_policy_is_editable() {
        assert!(PolicyOrigin::Local.editable());
        assert!(!PolicyOrigin::Builtin.editable());
    }
}
