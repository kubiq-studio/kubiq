//! Furniture the settings pages share: a heading, a label/control row, a hinted one, a nav item.
//!
//! **Nothing here is a new primitive.** The kitchen sink composed these first; the live settings
//! overlay and project settings use the same functions, so a row looked at on the sink is the row
//! a screen draws.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, ClickEvent, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px, relative,
};
use gpui_component::{Icon, IconName, Sizable as _, Size};

use crate::theme;
use crate::theme::{Family, Role};
use crate::ui::kit::mono;

pub fn heading(title: &str, note: &str) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .pb_2()
        .child(
            div()
                .text_size(theme::font(Family::Chrome, Role::Title))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .child(SharedString::from(title.to_string())),
        )
        .child(
            div()
                .max_w(px(560.))
                .text_size(theme::font(Family::Chrome, Role::Body))
                .text_color(theme::text_muted())
                .child(SharedString::from(note.to_string())),
        )
        .into_any_element()
}

/// The label column's floor. A question whose control is wide — a field-style picker, or four
/// pills naming an indexing level — used to squeeze this column to one character a line rather
/// than give way; below this width the control wraps onto its own line instead.
const LABEL_MIN_WIDTH: f32 = 220.0;

pub fn setting_row(label: &str, note: &str, control: AnyElement) -> AnyElement {
    div()
        .w(relative(1.))
        .py_3()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_x_6()
        .gap_y_2()
        .border_b_1()
        .border_color(theme::border())
        .child(
            div()
                .flex()
                .flex_1()
                .min_w(px(LABEL_MIN_WIDTH))
                .child(label_block(label, note)),
        )
        .child(control)
        .into_any_element()
}

/// The nav-and-body split **both** settings containers draw: a scrolling nav beside a scrolling
/// page (`D180` — the container owns layout and scroll, a section owns content).
///
/// One component rather than two near-identical ones, which is what had let the two drift into
/// different widths and different padding. Both halves scroll, so neither a nav longer than the
/// panel nor a section longer than it can grow the dialog — the bug `T-244` is about.
///
/// `prefix` is the container's own id prefix (`app-settings`, `project-settings`,
/// `sink-project`); the two scrollers take `{prefix}-nav` and `{prefix}-body`, because
/// `overflow_y_scroll` does nothing without an id.
pub fn settings_split(prefix: &str, items: Vec<AnyElement>, body: AnyElement) -> AnyElement {
    div()
        .flex()
        .flex_1()
        .min_h(px(0.))
        .min_w(px(0.))
        .child(
            div()
                .id(ElementId::Name(format!("{prefix}-nav").into()))
                .w(px(theme::settings_nav_width()))
                .flex()
                .flex_none()
                .flex_col()
                .gap_1()
                .px_2()
                .py_3()
                .overflow_y_scroll()
                .bg(theme::pane_bg())
                .border_r_1()
                .border_color(theme::border())
                .children(items),
        )
        .child(
            div()
                .id(ElementId::Name(format!("{prefix}-body").into()))
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .min_h(px(0.))
                .overflow_y_scroll()
                .px_5()
                .py_4()
                .child(body),
        )
        .into_any_element()
}

/// A label with its explanation folded into a hint icon beside it, and a row built out of one.
///
/// [`label_block`] spends a whole line on the note, which is what turns a form of eight questions
/// into a form that scrolls. The words are worth having and worth reading **once**, so they move
/// onto the hover of a mark next to the label: the row stays one line high, and the explanation is
/// a pointer away rather than gone.
///
/// It takes an id because a tooltip needs a stateful element to hang off — the same bargain
/// [`crate::ui::kit::elided`] makes.
pub fn label_hint(id: impl Into<ElementId>, label: &str, hint: &str) -> AnyElement {
    let hint: SharedString = hint.to_string().into();
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .child(
            div()
                .text_size(theme::font(Family::Chrome, Role::Body))
                .text_color(theme::text())
                .child(SharedString::from(label.to_string())),
        )
        .child(
            div()
                .id(id)
                .flex()
                .flex_none()
                .items_center()
                .child(
                    Icon::new(IconName::Info)
                        .with_size(Size::XSmall)
                        .text_color(theme::text_faint()),
                )
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(hint.clone()).build(window, cx)
                }),
        )
        .into_any_element()
}

/// One question on one line: the label and its hint on the left, the control on the right.
///
/// [`setting_row`]'s shape without the note under it, for a form dense enough that the note is a
/// hint — see [`label_hint`]. No rule between rows either: what groups these is the block they are
/// drawn in, and a hairline under every one of eight rows reads as eight sections.
pub fn hint_row(
    id: impl Into<ElementId>,
    label: &str,
    hint: &str,
    control: AnyElement,
) -> AnyElement {
    div()
        .w(relative(1.))
        .py_1()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(label_hint(id, label, hint))
        .child(control)
        .into_any_element()
}

pub fn label_block(label: &str, note: &str) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .flex_1()
        .min_w(px(0.))
        .child(
            div()
                .text_size(theme::font(Family::Chrome, Role::Body))
                .text_color(theme::text())
                .child(SharedString::from(label.to_string())),
        )
        .child(
            div()
                .text_size(theme::font(Family::Chrome, Role::Meta))
                .text_color(theme::text_muted())
                .child(SharedString::from(note.to_string())),
        )
        .into_any_element()
}

pub fn nav_item(
    id: impl Into<ElementId>,
    icon: impl Into<Icon>,
    label: &str,
    count: Option<usize>,
    selected: bool,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let fg = if !enabled {
        theme::text_faint()
    } else if selected {
        theme::text()
    } else {
        theme::text_muted()
    };
    let icon_fg = if !enabled {
        theme::text_faint()
    } else if selected {
        theme::accent()
    } else {
        theme::text_muted()
    };

    let mut row = div()
        .id(id)
        .h(px(32.))
        .px_2()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .child(Icon::new(icon).with_size(Size::Small).text_color(icon_fg))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(theme::font(Family::Chrome, Role::Body))
                .text_color(fg)
                .child(SharedString::from(label.to_string())),
        );

    if let Some(count) = count {
        row = row.child(
            mono(format!("{count}"), theme::text_faint())
                .text_size(theme::font(Family::Chrome, Role::Meta)),
        );
    }
    if selected && enabled {
        row = row
            .bg(theme::accent_soft())
            .border_l(px(theme::accent_edge()))
            .border_color(theme::accent());
    }

    if enabled {
        row = row
            .cursor_pointer()
            .hover(|this| this.bg(theme::hover()))
            .on_click(on_click);
    }

    row.into_any_element()
}

pub fn column(children: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .w(relative(1.))
        .children(children)
        .into_any_element()
}

// ── List and detail pages ───────────────────────────────────────────
//
// The settings CRUD pattern (`_docs/features/workbench.md`, "Settings pages"): a **list page** is
// a [`list_header`] over [`list_row`]s whose verbs are icon-only `row_action`s; Add or a row's
// Edit **drills in** — the section draws a [`detail_page`] in place of the list (and of its tabs),
// with a breadcrumb back, the form in [`form_group`]s, and a pinned [`detail_footer`]. No editor
// is drawn inline in a list, and none is a modal over the settings overlay.

/// A list page's header: the title, how many rows it holds, the Add (an icon+label
/// `primary_button`) at the right, and one line of explanation under it.
pub fn list_header(
    title: &str,
    count: Option<usize>,
    note: &str,
    add: Option<AnyElement>,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .pt_2()
        .child(
            div()
                .h(px(30.))
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_size(theme::font(Family::Chrome, Role::Title))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text())
                        .child(SharedString::from(title.to_string())),
                )
                .children(count.map(|n| mono(n.to_string(), theme::text_faint())))
                .child(div().flex_1())
                .children(add),
        )
        .when(!note.is_empty(), |this| {
            this.child(
                div()
                    .max_w(px(560.))
                    .text_size(theme::font(Family::Chrome, Role::Meta))
                    .text_color(theme::text_muted())
                    .child(SharedString::from(note.to_string())),
            )
        })
        .into_any_element()
}

/// One list row, always [`theme::SETTINGS_LIST_ROW`] high: the name and a one-line detail at the
/// left (each elided, whole on hover), status chips, then the row's icon-only verbs. Anything
/// that does not fit one line — a test's verdict, a feature line — goes *under* the row, not in it.
pub fn list_row(
    id: impl Into<ElementId>,
    title: &str,
    detail: &str,
    chips: Vec<AnyElement>,
    actions: Vec<AnyElement>,
) -> AnyElement {
    let id: ElementId = id.into();
    div()
        .w(relative(1.))
        .h(px(theme::SETTINGS_LIST_ROW))
        .flex()
        .flex_none()
        .items_center()
        .gap_3()
        .border_b_1()
        .border_color(theme::border())
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .child(crate::ui::kit::elided(
                    ElementId::Name(format!("{id}-title").into()),
                    SharedString::from(title.to_string()),
                    theme::text(),
                    theme::font(Family::Chrome, Role::Body),
                ))
                .when(!detail.is_empty(), |this| {
                    this.child(crate::ui::kit::elided(
                        ElementId::Name(format!("{id}-detail").into()),
                        SharedString::from(detail.to_string()),
                        theme::text_faint(),
                        theme::font(Family::Chrome, Role::Meta),
                    ))
                }),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap_2()
                .children(chips),
        )
        .child(div().flex().flex_none().items_center().children(actions))
        .into_any_element()
}

/// One labelled group of a detail page's form: a section label over its rows, a rule under it.
pub fn form_group(label: &str, rows: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .pb_4()
        .border_b_1()
        .border_color(theme::border())
        .child(crate::ui::kit::section_label(label))
        .children(rows)
        .into_any_element()
}

/// A drill-in editor page, in place of the list it came from: a header with a back button and a
/// breadcrumb (`Remotes › my-host`, the last crumb in full text), an optional banner under it (the
/// unsaved-changes question, [`inline_confirm`]), a body that scrolls on its own, and the footer
/// pinned at the bottom. Sized to the settings body ([`theme::settings_detail_height`]), which is
/// what keeps the footer in view.
pub fn detail_page(
    id: &str,
    crumbs: &[&str],
    on_back: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    banner: Option<AnyElement>,
    body: Vec<AnyElement>,
    footer: AnyElement,
) -> AnyElement {
    let last = crumbs.len().saturating_sub(1);
    let trail = crumbs.iter().enumerate().flat_map(|(ix, crumb)| {
        let text = div()
            .text_size(theme::font(Family::Chrome, Role::Body))
            .text_color(if ix == last {
                theme::text()
            } else {
                theme::text_muted()
            })
            .when(ix == last, |this| this.font_weight(FontWeight::SEMIBOLD))
            .child(SharedString::from(crumb.to_string()))
            .into_any_element();
        let sep = (ix < last).then(|| mono("\u{203a}", theme::text_faint()).into_any_element());
        std::iter::once(text).chain(sep)
    });
    div()
        .w(relative(1.))
        .h(px(theme::settings_detail_height()))
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(34.))
                .flex()
                .flex_none()
                .items_center()
                .gap_2()
                .border_b_1()
                .border_color(theme::border())
                .child(
                    crate::ui::kit::icon_button(
                        ElementId::Name(format!("{id}-back").into()),
                        IconName::ArrowLeft,
                        false,
                        on_back,
                    )
                    .tooltip(|window, cx| {
                        gpui_component::tooltip::Tooltip::new("Back").build(window, cx)
                    }),
                )
                .children(trail),
        )
        .children(banner.map(|banner| div().flex_none().pt_2().child(banner)))
        .child(
            div()
                .id(ElementId::Name(format!("{id}-body").into()))
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .gap_4()
                .py_3()
                .pr_2()
                .overflow_y_scroll()
                .children(body),
        )
        .child(
            div()
                .flex_none()
                .pt_2()
                .border_t_1()
                .border_color(theme::border())
                .child(footer),
        )
        .into_any_element()
}

/// A detail page's footer: the destructive control at the left (a [`delete_control`], or
/// nothing for a new record), Cancel and Save at the right. Save is drawn faint while `ready` is
/// false; the caller still refuses the click.
pub fn detail_footer(
    id: &str,
    ready: bool,
    delete: Option<AnyElement>,
    on_cancel: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    on_save: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .children(delete)
        .child(div().flex_1())
        .child(crate::ui::kit::ghost_button(
            ElementId::Name(format!("{id}-cancel").into()),
            Some(IconName::Close),
            "Cancel",
            on_cancel,
        ))
        .child(
            crate::ui::kit::primary_button(
                ElementId::Name(format!("{id}-save").into()),
                Some(IconName::Check),
                "Save",
                on_save,
            )
            .when(!ready, |button| button.opacity(0.5)),
        )
        .into_any_element()
}

/// The footer's Delete, confirmed in place: a danger-coloured Delete, which turns into
/// [`inline_confirm`]'s "Delete …? / Delete / Keep" while `confirming`.
pub fn delete_control(
    id: &str,
    what: &str,
    confirming: bool,
    on_ask: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    on_confirm: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    on_keep: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    if confirming {
        return inline_confirm(
            &format!("{id}-delete"),
            &format!("Delete {what}?"),
            "Delete",
            "Keep",
            true,
            on_confirm,
            on_keep,
        );
    }
    crate::ui::kit::ghost_button(
        ElementId::Name(format!("{id}-delete").into()),
        Some(IconName::Delete),
        "Delete",
        on_ask,
    )
    .text_color(theme::danger())
    .into_any_element()
}

/// A question answered where it was asked, not in a modal: a sentence and two buttons — the
/// action (danger-coloured when `danger`) and a way back. The unsaved-changes banner and the
/// footer's delete confirm are both one.
pub fn inline_confirm(
    id: &str,
    text: &str,
    yes: &str,
    no: &str,
    danger: bool,
    on_yes: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    on_no: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let edge = if danger {
        theme::danger()
    } else {
        theme::warning()
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .pl_2()
        .border_l(px(theme::accent_edge()))
        .border_color(edge)
        .child(
            div()
                .text_size(theme::font(Family::Chrome, Role::Body))
                .text_color(theme::text())
                .child(SharedString::from(text.to_string())),
        )
        .child(
            crate::ui::kit::ghost_button(
                ElementId::Name(format!("{id}-yes").into()),
                None,
                SharedString::from(yes.to_string()),
                on_yes,
            )
            .text_color(edge),
        )
        .child(crate::ui::kit::ghost_button(
            ElementId::Name(format!("{id}-no").into()),
            None,
            SharedString::from(no.to_string()),
            on_no,
        ))
        .into_any_element()
}
