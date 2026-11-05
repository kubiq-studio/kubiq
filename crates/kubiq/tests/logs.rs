//! The log console's own state: the two pickers, on the same indexing the console draws them with.
//!
//! The sink itself is `kubiq-proto`'s and is tested there. What is left here is the half that
//! belongs to the window — which subsystem and which level the console is filtered to — and it is
//! pure, so it needs neither a frame nor a subscriber.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::SystemTime;

use gpui::{
    AppContext, Context, Entity, IntoElement, ListAlignment, ListState, Render, Styled,
    TestAppContext, VisualTestContext, Window, list, point, px, size,
};
use kubiq::state::{LogAction, LogPress, LogState, match_ranges, row_text, rows_text};
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

fn from(seq: u64, subsystem: Subsystem, message: &str) -> Arc<LogRecord> {
    Arc::new(LogRecord {
        seq,
        at: SystemTime::UNIX_EPOCH,
        level: LogLevel::Info,
        subsystem,
        target: "kubiq".into(),
        message: message.into(),
    })
}

#[test]
fn the_search_keeps_rows_whose_message_or_subsystem_contains_it_ignoring_case() {
    let records = vec![
        from(1, Subsystem::Pty, "opened at 80x24"),
        from(2, Subsystem::Ui, "window OPENED"),
        from(3, Subsystem::Mcp, "listening"),
        from(4, Subsystem::Ui, "drew a frame"),
    ];
    let seqs = |kept: Vec<Arc<LogRecord>>| kept.iter().map(|r| r.seq).collect::<Vec<_>>();
    let mut state = LogState::default();

    // An empty query passes every row the sink's filter did.
    assert_eq!(seqs(state.search(records.clone())), [1, 2, 3, 4]);

    assert!(state.set_query("Opened"));
    assert_eq!(seqs(state.search(records.clone())), [1, 2]);

    // The subsystem's label is searched as well as the message.
    state.set_query("mcp");
    assert_eq!(seqs(state.search(records.clone())), [3]);

    state.set_query("nothing like it");
    assert!(state.search(records.clone()).is_empty());
}

#[test]
fn a_query_change_bumps_the_generation_the_list_is_reset_by() {
    let mut state = LogState::default();
    let start = state.query_gen;
    assert!(state.set_query("pty"));
    assert_eq!(state.query_gen, start + 1);
    // The same text again is no change, so the list keeps what it measured.
    assert!(!state.set_query("pty"));
    assert_eq!(state.query_gen, start + 1);
    assert!(state.set_query(""));
    assert_eq!(state.query(), "");
    assert_eq!(state.query_gen, start + 2);
}

#[test]
fn matches_are_marked_on_the_texts_own_boundaries() {
    assert_eq!(match_ranges("Opened, then opened", "OPEN"), [0..4, 13..17]);
    assert!(match_ranges("anything", "").is_empty());
    assert!(match_ranges("anything", "zz").is_empty());
    // Non-overlapping: "aa" in "aaa" is one match, not two.
    assert_eq!(match_ranges("aaa", "aa"), [0..2]);
    // A character whose lowercase is longer in bytes still yields ranges on its own boundaries.
    let text = "ÀB àb";
    let found = match_ranges(text, "àb");
    assert_eq!(found.len(), 2);
    for range in found {
        assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
        assert_eq!(text[range].to_lowercase(), "àb");
    }
}

// ── The wrapped row's layout ───────────────────────────────────────────────────────────────

/// Draws one log row in a `gpui::list` the way the console does, at a chosen width.
struct OneRow {
    list: ListState,
    record: Arc<LogRecord>,
    wrap: bool,
}

impl Render for OneRow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let record = self.record.clone();
        let wrap = self.wrap;
        list(self.list.clone(), move |_, _, _| {
            kubiq::ui::logs::line(&record, wrap, false, "").into_any_element()
        })
        .size_full()
    }
}

fn row_height(
    cx: &mut VisualTestContext,
    view: &Entity<OneRow>,
    list: &ListState,
    width: f32,
) -> f32 {
    let view = view.clone();
    cx.draw(
        point(px(0.), px(0.)),
        size(px(width), px(600.)),
        move |_, _| view.into_any_element(),
    );
    let bounds = list.bounds_for_item(0).expect("the row was laid out");
    assert!(
        f32::from(bounds.size.width) <= width,
        "the row is {:?} wide in a {width}px list",
        bounds.size.width
    );
    f32::from(bounds.size.height)
}

#[gpui::test]
fn a_wrapped_message_breaks_within_the_panel_and_regrows_when_it_narrows(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        kubiq::theme::set_mode(kubiq::app::boot_theme(), cx);
    });
    let cx = cx.add_empty_window();
    let long = "word ".repeat(120);
    let list = ListState::new(1, ListAlignment::Top, px(1000.));

    let cut = cx.update(|_, cx| {
        cx.new(|_| OneRow {
            list: list.clone(),
            record: from(1, Subsystem::Ui, &long),
            wrap: false,
        })
    });
    let single = row_height(cx, &cut, &list, 600.);

    let list = ListState::new(1, ListAlignment::Top, px(1000.));
    let wrapped = cx.update(|_, cx| {
        cx.new(|_| OneRow {
            list: list.clone(),
            record: from(1, Subsystem::Ui, &long),
            wrap: true,
        })
    });
    let wide = row_height(cx, &wrapped, &list, 600.);
    assert!(
        wide > single * 2.,
        "a wrapped row is several lines tall: {wide} against {single}"
    );

    // The list drops its measurements when its width moves, so a narrower panel is a taller row.
    let narrow = row_height(cx, &wrapped, &list, 300.);
    assert!(
        narrow > wide,
        "narrowing regrows the row: {narrow} against {wide}"
    );
}
