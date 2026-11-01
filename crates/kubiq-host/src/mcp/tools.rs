//! What the built-in tools actually do.
//!
//! Most of them are answered on the listener's own thread, from the [`AgentFacts`] the URL
//! resolved to and nothing else. Task tools are the exception: they take the shared
//! [`super::WorkAccess`] and talk to [`crate::work::Work`] under its lock, then post the
//! resulting work-family messages to every window. They still never ask the coordinator a
//! question — the handle is the board, not a round trip through the run loop.
//!
//! **`kubiq-ask` waits for nothing.** `ask_user_question` arms a dialog for the end of the turn and
//! returns (`D175`, `D210`); the retired parked mode that needed a thread of its own (`D138`) is
//! kept in [`super::server`] for reference only.
//!
//! An `Err` here is not a JSON-RPC error: the caller turns it into MCP's in-band `isError`, which
//! is what a model can read and correct. See [`super::server::dispatch`].

use serde_json::{Value, json};
use kubiq_proto::bus::Voice;

use super::catalogue::{
    MANAGE_KUBIQ_TASKS, PROJECT_INFO, KUBIQ_ARCHIFY, KUBIQ_ASK, KUBIQ_BROWSER, KUBIQ_DOC,
    KUBIQ_HELP, KUBIQ_KB, KUBIQ_MISSION, KUBIQ_REMOTE, KUBIQ_SQL_READ, KUBIQ_SQL_WRITE,
    USE_TASK,
};
use super::registry::AgentFacts;
use super::{
    ArchifyReach, AskReach, HelpReach, KbReach, MissionReach, PlanReach, SqlReach, WorkAccess,
};

/// Call one tool. `server` and `tool` have already been matched against the catalogue's server;
/// the tool has not, so an unknown one ends here as the in-band error a model sees.
#[allow(clippy::too_many_arguments)]
pub fn call(
    server: &str,
    tool: &str,
    arguments: &Value,
    facts: &AgentFacts,
    voice: &Voice,
    work: Option<&WorkAccess>,
    plan: Option<&PlanReach>,
    mission: Option<&MissionReach>,
    kb: Option<&KbReach>,
    help: Option<&HelpReach>,
    ask: Option<&AskReach>,
    sql: Option<&SqlReach>,
    archify: Option<&ArchifyReach>,
) -> Result<Value, String> {
    match (server, tool) {
        (PROJECT_INFO, "project_info") => Ok(project_info(facts)),
        (PROJECT_INFO, "whoami") => Ok(whoami(facts)),
        (MANAGE_KUBIQ_TASKS, _) => {
            let access =
                work.ok_or_else(|| "this host has no task board for agents to use".to_string())?;
            super::tasks::manage_call(tool, arguments, facts, access)
        }
        (USE_TASK, _) => {
            let access =
                work.ok_or_else(|| "this host has no task board for agents to use".to_string())?;
            super::tasks::use_call(tool, arguments, facts, access)
        }
        // An annotated document: a file in the agent's project by `path` (`D161`), or a task's
        // plan by `task_id` (`D229`).
        (KUBIQ_DOC, _) => {
            let reach = plan
                .ok_or_else(|| "this host has no document store for agents to reach".to_string())?;
            super::plan::doc_call(tool, arguments, facts, reach)
        }
        (KUBIQ_MISSION, _) => {
            let reach = mission
                .ok_or_else(|| "this host has no missions for agents to reach".to_string())?;
            super::mission::call(tool, arguments, facts, reach, work, plan)
        }
        (KUBIQ_KB, _) => {
            let reach = kb
                .ok_or_else(|| "this host has no knowledge base for agents to reach".to_string())?;
            super::kb::call(tool, arguments, facts, reach)
        }
        (KUBIQ_HELP, _) => {
            let reach =
                help.ok_or_else(|| "this host has no help server for agents to reach".to_string())?;
            super::help::call(tool, arguments, reach)
        }
        // The one arm that waits for a person. It is reached on a thread
        // [`super::server::handle`] spawned for it and never on the listener's own (`D138`); a
        // build with no reach refuses here instead, which costs nothing and blocks nobody.
        (KUBIQ_ASK, _) => {
            let reach =
                ask.ok_or_else(|| "this host cannot put a question to the user".to_string())?;
            super::ask::call(tool, arguments, facts, voice, reach)
        }
        // Reached on a thread [`super::server::handle`] spawned for it, like the ask arm: a query
        // can run for minutes and the listener must stay free (`D202`).
        (KUBIQ_SQL_READ | KUBIQ_SQL_WRITE, _) => sql_call(server, tool, arguments, facts, sql),
        // Reached on a thread of its own too: a compile can take a second (`D138`).
        (KUBIQ_ARCHIFY, _) => {
            let reach = archify
                .ok_or_else(|| "this host has no diagram server for agents to reach".to_string())?;
            super::archify::call(tool, arguments, facts, reach)
        }
        // A browser call with a reach never gets here: [`super::server::handle`] serves it on a
        // thread of its own, because it waits on a window. Only a build with no reach lands here.
        (KUBIQ_BROWSER, _) => Err("this host has no browser for agents to reach".to_string()),
        // The same for a remote call: served on its own thread when the listener has a reach.
        (KUBIQ_REMOTE, _) => Err("this host has no remote machines for agents to reach".to_string()),
        _ => Err(format!("unknown tool: {server}/{tool}")),
    }
}

#[cfg(feature = "db")]
fn sql_call(
    server: &str,
    tool: &str,
    arguments: &Value,
    facts: &AgentFacts,
    sql: Option<&SqlReach>,
) -> Result<Value, String> {
    let reach = sql.ok_or_else(|| "this host has no databases for agents to reach".to_string())?;
    super::sql::call(server == KUBIQ_SQL_WRITE, tool, arguments, facts, reach)
}

#[cfg(not(feature = "db"))]
fn sql_call(
    _server: &str,
    _tool: &str,
    _arguments: &Value,
    _facts: &AgentFacts,
    _sql: Option<&SqlReach>,
) -> Result<Value, String> {
    Err("this host has no databases for agents to reach".to_string())
}

/// The project this agent was started in. No description field: the record has none.
fn project_info(facts: &AgentFacts) -> Value {
    json!({
        "id": facts.project.id,
        "name": facts.project.name,
        "path": facts.project.path,
        "colour": facts.project.colour,
    })
}

/// What this agent is. The account by id and nothing beside it — the rule holds here as it holds
/// on the wire.
fn whoami(facts: &AgentFacts) -> Value {
    json!({
        "agent_id": facts.key,
        "name": facts.name,
        "harness": facts.harness,
        "account": facts.account,
        "model": facts.model,
        "permission_mode": facts.mode,
        "session_id": facts.session,
        "cwd": facts.cwd,
    })
}
