//! The "Configure drone mode" dialog, raised from the remote menu (`D232`).
//!
//! One modal with two faces: the list of this process's drone modes — each with its switch, its
//! live status, its connect string — and, after Add or Edit, the form for one. A saved change is
//! `SetSettings` (`app::drone_modes`); the status line is whatever the host last said.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, ElementId, Entity, Focusable, IntoElement, ParentElement, SharedString,
    Styled, Window, div, px,
};
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use kubiq_proto::settings::{DroneMode, DroneModeKind, DroneModeState};
use serde_json::Value;

use crate::app::AppState;
use crate::ext::drone::{DroneField, DroneFieldInput, DroneKindSpec};
use crate::state::drone::{BindChoice, DroneEdit, EditBody, ExtBox};
use crate::theme::{self, Family, Role};
use crate::ui::kit::{
    RowAction, badge, check_box, elided, field, ghost_button, label_block, modal_note,
    modal_sized_pinned, mono, primary_button, row_action, toggle_pill,
};

pub fn render(app: &AppState, window: &mut Window, cx: &mut Context<AppState>) -> AnyElement {
    let drone = &app.workbench.drone_modes;
    let (title, body, footer) = match &drone.edit {
        Some(edit) => (
            if edit.is_new {
                "Add a drone mode"
            } else {
                "Edit drone mode"
            },
            form_body(app, edit, window, cx),
            form_footer(cx),
        ),
        None => ("Configure drone mode", list_body(app, cx), list_footer(cx)),
    };
    modal_sized_pinned(
        "drone-modes-modal",
        theme::accent(),
        theme::modal_width(),
        None,
        title,
        None,
        body,
        footer,
        crate::ui::dismiss(
            &cx.entity(),
            crate::state::Layer::DroneModes,
            |this, _, cx| this.close_drone_modes(cx),
        ),
        window,
    )
}

fn name(prefix: &str, id: &str, what: &str) -> ElementId {
    ElementId::Name(format!("{prefix}-{id}-{what}").into())
}

// ── the list ────────────────────────────────────────────────────────

fn list_body(app: &AppState, cx: &mut Context<AppState>) -> AnyElement {
    let modes = &app.workbench.settings.host.drone_modes;
    let mut body = div().flex().flex_col().gap_3().pt_3().child(modal_note(
        "A drone mode lets another Kubiq reach this one while it runs. Whoever holds a mode's \
         connect string has this workbench.",
    ));
    if modes.is_empty() {
        body = body.child(modal_note("No drone modes yet. Add one below."));
    }
    for mode in modes {
        body = body.child(mode_row(app, mode, cx));
    }
    body.into_any_element()
}

/// What a mode's kind is called on its line.
fn kind_label(kind: &DroneModeKind) -> String {
    match kind {
        DroneModeKind::Http { .. } => "HTTP".to_string(),
        DroneModeKind::Ext { kind, .. } => crate::ext::drone::kind_named(kind)
            .map(|spec| spec.label.to_string())
            .unwrap_or_else(|| format!("{kind} (not in this edition)")),
    }
}

fn mode_row(app: &AppState, mode: &DroneMode, cx: &mut Context<AppState>) -> AnyElement {
    let status = app.workbench.drone_modes.status.get(&mode.id);
    let (chip, colour) = match status {
        Some(DroneModeState::Serving { .. }) => ("serving", theme::success()),
        Some(DroneModeState::Starting) => ("starting", theme::warning()),
        Some(DroneModeState::Failed { .. }) => ("failed", theme::danger()),
        Some(DroneModeState::Stopped) | None => ("stopped", theme::text_faint()),
    };
    let editable = match &mode.kind {
        DroneModeKind::Http { .. } => true,
        DroneModeKind::Ext { kind, .. } => crate::ext::drone::kind_named(kind).is_some(),
    };
    let (toggle_id, edit_id, remove_id) = (mode.id.clone(), mode.id.clone(), mode.id.clone());

    let head = div()
        .flex()
        .items_center()
        .gap_2()
        .child(check_box(
            name("drone-mode", &mode.id, "enabled"),
            mode.enabled,
            cx.listener(move |this, _, _, cx| this.toggle_drone_mode(&toggle_id, cx)),
        ))
        .child(div().flex_1().min_w(px(0.)).child(elided(
            name("drone-mode", &mode.id, "name"),
            SharedString::from(mode.name.clone()),
            theme::text(),
            theme::font(Family::Chrome, Role::Body),
        )))
        .child(badge(&kind_label(&mode.kind), theme::text_muted()))
        .child(badge(chip, colour))
        .child(
            row_action(
                name("drone-mode", &mode.id, "edit"),
                RowAction::Edit,
                cx.listener(move |this, _, window, cx| {
                    this.begin_edit_drone_mode(&edit_id, window, cx)
                }),
            )
            .enabled(editable),
        )
        .child(
            row_action(
                name("drone-mode", &mode.id, "remove"),
                RowAction::Delete,
                cx.listener(move |this, _, _, cx| this.remove_drone_mode(&remove_id, cx)),
            )
            .tip("Remove"),
        );

    let mut row = div()
        .w_full()
        .py_2()
        .flex()
        .flex_col()
        .gap_1()
        .border_b_1()
        .border_color(theme::border())
        .child(head);

    match status {
        Some(DroneModeState::Serving {
            addr,
            connect_string,
            clients,
        }) => {
            let plural = if *clients == 1 { "client" } else { "clients" };
            let (copy_id, regen_id) = (mode.id.clone(), mode.id.clone());
            let is_http = matches!(mode.kind, DroneModeKind::Http { .. });
            row = row
                .child(note(&format!(
                    "Serving on {addr} \u{b7} {clients} {plural}"
                )))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .font_family(theme::MONO_FONT)
                                .child(elided(
                                    name("drone-mode", &mode.id, "connect"),
                                    SharedString::from(connect_string.clone()),
                                    theme::text_muted(),
                                    theme::font(Family::Chrome, Role::Meta),
                                )),
                        )
                        .child(ghost_button(
                            name("drone-mode", &mode.id, "copy"),
                            None,
                            "Copy",
                            cx.listener(move |this, _, _, cx| {
                                this.copy_drone_connect_string(&copy_id, cx)
                            }),
                        ))
                        .when(is_http, |this| {
                            this.child(ghost_button(
                                name("drone-mode", &mode.id, "regenerate"),
                                None,
                                "Regenerate token",
                                cx.listener(move |this, _, _, _| {
                                    this.regenerate_drone_token(&regen_id)
                                }),
                            ))
                        }),
                );
        }
        Some(DroneModeState::Failed { reason }) => {
            let restart_id = mode.id.clone();
            row = row.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(theme::font(Family::Chrome, Role::Meta))
                            .text_color(theme::danger())
                            .child(SharedString::from(reason.clone())),
                    )
                    .child(ghost_button(
                        name("drone-mode", &mode.id, "restart"),
                        None,
                        "Restart",
                        cx.listener(move |this, _, _, _| this.restart_drone_mode(&restart_id)),
                    )),
            );
        }
        _ => {}
    }
    row.into_any_element()
}

fn note(text: &str) -> impl IntoElement {
    div()
        .text_size(theme::font(Family::Chrome, Role::Meta))
        .text_color(theme::text_muted())
        .child(SharedString::from(text.to_string()))
}

fn list_footer(cx: &mut Context<AppState>) -> AnyElement {
    let mut footer = div()
        .flex()
        .items_center()
        .gap_2()
        .child(mono("Add", theme::text_faint()))
        .child(ghost_button(
            "drone-add-http",
            None,
            "HTTP",
            cx.listener(|this, _, window, cx| this.begin_add_drone_mode(None, window, cx)),
        ));
    for spec in crate::ext::drone::kinds() {
        let kind = spec.kind;
        footer = footer.child(ghost_button(
            ElementId::Name(format!("drone-add-{kind}").into()),
            None,
            spec.label,
            cx.listener(move |this, _, window, cx| {
                this.begin_add_drone_mode(Some(kind), window, cx)
            }),
        ));
    }
    footer
        .child(div().flex_1())
        .child(primary_button(
            "drone-modes-close",
            None,
            "Close",
            cx.listener(|this, _, _, cx| this.close_drone_modes(cx)),
        ))
        .into_any_element()
}

// ── the form ────────────────────────────────────────────────────────

fn form_footer(cx: &mut Context<AppState>) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(ghost_button(
            "drone-form-cancel",
            None,
            "Cancel",
            cx.listener(|this, _, _, cx| this.cancel_drone_edit(cx)),
        ))
        .child(primary_button(
            "drone-form-save",
            None,
            "Save",
            cx.listener(|this, _, _, cx| this.save_drone_edit(cx)),
        ))
        .into_any_element()
}

/// A labelled one-line box.
fn input_field(
    label: &str,
    note: &str,
    input: &Entity<InputState>,
    window: &Window,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let focused = input.read(cx).focus_handle(cx).is_focused(window);
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(label_block(label, note))
        .child(
            field(theme::border(), focused)
                .h(px(30.))
                .px_2()
                .child(Input::new(input).appearance(false)),
        )
        .into_any_element()
}

fn area_field(
    label: &str,
    note: &str,
    area: &Entity<TextareaState>,
    window: &Window,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let focused = area.read(cx).focus_handle(cx).is_focused(window);
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(label_block(label, note))
        .child(
            field(theme::border(), focused).px_2().py_1().child(
                Textarea::new(area)
                    .appearance(false)
                    .bordered(false)
                    .w_full()
                    .text_size(theme::font(Family::Chrome, Role::Body)),
            ),
        )
        .into_any_element()
}

fn form_body(
    app: &AppState,
    edit: &DroneEdit,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let mut body = div().flex().flex_col().gap_3().pt_3().child(input_field(
        "Name",
        "What this mode is called in the list.",
        &edit.name,
        window,
        cx,
    ));
    body = match &edit.body {
        EditBody::Http {
            bind,
            bind_custom,
            port,
            tls_cert,
            tls_key,
            tunnel_allow,
            ..
        } => body
            .child(bind_choice(*bind, cx))
            .when(*bind == BindChoice::Custom, |this| {
                this.child(input_field(
                    "Address",
                    "An IP address to bind.",
                    bind_custom,
                    window,
                    cx,
                ))
            })
            .child(input_field(
                "Port",
                "0 takes any free port. Default 7420.",
                port,
                window,
                cx,
            ))
            .child(input_field(
                "TLS certificate",
                "Optional. A PEM chain on this machine; with a key, the listener speaks https.",
                tls_cert,
                window,
                cx,
            ))
            .child(input_field(
                "TLS key",
                "A PEM private key, beside the certificate.",
                tls_key,
                window,
                cx,
            ))
            .child(input_field(
                "Tunnel allow-list",
                "Hosts a remote may tunnel to, separated by commas. Empty is this machine only; \
                 `none` turns tunnels off.",
                tunnel_allow,
                window,
                cx,
            ))
            .when(app.drone_form_exposed_without_tls(cx), |this| {
                this.child(
                    div()
                        .w_full()
                        .pl_2()
                        .border_l(px(theme::accent_edge()))
                        .border_color(theme::warning())
                        .text_size(theme::font(Family::Chrome, Role::Body))
                        .text_color(theme::warning())
                        .child(
                            "This listens on the network without TLS. The token is the only \
                             protection, and it travels in the clear.",
                        ),
                )
            }),
        EditBody::Ext {
            spec,
            config,
            inputs,
        } => body.child(modal_note(spec.description)).children(
            spec.fields
                .iter()
                .map(|field| ext_field(spec, field, config, inputs, window, cx)),
        ),
    };
    if let Some(error) = &edit.error {
        body = body.child(
            div()
                .text_size(theme::font(Family::Chrome, Role::Body))
                .text_color(theme::danger())
                .child(SharedString::from(error.clone())),
        );
    }
    body.into_any_element()
}

fn bind_choice(picked: BindChoice, cx: &mut Context<AppState>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(label_block("Listen on", "Who can reach this mode."))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(toggle_pill(
                    "drone-bind-loopback",
                    "This machine only (127.0.0.1)",
                    theme::accent(),
                    picked == BindChoice::Loopback,
                    cx.listener(|this, _, _, cx| this.set_drone_bind(BindChoice::Loopback, cx)),
                ))
                .child(toggle_pill(
                    "drone-bind-lan",
                    "Local network (0.0.0.0)",
                    theme::accent(),
                    picked == BindChoice::Lan,
                    cx.listener(|this, _, _, cx| this.set_drone_bind(BindChoice::Lan, cx)),
                ))
                .child(toggle_pill(
                    "drone-bind-custom",
                    "Custom address",
                    theme::accent(),
                    picked == BindChoice::Custom,
                    cx.listener(|this, _, _, cx| this.set_drone_bind(BindChoice::Custom, cx)),
                )),
        )
        .into_any_element()
}

/// One field of an edition kind's form, drawn from its declaration.
fn ext_field(
    _spec: &'static DroneKindSpec,
    field: &'static DroneField,
    config: &serde_json::Map<String, Value>,
    inputs: &[(&'static str, ExtBox)],
    window: &Window,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let key = field.key;
    match field.input {
        DroneFieldInput::Text { .. } | DroneFieldInput::Number { .. } => {
            match inputs.iter().find(|(held, _)| *held == key) {
                Some((_, ExtBox::Line(input))) => {
                    input_field(field.label, field.note, input, window, cx)
                }
                _ => div().into_any_element(),
            }
        }
        DroneFieldInput::Lines { .. } => match inputs.iter().find(|(held, _)| *held == key) {
            Some((_, ExtBox::Lines(area))) => area_field(field.label, field.note, area, window, cx),
            _ => div().into_any_element(),
        },
        DroneFieldInput::Bool => {
            let checked = config.get(key).and_then(Value::as_bool).unwrap_or(false);
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(check_box(
                    ElementId::Name(format!("drone-ext-{key}").into()),
                    checked,
                    cx.listener(move |this, _, _, cx| {
                        this.set_drone_ext_value(key, Value::Bool(!checked), cx)
                    }),
                ))
                .child(div().flex_1().child(label_block(field.label, field.note)))
                .into_any_element()
        }
        DroneFieldInput::Choice(options) => {
            let picked = config
                .get(key)
                .and_then(Value::as_str)
                .or(options.first().map(|(value, _)| *value));
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(label_block(field.label, field.note))
                .child(div().flex().flex_wrap().items_center().gap_2().children(
                    options.iter().map(|(value, label)| {
                        let value = *value;
                        toggle_pill(
                            ElementId::Name(format!("drone-ext-{key}-{value}").into()),
                            *label,
                            theme::accent(),
                            picked == Some(value),
                            cx.listener(move |this, _, _, cx| {
                                this.set_drone_ext_value(key, Value::from(value), cx)
                            }),
                        )
                    }),
                ))
                .into_any_element()
        }
    }
}
