//! Where Kubiq's config root is, and how it is found.
//!
//! Every store resolves against one directory, and that directory has to be movable: a development
//! run must not touch the accounts, catalogue and credentials a user works with all day. The order
//! is a flag, then the environment, then the nearest `kubiq.toml` walking up, then the default.
//!
//! `kubiq.toml` is a **bootstrap file, not a settings file** — it says where the settings are, and
//! nothing else. The discipline that keeps it useful is that it never grows a second answer to a
//! question a store already answers.
//!
//! Modelled on `crates/agent-manager/src/settings.rs`, which sets the convention this follows:
//! explicit argument, then environment variable, then default.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

/// What a `kubiq.toml` is allowed to say. One key, deliberately.
#[derive(Debug, Deserialize)]
struct Bootstrap {
    config_root: Option<String>,
}

/// Which of the four answers was taken. The status bar says so when it is not the default, because
/// a config root you cannot see is a foot-gun.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RootSource {
    /// `--config-root`, the highest.
    Flag,
    /// `KUBIQ_CONFIG_DIR`.
    Env,
    /// The `kubiq.toml` at this path said so.
    Bootstrap(PathBuf),
    /// `~/.config/kubiq`, which is where a normal run lands.
    Default,
}

/// The resolved root, and where the answer came from.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConfigRoot {
    pub path: PathBuf,
    pub source: RootSource,
}

impl ConfigRoot {
    /// Whether this is the ordinary `~/.config/kubiq`. The interface is told, so the status bar can
    /// say when a run is pointed somewhere else.
    pub fn is_default(&self) -> bool {
        self.source == RootSource::Default
    }
}

/// The file a bootstrap is read from, walking up.
pub const BOOTSTRAP: &str = "kubiq.toml";

/// The pre-rename (`ubiq`) bootstrap file name, still honoured where `kubiq.toml` is absent.
const LEGACY_BOOTSTRAP: &str = "ubiq.toml";

/// `dir/name`, unless only the pre-rename `dir/legacy` exists — then that one. The one place the
/// `ubiq` -> `kubiq` rename keeps working for existing installs: the user config dir, a project's
/// `.kubiq/`, a bootstrap file. Nothing is moved or created; a fresh path is always the new name.
pub fn prefer_legacy(dir: &Path, name: &str, legacy: &str) -> PathBuf {
    let new = dir.join(name);
    let old = dir.join(legacy);
    if !new.exists() && old.exists() { old } else { new }
}

/// `~/.config/kubiq` on every platform, matching the convention the harness library already sets
/// (or the legacy `~/.config/ubiq` when only that exists).
pub fn default_config_root() -> Option<PathBuf> {
    directories::BaseDirs::new()
        .map(|base| prefer_legacy(&base.home_dir().join(".config"), "kubiq", "ubiq"))
}

/// The nearest `kubiq.toml`, walking up from `cwd`.
///
/// The ascent stops after checking a directory that holds a `.git` entry — that directory is still
/// checked — so a bootstrap belongs to the repository it sits in and cannot be picked up from
/// somewhere further out.
pub fn discover_bootstrap(cwd: &Path) -> Option<PathBuf> {
    let mut current = Some(cwd.to_path_buf());
    while let Some(dir) = current {
        let candidate = prefer_legacy(&dir, BOOTSTRAP, LEGACY_BOOTSTRAP);
        if candidate.is_file() {
            return Some(candidate);
        }
        if dir.join(".git").exists() {
            break;
        }
        current = dir.parent().map(Path::to_path_buf);
    }
    None
}

/// Resolve the config root.
///
/// `env` is a parameter rather than something this reads, so precedence can be tested without
/// mutating the process's environment. [`resolve_from_env`] is the thin wrapper that reads it.
///
/// A `kubiq.toml` that cannot be parsed, or that names no `config_root`, is an **error rather than a
/// fallback**. Quietly falling back to the user's real config directory from a broken bootstrap is
/// exactly the trap this whole mechanism exists to avoid.
pub fn resolve(flag: Option<&Path>, env: Option<&Path>, cwd: &Path) -> Result<ConfigRoot> {
    if let Some(path) = flag {
        return Ok(ConfigRoot {
            path: absolute(cwd, path),
            source: RootSource::Flag,
        });
    }

    if let Some(path) = env.filter(|p| !p.as_os_str().is_empty()) {
        return Ok(ConfigRoot {
            path: absolute(cwd, path),
            source: RootSource::Env,
        });
    }

    if let Some(file) = discover_bootstrap(cwd) {
        let raw = std::fs::read_to_string(&file)
            .with_context(|| format!("reading {}", file.display()))?;
        let bootstrap: Bootstrap =
            toml::from_str(&raw).with_context(|| format!("parsing {}", file.display()))?;
        let named = bootstrap.config_root.ok_or_else(|| {
            anyhow!(
                "{} names no `config_root`. A bootstrap file exists to say where the config root \
                 is; remove the file or give it one.",
                file.display()
            )
        })?;
        // Relative to the bootstrap's own directory, which is what makes a checked-in
        // `config_root = "_data/config"` mean the same thing wherever the repository is cloned.
        let base = file.parent().unwrap_or(cwd);
        return Ok(ConfigRoot {
            path: absolute(base, Path::new(&named)),
            source: RootSource::Bootstrap(file),
        });
    }

    let path = default_config_root()
        .ok_or_else(|| anyhow!("could not determine the home directory for this OS"))?;
    Ok(ConfigRoot {
        path,
        source: RootSource::Default,
    })
}

/// [`resolve`], reading `KUBIQ_CONFIG_DIR` for the environment step.
pub fn resolve_from_env(flag: Option<&Path>, cwd: &Path) -> Result<ConfigRoot> {
    let env = std::env::var_os("KUBIQ_CONFIG_DIR")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("UBIQ_CONFIG_DIR")) // legacy name
        .map(PathBuf::from);
    resolve(flag, env.as_deref(), cwd)
}

/// Make `path` absolute against `base`, without requiring it to exist.
///
/// Deliberately not `canonicalize`: the config root is very often the directory this run is about
/// to create, and canonicalising a path that is not there yet fails.
fn absolute(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}
