//! The `kubiq-runbook` server: a runbook as tools an agent can call (`crate::runbook`).
//!
//! **The agent never runs a cell and never records a verdict.** It reads, writes (revision-checked;
//! the host's `output` blocks are regenerated from the evidence whatever it sends), reads logs, and
//! files requests the human answers in the viewer. Every path resolves inside the agent's own
//! project folder. Served on a thread per call ([`super::ServiceReach`]).

use std::path::{Path, PathBuf};

use kubiq_proto::ids::ProjectId;
use kubiq_runbook::service::WriteOutcome;
use kubiq_runbook::view::{OutputQuery, PendingKind};
use serde_json::{Value, json};

use super::ServiceReach;
use super::catalogue::KUBIQ_RUNBOOK;
use super::registry::AgentFacts;
use crate::runbook::Handle;

/// The server's reach: the runbooks handle, closed over.
pub fn reach(runbooks: Handle) -> ServiceReach {
    ServiceReach {
        slug: KUBIQ_RUNBOOK,
        call: std::sync::Arc::new(move |tool, args, facts| call(&runbooks, tool, args, facts)),
    }
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("`{key}` is required"))
}

/// The project-relative path of the runbook an agent named.
fn relative(root: &Path, given: &str) -> Result<String, String> {
    let given_path = Path::new(given);
    if given_path.is_absolute() {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let file = given_path
            .parent()
            .and_then(|p| std::fs::canonicalize(p).ok())
            .map(|p| p.join(given_path.file_name().unwrap_or_default()))
            .unwrap_or_else(|| PathBuf::from(given));
        let rel = file
            .strip_prefix(&root)
            .map_err(|_| format!("{given} is not inside the project"))?;
        Ok(rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"))
    } else {
        Ok(given.trim_start_matches("./").to_string())
    }
}

pub fn call(
    runbooks: &Handle,
    tool: &str,
    args: &Value,
    facts: &AgentFacts,
) -> Result<Value, String> {
    let project_id: ProjectId = facts
        .project
        .id
        .parse()
        .map_err(|_| "this agent has no project".to_string())?;
    let root = PathBuf::from(&facts.project.path);
    let rel = relative(&root, str_arg(args, "path")?)?;
    let doc = runbooks.doc(project_id, &root, &rel)?;
    let service = runbooks.service();
    match tool {
        "read_runbook" => {
            let (snap, text) = service.snapshot(&doc)?;
            Ok(json!({
                "path": rel,
                "rev": snap.rev,
                "text": text,
                "steps": snap.states.steps,
                "commands": snap.commands,
                "lints": snap.lints.lints,
                "pending": snap.pending,
                "params": snap.params,
                "status": snap.status,
                "attempts": snap.attempts.iter().rev().take(50).rev().collect::<Vec<_>>(),
                "env": snap.env,
            }))
        }
        "write_runbook" => {
            let text = str_arg(args, "text")?;
            let rev = args.get("rev").and_then(Value::as_str).unwrap_or("");
            match service.write(&doc, text, rev)? {
                WriteOutcome::Written { rev, lints } => Ok(json!({
                    "ok": true,
                    "rev": rev,
                    "lints": lints.lints,
                })),
                WriteOutcome::Conflict { rev } => Err(format!(
                    "conflict: the runbook changed since revision {}; it is now {rev}. Read it again and redo your edit.",
                    if rev.is_empty() {
                        "(none)"
                    } else {
                        rev.as_str()
                    }
                )),
            }
        }
        "ready_steps" => {
            let (snap, _) = service.snapshot(&doc)?;
            let waiting: Vec<Value> = snap
                .states
                .steps
                .iter()
                .filter(|s| !s.waits_on.is_empty())
                .map(|s| json!({"cell": s.cell, "state": s.state, "waits_on": s.waits_on}))
                .collect();
            Ok(json!({"ready": snap.states.ready(), "waiting": waiting}))
        }
        "runbook_changes" => {
            let since = args.get("since").and_then(Value::as_u64).unwrap_or(0);
            let (changes, cursor) = service.changes(&doc, since)?;
            Ok(json!({"changes": changes, "cursor": cursor}))
        }
        "read_output" => {
            let attempt = args
                .get("attempt")
                .and_then(Value::as_u64)
                .ok_or("`attempt` is required")?;
            let cell = str_arg(args, "cell")?;
            let n = args.get("n").and_then(Value::as_u64).unwrap_or(50);
            let query = match args.get("mode").and_then(Value::as_str).unwrap_or("tail") {
                "head" => OutputQuery::Head { n },
                "tail" => OutputQuery::Tail { n },
                "range" => OutputQuery::Range {
                    from: args.get("from").and_then(Value::as_u64).unwrap_or(1),
                    to: args.get("to").and_then(Value::as_u64).unwrap_or(u64::MAX),
                },
                "grep" => OutputQuery::Grep {
                    pattern: str_arg(args, "pattern")?.to_string(),
                    context: args.get("context").and_then(Value::as_u64).unwrap_or(0),
                },
                other => return Err(format!("unknown mode {other}: head, tail, range or grep")),
            };
            let slice = service.read_output(&doc, attempt, cell, &query)?;
            Ok(serde_json::to_value(slice).unwrap_or(Value::Null))
        }
        "request_run" => {
            let cells: Vec<String> = args
                .get("cells")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let why = str_arg(args, "why")?.to_string();
            let id = service.request(&doc, PendingKind::Run, cells, why, facts.name.clone())?;
            // Also the agent's ask dialog when its turn ends (`D210`); the viewer's bar stays.
            runbooks.arm_gate(project_id, &facts.key, &doc, id);
            Ok(
                json!({"request": id, "status": "filed: the human decides in the runbook; poll runbook_changes"}),
            )
        }
        "request_check" => {
            let check = str_arg(args, "check")?.to_string();
            let why = str_arg(args, "why")?.to_string();
            let id = service.request(
                &doc,
                PendingKind::Check,
                vec![check],
                why,
                facts.name.clone(),
            )?;
            Ok(json!({"request": id, "status": "filed: the human records the verdict"}))
        }
        "list_runs" => {
            let (snap, _) = service.snapshot(&doc)?;
            Ok(json!({"attempts": snap.attempts}))
        }
        other => Err(format!(
            "unknown tool {other}; tools: read_runbook, write_runbook, ready_steps, runbook_changes, read_output, request_run, request_check, list_runs"
        )),
    }
}
