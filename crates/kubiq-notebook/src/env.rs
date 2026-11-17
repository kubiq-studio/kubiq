//! The Python environment resolver's rules: *which interpreter or kernel runs code for this file?*
//!
//! The order and the shapes live here, pure; every question about the machine goes through
//! [`Probe`], which the host answers with real I/O (`kubiq-host/src/env_resolver.rs`) and a test
//! answers from a table. Candidates come in the order the first match wins:
//!
//! 1. **Project environment** — walking up from the notebook's folder to the project root, a
//!    `.venv/` then a `venv/` (`bin/python`, or `Scripts\python.exe` on Windows); then, for a uv
//!    project (`pyproject.toml` + `uv.lock`), the interpreter `uv python find` names.
//! 2. **Machine kernelspecs** — the standard Jupyter search path, listed and never written.
//!
//! A kernelspec is **synthesised** for an interpreter (`<python> -m ipykernel_launcher -f
//! {connection_file}`) rather than installed anywhere. Where the interpreter lacks `ipykernel`,
//! a uv project also gets the "run without installing" candidate: uv layers an ephemeral
//! `ipykernel` over the project's environment and nothing in the project changes.

use serde::{Deserialize, Serialize};

use crate::json::{self, Json};

/// How a kernel wants to be interrupted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterruptMode {
    /// SIGINT to the process group (Unix); the `JPY_INTERRUPT_EVENT` handle on Windows.
    Signal,
    /// `interrupt_request` on the control channel.
    Message,
}

/// A kernelspec: how to launch one kernel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelSpec {
    pub display_name: String,
    pub language: String,
    /// With `{connection_file}` (and maybe `{resource_dir}`) still in it.
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub interrupt_mode: InterruptMode,
}

/// Placeholder Jupyter substitutes with the connection file's path.
pub const CONNECTION_FILE: &str = "{connection_file}";
/// Placeholder Jupyter substitutes with the kernelspec's directory.
pub const RESOURCE_DIR: &str = "{resource_dir}";

impl KernelSpec {
    /// The argv with its placeholders filled.
    pub fn launch_argv(&self, connection_file: &str, resource_dir: &str) -> Vec<String> {
        self.argv
            .iter()
            .map(|a| a.replace(CONNECTION_FILE, connection_file).replace(RESOURCE_DIR, resource_dir))
            .collect()
    }

    /// Read a `kernel.json`.
    pub fn parse(text: &str) -> Option<KernelSpec> {
        let v = json::parse(text).ok()?;
        let argv: Vec<String> = v
            .get("argv")?
            .as_array()?
            .iter()
            .filter_map(|a| a.as_str().map(str::to_string))
            .collect();
        if argv.is_empty() {
            return None;
        }
        let env = match v.get("env") {
            Some(Json::Obj(pairs)) => pairs
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                .collect(),
            _ => Vec::new(),
        };
        Some(KernelSpec {
            display_name: v.get("display_name").and_then(Json::as_str).unwrap_or("kernel").into(),
            language: v.get("language").and_then(Json::as_str).unwrap_or("").into(),
            argv,
            env,
            interrupt_mode: match v.get("interrupt_mode").and_then(Json::as_str) {
                Some("message") => InterruptMode::Message,
                _ => InterruptMode::Signal,
            },
        })
    }
}

/// The in-memory kernelspec for an interpreter: nothing is installed anywhere.
pub fn synthesize(python: &str, label: &str) -> KernelSpec {
    KernelSpec {
        display_name: label.to_string(),
        language: "python".into(),
        argv: vec![
            python.to_string(),
            "-m".into(),
            "ipykernel_launcher".into(),
            "-f".into(),
            CONNECTION_FILE.into(),
        ],
        env: Vec::new(),
        interrupt_mode: InterruptMode::Signal,
    }
}

/// "Run without installing": uv layers an ephemeral `ipykernel` over the project's environment.
pub fn uv_ephemeral(uv: &str, project: &str) -> KernelSpec {
    KernelSpec {
        display_name: "uv run --with ipykernel (nothing installed)".into(),
        language: "python".into(),
        argv: vec![
            uv.to_string(),
            "run".into(),
            "--project".into(),
            project.to_string(),
            "--no-sync".into(),
            "--with".into(),
            "ipykernel".into(),
            "python".into(),
            "-m".into(),
            "ipykernel_launcher".into(),
            "-f".into(),
            CONNECTION_FILE.into(),
        ],
        env: Vec::new(),
        interrupt_mode: InterruptMode::Signal,
    }
}

/// Where a candidate came from — the picker's groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Group {
    /// The project's own environment (`.venv`, `venv`, uv).
    Environment,
    /// A kernelspec on the machine's Jupyter path.
    Machine,
    /// An interpreter the user picked by path.
    Manual,
}

/// One kernel the notebook could run on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// Stable for one resolve: what a start names.
    pub id: String,
    pub label: String,
    pub group: Group,
    /// The interpreter, when the candidate is one (a machine kernelspec may not be Python).
    pub python: Option<String>,
    /// Whether `python -c "import ipykernel"` succeeded; `None` when not asked (not Python).
    pub ipykernel: Option<bool>,
    pub spec: KernelSpec,
}

/// What the resolver asks of the machine.
pub trait Probe {
    fn is_file(&self, path: &str) -> bool;
    /// The entries of a directory, as full paths; empty when it is not one.
    fn list_dir(&self, path: &str) -> Vec<String>;
    fn read(&self, path: &str) -> Option<String>;
    /// `uv python find` run in `project`, bounded by a timeout; `None` without uv or an answer.
    fn uv_python(&self, project: &str) -> Option<String>;
    /// The `uv` executable, when there is one.
    fn uv(&self) -> Option<String>;
    /// Whether `python` imports `ipykernel`; cached by the implementation.
    fn has_ipykernel(&self, python: &str) -> bool;
}

/// The machine's facts the rules need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    pub windows: bool,
    pub macos: bool,
    pub home: Option<String>,
    /// `JUPYTER_PATH`, split.
    pub jupyter_path: Vec<String>,
    /// `%APPDATA%` and `%PROGRAMDATA%` on Windows.
    pub appdata: Option<String>,
    pub programdata: Option<String>,
}

fn join(dir: &str, leaf: &str, windows: bool) -> String {
    let sep = if windows { '\\' } else { '/' };
    if dir.ends_with(sep) || dir.ends_with('/') {
        format!("{dir}{leaf}")
    } else {
        format!("{dir}{sep}{leaf}")
    }
}

/// A virtual environment's interpreter.
pub fn venv_python(venv: &str, windows: bool) -> String {
    if windows {
        join(&join(venv, "Scripts", true), "python.exe", true)
    } else {
        join(&join(venv, "bin", false), "python", false)
    }
}

/// The folders from `start` up to and including `root`; just `root` when `start` is not inside it.
pub fn walk_up(start: &str, root: &str) -> Vec<String> {
    let norm = |s: &str| s.trim_end_matches(['/', '\\']).to_string();
    let (start, root) = (norm(start), norm(root));
    if !start.starts_with(&root) {
        return vec![root];
    }
    let mut out = Vec::new();
    let mut at = start;
    loop {
        out.push(at.clone());
        if at.len() <= root.len() {
            break;
        }
        match at.rfind(['/', '\\']) {
            Some(cut) if cut >= root.len() => at.truncate(cut),
            _ => {
                if out.last() != Some(&root) {
                    out.push(root.clone());
                }
                break;
            }
        }
    }
    out
}

/// The Jupyter data directories' `kernels/` folders, in Jupyter's order: user, `JUPYTER_PATH`,
/// system.
pub fn kernel_dirs(p: &Platform) -> Vec<String> {
    let w = p.windows;
    let mut dirs = Vec::new();
    for entry in &p.jupyter_path {
        dirs.push(join(entry, "kernels", w));
    }
    if w {
        if let Some(app) = &p.appdata {
            dirs.insert(0, join(&join(app, "jupyter", w), "kernels", w));
        }
        if let Some(data) = &p.programdata {
            dirs.push(join(&join(data, "jupyter", w), "kernels", w));
        }
    } else {
        if let Some(home) = &p.home {
            let user = if p.macos {
                join(&join(&join(home, "Library", w), "Jupyter", w), "kernels", w)
            } else {
                join(&join(home, ".local/share/jupyter", w), "kernels", w)
            };
            dirs.insert(0, user);
        }
        dirs.push("/usr/local/share/jupyter/kernels".into());
        dirs.push("/usr/share/jupyter/kernels".into());
    }
    dirs
}

fn leaf(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Every candidate for a notebook in `notebook_dir` of the project at `root`, the default first.
///
/// **Nothing inside the project runs without consent.** A project's `.venv` is the project's own
/// code: probing it for `ipykernel` executes it, and `uv python find` reads the project's own
/// settings. With `run_project` false those candidates are listed by path with `ipykernel: None`
/// (unknown) and uv is not asked; an interpreter outside the project is probed either way.
pub fn resolve(
    probe: &dyn Probe,
    p: &Platform,
    notebook_dir: &str,
    root: &str,
    run_project: bool,
) -> Vec<Candidate> {
    let w = p.windows;
    let mut out: Vec<Candidate> = Vec::new();
    let inside = |path: &str| path.starts_with(root.trim_end_matches(['/', '\\']));
    let python_candidate = |python: String, id: String, label: String, group: Group| {
        let ipykernel = (run_project || !inside(&python)).then(|| probe.has_ipykernel(&python));
        Candidate {
            id,
            spec: synthesize(&python, &label),
            label,
            group,
            python: Some(python),
            ipykernel,
        }
    };
    for dir in walk_up(notebook_dir, root) {
        for name in [".venv", "venv"] {
            let python = venv_python(&join(&dir, name, w), w);
            if probe.is_file(&python) && !out.iter().any(|c| c.python.as_deref() == Some(&python)) {
                let rel = dir.strip_prefix(root).unwrap_or("").trim_start_matches(['/', '\\']);
                let at = if rel.is_empty() { name.to_string() } else { format!("{rel}/{name}") };
                out.push(python_candidate(
                    python,
                    format!("env:{at}"),
                    format!("{at} (project environment)"),
                    Group::Environment,
                ));
            }
        }
    }
    let uv_project = probe.is_file(&join(root, "pyproject.toml", w)) && probe.is_file(&join(root, "uv.lock", w));
    if uv_project {
        if run_project
            && let Some(python) = probe.uv_python(root)
            && !out.iter().any(|c| c.python.as_deref() == Some(&python))
        {
            out.push(python_candidate(
                python,
                "uv:python".into(),
                "uv project interpreter".into(),
                Group::Environment,
            ));
        }
        let lacks = out.iter().all(|c| c.ipykernel != Some(true));
        if lacks && let Some(uv) = probe.uv() {
            out.push(Candidate {
                id: "uv:ephemeral".into(),
                label: "uv run --with ipykernel (nothing installed)".into(),
                group: Group::Environment,
                python: None,
                ipykernel: Some(true),
                spec: uv_ephemeral(&uv, root),
            });
        }
    }
    for dir in kernel_dirs(p) {
        let mut entries = probe.list_dir(&dir);
        entries.sort();
        for entry in entries {
            let file = join(&entry, "kernel.json", w);
            let Some(spec) = probe.read(&file).as_deref().and_then(KernelSpec::parse) else {
                continue;
            };
            let id = format!("spec:{}", leaf(&entry));
            if out.iter().any(|c| c.id == id) {
                continue; // the user directory shadows the system one, as in Jupyter
            }
            out.push(Candidate {
                id,
                label: format!("{} (Jupyter kernel)", spec.display_name),
                group: Group::Machine,
                python: None,
                ipykernel: None,
                spec,
            });
        }
    }
    out
}

/// A manually picked interpreter, as a candidate.
pub fn manual(probe: &dyn Probe, python: &str) -> Candidate {
    Candidate {
        id: format!("manual:{python}"),
        label: format!("{python} (picked)"),
        group: Group::Manual,
        python: Some(python.to_string()),
        ipykernel: Some(probe.has_ipykernel(python)),
        spec: synthesize(python, python),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[derive(Default)]
    struct Fake {
        files: HashSet<String>,
        dirs: HashMap<String, Vec<String>>,
        texts: HashMap<String, String>,
        uv: Option<String>,
        uv_python: Option<String>,
        with_ipykernel: HashSet<String>,
    }

    impl Probe for Fake {
        fn is_file(&self, path: &str) -> bool {
            self.files.contains(path)
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            self.dirs.get(path).cloned().unwrap_or_default()
        }
        fn read(&self, path: &str) -> Option<String> {
            self.texts.get(path).cloned()
        }
        fn uv_python(&self, _: &str) -> Option<String> {
            self.uv_python.clone()
        }
        fn uv(&self) -> Option<String> {
            self.uv.clone()
        }
        fn has_ipykernel(&self, python: &str) -> bool {
            self.with_ipykernel.contains(python)
        }
    }

    fn linux() -> Platform {
        Platform {
            windows: false,
            macos: false,
            home: Some("/home/u".into()),
            jupyter_path: vec![],
            appdata: None,
            programdata: None,
        }
    }

    #[test]
    fn a_project_interpreter_is_not_run_without_consent() {
        let mut f = Fake::default();
        f.files.insert("/p/.venv/bin/python".into());
        f.files.insert("/p/pyproject.toml".into());
        f.files.insert("/p/uv.lock".into());
        f.with_ipykernel.insert("/p/.venv/bin/python".into());
        f.uv_python = Some("/opt/py/bin/python3".into());
        let c = resolve(&f, &linux(), "/p", "/p", false);
        assert_eq!(c[0].id, "env:.venv");
        assert_eq!(c[0].ipykernel, None, "listed by path, not probed");
        assert!(c.iter().all(|c| c.id != "uv:python"), "uv is not asked");
        let c = resolve(&f, &linux(), "/p", "/p", true);
        assert_eq!(c[0].ipykernel, Some(true));
        assert!(c.iter().any(|c| c.id == "uv:python"));
    }

    #[test]
    fn walk_up_stops_at_the_root() {
        assert_eq!(walk_up("/p/a/b", "/p"), vec!["/p/a/b", "/p/a", "/p"]);
        assert_eq!(walk_up("/p", "/p/"), vec!["/p"]);
        assert_eq!(walk_up("/elsewhere", "/p"), vec!["/p"]);
    }

    #[test]
    fn nearest_venv_first_then_uv_then_machine() {
        let mut f = Fake::default();
        f.files.insert("/p/nb/.venv/bin/python".into());
        f.files.insert("/p/venv/bin/python".into());
        f.files.insert("/p/pyproject.toml".into());
        f.files.insert("/p/uv.lock".into());
        f.uv = Some("/usr/bin/uv".into());
        f.uv_python = Some("/p/nb/.venv/bin/python".into());
        f.dirs.insert(
            "/home/u/.local/share/jupyter/kernels".into(),
            vec!["/home/u/.local/share/jupyter/kernels/ir".into()],
        );
        f.texts.insert(
            "/home/u/.local/share/jupyter/kernels/ir/kernel.json".into(),
            r#"{"argv":["R","--slave","-f","{connection_file}"],"display_name":"R","language":"R","interrupt_mode":"message"}"#.into(),
        );
        let c = resolve(&f, &linux(), "/p/nb", "/p", true);
        let ids: Vec<&str> = c.iter().map(|c| c.id.as_str()).collect();
        // Neither has ipykernel, so the uv ephemeral candidate is offered.
        assert_eq!(ids, vec!["env:nb/.venv", "env:venv", "uv:ephemeral", "spec:ir"]);
        assert_eq!(c[0].ipykernel, Some(false));
        assert_eq!(c[3].spec.interrupt_mode, InterruptMode::Message);
        assert_eq!(
            c[0].spec.launch_argv("/r/k.json", ""),
            vec!["/p/nb/.venv/bin/python", "-m", "ipykernel_launcher", "-f", "/r/k.json"]
        );
    }

    #[test]
    fn windows_layout() {
        let mut f = Fake::default();
        f.files.insert(r"C:\p\.venv\Scripts\python.exe".into());
        f.with_ipykernel.insert(r"C:\p\.venv\Scripts\python.exe".into());
        let p = Platform {
            windows: true,
            ..linux()
        };
        let c = resolve(&f, &p, r"C:\p", r"C:\p", true);
        assert_eq!(c[0].python.as_deref(), Some(r"C:\p\.venv\Scripts\python.exe"));
        assert_eq!(c[0].ipykernel, Some(true));
    }
}
