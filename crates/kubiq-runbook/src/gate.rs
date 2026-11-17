//! The run gate for an agent's request (proposal §6), as the shared [`kubiq_cells::RunGate`].
//!
//! Under `gate: every-cell` (the default) every request waits for a person: the answer is always
//! `Ask`. Under `gate: readonly-auto` a request runs at once only when **every** cell it names is
//! marked `readonly`, a human accepted that mark for the command as it reads now, the cell is ready,
//! it asks for no `confirm=`, no lint touches it, and no bound parameter says production. Anything
//! else asks. The human's own Run is not a request and does not come through here.

use std::collections::BTreeMap;

use kubiq_cells::{GateDecision, GateRequest, RunGate};

use crate::evidence::{DecisionKind, Evidence};
use crate::graph::{Command, States};
use crate::lint::Lints;
use crate::model::{CellKind, Runbook};
use crate::params::Bindings;

pub const EVERY_CELL: &str = "every-cell";
pub const READONLY_AUTO: &str = "readonly-auto";

/// The words that mark production, as a token anywhere in a value.
const PRODUCTION: &[&str] = &["prod", "prd", "production", "live"];

/// Whether a run may touch production, conservatively: any bound value, or any target, holding
/// `prod`, `prd`, `production` or `live` as a token (split on anything not a letter or digit, any
/// case) — `db.prod.internal`, `PRD-eu`, `live`. A secret's value is not looked at; its name is.
/// `readonly-auto` never applies when this says yes, so a false yes only costs a click.
pub fn is_production(bindings: &Bindings, targets: &[&str]) -> bool {
    let marked = |text: &str| {
        text.split(|c: char| !c.is_ascii_alphanumeric())
            .any(|t| PRODUCTION.contains(&t.to_ascii_lowercase().as_str()))
    };
    bindings
        .values
        .iter()
        .any(|(name, b)| marked(name) || (!b.secret && marked(&b.value)))
        || targets.iter().any(|t| marked(t))
}

/// Whether a human accepted `cell`'s readonly mark for its current command.
pub fn accepted(evidence: &Evidence, cell: &str, hash: &str) -> bool {
    evidence
        .decisions(cell)
        .iter()
        .any(|d| d.kind == DecisionKind::AcceptReadonly && d.hash == hash)
}

pub struct Gate<'a> {
    pub book: &'a Runbook,
    pub states: &'a States,
    pub evidence: &'a Evidence,
    pub commands: &'a BTreeMap<String, Command>,
    pub bindings: &'a Bindings,
    pub lints: &'a Lints,
}

impl RunGate for Gate<'_> {
    fn decide(&self, request: &GateRequest) -> GateDecision {
        let mode = self.book.front.gate.as_deref().unwrap_or(EVERY_CELL);
        if mode != READONLY_AUTO {
            return GateDecision::Ask {
                reason: format!(
                    "{} asks to run it; every cell is a human decision",
                    request.agent
                ),
            };
        }
        let mut targets: Vec<&str> = self.book.front.target.as_deref().into_iter().collect();
        targets.extend(
            request
                .cells
                .iter()
                .filter_map(|id| self.book.cell(id).and_then(|c| c.target.as_deref())),
        );
        if is_production(self.bindings, &targets) {
            return GateDecision::Ask {
                reason: "readonly-auto never applies to a production run".into(),
            };
        }
        let ready = self.states.ready();
        for id in &request.cells {
            let Some(cell) = self.book.cell(id) else {
                return GateDecision::Refuse {
                    reason: format!("no cell #{id}"),
                };
            };
            let hash = self.commands.get(id).map(|c| c.hash.as_str()).unwrap_or("");
            let why = if cell.kind == CellKind::Manual {
                Some("a manual check is a human verdict")
            } else if !cell.readonly {
                Some("it is not marked readonly")
            } else if !accepted(self.evidence, id, hash) {
                Some("its readonly mark is not accepted for this text")
            } else if cell.confirm.is_some() {
                Some("it asks for confirmation")
            } else if self.lints.blocked.contains(id) {
                Some("a lint error touches it")
            } else if !ready.contains(id) {
                Some("it is not ready")
            } else {
                None
            };
            if let Some(why) = why {
                return GateDecision::Ask {
                    reason: format!("#{id}: {why}"),
                };
            }
        }
        GateDecision::Run
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::{Decision, LOCAL_USER, Record};
    use crate::graph::{commands, states};
    use crate::lint::lint;
    use crate::params::Binding;

    fn decide(text: &str, accept: bool, bindings: Bindings) -> GateDecision {
        let book = Runbook::parse(text);
        let cmds = commands(&book, &bindings);
        let mut ev = Evidence::default();
        if accept {
            ev.records.push(Record::Decision(Decision {
                id: 1,
                cell: "a".into(),
                kind: DecisionKind::AcceptReadonly,
                reason: "looked at it".into(),
                by: LOCAL_USER.into(),
                at: "t".into(),
                hash: cmds["a"].hash.clone(),
            }));
        }
        let lints = lint(&book, Some(&bindings));
        let st = states(&book, &cmds, &ev, &lints);
        Gate {
            book: &book,
            states: &st,
            evidence: &ev,
            commands: &cmds,
            bindings: &bindings,
            lints: &lints,
        }
        .decide(&GateRequest {
            agent: "agent".into(),
            cells: vec!["a".into()],
        })
    }

    const AUTO: &str = "---\ngate: readonly-auto\n---\n```sh {#a readonly}\nuptime\n```\n";

    #[test]
    fn production_is_any_marked_token_in_a_value_name_or_target() {
        let with = |k: &str, v: &str| {
            let mut b = Bindings::default();
            b.values.insert(
                k.into(),
                Binding {
                    value: v.into(),
                    secret: false,
                },
            );
            b
        };
        assert!(is_production(&with("host", "db.prod.internal"), &[]));
        assert!(is_production(&with("region", "PRD-eu"), &[]));
        assert!(is_production(&with("env", "Live"), &[]));
        assert!(is_production(&with("prod_host", "x"), &[]));
        assert!(is_production(&Bindings::default(), &["ssh:prod-db"]));
        assert!(!is_production(&with("host", "productivity.local"), &[]));
        assert!(!is_production(&with("env", "staging"), &["local"]));
    }

    #[test]
    fn every_cell_always_asks() {
        let text = AUTO.replace("readonly-auto", "every-cell");
        assert!(matches!(
            decide(&text, true, Bindings::default()),
            GateDecision::Ask { .. }
        ));
    }

    #[test]
    fn readonly_auto_runs_only_an_accepted_readonly_ready_cell_and_never_on_prod() {
        assert_eq!(decide(AUTO, true, Bindings::default()), GateDecision::Run);
        assert!(matches!(
            decide(AUTO, false, Bindings::default()),
            GateDecision::Ask { .. }
        ));
        let mut prod = Bindings::default();
        prod.values.insert(
            "env".into(),
            Binding {
                value: "prod".into(),
                secret: false,
            },
        );
        assert!(matches!(decide(AUTO, true, prod), GateDecision::Ask { .. }));
        // An edit retires the acceptance (the hash moved).
        let book = Runbook::parse(AUTO);
        let _ = book;
        let edited = AUTO.replace("uptime", "uptime; rm -rf /tmp/x");
        assert!(matches!(
            decide(&edited, false, Bindings::default()),
            GateDecision::Ask { .. }
        ));
    }
}
