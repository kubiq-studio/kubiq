//! The `kubiq-notebook` server: a notebook's cells as tools any hosted agent can call
//! (`crate::notebook`).
//!
//! Every call is a request to the notebook worker against the **live document** — the one every
//! window draws — so an agent's edit reaches each open view at once and lands on the notebook's
//! undo stack as the agent's. A run goes through the notebook's agent policy (`ask` / `allow` /
//! `deny`); under `allow`, `run_cells` waits for its runs here, polling the worker, so the listener
//! thread is never the one waiting. Served on a thread per call ([`super::ServiceReach`]).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use kubiq_notebook::{CellKind, CellOp};
use kubiq_proto::ids::{NotebookRunId, ProjectId};
use serde_json::{Value, json};

use super::ServiceReach;
use super::catalogue::KUBIQ_NOTEBOOK;
use super::registry::AgentFacts;
use crate::notebook::{AgentCall, Notebooks};

/// How long a worker answer may take.
const ANSWER: Duration = Duration::from_secs(30);
/// `run_cells`'s wait when the agent names none.
const DEFAULT_WAIT: u64 = 60;

pub fn reach(notebooks: Notebooks) -> ServiceReach {
    ServiceReach {
        slug: KUBIQ_NOTEBOOK,
        call: std::sync::Arc::new(move |tool, args, facts| call(&notebooks, tool, args, facts)),
    }
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("`{key}` is required"))
}

pub fn call(notebooks: &Notebooks, tool: &str, args: &Value, facts: &AgentFacts) -> Result<Value, String> {
    let project_id: ProjectId = facts
        .project
        .id
        .parse()
        .map_err(|_| "this agent has no project".to_string())?;
    let root = PathBuf::from(&facts.project.path);
    let agent = facts.key.clone();
    let ask = |call: AgentCall| notebooks.ask(project_id, root.clone(), agent.clone(), call, ANSWER);
    if tool == "list_notebooks" {
        return ask(AgentCall::List);
    }
    let path = str_arg(args, "path")?.to_string();
    let edit = |op: CellOp| ask(AgentCall::Edit { path: path.clone(), ops: vec![op] });
    match tool {
        "read_notebook" => ask(AgentCall::Read {
            path: path.clone(),
            full: args.get("outputs").and_then(Value::as_str) == Some("full"),
            cells: args.get("cells").and_then(Value::as_array).map(|ids| {
                ids.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
            }),
        }),
        "edit_cell" => edit(CellOp::Source {
            id: str_arg(args, "cell_id")?.into(),
            source: str_arg(args, "source")?.into(),
        }),
        "delete_cell" => edit(CellOp::Delete { id: str_arg(args, "cell_id")?.into() }),
        "move_cell" => edit(CellOp::Move {
            id: str_arg(args, "cell_id")?.into(),
            to: args.get("to").and_then(Value::as_u64).ok_or("`to` is required")? as usize,
        }),
        "insert_cell" => {
            let kind = match args.get("kind").and_then(Value::as_str) {
                Some("markdown") => CellKind::Markdown,
                Some("raw") => CellKind::Raw,
                _ => CellKind::Code,
            };
            // The position is read now: `after` names a cell, never an index.
            let index = match args.get("after").and_then(Value::as_str) {
                None => 0,
                Some(after) => {
                    let order = cell_order(notebooks, project_id, &root, &agent, &path)?;
                    order
                        .iter()
                        .position(|id| id == after)
                        .map(|n| n + 1)
                        .ok_or_else(|| format!("No cell with id {after}"))?
                }
            };
            edit(CellOp::Insert {
                index,
                id: None,
                kind,
                source: str_arg(args, "source")?.into(),
            })
        }
        "run_cells" => {
            let cells: Vec<String> = args
                .get("cell_ids")
                .and_then(Value::as_array)
                .ok_or("`cell_ids` is required")?
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            let started = ask(AgentCall::Run { path: path.clone(), cells: cells.clone() })?;
            if started["status"] != "running" {
                return Ok(started);
            }
            let runs: Vec<NotebookRunId> = started["runs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str()?.parse().ok())
                .collect();
            let wait = Duration::from_secs(args.get("timeout").and_then(Value::as_u64).unwrap_or(DEFAULT_WAIT).min(600));
            let until = Instant::now() + wait;
            loop {
                let state = ask(AgentCall::Runs { path: path.clone(), runs: runs.clone() })?;
                if state["done"] == true {
                    let read = ask(AgentCall::Read { path: path.clone(), full: false, cells: Some(cells) })?;
                    return Ok(json!({"status": "done", "runs": state["runs"], "notebook": read["notebook"]}));
                }
                if Instant::now() >= until {
                    return Ok(json!({"status": "running", "runs": state["runs"], "note": "Still running; call read_notebook later."}));
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
        "kernel" => ask(AgentCall::Kernel {
            path: path.clone(),
            action: str_arg(args, "action")?.into(),
        }),
        other => Err(format!("unknown tool: {other}")),
    }
}

/// The notebook's cell ids in order, from an empty edit (which changes nothing and answers them).
fn cell_order(notebooks: &Notebooks, project_id: ProjectId, root: &std::path::Path, agent: &str, path: &str) -> Result<Vec<String>, String> {
    let answer = notebooks.ask(
        project_id,
        root.to_path_buf(),
        agent.to_string(),
        AgentCall::Edit { path: path.to_string(), ops: vec![] },
        ANSWER,
    )?;
    Ok(answer["cells"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect())
}
