//! The log console's own state: which subsystem it is showing, the level it cuts off at, and
//! whether it follows the tail.
//!
//! The records are not here. They belong to the process-wide sink in [`kubiq_proto::log`], and this is
//! only one window's view onto it — which is why nothing in this file is a fixture.

use std::collections::BTreeSet;
use std::sync::Arc;

use kubiq_proto::log::{Filter, LogLevel, LogRecord, Subsystem};

/// What the selector calls every subsystem at once.
const ALL: &str = "All";

pub struct LogState {
    /// `None` is every subsystem, which the selector shows as `All`.
    pub subsystem: Option<Subsystem>,
    /// The floor: a record quieter than this is not drawn.
    pub min_level: LogLevel,
    /// Whether an arriving record scrolls the list to the tail.
    pub follow: bool,
    /// Whether a long message wraps onto further lines instead of being cut off. A window
    /// preference held in memory, like the filter.
    pub wrap: bool,
    /// The selected rows, by the record's sequence number: the ring drops from the front, and a
    /// sequence number keeps its row's identity where an index would slide.
    pub selected: BTreeSet<u64>,
    /// The row a shift-click extends from: the last one pressed without shift.
    anchor: Option<u64>,
    /// The right-click menu, while one is open.
    pub menu: Option<LogMenu>,
    menu_epoch: u64,
}

/// How a press on a row changes the selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogPress {
    /// A plain click: only this row.
    Only,
    /// Shift-click: everything from the anchor to this row.
    Extend,
    /// Cmd-click (Ctrl elsewhere): this row in or out of the set.
    Toggle,
}

/// The row menu's verbs, in the order it draws them. A separator is a row of its own, because a
/// menu's rows and the actions behind them are matched by position.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogAction {
    Copy,
    CopyMessage,
    Separator,
    SelectAll,
}

impl LogAction {
    pub const ALL: [LogAction; 4] = [
        LogAction::Copy,
        LogAction::CopyMessage,
        LogAction::Separator,
        LogAction::SelectAll,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LogAction::Copy => "Copy",
            LogAction::CopyMessage => "Copy message",
            LogAction::Separator => "",
            LogAction::SelectAll => "Select all",
        }
    }
}

/// The right-click menu: where it opened, and the row it opened on.
#[derive(Clone, Debug)]
pub struct LogMenu {
    pub x: f32,
    pub y: f32,
    pub seq: u64,
    /// Which menu this is, so the outside click of one already replaced does nothing.
    pub epoch: u64,
}

/// One row as it is displayed: `time level subsystem message`.
pub fn row_text(record: &LogRecord) -> String {
    format!(
        "{} {} {} {}",
        record.time(),
        record.level.label(),
        record.subsystem.label(),
        record.message
    )
}

/// The rows of `records` whose sequence number is in `seqs`, one per line.
pub fn rows_text(records: &[Arc<LogRecord>], seqs: &BTreeSet<u64>, message_only: bool) -> String {
    records
        .iter()
        .filter(|record| seqs.contains(&record.seq))
        .map(|record| match message_only {
            true => record.message.clone(),
            false => row_text(record),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl Default for LogState {
    fn default() -> Self {
        Self {
            subsystem: None,
            min_level: LogLevel::Debug,
            follow: true,
            wrap: false,
            selected: BTreeSet::new(),
            anchor: None,
            menu: None,
            menu_epoch: 0,
        }
    }
}

impl LogState {
    pub fn filter(&self) -> Filter {
        Filter {
            subsystem: self.subsystem,
            min_level: self.min_level,
        }
    }

    /// The subsystem selector's rows: `All`, then every subsystem in its own order.
    pub fn subsystem_items() -> Vec<&'static str> {
        let mut items = vec![ALL];
        items.extend(Subsystem::ALL.iter().map(|subsystem| subsystem.label()));
        items
    }

    /// Which row the selector marks, on the same indexing as [`Self::subsystem_items`].
    pub fn subsystem_index(&self) -> usize {
        self.subsystem
            .and_then(|chosen| Subsystem::ALL.iter().position(|s| *s == chosen))
            .map_or(0, |index| index + 1)
    }

    pub fn subsystem_label(&self) -> &'static str {
        self.subsystem.map_or(ALL, |subsystem| subsystem.label())
    }

    /// Take the selector's answer. Row zero is `All`; anything the list does not have is ignored.
    pub fn pick_subsystem(&mut self, index: usize) {
        self.subsystem = index
            .checked_sub(1)
            .and_then(|index| Subsystem::ALL.get(index).copied());
    }

    pub fn level_items() -> Vec<&'static str> {
        LogLevel::ALL.iter().map(|level| level.label()).collect()
    }

    pub fn level_index(&self) -> usize {
        LogLevel::ALL
            .iter()
            .position(|level| *level == self.min_level)
            .unwrap_or(0)
    }

    pub fn pick_level(&mut self, index: usize) {
        if let Some(level) = LogLevel::ALL.get(index) {
            self.min_level = *level;
        }
    }

    /// Apply a press on row `seq`, among the drawn rows `seqs` in order. A row the list no longer
    /// holds changes nothing.
    pub fn press(&mut self, seqs: &[u64], seq: u64, how: LogPress) {
        let Some(at) = seqs.iter().position(|s| *s == seq) else {
            return;
        };
        match how {
            LogPress::Only => {
                self.selected = BTreeSet::from([seq]);
                self.anchor = Some(seq);
            }
            LogPress::Toggle => {
                if !self.selected.remove(&seq) {
                    self.selected.insert(seq);
                }
                self.anchor = Some(seq);
            }
            LogPress::Extend => {
                let from = self
                    .anchor
                    .and_then(|anchor| seqs.iter().position(|s| *s == anchor))
                    .unwrap_or(at);
                let (lo, hi) = (from.min(at), from.max(at));
                self.selected = seqs[lo..=hi].iter().copied().collect();
                self.anchor.get_or_insert(seq);
            }
        }
    }

    pub fn select_all(&mut self, seqs: &[u64]) {
        self.selected = seqs.iter().copied().collect();
        self.anchor = seqs.first().copied();
    }

    pub fn clear_selection(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }

    /// What a menu opened on `seq` acts on: the selection if the row is in it, else the row alone.
    pub fn targets(&self, seq: u64) -> BTreeSet<u64> {
        if self.selected.contains(&seq) {
            self.selected.clone()
        } else {
            BTreeSet::from([seq])
        }
    }

    pub fn open_menu(&mut self, seq: u64, x: f32, y: f32) {
        self.menu_epoch = self.menu_epoch.wrapping_add(1);
        self.menu = Some(LogMenu {
            x,
            y,
            seq,
            epoch: self.menu_epoch,
        });
    }

    pub fn close_menu(&mut self, epoch: u64) {
        if self.menu.as_ref().is_some_and(|menu| menu.epoch == epoch) {
            self.menu = None;
        }
    }
}
