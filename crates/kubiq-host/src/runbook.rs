//! Runbooks: the host half. The engine and its worker are `kubiq-runbook`; this module owns where a
//! runbook's evidence lives (resolved through [`ProjectData`], never composed), the one thread the
//! window's requests are served on, the bus adapter the worker reports through, and the
//! [`Registry`] of providers an edition fills.
//!
//! **A provider is the seam an edition's templating and history go through** (`Provider`, a host
//! service like `tasksrc` and `runners`): it binds a document's parameters, may lock it, is told
//! every record the worker appends, and offers actions the viewer draws as buttons. The base
//! registers none, so a runbook here is a scratch run: its literal defaults, its own evidence.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use std::collections::HashMap;

use kubiq_proto::bus::{Mailbox, Voice};
use kubiq_proto::ids::ProjectId;
use kubiq_proto::messages::Message;
use kubiq_proto::runbook::{RunbookEvent, RunbookRequest};
use kubiq_proto::work::AgentId;

use crate::store::project_dir::ProjectData;

pub use kubiq_runbook as engine;
pub use kubiq_runbook::service::{DocRef, Event, Limits, Provider, Service};

/// The MCP server a runbook's events point the agent at (`mcp::catalogue::KUBIQ_RUNBOOK`, named
/// here too because a lean host — a drone — has no `mcp`).
pub const SERVER: &str = "kubiq-runbook";

/// The providers this build binds runbooks through. Empty in the base.
#[derive(Default)]
pub struct Registry {
    providers: Vec<Arc<dyn Provider>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one. A second registration under an id already held replaces it.
    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        let id = provider.id().to_string();
        self.providers.retain(|held| held.id() != id);
        self.providers.push(provider);
    }

    /// Drop one by id, and say whether there was one.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.providers.len();
        self.providers.retain(|held| held.id() != id);
        before != self.providers.len()
    }

    pub fn ids(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.id()).collect()
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

/// Which agent each runbook is bound to, keyed by (project, rel). Shared by the sink, which
/// queues the bound agent's events, and the request thread, which binds.
type Bound = Arc<std::sync::Mutex<HashMap<(String, String), AgentId>>>;

/// The worker's reports, as bus messages: state and live tails to every window, and the changes the
/// bound agent should hear into the host's own inbox as `QueueDocEvent` (`D207`'s doc queue).
struct Reporter {
    everyone: Mailbox,
    voice: Option<Voice>,
    bound: Bound,
}

/// The change-log kinds the bound agent is told about. Its own writes and requests are not news.
const NEWS: &[&str] = &["finished", "verdict", "decision", "resolved", "edited"];

impl kubiq_runbook::service::Sink for Reporter {
    fn event(&self, doc: &DocRef, event: Event) {
        let Ok(project_id) = doc.project.parse::<ProjectId>() else {
            return;
        };
        let bound = self
            .bound
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(doc.project.clone(), doc.rel.clone()))
            .copied();
        let event = match event {
            Event::State(snapshot) => RunbookEvent::State {
                snapshot: *snapshot,
                bound,
            },
            Event::Tail(tail) => RunbookEvent::Tail(tail),
            Event::Finished {
                cell,
                attempt,
                label,
            } => RunbookEvent::Finished {
                cell,
                attempt,
                label,
            },
            Event::Changed(change) => {
                if let (Some(agent_id), Some(voice)) = (bound, &self.voice)
                    && NEWS.contains(&change.kind.as_str())
                {
                    voice.say(Message::QueueDocEvent {
                        project_id,
                        agent_id,
                        event: kubiq_cells::DocEvent {
                            doc: doc.rel.clone(),
                            server: SERVER.to_string(),
                            text: change.text,
                        },
                    });
                }
                return;
            }
        };
        self.everyone.send(Message::RunbookEvent {
            project_id,
            rel: doc.rel.clone(),
            event: Box::new(event),
        });
    }
}

/// One window request, queued for the runbook thread.
struct Job {
    asker: Mailbox,
    doc: DocRef,
    request: RunbookRequest,
}

/// The runbooks of this host: the worker and the queue in front of it.
pub struct Runbooks {
    service: Service,
    bound: Bound,
    /// The host's own inbox: where an agent's request is armed as its ask dialog (`ArmCellGate`).
    voice: Option<Voice>,
    config_root: PathBuf,
    jobs: flume::Sender<Job>,
}

/// Shared by the coordinator and the MCP listener.
pub type Handle = Arc<Runbooks>;

impl Runbooks {
    /// Start the worker and the thread window requests are served on, in the order they came.
    /// `voice` is the host's own inbox, where a bound agent's events are queued; `None` (a test)
    /// queues nothing.
    pub fn start(
        config_root: PathBuf,
        everyone: Mailbox,
        voice: Option<Voice>,
        providers: Registry,
    ) -> Handle {
        let bound: Bound = Arc::default();
        let service = Service::new(
            Arc::new(Reporter {
                everyone,
                voice: voice.clone(),
                bound: bound.clone(),
            }),
            providers.providers,
            Limits::default(),
        );
        let (jobs, rx) = flume::unbounded::<Job>();
        let worker = service.clone();
        let binder = bound.clone();
        std::thread::Builder::new()
            .name("kubiq-runbook".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    serve(&worker, &binder, job);
                }
            })
            .expect("the runbook thread");
        Arc::new(Runbooks {
            service,
            bound,
            voice,
            config_root,
            jobs,
        })
    }

    pub fn service(&self) -> &Service {
        &self.service
    }

    /// Arm an agent's run request as its `ask_user_question` dialog, raised when its turn ends
    /// (`D210`): the cells with their substituted commands, and the gate's three answers. Nothing
    /// is armed for a request already gone (a `readonly-auto` policy ran it), for an agent that is
    /// not a conversation, or on a host with no inbox (a drone, a test).
    pub fn arm_gate(&self, project_id: ProjectId, agent_key: &str, doc: &DocRef, request: u64) {
        let (Some(voice), Ok(agent_id)) = (&self.voice, agent_key.parse::<AgentId>()) else {
            return;
        };
        let Ok((snap, _)) = self.service.snapshot(doc) else {
            return;
        };
        let Some(pending) = snap.pending.iter().find(|p| p.id == request) else {
            return;
        };
        use kubiq_proto::ask::{AskOption, AskQuestion};
        let mut listed = String::new();
        for cell in &pending.cells {
            let command = snap
                .commands
                .get(cell)
                .map(|c| c.display.as_str())
                .unwrap_or("");
            listed.push_str(&format!("\n\n{cell}:\n{command}"));
        }
        let option = |label: &str, description: &str| AskOption {
            label: label.to_string(),
            description: description.to_string(),
            preview: None,
        };
        voice.say(Message::ArmCellGate {
            project_id,
            agent_id,
            doc: doc.rel.clone(),
            server: SERVER.to_string(),
            run: request.to_string(),
            question: AskQuestion {
                question: format!(
                    "Run {} runbook cell(s) for {}? {}{listed}",
                    pending.cells.len(),
                    pending.by,
                    pending.why
                ),
                header: "Run cells".into(),
                options: vec![
                    option(
                        crate::notebook::GATE_ALLOW,
                        "Run exactly these commands now, if none changed since.",
                    ),
                    option(
                        crate::notebook::GATE_DENY,
                        "Do not run them; the agent is told they were skipped.",
                    ),
                ],
                multi_select: false,
            },
        });
    }

    /// The agent bound to a runbook, if any.
    pub fn bound(&self, doc: &DocRef) -> Option<AgentId> {
        self.bound
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(doc.project.clone(), doc.rel.clone()))
            .copied()
    }

    /// The document `rel` of a project at `root`: refused unless it is a `*.runbook.md` inside
    /// the project's folder.
    pub fn doc(&self, project_id: ProjectId, root: &Path, rel: &str) -> Result<DocRef, String> {
        let rel = rel.trim_start_matches("./").replace('\\', "/");
        if !kubiq_runbook::is_runbook(&rel) {
            return Err(format!("{rel} is not a runbook (*.runbook.md)"));
        }
        let path = Path::new(&rel);
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
        {
            return Err(format!("{rel} is not inside the project"));
        }
        let data = ProjectData::resolved(&self.config_root, project_id)
            .runbooks()
            .join(kubiq_runbook::service::data_dir_name(&rel));
        Ok(DocRef {
            project: project_id.to_string(),
            root: root.to_path_buf(),
            rel,
            data,
        })
    }

    /// Queue a window's request; the answer goes to `asker`, the state to everyone.
    pub fn submit(
        &self,
        asker: Mailbox,
        project_id: ProjectId,
        root: &Path,
        rel: &str,
        request: RunbookRequest,
    ) {
        match self.doc(project_id, root, rel) {
            Ok(doc) => {
                let _ = self.jobs.send(Job {
                    asker,
                    doc,
                    request,
                });
            }
            Err(reason) => {
                asker.send(Message::RunbookEvent {
                    project_id,
                    rel: rel.to_string(),
                    event: Box::new(RunbookEvent::Refused { reason }),
                });
            }
        }
    }
}

fn serve(service: &Service, bound: &Bound, job: Job) {
    let Job {
        asker,
        doc,
        request,
    } = job;
    let refused = |reason: String| RunbookEvent::Refused { reason };
    let answer = match request {
        RunbookRequest::Open => service.open(&doc).err().map(refused),
        RunbookRequest::Run {
            approvals,
            mut options,
        } => {
            // Only the worker's own gate names a policy; a window's run is a person's.
            options.policy = None;
            Some(RunbookEvent::Ran(service.run(&doc, &approvals, &options)))
        }
        RunbookRequest::Stop { cell } => service.stop(&doc, &cell).err().map(refused),
        RunbookRequest::Skip { cell, reason } => service
            .decide(&doc, &cell, engine::evidence::DecisionKind::Skip, reason)
            .err()
            .map(refused),
        RunbookRequest::Override { cell, reason } => service
            .decide(
                &doc,
                &cell,
                engine::evidence::DecisionKind::Override,
                reason,
            )
            .err()
            .map(refused),
        RunbookRequest::Verdict { cell, pass, note } => {
            service.verdict(&doc, &cell, pass, note).err().map(refused)
        }
        RunbookRequest::Dismiss { request } => service.dismiss(&doc, request).err().map(refused),
        RunbookRequest::AcceptReadonly { cell, reason, hash } => service
            .accept_readonly(&doc, &cell, reason, &hash)
            .err()
            .map(refused),
        RunbookRequest::Bind { agent } => {
            let key = (doc.project.clone(), doc.rel.clone());
            match agent {
                Some(agent) => bound
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(key, agent),
                None => bound.lock().unwrap_or_else(|e| e.into_inner()).remove(&key),
            };
            service.open(&doc).err().map(refused)
        }
        RunbookRequest::ReadOutput {
            cell,
            attempt,
            query,
        } => Some(match service.read_output(&doc, attempt, &cell, &query) {
            Ok(slice) => RunbookEvent::Output {
                cell,
                attempt,
                slice: Some(slice),
                error: None,
            },
            Err(error) => RunbookEvent::Output {
                cell,
                attempt,
                slice: None,
                error: Some(error),
            },
        }),
        RunbookRequest::Action { action, fields } => {
            Some(match service.action(&doc, &action, &fields) {
                Ok(outcome) => RunbookEvent::ActionDone {
                    action,
                    outcome: Some(outcome),
                    error: None,
                },
                Err(error) => RunbookEvent::ActionDone {
                    action,
                    outcome: None,
                    error: Some(error),
                },
            })
        }
    };
    if let Some(event) = answer
        && let Ok(project_id) = doc.project.parse::<ProjectId>()
    {
        asker.send(Message::RunbookEvent {
            project_id,
            rel: doc.rel.clone(),
            event: Box::new(event),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_must_be_a_runbook_inside_the_project() {
        let handle = Runbooks::start(
            std::env::temp_dir().join("kubiq-runbook-host-doc"),
            Mailbox::nowhere(),
            None,
            Registry::new(),
        );
        let id = ProjectId::generate();
        let root = Path::new("/p");
        assert!(handle.doc(id, root, "ops/rotate.runbook.md").is_ok());
        assert!(handle.doc(id, root, "notes.md").is_err());
        assert!(handle.doc(id, root, "../x.runbook.md").is_err());
        assert!(handle.doc(id, root, "/etc/x.runbook.md").is_err());
        let doc = handle.doc(id, root, "./ops/rotate.runbook.md").unwrap();
        assert_eq!(doc.rel, "ops/rotate.runbook.md");
        assert!(doc.data.to_string_lossy().contains("runbooks"));
    }
}
