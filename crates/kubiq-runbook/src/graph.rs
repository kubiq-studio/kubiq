//! The step graph and the states derived from it (proposal §3.2). Nothing here is stored: a state is
//! a projection of the current text, the bindings and the evidence, recomputed on every read.
//!
//! - A step needs the steps `needs=` names, or the previous step when it names none.
//! - A step is **done** when its latest attempt exited 0, every check of that attempt passed, and the
//!   attempt is still fresh: the command it ran hashes the same as the current one, and every
//!   prerequisite's proof is the one it started on.
//! - A check is eligible once its step's command succeeded, and judges that attempt only.
//! - A skip or an override counts as done while the step's command is unchanged.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::evidence::{Attempt, DecisionKind, Evidence, Outcome, Proof};
use crate::lint::{Lints, resolved_needs};
use crate::model::{CellKind, Runbook};
use crate::params::{self, Bindings, Form};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepState {
    /// A prerequisite is not done.
    Waiting,
    /// Every prerequisite is done.
    Ready,
    /// The step or one of its checks is running.
    Running,
    /// The step exited 0 and its checks are pending.
    Checking,
    Done,
    Failed,
    Skipped,
    Overridden,
    Cancelled,
    TimedOut,
    Interrupted,
    /// Earlier evidence no longer proves the current definition or prerequisites.
    Stale,
    /// A lint error touches it: it runs nothing until fixed.
    Invalid,
}

impl StepState {
    /// Whether a step that needs this one may proceed.
    pub fn satisfies(self) -> bool {
        matches!(
            self,
            StepState::Done | StepState::Skipped | StepState::Overridden
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            StepState::Waiting => "waiting",
            StepState::Ready => "ready",
            StepState::Running => "running",
            StepState::Checking => "checking",
            StepState::Done => "done",
            StepState::Failed => "failed",
            StepState::Skipped => "skipped",
            StepState::Overridden => "overridden",
            StepState::Cancelled => "cancelled",
            StepState::TimedOut => "timed-out",
            StepState::Interrupted => "interrupted",
            StepState::Stale => "stale",
            StepState::Invalid => "invalid",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckState {
    /// Its step has not succeeded yet.
    Waiting,
    /// Eligible, not run (automatic) or not judged (manual).
    Pending,
    Running,
    Passed,
    Failed,
}

/// One cell's command as it would run now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    pub cell: String,
    pub shell: String,
    /// After substitution, secrets as `•••`.
    pub display: String,
    /// What an approval binds ([`params::command_hash`]); for a manual check, the checklist's hash.
    pub hash: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unbound: Vec<String>,
    /// Where it runs, when not here (`ssh:<alias>[@windows]`): part of what an approval binds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// Every cell's current command, on this machine, assuming every interpreter it names exists.
pub fn commands(book: &Runbook, bindings: &Bindings) -> BTreeMap<String, Command> {
    commands_on(book, bindings, crate::shell::Os::current(), &|_| true)
}

/// Every cell's current command on a target of `os`, where `exists` says which interpreters are
/// installed ([`crate::shell::interpreter`]).
pub fn commands_on(
    book: &Runbook,
    bindings: &Bindings,
    os: crate::shell::Os,
    exists: &dyn Fn(&str) -> bool,
) -> BTreeMap<String, Command> {
    book.cells
        .iter()
        .map(|cell| {
            // An ssh target's interpreter is chosen for its OS, and its existence is not this
            // host's to answer.
            let target = crate::shell::Target::parse(
                cell.target.as_deref().or(book.front.target.as_deref()),
            );
            let shell = match &target {
                crate::shell::Target::Ssh { os: far, .. } => {
                    crate::shell::interpreter(&cell.lang, cell.shell.as_deref(), *far, &|p| {
                        p == "pwsh"
                    })
                }
                _ => crate::shell::interpreter(&cell.lang, cell.shell.as_deref(), os, exists),
            };
            let command = if cell.kind == CellKind::Manual {
                Command {
                    cell: cell.id.clone(),
                    shell,
                    display: cell.body.clone(),
                    hash: params::short_hash(cell.body.as_bytes()),
                    unbound: vec![],
                    target: None,
                }
            } else {
                let (display, unbound) = params::subst(&cell.body, bindings, Form::Display);
                let target = match &target {
                    crate::shell::Target::Local => None,
                    _ => cell.target.clone().or_else(|| book.front.target.clone()),
                };
                let bound_to = match &target {
                    Some(t) => format!("{t}|{shell}"),
                    None => shell.clone(),
                };
                Command {
                    cell: cell.id.clone(),
                    hash: params::command_hash(
                        &bound_to,
                        &crate::shell::normalise(
                            &params::subst(&cell.body, bindings, Form::Hash).0,
                        ),
                    ),
                    shell,
                    display,
                    unbound,
                    target,
                }
            };
            (cell.id.clone(), command)
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckView {
    pub cell: String,
    pub manual: bool,
    pub state: CheckState,
}

/// One step as the viewer and the agent see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepView {
    pub cell: String,
    pub state: StepState,
    pub needs: Vec<String>,
    /// The needs that are not done, when it is waiting.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waits_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<CheckView>,
    /// The latest attempt of the step itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<u64>,
    pub runs: u32,
    /// The proof a dependent records: `a<attempt>`, `s<skip>`, `o<override>`; empty when the step
    /// does not satisfy.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proof: String,
}

/// The whole graph's states.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct States {
    pub steps: Vec<StepView>,
}

impl States {
    pub fn step(&self, id: &str) -> Option<&StepView> {
        self.steps.iter().find(|s| s.cell == id)
    }

    /// What can run now: ready steps, stale steps whose needs are done, and pending automatic
    /// checks of succeeded steps.
    pub fn ready(&self) -> Vec<String> {
        let mut out = Vec::new();
        for step in &self.steps {
            match step.state {
                StepState::Ready => out.push(step.cell.clone()),
                StepState::Stale if step.waits_on.is_empty() => out.push(step.cell.clone()),
                StepState::Checking => out.extend(
                    step.checks
                        .iter()
                        .filter(|c| !c.manual && c.state == CheckState::Pending)
                        .map(|c| c.cell.clone()),
                ),
                _ => {}
            }
        }
        out
    }

    /// The proofs of `cell`'s needs, as an attempt of it records them at start.
    pub fn proofs_for(&self, needs: &[String]) -> Vec<Proof> {
        needs
            .iter()
            .map(|n| Proof {
                cell: n.clone(),
                token: self.step(n).map(|s| s.proof.clone()).unwrap_or_default(),
            })
            .collect()
    }
}

fn outcome_state(outcome: &Outcome) -> StepState {
    match outcome {
        Outcome::Exit { code: 0 } => StepState::Done,
        Outcome::Exit { .. } | Outcome::Error { .. } => StepState::Failed,
        Outcome::Cancelled => StepState::Cancelled,
        Outcome::TimedOut => StepState::TimedOut,
        Outcome::Interrupted => StepState::Interrupted,
    }
}

/// Derive every step's state.
pub fn states(
    book: &Runbook,
    commands: &BTreeMap<String, Command>,
    evidence: &Evidence,
    lints: &Lints,
) -> States {
    let needs = resolved_needs(book);
    let attempts = evidence.attempts();
    let by_cell = |cell: &str| -> Vec<&Attempt> {
        attempts.iter().filter(|a| a.started.cell == cell).collect()
    };
    let hash = |cell: &str| commands.get(cell).map(|c| c.hash.as_str()).unwrap_or("");
    let mut done: HashMap<String, StepView> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    // Topological by repeated passes: document order, each step once its needs are computed (or
    // unknown, or blocked by a cycle).
    let steps: Vec<_> = book.steps().collect();
    let mut remaining: Vec<_> = steps.clone();
    while !remaining.is_empty() {
        let before = remaining.len();
        let mut next = Vec::new();
        for step in remaining {
            let own = needs.get(&step.id).cloned().unwrap_or_default();
            let computable = lints.blocked.contains(&step.id)
                || own
                    .iter()
                    .all(|n| done.contains_key(n) || book.cell(n).is_none_or(|c| !c.is_step()));
            if !computable {
                next.push(step);
                continue;
            }
            let view = step_view(&step.id, own, book, &by_cell, &hash, evidence, lints, &done);
            order.push(step.id.clone());
            done.insert(step.id.clone(), view);
        }
        if next.len() == before {
            // Only cycle members left, which lints already block.
            for step in next {
                let own = needs.get(&step.id).cloned().unwrap_or_default();
                done.insert(
                    step.id.clone(),
                    StepView {
                        cell: step.id.clone(),
                        state: StepState::Invalid,
                        needs: own,
                        waits_on: vec![],
                        checks: vec![],
                        latest: None,
                        runs: 0,
                        proof: String::new(),
                    },
                );
                order.push(step.id.clone());
            }
            break;
        }
        remaining = next;
    }
    let steps = steps.iter().filter_map(|s| done.remove(&s.id)).collect();
    States { steps }
}

#[allow(clippy::too_many_arguments)]
fn step_view<'a>(
    id: &str,
    needs: Vec<String>,
    book: &Runbook,
    by_cell: &dyn Fn(&str) -> Vec<&'a Attempt>,
    hash: &dyn Fn(&str) -> &'a str,
    evidence: &Evidence,
    lints: &Lints,
    done: &HashMap<String, StepView>,
) -> StepView {
    let current = hash(id).to_string();
    let own = by_cell(id);
    let runs = own.len() as u32;
    let latest = own.last().map(|a| a.id());
    let waits_on: Vec<String> = needs
        .iter()
        .filter(|n| !done.get(*n).is_some_and(|v| v.state.satisfies()))
        .cloned()
        .collect();
    let proofs: Vec<Proof> = needs
        .iter()
        .map(|n| Proof {
            cell: n.clone(),
            token: done.get(n).map(|v| v.proof.clone()).unwrap_or_default(),
        })
        .collect();
    let checks_of: Vec<_> = book.checks_of(id).collect();
    let mut view = StepView {
        cell: id.to_string(),
        state: StepState::Ready,
        needs: needs.clone(),
        waits_on: vec![],
        checks: checks_of
            .iter()
            .map(|c| CheckView {
                cell: c.id.clone(),
                manual: c.kind == CellKind::Manual,
                state: CheckState::Waiting,
            })
            .collect(),
        latest,
        runs,
        proof: String::new(),
    };
    if lints.blocked.contains(id) {
        view.state = StepState::Invalid;
        return view;
    }

    let decision = evidence
        .decisions(id)
        .into_iter()
        .filter(|d| d.kind != DecisionKind::AcceptReadonly)
        .rfind(|d| d.hash == current)
        .cloned();
    let check_running = checks_of
        .iter()
        .any(|c| by_cell(&c.id).iter().any(|a| a.finished.is_none()));
    if own.last().is_some_and(|a| a.finished.is_none()) || check_running {
        view.state = StepState::Running;
        if own.last().is_some_and(|a| a.finished.is_none()) {
            return view;
        }
    }
    // A fresh skip wins over everything recorded before it.
    if let Some(d) = &decision
        && d.kind == DecisionKind::Skip
        && latest.is_none_or(|a| d.id > a)
    {
        view.state = StepState::Skipped;
        view.proof = format!("s{}", d.id);
        return view;
    }
    let overridden = |after: Option<u64>| {
        decision
            .as_ref()
            .filter(|d| d.kind == DecisionKind::Override && after.is_none_or(|a| d.id > a))
    };

    let finished = own.iter().rev().find(|a| a.finished.is_some());
    let Some(attempt) = finished else {
        if let Some(d) = overridden(None) {
            view.state = StepState::Overridden;
            view.proof = format!("o{}", d.id);
        } else if !waits_on.is_empty() {
            view.state = StepState::Waiting;
            view.waits_on = waits_on;
        } else {
            view.state = StepState::Ready;
        }
        return view;
    };
    let fresh = attempt.started.hash == current && same_proofs(&attempt.started.prereqs, &proofs);
    let outcome = &attempt.finished.as_ref().expect("finished").outcome;
    let mut state = if !fresh {
        StepState::Stale
    } else {
        outcome_state(outcome)
    };
    if state == StepState::Done {
        // The checks judge this attempt only.
        let mut any_failed = false;
        let mut any_pending = false;
        for (check, cv) in checks_of.iter().zip(view.checks.iter_mut()) {
            let chash = hash(&check.id);
            cv.state = if check.kind == CellKind::Manual {
                match evidence
                    .verdicts(&check.id)
                    .into_iter()
                    .rfind(|v| v.of == Some(attempt.id()) && v.hash == chash)
                {
                    Some(v) if v.pass => CheckState::Passed,
                    Some(_) => CheckState::Failed,
                    None => CheckState::Pending,
                }
            } else {
                let mine: Vec<_> = by_cell(&check.id)
                    .into_iter()
                    .filter(|a| a.started.of == Some(attempt.id()) && a.started.hash == chash)
                    .collect();
                match mine.last() {
                    Some(a) if a.finished.is_none() => CheckState::Running,
                    Some(a) if a.passed() => CheckState::Passed,
                    Some(_) => CheckState::Failed,
                    None => CheckState::Pending,
                }
            };
            any_failed |= cv.state == CheckState::Failed;
            any_pending |= matches!(cv.state, CheckState::Pending | CheckState::Running);
        }
        state = if any_failed {
            StepState::Failed
        } else if any_pending {
            StepState::Checking
        } else {
            StepState::Done
        };
    }
    if state == StepState::Done {
        view.proof = format!("a{}", attempt.id());
    } else if let Some(d) = overridden(Some(attempt.id())) {
        state = StepState::Overridden;
        view.proof = format!("o{}", d.id);
    }
    if view.state == StepState::Running && state != StepState::Done {
        // A check is running: say so, but keep the checks' states.
        return view;
    }
    view.state = state;
    if state == StepState::Stale {
        view.waits_on = waits_on;
    }
    view
}

fn same_proofs(recorded: &[Proof], now: &[Proof]) -> bool {
    let mut a: Vec<_> = recorded.iter().map(|p| (&p.cell, &p.token)).collect();
    let mut b: Vec<_> = now.iter().map(|p| (&p.cell, &p.token)).collect();
    a.sort();
    b.sort();
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::{Decision, Finished, LOCAL_USER, Record, Started, Verdict};
    use crate::lint::lint;

    /// A small in-memory runner over the pure functions: start and finish attempts the way the
    /// worker does, so the tests read as scenarios.
    struct Sim {
        text: String,
        ev: Evidence,
    }

    impl Sim {
        fn new(text: &str) -> Sim {
            Sim {
                text: text.into(),
                ev: Evidence::default(),
            }
        }
        fn book(&self) -> Runbook {
            Runbook::parse(&self.text)
        }
        fn states(&self) -> States {
            let book = self.book();
            let cmds = commands(&book, &Bindings::default());
            states(&book, &cmds, &self.ev, &lint(&book, None))
        }
        fn state(&self, id: &str) -> StepState {
            self.states().step(id).unwrap().state
        }
        fn start(&mut self, cell: &str) -> u64 {
            let book = self.book();
            let cmds = commands(&book, &Bindings::default());
            let st = self.states();
            let c = book.cell(cell).unwrap();
            let (of, prereqs) = match &c.check_of {
                Some(step) if !c.is_step() => (st.step(step).and_then(|s| s.latest), vec![]),
                _ => (None, st.proofs_for(&st.step(cell).unwrap().needs)),
            };
            let id = self.ev.next_id();
            self.ev.records.push(Record::Started(Started {
                attempt: id,
                cell: cell.into(),
                run: 1,
                prev: None,
                by: LOCAL_USER.into(),
                at: "t".into(),
                shell: "sh".into(),
                target: None,
                command: cmds[cell].display.clone(),
                hash: cmds[cell].hash.clone(),
                of,
                prereqs,
                forced: false,
                request: None,
                log: String::new(),
            }));
            id
        }
        fn finish(&mut self, id: u64, outcome: Outcome) {
            let passed = outcome == Outcome::Exit { code: 0 };
            self.ev.records.push(Record::Finished(Finished {
                attempt: id,
                at: "t".into(),
                outcome,
                passed,
                took_ms: 1,
                lines: 0,
                bytes: 0,
                truncated: false,
                excerpt: String::new(),
            }));
        }
        fn run(&mut self, cell: &str, code: i32) {
            let id = self.start(cell);
            self.finish(id, Outcome::Exit { code });
        }
        fn verdict(&mut self, cell: &str, pass: bool) {
            let book = self.book();
            let step = book.cell(cell).unwrap().check_of.clone().unwrap();
            let of = self.states().step(&step).unwrap().latest;
            let hash = commands(&book, &Bindings::default())[cell].hash.clone();
            let id = self.ev.next_id();
            self.ev.records.push(Record::Verdict(Verdict {
                id,
                cell: cell.into(),
                pass,
                note: String::new(),
                by: LOCAL_USER.into(),
                at: "t".into(),
                of,
                hash,
            }));
        }
        fn decide(&mut self, cell: &str, kind: DecisionKind) {
            let book = self.book();
            let hash = commands(&book, &Bindings::default())[cell].hash.clone();
            let id = self.ev.next_id();
            self.ev.records.push(Record::Decision(Decision {
                id,
                cell: cell.into(),
                kind,
                reason: "because".into(),
                by: LOCAL_USER.into(),
                at: "t".into(),
                hash,
            }));
        }
    }

    const LINEAR: &str = "```sh {#a}\ntrue\n```\n```sh {#b}\ntrue\n```\n";

    #[test]
    fn a_linear_runbook_waits_then_readies_then_is_done() {
        let mut s = Sim::new(LINEAR);
        assert_eq!(s.state("a"), StepState::Ready);
        assert_eq!(s.state("b"), StepState::Waiting);
        assert_eq!(s.states().step("b").unwrap().waits_on, ["a"]);
        let id = s.start("a");
        assert_eq!(s.state("a"), StepState::Running);
        s.finish(id, Outcome::Exit { code: 0 });
        assert_eq!(s.state("a"), StepState::Done);
        assert_eq!(s.states().ready(), ["b"]);
    }

    #[test]
    fn failure_cancel_timeout_and_interrupt_are_distinct_and_block() {
        for (outcome, want) in [
            (Outcome::Exit { code: 2 }, StepState::Failed),
            (Outcome::Cancelled, StepState::Cancelled),
            (Outcome::TimedOut, StepState::TimedOut),
            (Outcome::Interrupted, StepState::Interrupted),
        ] {
            let mut s = Sim::new(LINEAR);
            let id = s.start("a");
            s.finish(id, outcome);
            assert_eq!(s.state("a"), want);
            assert_eq!(s.state("b"), StepState::Waiting);
        }
    }

    /// The S1 done-when scenario: two parallel branches and a join, each step with a check; a
    /// failing check blocks the join; a fix step added before it lets it through.
    #[test]
    fn parallel_branches_join_and_a_failing_check_blocks_the_join() {
        let text = "\
```sh {#prep needs=}\ntrue\n```\n\
```sh {#prep-ok check-of=prep}\ntrue\n```\n\
```sh {#left needs=prep}\ntrue\n```\n\
```sh {#left-ok check-of=left}\ntrue\n```\n\
```sh {#right needs=prep}\ntrue\n```\n\
```check {#right-ok check-of=right manual}\n- [ ] looks right\n```\n\
```sh {#join needs=left,right}\ntrue\n```\n\
```sh {#join-ok check-of=join}\ntrue\n```\n";
        let mut s = Sim::new(text);
        assert_eq!(s.states().ready(), ["prep"]);
        s.run("prep", 0);
        assert_eq!(s.state("prep"), StepState::Checking);
        assert_eq!(s.state("left"), StepState::Waiting);
        assert_eq!(s.states().ready(), ["prep-ok"]);
        s.run("prep-ok", 0);
        assert_eq!(s.state("prep"), StepState::Done);
        // Two parallel paths.
        assert_eq!(s.states().ready(), ["left", "right"]);
        s.run("left", 0);
        s.run("right", 0);
        s.run("left-ok", 1);
        assert_eq!(s.state("left"), StepState::Failed);
        s.verdict("right-ok", true);
        assert_eq!(s.state("right"), StepState::Done);
        let join = s.states().step("join").unwrap().clone();
        assert_eq!(join.state, StepState::Waiting);
        assert_eq!(join.waits_on, ["left"]);

        // The agent adds a fix step before `left`, which now needs it: `left` is stale (its proofs
        // changed) and waits on the fix.
        s.text = text.replace(
            "```sh {#left needs=prep}",
            "```sh {#fix needs=prep}\necho fix\n```\n```sh {#left needs=fix}",
        );
        assert_eq!(s.state("fix"), StepState::Ready);
        assert_eq!(s.state("left"), StepState::Stale);
        s.run("fix", 0);
        assert_eq!(s.states().ready(), ["left"]);
        s.run("left", 0);
        s.run("left-ok", 0);
        assert_eq!(s.state("left"), StepState::Done);
        // `right` and its manual verdict are untouched by the fix.
        assert_eq!(s.state("right"), StepState::Done);
        assert_eq!(s.state("join"), StepState::Ready);
        s.run("join", 0);
        s.run("join-ok", 0);
        assert_eq!(s.state("join"), StepState::Done);
    }

    #[test]
    fn editing_or_rerunning_a_prerequisite_makes_dependents_and_verdicts_stale() {
        let text = "```sh {#a}\ntrue\n```\n```sh {#b}\ntrue\n```\n```check {#b-ok check-of=b manual}\n- [ ] ok\n```\n";
        let mut s = Sim::new(text);
        s.run("a", 0);
        s.run("b", 0);
        s.verdict("b-ok", true);
        assert_eq!(s.state("b"), StepState::Done);
        // Rerun the prerequisite: b's evidence no longer proves anything.
        s.run("a", 0);
        assert_eq!(s.state("b"), StepState::Stale);
        s.run("b", 0);
        // The old verdict judged the old attempt.
        assert_eq!(s.state("b"), StepState::Checking);
        s.verdict("b-ok", true);
        assert_eq!(s.state("b"), StepState::Done);
        // Edit the prerequisite's text: a is stale, and b with it.
        s.text = s
            .text
            .replace("```sh {#a}\ntrue", "```sh {#a}\ntrue # edited");
        assert_eq!(s.state("a"), StepState::Stale);
        assert_eq!(s.state("b"), StepState::Stale);
        // Editing the checklist retires its verdict.
        let mut s2 = Sim::new(text);
        s2.run("a", 0);
        s2.run("b", 0);
        s2.verdict("b-ok", true);
        s2.text = s2.text.replace("- [ ] ok", "- [ ] ok, for two minutes");
        assert_eq!(s2.state("b"), StepState::Checking);
    }

    #[test]
    fn override_and_skip_satisfy_and_an_edit_retires_them() {
        let mut s = Sim::new(LINEAR);
        s.run("a", 1);
        assert_eq!(s.state("a"), StepState::Failed);
        s.decide("a", DecisionKind::Override);
        assert_eq!(s.state("a"), StepState::Overridden);
        assert_eq!(s.state("b"), StepState::Ready);
        // A retry after the override is judged on its own.
        s.run("a", 1);
        assert_eq!(s.state("a"), StepState::Failed);

        let mut s = Sim::new(LINEAR);
        s.decide("a", DecisionKind::Skip);
        assert_eq!(s.state("a"), StepState::Skipped);
        assert_eq!(s.state("b"), StepState::Ready);
        s.text = s.text.replacen("true", "false", 1);
        assert_eq!(s.state("a"), StepState::Ready);
    }

    #[test]
    fn a_lint_error_makes_a_step_invalid_and_its_dependents_wait() {
        let s = Sim::new("```sh {#a timeout=soon}\ntrue\n```\n```sh {#b}\ntrue\n```\n");
        assert_eq!(s.state("a"), StepState::Invalid);
        assert_eq!(s.state("b"), StepState::Waiting);
    }
}
