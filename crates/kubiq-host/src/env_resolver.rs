//! Which interpreter or kernel runs code for a file: the environment resolver's machine half.
//!
//! The rules — the candidate order, the venv layouts, the Jupyter search path, the synthesised
//! kernelspec — are [`kubiq_notebook::env`], pure. This file answers its [`Probe`] questions with
//! real I/O: a file test, a directory listing, `uv python find` bounded by [`UV_TIMEOUT`], and the
//! `import ipykernel` probe cached per interpreter for the life of the process. A notebook's kernel
//! picker is the first consumer; a debugger's or a test runner's "the project's interpreter" is the
//! same question and asks it here.
//!
//! Nothing is installed or registered: a kernelspec is never written, and the probe only imports.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub use kubiq_notebook::env::{Candidate, Group, KernelSpec, Platform, Probe};

/// The longest a `uv` or `python` probe may take before it is called a no.
pub const UV_TIMEOUT: Duration = Duration::from_secs(5);

/// The real machine.
#[derive(Default)]
pub struct MachineProbe {
    ipykernel: Mutex<HashMap<String, bool>>,
}

impl MachineProbe {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget what the `ipykernel` probe answered — after an install changed it.
    pub fn forget(&self, python: &str) {
        self.ipykernel.lock().unwrap_or_else(|e| e.into_inner()).remove(python);
    }
}

/// Run `cmd` and return its stdout, or `None` when it fails, exits non-zero or outlives `timeout`.
pub fn run_bounded(mut cmd: Command, timeout: Duration) -> Option<String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    use std::io::Read;
                    let _ = stdout.read_to_string(&mut out);
                }
                return status.success().then_some(out);
            }
            Ok(None) if started.elapsed() < timeout => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// The first `name` on `PATH`.
pub fn which(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    std::env::split_paths(&path)
        .map(|dir| dir.join(&exe))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

impl Probe for MachineProbe {
    fn is_file(&self, path: &str) -> bool {
        Path::new(path).is_file()
    }

    fn list_dir(&self, path: &str) -> Vec<String> {
        std::fs::read_dir(path)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn read(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn uv_python(&self, project: &str) -> Option<String> {
        let uv = self.uv()?;
        let mut cmd = Command::new(uv);
        cmd.args(["python", "find"]).current_dir(project);
        let out = run_bounded(cmd, UV_TIMEOUT)?;
        let line = out.lines().next()?.trim();
        (!line.is_empty()).then(|| line.to_string())
    }

    fn uv(&self) -> Option<String> {
        which("uv")
    }

    fn has_ipykernel(&self, python: &str) -> bool {
        if let Some(known) = self.ipykernel.lock().unwrap_or_else(|e| e.into_inner()).get(python) {
            return *known;
        }
        let mut cmd = Command::new(python);
        cmd.args(["-c", "import ipykernel"]);
        let found = run_bounded(cmd, UV_TIMEOUT).is_some();
        self.ipykernel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(python.to_string(), found);
        found
    }
}

/// This machine's facts, from the environment.
pub fn platform() -> Platform {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    Platform {
        windows: cfg!(windows),
        macos: cfg!(target_os = "macos"),
        home: var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }),
        jupyter_path: std::env::var_os("JUPYTER_PATH")
            .map(|v| {
                std::env::split_paths(&v)
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default(),
        appdata: var("APPDATA"),
        programdata: var("PROGRAMDATA"),
    }
}

/// Every candidate for a notebook in `notebook_dir`, inside the project at `root`. Only with
/// `run_project` (a person's consent) is anything inside the project executed to probe it.
pub fn resolve(probe: &MachineProbe, notebook_dir: &Path, root: &Path, run_project: bool) -> Vec<Candidate> {
    kubiq_notebook::env::resolve(
        probe,
        &platform(),
        &notebook_dir.to_string_lossy(),
        &root.to_string_lossy(),
        run_project,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_fake_venv_is_found_and_probed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("kubiq-env-{}", std::process::id()));
        let bin = dir.join("nb/.venv/bin");
        std::fs::create_dir_all(&bin).unwrap();
        // A "python" that imports everything: exits 0.
        let python = bin.join("python");
        std::fs::write(&python, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
        let probe = MachineProbe::new();
        let unprobed = resolve(&probe, &dir.join("nb"), &dir, false);
        assert_eq!(unprobed[0].ipykernel, None, "nothing in the project runs without consent");
        let found = resolve(&probe, &dir.join("nb"), &dir, true);
        assert_eq!(found[0].id, "env:nb/.venv");
        assert_eq!(found[0].ipykernel, Some(true));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
