//! Sandbox policies — the isol8 profile layers an isolated agent runs under.
//!
//! A policy is one isol8 **layer**: a TOML file of path grants, environment defaults and home
//! rules, named by its path relative to the folder it lives in (`toolchains/rust`). The host keeps
//! a local policy repository under `<config root>/isol8/profiles/`; the built-in layers isol8
//! embeds and the ones imported from a remote git repository sit beside it, read-only.
//!
//! The interface never parses a layer itself: it edits a [`PolicyDoc`], the structured mirror of
//! isol8's `Profile`, and the host turns it back into TOML. Enum-valued fields travel as the very
//! strings isol8 writes (`"ro"`, `"subpath"`, `"mach-lookup"`), so this crate carries no isol8
//! dependency and the vocabulary is isol8's own.

use serde::{Deserialize, Serialize};

/// Where a policy comes from, which decides what may be done with it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyOrigin {
    /// Embedded in isol8 itself. Read-only: clone it to change it.
    Builtin,
    /// Written by the user into Kubiq's local policy repository. Editable and deletable.
    Local,
    /// Copied from a remote repository by [`crate::messages::Message::ImportPolicies`]. Read-only
    /// (a re-import overwrites it); deletable.
    Imported {
        /// The repository's clone URL, as the user typed it.
        source: String,
    },
    /// Found in isol8's own user folder (`~/.config/isol8/profiles`), outside Kubiq. Read-only.
    User,
}

impl PolicyOrigin {
    /// Whether the policy editor may save over it.
    pub fn editable(&self) -> bool {
        matches!(self, PolicyOrigin::Local)
    }

    /// Whether [`crate::messages::Message::DeletePolicy`] may remove it.
    pub fn deletable(&self) -> bool {
        matches!(self, PolicyOrigin::Local | PolicyOrigin::Imported { .. })
    }
}

/// One row of the policy list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyInfo {
    /// The layer name, e.g. `toolchains/rust` — what a definition stores and isol8 resolves.
    pub name: String,
    pub origin: PolicyOrigin,
    /// A line saying what the policy is for. Kubiq's own metadata: isol8 layers carry none.
    #[serde(default)]
    pub description: String,
    /// Whether every isolated agent runs under it, on top of whatever its definition picks.
    #[serde(default)]
    pub default: bool,
}

/// One filesystem grant: isol8's `PathGrant`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyPath {
    /// Absolute, `~`-prefixed (the effective home) or containing `#HOME` (the real home).
    pub path: String,
    /// `ro`, `rw`, `none` (an explicit deny) or `metadata` (stat only).
    pub access: String,
    /// `subpath` (the default), `literal`, `prefix` or `regex`.
    pub matching: String,
}

/// isol8's `ProfileFilter`: AND across fields, an empty list is no constraint.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyFilter {
    /// `macos`, `linux`, `windows`.
    pub os: Vec<String>,
    /// `x86_64`, `aarch64`, …
    pub arch: Vec<String>,
    /// Basenames of the command a layer applies to; non-empty makes it auto-selectable.
    pub executables: Vec<String>,
}

/// isol8's `HomeReplace`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyHome {
    pub enabled: bool,
    pub auto_scratch: bool,
    pub path: Option<String>,
    /// Real-home entries copied read-only into the replacement home.
    pub seed: Vec<String>,
}

/// One `[[policies]]` block: a conditional set of grants inside a layer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyRule {
    pub filter: PolicyFilter,
    pub paths: Vec<PolicyPath>,
}

/// The structured mirror of an isol8 layer (`isol8::Profile`), in the order isol8 documents it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDoc {
    /// Layers this one builds on (`requires`, alias `extends`), resolved first.
    pub requires: Vec<String>,
    /// The layer's own gate. `None` applies everywhere.
    pub filter: Option<PolicyFilter>,
    /// Filesystem grants.
    pub paths: Vec<PolicyPath>,
    /// Environment defaults, `(name, value)`, in display order.
    pub env: Vec<(String, String)>,
    pub home_replace: Option<PolicyHome>,
    /// `rewrite.ensure_args`: arguments inserted after the command if absent.
    pub ensure_args: Vec<String>,
    /// Conditional grant blocks.
    pub rules: Vec<PolicyRule>,
    /// `macos.capabilities`, kebab-case.
    pub macos_capabilities: Vec<String>,
    /// `macos.raw`: verbatim SBPL.
    pub macos_raw: String,
    /// `windows.capabilities`, kebab-case.
    pub windows_capabilities: Vec<String>,
}
