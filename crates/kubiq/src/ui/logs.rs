//! The log console: a panel in the window's dock, like any other.
//!
//! Everything the application, the coordinator and the harness library say through `tracing`
//! lands in the process-wide sink in [`kubiq_proto::log`]; this draws a filtered view of it. It reads and
//! never writes — clearing the ring is the one thing it asks of the sink.
//!
//! The console used to borrow the pane strip, which cost the strip one tab with no pane behind it
//! and meant a pane and the console could not be read at once. Under a dock it is a panel: it has
//! its own tab, its own toolbar in [`actions`], and it sits wherever the user has dragged it —
//! beside a terminal, in the centre, or in a group of its own. That is `D25` superseded.
//!
//! Rows are drawn lazily by `gpui::list`, because a full ring is thousands of them and only the
//! visible ones are worth laying out. A row is one cut-off line, or wraps to its message's height
//! when the toolbar's wrap toggle is on; rows are selectable and carry a right-click menu. The
//! toolbar's search field narrows the rows to those containing its text and marks the matches.

use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, Context, Div, Entity, Focusable, HighlightStyle, InteractiveElement,
    IntoElement, KeyBinding, MouseButton, ParentElement, Rgba, SharedString, Stateful,
    StatefulInteractiveElement, Styled, StyledText, Window, div, list, point, px,
};
use gpui_component::input::Input;
use gpui_component::tooltip::Tooltip;
use gpui_component::{Icon, IconName, Sizable, Size};

use crate::app::AppState;
use crate::state::{LogAction, LogPress, LogState, MenuId, match_ranges};
use crate::theme;
use crate::ui::kit::{
    ContextItem, KubiqIcon, Picker, context_menu, field, ghost_button, icon_button, mono,
};
use crate::ui::{eid, handler, indexed};
use kubiq_proto::log::{LogLevel, LogRecord};

/// The key context the console's chords are bound in.
const CONTEXT: &str = "Logs";

/// The search field's context: the console's own, with the component library's field under it.
const FIELD_CONTEXT: &str = "Logs > Input";

gpui::actions!(kubiq_logs, [LogsCopy, LogsSelectAll, LogsClearSearch]);

/// The height of one row, and the least a wrapped one takes.
const ROW_HEIGHT: f32 = 22.0;

/// The colour a level is reported in. Status is never shown by wording alone, so the level word,
/// the message and the row's edge all take it.
pub fn level_colour(level: LogLevel) -> Rgba {
    match level {
        LogLevel::Error => theme::danger(),
        LogLevel::Warn => theme::warning(),
        LogLevel::Info => theme::info(),
        LogLevel::Debug => theme::text_muted(),
        LogLevel::Trace => theme::text_faint(),
    }
}

/// The whole panel: its toolbar over its records.
pub fn render(app: &AppState, window: &Window, cx: &mut Context<AppState>) -> AnyElement {
    div()
        .id("logs")
        .key_context(CONTEXT)
        .track_focus(&app.log_focus)
        .on_action(cx.listener(|this, _: &LogsCopy, _, cx| {
            if !this.copy_log_selection(cx) {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|this, _: &LogsSelectAll, _, cx| this.select_all_logs(cx)))
        .on_action(cx.listener(|this, _: &LogsClearSearch, window, cx| {
            if !this.clear_log_search(window, cx) {
                cx.propagate();
            }
        }))
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .bg(theme::pane_bg())
        .child(
            div()
                .h(px(34.))
                .px_2()
                .flex()
                .flex_none()
                .items_center()
                .justify_end()
                .border_b_1()
                .border_color(theme::border())
                .gap_2()
                .child(search_field(app, window, cx))
                .child(actions(app, cx)),
        )
        .child(body(app, cx))
        .into_any_element()
}

/// The chords the console answers while it holds the keyboard: the platform's copy and select-all,
/// and Escape in the search field.
///
/// Copy and select-all are bound in the console's context only, so with the keyboard in the
/// search field the field's own `Input` bindings — one node deeper — win and copy its text. Escape
/// is the other way round: the field binds it too, so it is bound at the field's depth and must be
/// registered after the component library's (`app::install_key_bindings`).
pub fn key_bindings() -> Vec<KeyBinding> {
    let (copy, all) = if cfg!(target_os = "macos") {
        ("cmd-c", "cmd-a")
    } else {
        ("ctrl-c", "ctrl-a")
    };
    vec![
        KeyBinding::new(copy, LogsCopy, Some(CONTEXT)),
        KeyBinding::new(all, LogsSelectAll, Some(CONTEXT)),
        KeyBinding::new("escape", LogsClearSearch, Some(FIELD_CONTEXT)),
    ]
}

/// The search field: rows whose message or subsystem contains its text, ignoring case.
fn search_field(app: &AppState, window: &Window, cx: &App) -> impl IntoElement {
    let focused = app.log_search.read(cx).focus_handle(cx).is_focused(window);
    field(theme::border(), focused)
        .w(px(220.))
        .min_w(px(0.))
        .h(px(26.))
        .px_2()
        .gap_2()
        .child(
            Icon::new(IconName::Search)
                .with_size(Size::XSmall)
                .text_color(theme::text_faint()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(theme::font(theme::Family::Chrome, theme::Role::Body))
                .child(Input::new(&app.log_search).appearance(false)),
        )
}

/// The console's toolbar: what it is showing, and the two selectors that decide.
pub fn actions(app: &AppState, cx: &mut Context<AppState>) -> AnyElement {
    let view = cx.entity();
    let logs = &app.logs;
    let (kept, dropped) = kubiq_proto::log::logs().counts();

    let counted = if dropped == 0 {
        format!("{kept} records")
    } else {
        format!("{kept} records \u{b7} {dropped} dropped")
    };

    div()
        .flex()
        .items_center()
        .gap_1()
        .child(
            Picker::new("log-subsystem", logs.subsystem_label())
                .items(LogState::subsystem_items())
                .selected(logs.subsystem_index())
                .open(app.workbench.open_menu == Some(MenuId::LogSubsystem))
                .on_toggle(handler(&view, |this, _, cx| {
                    this.open_menu(MenuId::LogSubsystem, cx)
                }))
                .on_pick(indexed(&view, |this, index, _, cx| {
                    this.pick_log_subsystem(index, cx)
                }))
                .on_dismiss(handler(&view, |this, _, cx| this.close_menu(cx))),
        )
        .child(
            Picker::new("log-level", logs.min_level.label())
                .items(LogState::level_items())
                .selected(logs.level_index())
                .open(app.workbench.open_menu == Some(MenuId::LogLevel))
                .on_toggle(handler(&view, |this, _, cx| {
                    this.open_menu(MenuId::LogLevel, cx)
                }))
                .on_pick(indexed(&view, |this, index, _, cx| {
                    this.pick_log_level(index, cx)
                }))
                .on_dismiss(handler(&view, |this, _, cx| this.close_menu(cx))),
        )
        .child(
            mono(counted, theme::text_faint())
                .text_size(theme::font(theme::Family::Content, theme::Role::Meta)),
        )
        .child(
            icon_button(
                "log-wrap",
                KubiqIcon::ViewWrap,
                logs.wrap,
                cx.listener(|this, _, _, cx| this.toggle_log_wrap(cx)),
            )
            .tooltip(|window, cx| Tooltip::new("Wrap lines").build(window, cx)),
        )
        .child(
            icon_button(
                "log-follow",
                IconName::ArrowDown,
                logs.follow,
                cx.listener(|this, _, _, cx| this.toggle_log_follow(cx)),
            )
            .tooltip(|window, cx| Tooltip::new("Follow the tail").build(window, cx)),
        )
        .child(ghost_button(
            "log-clear",
            Some(IconName::Delete),
            "Clear",
            cx.listener(|this, _, _, cx| this.clear_logs(cx)),
        ))
        .into_any_element()
}

/// The records themselves, in the space a pane's emulator would fill.
pub fn body(app: &AppState, cx: &mut Context<AppState>) -> AnyElement {
    let records = app.log_rows();
    let view = cx.entity();
    let wrap = app.logs.wrap;
    let query = SharedString::from(app.logs.query().to_owned());

    // Rows measure themselves once they can wrap, so the list is told when the rows under it
    // changed identity (the ring dropped its front, the filter or the search moved, wrap flipped)
    // and only otherwise grows by splice, which keeps what is already measured. A change of the
    // list's width is the list's own business: it drops every measurement when its width moves.
    let key = (
        records.first().map(|r| r.seq),
        records.len(),
        wrap,
        app.logs.query_gen,
    );
    let last = app.log_list_key.replace(key);
    let same = last.0 == key.0 && last.2 == key.2 && last.3 == key.3;
    if app.log_list.item_count() != records.len() || !same {
        if same && key.1 > app.log_list.item_count() {
            let old = app.log_list.item_count();
            app.log_list.splice(old..old, key.1 - old);
        } else {
            app.log_list.reset(records.len());
        }
    }

    // Following means the tail stays in view as records arrive.
    if app.logs.follow && !records.is_empty() {
        app.log_list.scroll_to_end();
    }

    let mut surface = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .bg(theme::pane_bg())
        // The console is a surface like a pane, and its edge carries the loudest thing it holds.
        .border_l(px(theme::accent_edge()))
        .border_color(
            kubiq_proto::log::logs()
                .loudest()
                .filter(|level| *level >= LogLevel::Warn)
                .map_or(theme::text_faint(), level_colour),
        );

    if records.is_empty() {
        surface = surface.child(empty(app));
    } else {
        let rows_view = view.clone();
        surface = surface.child(
            list(
                app.log_list.clone(),
                move |index, _window, cx| match records.get(index) {
                    Some(record) => row(record, wrap, &query, &rows_view, cx),
                    None => div().into_any_element(),
                },
            )
            .flex_1()
            .min_h(px(0.)),
        );
    }

    if app.workbench.open_menu == Some(MenuId::LogRow)
        && let Some(menu) = app.logs.menu.clone()
    {
        let epoch = menu.epoch;
        let items: Vec<ContextItem> = LogAction::ALL
            .iter()
            .map(|action| match action {
                LogAction::Separator => ContextItem::separator(),
                action => ContextItem::new(action.label()),
            })
            .collect();
        surface = surface.child(context_menu(
            "log-menu",
            point(px(menu.x), px(menu.y)),
            items,
            indexed(&view, |this, index, _, cx| this.pick_log_action(index, cx)),
            handler(&view, move |this, _, cx| this.dismiss_log_menu(epoch, cx)),
        ));
    }

    surface.into_any_element()
}

/// One record with its pointer handling: click selects, shift extends, cmd toggles, right-click
/// raises the menu.
fn row(
    record: &Arc<LogRecord>,
    wrap: bool,
    query: &str,
    view: &Entity<AppState>,
    cx: &App,
) -> AnyElement {
    let seq = record.seq;
    let selected = view.read(cx).logs.selected.contains(&seq);
    line(record, wrap, selected, query)
        .on_mouse_down(MouseButton::Left, {
            let view = view.clone();
            move |event, window, cx| {
                let how = if event.modifiers.shift {
                    LogPress::Extend
                } else if event.modifiers.secondary() {
                    LogPress::Toggle
                } else {
                    LogPress::Only
                };
                view.update(cx, |this, cx| this.press_log_row(seq, how, window, cx));
            }
        })
        .on_mouse_down(MouseButton::Right, {
            let view = view.clone();
            move |event, _, cx| {
                let at = (f32::from(event.position.x), f32::from(event.position.y));
                view.update(cx, |this, cx| this.open_log_menu(seq, at, cx));
            }
        })
        .into_any_element()
}

/// One record as drawn: when, how loud, from where, and what it said, with the search's matches
/// marked. Public so a test can lay it out the way the list does.
///
/// **The row is as wide as the list.** `gpui::list` lays a row out as a root of its own
/// (`layout_as_root`), with the list's width as *available* space only — and a flex row of auto
/// width under a definite available width is sized by taffy to the sum of its children's bases,
/// not to that width. Without `w_full` the message cell is never squeezed: the row grows past the
/// panel's right edge and the message is clipped instead of wrapped. `w_full` resolves against the
/// width the list offers, which leaves the message `flex_1` + `min_w(0)` the rest of it to wrap in.
///
/// **Padding, not margin**, and a wrapped row is as tall as its message: `gpui::list` measures
/// the border box.
pub fn line(record: &LogRecord, wrap: bool, selected: bool, query: &str) -> Stateful<Div> {
    let colour = level_colour(record.level);
    let warns = record.level >= LogLevel::Warn;
    let meta = theme::font(theme::Family::Content, theme::Role::Meta);

    let mut message = div()
        .font_family(theme::MONO_FONT)
        .text_size(theme::font(theme::Family::Chrome, theme::Role::Label))
        .text_color(if warns { colour } else { theme::text() })
        .flex_1()
        .min_w(px(0.))
        .child(highlighted(&record.message, query));
    if !wrap {
        message = message.overflow_hidden().whitespace_nowrap();
    }

    let line = div()
        .id(eid("log-row", record.seq))
        .w_full()
        .min_h(px(ROW_HEIGHT))
        .px_2()
        .flex()
        .gap_3()
        .overflow_hidden()
        .when(selected, |this| this.bg(theme::selected()))
        .when(!selected, |this| this.hover(|this| this.bg(theme::hover())))
        .child(
            mono(record.time(), theme::text_faint())
                .flex_none()
                .text_size(meta),
        )
        .child(
            mono(record.level.label(), colour)
                .w(px(46.))
                .flex_none()
                .text_size(meta),
        )
        .child(
            div()
                .font_family(theme::MONO_FONT)
                .text_color(theme::text_muted())
                .w(px(88.))
                .flex_none()
                .text_size(meta)
                .child(highlighted(record.subsystem.label(), query)),
        )
        .child(message);

    // A single-line row is centred in its fixed height; a wrapped one hangs from its first line.
    if wrap {
        line.items_start().py(px(3.))
    } else {
        line.h(px(ROW_HEIGHT)).items_center()
    }
}

/// `text` with every case-insensitive occurrence of `query` on the search-hit ground — the same
/// marking the project search draws its hits with. Plain text when nothing matches.
fn highlighted(text: &str, query: &str) -> AnyElement {
    let text = SharedString::from(text.to_owned());
    let ranges = match_ranges(&text, query);
    if ranges.is_empty() {
        return text.into_any_element();
    }
    let mark = HighlightStyle {
        color: Some(theme::text().into()),
        background_color: Some(theme::accent_soft().into()),
        ..Default::default()
    };
    StyledText::new(text)
        .with_highlights(ranges.into_iter().map(move |range| (range, mark)))
        .into_any_element()
}

/// What the console shows when the filter matches nothing. It names the filter, because an empty
/// console is usually a filter and not a quiet application.
fn empty(app: &AppState) -> impl IntoElement {
    let level = app.logs.min_level.label();
    let note = match app.logs.subsystem {
        Some(subsystem) => format!("Nothing from {} at {level} or above.", subsystem.label()),
        None => format!("Nothing at {level} or above."),
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .items_center()
        .justify_center()
        .child(mono(note, theme::text_faint()))
}
