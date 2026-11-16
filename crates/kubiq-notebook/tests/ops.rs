//! Cell operations, the notebook-level undo stack, staleness, and reload matched by id.

use kubiq_notebook::doc::{CellOp, Notebook, Origin};
use kubiq_notebook::json::{self, Json};
use kubiq_notebook::nbformat::{Output, output_from_message};
use kubiq_notebook::CellKind;

fn three() -> Notebook {
    let mut nb = Notebook::new_empty(3);
    let ops = ["a", "b"]
        .iter()
        .enumerate()
        .map(|(n, id)| CellOp::Insert {
            index: n + 1,
            id: Some((*id).into()),
            kind: CellKind::Code,
            source: format!("{id} = {n}"),
        })
        .collect();
    nb.apply(ops, Origin::Human).unwrap();
    nb
}

#[test]
fn ops_by_id_and_undo() {
    let mut nb = three();
    let first = nb.order()[0].clone();
    assert_eq!(nb.order()[1..], ["a".to_string(), "b".to_string()]);
    nb.apply(vec![CellOp::Move { id: "b".into(), to: 0 }], Origin::Human).unwrap();
    assert_eq!(nb.order(), vec!["b".to_string(), first.clone(), "a".to_string()]);
    nb.apply(vec![CellOp::Delete { id: "a".into() }], Origin::Agent("x".into())).unwrap();
    assert_eq!(nb.order().len(), 2);
    // A human's typing is the cell editor's history, not the notebook's.
    nb.apply(vec![CellOp::Source { id: "b".into(), source: "typed".into() }], Origin::Human).unwrap();
    // An agent's source edit is one notebook step.
    nb.apply(vec![CellOp::Source { id: "b".into(), source: "agent".into() }], Origin::Agent("x".into()))
        .unwrap();
    let (_, origin) = nb.undo().unwrap();
    assert_eq!(origin, Origin::Agent("x".into()));
    assert_eq!(nb.cell("b").unwrap().source, "typed");
    nb.undo().unwrap(); // the delete
    assert!(nb.cell("a").is_some());
    nb.undo().unwrap(); // the move
    assert_eq!(nb.order(), vec![first, "a".to_string(), "b".to_string()]);
    assert!(
        nb.apply(vec![CellOp::Delete { id: "nope".into() }], Origin::Human).is_err(),
        "an unknown id refuses the batch"
    );
}

#[test]
fn kind_change_drops_outputs_and_undo_restores_them() {
    let mut nb = three();
    let out = output_from_message("stream", &json::parse(r#"{"name":"stdout","text":"hi\n"}"#).unwrap()).unwrap();
    nb.run_started("a", "a = 0");
    nb.push_output("a", out);
    nb.apply(vec![CellOp::Kind { id: "a".into(), kind: CellKind::Markdown }], Origin::Human).unwrap();
    assert!(nb.cell("a").unwrap().outputs.is_empty());
    nb.undo().unwrap();
    assert_eq!(nb.cell("a").unwrap().outputs.len(), 1);
}

#[test]
fn staleness() {
    let mut nb = three();
    nb.run_started("a", "a = 0");
    nb.run_started("b", "b = 1");
    assert!(!nb.stale("a") && !nb.stale("b"));
    // Re-running a cell above makes the one below stale.
    nb.run_started("a", "a = 0");
    assert!(nb.stale("b") && !nb.stale("a"));
    // Editing a ran cell's source makes it stale.
    nb.apply(vec![CellOp::Source { id: "a".into(), source: "a = 9".into() }], Origin::Human).unwrap();
    assert!(nb.stale("a"));
}

#[test]
fn streams_merge_and_display_updates_in_place() {
    let mut nb = three();
    let msg = |t: &str, c: &str| output_from_message(t, &json::parse(c).unwrap()).unwrap();
    nb.run_started("a", "");
    nb.push_output("a", msg("stream", r#"{"name":"stdout","text":"one\n"}"#));
    nb.push_output("a", msg("stream", r#"{"name":"stdout","text":"two\n"}"#));
    nb.push_output(
        "a",
        msg("display_data", r#"{"data":{"text/plain":"0%"},"metadata":{},"transient":{"display_id":"p"}}"#),
    );
    let touched = nb.update_display(
        "p",
        msg("update_display_data", r#"{"data":{"text/plain":"100%"},"metadata":{},"transient":{"display_id":"p"}}"#),
    );
    assert_eq!(touched, vec!["a".to_string()]);
    let view = nb.view("a").unwrap();
    assert_eq!(view.outputs.len(), 2);
    assert_eq!(view.outputs[0], Output::Stream { name: "stdout".into(), text: "one\ntwo\n".into() });
    let Output::Data { bundle, .. } = &view.outputs[1] else { panic!() };
    assert_eq!(bundle[0].1, "100%");
    // The saved form is nbformat's: lines, sorted keys, no transient.
    let raw = &nb.cell("a").unwrap().outputs[1].raw;
    assert_eq!(raw.get("data").unwrap().get("text/plain"), Some(&Json::Arr(vec![Json::str("100%")])));
    assert!(raw.get("transient").is_none());
}

#[test]
fn an_approval_covers_only_the_source_that_was_shown() {
    let mut nb = three();
    let shown = vec![("a".to_string(), "a = 0".to_string()), ("b".to_string(), "b = 1".to_string())];
    assert!(nb.changed_since(&shown).is_empty());
    nb.apply(vec![CellOp::Source { id: "b".into(), source: "import os".into() }], Origin::Agent("x".into()))
        .unwrap();
    assert_eq!(nb.changed_since(&shown), vec!["b".to_string()]);
    nb.apply(vec![CellOp::Delete { id: "a".into() }], Origin::Human).unwrap();
    assert_eq!(nb.changed_since(&shown), vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn reload_matches_by_id() {
    let mut nb = three();
    nb.run_started("a", "a = 0");
    nb.run_started("b", "b = 1");
    nb.run_started("a", "a = 0");
    assert!(nb.stale("b"));
    let text = nb.serialize();
    let file = Notebook::parse(&text, 5).unwrap().file;
    nb.reload(file);
    assert!(nb.stale("b"), "a kept id keeps its run marks");
    assert!(!nb.dirty());
}
