//! Validation, run on every read and every write, by a human or an agent. A runbook with errors
//! still reads and edits; it only runs no step an error touches ([`Lints::blocked`]).

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::model::{CellKind, Runbook};
use crate::params::Bindings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lint {
    pub severity: Severity,
    /// `missing-id`, `duplicate-id`, `unknown-need`, `needs-check`, `cycle`, `check-of-check`,
    /// `check-of-unknown`, `check-without-step`, `attribute`, `frontmatter`, `unbound-param`,
    /// `unsupported`.
    pub code: String,
    /// The cell it is about, when it is about one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    pub line: usize,
    pub message: String,
}

/// Every lint of one document, and the cells the errors keep from running.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Lints {
    pub lints: Vec<Lint>,
    pub blocked: BTreeSet<String>,
}

impl Lints {
    pub fn errors(&self) -> usize {
        self.lints
            .iter()
            .filter(|l| l.severity == Severity::Error)
            .count()
    }
}

/// The steps each step needs, as resolved: `needs=` when written, else the previous step in
/// document order (checks ignored), else nothing.
pub fn resolved_needs(book: &Runbook) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    let mut prev: Option<&str> = None;
    for step in book.steps() {
        let needs = match &step.needs {
            Some(list) => list.clone(),
            None => prev.map(|p| vec![p.to_string()]).unwrap_or_default(),
        };
        out.entry(step.id.clone()).or_insert(needs);
        prev = Some(&step.id);
    }
    out
}

/// Lint `book`. `bindings` adds the unbound-parameter check.
pub fn lint(book: &Runbook, bindings: Option<&Bindings>) -> Lints {
    let mut out = Lints::default();
    let push = |out: &mut Lints, sev, code: &str, cell: Option<&str>, line, msg: String| {
        if sev == Severity::Error
            && let Some(cell) = cell
        {
            out.blocked.insert(cell.to_string());
        }
        out.lints.push(Lint {
            severity: sev,
            code: code.into(),
            cell: cell.map(str::to_string),
            line,
            message: msg,
        });
    };

    if let Some(error) = &book.front.error {
        push(
            &mut out,
            Severity::Error,
            "frontmatter",
            None,
            1,
            format!("frontmatter: {error}"),
        );
    }

    let mut seen: HashMap<&str, usize> = HashMap::new();
    for cell in &book.cells {
        if !cell.explicit {
            push(
                &mut out,
                Severity::Error,
                "missing-id",
                Some(&cell.id),
                cell.line,
                format!("line {}: a runnable cell needs an explicit #id", cell.line),
            );
        } else if let Some(first) = seen.insert(&cell.id, cell.line) {
            push(
                &mut out,
                Severity::Error,
                "duplicate-id",
                Some(&cell.id),
                cell.line,
                format!(
                    "#{} is already the id of the cell at line {first}; a copied cell needs a new id",
                    cell.id
                ),
            );
        }
        for problem in &cell.problems {
            push(
                &mut out,
                Severity::Error,
                "attribute",
                Some(&cell.id),
                cell.line,
                format!("#{}: {problem}", cell.id),
            );
        }
        if cell.interactive {
            push(
                &mut out,
                Severity::Error,
                "unsupported",
                Some(&cell.id),
                cell.line,
                format!("#{}: interactive cells are not supported yet", cell.id),
            );
        }
        let target = cell.target.as_deref().or(book.front.target.as_deref());
        let why = match crate::shell::Target::parse(target) {
            crate::shell::Target::Local | crate::shell::Target::Ssh { .. } => None,
            crate::shell::Target::Drone(_) => Some(
                "a drone runs the runbooks of the projects it serves; open this project from the drone",
            ),
            crate::shell::Target::Unknown(_) => Some("targets are local, ssh:<alias>[@windows]"),
        };
        if let Some(why) = why {
            push(
                &mut out,
                Severity::Error,
                "unsupported",
                Some(&cell.id),
                cell.line,
                format!("#{}: target {}: {why}", cell.id, target.unwrap_or_default()),
            );
        }
        match (&cell.kind, &cell.check_of) {
            (CellKind::Manual, None) => push(
                &mut out,
                Severity::Error,
                "check-without-step",
                Some(&cell.id),
                cell.line,
                format!("#{}: a manual check needs check-of=<step>", cell.id),
            ),
            (CellKind::Check | CellKind::Manual, Some(of)) => match book.cell(of) {
                None => push(
                    &mut out,
                    Severity::Error,
                    "check-of-unknown",
                    Some(&cell.id),
                    cell.line,
                    format!("#{}: check-of={of} names no cell", cell.id),
                ),
                Some(target) if !target.is_step() => push(
                    &mut out,
                    Severity::Error,
                    "check-of-check",
                    Some(&cell.id),
                    cell.line,
                    format!("#{}: check-of={of} names a check, not a step", cell.id),
                ),
                _ => {}
            },
            _ => {}
        }
        if !cell.is_step() && cell.needs.is_some() {
            push(
                &mut out,
                Severity::Warning,
                "attribute",
                Some(&cell.id),
                cell.line,
                format!(
                    "#{}: a check runs when its step succeeds; needs= is ignored",
                    cell.id
                ),
            );
        }
    }

    let needs = resolved_needs(book);
    for step in book.steps() {
        for need in needs.get(&step.id).into_iter().flatten() {
            match book.cell(need) {
                None => push(
                    &mut out,
                    Severity::Error,
                    "unknown-need",
                    Some(&step.id),
                    step.line,
                    format!("#{}: needs={need} names no cell", step.id),
                ),
                Some(c) if !c.is_step() => push(
                    &mut out,
                    Severity::Error,
                    "needs-check",
                    Some(&step.id),
                    step.line,
                    format!(
                        "#{}: needs={need} names a check; need its step instead",
                        step.id
                    ),
                ),
                _ => {}
            }
        }
    }

    for cycle in cycles(&needs) {
        let line = book.cell(&cycle[0]).map(|c| c.line).unwrap_or(0);
        for id in &cycle {
            out.blocked.insert(id.clone());
        }
        out.lints.push(Lint {
            severity: Severity::Error,
            code: "cycle".into(),
            cell: Some(cycle[0].clone()),
            line,
            message: format!("a cycle: {} → {}", cycle.join(" → "), cycle[0]),
        });
    }

    if let Some(bindings) = bindings {
        for missing in &bindings.missing {
            push(
                &mut out,
                Severity::Warning,
                "unbound-param",
                None,
                1,
                missing.clone(),
            );
        }
        for cell in book.cells.iter().filter(|c| c.kind != CellKind::Manual) {
            let (_, unbound) =
                crate::params::subst(&cell.body, bindings, crate::params::Form::Display);
            if !unbound.is_empty() {
                push(
                    &mut out,
                    Severity::Error,
                    "unbound-param",
                    Some(&cell.id),
                    cell.line,
                    format!("#{}: unbound {}", cell.id, unbound.join(", ")),
                );
            }
        }
    }
    out
}

/// Every cycle of the needs graph, each once, as the ids along it.
fn cycles(needs: &HashMap<String, Vec<String>>) -> Vec<Vec<String>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        New,
        Open,
        Done,
    }
    fn visit(
        id: &str,
        needs: &HashMap<String, Vec<String>>,
        marks: &mut HashMap<String, Mark>,
        stack: &mut Vec<String>,
        found: &mut Vec<Vec<String>>,
    ) {
        marks.insert(id.to_string(), Mark::Open);
        stack.push(id.to_string());
        for next in needs.get(id).into_iter().flatten() {
            if !needs.contains_key(next) {
                continue;
            }
            match marks.get(next).copied().unwrap_or(Mark::New) {
                Mark::New => visit(next, needs, marks, stack, found),
                Mark::Open => {
                    let at = stack.iter().position(|s| s == next).unwrap_or(0);
                    found.push(stack[at..].to_vec());
                }
                Mark::Done => {}
            }
        }
        stack.pop();
        marks.insert(id.to_string(), Mark::Done);
    }
    let mut keys: Vec<&String> = needs.keys().collect();
    keys.sort();
    let mut marks = HashMap::new();
    let mut found = Vec::new();
    for key in keys {
        if marks.get(key).copied().unwrap_or(Mark::New) == Mark::New {
            visit(key, needs, &mut marks, &mut Vec::new(), &mut found);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(text: &str) -> Vec<String> {
        lint(&Runbook::parse(text), None)
            .lints
            .into_iter()
            .map(|l| l.code)
            .collect()
    }

    #[test]
    fn a_clean_runbook_has_no_lints() {
        let text =
            "```sh {#a}\ntrue\n```\n```sh {#b}\ntrue\n```\n```sh {#b-ok check-of=b}\ntrue\n```\n";
        assert!(codes(text).is_empty());
    }

    #[test]
    fn missing_and_duplicate_ids() {
        let text = "```sh {readonly}\ntrue\n```\n```sh {#a}\ntrue\n```\n```sh {#a}\ntrue\n```\n";
        assert_eq!(codes(text), ["missing-id", "duplicate-id"]);
    }

    #[test]
    fn unknown_needs_needs_on_a_check_and_check_of_a_check() {
        let text = "```sh {#a needs=zz}\ntrue\n```\n```sh {#c check-of=a}\ntrue\n```\n```sh {#d check-of=c}\ntrue\n```\n```sh {#e needs=c}\ntrue\n```\n";
        let got = codes(text);
        assert!(got.contains(&"unknown-need".to_string()), "{got:?}");
        assert!(got.contains(&"check-of-check".to_string()), "{got:?}");
        assert!(got.contains(&"needs-check".to_string()), "{got:?}");
    }

    #[test]
    fn a_cycle_blocks_every_step_on_it() {
        let text = "```sh {#a needs=c}\ntrue\n```\n```sh {#b needs=a}\ntrue\n```\n```sh {#c needs=b}\ntrue\n```\n```sh {#d needs=}\ntrue\n```\n";
        let lints = lint(&Runbook::parse(text), None);
        assert_eq!(lints.lints.len(), 1);
        assert_eq!(lints.lints[0].code, "cycle");
        assert_eq!(
            lints.blocked.iter().cloned().collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
    }

    #[test]
    fn unbound_and_required_params_are_reported_with_bindings() {
        let book = Runbook::parse(
            "---\nparams:\n  env: { required: true }\n---\n```sh {#a}\necho {{env}}\n```\n",
        );
        let bindings = Bindings::defaults(&book);
        let lints = lint(&book, Some(&bindings));
        let got: Vec<_> = lints.lints.iter().map(|l| l.code.as_str()).collect();
        assert_eq!(got, ["unbound-param", "unbound-param"]);
        assert!(lints.blocked.contains("a"));
    }

    #[test]
    fn default_needs_skip_checks() {
        let book = Runbook::parse(
            "```sh {#a}\ntrue\n```\n```sh {#a-ok check-of=a}\ntrue\n```\n```sh {#b}\ntrue\n```\n```sh {#r needs=}\ntrue\n```\n",
        );
        let needs = resolved_needs(&book);
        assert_eq!(needs["b"], ["a"]);
        assert!(needs["a"].is_empty());
        assert!(needs["r"].is_empty());
    }
}
