//! The log console's own state: the two pickers, on the same indexing the console draws them with.
//!
//! The sink itself is `kubiq-proto`'s and is tested there. What is left here is the half that
//! belongs to the window — which subsystem and which level the console is filtered to — and it is
//! pure, so it needs neither a frame nor a subscriber.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::SystemTime;

use kubiq::state::{LogAction, LogPress, LogState, row_text, rows_text};
use kubiq_proto::log::{LogLevel, LogRecord, Subsystem};

#[test]
fn the_console_pickers_answer_on_the_indexing_they_are_drawn_with() {
    let mut state = LogState::default();
    assert_eq!(state.subsystem_label(), "All");

    state.pick_subsystem(3);
    assert_eq!(state.subsystem, Some(Subsystem::Pty));
    assert_eq!(state.subsystem_index(), 3);

    // Index zero is "All", which is an absent filter rather than a subsystem.
    state.pick_subsystem(0);
    assert_eq!(state.filter().subsystem, None);
    assert_eq!(state.subsystem_index(), 0);

    state.pick_level(4);
    assert_eq!(state.min_level, LogLevel::Error);
    assert_eq!(state.filter().min_level, LogLevel::Error);
}

fn record(seq: u64, message: &str) -> Arc<LogRecord> {
    Arc::new(LogRecord {
        seq,
        at: SystemTime::UNIX_EPOCH,
        level: LogLevel::Warn,
        subsystem: Subsystem::Pty,
        target: "kubiq_host::pty".into(),
        message: message.into(),
    })
}

#[test]
fn a_click_selects_shift_extends_and_cmd_toggles() {
    let seqs = [10, 11, 12, 13, 14];
    let mut state = LogState::default();

    state.press(&seqs, 11, LogPress::Only);
    assert_eq!(state.selected, BTreeSet::from([11]));

    // Shift extends from the anchor, in either direction, and replaces the previous range.
    state.press(&seqs, 13, LogPress::Extend);
    assert_eq!(state.selected, BTreeSet::from([11, 12, 13]));
    state.press(&seqs, 10, LogPress::Extend);
    assert_eq!(state.selected, BTreeSet::from([10, 11]));

    // Toggle adds and removes one row, and moves the anchor.
    state.press(&seqs, 14, LogPress::Toggle);
    assert_eq!(state.selected, BTreeSet::from([10, 11, 14]));
    state.press(&seqs, 11, LogPress::Toggle);
    assert_eq!(state.selected, BTreeSet::from([10, 14]));

    // A row the list no longer holds changes nothing.
    state.press(&seqs, 99, LogPress::Only);
    assert_eq!(state.selected, BTreeSet::from([10, 14]));

    state.select_all(&seqs);
    assert_eq!(state.selected.len(), 5);
    state.clear_selection();
    assert!(state.selected.is_empty());
}

#[test]
fn the_menu_acts_on_the_selection_or_the_clicked_row() {
    let seqs = [1, 2, 3];
    let mut state = LogState::default();
    state.press(&seqs, 1, LogPress::Only);
    state.press(&seqs, 2, LogPress::Toggle);

    assert_eq!(state.targets(2), BTreeSet::from([1, 2]));
    assert_eq!(state.targets(3), BTreeSet::from([3]));

    state.open_menu(3, 5.0, 6.0);
    let stale = state.menu.as_ref().unwrap().epoch;
    state.open_menu(2, 7.0, 8.0);
    // The first menu's outside click arrives after the second opened and does nothing.
    state.close_menu(stale);
    assert!(state.menu.is_some());
    state.close_menu(state.menu.as_ref().unwrap().epoch);
    assert!(state.menu.is_none());

    assert_eq!(
        LogAction::ALL[LogAction::ALL.len() - 1].label(),
        "Select all"
    );
}

#[test]
fn copied_rows_are_one_per_line_as_displayed() {
    let records = vec![
        record(1, "one"),
        record(2, "two\nlines"),
        record(3, "three"),
    ];
    let chosen = BTreeSet::from([1, 3]);

    let first = row_text(&records[0]);
    assert!(first.ends_with(" WARN PTY one"), "{first}");
    assert_eq!(
        rows_text(&records, &chosen, false),
        format!("{}\n{}", row_text(&records[0]), row_text(&records[2]))
    );
    // Message-only carries no time, level or subsystem.
    assert_eq!(rows_text(&records, &chosen, true), "one\nthree");
    assert_eq!(rows_text(&records, &BTreeSet::new(), false), "");
}
