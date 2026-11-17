//! The worker (feature `service`): everything a runbook does to the world. It reads and writes the
//! document, appends the evidence, runs captured commands, keeps the gate's pending requests and
//! the change log the agent polls, and tells whoever holds the [`Sink`] what changed.
//!
//! **Where things are.** The document is the project's file (`DocRef::abs`). Its evidence lives
//! beside it under the host's data for the project (`DocRef::data`, resolved by the host, never
//! composed here): `evidence.jsonl`, `env` (`$RUNBOOK_ENV`) and `logs/<cell>-<attempt>.log`.
//!
//! **A captured run** is a fresh process per cell — `sh -c` or `shell=` — with no terminal, its own
//! process group, stdout and stderr streamed, the bound secrets masked before any byte is written,
//! sent or kept. A timeout or Stop ends the whole group. Several cells may run at once, each with
//! its own log and tail; their results are written into the document one at a time.
//!
//! **Templating and history are not here.** A [`Provider`] (registered by an edition) binds the
//! parameters, may lock a document, is told every record, and offers actions; with none, a
//! runbook is a scratch run bound to its literal defaults.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::evidence::{
    Decision, DecisionKind, Evidence, Finished, LOCAL_USER, Outcome, Record, Started, Verdict,
};
use crate::excerpt::Collector;
use crate::graph::{self, CheckState, StepState};
use crate::lint::{self, Lints};
use crate::model::{CellKind, Runbook};
use crate::params::{self, Bindings, Form};
use crate::redact::Masker;
use crate::view::{
    ActionOutcome, Approval, AttemptView, DocStatus, OutputQuery, OutputSlice, Pending,
    PendingKind, RunOptions, RunReport, Snapshot, Tail,
};

/// One runbook document: which project it is in, where it is, and where its evidence lives.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DocRef {
    /// The project's id, as the host spells it.
    pub project: String,
    /// The project's folder.
    pub root: PathBuf,
    /// The document, relative to `root`, `/`-separated.
    pub rel: String,
    /// The document's evidence directory, under the host's data for the project.
    pub data: PathBuf,
}

impl DocRef {
    pub fn abs(&self) -> PathBuf {
        self.root.join(&self.rel)
    }
}

/// The directory name a document's evidence lives under: a readable slug and a hash of the path,
/// so two documents never share one.
pub fn data_dir_name(rel: &str) -> String {
    let slug: String = rel
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    let slug: String = slug
        .chars()
        .rev()
        .take(48)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{slug}-{}", params::short_hash(rel.as_bytes()))
}

/// An edition's templating and history (the seam). Every method has the scratch answer as its
/// default, so a provider implements only what it owns, and answers `None` for a document that is
/// not its own.
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    /// The bound parameters of `doc`.
    fn bindings(&self, _doc: &DocRef, _text: &str, _book: &Runbook) -> Option<Bindings> {
        None
    }
    /// What this provider says about `doc`: its role, whether it is locked, its actions.
    fn status(&self, _doc: &DocRef, _text: &str, _book: &Runbook) -> Option<DocStatus> {
        None
    }
    /// A record was appended to `doc`'s evidence. `text` is the document as it stood.
    fn record(&self, _doc: &DocRef, _text: &str, _record: &Record) {}
    /// Run one of the actions `status` offered.
    fn action(
        &self,
        _doc: &DocRef,
        _text: &str,
        _action: &str,
        _fields: &BTreeMap<String, String>,
    ) -> Option<Result<ActionOutcome, String>> {
        None
    }
}

/// Where the worker says what happened. The host's adapter turns these into bus messages.
pub trait Sink: Send + Sync {
    fn event(&self, doc: &DocRef, event: Event);
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    State(Box<Snapshot>),
    Tail(Tail),
    /// An attempt ended: the agent's nudge and a notice.
    Finished {
        cell: String,
        attempt: u64,
        label: String,
    },
    /// An entry of the change log: what the agent bound to the document hears.
    Changed(Change),
}

/// The caps and rhythms.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Bytes one attempt's log may hold before it is truncated.
    pub log_cap: u64,
    /// Lines a tail frame carries.
    pub tail_lines: usize,
    /// How long chunks are batched before a frame goes out.
    pub frame: Duration,
    /// Between the polite stop and the kill.
    pub grace: Duration,
    /// The most a `read_output` answer carries.
    pub read_cap: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            log_cap: 16 * 1024 * 1024,
            tail_lines: 15,
            frame: Duration::from_millis(40),
            grace: Duration::from_secs(2),
            read_cap: 64 * 1024,
        }
    }
}

/// The full-output view reads at most this much.
pub const FULL_READ_CAP: usize = 2 * 1024 * 1024;

/// One entry of the change log `runbook_changes` reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub seq: u64,
    pub at: String,
    /// `edited`, `written`, `started`, `finished`, `verdict`, `decision`, `requested`, `resolved`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u64>,
    pub text: String,
}

/// What `write_runbook` answered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WriteOutcome {
    Written {
        rev: String,
        lints: Lints,
    },
    /// The file moved on since `base`: nothing was written.
    Conflict {
        rev: String,
    },
}

struct Control {
    stop: AtomicBool,
}

struct DocState {
    doc: DocRef,
    evidence: Evidence,
    pending: Vec<Pending>,
    running: HashMap<u64, Arc<Control>>,
    changes: Vec<Change>,
    seq: u64,
    next_request: u64,
    last_rev: String,
    /// Changes not yet told to the sink.
    outbox: Vec<Change>,
}

const MAX_CHANGES: usize = 500;

impl DocState {
    fn change(&mut self, kind: &str, cell: Option<&str>, attempt: Option<u64>, text: String) {
        self.seq += 1;
        let change = Change {
            seq: self.seq,
            at: now(),
            kind: kind.into(),
            cell: cell.map(str::to_string),
            attempt,
            text,
        };
        self.outbox.push(change.clone());
        self.changes.push(change);
        if self.changes.len() > MAX_CHANGES {
            let over = self.changes.len() - MAX_CHANGES;
            self.changes.drain(..over);
        }
    }
}

struct Inner {
    sink: Arc<dyn Sink>,
    providers: Vec<Arc<dyn Provider>>,
    limits: Limits,
    docs: Mutex<HashMap<PathBuf, DocState>>,
    /// Serialises every write of a document, so parallel finishes never race on the file.
    write: Mutex<()>,
}

/// The worker. Cheap to clone; every clone is the same worker.
#[derive(Clone)]
pub struct Service {
    inner: Arc<Inner>,
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Write `bytes` so a reader sees the old file or the new one: a sibling temp file, then a rename.
/// The file's permissions carry over.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "runbook".into());
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if let Ok(meta) = std::fs::metadata(path) {
            std::fs::set_permissions(&tmp, meta.permissions())?;
        }
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// What a document reads as now, everything derived.
struct Reading {
    text: String,
    book: Runbook,
    bindings: Bindings,
    commands: BTreeMap<String, graph::Command>,
    lints: Lints,
    states: graph::States,
    status: DocStatus,
}

impl Service {
    pub fn new(sink: Arc<dyn Sink>, providers: Vec<Arc<dyn Provider>>, limits: Limits) -> Service {
        Service {
            inner: Arc::new(Inner {
                sink,
                providers,
                limits,
                docs: Mutex::new(HashMap::new()),
                write: Mutex::new(()),
            }),
        }
    }

    // ── reading ──────────────────────────────────────────────────────────────────────────────

    fn evidence_path(doc: &DocRef) -> PathBuf {
        doc.data.join("evidence.jsonl")
    }

    fn env_path(doc: &DocRef) -> PathBuf {
        doc.data.join("env")
    }

    /// The document's state, loaded on first use. A start with no finish from an earlier process is
    /// closed as interrupted here, before anything reads the evidence.
    fn state<'a>(
        &self,
        docs: &'a mut HashMap<PathBuf, DocState>,
        doc: &DocRef,
    ) -> &'a mut DocState {
        let key = doc.abs();
        if !docs.contains_key(&key) {
            let text = std::fs::read_to_string(Self::evidence_path(doc)).unwrap_or_default();
            let (evidence, _bad) = Evidence::from_jsonl(&text);
            let mut state = DocState {
                doc: doc.clone(),
                evidence,
                pending: Vec::new(),
                running: HashMap::new(),
                changes: Vec::new(),
                seq: 0,
                next_request: 1,
                last_rev: String::new(),
                outbox: Vec::new(),
            };
            let lost = state.evidence.unfinished();
            for started in lost {
                let record = Record::Finished(Finished {
                    attempt: started.attempt,
                    at: now(),
                    outcome: Outcome::Interrupted,
                    passed: false,
                    took_ms: 0,
                    lines: 0,
                    bytes: 0,
                    truncated: false,
                    excerpt: String::new(),
                });
                let _ = self.append(doc, &mut state.evidence, record);
                state.change(
                    "finished",
                    Some(&started.cell),
                    Some(started.attempt),
                    format!(
                        "#{} interrupted: the host went away while it ran",
                        started.cell
                    ),
                );
            }
            docs.insert(key.clone(), state);
        }
        docs.get_mut(&key).expect("just inserted")
    }

    /// Append a record to the file and to memory, and tell the provider.
    fn append(&self, doc: &DocRef, evidence: &mut Evidence, record: Record) -> std::io::Result<()> {
        std::fs::create_dir_all(&doc.data)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(Self::evidence_path(doc))?;
        file.write_all(Evidence::line(&record).as_bytes())?;
        file.flush()?;
        let text = std::fs::read_to_string(doc.abs()).unwrap_or_default();
        for p in &self.inner.providers {
            p.record(doc, &text, &record);
        }
        evidence.records.push(record);
        Ok(())
    }

    fn read(&self, doc: &DocRef, evidence: &Evidence) -> Result<Reading, String> {
        let text = std::fs::read_to_string(doc.abs())
            .map_err(|e| format!("cannot read {}: {e}", doc.rel))?;
        Ok(self.reading(doc, text, evidence))
    }

    fn reading(&self, doc: &DocRef, text: String, evidence: &Evidence) -> Reading {
        let book = Runbook::parse(&text);
        let bindings = self
            .inner
            .providers
            .iter()
            .find_map(|p| p.bindings(doc, &text, &book))
            .unwrap_or_else(|| Bindings::defaults(&book));
        let status = self
            .inner
            .providers
            .iter()
            .find_map(|p| p.status(doc, &text, &book))
            .unwrap_or_default();
        let os = crate::shell::Os::current();
        let commands = graph::commands_on(&book, &bindings, os, &|p| on_path(p).is_some());
        let mut lints = lint::lint(&book, Some(&bindings));
        // An interpreter this host does not have is an error on the cell, not a failed run.
        for cell in book.cells.iter().filter(|c| c.kind != CellKind::Manual) {
            let program = |cmd: &graph::Command| {
                if cmd.target.is_some() {
                    "ssh".to_string()
                } else {
                    cmd.shell.clone()
                }
            };
            if let Some(cmd) = commands.get(&cell.id)
                && on_path(&program(cmd)).is_none()
            {
                lints.blocked.insert(cell.id.clone());
                lints.lints.push(lint::Lint {
                    severity: lint::Severity::Error,
                    code: "shell".into(),
                    cell: Some(cell.id.clone()),
                    line: cell.line,
                    message: format!(
                        "#{}: {} is not installed on this host",
                        cell.id,
                        program(cmd)
                    ),
                });
            }
        }
        let states = graph::states(&book, &commands, evidence, &lints);
        Reading {
            text,
            book,
            bindings,
            commands,
            lints,
            states,
            status,
        }
    }

    fn snapshot_of(&self, state: &DocState, r: &Reading) -> Snapshot {
        let attempts = state
            .evidence
            .attempts()
            .into_iter()
            .map(|a| {
                let f = a.finished.as_ref();
                AttemptView {
                    attempt: a.id(),
                    cell: a.started.cell.clone(),
                    run: a.started.run,
                    status: f
                        .map(|f| f.outcome.label())
                        .unwrap_or_else(|| "running".into()),
                    passed: a.passed(),
                    lines: f.map(|f| f.lines).unwrap_or(0),
                    bytes: f.map(|f| f.bytes).unwrap_or(0),
                    truncated: f.is_some_and(|f| f.truncated),
                    at: a.started.at.clone(),
                    by: a.started.by.clone(),
                }
            })
            .collect();
        let masker = Masker::new(&r.bindings.secrets());
        let env = std::fs::read_to_string(Self::env_path(&state.doc)).unwrap_or_default();
        Snapshot {
            rev: params::short_hash(r.text.as_bytes()),
            states: r.states.clone(),
            commands: r.commands.clone(),
            lints: r.lints.clone(),
            pending: state.pending.clone(),
            status: r.status.clone(),
            params: r
                .bindings
                .values
                .iter()
                .map(|(k, b)| {
                    let v = if b.secret {
                        params::MASK.to_string()
                    } else {
                        b.value.clone()
                    };
                    (k.clone(), v)
                })
                .collect(),
            attempts,
            env: masker.mask_all(&env),
        }
    }

    fn publish(&self, state: &mut DocState, reading: &Reading) {
        for change in std::mem::take(&mut state.outbox) {
            self.inner.sink.event(&state.doc, Event::Changed(change));
        }
        let snapshot = self.snapshot_of(state, reading);
        self.inner
            .sink
            .event(&state.doc, Event::State(Box::new(snapshot)));
    }

    /// Note an edit made outside the worker (the editor, an agent's own file tools).
    fn note_rev(state: &mut DocState, rev: &str) {
        if !state.last_rev.is_empty() && state.last_rev != rev {
            state.change("edited", None, None, "the document changed on disk".into());
        }
        state.last_rev = rev.to_string();
    }

    /// Open (or refresh) a document: recover, project, answer its state and publish it.
    pub fn open(&self, doc: &DocRef) -> Result<Snapshot, String> {
        let mut docs = lock(&self.inner.docs);
        let state = self.state(&mut docs, doc);
        self.project_locked(state);
        let reading = self.read(doc, &state.evidence)?;
        let rev = params::short_hash(reading.text.as_bytes());
        Self::note_rev(state, &rev);
        self.publish(state, &reading);
        Ok(self.snapshot_of(state, &reading))
    }

    /// The state without publishing it (the agent's read).
    pub fn snapshot(&self, doc: &DocRef) -> Result<(Snapshot, String), String> {
        let mut docs = lock(&self.inner.docs);
        let state = self.state(&mut docs, doc);
        let reading = self.read(doc, &state.evidence)?;
        let rev = params::short_hash(reading.text.as_bytes());
        Self::note_rev(state, &rev);
        Ok((self.snapshot_of(state, &reading), reading.text))
    }

    /// Rewrite the document's output blocks from the evidence, if they differ.
    fn project_locked(&self, state: &mut DocState) {
        let _w = lock(&self.inner.write);
        let path = state.doc.abs();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        let projected = crate::project::project(&text, &state.evidence);
        if projected != text && write_atomic(&path, projected.as_bytes()).is_ok() {
            state.last_rev = params::short_hash(projected.as_bytes());
        }
    }

    // ── the gate ─────────────────────────────────────────────────────────────────────────────

    /// File an agent's request. Returns at once; nothing runs.
    pub fn request(
        &self,
        doc: &DocRef,
        kind: PendingKind,
        cells: Vec<String>,
        why: String,
        by: String,
    ) -> Result<u64, String> {
        let mut docs = lock(&self.inner.docs);
        let state = self.state(&mut docs, doc);
        let reading = self.read(doc, &state.evidence)?;
        if let Some(why) = reading.status.no_runs() {
            return Err(format!("{} is locked: {why}", doc.rel));
        }
        if cells.is_empty() {
            return Err("name at least one cell".into());
        }
        let mut hashes = BTreeMap::new();
        for cell in &cells {
            let Some(c) = reading.book.cell(cell) else {
                return Err(format!("no cell #{cell}"));
            };
            match (kind, c.kind) {
                (PendingKind::Check, CellKind::Manual) => {}
                (PendingKind::Check, _) => {
                    return Err(format!("#{cell} is not a manual check; use request_run"));
                }
                (PendingKind::Run, CellKind::Manual) => {
                    return Err(format!("#{cell} is a manual check; use request_check"));
                }
                _ => {}
            }
            if let Some(cmd) = reading.commands.get(cell) {
                hashes.insert(cell.clone(), cmd.hash.clone());
            }
        }
        // The shared gate: under `readonly-auto` an accepted readonly ready cell runs at once.
        let auto = match kind {
            PendingKind::Check => None,
            PendingKind::Run => {
                let gate = crate::gate::Gate {
                    book: &reading.book,
                    states: &reading.states,
                    evidence: &state.evidence,
                    commands: &reading.commands,
                    bindings: &reading.bindings,
                    lints: &reading.lints,
                };
                match kubiq_cells::RunGate::decide(
                    &gate,
                    &kubiq_cells::GateRequest {
                        agent: by.clone(),
                        cells: cells.clone(),
                    },
                ) {
                    kubiq_cells::GateDecision::Refuse { reason } => return Err(reason),
                    kubiq_cells::GateDecision::Ask { .. } => None,
                    kubiq_cells::GateDecision::Run => Some(
                        cells
                            .iter()
                            .map(|c| Approval {
                                cell: c.clone(),
                                hash: hashes.get(c).cloned().unwrap_or_default(),
                            })
                            .collect::<Vec<_>>(),
                    ),
                }
            }
        };
        let id = state.next_request;
        state.next_request += 1;
        let text = format!(
            "{by} asks to {} {}: {why}",
            match kind {
                PendingKind::Run => "run",
                PendingKind::Check => "judge",
            },
            cells
                .iter()
                .map(|c| format!("#{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        state.pending.push(Pending {
            id,
            kind,
            cells,
            why,
            by: by.clone(),
            at: now(),
            hashes,
        });
        state.change("requested", None, None, text);
        self.publish(state, &reading);
        drop(docs);
        if let Some(approvals) = auto {
            let options = RunOptions {
                request: Some(id),
                policy: Some(format!("{} for {by}", crate::gate::READONLY_AUTO)),
                ..RunOptions::default()
            };
            self.run(doc, &approvals, &options);
        }
        Ok(id)
    }

    /// The human set a request aside without running it.
    pub fn dismiss(&self, doc: &DocRef, request: u64) -> Result<(), String> {
        let mut docs = lock(&self.inner.docs);
        let state = self.state(&mut docs, doc);
        let before = state.pending.len();
        state.pending.retain(|p| p.id != request);
        if state.pending.len() != before {
            state.change(
                "resolved",
                None,
                None,
                format!("request {request} dismissed"),
            );
        }
        let reading = self.read(doc, &state.evidence)?;
        self.publish(state, &reading);
        Ok(())
    }

    /// A person answered an agent's run request in the agent's armed dialog (`ArmCellGate`).
    ///
    /// **Allow** runs the requested cells only when every one's command hash — what was asked
    /// about, target included — is still the current one ([`gate_approvals`]); otherwise nothing
    /// runs and the request stays pending with a note saying which cells changed, for the viewer's
    /// bar or a fresh request. **Deny** resolves the request as skipped by a person, so the
    /// agent's `runbook_changes` says so. The run is the person's: no policy actor.
    pub fn decide_gate(
        &self,
        doc: &DocRef,
        request: u64,
        allow: bool,
        by: &str,
    ) -> Result<GateDecided, String> {
        let (snap, _) = self.snapshot(doc)?;
        let Some(pending) = snap.pending.iter().find(|p| p.id == request) else {
            return Ok(GateDecided::Gone);
        };
        if allow {
            match gate_approvals(pending, &snap.commands) {
                Ok(approvals) => {
                    let opts = RunOptions {
                        request: Some(request),
                        ..Default::default()
                    };
                    return Ok(GateDecided::Ran(self.run(doc, &approvals, &opts)));
                }
                Err(changed) => {
                    let mut docs = lock(&self.inner.docs);
                    let state = self.state(&mut docs, doc);
                    state.change(
                        "request",
                        None,
                        None,
                        format!(
                            "request {request} allowed by {by} but not run: {} changed since it was asked",
                            changed.join(", ")
                        ),
                    );
                    let reading = self.read(doc, &state.evidence)?;
                    self.publish(state, &reading);
                    return Ok(GateDecided::Changed(changed));
                }
            }
        }
        let mut docs = lock(&self.inner.docs);
        let state = self.state(&mut docs, doc);
        state.pending.retain(|p| p.id != request);
        state.change(
            "resolved",
            None,
            None,
            format!("request {request} skipped by {by}"),
        );
        let reading = self.read(doc, &state.evidence)?;
        self.publish(state, &reading);
        Ok(GateDecided::Skipped)
    }

    /// Run approved cells. Each cell is checked on its own: a changed command, a lint, a waiting
    /// step without `skip_prereqs`, a `confirm=` without `confirmed` refuses that cell only.
    pub fn run(&self, doc: &DocRef, approvals: &[Approval], opts: &RunOptions) -> RunReport {
        let mut report = RunReport::default();
        let mut docs = lock(&self.inner.docs);
        let state = self.state(&mut docs, doc);
        let reading = match self.read(doc, &state.evidence) {
            Ok(r) => r,
            Err(e) => {
                report.refused = approvals
                    .iter()
                    .map(|a| (a.cell.clone(), e.clone()))
                    .collect();
                return report;
            }
        };
        if let Some(why) = reading.status.no_runs() {
            report.refused = approvals
                .iter()
                .map(|a| (a.cell.clone(), format!("locked: {why}")))
                .collect();
            return report;
        }
        for approval in approvals {
            match self.check_approval(state, &reading, approval, opts) {
                Err(why) => report.refused.push((approval.cell.clone(), why)),
                Ok(started) => {
                    let id = started.attempt;
                    let cell = started.cell.clone();
                    match self.start(state, &reading, started, opts) {
                        Ok(()) => report.started.push((cell, id)),
                        Err(e) => report.refused.push((cell, e)),
                    }
                }
            }
        }
        if let Some(request) = opts.request {
            let before = state.pending.len();
            // A request is answered by the run of every cell it named, or of any of them.
            state.pending.retain(|p| p.id != request);
            if before != state.pending.len() {
                state.change("resolved", None, None, format!("request {request} run"));
            }
        }
        let reading = self.read(doc, &state.evidence).unwrap_or(reading);
        self.publish(state, &reading);
        report
    }

    fn check_approval(
        &self,
        state: &DocState,
        r: &Reading,
        approval: &Approval,
        opts: &RunOptions,
    ) -> Result<Started, String> {
        let id = &approval.cell;
        let cell = r.book.cell(id).ok_or_else(|| format!("no cell #{id}"))?;
        if cell.kind == CellKind::Manual {
            return Err(format!("#{id} is a manual check: record a verdict instead"));
        }
        if r.lints.blocked.contains(id) {
            let why = r
                .lints
                .lints
                .iter()
                .find(|l| l.cell.as_deref() == Some(id.as_str()))
                .map(|l| l.message.clone())
                .unwrap_or_else(|| "a lint error".into());
            return Err(format!("#{id} cannot run: {why}"));
        }
        let cmd = &r.commands[id];
        if cmd.hash != approval.hash {
            return Err(format!(
                "#{id} changed since it was approved (or the editor holds unsaved changes); review it and run it again"
            ));
        }
        if state
            .evidence
            .of_cell(id)
            .iter()
            .any(|a| a.finished.is_none())
        {
            return Err(format!("#{id} is already running"));
        }
        let prior = state.evidence.of_cell(id);
        let (of, prereqs, forced) = if cell.is_step() {
            let view = r
                .states
                .step(id)
                .ok_or_else(|| format!("#{id} is not in the graph"))?;
            let waiting = !view.waits_on.is_empty()
                && matches!(view.state, StepState::Waiting | StepState::Stale);
            if waiting && !opts.skip_prereqs {
                return Err(format!(
                    "#{id} waits on {}; confirm skipping its prerequisites to run it anyway",
                    view.waits_on
                        .iter()
                        .map(|w| format!("#{w}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            (None, r.states.proofs_for(&view.needs), waiting)
        } else {
            let step = cell.check_of.clone().unwrap_or_default();
            let view = r
                .states
                .step(&step)
                .ok_or_else(|| format!("no step #{step}"))?;
            let succeeded = view
                .latest
                .and_then(|a| state.evidence.attempt(a))
                .filter(|a| a.passed());
            let eligible = view
                .checks
                .iter()
                .find(|c| &c.cell == id)
                .is_some_and(|c| c.state != CheckState::Waiting);
            match succeeded {
                Some(a) if eligible || opts.skip_prereqs => (Some(a.id()), vec![], !eligible),
                _ if opts.skip_prereqs => (view.latest, vec![], true),
                _ => {
                    return Err(format!(
                        "#{id} checks #{step}, which has not succeeded; confirm skipping prerequisites to run it anyway"
                    ));
                }
            }
        };
        if cell.confirm.is_some() && !opts.confirmed {
            return Err(format!(
                "#{id} asks for confirmation: {}",
                cell.confirm.as_deref().unwrap_or_default()
            ));
        }
        let attempt = state.evidence.next_id();
        Ok(Started {
            attempt,
            cell: id.clone(),
            run: prior.len() as u32 + 1,
            prev: prior.last().map(|a| a.id()),
            by: opts.policy.clone().unwrap_or_else(|| LOCAL_USER.into()),
            at: now(),
            shell: cmd.shell.clone(),
            target: cmd.target.clone(),
            command: cmd.display.clone(),
            hash: cmd.hash.clone(),
            of,
            prereqs,
            forced,
            request: opts.request,
            log: format!("logs/{}-{attempt}.log", id),
        })
    }

    fn start(
        &self,
        state: &mut DocState,
        r: &Reading,
        started: Started,
        opts: &RunOptions,
    ) -> Result<(), String> {
        let cell = r.book.cell(&started.cell).cloned().expect("checked");
        let (exec, _) = params::subst(&cell.body, &r.bindings, Form::Exec);
        let secrets = r.bindings.secrets();
        let control = Arc::new(Control {
            stop: AtomicBool::new(false),
        });
        self.append(
            &state.doc,
            &mut state.evidence,
            Record::Started(started.clone()),
        )
        .map_err(|e| format!("cannot record the attempt: {e}"))?;
        state.running.insert(started.attempt, control.clone());
        state.change(
            "started",
            Some(&started.cell),
            Some(started.attempt),
            format!("#{} started (attempt {})", started.cell, started.attempt),
        );
        let job = Job {
            doc: state.doc.clone(),
            started,
            exec,
            secrets,
            timeout: cell
                .timeout_secs
                .or(opts.default_timeout_secs)
                .map(Duration::from_secs),
            log_cap: opts.log_cap_mib.map(|m| u64::from(m) * 1024 * 1024),
            retry: cell.retry,
            expect: cell.expect.clone(),
            check: !cell.is_step(),
            control,
        };
        let service = self.clone();
        std::thread::Builder::new()
            .name("kubiq-runbook-run".into())
            .spawn(move || service.execute(job))
            .map(|_| ())
            .map_err(|e| format!("cannot start a thread: {e}"))
    }

    /// Stop a running cell: its process group is asked to end, then killed.
    pub fn stop(&self, doc: &DocRef, cell: &str) -> Result<(), String> {
        let mut docs = lock(&self.inner.docs);
        let state = self.state(&mut docs, doc);
        let ids: Vec<u64> = state
            .evidence
            .of_cell(cell)
            .iter()
            .filter(|a| a.finished.is_none())
            .map(|a| a.id())
            .collect();
        if ids.is_empty() {
            return Err(format!("#{cell} is not running"));
        }
        for id in ids {
            if let Some(c) = state.running.get(&id) {
                c.stop.store(true, Ordering::SeqCst);
            }
        }
        Ok(())
    }

    /// Record a skip or an override.
    /// A person accepts `cell`'s readonly mark for the command they were shown: refused when the
    /// command's hash is no longer `hash`, so the acceptance binds exactly what was reviewed.
    pub fn accept_readonly(
        &self,
        doc: &DocRef,
        cell: &str,
        reason: String,
        hash: &str,
    ) -> Result<(), String> {
        let (snap, _) = self.snapshot(doc)?;
        match snap.commands.get(cell) {
            Some(cmd) if cmd.hash == hash => {}
            Some(_) => {
                return Err(format!(
                    "#{cell} changed since you reviewed it; review it again before accepting it"
                ));
            }
            None => return Err(format!("no cell #{cell}")),
        }
        self.decide(doc, cell, DecisionKind::AcceptReadonly, reason)
    }

    pub fn decide(
        &self,
        doc: &DocRef,
        cell: &str,
        kind: DecisionKind,
        reason: String,
    ) -> Result<(), String> {
        if reason.trim().is_empty() {
            return Err("say why".into());
        }
        let mut docs = lock(&self.inner.docs);
        let state = self.state(&mut docs, doc);
        let reading = self.read(doc, &state.evidence)?;
        if let Some(why) = reading.status.no_runs() {
            return Err(format!("locked: {why}"));
        }
        let c = reading
            .book
            .cell(cell)
            .ok_or_else(|| format!("no cell #{cell}"))?;
        match kind {
            DecisionKind::AcceptReadonly if !c.readonly || c.kind == CellKind::Manual => {
                return Err(format!("#{cell} is not a readonly cell"));
            }
            DecisionKind::AcceptReadonly => {}
            _ if !c.is_step() => {
                return Err(format!("#{cell} is a check; skip or override its step"));
            }
            _ => {}
        }
        let record = Record::Decision(Decision {
            id: state.evidence.next_id(),
            cell: cell.into(),
            kind,
            reason: reason.clone(),
            by: LOCAL_USER.into(),
            at: now(),
            hash: reading.commands[cell].hash.clone(),
        });
        self.append(doc, &mut state.evidence, record)
            .map_err(|e| format!("cannot record: {e}"))?;
        let word = match kind {
            DecisionKind::Skip => "skipped",
            DecisionKind::Override => "overridden",
            DecisionKind::AcceptReadonly => "accepted as readonly",
        };
        // A skip answers any request naming the cell.
        if kind == DecisionKind::Skip {
            state.pending.retain(|p| !p.cells.iter().any(|c| c == cell));
        }
        state.change(
            "decision",
            Some(cell),
            None,
            format!("#{cell} {word}: {reason}"),
        );
        self.project_locked(state);
        let reading = self.read(doc, &state.evidence)?;
        self.publish(state, &reading);
        Ok(())
    }

    /// Record a human verdict on a manual check. Only the human's path reaches this.
    pub fn verdict(
        &self,
        doc: &DocRef,
        cell: &str,
        pass: bool,
        note: String,
    ) -> Result<(), String> {
        let mut docs = lock(&self.inner.docs);
        let state = self.state(&mut docs, doc);
        let reading = self.read(doc, &state.evidence)?;
        if let Some(why) = reading.status.no_runs() {
            return Err(format!("locked: {why}"));
        }
        let c = reading
            .book
            .cell(cell)
            .ok_or_else(|| format!("no cell #{cell}"))?;
        if c.kind != CellKind::Manual {
            return Err(format!("#{cell} is not a manual check"));
        }
        let step = c.check_of.clone().unwrap_or_default();
        let of = reading
            .states
            .step(&step)
            .and_then(|s| s.latest)
            .and_then(|a| state.evidence.attempt(a))
            .filter(|a| a.passed())
            .map(|a| a.id());
        if of.is_none() {
            return Err(format!("#{cell} judges #{step}, which has not succeeded"));
        }
        let record = Record::Verdict(Verdict {
            id: state.evidence.next_id(),
            cell: cell.into(),
            pass,
            note,
            by: LOCAL_USER.into(),
            at: now(),
            of,
            hash: reading.commands[cell].hash.clone(),
        });
        self.append(doc, &mut state.evidence, record)
            .map_err(|e| format!("cannot record: {e}"))?;
        state
            .pending
            .retain(|p| !(p.kind == PendingKind::Check && p.cells.iter().any(|c| c == cell)));
        state.change(
            "verdict",
            Some(cell),
            None,
            format!("#{cell} {}", if pass { "passed" } else { "failed" }),
        );
        self.project_locked(state);
        let reading = self.read(doc, &state.evidence)?;
        self.publish(state, &reading);
        Ok(())
    }

    // ── the agent's side ─────────────────────────────────────────────────────────────────────

    /// A whole-body write, checked against `base`. Host-owned output blocks are regenerated from
    /// the evidence whatever the text says about them.
    pub fn write(&self, doc: &DocRef, text: &str, base: &str) -> Result<WriteOutcome, String> {
        let mut docs = lock(&self.inner.docs);
        let state = self.state(&mut docs, doc);
        let current = std::fs::read_to_string(doc.abs()).unwrap_or_default();
        let rev = params::short_hash(current.as_bytes());
        if !current.is_empty() || !base.is_empty() {
            if rev != base {
                return Ok(WriteOutcome::Conflict { rev });
            }
            let reading = self.reading(doc, current, &state.evidence);
            if let Some(why) = &reading.status.locked {
                return Err(format!("locked: {why}"));
            }
        }
        let projected = crate::project::project(text, &state.evidence);
        {
            let _w = lock(&self.inner.write);
            write_atomic(&doc.abs(), projected.as_bytes())
                .map_err(|e| format!("cannot write {}: {e}", doc.rel))?;
        }
        let rev = params::short_hash(projected.as_bytes());
        state.last_rev = rev.clone();
        state.change("written", None, None, "the agent wrote the document".into());
        let reading = self.reading(doc, projected, &state.evidence);
        let lints = reading.lints.clone();
        self.publish(state, &reading);
        Ok(WriteOutcome::Written { rev, lints })
    }

    /// The change log after `since`, and the cursor to pass next.
    pub fn changes(&self, doc: &DocRef, since: u64) -> Result<(Vec<Change>, u64), String> {
        let mut docs = lock(&self.inner.docs);
        let state = self.state(&mut docs, doc);
        if let Ok(text) = std::fs::read_to_string(doc.abs()) {
            let rev = params::short_hash(text.as_bytes());
            Self::note_rev(state, &rev);
        }
        let out = state
            .changes
            .iter()
            .filter(|c| c.seq > since)
            .cloned()
            .collect();
        Ok((out, state.seq))
    }

    /// Read part of one attempt's retained log. `cell` must be the attempt's own.
    pub fn read_output(
        &self,
        doc: &DocRef,
        attempt: u64,
        cell: &str,
        query: &OutputQuery,
    ) -> Result<OutputSlice, String> {
        let a = {
            let mut docs = lock(&self.inner.docs);
            let state = self.state(&mut docs, doc);
            state
                .evidence
                .attempt(attempt)
                .ok_or_else(|| format!("no attempt {attempt}"))?
        };
        if a.started.cell != cell {
            return Err(format!(
                "attempt {attempt} is #{}, not #{cell}",
                a.started.cell
            ));
        }
        let path = doc.data.join(&a.started.log);
        let mut file = std::fs::File::open(&path).map_err(|_| {
            format!("the log of attempt {attempt} is missing (expired or never written)")
        })?;
        let cap = match query {
            OutputQuery::All => FULL_READ_CAP,
            _ => self.inner.limits.read_cap,
        };
        let total_bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mut out = String::new();
        let mut capped = false;
        let total_lines;
        let emit = |n: u64, line: &str, out: &mut String| -> bool {
            let row = format!("{n} | {}\n", crate::excerpt::strip_ansi(line));
            if out.len() + row.len() > cap {
                return false;
            }
            out.push_str(&row);
            true
        };
        match query {
            OutputQuery::Tail { n } => {
                // Count lines first, then emit the last n; logs are capped so a scan is bounded.
                let mut all = String::new();
                file.read_to_string(&mut all).ok();
                let lines: Vec<&str> = all.lines().collect();
                total_lines = lines.len() as u64;
                let from = lines.len().saturating_sub(*n as usize);
                let mut rows = Vec::new();
                let mut used = 0;
                for (i, l) in lines.iter().enumerate().skip(from).rev() {
                    let row = format!("{} | {}\n", i + 1, crate::excerpt::strip_ansi(l));
                    if used + row.len() > cap {
                        capped = true;
                        break;
                    }
                    used += row.len();
                    rows.push(row);
                }
                rows.reverse();
                out = rows.concat();
            }
            _ => {
                let mut all = String::new();
                file.seek(SeekFrom::Start(0)).ok();
                file.read_to_string(&mut all).ok();
                let lines: Vec<&str> = all.lines().collect();
                total_lines = lines.len() as u64;
                let keep: Box<dyn Fn(u64) -> bool> = match query {
                    OutputQuery::Head { n } => {
                        let n = *n;
                        Box::new(move |i| i <= n)
                    }
                    OutputQuery::Range { from, to } => {
                        let (from, to) = (*from, *to);
                        Box::new(move |i| i >= from && i <= to)
                    }
                    OutputQuery::Grep { pattern, context } => {
                        let re =
                            regex::Regex::new(pattern).map_err(|e| format!("bad pattern: {e}"))?;
                        let hits: Vec<u64> = lines
                            .iter()
                            .enumerate()
                            .filter(|(_, l)| re.is_match(l))
                            .map(|(i, _)| i as u64 + 1)
                            .collect();
                        let c = *context;
                        Box::new(move |i| hits.iter().any(|h| i + c >= *h && i <= h + c))
                    }
                    _ => Box::new(|_| true),
                };
                for (i, l) in lines.iter().enumerate() {
                    let n = i as u64 + 1;
                    if keep(n) && !emit(n, l, &mut out) {
                        capped = true;
                        break;
                    }
                }
            }
        }
        Ok(OutputSlice {
            cell: cell.into(),
            attempt,
            text: out,
            total_lines,
            total_bytes,
            truncated: a.finished.as_ref().is_some_and(|f| f.truncated),
            capped,
        })
    }

    /// Run a provider's action.
    pub fn action(
        &self,
        doc: &DocRef,
        action: &str,
        fields: &BTreeMap<String, String>,
    ) -> Result<ActionOutcome, String> {
        let text = std::fs::read_to_string(doc.abs())
            .map_err(|e| format!("cannot read {}: {e}", doc.rel))?;
        let result = self
            .inner
            .providers
            .iter()
            .find_map(|p| p.action(doc, &text, action, fields))
            .unwrap_or_else(|| Err(format!("no provider offers {action}")));
        if result.is_ok() {
            let _ = self.open(doc);
        }
        result
    }

    // ── a captured run ───────────────────────────────────────────────────────────────────────

    fn execute(&self, job: Job) {
        let t0 = Instant::now();
        let result = self.capture(&job);
        let (outcome, passed, lines, bytes, truncated, excerpt) = match result {
            Ok(c) => c,
            Err(reason) => (Outcome::Error { reason }, false, 0, 0, false, String::new()),
        };
        let finished = Finished {
            attempt: job.started.attempt,
            at: now(),
            outcome: outcome.clone(),
            passed,
            took_ms: t0.elapsed().as_millis() as u64,
            lines,
            bytes,
            truncated,
            excerpt,
        };
        let mut docs = lock(&self.inner.docs);
        let state = self.state(&mut docs, &job.doc);
        state.running.remove(&job.started.attempt);
        let _ = self.append(&job.doc, &mut state.evidence, Record::Finished(finished));
        let label = if job.check {
            format!(
                "{} ({})",
                outcome.label(),
                if passed { "passed" } else { "failed" }
            )
        } else {
            outcome.label()
        };
        state.change(
            "finished",
            Some(&job.started.cell),
            Some(job.started.attempt),
            format!("#{} finished: {label}", job.started.cell),
        );
        self.project_locked(state);
        self.inner.sink.event(
            &job.doc,
            Event::Finished {
                cell: job.started.cell.clone(),
                attempt: job.started.attempt,
                label,
            },
        );
        if let Ok(reading) = self.read(&job.doc, &state.evidence) {
            self.publish(state, &reading);
        }
    }

    /// Run the job's command (and its retries) and answer (outcome, passed, lines, bytes,
    /// truncated, excerpt).
    #[allow(clippy::type_complexity)]
    fn capture(&self, job: &Job) -> Result<(Outcome, bool, u64, u64, bool, String), String> {
        let limits = &self.inner.limits;
        let log_path = job.doc.data.join(&job.started.log);
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot create the log: {e}"))?;
        }
        let mut log =
            std::fs::File::create(&log_path).map_err(|e| format!("cannot create the log: {e}"))?;
        let env_path = Service::env_path(&job.doc);
        if !env_path.exists() {
            let _ = std::fs::write(&env_path, "");
        }
        let expect = match &job.expect {
            Some(p) => Some(regex::Regex::new(p).map_err(|e| format!("expect=: {e}"))?),
            None => None,
        };
        let mut sink = Capture {
            masker: Masker::new(&job.secrets),
            collector: Collector::new(),
            written: 0,
            cap: job.log_cap.unwrap_or(limits.log_cap),
            truncated: false,
            tail: Vec::new(),
            partial: String::new(),
            skipped: 0,
            matched_text: String::new(),
        };
        let tries = job.retry.map(|r| r.times).unwrap_or(1).max(1);
        let mut outcome = Outcome::Error {
            reason: "never ran".into(),
        };
        let mut passed = false;
        for attempt in 1..=tries {
            if attempt > 1 {
                let note = format!("[runbook: try {attempt} of {tries}]\n");
                sink.feed(note.as_bytes(), &mut log);
            }
            sink.matched_text.clear();
            outcome = self.run_once(job, &env_path, &mut sink, &mut log)?;
            passed = match &expect {
                Some(re) => re.is_match(&sink.matched_text),
                None => outcome == Outcome::Exit { code: 0 },
            };
            if !job.check {
                passed = outcome == Outcome::Exit { code: 0 };
            }
            if passed || attempt == tries || !matches!(outcome, Outcome::Exit { .. }) {
                break;
            }
            // Wait for the next try, still answering Stop.
            let every = Duration::from_secs(job.retry.map(|r| r.every_secs).unwrap_or(0));
            let until = Instant::now() + every;
            while Instant::now() < until {
                if job.control.stop.load(Ordering::SeqCst) {
                    return Ok(self.close_capture(sink, &mut log, Outcome::Cancelled, false, job));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        Ok(self.close_capture(sink, &mut log, outcome, passed, job))
    }

    fn close_capture(
        &self,
        mut sink: Capture,
        log: &mut std::fs::File,
        outcome: Outcome,
        passed: bool,
        job: &Job,
    ) -> (Outcome, bool, u64, u64, bool, String) {
        let rest = sink.masker.finish();
        sink.write(&rest, log);
        sink.flush_tail(self, job, true);
        let _ = log.flush();
        let truncated = sink.truncated;
        let (lines, bytes, excerpt) = sink.collector.finish();
        (outcome, passed, lines, bytes, truncated, excerpt)
    }

    fn run_once(
        &self,
        job: &Job,
        env_path: &Path,
        sink: &mut Capture,
        log: &mut std::fs::File,
    ) -> Result<Outcome, String> {
        // Locally the command is an owner-only script file, removed when this returns; over ssh
        // it is the far side's stdin. Never an argument: a secret must not reach the process table.
        let mut _script: Option<ScriptFile> = None;
        let how = match crate::shell::Target::parse(job.started.target.as_deref()) {
            crate::shell::Target::Ssh { alias, os } => {
                crate::shell::over_ssh(&alias, os, &job.started.shell, &job.exec)
            }
            _ => {
                let (ext, body) = crate::shell::script_file(&job.started.shell, &job.exec);
                let path = job.doc.data.join("tmp").join(format!(
                    "{}-{}.{ext}",
                    job.started.cell, job.started.attempt
                ));
                let file = ScriptFile::write(&path, body.as_bytes())
                    .map_err(|e| format!("cannot write the cell's script: {e}"))?;
                let how = crate::shell::invocation(&job.started.shell, &file.0.to_string_lossy());
                _script = Some(file);
                how
            }
        };
        let program = on_path(&how.program).unwrap_or_else(|| PathBuf::from(&how.program));
        let mut cmd = Process::new(program);
        cmd.args(&how.args)
            .current_dir(&job.doc.root)
            .env("RUNBOOK_ENV", env_path)
            .stdin(if how.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in read_env(env_path) {
            cmd.env(k, v);
        }
        // The shared cell layer's process group: a Unix process group, a Windows Job object.
        let (mut child, group) = kubiq_cells::process::spawn(&mut cmd)
            .map_err(|e| format!("cannot start {}: {e}", how.program))?;
        if let (Some(script), Some(mut stdin)) = (how.stdin, child.stdin.take()) {
            // On a thread of its own: a script larger than the pipe would otherwise block here
            // until the reader below drains output, and the loop that answers Stop and the timeout
            // would never run. The pipe closes when the thread ends, so the far side reads EOF.
            std::thread::spawn(move || {
                let _ = stdin.write_all(script.as_bytes());
            });
        }
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let mut readers = Vec::new();
        if let Some(out) = child.stdout.take() {
            readers.push(pump(out, tx.clone()));
        }
        if let Some(err) = child.stderr.take() {
            readers.push(pump(err, tx.clone()));
        }
        drop(tx);
        let started = Instant::now();
        let deadline = job.timeout.map(|t| started + t);
        let mut ending: Option<(Outcome, Instant)> = None;
        let mut last_frame = Instant::now();
        let limits = &self.inner.limits;
        loop {
            match rx.recv_timeout(limits.frame) {
                Ok(chunk) => {
                    let masked = sink.masker.push(&chunk);
                    sink.feed_masked(&masked, log);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if last_frame.elapsed() >= limits.frame {
                sink.flush_tail(self, job, false);
                last_frame = Instant::now();
            }
            let now = Instant::now();
            match &ending {
                None => {
                    if job.control.stop.load(Ordering::SeqCst) {
                        let _ = group.terminate();
                        ending = Some((Outcome::Cancelled, now + limits.grace));
                    } else if deadline.is_some_and(|d| now >= d) {
                        let _ = group.terminate();
                        ending = Some((Outcome::TimedOut, now + limits.grace));
                    }
                }
                Some((_, kill_at)) if now >= *kill_at => {
                    let _ = group.kill_tree();
                }
                _ => {}
            }
            // The group may hold the pipes open after the shell exits (a background child); once the
            // shell is gone and a stop is pending, the kill above ends them.
        }
        for r in readers {
            let _ = r.join();
        }
        let status = child.wait().map_err(|e| format!("cannot wait: {e}"))?;
        if let Some((outcome, _)) = ending {
            return Ok(outcome);
        }
        let code = status.code().unwrap_or_else(|| {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                128 + status.signal().unwrap_or(0)
            }
            #[cfg(not(unix))]
            {
                -1
            }
        });
        Ok(Outcome::Exit { code })
    }
}

/// `KEY=value` lines of the env file; blank lines and `#` comments skipped, `export ` tolerated.
pub fn read_env(path: &Path) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    // A file a Windows editor saved: a BOM and CRLF lines read the same.
    let text = crate::shell::normalise(&text);
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            let l = l.strip_prefix("export ").unwrap_or(l);
            let (k, v) = l.split_once('=')?;
            let k = k.trim();
            if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return None;
            }
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(v);
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

fn pump(
    mut from: impl Read + Send + 'static,
    to: std::sync::mpsc::Sender<Vec<u8>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match from.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if to.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    })
}

/// A cell's script on disk, owner-only, removed when dropped.
struct ScriptFile(PathBuf);

impl ScriptFile {
    fn write(path: &Path, bytes: &[u8]) -> std::io::Result<ScriptFile> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(ScriptFile(path.to_path_buf()))
    }
}

impl Drop for ScriptFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A program on this host's `PATH` (with `PATHEXT` on Windows), or the path itself when it names
/// an existing file.
pub fn on_path(program: &str) -> Option<PathBuf> {
    let direct = Path::new(program);
    if direct.components().count() > 1 {
        return direct.is_file().then(|| direct.to_path_buf());
    }
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|e| e.to_ascii_lowercase())
            .chain(std::iter::once(String::new()))
            .collect()
    } else {
        vec![String::new()]
    };
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        exts.iter()
            .map(|ext| dir.join(format!("{program}{ext}")))
            .find(|candidate| candidate.is_file())
    })
}

struct Job {
    doc: DocRef,
    started: Started,
    /// The command with secrets in it: handed to the shell and nowhere else.
    exec: String,
    secrets: Vec<String>,
    timeout: Option<Duration>,
    /// The person's log cap, over the worker's default.
    log_cap: Option<u64>,
    retry: Option<crate::model::Retry>,
    expect: Option<String>,
    check: bool,
    control: Arc<Control>,
}

/// One attempt's capture: masked bytes go to the log (up to its cap), the excerpt collector and the
/// tail.
struct Capture {
    masker: Masker,
    collector: Collector,
    written: u64,
    cap: u64,
    truncated: bool,
    tail: Vec<String>,
    partial: String,
    skipped: u64,
    /// This try's output for `expect=`, bounded.
    matched_text: String,
}

const EXPECT_CAP: usize = 1024 * 1024;

impl Capture {
    fn feed(&mut self, raw: &[u8], log: &mut std::fs::File) {
        let masked = self.masker.push(raw);
        self.feed_masked(&masked, log);
    }

    fn feed_masked(&mut self, masked: &[u8], log: &mut std::fs::File) {
        self.write(masked, log);
    }

    fn write(&mut self, masked: &[u8], log: &mut std::fs::File) {
        if masked.is_empty() {
            return;
        }
        self.collector.push(masked);
        if self.matched_text.len() < EXPECT_CAP {
            self.matched_text.push_str(&String::from_utf8_lossy(masked));
        }
        if !self.truncated {
            let room = self.cap.saturating_sub(self.written) as usize;
            let take = room.min(masked.len());
            let _ = log.write_all(&masked[..take]);
            self.written += take as u64;
            if take < masked.len() {
                self.truncated = true;
                let _ = log.write_all(
                    format!(
                        "\n[runbook: log capped at {}; later output discarded]\n",
                        crate::excerpt::size(self.cap)
                    )
                    .as_bytes(),
                );
            }
        }
        let text = String::from_utf8_lossy(masked);
        for ch in text.chars() {
            if ch == '\n' {
                let line = std::mem::take(&mut self.partial);
                self.tail.push(crate::excerpt::strip_ansi(&line));
            } else if self.partial.len() < 2000 {
                self.partial.push(ch);
            }
        }
    }

    fn flush_tail(&mut self, service: &Service, job: &Job, last: bool) {
        if last && !self.partial.is_empty() {
            let line = std::mem::take(&mut self.partial);
            self.tail.push(crate::excerpt::strip_ansi(&line));
        }
        if self.tail.is_empty() {
            return;
        }
        let keep = service.inner.limits.tail_lines;
        let mut lines = std::mem::take(&mut self.tail);
        let skipped = lines.len().saturating_sub(keep) as u64;
        if skipped > 0 {
            lines.drain(..skipped as usize);
        }
        self.skipped += skipped;
        service.inner.sink.event(
            &job.doc,
            Event::Tail(Tail {
                cell: job.started.cell.clone(),
                attempt: job.started.attempt,
                lines,
                skipped,
            }),
        );
    }
}

/// What [`Service::decide_gate`] did.
#[derive(Debug)]
pub enum GateDecided {
    /// Allowed, and the cells were handed to [`Service::run`] (which may still refuse one: a
    /// lint, a waiting step, a `confirm=`).
    Ran(RunReport),
    /// Allowed, but these cells' commands changed since the request: nothing ran.
    Changed(Vec<String>),
    /// Denied: resolved as skipped by a person.
    Skipped,
    /// No such request is pending (answered elsewhere first, or already run by policy).
    Gone,
}

/// The approvals a request still earns: one per requested cell, bound to the hash recorded when
/// the agent asked — `Err` with every cell whose current command hash differs from it, or that is
/// gone. A person who said yes to one command never runs another.
pub fn gate_approvals(
    pending: &Pending,
    commands: &BTreeMap<String, graph::Command>,
) -> Result<Vec<Approval>, Vec<String>> {
    let mut approvals = Vec::new();
    let mut changed = Vec::new();
    for cell in &pending.cells {
        match (pending.hashes.get(cell), commands.get(cell)) {
            (Some(asked), Some(now)) if *asked == now.hash => approvals.push(Approval {
                cell: cell.clone(),
                hash: asked.clone(),
            }),
            _ => changed.push(cell.clone()),
        }
    }
    if changed.is_empty() {
        Ok(approvals)
    } else {
        Err(changed)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_gate_approval_binds_the_hash_that_was_asked_about() {
        let command = |cell: &str, hash: &str| graph::Command {
            cell: cell.into(),
            shell: "sh".into(),
            display: String::new(),
            hash: hash.into(),
            unbound: Vec::new(),
            target: None,
        };
        let pending = Pending {
            id: 1,
            kind: PendingKind::Run,
            cells: vec!["a".into(), "b".into()],
            why: String::new(),
            by: "agent".into(),
            at: String::new(),
            hashes: [
                ("a".to_string(), "h1".to_string()),
                ("b".to_string(), "h2".to_string()),
            ]
            .into_iter()
            .collect(),
        };
        let same: BTreeMap<_, _> = [
            ("a".to_string(), command("a", "h1")),
            ("b".to_string(), command("b", "h2")),
        ]
        .into_iter()
        .collect();
        let approvals = gate_approvals(&pending, &same).unwrap();
        assert_eq!(
            approvals
                .iter()
                .map(|a| a.hash.as_str())
                .collect::<Vec<_>>(),
            ["h1", "h2"]
        );
        // An edited command (or a changed target, which is in the hash) runs nothing.
        let edited: BTreeMap<_, _> = [
            ("a".to_string(), command("a", "h1")),
            ("b".to_string(), command("b", "h9")),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            gate_approvals(&pending, &edited).unwrap_err(),
            vec!["b".to_string()]
        );
        // A cell that is gone is a change too.
        let gone: BTreeMap<_, _> = [("a".to_string(), command("a", "h1"))]
            .into_iter()
            .collect();
        assert_eq!(
            gate_approvals(&pending, &gone).unwrap_err(),
            vec!["b".to_string()]
        );
    }

    #[derive(Default)]
    struct Recorder {
        events: Mutex<Vec<Event>>,
    }

    impl Sink for Recorder {
        fn event(&self, _doc: &DocRef, event: Event) {
            lock(&self.events).push(event);
        }
    }

    struct Fixture {
        dir: PathBuf,
        doc: DocRef,
        service: Service,
        rec: Arc<Recorder>,
    }

    fn fixture(
        name: &str,
        text: &str,
        limits: Limits,
        providers: Vec<Arc<dyn Provider>>,
    ) -> Fixture {
        let dir = std::env::temp_dir().join(format!(
            "kubiq-runbook-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("project")).unwrap();
        std::fs::write(dir.join("project/ops.runbook.md"), text).unwrap();
        let doc = DocRef {
            project: "P".into(),
            root: dir.join("project"),
            rel: "ops.runbook.md".into(),
            data: dir.join("data").join(data_dir_name("ops.runbook.md")),
        };
        let rec = Arc::new(Recorder::default());
        let service = Service::new(rec.clone(), providers, limits);
        Fixture {
            dir,
            doc,
            service,
            rec,
        }
    }

    impl Fixture {
        fn approve(&self, cells: &[&str]) -> Vec<Approval> {
            let (snap, _) = self.service.snapshot(&self.doc).unwrap();
            cells
                .iter()
                .map(|c| Approval {
                    cell: c.to_string(),
                    hash: snap.commands[*c].hash.clone(),
                })
                .collect()
        }
        fn run(&self, cells: &[&str]) -> RunReport {
            self.service
                .run(&self.doc, &self.approve(cells), &RunOptions::default())
        }
        fn wait(&self) {
            let until = Instant::now() + Duration::from_secs(20);
            loop {
                let docs = lock(&self.service.inner.docs);
                let busy = docs.values().any(|s| !s.running.is_empty());
                drop(docs);
                if !busy {
                    return;
                }
                assert!(Instant::now() < until, "runs did not finish");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        fn state(&self, cell: &str) -> StepState {
            let (snap, _) = self.service.snapshot(&self.doc).unwrap();
            snap.states.step(cell).unwrap().state
        }
        fn text(&self) -> String {
            std::fs::read_to_string(self.doc.abs()).unwrap()
        }
        fn attempt(&self, cell: &str) -> crate::evidence::Attempt {
            let docs = lock(&self.service.inner.docs);
            docs.values()
                .next()
                .unwrap()
                .evidence
                .of_cell(cell)
                .pop()
                .unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn exit_codes_logs_and_the_output_block_land_and_the_doc_is_otherwise_untouched() {
        let text =
            "# Ops\n\n```sh {#ok}\necho hello; echo err >&2\n```\n\n```sh {#bad}\nexit 3\n```\n";
        let f = fixture("exit", text, Limits::default(), vec![]);
        let report = f.run(&["ok"]);
        assert_eq!(report.started.len(), 1, "{report:?}");
        f.wait();
        assert_eq!(f.state("ok"), StepState::Done);
        let a = f.attempt("ok");
        let log = std::fs::read_to_string(f.doc.data.join(&a.started.log)).unwrap();
        assert!(log.contains("hello") && log.contains("err"), "{log}");
        let doc = f.text();
        assert!(doc.starts_with("# Ops\n\n```sh {#ok}\necho hello; echo err >&2\n```\n\n```output {cell=ok run=1 attempt=1 exit=0 "), "{doc}");
        f.run(&["bad"]);
        f.wait();
        assert_eq!(f.state("bad"), StepState::Failed);
        assert!(f.text().contains("exit=3"));
        // The evidence file is append-only JSON lines.
        let ev = std::fs::read_to_string(f.doc.data.join("evidence.jsonl")).unwrap();
        assert_eq!(ev.lines().count(), 4);
    }

    #[test]
    fn approval_binds_the_hash_and_gates_waiting_steps_and_confirm() {
        let text = "```sh {#a}\ntrue\n```\n```sh {#b confirm=\"really?\"}\ntrue\n```\n";
        let f = fixture("gate", text, Limits::default(), vec![]);
        let stale = vec![Approval {
            cell: "a".into(),
            hash: "000000000000".into(),
        }];
        let r = f.service.run(&f.doc, &stale, &RunOptions::default());
        assert!(r.refused[0].1.contains("changed since"), "{r:?}");
        let r = f.run(&["b"]);
        assert!(r.refused[0].1.contains("waits on #a"), "{r:?}");
        let opts = RunOptions {
            skip_prereqs: true,
            ..Default::default()
        };
        let r = f.service.run(&f.doc, &f.approve(&["b"]), &opts);
        assert!(r.refused[0].1.contains("confirmation"), "{r:?}");
        let opts = RunOptions {
            skip_prereqs: true,
            confirmed: true,
            ..Default::default()
        };
        let r = f.service.run(&f.doc, &f.approve(&["b"]), &opts);
        assert_eq!(r.started.len(), 1, "{r:?}");
        f.wait();
        assert!(f.attempt("b").started.forced);
        assert!(f.text().contains("forced"));
    }

    #[test]
    fn timeout_and_stop_end_the_process_group() {
        let text = "```sh {#slow timeout=1s needs=}\nsleep 30 & sleep 30\n```\n```sh {#stop needs=}\necho start; sleep 30\n```\n";
        let limits = Limits {
            grace: Duration::from_millis(300),
            ..Limits::default()
        };
        let f = fixture("timeout", text, limits, vec![]);
        let t0 = Instant::now();
        f.run(&["slow", "stop"]);
        std::thread::sleep(Duration::from_millis(300));
        f.service.stop(&f.doc, "stop").unwrap();
        f.wait();
        assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
        assert_eq!(f.state("slow"), StepState::TimedOut);
        assert_eq!(f.state("stop"), StepState::Cancelled);
        assert!(f.text().contains("outcome=timed-out"));
        assert!(f.text().contains("outcome=cancelled"));
    }

    #[test]
    fn parallel_cells_each_get_a_log_and_tail_frames_carry_bounded_lines() {
        let text = "```sh {#one needs=}\nfor i in $(seq 1 200); do echo one-$i; done\n```\n```sh {#two needs=}\nfor i in $(seq 1 50); do echo two-$i; done\n```\n";
        let f = fixture("parallel", text, Limits::default(), vec![]);
        let r = f.run(&["one", "two"]);
        assert_eq!(r.started.len(), 2);
        f.wait();
        assert_eq!(f.state("one"), StepState::Done);
        assert_eq!(f.state("two"), StepState::Done);
        assert!(f.attempt("one").finished.unwrap().lines == 200);
        let events = lock(&f.rec.events);
        let tails: Vec<&Tail> = events
            .iter()
            .filter_map(|e| match e {
                Event::Tail(t) => Some(t),
                _ => None,
            })
            .collect();
        assert!(tails.iter().all(|t| t.lines.len() <= 15));
        let last_one = tails.iter().rfind(|t| t.cell == "one").unwrap();
        assert_eq!(last_one.lines.last().unwrap(), "one-200");
        drop(events);
        let doc = f.text();
        assert!(doc.contains("cell=one") && doc.contains("cell=two"));
        assert!(doc.contains("… 160 lines"), "{doc}");
    }

    #[test]
    fn the_log_cap_truncates_with_a_marker_and_completion_is_kept() {
        let text = "```sh {#big}\nyes 0123456789 | head -c 200000; echo; exit 0\n```\n";
        let limits = Limits {
            log_cap: 10_000,
            ..Limits::default()
        };
        let f = fixture("cap", text, limits, vec![]);
        f.run(&["big"]);
        f.wait();
        let a = f.attempt("big");
        let fin = a.finished.clone().unwrap();
        assert!(fin.truncated);
        assert_eq!(fin.outcome, Outcome::Exit { code: 0 });
        assert!(fin.bytes >= 200_000);
        let log = std::fs::read_to_string(f.doc.data.join(&a.started.log)).unwrap();
        assert!(log.len() < 11_000);
        assert!(log.contains("[runbook: log capped"));
        assert!(f.text().contains("truncated"));
    }

    struct Secret;
    impl Provider for Secret {
        fn id(&self) -> &str {
            "test"
        }
        fn bindings(&self, _: &DocRef, _: &str, _: &Runbook) -> Option<Bindings> {
            let mut b = Bindings::default();
            b.values.insert(
                "pw".into(),
                params::Binding {
                    value: "s3cr3t-value".into(),
                    secret: true,
                },
            );
            Some(b)
        }
    }

    #[test]
    fn a_secret_reaches_the_shell_and_never_the_log_the_doc_or_the_hash() {
        let text = "```sh {#s}\necho \"pw={{pw}}\"; printf 's3cr3'; printf 't-value\\n'\n```\n";
        let f = fixture("secret", text, Limits::default(), vec![Arc::new(Secret)]);
        f.run(&["s"]);
        f.wait();
        let a = f.attempt("s");
        assert!(a.started.command.contains("•••"));
        let log = std::fs::read_to_string(f.doc.data.join(&a.started.log)).unwrap();
        assert!(!log.contains("s3cr3t-value"), "{log}");
        assert_eq!(log, "pw=•••\n•••\n");
        assert!(!f.text().contains("s3cr3t-value"));
        let ev = std::fs::read_to_string(f.doc.data.join("evidence.jsonl")).unwrap();
        assert!(!ev.contains("s3cr3t-value"));
    }

    #[test]
    fn checks_retry_and_expect_and_manual_verdicts_and_env_carries_state() {
        let text = "```sh {#set}\necho \"TOKEN=abc\" >> \"$RUNBOOK_ENV\"\n```\n\
```sh {#set-ok check-of=set retry=3x0s expect=\"^abc\"}\necho $TOKEN\n```\n\
```check {#look check-of=set manual}\n- [ ] fine\n```\n";
        let f = fixture("checks", text, Limits::default(), vec![]);
        // A check cannot run before its step succeeded.
        assert!(!f.run(&["set-ok"]).refused.is_empty());
        f.run(&["set"]);
        f.wait();
        assert_eq!(f.state("set"), StepState::Checking);
        f.run(&["set-ok"]);
        f.wait();
        assert_eq!(f.attempt("set-ok").finished.unwrap().passed, true);
        assert_eq!(f.state("set"), StepState::Checking);
        f.service
            .verdict(&f.doc, "look", true, "all good".into())
            .unwrap();
        assert_eq!(f.state("set"), StepState::Done);
        assert!(f.text().contains("verdict=pass"));
        let (snap, _) = f.service.snapshot(&f.doc).unwrap();
        assert!(snap.env.contains("TOKEN=abc"));
    }

    #[test]
    fn unfinished_attempts_are_interrupted_on_reopen_and_requests_resolve() {
        let text = "```sh {#a}\ntrue\n```\n";
        let f = fixture("recover", text, Limits::default(), vec![]);
        std::fs::create_dir_all(&f.doc.data).unwrap();
        let started = Record::Started(Started {
            attempt: 1,
            cell: "a".into(),
            run: 1,
            prev: None,
            by: LOCAL_USER.into(),
            at: now(),
            shell: "sh".into(),
            target: None,
            command: "true".into(),
            hash: graph::commands(&Runbook::parse(text), &Bindings::default())["a"]
                .hash
                .clone(),
            of: None,
            prereqs: vec![],
            forced: false,
            request: None,
            log: "logs/a-1.log".into(),
        });
        std::fs::write(f.doc.data.join("evidence.jsonl"), Evidence::line(&started)).unwrap();
        f.service.open(&f.doc).unwrap();
        assert_eq!(f.state("a"), StepState::Interrupted);
        assert!(f.text().contains("outcome=interrupted"));

        let id = f
            .service
            .request(
                &f.doc,
                PendingKind::Run,
                vec!["a".into()],
                "retry it".into(),
                "agent".into(),
            )
            .unwrap();
        let (snap, _) = f.service.snapshot(&f.doc).unwrap();
        assert_eq!(snap.pending.len(), 1);
        let opts = RunOptions {
            request: Some(id),
            ..Default::default()
        };
        f.service.run(&f.doc, &f.approve(&["a"]), &opts);
        f.wait();
        let (snap, _) = f.service.snapshot(&f.doc).unwrap();
        assert!(snap.pending.is_empty());
        assert_eq!(f.state("a"), StepState::Done);
        let (changes, cursor) = f.service.changes(&f.doc, 0).unwrap();
        assert!(changes.iter().any(|c| c.kind == "finished"));
        assert!(f.service.changes(&f.doc, cursor).unwrap().0.is_empty());
        // read_output pages the log and refuses the wrong cell.
        let a = f.attempt("a");
        assert!(
            f.service
                .read_output(&f.doc, a.id(), "zz", &OutputQuery::All)
                .is_err()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_command_is_never_on_the_argv_and_its_script_file_is_removed() {
        let text = "```sh {#argv}\ntr '\\0' ' ' < /proc/$$/cmdline; echo\n```\n";
        let f = fixture("argv", text, Limits::default(), vec![]);
        f.run(&["argv"]);
        f.wait();
        let a = f.attempt("argv");
        let log = std::fs::read_to_string(f.doc.data.join(&a.started.log)).unwrap();
        assert!(log.contains("argv-1.sh"), "{log}");
        assert!(
            !log.contains("cmdline"),
            "the script text reached argv: {log}"
        );
        let tmp = f.doc.data.join("tmp");
        assert!(std::fs::read_dir(&tmp).map(|d| d.count()).unwrap_or(0) == 0);
    }

    #[test]
    fn readonly_auto_runs_an_accepted_cell_on_request_and_every_cell_only_files_it() {
        let text = "---\ngate: readonly-auto\n---\n```sh {#up readonly}\necho up\n```\n";
        let f = fixture("auto", text, Limits::default(), vec![]);
        f.service
            .request(
                &f.doc,
                PendingKind::Run,
                vec!["up".into()],
                "look".into(),
                "agent".into(),
            )
            .unwrap();
        let (snap, _) = f.service.snapshot(&f.doc).unwrap();
        assert_eq!(
            snap.pending.len(),
            1,
            "not accepted yet: it waits for the human"
        );
        // An acceptance binds the hash the person saw.
        assert!(
            f.service
                .accept_readonly(&f.doc, "up", "reviewed".into(), "000000000000")
                .unwrap_err()
                .contains("changed since")
        );
        let hash = snap.commands["up"].hash.clone();
        f.service
            .accept_readonly(&f.doc, "up", "reviewed".into(), &hash)
            .unwrap();
        f.service
            .request(
                &f.doc,
                PendingKind::Run,
                vec!["up".into()],
                "look".into(),
                "agent".into(),
            )
            .unwrap();
        f.wait();
        assert_eq!(f.state("up"), StepState::Done);
        assert_eq!(f.attempt("up").started.by, "readonly-auto for agent");
    }

    #[test]
    fn a_missing_interpreter_is_a_lint_and_the_env_file_tolerates_crlf_and_bom() {
        let text = "```sh {#a shell=no-such-shell-xyz}\ntrue\n```\n";
        let f = fixture("noshell", text, Limits::default(), vec![]);
        let (snap, _) = f.service.snapshot(&f.doc).unwrap();
        assert!(snap.lints.lints.iter().any(|l| l.code == "shell"));
        assert_eq!(snap.states.step("a").unwrap().state, StepState::Invalid);
        let env = f.dir.join("env");
        std::fs::write(&env, "\u{feff}A=1\r\nexport B=\"two\"\r\n# c\r\n").unwrap();
        assert_eq!(
            read_env(&env),
            [
                ("A".to_string(), "1".to_string()),
                ("B".to_string(), "two".to_string())
            ]
        );
    }

    #[test]
    fn write_is_revision_checked_and_outputs_stay_host_owned() {
        let text = "```sh {#a}\necho one\n```\n";
        let f = fixture("write", text, Limits::default(), vec![]);
        f.run(&["a"]);
        f.wait();
        let (snap, current) = f.service.snapshot(&f.doc).unwrap();
        let conflict = f.service.write(&f.doc, "x", "nope").unwrap();
        assert!(matches!(conflict, WriteOutcome::Conflict { .. }));
        let edited = format!(
            "# Added by the agent\n\n{}",
            current.replace("1 | one", "1 | forged")
        );
        let WriteOutcome::Written { lints, .. } =
            f.service.write(&f.doc, &edited, &snap.rev).unwrap()
        else {
            panic!()
        };
        assert!(lints.lints.is_empty());
        let now = f.text();
        assert!(now.starts_with("# Added by the agent"));
        assert!(now.contains("1 | one") && !now.contains("forged"));
        let a = f.attempt("a");
        let out = f
            .service
            .read_output(
                &f.doc,
                a.id(),
                "a",
                &OutputQuery::Grep {
                    pattern: "one".into(),
                    context: 0,
                },
            )
            .unwrap();
        assert_eq!(out.text, "1 | one\n");
    }
}
