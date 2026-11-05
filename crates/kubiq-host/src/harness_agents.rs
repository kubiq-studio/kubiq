//! A project's **harness agents** as agent definitions (`D233`): the subagent files each harness
//! reads from its own folder inside the project, listed beside the project's own definitions,
//! written and deleted from the same form, and kept aligned across folders.
//!
//! Which folders exist, their formats, how a file is parsed without losing a key and how the
//! copies are reconciled are all `agent_manager::harness_agents`'s. What is Kubiq's is the mapping
//! between a file's portable fields and an [`AgentDefinition`]: the name is the id, the body is
//! the agent prompt, and every definition property a harness format has no key for rides a
//! `kubiq-*` key ([`to_content`]). Nothing here names a folder.

use std::collections::BTreeMap;
use std::path::Path;

use agent_manager::harness_agents::{self as files, AgentContent, LocalAgent as Found};
use anyhow::{Result, bail};
use kubiq_proto::ids::ProjectId;
use kubiq_proto::messages::{AgentDefinition, LocalAgent};

const HARNESS: &str = "harness";
const ACCOUNT: &str = "account";
const MODEL: &str = "model";
const MODE: &str = "mode";
const THINKING: &str = "thinking";
const MAX_SUBAGENTS: &str = "max-subagents";
const MCPS: &str = "mcps";
const SKILLS: &str = "skills";
const TAGS: &str = "tags";
const MISSION_ASSISTANT: &str = "mission-assistant";
const DISABLED: &str = "disabled";

fn list_of(value: Option<&String>) -> Vec<String> {
    value
        .map(|it| {
            it.split(',')
                .map(str::trim)
                .filter(|it| !it.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The portable fields a definition is written as. Its grants are not carried: they are paths
/// on this machine, and a harness agent file is shared with every clone of the project.
pub fn to_content(definition: &AgentDefinition) -> AgentContent {
    let mut kubiq = BTreeMap::new();
    let mut put = |key: &str, value: Option<String>| {
        if let Some(value) = value.filter(|it| !it.trim().is_empty()) {
            kubiq.insert(key.to_string(), value);
        }
    };
    put(HARNESS, Some(definition.agent_type.clone()));
    put(ACCOUNT, definition.account.clone());
    put(MODEL, definition.model.clone());
    put(MODE, definition.mode.clone());
    put(THINKING, definition.thinking.clone());
    put(
        MAX_SUBAGENTS,
        definition.max_subagents.map(|it| it.to_string()),
    );
    put(MCPS, Some(definition.mcps.join(", ")));
    put(SKILLS, Some(definition.skills.join(", ")));
    put(TAGS, Some(definition.tags.join(", ")));
    put(
        MISSION_ASSISTANT,
        (definition.mission_assistant == Some(true)).then(|| "true".to_string()),
    );
    put(DISABLED, definition.disabled.then(|| "true".to_string()));
    AgentContent {
        name: definition.id.clone(),
        description: definition
            .description
            .clone()
            .filter(|it| !it.trim().is_empty()),
        instructions: definition.prompt.clone().unwrap_or_default(),
        kubiq,
    }
}

/// One found agent as a definition of `project`. Its harness is its `kubiq-harness`, else the
/// harness of a folder it is in, else `fallback`.
pub fn to_definition(agent: &Found, project: ProjectId, fallback: &str) -> AgentDefinition {
    let kubiq = &agent.content.kubiq;
    let text = |key: &str| kubiq.get(key).cloned().filter(|it| !it.is_empty());
    let harness = text(HARNESS)
        .or_else(|| {
            agent
                .files
                .iter()
                .find_map(|it| files::folder_harness(it.folder))
                .map(str::to_string)
        })
        .unwrap_or_else(|| fallback.to_string());
    AgentDefinition {
        id: agent.content.name.clone(),
        agent_type: harness,
        account: text(ACCOUNT),
        model: text(MODEL),
        mode: text(MODE),
        thinking: text(THINKING),
        max_subagents: text(MAX_SUBAGENTS).and_then(|it| it.parse().ok()),
        prompt: Some(agent.content.instructions.clone()).filter(|it| !it.is_empty()),
        description: agent.content.description.clone(),
        mcps: list_of(kubiq.get(MCPS)),
        skills: list_of(kubiq.get(SKILLS)),
        mission_assistant: (text(MISSION_ASSISTANT).as_deref() == Some("true")).then_some(true),
        tags: list_of(kubiq.get(TAGS)),
        disabled: text(DISABLED).as_deref() == Some("true"),
        grants: Vec::new(),
        project: Some(project),
        local: Some(LocalAgent {
            folders: agent.folders().into_iter().map(str::to_string).collect(),
        }),
    }
}

/// The harness agents under `root`, as definitions of `project`.
pub fn definitions(root: &Path, project: ProjectId, fallback: &str) -> Vec<AgentDefinition> {
    files::list(root)
        .iter()
        .map(|agent| to_definition(agent, project, fallback))
        .collect()
}

/// Whether `root` holds a harness agent called `id`.
pub fn holds(root: &Path, id: &str) -> bool {
    files::list(root).iter().any(|it| it.content.name == id)
}

/// Write `definition` as a harness agent under `root`. `replaces` is the name it was opened
/// under, for a rename. A new one goes into every folder `sync` aligns when it is on, else into
/// its harness's own folder, else the cross-harness one.
pub fn save(
    root: &Path,
    definition: &AgentDefinition,
    replaces: Option<&str>,
    sync: bool,
) -> Result<()> {
    if !root.is_dir() {
        bail!(
            "the project folder {} is not on this machine",
            root.display()
        );
    }
    let folders = if sync {
        files::sync_folders(root)
    } else {
        vec![files::home_folder(Some(&definition.agent_type))]
    };
    files::save(root, &to_content(definition), replaces, &folders, sync)
}

/// Remove every copy of the harness agent `id` under `root`.
pub fn delete(root: &Path, id: &str) -> Result<()> {
    files::delete(root, id)
}

/// Align `root`'s harness agents, logging rather than failing: a sync is housekeeping run on the
/// way to answering something else.
pub fn sync(root: &Path) {
    if !root.is_dir() {
        return;
    }
    match files::sync(root) {
        Ok(report) if !report.written.is_empty() => {
            tracing::info!(
                "harness agents aligned under {}: {} file(s) written",
                root.display(),
                report.written.len()
            );
        }
        Ok(_) => {}
        Err(error) => tracing::warn!(
            "harness agents under {} could not be aligned: {error:#}",
            root.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition(id: &str, project: ProjectId) -> AgentDefinition {
        AgentDefinition {
            id: id.to_string(),
            agent_type: "codex".into(),
            account: None,
            model: Some("gpt-5".into()),
            mode: Some("plan".into()),
            thinking: None,
            max_subagents: Some(2),
            prompt: Some("Review.".into()),
            description: Some("Reviews".into()),
            mcps: vec!["use-task".into()],
            skills: Vec::new(),
            mission_assistant: None,
            tags: vec!["worker".into()],
            disabled: false,
            grants: Vec::new(),
            project: Some(project),
            local: Some(LocalAgent::default()),
        }
    }

    #[test]
    fn a_definition_round_trips_through_a_harness_agent_file() {
        let dir = tempfile::tempdir().unwrap();
        let project = ProjectId::generate();
        save(dir.path(), &definition("reviewer", project), None, false).unwrap();
        let listed = definitions(dir.path(), project, "claude-code");
        assert_eq!(listed.len(), 1);
        let mut back = listed[0].clone();
        assert!(!back.local.as_ref().unwrap().folders.is_empty());
        back.local = Some(LocalAgent::default());
        assert_eq!(back, definition("reviewer", project));
    }

    #[test]
    fn a_hand_written_claude_agent_is_listed_on_its_folders_harness() {
        let dir = tempfile::tempdir().unwrap();
        // The folder is the library's to name, even in a test.
        let agents = dir.path().join(files::home_folder(Some("claude-code")).dir);
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::write(
            agents.join("helper.md"),
            "---\nname: helper\ndescription: Helps\ntools: Read\n---\nHelp.\n",
        )
        .unwrap();
        let project = ProjectId::generate();
        let listed = definitions(dir.path(), project, "opencode");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].agent_type, "claude-code");
        assert_eq!(listed[0].prompt.as_deref(), Some("Help."));
        assert_eq!(listed[0].project, Some(project));
        assert!(holds(dir.path(), "helper"));
        delete(dir.path(), "helper").unwrap();
        assert!(!holds(dir.path(), "helper"));
    }
}
