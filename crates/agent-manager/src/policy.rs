//! The sandbox policies an isolated run can be composed against — isol8's profile *layers*, with a
//! place to keep the user's own.
//!
//! isol8 knows built-in layers, the ones under `~/.config/isol8/profiles`, and any directory a
//! caller names as a profile path. It has no editor, no importer and no notion of a description or
//! a default; this module adds exactly that, over the one directory [`isolate::plan`] already puts
//! on the profile path: `<state_dir>/profiles`.
//!
//! - **Local** layers sit in that directory and are the only ones saved over.
//! - **Imported** layers sit under `profiles/imported/<repo>/`, copied there by [`PolicyStore::import`]
//!   from a git repository, read-only (a re-import replaces the folder).
//! - Everything Kubiq itself knows about a layer — its description, whether it is a default, where an
//!   import came from — is in `<state_dir>/policies.toml`, because a layer file has no field for
//!   it (isol8 rejects unknown keys).
//!
//! The types here are plain: the embedder turns them into whatever its own wire speaks.
//!
//! [`isolate::plan`]: crate::isolate::plan

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// An isol8 layer, as parsed. Re-exported so an embedder converts it without a dependency of its
/// own on the engine.
pub use isol8::Profile as Layer;

/// The folder under `profiles/` imports are copied into.
const IMPORTED: &str = "imported";

/// Where a policy comes from, which decides what may be done with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Embedded in isol8. Read-only.
    Builtin,
    /// In `<state_dir>/profiles`, outside `imported/`. Editable and deletable.
    Local,
    /// Copied from a git repository. Read-only, deletable.
    Imported {
        /// The clone URL as it was given, or empty when the record of it is lost.
        source: String,
    },
    /// In isol8's own user folder, outside this store. Read-only.
    User,
}

/// One row of the policy list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    /// The layer name — its path relative to its folder, no extension, `/`-separated.
    pub name: String,
    /// Where it comes from.
    pub origin: Origin,
    /// What it is for; empty when nobody said.
    pub description: String,
    /// Whether every isolated run gets it.
    pub default: bool,
}

/// `<state_dir>/policies.toml`: what the layer files cannot say.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Meta {
    /// Layer names every isolated run gets.
    #[serde(default)]
    defaults: BTreeSet<String>,
    /// A line per layer name.
    #[serde(default)]
    descriptions: BTreeMap<String, String>,
    /// Import folder (`imported/<slug>`'s slug) to the clone URL it came from.
    #[serde(default)]
    imports: BTreeMap<String, String>,
}

/// The policy repository under one state directory. Holds no state of its own: every call reads
/// the disk, so two handles, or a hand edit, always agree.
#[derive(Debug, Clone)]
pub struct PolicyStore {
    state_dir: PathBuf,
}

/// Whether `name` is a layer name this store will write: `/`-separated segments of
/// `[A-Za-z0-9._-]`, none empty and none made only of dots, so it can never leave the folder.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('/').all(|segment| {
            !segment.is_empty()
                && !segment.chars().all(|c| c == '.')
                && segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        })
}

fn is_imported(name: &str) -> bool {
    name.strip_prefix(IMPORTED)
        .is_some_and(|rest| rest.starts_with('/'))
}

impl PolicyStore {
    /// A store over `state_dir` — the same directory [`IsolateOptions::state_dir`] names.
    ///
    /// [`IsolateOptions::state_dir`]: crate::isolate::IsolateOptions::state_dir
    pub fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
        }
    }

    /// The folder local and imported layers live in.
    pub fn profiles_dir(&self) -> PathBuf {
        self.state_dir.join("profiles")
    }

    fn meta_path(&self) -> PathBuf {
        self.state_dir.join("policies.toml")
    }

    fn meta(&self) -> Meta {
        fs::read_to_string(self.meta_path())
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn write_meta(&self, meta: &Meta) -> Result<()> {
        fs::create_dir_all(&self.state_dir)?;
        let text = toml::to_string_pretty(meta).context("serializing policies.toml")?;
        fs::write(self.meta_path(), text)
            .with_context(|| format!("writing {}", self.meta_path().display()))
    }

    fn file_of(&self, name: &str) -> PathBuf {
        let mut path = self.profiles_dir();
        path.extend(name.split('/'));
        let mut file = path.into_os_string();
        file.push(".toml");
        PathBuf::from(file)
    }

    fn registry(&self) -> Result<isol8::profile::LayerRegistry> {
        let dir = self.profiles_dir();
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        isol8::profile::LayerRegistry::load(&[dir.to_string_lossy().into_owned()])
            .context("loading the policy layers")
    }

    fn info_of(name: &str, source: &isol8::profile::LayerSource, meta: &Meta) -> Info {
        use isol8::profile::LayerSource;
        let origin = match source {
            LayerSource::Builtin => Origin::Builtin,
            LayerSource::UserConfig => Origin::User,
            LayerSource::ProfilePath(_) if is_imported(name) => {
                let slug = name.split('/').nth(1).unwrap_or_default();
                Origin::Imported {
                    source: meta.imports.get(slug).cloned().unwrap_or_default(),
                }
            }
            LayerSource::ProfilePath(_) => Origin::Local,
        };
        Info {
            name: name.to_string(),
            origin,
            description: meta.descriptions.get(name).cloned().unwrap_or_default(),
            default: meta.defaults.contains(name),
        }
    }

    /// Every policy this store can resolve, sorted by name.
    pub fn list(&self) -> Result<Vec<Info>> {
        let registry = self.registry()?;
        let meta = self.meta();
        Ok(registry
            .list()
            .iter()
            .map(|(name, source)| Self::info_of(name, source, &meta))
            .collect())
    }

    /// One policy: its row, the parsed layer and the TOML it serializes to.
    pub fn read(&self, name: &str) -> Result<(Info, Layer, String)> {
        let registry = self.registry()?;
        let (Some(layer), Some(source)) = (registry.get(name), registry.source(name)) else {
            bail!("no policy named '{name}'");
        };
        let info = Self::info_of(name, source, &self.meta());
        let toml = isol8::profile::format_layer(layer).context("serializing the policy")?;
        Ok((info, layer.clone(), toml))
    }

    /// Write a local policy. `replaces` is the name it was opened under, when this is a rename.
    ///
    /// Refused when `name` is not a [`valid_name`] outside `imported/`, when it names a policy that
    /// is not local, when it names a *different* local one than `replaces`, or when its `requires`
    /// do not resolve.
    pub fn save(
        &self,
        name: &str,
        replaces: Option<&str>,
        description: &str,
        layer: &Layer,
    ) -> Result<()> {
        if !valid_name(name) || is_imported(name) {
            bail!(
                "'{name}' is not a usable policy name: use segments of letters, digits, '.', '_' and '-' separated by '/'"
            );
        }
        let registry = self.registry()?;
        let mut meta = self.meta();
        let existing = registry
            .source(name)
            .map(|source| Self::info_of(name, source, &meta));
        if let Some(existing) = &existing {
            if existing.origin != Origin::Local {
                bail!(
                    "'{name}' is a {} policy and cannot be overwritten",
                    describe(&existing.origin)
                );
            }
            // A new policy (no `replaces`) or a rename never lands on an existing name: only an
            // edit of the policy itself writes over it.
            if replaces != Some(name) {
                bail!("a policy named '{name}' already exists");
            }
        }
        let old = replaces.filter(|old| *old != name);
        if let Some(old) = old {
            let held = registry
                .source(old)
                .map(|source| Self::info_of(old, source, &meta));
            if held.is_none_or(|held| held.origin != Origin::Local) {
                bail!("'{old}' is not a local policy and cannot be renamed");
            }
        }

        // The graph it would join, with the old name gone and this one in: an unknown `requires`
        // or a cycle is found here rather than at the next launch.
        let mut all = registry.profiles();
        if let Some(old) = old {
            all.remove(old);
        }
        all.insert(name.to_string(), layer.clone());
        isol8::profile::resolve_requires(&[name.to_string()], &all)
            .context("the policy's requires do not resolve")?;

        let body = isol8::profile::format_layer(layer).context("serializing the policy")?;
        let file = self.file_of(name);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&file, body).with_context(|| format!("writing {}", file.display()))?;

        if let Some(old) = old {
            self.remove_file(old)?;
            if meta.defaults.remove(old) {
                meta.defaults.insert(name.to_string());
            }
            meta.descriptions.remove(old);
        }
        if description.trim().is_empty() {
            meta.descriptions.remove(name);
        } else {
            meta.descriptions
                .insert(name.to_string(), description.trim().to_string());
        }
        self.write_meta(&meta)
    }

    /// Copy any policy to a local one named `new_name`. Never overwrites: a name any policy
    /// already holds is refused.
    pub fn clone_policy(&self, name: &str, new_name: &str) -> Result<()> {
        if !valid_name(new_name) || is_imported(new_name) {
            bail!("'{new_name}' is not a usable policy name");
        }
        let registry = self.registry()?;
        if registry.get(new_name).is_some() {
            bail!("a policy named '{new_name}' already exists");
        }
        let Some(layer) = registry.get(name) else {
            bail!("no policy named '{name}'");
        };
        let body = isol8::profile::format_layer(layer).context("serializing the policy")?;
        let file = self.file_of(new_name);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&file, body).with_context(|| format!("writing {}", file.display()))?;
        let mut meta = self.meta();
        if let Some(description) = meta.descriptions.get(name).cloned() {
            meta.descriptions.insert(new_name.to_string(), description);
            self.write_meta(&meta)?;
        }
        Ok(())
    }

    /// Remove a local or imported policy, and what the metadata held about it.
    pub fn delete(&self, name: &str) -> Result<()> {
        let registry = self.registry()?;
        let mut meta = self.meta();
        let Some(source) = registry.source(name) else {
            bail!("no policy named '{name}'");
        };
        let info = Self::info_of(name, source, &meta);
        if !matches!(info.origin, Origin::Local | Origin::Imported { .. }) {
            bail!(
                "'{name}' is a {} policy and cannot be deleted",
                describe(&info.origin)
            );
        }
        self.remove_file(name)?;
        meta.defaults.remove(name);
        meta.descriptions.remove(name);
        if is_imported(name) {
            let slug = name.split('/').nth(1).unwrap_or_default();
            if !self.profiles_dir().join(IMPORTED).join(slug).exists() {
                meta.imports.remove(slug);
            }
        }
        self.write_meta(&meta)
    }

    /// Remove a layer's file and every folder it leaves empty, up to the profiles folder.
    fn remove_file(&self, name: &str) -> Result<()> {
        let file = self.file_of(name);
        fs::remove_file(&file).with_context(|| format!("removing {}", file.display()))?;
        let root = self.profiles_dir();
        let mut dir = file.parent().map(Path::to_path_buf);
        while let Some(current) = dir {
            if current == root || fs::remove_dir(&current).is_err() {
                break;
            }
            dir = current.parent().map(Path::to_path_buf);
        }
        Ok(())
    }

    /// Mark a policy as a default, or not. A policy that does not resolve cannot become one.
    pub fn set_default(&self, name: &str, default: bool) -> Result<()> {
        let mut meta = self.meta();
        if default {
            if self.registry()?.get(name).is_none() {
                bail!("no policy named '{name}'");
            }
            meta.defaults.insert(name.to_string());
        } else {
            meta.defaults.remove(name);
        }
        self.write_meta(&meta)
    }

    /// The names marked as defaults, sorted. Not checked against what resolves: pass them through
    /// [`Self::resolving`].
    pub fn defaults(&self) -> Vec<String> {
        self.meta().defaults.into_iter().collect()
    }

    /// Split `names` into the ones that resolve and the ones that no longer do, order kept and
    /// duplicates dropped.
    pub fn resolving(&self, names: &[String]) -> (Vec<String>, Vec<String>) {
        let known: BTreeSet<String> = match self.list() {
            Ok(list) => list.into_iter().map(|info| info.name).collect(),
            Err(_) => return (Vec::new(), names.to_vec()),
        };
        let mut kept: Vec<String> = Vec::new();
        let mut dropped = Vec::new();
        for name in names {
            if known.contains(name) {
                if !kept.contains(name) {
                    kept.push(name.clone());
                }
            } else {
                dropped.push(name.clone());
            }
        }
        (kept, dropped)
    }

    /// Copy every valid layer of a git repository into `profiles/imported/<repo>/`, replacing a
    /// previous import of the same repository, and return the names written.
    ///
    /// The layers are the `*.toml` files under the repository's `profiles/` folder, or under its
    /// root when it has none; a file that does not parse as a layer is skipped. Slow — it runs
    /// `git clone` — so the caller keeps it off any thread that must stay responsive.
    pub fn import(&self, url: &str, git_ref: Option<&str>) -> Result<Vec<String>> {
        let url = url.trim();
        if url.is_empty() {
            bail!("a repository URL is needed");
        }
        let temp = self.state_dir.join(format!(
            "import-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        fs::create_dir_all(&temp)?;
        let outcome = self.import_from(url, git_ref, &temp);
        let _ = fs::remove_dir_all(&temp);
        outcome
    }

    fn import_from(&self, url: &str, git_ref: Option<&str>, temp: &Path) -> Result<Vec<String>> {
        let checkout = temp.join("repo");
        let mut git = Command::new("git");
        git.arg("clone").arg("--depth").arg("1");
        if let Some(git_ref) = git_ref.filter(|it| !it.trim().is_empty()) {
            git.arg("--branch").arg(git_ref.trim());
        }
        // A prompt for credentials has no terminal to land on: fail instead of hanging.
        let output = git
            .arg("--")
            .arg(url)
            .arg(&checkout)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .output()
            .context("running git — is it installed?")?;
        if !output.status.success() {
            bail!(
                "git clone failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let root = if checkout.join("profiles").is_dir() {
            checkout.join("profiles")
        } else {
            checkout.clone()
        };
        let mut found: Vec<(String, PathBuf)> = Vec::new();
        for entry in walkdir::WalkDir::new(&root)
            .into_iter()
            .filter_entry(|entry| entry.file_name() != ".git")
            .filter_map(|entry| entry.ok())
        {
            let path = entry.path();
            if !entry.file_type().is_file() || path.extension().is_none_or(|e| e != "toml") {
                continue;
            }
            let Ok(relative) = path.strip_prefix(&root) else {
                continue;
            };
            let name = relative
                .with_extension("")
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let parses = fs::read_to_string(path)
                .ok()
                .is_some_and(|text| toml::from_str::<Layer>(&text).is_ok());
            if valid_name(&name) && parses {
                found.push((name, path.to_path_buf()));
            } else {
                tracing::warn!(file = %path.display(), "not a policy layer; skipped on import");
            }
        }
        if found.is_empty() {
            bail!("no policy layers found in {url}");
        }
        found.sort();

        let mut meta = self.meta();
        let slug = self.slug_for(url, &meta);
        let target = self.profiles_dir().join(IMPORTED).join(&slug);
        if target.exists() {
            fs::remove_dir_all(&target)
                .with_context(|| format!("replacing {}", target.display()))?;
        }
        let mut names = Vec::new();
        for (name, from) in found {
            let full = format!("{IMPORTED}/{slug}/{name}");
            let to = self.file_of(&full);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&from, &to).with_context(|| format!("writing {}", to.display()))?;
            names.push(full);
        }
        meta.imports.insert(slug, url.to_string());
        self.write_meta(&meta)?;
        Ok(names)
    }

    /// The folder name an import of `url` goes under: the repository's own name, numbered when a
    /// different repository already holds it.
    fn slug_for(&self, url: &str, meta: &Meta) -> String {
        let last = url
            .trim_end_matches('/')
            .rsplit(['/', ':'])
            .next()
            .unwrap_or_default();
        let last = last.strip_suffix(".git").unwrap_or(last);
        let mut base: String = last
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '-') {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        if base.is_empty() {
            base = "repo".to_string();
        }
        let mut slug = base.clone();
        let mut n = 1;
        while meta.imports.get(&slug).is_some_and(|held| held != url) {
            n += 1;
            slug = format!("{base}-{n}");
        }
        slug
    }
}

fn describe(origin: &Origin) -> &'static str {
    match origin {
        Origin::Builtin => "built-in",
        Origin::Local => "local",
        Origin::Imported { .. } => "imported",
        Origin::User => "user-folder",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(path: &str) -> Layer {
        toml::from_str(&format!(
            "paths = [{{ path = \"{path}\", access = \"ro\" }}]"
        ))
        .unwrap()
    }

    fn store() -> (tempfile::TempDir, PolicyStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = PolicyStore::new(dir.path());
        (dir, store)
    }

    fn origin_of(store: &PolicyStore, name: &str) -> Option<Origin> {
        store
            .list()
            .unwrap()
            .into_iter()
            .find(|info| info.name == name)
            .map(|info| info.origin)
    }

    #[test]
    fn names_are_safe_relative_paths() {
        for ok in ["a", "team/rust", "a.b_c-d/e"] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in [
            "", "..", "a/../b", "/abs", "a//b", "a/", "a b", ".", "a/./b", "é",
        ] {
            assert!(!valid_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn lists_builtins_and_saves_reads_renames_deletes() {
        let (_dir, store) = store();
        assert_eq!(origin_of(&store, "base"), Some(Origin::Builtin));

        store
            .save("team/mine", None, "my grants", &layer("/opt/x"))
            .unwrap();
        assert_eq!(origin_of(&store, "team/mine"), Some(Origin::Local));
        let (info, read, toml) = store.read("team/mine").unwrap();
        assert_eq!(info.description, "my grants");
        assert_eq!(read.paths[0].path, "/opt/x");
        assert!(toml.contains("/opt/x"));

        // Edit in place, then rename: the default flag and the description follow.
        store
            .save("team/mine", Some("team/mine"), "v2", &layer("/opt/y"))
            .unwrap();
        store.set_default("team/mine", true).unwrap();
        store
            .save("renamed", Some("team/mine"), "v2", &layer("/opt/y"))
            .unwrap();
        assert_eq!(origin_of(&store, "team/mine"), None);
        assert!(
            !store.profiles_dir().join("team").exists(),
            "empty folder pruned"
        );
        assert_eq!(store.defaults(), vec!["renamed".to_string()]);
        assert_eq!(store.read("renamed").unwrap().0.description, "v2");

        store.delete("renamed").unwrap();
        assert_eq!(origin_of(&store, "renamed"), None);
        assert!(store.defaults().is_empty());
    }

    #[test]
    fn refuses_what_it_must() {
        let (_dir, store) = store();
        assert!(store.save("../evil", None, "", &layer("/a")).is_err());
        assert!(store.save("imported/x", None, "", &layer("/a")).is_err());
        assert!(
            store.save("base", None, "", &layer("/a")).is_err(),
            "builtin"
        );
        assert!(store.delete("base").is_err());
        assert!(store.set_default("nope", true).is_err());
        assert!(store.read("nope").is_err());

        store.save("one", None, "", &layer("/a")).unwrap();
        store.save("two", None, "", &layer("/b")).unwrap();
        assert!(
            store.save("two", Some("one"), "", &layer("/b")).is_err(),
            "collision"
        );
        assert!(
            store.save("two", None, "", &layer("/c")).is_err(),
            "a new policy never overwrites"
        );

        let mut needs = layer("/a");
        needs.requires = vec!["does/not-exist".into()];
        assert!(store.save("bad", None, "", &needs).is_err());
        needs.requires = vec!["bad".into()];
        assert!(store.save("bad", None, "", &needs).is_err(), "cycle");
    }

    #[test]
    fn clone_never_overwrites() {
        let (_dir, store) = store();
        store.clone_policy("toolchains/rust", "my-rust").unwrap();
        assert_eq!(origin_of(&store, "my-rust"), Some(Origin::Local));
        assert!(store.clone_policy("toolchains/rust", "my-rust").is_err());
        assert!(store.clone_policy("toolchains/rust", "base").is_err());
        assert!(store.clone_policy("missing", "x").is_err());
    }

    #[test]
    fn resolving_drops_unknown_and_duplicates() {
        let (_dir, store) = store();
        store.save("here", None, "", &layer("/a")).unwrap();
        let names: Vec<String> = ["base", "gone", "here", "base"].map(String::from).into();
        let (kept, dropped) = store.resolving(&names);
        assert_eq!(kept, ["base", "here"]);
        assert_eq!(dropped, ["gone"]);
    }

    #[test]
    fn imports_a_git_repository() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let repo = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let ok = Command::new("git")
                .args(["-c", "user.email=t@t", "-c", "user.name=t"])
                .args(args)
                .current_dir(repo.path())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&["init", "-q"]);
        fs::create_dir_all(repo.path().join("profiles/lang")).unwrap();
        fs::write(
            repo.path().join("profiles/lang/zig.toml"),
            "paths = [{ path = \"/opt/zig\", access = \"ro\" }]",
        )
        .unwrap();
        fs::write(repo.path().join("profiles/broken.toml"), "nonsense = 1").unwrap();
        fs::write(repo.path().join("profiles/notes.txt"), "hi").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "init"]);

        let (_dir, store) = store();
        let url = format!("file://{}", repo.path().display());
        let names = store.import(&url, None).unwrap();
        let slug = names[0].split('/').nth(1).unwrap().to_string();
        assert_eq!(names, vec![format!("imported/{slug}/lang/zig")]);
        assert_eq!(
            origin_of(&store, &names[0]),
            Some(Origin::Imported {
                source: url.clone()
            })
        );
        assert!(
            store.save(&names[0], None, "", &layer("/a")).is_err(),
            "read-only"
        );

        // A re-import replaces the folder; deleting the last layer forgets the source.
        assert_eq!(store.import(&url, None).unwrap(), names);
        store.delete(&names[0]).unwrap();
        assert_eq!(origin_of(&store, &names[0]), None);
        assert!(store.meta().imports.is_empty());

        assert!(store.import(&format!("{url}-missing"), None).is_err());
    }
}
