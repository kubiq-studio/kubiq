//! Which interpreter a cell runs under, and how it is invoked, by the **target's** operating
//! system — never the interface's. Pure: the worker asks the host which programs exist and runs
//! the [`Invocation`] this answers.
//!
//! - The fence language decides: `sh`, `bash`, `zsh`, `fish`, `dash` are themselves;
//!   `powershell`, `pwsh`, `ps1` are PowerShell (`pwsh` preferred over Windows PowerShell 5.1);
//!   `cmd`, `bat`, `batch` are `cmd`. `shell=` overrides the language.
//! - Anything else (a bare fence, `console`, `shell`) is the target's default: `sh` on Unix,
//!   PowerShell on Windows.
//! - Every invocation is non-interactive and asks for UTF-8 output. Locally the command is a script
//!   file ([`script_file`]: `sh <file>`, `pwsh -NoProfile -NonInteractive -File <file>.ps1`,
//!   `cmd /d /c <file>.cmd` after `chcp 65001`); over ssh it travels on stdin. A substituted value
//!   — a secret — is never a command-line argument on either machine.

/// The target's family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Unix,
    Windows,
}

impl Os {
    /// The machine this code runs on — the host, which is where a captured cell runs.
    pub fn current() -> Os {
        if cfg!(windows) { Os::Windows } else { Os::Unix }
    }
}

/// The interpreter family of a program name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Posix,
    PowerShell,
    Cmd,
}

pub fn family(program: &str) -> Family {
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    let name = name.trim_end_matches(".exe");
    match name {
        "pwsh" | "powershell" => Family::PowerShell,
        "cmd" => Family::Cmd,
        _ => Family::Posix,
    }
}

/// The interpreter for a cell: `shell=` if given, else from its fence language, else the target's
/// default. `exists` says whether a program is on the target's path (it prefers `pwsh`).
pub fn interpreter(
    lang: &str,
    shell: Option<&str>,
    os: Os,
    exists: &dyn Fn(&str) -> bool,
) -> String {
    if let Some(shell) = shell.filter(|s| !s.is_empty()) {
        return shell.to_string();
    }
    let powershell = || {
        if exists("pwsh") || os == Os::Unix {
            "pwsh".to_string()
        } else {
            "powershell".to_string()
        }
    };
    match lang {
        "sh" | "bash" | "zsh" | "fish" | "dash" | "ksh" => lang.to_string(),
        "powershell" | "pwsh" | "ps1" | "ps" => powershell(),
        "cmd" | "bat" | "batch" => "cmd".to_string(),
        _ => match os {
            Os::Unix => "sh".to_string(),
            Os::Windows => powershell(),
        },
    }
}

/// The fence languages that name an interpreter, so a lint can tell a typo from a default.
pub fn names_interpreter(lang: &str) -> bool {
    matches!(
        lang,
        "sh" | "bash"
            | "zsh"
            | "fish"
            | "dash"
            | "ksh"
            | "powershell"
            | "pwsh"
            | "ps1"
            | "ps"
            | "cmd"
            | "bat"
            | "batch"
    )
}

/// How to start a script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
    /// Written to the process's stdin, which is then closed.
    pub stdin: Option<String>,
}

/// The script file a cell's command is written to — its extension and its contents — so that no
/// substituted value, a secret least of all, is ever a command-line argument (readable by any
/// local user in the process table). The worker writes it owner-only next to the run's log and
/// removes it when the run ends.
pub fn script_file(program: &str, script: &str) -> (&'static str, String) {
    match family(program) {
        Family::Posix => ("sh", format!("{}\n", script.trim_end())),
        // A BOM so Windows PowerShell 5.1 reads it as UTF-8, then UTF-8 output.
        Family::PowerShell => ("ps1", format!("\u{feff}{}", powershell_body(script))),
        Family::Cmd => (
            "cmd",
            format!(
                "@echo off\r\nchcp 65001>nul\r\n{}\r\nexit /b %ERRORLEVEL%\r\n",
                normalise(script).trim_end().replace('\n', "\r\n")
            ),
        ),
    }
}

/// The body of a PowerShell script: UTF-8 output, the cell, its last native exit code.
fn powershell_body(script: &str) -> String {
    format!(
        "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8\n$OutputEncoding = [System.Text.Encoding]::UTF8\n{}\nexit $LASTEXITCODE\n",
        normalise(script).trim_end()
    )
}

/// How to run the script file at `path` under `program`: non-interactively, nothing on stdin.
pub fn invocation(program: &str, path: &str) -> Invocation {
    let args: Vec<&str> = match family(program) {
        Family::Posix => vec![path],
        Family::PowerShell => vec![
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            path,
        ],
        Family::Cmd => vec!["/d", "/c", path],
    };
    Invocation {
        program: program.into(),
        args: args.into_iter().map(String::from).collect(),
        stdin: None,
    }
}

/// CRLF and a stray BOM as LF and nothing: what a hash and an excerpt see.
pub fn normalise(text: &str) -> String {
    text.trim_start_matches('\u{feff}').replace("\r\n", "\n")
}

/// How a script reads the env file's path in this interpreter, for the guide and the template.
pub fn env_reference(program: &str) -> &'static str {
    match family(program) {
        Family::Posix => "$RUNBOOK_ENV",
        Family::PowerShell => "$env:RUNBOOK_ENV",
        Family::Cmd => "%RUNBOOK_ENV%",
    }
}

/// Where a cell runs (`target=`, else the document's `target:`, else `local`).
///
/// A **drone-served project** needs no target: the drone that serves it is the host that runs its
/// runbooks, so `local` there is the drone's machine. `ssh:<alias>` runs through the host's own
/// `ssh` with the alias from its SSH config; `@windows` after the alias says the far shell is
/// Windows, which decides the default interpreter there. `drone:<id>` on a cell of a project served
/// here is not supported: open the project from the drone instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Local,
    Ssh { alias: String, os: Os },
    Drone(String),
    Unknown(String),
}

impl Target {
    pub fn parse(text: Option<&str>) -> Target {
        let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) else {
            return Target::Local;
        };
        if text == "local" {
            return Target::Local;
        }
        // Exactly `ssh:<alias>` or `ssh:<alias>@windows`. An alias is a name from the SSH config,
        // so `user@host`, an option or anything shell-special is refused, never guessed at.
        if let Some(rest) = text.strip_prefix("ssh:") {
            let (alias, os) = match rest.split_once('@') {
                Some((alias, "windows")) => (alias, Some(Os::Windows)),
                Some(_) => ("", None),
                None => (rest, Some(Os::Unix)),
            };
            let plain = !alias.is_empty()
                && !alias.starts_with(['-', '.'])
                && alias
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
            if let (true, Some(os)) = (plain, os) {
                return Target::Ssh {
                    alias: alias.to_string(),
                    os,
                };
            }
        }
        if let Some(id) = text.strip_prefix("drone:") {
            return Target::Drone(id.to_string());
        }
        Target::Unknown(text.to_string())
    }

    /// The OS the cell's interpreter is chosen for.
    pub fn os(&self) -> Os {
        match self {
            Target::Ssh { os, .. } => *os,
            _ => Os::current(),
        }
    }
}

/// `invocation` run on the far side of `ssh <alias>`: the interpreter and its arguments become the
/// remote command line (each argument quoted for the far shell), and the script always travels on
/// stdin, so nothing of it is ever a command-line argument on either machine. `-T` (no terminal)
/// and `BatchMode` (no password prompt: it fails instead of hanging).
pub fn over_ssh(alias: &str, os: Os, program: &str, script: &str) -> Invocation {
    let remote: Vec<String> = match family(program) {
        Family::Posix => vec![program.into(), "-s".into()],
        Family::PowerShell => ["-NoProfile", "-NonInteractive", "-Command", "-"]
            .iter()
            .map(|a| a.to_string())
            .fold(vec![program.to_string()], |mut v, a| {
                v.push(a);
                v
            }),
        // cmd reads a script from stdin as if typed: echo off keeps it to the output.
        Family::Cmd => vec![
            program.into(),
            "/d".into(),
            "/q".into(),
            "/k".into(),
            "@echo off".into(),
        ],
    };
    let quote = |a: &str| -> String {
        match os {
            Os::Unix => format!("'{}'", a.replace('\'', "'\\''")),
            // OpenSSH on Windows hands the line to its default shell (cmd or PowerShell): plain
            // words need no quoting, and the ones above are plain.
            Os::Windows if a.contains(' ') => format!("\"{a}\""),
            Os::Windows => a.to_string(),
        }
    };
    let mut args: Vec<String> = ["-T", "-o", "BatchMode=yes", "--", alias]
        .iter()
        .map(|a| a.to_string())
        .collect();
    args.push(
        remote
            .iter()
            .map(|a| quote(a))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let stdin = match family(program) {
        Family::Posix => format!("{}\n", script.trim_end()),
        Family::PowerShell => powershell_body(script),
        Family::Cmd => format!(
            "chcp 65001>nul\r\n{}\r\nexit %ERRORLEVEL%\r\n",
            normalise(script).trim_end().replace('\n', "\r\n")
        ),
    };
    Invocation {
        program: "ssh".into(),
        args,
        stdin: Some(stdin),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_language_decides_and_the_default_follows_the_target_os() {
        let none = |_: &str| false;
        let pwsh = |p: &str| p == "pwsh";
        assert_eq!(interpreter("bash", None, Os::Windows, &none), "bash");
        assert_eq!(interpreter("", None, Os::Unix, &none), "sh");
        assert_eq!(
            interpreter("console", None, Os::Windows, &none),
            "powershell"
        );
        assert_eq!(interpreter("console", None, Os::Windows, &pwsh), "pwsh");
        assert_eq!(interpreter("powershell", None, Os::Windows, &pwsh), "pwsh");
        assert_eq!(interpreter("bat", None, Os::Windows, &none), "cmd");
        assert_eq!(interpreter("sh", Some("zsh"), Os::Unix, &none), "zsh");
        assert_eq!(
            family("C:\\Program Files\\PowerShell\\7\\pwsh.exe"),
            Family::PowerShell
        );
        assert_eq!(family("/bin/bash"), Family::Posix);
    }

    #[test]
    fn targets_parse_and_ssh_carries_the_script_on_stdin_for_either_os() {
        assert_eq!(Target::parse(None), Target::Local);
        assert_eq!(Target::parse(Some("local")), Target::Local);
        assert_eq!(
            Target::parse(Some("ssh:build-box")),
            Target::Ssh {
                alias: "build-box".into(),
                os: Os::Unix
            }
        );
        assert_eq!(
            Target::parse(Some("ssh:win01@windows")),
            Target::Ssh {
                alias: "win01".into(),
                os: Os::Windows
            }
        );
        assert_eq!(
            Target::parse(Some("ssh:-oProxyCommand=x")),
            Target::Unknown("ssh:-oProxyCommand=x".into())
        );
        for bad in [
            "ssh:deploy@prod-box",
            "ssh:a b",
            "ssh:x@Windows",
            "ssh:",
            "ssh:a;rm",
        ] {
            assert_eq!(
                Target::parse(Some(bad)),
                Target::Unknown(bad.into()),
                "{bad}"
            );
        }
        assert_eq!(
            Target::parse(Some("drone:abc")),
            Target::Drone("abc".into())
        );
        assert_eq!(Target::parse(Some("ssh:win01@windows")).os(), Os::Windows);

        let unix = over_ssh("box", Os::Unix, "bash", "echo 'hi'\n");
        assert_eq!(unix.program, "ssh");
        assert_eq!(
            unix.args,
            ["-T", "-o", "BatchMode=yes", "--", "box", "'bash' '-s'"]
        );
        assert_eq!(unix.stdin.as_deref(), Some("echo 'hi'\n"));

        // A Windows target, built on this (any) host: the default interpreter is the target's.
        let program = interpreter("", None, Os::Windows, &|p| p == "pwsh");
        let win = over_ssh("win01", Os::Windows, &program, "Get-Date\r\n");
        assert_eq!(
            win.args.last().unwrap(),
            "pwsh -NoProfile -NonInteractive -Command -"
        );
        assert!(
            win.stdin
                .as_deref()
                .unwrap()
                .contains("Get-Date\nexit $LASTEXITCODE")
        );
        let cmd = over_ssh("win01", Os::Windows, "cmd", "echo one\necho two\n");
        assert_eq!(cmd.args.last().unwrap(), "cmd /d /q /k \"@echo off\"");
        assert_eq!(
            cmd.stdin.as_deref(),
            Some("chcp 65001>nul\r\necho one\r\necho two\r\nexit %ERRORLEVEL%\r\n")
        );
    }

    #[test]
    fn invocations_are_non_interactive_and_ask_for_utf8_and_carry_no_script_on_argv() {
        let sh = invocation("sh", "/d/rb-1.sh");
        assert_eq!(sh.args, ["/d/rb-1.sh"]);
        assert_eq!(script_file("sh", "echo hi").1, "echo hi\n");
        let ps = invocation("pwsh", "C:\\d\\rb-1.ps1");
        assert_eq!(
            ps.args,
            [
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                "C:\\d\\rb-1.ps1"
            ]
        );
        let (ext, body) = script_file("pwsh", "Write-Output hi\r\n");
        assert_eq!(ext, "ps1");
        assert!(body.starts_with("\u{feff}[Console]::OutputEncoding"));
        assert!(body.contains("Write-Output hi\nexit $LASTEXITCODE"));
        let cmd = invocation("cmd", "C:\\d\\rb-1.cmd");
        assert_eq!(cmd.args, ["/d", "/c", "C:\\d\\rb-1.cmd"]);
        assert_eq!(
            script_file("cmd", "echo one\necho two\n").1,
            "@echo off\r\nchcp 65001>nul\r\necho one\r\necho two\r\nexit /b %ERRORLEVEL%\r\n"
        );
        for how in [sh, ps, cmd] {
            assert!(how.stdin.is_none());
        }
        assert_eq!(env_reference("powershell.exe"), "$env:RUNBOOK_ENV");
        assert_eq!(env_reference("cmd"), "%RUNBOOK_ENV%");
        assert_eq!(normalise("\u{feff}a\r\nb"), "a\nb");
    }
}
