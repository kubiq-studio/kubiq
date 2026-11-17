//! Notebooks: the host owns every open `.ipynb` and the kernel behind it.
//!
//! One worker thread on [`crate::watch`]'s shape — the coordinator hands it a [`Job`] and returns —
//! holds one [`kubiq_notebook::Notebook`] per (project, path), read with no size cap, and at most
//! one kernel for it ([`kernel`]). Every window showing the path, and every agent working on it
//! through `kubiq-notebook` ([`crate::mcp`]), works on that one document: an edit from any of them
//! reaches every window as one `NotebookEvent::Changed`, and lands on the notebook's undo stack
//! with its origin.
//!
//! **The worker never waits.** A kernel start (seconds; minutes when uv fetches `ipykernel`), an
//! environment resolve (it runs `python` and `uv`), a shutdown's grace period and an install all
//! run on threads of their own and report back on [`Internal`]. Kernel output arrives from the
//! socket reader threads as [`kernel::Tagged`] events and is applied in batches: everything that
//! has arrived is drained before the touched cells are sent, so a chatty cell costs one message
//! per batch, not one per line.
//!
//! **The file.** Saved the way Jupyter writes it (`kubiq_notebook::nbformat`), atomically, keeping
//! its mode. Checked for outside changes once a second: a clean document re-reads and matches its
//! cells by id; a dirty one raises `Conflict` and keeps the person's edits.
//!
//! **Agents run code under a policy** ([`RunPolicy`]), kept per notebook in the project's `local/`
//! data. `Ask` turns a run into a question in the notebook's header; `Allow` runs it; `Deny`
//! refuses. "Allow for this session" lasts until the kernel restarts.

pub mod kernel;
mod zmtp;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use kubiq_notebook::doc::Change;
use kubiq_notebook::env::Candidate;
use kubiq_notebook::json::Json;
use kubiq_notebook::nbformat::output_from_message;
use kubiq_notebook::output;
use kubiq_notebook::wire::JupyterMessage;
use kubiq_notebook::{CellKind, CellOp, Notebook, Origin};
use kubiq_proto::bus::{ClientId, Mailbox, Voice};
use kubiq_proto::work::AgentId;
use kubiq_cells::DocEvent;
use kubiq_proto::ids::{NotebookRunId, ProjectId};
use kubiq_proto::messages::Message;
use kubiq_proto::notebook::{
    EnvAction, KernelChoice, KernelSpec, KernelState, NotebookEvent, NotebookRequest, RunPolicy, RunStatus,
};
use serde_json::{Value, json};

use crate::env_resolver::{self, MachineProbe};
use crate::store::project_dir::ProjectDirs;
use kernel::{Channel, Kernel, KernelEvent, Launch, Tagged};

/// How often open files are checked for outside changes.
const TICK: Duration = Duration::from_secs(1);
/// The policy file's name in a project's `local/` directory.
const POLICY_FILE: &str = "notebook-policy.json";
/// Kernel events applied before the touched cells are sent.
const BATCH: usize = 256;
/// The longest an environment action (an install) may run.
const ENV_TIMEOUT: Duration = Duration::from_secs(600);

type Key = (ProjectId, String);

/// What an agent's tool asks. Answered on `reply` with the tool's JSON result or an error sentence.
pub enum AgentCall {
    List,
    Read { path: String, full: bool, cells: Option<Vec<String>> },
    Edit { path: String, ops: Vec<CellOp> },
    Run { path: String, cells: Vec<String> },
    /// Whether these runs have all ended; their cells' views when they have.
    Runs { path: String, runs: Vec<NotebookRunId> },
    Kernel { path: String, action: String },
}

/// One thing the worker is asked to do.
pub enum Job {
    Request {
        client: ClientId,
        asker: Mailbox,
        project_id: ProjectId,
        root: PathBuf,
        path: String,
        request: NotebookRequest,
    },
    Agent {
        project_id: ProjectId,
        root: PathBuf,
        agent: String,
        call: AgentCall,
        reply: flume::Sender<Result<Value, String>>,
    },
    ClientGone(ClientId),
    /// The answer to an agent's armed run gate (`Message::ArmCellGate`).
    Decide {
        project_id: ProjectId,
        root: PathBuf,
        path: String,
        run: NotebookRunId,
        allow: bool,
        session: bool,
    },
}

/// The gate dialog's options; the coordinator reads the chosen label back.
pub const GATE_ALLOW: &str = "Allow";
pub const GATE_SESSION: &str = "Allow for this session";
pub const GATE_DENY: &str = "Deny";

/// What the worker's own threads report.
enum Internal {
    Started {
        key: Key,
        generation: u64,
        label: String,
        candidates: Vec<Candidate>,
        result: Result<(Kernel, KernelSpec), String>,
    },
    Resolved {
        key: Key,
        candidates: Vec<Candidate>,
        asker: Mailbox,
    },
    EnvDone {
        key: Key,
        asker: Mailbox,
        ok: bool,
        log: String,
    },
}

/// The handle the coordinator (and the MCP listener) holds. Cloning shares the worker; dropping
/// every clone ends it, and its kernels with it.
#[derive(Clone)]
pub struct Notebooks {
    jobs: flume::Sender<Job>,
}

impl Notebooks {
    /// Start the worker over the config root. `everyone` is where a change is announced.
    pub fn start(config_root: PathBuf, everyone: Mailbox, voice: Voice) -> Self {
        let (jobs, inbox) = flume::unbounded::<Job>();
        let runtime = config_root.join("runtime").join("kernels");
        kernel::sweep(&runtime);
        let spawned = std::thread::Builder::new()
            .name("kubiq-notebook".to_string())
            .spawn(move || Worker::new(config_root, runtime, everyone, voice).run(inbox));
        if let Err(error) = spawned {
            tracing::error!("the notebook worker could not start: {error}");
        }
        Self { jobs }
    }

    pub fn submit(&self, job: Job) {
        let _ = self.jobs.send(job);
    }

    /// Ask on behalf of an agent and wait for the answer, at most `timeout`.
    pub fn ask(
        &self,
        project_id: ProjectId,
        root: PathBuf,
        agent: String,
        call: AgentCall,
        timeout: Duration,
    ) -> Result<Value, String> {
        let (reply, answer) = flume::bounded(1);
        self.jobs
            .send(Job::Agent {
                project_id,
                root,
                agent,
                call,
                reply,
            })
            .map_err(|_| "the notebook worker is not running".to_string())?;
        answer
            .recv_timeout(timeout)
            .map_err(|_| "the notebook worker did not answer".to_string())?
    }
}

struct Run {
    run: NotebookRunId,
    cell: String,
    origin: Origin,
    /// The source a person approved, for an agent's run: run only if the cell still holds it.
    expected: Option<String>,
}

struct Current {
    run: NotebookRunId,
    origin: Origin,
    cell: String,
    msg_id: String,
    count: Option<i64>,
    status: RunStatus,
    idle: bool,
    replied: bool,
    clear_wait: bool,
    input_parent: Option<Json>,
}

struct Pending {
    run: NotebookRunId,
    agent: String,
    cells: Vec<String>,
    /// What the question showed: each cell and its source when the agent asked. An approval
    /// covers exactly these sources.
    shown: Vec<(String, String)>,
}

struct Doc {
    project_id: ProjectId,
    rel: String,
    abs: PathBuf,
    root: PathBuf,
    nb: Notebook,
    views: HashSet<ClientId>,
    disk: Option<(SystemTime, u64)>,
    conflict: bool,
    kernel: Option<Kernel>,
    generation: u64,
    state: KernelState,
    label: Option<String>,
    spec: Option<(KernelSpec, String)>,
    candidates: Vec<Candidate>,
    queue: VecDeque<Run>,
    current: Option<Current>,
    pending: Vec<Pending>,
    /// Agents a person let run here "for this session", until the kernel restarts or exits.
    allow_session: HashSet<String>,
    finished: HashMap<NotebookRunId, RunStatus>,
    /// The agent this notebook's events are queued to.
    bound: Option<AgentId>,
    /// The policy a notebook with none stored follows (the opening window's preference).
    default_policy: RunPolicy,
}

fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let md = std::fs::metadata(path).ok()?;
    Some((md.modified().ok()?, md.len()))
}

fn seed() -> u64 {
    u64::from_le_bytes(kernel::random_bytes(8).try_into().unwrap_or([7; 8]))
}

/// `rel` inside `root`, refusing anything that climbs out.
fn inside(root: &Path, rel: &str) -> Result<(String, PathBuf), String> {
    let given = Path::new(rel);
    let rel_path = if given.is_absolute() {
        given
            .strip_prefix(root)
            .map_err(|_| format!("Path outside the project: {rel}"))?
            .to_path_buf()
    } else {
        given.to_path_buf()
    };
    if rel_path
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(format!("Path outside the project: {rel}"));
    }
    let rel = rel_path.to_string_lossy().replace('\\', "/");
    Ok((rel, root.join(rel_path)))
}

struct Worker {
    dirs: ProjectDirs,
    runtime: PathBuf,
    everyone: Mailbox,
    voice: Voice,
    docs: HashMap<Key, Doc>,
    policies: HashMap<ProjectId, HashMap<String, RunPolicy>>,
    gens: HashMap<u64, Key>,
    next_gen: u64,
    probe: Arc<MachineProbe>,
    /// Projects whose own interpreters a person allowed to be run ("Detect kernels"), for this run
    /// of the host.
    consent: HashSet<ProjectId>,
    events_tx: flume::Sender<Tagged>,
    events: flume::Receiver<Tagged>,
    internal_tx: flume::Sender<Internal>,
    internal: flume::Receiver<Internal>,
    touched: HashMap<Key, HashSet<String>>,
}

enum Wake {
    Job(Result<Job, flume::RecvError>),
    Event(Tagged),
    Internal(Internal),
    Tick,
}

impl Worker {
    fn new(config_root: PathBuf, runtime: PathBuf, everyone: Mailbox, voice: Voice) -> Self {
        let (events_tx, events) = flume::unbounded();
        let (internal_tx, internal) = flume::unbounded();
        Worker {
            dirs: ProjectDirs::new(config_root),
            runtime,
            everyone,
            voice,
            docs: HashMap::new(),
            policies: HashMap::new(),
            gens: HashMap::new(),
            next_gen: 1,
            probe: Arc::new(MachineProbe::new()),
            consent: HashSet::new(),
            events_tx,
            events,
            internal_tx,
            internal,
            touched: HashMap::new(),
        }
    }

    fn run(mut self, inbox: flume::Receiver<Job>) {
        loop {
            let wake = flume::Selector::new()
                .recv(&inbox, Wake::Job)
                .recv(&self.events, |e| e.map_or(Wake::Tick, Wake::Event))
                .recv(&self.internal, |e| e.map_or(Wake::Tick, Wake::Internal))
                .wait_timeout(TICK)
                .unwrap_or(Wake::Tick);
            match wake {
                Wake::Job(Ok(job)) => self.job(job),
                Wake::Job(Err(_)) => break,
                Wake::Event(event) => {
                    self.kernel_event(event);
                    for _ in 0..BATCH {
                        match self.events.try_recv() {
                            Ok(event) => self.kernel_event(event),
                            Err(_) => break,
                        }
                    }
                }
                Wake::Internal(internal) => self.internal(internal),
                Wake::Tick => self.check_disk(),
            }
            self.flush();
        }
        // The host is going: no kernel outlives it.
        for (_, doc) in self.docs.drain() {
            if let Some(kernel) = doc.kernel {
                kernel.shutdown();
            }
        }
    }

    // ── sending ─────────────────────────────────────────────────────────────────────────────

    fn message(key: &Key, event: NotebookEvent) -> Message {
        Message::NotebookEvent {
            project_id: key.0,
            path: key.1.clone(),
            event: Box::new(event),
        }
    }

    fn tell(&self, key: &Key, event: NotebookEvent) {
        self.everyone.send(Self::message(key, event));
    }

    /// Queue `text` to the agent bound to the notebook, through the coordinator's doc queue.
    fn tell_agent(&self, key: &Key, text: String) {
        let Some(agent) = self.docs.get(key).and_then(|d| d.bound) else {
            return;
        };
        self.voice.say(Message::QueueDocEvent {
            project_id: key.0,
            agent_id: agent,
            event: DocEvent {
                doc: key.1.clone(),
                server: "kubiq-notebook".into(),
                text,
            },
        });
    }

    fn touch(&mut self, key: &Key, cell: &str) {
        self.touched.entry(key.clone()).or_default().insert(cell.to_string());
    }

    /// Send what kernel output changed since the last flush.
    fn flush(&mut self) {
        for (key, cells) in std::mem::take(&mut self.touched) {
            if let Some(doc) = self.docs.get(&key) {
                let change = Change {
                    cells: cells.into_iter().collect(),
                    order: false,
                };
                let event = changed(doc, &change, Origin::Human);
                self.tell(&key, event);
            }
        }
    }

    // ── documents ───────────────────────────────────────────────────────────────────────────

    fn open(&mut self, project_id: ProjectId, root: &Path, path: &str) -> Result<Key, String> {
        let (rel, abs) = inside(root, path)?;
        let key = (project_id, rel.clone());
        if self.docs.contains_key(&key) {
            return Ok(key);
        }
        let nb = match std::fs::read(&abs) {
            Ok(bytes) => {
                let text = String::from_utf8(bytes).map_err(|_| format!("Notebook not read: {rel}: not UTF-8"))?;
                Notebook::parse(&text, seed()).map_err(|e| format!("Notebook not read: {rel}: {e}"))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Notebook::new_empty(seed()),
            Err(e) => return Err(format!("Notebook not read: {rel}: {e}")),
        };
        self.docs.insert(
            key.clone(),
            Doc {
                project_id,
                rel,
                disk: stamp(&abs),
                abs,
                root: root.to_path_buf(),
                nb,
                views: HashSet::new(),
                conflict: false,
                kernel: None,
                generation: 0,
                state: KernelState::Off,
                label: None,
                spec: None,
                candidates: Vec::new(),
                queue: VecDeque::new(),
                current: None,
                pending: Vec::new(),
                allow_session: HashSet::new(),
                finished: HashMap::new(),
                bound: None,
                default_policy: RunPolicy::Ask,
            },
        );
        Ok(key)
    }

    fn policy(&mut self, key: &Key) -> RunPolicy {
        let fallback = self.docs.get(key).map(|d| d.default_policy).unwrap_or_default();
        let dirs = &self.dirs;
        self.policies
            .entry(key.0)
            .or_insert_with(|| {
                let file = dirs.data(key.0).local().join(POLICY_FILE);
                std::fs::read_to_string(file)
                    .ok()
                    .and_then(|t| serde_json::from_str::<HashMap<String, RunPolicy>>(&t).ok())
                    .unwrap_or_default()
            })
            .get(&key.1)
            .copied()
            .unwrap_or(fallback)
    }

    fn set_policy(&mut self, key: &Key, policy: RunPolicy) {
        self.policy(key);
        let map = self.policies.entry(key.0).or_default();
        map.insert(key.1.clone(), policy);
        let file = self.dirs.data(key.0).local().join(POLICY_FILE);
        let text = serde_json::to_string_pretty(map).unwrap_or_default();
        if let Err(error) = crate::atomic::write_atomic(&file, text.as_bytes()) {
            tracing::warn!("the notebook policy was not saved: {error}");
        }
        self.tell(key, NotebookEvent::Policy { policy });
    }

    fn save(&mut self, key: &Key, strip_outputs: bool) -> Result<(), String> {
        let doc = self.docs.get_mut(key).ok_or("Notebook not open")?;
        let text = if strip_outputs {
            doc.nb.serialize_without_outputs()
        } else {
            doc.nb.serialize()
        };
        let mode = std::fs::metadata(&doc.abs).ok().map(|m| m.permissions());
        crate::atomic::write_atomic_with(&doc.abs, text.as_bytes(), mode)
            .map_err(|e| format!("Notebook not saved: {}: {e}", doc.rel))?;
        doc.nb.mark_saved();
        doc.disk = stamp(&doc.abs);
        doc.conflict = false;
        let rev = doc.nb.rev;
        self.tell(key, NotebookEvent::Saved { rev });
        Ok(())
    }

    fn check_disk(&mut self) {
        let mut events = Vec::new();
        for (key, doc) in &mut self.docs {
            let now = stamp(&doc.abs);
            if now.is_none() || now == doc.disk {
                continue;
            }
            doc.disk = now;
            let Ok(text) = std::fs::read_to_string(&doc.abs) else {
                continue;
            };
            if text == doc.nb.serialize() {
                continue;
            }
            if doc.nb.dirty() {
                if !doc.conflict {
                    doc.conflict = true;
                    events.push((key.clone(), NotebookEvent::Conflict));
                }
                continue;
            }
            match Notebook::parse(&text, seed()) {
                Ok(fresh) => {
                    let change = doc.nb.reload(fresh.file);
                    events.push((key.clone(), changed(doc, &change, Origin::Disk)));
                }
                Err(error) => tracing::debug!("a notebook changed on disk and does not parse: {error}"),
            }
        }
        for (key, event) in events {
            self.tell(&key, event);
        }
    }

    // ── requests ────────────────────────────────────────────────────────────────────────────

    fn job(&mut self, job: Job) {
        match job {
            Job::Request {
                client,
                asker,
                project_id,
                root,
                path,
                request,
            } => {
                let key = match self.open(project_id, &root, &path) {
                    Ok(key) => key,
                    Err(error) => {
                        asker.send(Self::message(&(project_id, path), NotebookEvent::Failed { error }));
                        return;
                    }
                };
                if let Err(error) = self.request(client, &asker, &key, request) {
                    asker.send(Self::message(&key, NotebookEvent::Failed { error }));
                }
            }
            Job::Agent {
                project_id,
                root,
                agent,
                call,
                reply,
            } => {
                let _ = reply.send(self.agent(project_id, &root, &agent, call));
            }
            Job::Decide { project_id, root, path, run, allow, session } => {
                if let Ok(key) = self.open(project_id, &root, &path) {
                    self.decide(&key, run, allow, session);
                }
            }
            Job::ClientGone(client) => {
                let keys: Vec<Key> = self.docs.keys().cloned().collect();
                for key in keys {
                    self.leave(client, &key);
                }
            }
        }
    }

    fn leave(&mut self, client: ClientId, key: &Key) {
        let Some(doc) = self.docs.get_mut(key) else { return };
        if !doc.views.remove(&client) || !doc.views.is_empty() {
            return;
        }
        // The last view left: the kernel goes, unless an agent's run is waiting on it.
        if doc.pending.is_empty() && doc.queue.is_empty() && doc.current.is_none() {
            stop_kernel(doc);
            if !doc.nb.dirty() {
                self.docs.remove(key);
            }
        }
    }

    fn request(
        &mut self,
        client: ClientId,
        asker: &Mailbox,
        key: &Key,
        request: NotebookRequest,
    ) -> Result<(), String> {
        let policy = self.policy(key);
        let doc = self.docs.get_mut(key).ok_or("Notebook not open")?;
        match request {
            NotebookRequest::Open { default_policy } => {
                doc.default_policy = default_policy;
                doc.views.insert(client);
                asker.send(Self::message(
                    key,
                    NotebookEvent::Snapshot {
                        rev: doc.nb.rev,
                        cells: doc.nb.views(),
                        dirty: doc.nb.dirty(),
                        can_undo: doc.nb.can_undo(),
                        kernel: doc.state.clone(),
                        kernel_label: doc.label.clone(),
                        policy,
                        bound: doc.bound,
                    },
                ));
                for p in &doc.pending {
                    asker.send(Self::message(key, run_question(p)));
                }
            }
            NotebookRequest::Close => self.leave(client, key),
            NotebookRequest::Edit { rev: _, ops } => {
                let change = doc.nb.apply(ops, Origin::Human)?;
                let event = changed(doc, &change, Origin::Human);
                self.tell(key, event);
            }
            NotebookRequest::Undo => {
                if let Some((change, origin)) = doc.nb.undo() {
                    let event = changed(doc, &change, origin);
                    self.tell(key, event);
                }
            }
            NotebookRequest::Save { strip_outputs } => self.save(key, strip_outputs)?,
            NotebookRequest::ResolveKernels { run_project } => {
                if run_project {
                    self.consent.insert(key.0);
                }
                let run_project = self.consent.contains(&key.0);
                let Some(doc) = self.docs.get(key) else { return Ok(()) };
                let (probe, tx, key, asker) =
                    (self.probe.clone(), self.internal_tx.clone(), key.clone(), asker.clone());
                let (dir, root) = (doc.abs.parent().unwrap_or(&doc.root).to_path_buf(), doc.root.clone());
                std::thread::spawn(move || {
                    let candidates = env_resolver::resolve(&probe, &dir, &root, run_project);
                    let _ = tx.send(Internal::Resolved { key, candidates, asker });
                });
            }
            NotebookRequest::StartKernel { choice } => self.start_kernel(key, Some(choice)),
            NotebookRequest::Execute { run, cell } => {
                self.enqueue(key, vec![Run { run, cell, origin: Origin::Human, expected: None }]);
            }
            NotebookRequest::Interrupt => {
                if let Some(kernel) = doc.kernel.as_mut() {
                    kernel.interrupt()?;
                }
            }
            NotebookRequest::Restart { run_all } => self.restart(key, run_all),
            NotebookRequest::Shutdown => {
                abort_all(doc);
                stop_kernel(doc);
                let finished = std::mem::take(&mut doc.finished);
                self.tell(key, NotebookEvent::Kernel { state: KernelState::Off, label: None });
                self.docs.get_mut(key).map(|d| d.finished = finished);
            }
            NotebookRequest::InputReply { run, value } => {
                let parent = doc
                    .current
                    .as_mut()
                    .filter(|c| c.run == run)
                    .and_then(|c| c.input_parent.take());
                if let (Some(parent), Some(kernel)) = (parent, doc.kernel.as_mut()) {
                    kernel.input_reply(parent, &value)?;
                }
            }
            NotebookRequest::SetPolicy { policy } => self.set_policy(key, policy),
            NotebookRequest::DecideRun { run, allow, session } => self.decide(key, run, allow, session),
            NotebookRequest::Env { action } => self.env_action(key, asker.clone(), action),
            NotebookRequest::Bind { agent } => {
                doc.bound = agent;
                self.tell(key, NotebookEvent::Bound { agent });
            }
            NotebookRequest::Ask { cell, text } => {
                if doc.bound.is_none() {
                    return Err("Not sent: no agent is bound to this notebook".into());
                }
                let source = doc.nb.cell(&cell).map(|c| c.source.clone()).unwrap_or_default();
                let excerpt: String = source.lines().take(3).collect::<Vec<_>>().join(" ⏎ ");
                self.tell_agent(key, format!("the user asks about cell {cell} (`{excerpt}`): {text}"));
            }
        }
        Ok(())
    }

    /// A person's answer to an agent's run waiting under `Ask` — from the notebook's header or
    /// from the agent's armed dialog (`Job::Decide`). The first answer wins; a later one finds
    /// nothing pending.
    fn decide(&mut self, key: &Key, run: NotebookRunId, allow: bool, session: bool) {
        let Some(doc) = self.docs.get_mut(key) else { return };
        let Some(at) = doc.pending.iter().position(|p| p.run == run) else {
            return;
        };
        let pending = doc.pending.remove(at);
        // An approval covers the source the question showed, nothing else.
        let changed = doc.nb.changed_since(&pending.shown);
        if allow && !changed.is_empty() {
            let note = format!(
                "Not run: {} changed since the agent asked; the question stays open",
                changed.join(", ")
            );
            doc.pending.insert(at, pending);
            self.tell(key, NotebookEvent::Failed { error: note.clone() });
            self.tell_agent(key, format!("{note}. Ask again with run_cells for the new source."));
            return;
        }
        if session && allow {
            doc.allow_session.insert(pending.agent.clone());
        }
        self.tell(key, NotebookEvent::RunDecided { run });
        let verdict = if allow { "approved" } else { "declined" };
        self.tell_agent(key, format!("the user {verdict} your run of cells {}", pending.cells.join(", ")));
        if allow {
            let runs = pending
                .shown
                .into_iter()
                .enumerate()
                .map(|(n, (cell, source))| Run {
                    // The first keeps the id the agent was given, so it can ask after it.
                    run: if n == 0 { pending.run } else { NotebookRunId::generate() },
                    cell,
                    origin: Origin::Agent(pending.agent.clone()),
                    expected: Some(source),
                })
                .collect();
            self.enqueue(key, runs);
        } else if let Some(doc) = self.docs.get_mut(key) {
            doc.finished.insert(run, RunStatus::Aborted);
        }
    }

    fn env_action(&mut self, key: &Key, asker: Mailbox, action: EnvAction) {
        let Some(doc) = self.docs.get_mut(key) else { return };
        doc.candidates.clear();
        let root = doc.root.clone();
        let tx = self.internal_tx.clone();
        let probe = self.probe.clone();
        let key = key.clone();
        std::thread::spawn(move || {
            let uv = env_resolver::which("uv");
            let uv_project = root.join("pyproject.toml").is_file() && root.join("uv.lock").is_file();
            let mut steps: Vec<Vec<String>> = Vec::new();
            let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<String>>();
            match (&action, &uv) {
                (EnvAction::CreateVenv, Some(uv)) => {
                    steps.push(s(&[uv, "venv"]));
                    if uv_project {
                        steps.push(s(&[uv, "add", "--dev", "ipykernel"]));
                    } else {
                        steps.push(s(&[uv, "pip", "install", "--python", ".venv", "ipykernel"]));
                    }
                }
                (EnvAction::InstallIpykernel { .. }, Some(uv)) if uv_project => {
                    steps.push(s(&[uv, "add", "--dev", "ipykernel"]));
                }
                (EnvAction::InstallIpykernel { python: Some(python) }, _)
                    if Path::new(python).starts_with(&root) =>
                {
                    steps.push(s(&[python, "-m", "pip", "install", "ipykernel"]));
                }
                _ => {}
            }
            if steps.is_empty() {
                let _ = tx.send(Internal::EnvDone {
                    key,
                    asker,
                    ok: false,
                    log: "Nothing to run: no uv, and no interpreter inside the project".into(),
                });
                return;
            }
            let mut log = String::new();
            let mut ok = true;
            for step in steps {
                log.push_str(&format!("$ {}\n", step.join(" ")));
                let out = std::process::Command::new(&step[0])
                    .args(&step[1..])
                    .current_dir(&root)
                    .stdin(std::process::Stdio::null())
                    .output();
                match out {
                    Ok(out) => {
                        log.push_str(&String::from_utf8_lossy(&out.stdout));
                        log.push_str(&String::from_utf8_lossy(&out.stderr));
                        if !out.status.success() {
                            ok = false;
                            break;
                        }
                    }
                    Err(e) => {
                        log.push_str(&format!("{e}\n"));
                        ok = false;
                        break;
                    }
                }
            }
            let _ = ENV_TIMEOUT; // the commands run to completion; a hung install is the user's to stop
            if let Some(python) = match &action {
                EnvAction::InstallIpykernel { python } => python.clone(),
                EnvAction::CreateVenv => None,
            } {
                probe.forget(&python);
            }
            let _ = tx.send(Internal::EnvDone { key, asker, ok, log });
        });
    }

    // ── kernels ─────────────────────────────────────────────────────────────────────────────

    /// Start (or restart) the notebook's kernel on a thread. `None` reuses the last spec, or picks
    /// the default candidate — only with the person's consent to run the project's interpreters
    /// ("Detect kernels"); a candidate or interpreter the person picked is that consent for itself.
    fn start_kernel(&mut self, key: &Key, choice: Option<KernelChoice>) {
        let run_project = self.consent.contains(&key.0);
        let want_default = choice.is_none();
        let generation = self.next_gen;
        self.next_gen += 1;
        let Some(doc) = self.docs.get_mut(key) else { return };
        stop_kernel(doc);
        doc.generation = generation;
        doc.state = KernelState::Starting;
        self.gens.insert(generation, key.clone());
        let last = doc.spec.clone().filter(|_| choice.is_none());
        let candidates = doc.candidates.clone();
        let (cwd, root) = (doc.abs.parent().unwrap_or(&doc.root).to_path_buf(), doc.root.clone());
        let (probe, tx, events, runtime, key2) = (
            self.probe.clone(),
            self.internal_tx.clone(),
            self.events_tx.clone(),
            self.runtime.clone(),
            key.clone(),
        );
        let label = doc.label.clone();
        self.tell(key, NotebookEvent::Kernel { state: KernelState::Starting, label });
        std::thread::spawn(move || {
            let mut candidates = candidates;
            let picked: Result<(KernelSpec, String), String> = match (last, choice) {
                (Some(spec), _) => Ok(spec),
                (None, Some(KernelChoice::Interpreter(python))) => {
                    let c = kubiq_notebook::env::manual(probe.as_ref(), &python);
                    Ok((c.spec, c.label))
                }
                (None, None) if !run_project => Err(
                    "No kernel: pick one, or Detect kernels to let Kubiq run the project's Python".to_string(),
                ),
                (None, choice) => {
                    if candidates.is_empty() || (want_default && run_project) {
                        candidates = env_resolver::resolve(&probe, &cwd, &root, run_project);
                    }
                    let want = match choice {
                        Some(KernelChoice::Candidate(id)) => Some(id),
                        _ => None,
                    };
                    let found = match &want {
                        Some(id) => candidates.iter().find(|c| &c.id == id),
                        None => candidates
                            .iter()
                            .find(|c| c.ipykernel == Some(true))
                            .or_else(|| candidates.iter().find(|c| c.spec.language == "python" && c.python.is_none())),
                    };
                    found
                        .map(|c| (c.spec.clone(), c.label.clone()))
                        .ok_or_else(|| "No kernel: no interpreter with ipykernel found; pick one".to_string())
                }
            };
            let (result, label) = match picked {
                Ok((spec, label)) => (
                    kernel::start(Launch { spec: spec.clone(), cwd, runtime_dir: runtime }, generation, events).map(|k| (k, spec)),
                    label,
                ),
                Err(e) => (Err(e), String::new()),
            };
            let _ = tx.send(Internal::Started { key: key2, generation, label, candidates, result });
        });
    }

    fn restart(&mut self, key: &Key, run_all: bool) {
        let Some(doc) = self.docs.get_mut(key) else { return };
        abort_all(doc);
        doc.allow_session.clear();
        let cells: Vec<String> = doc
            .nb
            .file
            .cells
            .iter()
            .filter(|c| c.kind == CellKind::Code)
            .map(|c| c.id.clone())
            .collect();
        self.start_kernel(key, None);
        if run_all {
            let runs = cells
                .into_iter()
                .map(|cell| Run { run: NotebookRunId::generate(), cell, origin: Origin::Human, expected: None })
                .collect();
            self.enqueue(key, runs);
        }
    }

    fn enqueue(&mut self, key: &Key, runs: Vec<Run>) {
        let Some(doc) = self.docs.get_mut(key) else { return };
        let mut events = Vec::new();
        for run in runs {
            events.push(NotebookEvent::Running {
                run: run.run,
                cell: run.cell.clone(),
                origin: run.origin.clone(),
            });
            doc.queue.push_back(run);
        }
        let start = doc.kernel.is_none() && !matches!(doc.state, KernelState::Starting);
        for e in events {
            self.tell(key, e);
        }
        if start {
            self.start_kernel(key, None);
        } else {
            self.pump(key);
        }
    }

    /// Start the next queued run when the kernel is free.
    fn pump(&mut self, key: &Key) {
        loop {
            let Some(doc) = self.docs.get_mut(key) else { return };
            if doc.current.is_some() || doc.kernel.is_none() {
                return;
            }
            let Some(next) = doc.queue.pop_front() else { return };
            // An approved agent run whose cell was edited since runs nothing.
            if let Some(expected) = &next.expected
                && doc.nb.cell(&next.cell).is_none_or(|c| &c.source != expected)
            {
                doc.finished.insert(next.run, RunStatus::Aborted);
                let cell = next.cell.clone();
                self.tell(key, NotebookEvent::Done { run: next.run, cell: next.cell, count: None, status: RunStatus::Aborted });
                self.tell_agent(key, format!("cell {cell} was not run: it changed since it was approved"));
                continue;
            }
            let source = match doc.nb.cell(&next.cell) {
                Some(c) if c.kind == CellKind::Code && !c.source.trim().is_empty() => c.source.clone(),
                _ => {
                    doc.finished.insert(next.run, RunStatus::Ok);
                    let event = NotebookEvent::Done { run: next.run, cell: next.cell, count: None, status: RunStatus::Ok };
                    self.tell(key, event);
                    continue;
                }
            };
            doc.nb.run_started(&next.cell, &source);
            let sent = doc.kernel.as_mut().map(|k| k.execute(&source));
            match sent {
                Some(Ok(msg_id)) => {
                    doc.current = Some(Current {
                        run: next.run,
                        origin: next.origin.clone(),
                        cell: next.cell.clone(),
                        msg_id,
                        count: None,
                        status: RunStatus::Ok,
                        idle: false,
                        replied: false,
                        clear_wait: false,
                        input_parent: None,
                    });
                    // Every cell's stale mark may have moved.
                    let change = Change { cells: vec![next.cell], order: false };
                    let event = changed(doc, &change, next.origin);
                    self.tell(key, event);
                }
                _ => {
                    doc.finished.insert(next.run, RunStatus::Aborted);
                    let event = NotebookEvent::Done { run: next.run, cell: next.cell, count: None, status: RunStatus::Aborted };
                    self.tell(key, event);
                }
            }
            return;
        }
    }

    fn internal(&mut self, internal: Internal) {
        match internal {
            Internal::Started { key, generation, label, candidates, result } => {
                let Some(doc) = self.docs.get_mut(&key) else {
                    if let Ok((kernel, _)) = result {
                        std::thread::spawn(move || kernel.shutdown());
                    }
                    return;
                };
                if doc.generation != generation {
                    if let Ok((kernel, _)) = result {
                        std::thread::spawn(move || kernel.shutdown());
                    }
                    return;
                }
                if !candidates.is_empty() {
                    doc.candidates = candidates;
                }
                match result {
                    Ok((kernel, spec)) => {
                        doc.kernel = Some(kernel);
                        doc.state = KernelState::Idle;
                        doc.label = Some(label.clone());
                        doc.spec = Some((spec, label.clone()));
                        self.tell(&key, NotebookEvent::Kernel { state: KernelState::Idle, label: Some(label) });
                        self.pump(&key);
                    }
                    Err(error) => {
                        doc.state = KernelState::Dead { code: None, tail: error.clone() };
                        abort_all(doc);
                        self.tell(&key, NotebookEvent::Kernel { state: KernelState::Dead { code: None, tail: error }, label: None });
                    }
                }
            }
            Internal::Resolved { key, candidates, asker } => {
                if let Some(doc) = self.docs.get_mut(&key) {
                    doc.candidates = candidates.clone();
                }
                asker.send(Self::message(&key, NotebookEvent::Kernels { candidates }));
            }
            Internal::EnvDone { key, asker, ok, log } => {
                asker.send(Self::message(&key, NotebookEvent::EnvDone { ok, log }));
            }
        }
    }

    fn kernel_event(&mut self, (generation, event): Tagged) {
        let Some(key) = self.gens.get(&generation).cloned() else { return };
        let Some(doc) = self.docs.get_mut(&key) else { return };
        if doc.generation != generation {
            return;
        }
        match event {
            KernelEvent::Exited { code, tail } => {
                doc.kernel = None;
                doc.allow_session.clear();
                abort_all(doc);
                doc.state = KernelState::Dead { code, tail: tail.clone() };
                self.gens.remove(&generation);
                let label = doc.label.clone();
                self.tell(&key, NotebookEvent::Kernel { state: KernelState::Dead { code, tail }, label });
            }
            KernelEvent::Message(channel, msg) => self.kernel_message(&key, channel, msg),
        }
    }

    fn kernel_message(&mut self, key: &Key, channel: Channel, msg: JupyterMessage) {
        let Some(doc) = self.docs.get_mut(key) else { return };
        let mine = doc.current.as_ref().is_some_and(|c| c.msg_id == msg.parent_id());
        let mut events: Vec<NotebookEvent> = Vec::new();
        let mut touched: Vec<String> = Vec::new();
        match (channel, msg.msg_type()) {
            (Channel::Iopub, "status") => {
                let busy = msg.content.get("execution_state").and_then(Json::as_str) == Some("busy");
                let state = if busy { KernelState::Busy } else { KernelState::Idle };
                if doc.state != state && !matches!(doc.state, KernelState::Dead { .. }) {
                    doc.state = state.clone();
                    events.push(NotebookEvent::Kernel { state, label: doc.label.clone() });
                }
                if mine && !busy && let Some(current) = doc.current.as_mut() {
                    current.idle = true;
                }
            }
            (Channel::Iopub, "execute_input") if mine => {
                if let Some(current) = doc.current.as_mut() {
                    current.count = msg.content.get("execution_count").and_then(Json::as_i64);
                }
            }
            (Channel::Iopub, "clear_output") if mine => {
                let wait = msg.content.get("wait") == Some(&Json::Bool(true));
                if let Some(current) = doc.current.as_mut() {
                    if wait {
                        current.clear_wait = true;
                    } else {
                        let cell = current.cell.clone();
                        doc.nb.clear_outputs(&cell);
                        touched.push(cell);
                    }
                }
            }
            (Channel::Iopub, "update_display_data") => {
                if let Some(out) = output_from_message("update_display_data", &msg.content)
                    && let Some(id) = out.display_id.clone()
                {
                    touched.extend(doc.nb.update_display(&id, out));
                }
            }
            (Channel::Iopub, kind @ ("stream" | "display_data" | "execute_result" | "error")) if mine => {
                if let (Some(out), Some(current)) = (output_from_message(kind, &msg.content), doc.current.as_mut()) {
                    let cell = current.cell.clone();
                    if current.clear_wait {
                        current.clear_wait = false;
                        doc.nb.clear_outputs(&cell);
                    }
                    if kind == "error" {
                        current.status = RunStatus::Error;
                    }
                    if kind == "execute_result" {
                        current.count = msg.content.get("execution_count").and_then(Json::as_i64).or(current.count);
                    }
                    doc.nb.push_output(&cell, out);
                    touched.push(cell);
                }
            }
            (Channel::Shell, "execute_reply") if mine => {
                if let Some(current) = doc.current.as_mut() {
                    current.replied = true;
                    current.count = msg.content.get("execution_count").and_then(Json::as_i64).or(current.count);
                    match msg.content.get("status").and_then(Json::as_str) {
                        Some("error") => current.status = RunStatus::Error,
                        Some("aborted") => current.status = RunStatus::Aborted,
                        _ => {}
                    }
                }
            }
            (Channel::Stdin, "input_request") => {
                if let Some(current) = doc.current.as_mut() {
                    current.input_parent = Some(msg.header.clone());
                    events.push(NotebookEvent::InputRequest {
                        run: current.run,
                        cell: current.cell.clone(),
                        prompt: msg.content.get("prompt").and_then(Json::as_str).unwrap_or("").to_string(),
                        password: msg.content.get("password") == Some(&Json::Bool(true)),
                    });
                }
            }
            _ => {}
        }
        let done = doc.current.as_ref().is_some_and(|c| c.idle && c.replied);
        let mut agent_note = None;
        if done && let Some(current) = doc.current.take() {
            if matches!(current.origin, Origin::Agent(_)) {
                agent_note = Some(format!(
                    "your run of cell {} finished: {:?}; read_notebook to see its outputs",
                    current.cell, current.status
                ));
            }
            doc.nb.set_count(&current.cell, current.count);
            doc.finished.insert(current.run, current.status);
            touched.push(current.cell.clone());
            if current.status != RunStatus::Ok {
                // The kernel aborts what was queued behind an error (`stop_on_error`); say so.
                for run in doc.queue.drain(..) {
                    doc.finished.insert(run.run, RunStatus::Aborted);
                    events.push(NotebookEvent::Done { run: run.run, cell: run.cell, count: None, status: RunStatus::Aborted });
                }
            }
            events.insert(0, NotebookEvent::Done { run: current.run, cell: current.cell, count: current.count, status: current.status });
        }
        for cell in touched {
            self.touch(key, &cell);
        }
        if done {
            self.flush();
        }
        for event in events {
            self.tell(key, event);
        }
        if let Some(note) = agent_note {
            self.tell_agent(key, note);
        }
        if done {
            self.pump(key);
        }
    }

    // ── agents ──────────────────────────────────────────────────────────────────────────────

    fn agent(&mut self, project_id: ProjectId, root: &Path, agent: &str, call: AgentCall) -> Result<Value, String> {
        let path = match &call {
            AgentCall::List => return Ok(self.list(project_id, root)),
            AgentCall::Read { path, .. }
            | AgentCall::Edit { path, .. }
            | AgentCall::Run { path, .. }
            | AgentCall::Runs { path, .. }
            | AgentCall::Kernel { path, .. } => path.clone(),
        };
        if !path.ends_with(".ipynb") {
            return Err(format!("Not a notebook: {path}"));
        }
        let key = self.open(project_id, root, &path)?;
        let origin = Origin::Agent(agent.to_string());
        match call {
            AgentCall::List => unreachable!("answered above"),
            AgentCall::Read { full, cells, .. } => {
                let doc = &self.docs[&key];
                Ok(read(doc, full, cells.as_deref()))
            }
            AgentCall::Edit { ops, .. } => {
                let doc = self.docs.get_mut(&key).ok_or("Notebook not open")?;
                // A cell waiting for a person's yes keeps the source the question shows.
                let waiting: Vec<&String> = doc.pending.iter().flat_map(|p| p.cells.iter()).collect();
                for op in &ops {
                    let target = match op {
                        CellOp::Source { id, .. } | CellOp::Kind { id, .. } | CellOp::Delete { id } => Some(id),
                        _ => None,
                    };
                    if let Some(id) = target
                        && waiting.contains(&id)
                    {
                        return Err(format!(
                            "Refused: cell {id} has a run waiting for the user's answer; edit it after"
                        ));
                    }
                }
                let change = doc.nb.apply(ops, origin.clone())?;
                let order = doc.nb.order();
                let event = changed(doc, &change, origin);
                self.tell(&key, event);
                Ok(json!({"ok": true, "cells": order, "note": "Edited the live notebook; the user saves it."}))
            }
            AgentCall::Run { cells, .. } => {
                let policy = self.policy(&key);
                let doc = self.docs.get_mut(&key).ok_or("Notebook not open")?;
                for id in &cells {
                    if doc.nb.cell(id).is_none() {
                        return Err(format!("No cell with id {id}"));
                    }
                }
                let allowed = policy == RunPolicy::Allow || doc.allow_session.contains(agent);
                match policy {
                    RunPolicy::Deny => Err("Run refused: this notebook's agent policy is deny".into()),
                    _ if allowed => {
                        let runs: Vec<Run> = cells
                            .into_iter()
                            .map(|cell| Run { run: NotebookRunId::generate(), cell, origin: origin.clone(), expected: None })
                            .collect();
                        let ids: Vec<String> = runs.iter().map(|r| r.run.to_string()).collect();
                        self.enqueue(&key, runs);
                        Ok(json!({"status": "running", "runs": ids}))
                    }
                    _ => {
                        let run = NotebookRunId::generate();
                        let shown = cells
                            .iter()
                            .map(|id| (id.clone(), doc.nb.cell(id).map(|c| c.source.clone()).unwrap_or_default()))
                            .collect();
                        let pending = Pending { run, agent: agent.to_string(), cells, shown };
                        let question = run_question(&pending);
                        let gate = gate_question(&pending);
                        doc.pending.push(pending);
                        self.tell(&key, question);
                        // And as the agent's armed dialog, raised when its turn ends (`D210`).
                        if let Ok(agent_id) = agent.parse() {
                            self.voice.say(Message::ArmCellGate {
                                project_id: key.0,
                                agent_id,
                                doc: key.1.clone(),
                                server: "kubiq-notebook".into(),
                                run: run.to_string(),
                                question: gate,
                            });
                        }
                        Ok(json!({
                            "status": "pending",
                            "runs": [run.to_string()],
                            "note": "The user is asked in the notebook's header. End your turn; read_notebook later to see the outputs."
                        }))
                    }
                }
            }
            AgentCall::Runs { runs, .. } => {
                let doc = &self.docs[&key];
                let done = runs.iter().all(|r| doc.finished.contains_key(r));
                let statuses: Vec<Value> = runs
                    .iter()
                    .map(|r| json!({"run": r.to_string(), "status": doc.finished.get(r).map(|s| format!("{s:?}").to_lowercase())}))
                    .collect();
                Ok(json!({"done": done, "runs": statuses}))
            }
            AgentCall::Kernel { action, .. } => {
                let policy = self.policy(&key);
                let doc = self.docs.get_mut(&key).ok_or("Notebook not open")?;
                let allowed = policy == RunPolicy::Allow || doc.allow_session.contains(agent);
                match action.as_str() {
                    "status" => Ok(json!({
                        "state": format!("{:?}", doc.state),
                        "kernel": doc.label,
                        "queued": doc.queue.len(),
                        "policy": policy.as_str(),
                    })),
                    "interrupt" | "restart" if !allowed => Err(format!(
                        "Refused: {action} needs the notebook's agent policy to be allow (it is {})",
                        policy.as_str()
                    )),
                    "interrupt" => {
                        if let Some(kernel) = doc.kernel.as_mut() {
                            kernel.interrupt()?;
                        }
                        Ok(json!({"ok": true}))
                    }
                    "restart" => {
                        self.restart(&key, false);
                        Ok(json!({"ok": true}))
                    }
                    other => Err(format!("Unknown kernel action: {other}")),
                }
            }
        }
    }

    fn list(&self, project_id: ProjectId, root: &Path) -> Value {
        let mut out: Vec<Value> = self
            .docs
            .values()
            .filter(|d| d.project_id == project_id)
            .map(|d| {
                json!({
                    "path": d.rel,
                    "open": true,
                    "kernel": format!("{:?}", d.state),
                    "dirty": d.nb.dirty(),
                    "cells": d.nb.file.cells.len(),
                })
            })
            .collect();
        let walker = ignore::WalkBuilder::new(root).max_depth(Some(8)).build();
        for entry in walker.flatten().take(5000) {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "ipynb")
                && let Ok(rel) = p.strip_prefix(root)
            {
                let rel = rel.to_string_lossy().replace('\\', "/");
                if !out.iter().any(|v| v["path"] == rel.as_str()) {
                    out.push(json!({"path": rel, "open": false}));
                }
                if out.len() >= 200 {
                    break;
                }
            }
        }
        json!({"notebooks": out})
    }
}

/// Take the kernel down, off the worker's thread.
fn stop_kernel(doc: &mut Doc) {
    if let Some(kernel) = doc.kernel.take() {
        std::thread::spawn(move || kernel.shutdown());
    }
    // A start still in flight belongs to no generation now: when it lands it is shut down
    // (`Internal::Started`), never installed.
    doc.generation = 0;
    doc.state = KernelState::Off;
}

/// End the running and queued runs as aborted.
fn abort_all(doc: &mut Doc) {
    if let Some(current) = doc.current.take() {
        doc.finished.insert(current.run, RunStatus::Aborted);
    }
    for run in doc.queue.drain(..) {
        doc.finished.insert(run.run, RunStatus::Aborted);
    }
}

fn changed(doc: &Doc, change: &Change, origin: Origin) -> NotebookEvent {
    NotebookEvent::Changed {
        rev: doc.nb.rev,
        order: doc.nb.order(),
        cells: change.cells.iter().filter_map(|id| doc.nb.view(id)).collect(),
        stale: doc
            .nb
            .file
            .cells
            .iter()
            .filter(|c| doc.nb.stale(&c.id))
            .map(|c| c.id.clone())
            .collect(),
        origin,
        dirty: doc.nb.dirty(),
        can_undo: doc.nb.can_undo(),
    }
}

/// The armed dialog's question for an agent's waiting run: the cells and their source, and the
/// gate's three answers.
fn gate_question(p: &Pending) -> kubiq_proto::ask::AskQuestion {
    use kubiq_proto::ask::{AskOption, AskQuestion};
    let mut listed = String::new();
    for id in &p.cells {
        let source = p.shown.iter().find(|(c, _)| c == id).map(|(_, s)| s.clone()).unwrap_or_default();
        listed.push_str(&format!("\n\ncell {id}:\n{}", output::trim(&source, 20, 5)));
    }
    let option = |label: &str, description: &str| AskOption {
        label: label.to_string(),
        description: description.to_string(),
        preview: None,
    };
    AskQuestion {
        question: format!("Run {} notebook cell(s) for the agent?{listed}", p.cells.len()),
        header: "Run cells".into(),
        options: vec![
            option(GATE_ALLOW, "Run these cells now, on the notebook's kernel."),
            option(GATE_SESSION, "Run these and let this agent run cells here until the kernel restarts."),
            option(GATE_DENY, "Do not run them."),
        ],
        multi_select: false,
    }
}

fn run_question(p: &Pending) -> NotebookEvent {
    NotebookEvent::RunRequested {
        run: p.run,
        agent: p.agent.clone(),
        cells: p
            .cells
            .iter()
            .map(|id| (id.clone(), p.shown.iter().find(|(c, _)| c == id).map(|(_, s)| s.clone()).unwrap_or_default()))
            .collect(),
    }
}

/// A notebook as an agent reads it: every cell with its id, kind, count and stale mark, its source,
/// and its outputs as text — trimmed head and tail unless `full`. Outputs are fenced: they are
/// data the kernel produced, not instructions.
fn read(doc: &Doc, full: bool, only: Option<&[String]>) -> Value {
    let mut text = String::new();
    for view in doc.nb.views() {
        if only.is_some_and(|ids| !ids.contains(&view.id)) {
            continue;
        }
        let count = view.execution_count.map_or(" ".into(), |n| n.to_string());
        text.push_str(&format!(
            "## cell {} · {} · [{count}]{}\n```{}\n{}\n```\n",
            view.id,
            view.kind_label(),
            if view.stale { " · stale" } else { "" },
            if view.kind == CellKind::Code { "python" } else { "" },
            view.source
        ));
        for out in &view.outputs {
            let body = if full {
                match out {
                    kubiq_notebook::Output::Stream { text, .. } => output::terminal_text(text),
                    other => output::agent_text(other),
                }
            } else {
                output::agent_text(out)
            };
            text.push_str(&format!("<output>\n{body}\n</output>\n"));
        }
    }
    json!({
        "path": doc.rel,
        "kernel": format!("{:?}", doc.state),
        "dirty": doc.nb.dirty(),
        "notebook": text,
    })
}

trait KindLabel {
    fn kind_label(&self) -> &'static str;
}

impl KindLabel for kubiq_notebook::CellView {
    fn kind_label(&self) -> &'static str {
        self.kind.as_str()
    }
}
