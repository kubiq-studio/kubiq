//! The policy manager: the Settings → Policies section, and the editor, import question, clone
//! prompt and removal question it raises.
//!
//! The list is grouped by origin — the origin decides what may be done with a row, so the group
//! says it once instead of every row saying it. The editor follows isol8's layer structure in the
//! order isol8 documents it, with the rarely used sections (home, rewrite, rules, macOS, Windows)
//! collapsed until asked for.
//!
//! The editor draws from [`crate::state::policy::PolicyEditor`] and nothing else; a policy that is
//! not local is shown as the TOML the host read, with Clone as the way to change it.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, ElementId, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, px,
};
use gpui_component::IconName;
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use kubiq_proto::policy::{PolicyInfo, PolicyOrigin};

use crate::app::AppState;
use crate::state::Layer;
use crate::state::policy::{
    ARCHES, FilterRows, MACOS_CAPABILITIES, OSES, PathRow, PolicyEditor, Section, Tick,
    WINDOWS_CAPABILITIES,
};
use crate::theme;
use crate::theme::{Family, Role};
use crate::ui::handler;
use crate::ui::kit::{
    RowAction, badge, check_box, choice_pill, confirm_modal, elided, field, ghost_button, heading,
    label_block, modal, modal_note, primary_button, prompt_modal, row_action, section_label,
};

// ── Shared furniture ────────────────────────────────────────────────

fn meta(text: impl Into<SharedString>, colour: gpui::Rgba) -> AnyElement {
    div()
        .text_size(theme::font(Family::Chrome, Role::Meta))
        .text_color(colour)
        .child(text.into())
        .into_any_element()
}

fn id(text: String) -> ElementId {
    ElementId::Name(text.into())
}

/// The host's last refusal, dismissible. The catalog's banner shape.
fn error_banner(error: &str, cx: &mut Context<AppState>) -> AnyElement {
    div()
        .px_3()
        .py_2()
        .flex()
        .items_center()
        .gap_2()
        .bg(theme::warning_soft())
        .border_l(px(theme::accent_edge()))
        .border_color(theme::warning())
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(theme::font(Family::Chrome, Role::Label))
                .text_color(theme::text())
                .child(SharedString::from(error.to_string())),
        )
        .child(ghost_button(
            "policy-error-dismiss",
            None,
            "Dismiss",
            cx.listener(|this, _, _, cx| this.dismiss_policy_error(cx)),
        ))
        .into_any_element()
}

/// One labelled single-line box.
fn line_box(label: &str, note: &str, input: &gpui::Entity<InputState>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(label_block(label, note))
        .child(
            field(theme::border(), false)
                .h(px(30.))
                .px_2()
                .child(Input::new(input).appearance(false)),
        )
        .into_any_element()
}

/// One labelled multi-line box.
fn area_box(label: &str, note: &str, input: &gpui::Entity<TextareaState>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(label_block(label, note))
        .child(area(input))
        .into_any_element()
}

fn area(input: &gpui::Entity<TextareaState>) -> AnyElement {
    field(theme::border(), false)
        .p_2()
        .w_full()
        .child(
            Textarea::new(input)
                .appearance(false)
                .bordered(false)
                .w_full()
                .text_size(theme::font(Family::Chrome, Role::Body)),
        )
        .into_any_element()
}

/// A wrap of pills, one per option, lit where `chosen` holds it.
fn tick_pills(
    prefix: &str,
    options: &[&str],
    chosen: &[String],
    tick: Tick,
    cx: &mut Context<AppState>,
) -> AnyElement {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .children(options.iter().map(|option| {
            let value = option.to_string();
            choice_pill(
                id(format!("{prefix}-{option}")),
                *option,
                chosen.iter().any(|it| it == option),
                cx.listener(move |this, _, _, cx| this.policy_tick(tick, value.clone(), cx)),
            )
            .into_any_element()
        }))
        .into_any_element()
}

fn origin_label(origin: &PolicyOrigin) -> (&'static str, gpui::Rgba) {
    match origin {
        PolicyOrigin::Builtin => ("Built-in", theme::text_faint()),
        PolicyOrigin::Local => ("Local", theme::accent()),
        PolicyOrigin::Imported { .. } => ("Imported", theme::info()),
        PolicyOrigin::User => ("isol8 user", theme::text_muted()),
    }
}

/// Which origins a list group holds.
type OriginTest = fn(&PolicyOrigin) -> bool;

// ── The section ─────────────────────────────────────────────────────

/// Settings → Policies.
pub fn section(app: &AppState, cx: &mut Context<AppState>) -> AnyElement {
    let state = &app.workbench.policy;
    let mut rows: Vec<AnyElement> = vec![heading(
        "Policies",
        "The sandbox rules an isolated agent runs under. Defaults apply to every isolated run; \
         the rest are picked per agent definition or per start.",
    )];
    if let Some(error) = state.error.as_deref()
        && state.editor.is_none()
        && state.import.is_none()
    {
        rows.push(error_banner(error, cx));
    }
    rows.push(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .child(ghost_button(
                "policy-new",
                Some(IconName::Plus),
                "New policy",
                cx.listener(|this, _, window, cx| this.open_policy_editor_new(window, cx)),
            ))
            .child(ghost_button(
                "policy-import",
                Some(IconName::ArrowDown),
                "Import\u{2026}",
                cx.listener(|this, _, window, cx| this.open_policy_import(window, cx)),
            ))
            .into_any_element(),
    );

    if state.policies.is_empty() {
        rows.push(meta("No policies yet.", theme::text_faint()));
    }
    let groups: [(&str, OriginTest); 4] = [
        ("Local", |origin| matches!(origin, PolicyOrigin::Local)),
        ("Imported", |origin| {
            matches!(origin, PolicyOrigin::Imported { .. })
        }),
        ("isol8 user", |origin| matches!(origin, PolicyOrigin::User)),
        ("Built-in", |origin| matches!(origin, PolicyOrigin::Builtin)),
    ];
    for (label, wanted) in groups {
        let members: Vec<&PolicyInfo> = state
            .policies
            .iter()
            .filter(|policy| wanted(&policy.origin))
            .collect();
        if members.is_empty() {
            continue;
        }
        rows.push(
            div()
                .pt_2()
                .child(section_label(&format!("{label} \u{b7} {}", members.len())))
                .into_any_element(),
        );
        rows.push(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .children(members.into_iter().map(|policy| policy_row(policy, cx)))
                .into_any_element(),
        );
    }

    div()
        .flex()
        .flex_col()
        .gap_3()
        .w_full()
        .children(rows)
        .into_any_element()
}

fn policy_row(policy: &PolicyInfo, cx: &mut Context<AppState>) -> AnyElement {
    let name = policy.name.clone();
    let (label, colour) = origin_label(&policy.origin);
    let editable = policy.origin.editable();
    let default = policy.default;

    let mut actions = div().flex().flex_none().items_center().gap_1();
    actions = actions.child({
        let name = name.clone();
        let action = row_action(
            id(format!("policy-{name}-edit")),
            RowAction::Edit,
            cx.listener(move |this, _, _, cx| this.open_policy_editor(name.clone(), cx)),
        );
        match editable {
            true => action,
            false => action.tip("View"),
        }
    });
    actions = actions.child({
        let name = name.clone();
        row_action(
            id(format!("policy-{name}-clone")),
            RowAction::Clone,
            cx.listener(move |this, _, window, cx| {
                this.open_policy_clone(name.clone(), window, cx)
            }),
        )
    });
    if policy.origin.deletable() {
        let name = name.clone();
        actions = actions.child(row_action(
            id(format!("policy-{name}-delete")),
            RowAction::Delete,
            cx.listener(move |this, _, _, cx| this.ask_delete_policy(name.clone(), cx)),
        ));
    }

    let default_toggle = {
        let (name, toggled) = (name.clone(), name.clone());
        div()
            .id(id(format!("policy-{name}-default-row")))
            .flex()
            .flex_none()
            .items_center()
            .gap_1p5()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_policy_default(toggled.clone(), !default, cx)
            }))
            .child(check_box(
                id(format!("policy-{name}-default")),
                default,
                |_, _, _| {},
            ))
            .child(meta("Default", theme::text_muted()))
    };

    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .py_1()
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .gap_0p5()
                .min_w(px(0.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .min_w(px(0.))
                        .child(
                            div()
                                .text_size(theme::font(Family::Chrome, Role::Body))
                                .text_color(theme::text())
                                .child(SharedString::from(name.clone())),
                        )
                        .child(badge(label, colour)),
                )
                .when(!policy.description.is_empty(), |this| {
                    this.child(elided(
                        id(format!("policy-{name}-note")),
                        policy.description.clone(),
                        theme::text_muted(),
                        theme::font(Family::Chrome, Role::Meta),
                    ))
                }),
        )
        .child(default_toggle)
        .child(actions)
        .into_any_element()
}

// ── The removal and clone questions ─────────────────────────────────

/// "Delete this policy?" over the page.
pub fn delete_confirm(
    app: &AppState,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let Some(name) = app.workbench.policy.confirm_delete.clone() else {
        return div().into_any_element();
    };
    let view = cx.entity();
    confirm_modal(
        "policy-delete",
        "Delete policy",
        &format!("Delete \u{201c}{name}\u{201d}? Agent definitions that name it stop applying it."),
        "Delete",
        true,
        handler(&view, |this, _, cx| this.confirm_delete_policy(cx)),
        crate::ui::dismiss(&view, Layer::PolicyDelete, |this, _, cx| {
            this.cancel_delete_policy(cx)
        }),
        window,
    )
}

/// "What should the copy be called?"
pub fn clone_prompt(app: &AppState, window: &mut Window, cx: &mut Context<AppState>) -> AnyElement {
    let Some(ask) = app.workbench.policy.clone_as.as_ref() else {
        return div().into_any_element();
    };
    let view = cx.entity();
    let named = !ask.name.read(cx).value().trim().is_empty();
    prompt_modal(
        "policy-clone",
        "Clone policy",
        Some(&format!(
            "A local copy of \u{201c}{}\u{201d} you can edit. A name with a slash files it in a folder.",
            ask.source
        )),
        "Name",
        &ask.name,
        "Clone",
        named,
        handler(&view, |this, _, cx| this.run_policy_clone(cx)),
        crate::ui::dismiss(&view, Layer::PolicyClone, |this, _, cx| {
            this.close_policy_clone(cx)
        }),
        window,
        cx,
    )
}

// ── Import ──────────────────────────────────────────────────────────

/// The import question: a repository, an optional ref, and what came of it.
pub fn import_modal(app: &AppState, window: &mut Window, cx: &mut Context<AppState>) -> AnyElement {
    let state = &app.workbench.policy;
    let Some(import) = state.import.as_ref() else {
        return div().into_any_element();
    };
    let view = cx.entity();
    let ready = !import.busy && !import.url.read(cx).value().trim().is_empty();

    let mut body = div()
        .flex()
        .flex_col()
        .gap_3()
        .pt_3()
        .child(modal_note(
            "Copies every policy layer in a git repository into Kubiq, read-only, under \
             imported/. Importing it again replaces them.",
        ))
        .child(line_box("Repository", "A git clone URL.", &import.url))
        .child(line_box(
            "Branch or tag",
            "Optional \u{2014} the remote\u{2019}s default when empty.",
            &import.git_ref,
        ));
    if import.busy {
        body = body.child(meta("Importing\u{2026}", theme::text_faint()));
    }
    if let Some(error) = state.error.as_deref() {
        body = body.child(meta(error.to_string(), theme::danger()));
    }
    if let Some(names) = import.result.as_ref() {
        body = body.child(meta(
            match names.len() {
                0 => "No policies found in that repository.".to_string(),
                n => format!("Imported {n} policies: {}", names.join(", ")),
            },
            theme::success(),
        ));
    }

    let footer = div()
        .flex()
        .items_center()
        .gap_2()
        .child(ghost_button(
            "policy-import-close",
            None,
            if import.result.is_some() {
                "Done"
            } else {
                "Cancel"
            },
            cx.listener(|this, _, _, cx| this.close_policy_import(cx)),
        ))
        .child(
            primary_button(
                "policy-import-run",
                None,
                "Import",
                cx.listener(|this, _, _, cx| this.run_policy_import(cx)),
            )
            .when(!ready, |button| button.opacity(0.5)),
        )
        .into_any_element();

    modal(
        "policy-import-modal",
        theme::accent(),
        "Import policies",
        body.into_any_element(),
        footer,
        crate::ui::dismiss(&view, Layer::PolicyImport, |this, _, cx| {
            this.close_policy_import(cx)
        }),
        window,
    )
}

// ── The editor ──────────────────────────────────────────────────────

/// The editor modal: a form for a local policy, the TOML for any other.
pub fn editor_modal(app: &AppState, window: &mut Window, cx: &mut Context<AppState>) -> AnyElement {
    let state = &app.workbench.policy;
    let Some(editor) = state.editor.as_ref() else {
        return div().into_any_element();
    };
    let view = cx.entity();
    let dismiss = crate::ui::dismiss(&view, Layer::PolicyEditor, |this, _, cx| {
        this.close_policy_editor(cx)
    });

    if editor.readonly() {
        return readonly_modal(editor, cx, dismiss, window);
    }

    let mut body = div().flex().flex_col().gap_3().pt_3();
    if let Some(error) = state.error.as_deref() {
        body = body.child(meta(error.to_string(), theme::danger()));
    }
    body = body
        .child(line_box(
            "Name",
            "Its path in the policy folder; a slash files it in a subfolder.",
            &editor.name,
        ))
        .child(line_box(
            "Description",
            "What it is for, shown beside it in the lists.",
            &editor.description,
        ))
        .child(requires_block(app, editor, cx))
        .child(filter_block(
            "policy-filter",
            None,
            "Filter",
            &editor.filter,
            cx,
        ))
        .child(paths_block(
            "policy-paths",
            None,
            "Paths",
            "Filesystem grants. A path is absolute, ~-prefixed or contains #HOME.",
            &editor.paths,
            cx,
        ))
        .child(env_block(editor, cx))
        .child(advanced(editor, cx));

    let ready = editor.ready(cx);
    let footer = div()
        .flex()
        .items_center()
        .gap_2()
        .child(ghost_button(
            "policy-editor-cancel",
            None,
            "Cancel",
            cx.listener(|this, _, _, cx| this.close_policy_editor(cx)),
        ))
        .child(
            primary_button(
                "policy-editor-save",
                None,
                if editor.saving {
                    "Saving\u{2026}"
                } else {
                    "Save"
                },
                cx.listener(|this, _, _, cx| this.save_policy_editor(cx)),
            )
            .when(!ready, |button| button.opacity(0.5)),
        )
        .into_any_element();

    modal(
        "policy-editor",
        theme::accent(),
        if editor.replaces.is_some() {
            "Edit policy"
        } else {
            "New policy"
        },
        body.into_any_element(),
        footer,
        dismiss,
        window,
    )
}

/// A policy that is not the user's own: its facts, the TOML, and Clone.
fn readonly_modal(
    editor: &PolicyEditor,
    cx: &mut Context<AppState>,
    dismiss: impl Fn(&mut Window, &mut gpui::App) + 'static,
    window: &Window,
) -> AnyElement {
    let name = editor.name_text(cx);
    let description = editor.description.read(cx).value().to_string();
    let (label, colour) = origin_label(&editor.origin);
    let source = match &editor.origin {
        PolicyOrigin::Imported { source } => Some(source.clone()),
        _ => None,
    };

    let body = div()
        .flex()
        .flex_col()
        .gap_3()
        .pt_3()
        .child(modal_note(
            "Read-only. Clone it to make a local copy you can change.",
        ))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_size(theme::font(Family::Chrome, Role::Body))
                        .text_color(theme::text())
                        .child(SharedString::from(name.clone())),
                )
                .child(badge(label, colour)),
        )
        .when(!description.is_empty(), |this| {
            this.child(meta(description.clone(), theme::text_muted()))
        })
        .when_some(source, |this, source| {
            this.child(meta(format!("From {source}"), theme::text_faint()))
        })
        .child(
            div()
                .id("policy-toml")
                .p_2()
                .max_h(px(360.))
                .overflow_y_scroll()
                .bg(theme::surface())
                .child(
                    div()
                        .font_family(theme::MONO_FONT)
                        .text_size(theme::font(Family::Chrome, Role::Meta))
                        .text_color(theme::text_muted())
                        .whitespace_normal()
                        .child(SharedString::from(editor.toml.clone())),
                ),
        )
        .into_any_element();

    let footer = div()
        .flex()
        .items_center()
        .gap_2()
        .child(ghost_button(
            "policy-editor-close",
            None,
            "Close",
            cx.listener(|this, _, _, cx| this.close_policy_editor(cx)),
        ))
        .child(primary_button(
            "policy-editor-clone",
            None,
            "Clone",
            cx.listener(move |this, _, window, cx| {
                this.open_policy_clone(name.clone(), window, cx)
            }),
        ))
        .into_any_element();

    modal(
        "policy-editor",
        theme::accent(),
        "Policy",
        body,
        footer,
        dismiss,
        window,
    )
}

/// `requires`: the policies this one builds on, ticked from the known names.
fn requires_block(app: &AppState, editor: &PolicyEditor, cx: &mut Context<AppState>) -> AnyElement {
    let own = editor.name_text(cx);
    let names: Vec<String> = app
        .workbench
        .policy
        .policies
        .iter()
        .map(|policy| policy.name.clone())
        .filter(|name| *name != own)
        .collect();
    let options: Vec<&str> = names.iter().map(String::as_str).collect();
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(label_block("Requires", "Policies applied before this one."))
        .child(
            div()
                .id("policy-requires-scroll")
                .max_h(px(120.))
                .overflow_y_scroll()
                .child(tick_pills(
                    "policy-requires",
                    &options,
                    &editor.requires,
                    Tick::Requires,
                    cx,
                )),
        )
        .into_any_element()
}

/// An `os`/`arch`/`executables` gate, for the layer (`rule: None`) or one conditional rule.
fn filter_block(
    prefix: &str,
    rule: Option<usize>,
    title: &str,
    filter: &FilterRows,
    cx: &mut Context<AppState>,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(label_block(
            title,
            "Applies only where all of these hold. Nothing ticked is no constraint.",
        ))
        .child(tick_pills(
            &format!("{prefix}-os"),
            &OSES,
            &filter.os,
            Tick::Os(rule),
            cx,
        ))
        .child(tick_pills(
            &format!("{prefix}-arch"),
            &ARCHES,
            &filter.arch,
            Tick::Arch(rule),
            cx,
        ))
        .child(area_box(
            "Executables",
            "One command name per line; makes the policy selectable by the command it wraps.",
            &filter.executables,
        ))
        .into_any_element()
}

/// Grant rows: path, access, match, remove — and Add.
fn paths_block(
    prefix: &str,
    rule: Option<usize>,
    title: &str,
    note: &str,
    paths: &[PathRow],
    cx: &mut Context<AppState>,
) -> AnyElement {
    let rows = paths.iter().enumerate().map(|(index, row)| {
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                field(theme::border(), false)
                    .h(px(30.))
                    .flex_1()
                    .min_w(px(0.))
                    .px_2()
                    .child(Input::new(&row.path).appearance(false)),
            )
            .child(
                ghost_button(
                    id(format!("{prefix}-{index}-access")),
                    None,
                    row.access.clone(),
                    cx.listener(move |this, _, _, cx| {
                        this.policy_cycle_path(rule, index, false, cx)
                    }),
                )
                .w(px(76.))
                .tooltip(|window, cx| {
                    gpui_component::tooltip::Tooltip::new("Access: ro, rw, none or metadata")
                        .build(window, cx)
                }),
            )
            .child(
                ghost_button(
                    id(format!("{prefix}-{index}-match")),
                    None,
                    row.matching.clone(),
                    cx.listener(move |this, _, _, cx| {
                        this.policy_cycle_path(rule, index, true, cx)
                    }),
                )
                .w(px(76.))
                .tooltip(|window, cx| {
                    gpui_component::tooltip::Tooltip::new(
                        "Match: subpath, literal, prefix or regex",
                    )
                    .build(window, cx)
                }),
            )
            .child(
                row_action(
                    id(format!("{prefix}-{index}-remove")),
                    RowAction::Delete,
                    cx.listener(move |this, _, _, cx| this.policy_remove_path(rule, index, cx)),
                )
                .tip("Remove"),
            )
            .into_any_element()
    });
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(label_block(title, note))
        .children(rows)
        .child(div().child(ghost_button(
            id(format!("{prefix}-add")),
            Some(IconName::Plus),
            "Add path",
            cx.listener(move |this, _, window, cx| this.policy_add_path(rule, window, cx)),
        )))
        .into_any_element()
}

/// Environment defaults: name and value rows.
fn env_block(editor: &PolicyEditor, cx: &mut Context<AppState>) -> AnyElement {
    let rows = editor.env.iter().enumerate().map(|(index, row)| {
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                field(theme::border(), false)
                    .h(px(30.))
                    .w(px(160.))
                    .px_2()
                    .child(Input::new(&row.key).appearance(false)),
            )
            .child(
                field(theme::border(), false)
                    .h(px(30.))
                    .flex_1()
                    .min_w(px(0.))
                    .px_2()
                    .child(Input::new(&row.value).appearance(false)),
            )
            .child(
                row_action(
                    id(format!("policy-env-{index}-remove")),
                    RowAction::Delete,
                    cx.listener(move |this, _, _, cx| this.policy_remove_env(index, cx)),
                )
                .tip("Remove"),
            )
            .into_any_element()
    });
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(label_block(
            "Environment",
            "Variables set for the agent unless it already has them.",
        ))
        .children(rows)
        .child(div().child(ghost_button(
            "policy-env-add",
            Some(IconName::Plus),
            "Add variable",
            cx.listener(|this, _, window, cx| this.policy_add_env(window, cx)),
        )))
        .into_any_element()
}

/// The collapsed sections: home, rewrite, rules, macOS, Windows.
fn advanced(editor: &PolicyEditor, cx: &mut Context<AppState>) -> AnyElement {
    let head = |section: Section, title: &str, cx: &mut Context<AppState>| -> AnyElement {
        let open = editor.is_open(section);
        div()
            .pt_2()
            .child(ghost_button(
                id(format!("policy-section-{section:?}")),
                Some(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                }),
                title.to_string(),
                cx.listener(move |this, _, _, cx| this.policy_toggle_section(section, cx)),
            ))
            .into_any_element()
    };

    let mut out = div().flex().flex_col().gap_2();

    out = out.child(head(Section::Home, "Home replacement", cx));
    if editor.is_open(Section::Home) {
        out = out
            .child(check_line(
                "policy-home-enabled",
                "Replace the agent\u{2019}s home",
                editor.home_enabled,
                cx.listener(|this, _, _, cx| this.policy_toggle_home(false, cx)),
            ))
            .child(check_line(
                "policy-home-scratch",
                "Use a throwaway home",
                editor.home_scratch,
                cx.listener(|this, _, _, cx| this.policy_toggle_home(true, cx)),
            ))
            .child(line_box(
                "Home path",
                "Optional \u{2014} a fixed replacement home.",
                &editor.home_path,
            ))
            .child(area_box(
                "Seed",
                "Real-home entries copied read-only into the replacement, one per line.",
                &editor.home_seed,
            ));
    }

    out = out.child(head(Section::Rewrite, "Rewrite", cx));
    if editor.is_open(Section::Rewrite) {
        out = out.child(area_box(
            "Ensure arguments",
            "Inserted after the command when absent, one per line.",
            &editor.ensure_args,
        ));
    }

    out = out.child(head(
        Section::Rules,
        &format!("Conditional rules \u{b7} {}", editor.rules.len()),
        cx,
    ));
    if editor.is_open(Section::Rules) {
        for (index, rule) in editor.rules.iter().enumerate() {
            out = out.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .bg(theme::surface())
                    .border_l(px(theme::accent_edge()))
                    .border_color(theme::border())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(section_label(&format!("Rule {}", index + 1)))
                            .child(
                                row_action(
                                    id(format!("policy-rule-{index}-remove")),
                                    RowAction::Delete,
                                    cx.listener(move |this, _, _, cx| {
                                        this.policy_remove_rule(index, cx)
                                    }),
                                )
                                .tip("Remove rule"),
                            ),
                    )
                    .child(filter_block(
                        &format!("policy-rule-{index}-filter"),
                        Some(index),
                        "When",
                        &rule.filter,
                        cx,
                    ))
                    .child(paths_block(
                        &format!("policy-rule-{index}-paths"),
                        Some(index),
                        "Grants",
                        "Added when the filter holds.",
                        &rule.paths,
                        cx,
                    )),
            );
        }
        out = out.child(div().child(ghost_button(
            "policy-rule-add",
            Some(IconName::Plus),
            "Add rule",
            cx.listener(|this, _, window, cx| this.policy_add_rule(window, cx)),
        )));
    }

    out = out.child(head(Section::Macos, "macOS", cx));
    if editor.is_open(Section::Macos) {
        out = out
            .child(label_block(
                "Capabilities",
                "Seatbelt operation classes with no Linux equivalent.",
            ))
            .child(tick_pills(
                "policy-macos",
                &MACOS_CAPABILITIES,
                &editor.macos_capabilities,
                Tick::MacosCapability,
                cx,
            ))
            .child(area_box(
                "Raw SBPL",
                "Appended verbatim to the sandbox profile.",
                &editor.macos_raw,
            ));
    }

    out = out.child(head(Section::Windows, "Windows", cx));
    if editor.is_open(Section::Windows) {
        out = out
            .child(label_block(
                "Capabilities",
                "Reserved; parsed but not applied at spawn yet.",
            ))
            .child(tick_pills(
                "policy-windows",
                &WINDOWS_CAPABILITIES,
                &editor.windows_capabilities,
                Tick::WindowsCapability,
                cx,
            ));
    }

    out.into_any_element()
}

/// A check box and its words, the whole line clickable.
fn check_line(
    key: &'static str,
    label: &str,
    checked: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    div()
        .id(key)
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .on_click(on_click)
        .child(check_box(id(format!("{key}-box")), checked, |_, _, _| {}))
        .child(
            div()
                .text_size(theme::font(Family::Chrome, Role::Body))
                .text_color(theme::text())
                .child(SharedString::from(label.to_string())),
        )
        .into_any_element()
}
