//! The runbook viewer: a `*.runbook.md` drawn as prose and runnable cells (`D204` viewer seam,
//! registered by the base, claimed over the Markdown view with `Claim::Specific`).
//!
//! **The tab's buffer is the document.** The editor reads, saves and (with `live_reload`) re-reads
//! it when the host writes a run's output into it; Source is the buffer itself. This view parses
//! the buffer (pure, `kubiq_runbook`) and draws, per cell: its id, kind and derived state, what it
//! needs, the command after substitution (secrets as `•••`, selectable, zoomable), its controls,
//! an agent's pending request as a Run · Edit · Skip bar, a fixed-height live tail while it runs,
//! and the output blocks the host wrote, each selectable with a zoom and a full-output view. The
//! header carries the role, Run ready (n), the provider's actions, the parameters and the bound
//! agent. Keys: `ui::cells::keys` (Shift/Ctrl-Enter = approve and run, `I I` = Stop, F = zoom).
//!
//! Every run is a person's click on the command shown: the approval carries the command's hash,
//! and an edit (or an unsaved buffer) refuses it.

use std::rc::Rc;

use gpui::{
    AnyElement, AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::IconName;
use gpui_component::input::{Input, InputState};
use kubiq_proto::runbook::{CheckState, RunbookRequest, StepState};
use kubiq_runbook::format::BlockKind;
use kubiq_runbook::model::{CellKind, OUTPUT, Runbook};

use crate::app::AppState;
use crate::ext::viewer::{self, Claim, ViewerCtx, ViewerSpec};
use crate::ext::{Registry, ids};
use crate::state::editor::{FileLanguage, ViewLayout};
use crate::state::runbook::{Ask, Key, Tab, runbooks};
use crate::theme::{self, Family, Role};
use crate::ui::cells::{self, keys, output};
use crate::ui::{eid, eid2, handler, kit};

const LAYOUTS: &[ViewLayout] = &[ViewLayout::Preview, ViewLayout::Source];

pub fn register(reg: &mut Registry<ViewerSpec>) {
    viewer::register(
        reg,
        ViewerSpec {
            id: ids::RUNBOOK_VIEWER,
            label: "Runbook",
            claims,
            layouts: LAYOUTS,
            default_layout: ViewLayout::Preview,
            language: Some(FileLanguage::Markdown),
            live_reload: true,
            render,
            menu: None,
            header_left: None,
            header_right: None,
        },
    );
}

/// `*.runbook.md` opens here, over the Markdown view.
pub fn claims(path: &str, _head: Option<&str>) -> Claim {
    if kubiq_runbook::is_runbook(path) {
        Claim::Specific
    } else {
        Claim::No
    }
}

fn with_tab<R>(cx: &mut gpui::App, key: &Key, f: impl FnOnce(&mut Tab) -> R) -> Option<R> {
    runbooks(cx).tabs.get_mut(key).map(f)
}

/// A step state's colour and word. Status by colour and word together.
pub fn state_chip(state: StepState) -> (gpui::Rgba, &'static str) {
    let colour = match state {
        StepState::Waiting => theme::text_faint(),
        StepState::Ready | StepState::Running => theme::info(),
        StepState::Done => theme::success(),
        StepState::Failed | StepState::Invalid => theme::danger(),
        StepState::Skipped => theme::text_muted(),
        StepState::Checking
        | StepState::Overridden
        | StepState::Cancelled
        | StepState::TimedOut
        | StepState::Interrupted
        | StepState::Stale => theme::warning(),
    };
    (colour, state.label())
}

fn check_chip(state: CheckState) -> (gpui::Rgba, &'static str) {
    match state {
        CheckState::Waiting => (theme::text_faint(), "waiting"),
        CheckState::Pending => (theme::warning(), "pending"),
        CheckState::Running => (theme::info(), "running"),
        CheckState::Passed => (theme::success(), "passed"),
        CheckState::Failed => (theme::danger(), "failed"),
    }
}

fn render(ctx: &ViewerCtx<'_>, window: &mut Window, cx: &mut Context<AppState>) -> AnyElement {
    let Some(project) = ctx.app.project(cx) else {
        return div().into_any_element();
    };
    let key: Key = (project, ctx.file.path.clone());
    let text = ctx.buffer.read(cx).value().to_string();
    let rev = kubiq_runbook::params::short_hash(text.as_bytes());
    let dirty = ctx.file.dirty();
    // First draw, and every time the saved file moved past the host's last word: (re)open.
    let (first, behind) = {
        let tab = runbooks(cx).tabs.entry(key.clone()).or_default();
        let behind = tab.snapshot.as_ref().is_some_and(|s| s.rev != rev);
        (!tab.opened, behind)
    };
    if first {
        let focus = cx.focus_handle();
        with_tab(cx, &key, |t| {
            t.opened = true;
            t.focus = Some(focus);
        });
    }
    if ctx.app.runbook_unsupported(&key) {
        return kit::slab(theme::warning())
            .m_2()
            .px_2()
            .py_1()
            .bg(theme::warning_soft())
            .child("The drone serving this project is too old to run runbooks: update it.")
            .into_any_element();
    }
    if first || (behind && !dirty) {
        ctx.app.runbook_send(&key, RunbookRequest::Open);
    }
    let book = Runbook::parse(&text);
    let file_key = ctx.file.key();

    let header = header(ctx.app, &key, &book, dirty || behind, window, cx);
    let body = body(&key, &text, &book, &file_key, dirty || behind, window, cx);
    let focus = runbooks(cx).tabs.get(&key).and_then(|t| t.focus.clone());
    let mut root = div()
        .id(eid("rb", &key.1))
        .key_context(format!("{} mode=command", keys::CONTEXT).as_str())
        .size_full()
        .flex()
        .flex_col()
        .bg(theme::pane_bg())
        .child(header)
        .child(body);
    if let Some(focus) = &focus {
        root = root.track_focus(focus);
    }
    let root = actions(root, &key, &book, cx);
    let mut layers = div().size_full().relative().child(root);
    if let Some(zoom) = cells::zoom::overlay(window, cx) {
        layers = layers.child(zoom);
    }
    layers.into_any_element()
}

fn button(
    id: gpui::ElementId,
    icon: IconName,
    tip: &'static str,
    enabled: bool,
    cx: &mut Context<AppState>,
    f: impl Fn(&mut AppState, &mut Window, &mut Context<AppState>) + 'static,
) -> AnyElement {
    let entity = cx.entity();
    let f = Rc::new(f);
    kit::icon_button_tip(id, icon, tip, enabled, move |_, window, cx| {
        let f = f.clone();
        handler(&entity, move |this, window, cx| f(this, window, cx))(window, cx)
    })
    .into_any_element()
}

// ── header ─────────────────────────────────────────────────────────────────────────────────

fn header(
    app: &AppState,
    key: &Key,
    book: &Runbook,
    unsaved: bool,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let Some(tab) = runbooks(cx).tabs.get(key) else {
        return div().into_any_element();
    };
    let snap = tab.snapshot.clone();
    let (bound, agents_open) = (tab.bound, tab.agents_open);
    let notice = tab.notice.clone();
    let confirm = tab.confirm.clone();
    let report = tab.report.clone();
    let form = tab.form.clone();
    let status = snap.as_ref().map(|s| s.status.clone()).unwrap_or_default();
    let locked = status.no_runs().cloned();
    let ready = snap.as_ref().map(|s| s.states.ready()).unwrap_or_default();
    let running: Vec<String> = snap
        .as_ref()
        .map(|s| {
            s.attempts
                .iter()
                .filter(|a| a.status == "running")
                .map(|a| a.cell.clone())
                .collect()
        })
        .unwrap_or_default();
    let can_run = locked.is_none() && !unsaved;

    let mut bar: Vec<AnyElement> = Vec::new();
    if let Some(role) = &status.role {
        bar.push(kit::state_chip(role.clone(), theme::accent(), 0.8).into_any_element());
    }
    let gate = book
        .front
        .gate
        .clone()
        .unwrap_or_else(|| "every-cell".into());
    bar.push(
        div()
            .text_size(theme::font(Family::Chrome, Role::Label))
            .text_color(theme::text_muted())
            .child(format!("gate: {gate}"))
            .into_any_element(),
    );
    {
        let (entity, k, cells) = (cx.entity(), key.clone(), ready.clone());
        let label = format!("Run ready ({})", ready.len());
        let confirmed: Vec<(String, Option<String>)> = ready
            .iter()
            .map(|c| (c.clone(), book.cell(c).and_then(|x| x.confirm.clone())))
            .collect();
        let mut run_ready = kit::primary_button(
            eid("rb-ready", &key.1),
            Some(IconName::Play),
            label,
            move |_, window, cx| {
                let (k, cells, confirmed) = (k.clone(), cells.clone(), confirmed.clone());
                handler(&entity, move |this, _w, cx| {
                    let lookup = |c: &str| {
                        confirmed
                            .iter()
                            .find(|(x, _)| x == c)
                            .and_then(|(_, t)| t.clone())
                    };
                    this.runbook_run(&k, &cells, None, &lookup, cx);
                })(window, cx)
            },
        );
        if !can_run || ready.is_empty() {
            run_ready = run_ready.opacity(0.5);
        }
        bar.push(run_ready.into_any_element());
    }
    if !running.is_empty() {
        let k = key.clone();
        bar.push(button(
            eid("rb-stop-all", &key.1),
            IconName::CircleX,
            "Stop every running cell (I, I)",
            true,
            cx,
            move |this, _w, _cx| {
                for cell in &running {
                    this.runbook_send(&k, RunbookRequest::Stop { cell: cell.clone() });
                }
            },
        ));
    }
    for action in &status.actions {
        let (entity, k, a) = (cx.entity(), key.clone(), action.clone());
        let mut b = kit::ghost_button(
            eid2("rb-act", &key.1, &action.id),
            None,
            action.label.clone(),
            move |_, window, cx| {
                let (k, a) = (k.clone(), a.clone());
                handler(&entity, move |this, _w, cx| {
                    if a.fields.is_empty() && !a.danger {
                        this.runbook_send(
                            &k,
                            RunbookRequest::Action {
                                action: a.id.clone(),
                                fields: Default::default(),
                            },
                        );
                    } else {
                        let values = a
                            .fields
                            .iter()
                            .filter_map(|f| f.default.clone().map(|d| (f.name.clone(), d)))
                            .collect();
                        with_tab(cx, &k, |t| {
                            t.form = Some((a.clone(), values));
                            t.form_inputs.clear();
                        });
                    }
                    cx.notify();
                })(window, cx)
            },
        );
        if action.danger {
            b = b.text_color(theme::danger());
        }
        bar.push(b.into_any_element());
    }
    let (k1, k2) = (key.clone(), key.clone());
    bar.push(cells::agent::bound_agent(
        app,
        &format!("rb-agent-{}", key.1),
        bound,
        agents_open,
        Rc::new(move |_this, _w, cx| {
            with_tab(cx, &k1, |t| t.agents_open = !t.agents_open);
            cx.notify();
        }),
        Rc::new(move |this, agent, cx| {
            this.runbook_send(&k2, RunbookRequest::Bind { agent });
            with_tab(cx, &k2, |t| t.agents_open = false);
            cx.notify();
        }),
        cx,
    ));

    let mut col = div()
        .flex()
        .flex_col()
        .w_full()
        .border_b_1()
        .border_color(theme::border())
        .bg(theme::surface())
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .children(bar),
        );

    // Parameters and the provider's facts, one line.
    if let Some(snap) = &snap {
        let mut facts: Vec<String> = status
            .facts
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect();
        facts.extend(snap.params.iter().map(|(k, v)| format!("{k} = {v}")));
        let errors = snap.lints.errors();
        if errors > 0 {
            facts.push(format!("{errors} lint errors"));
        }
        if !facts.is_empty() {
            col = col.child(
                div()
                    .px_2()
                    .pb_1()
                    .text_size(theme::font(Family::Chrome, Role::Meta))
                    .text_color(theme::text_muted())
                    .font_family(theme::MONO_FONT)
                    .child(facts.join("  ·  ")),
            );
        }
    }
    let banner = |text: String, colour: gpui::Rgba, soft: gpui::Rgba| {
        kit::slab(colour)
            .w_full()
            .px_2()
            .py_1()
            .bg(soft)
            .text_size(theme::font(Family::Chrome, Role::Body))
            .child(text)
    };
    if let Some(why) = &locked {
        col = col.child(banner(
            format!("Runs nothing: {why}"),
            theme::info(),
            theme::info_soft(),
        ));
    } else if unsaved {
        col = col.child(banner(
            "Unsaved changes: save to run — a run binds the saved command".into(),
            theme::warning(),
            theme::warning_soft(),
        ));
    }
    if let Some((text, error)) = notice {
        let k = key.clone();
        let (edge, soft) = if error {
            (theme::danger(), theme::danger_soft())
        } else {
            (theme::success(), theme::success_soft())
        };
        col = col.child(
            banner(text, edge, soft)
                .flex()
                .items_center()
                .child(div().flex_1())
                .child(button(
                    eid("rb-notice-x", &key.1),
                    IconName::Close,
                    "Dismiss",
                    true,
                    cx,
                    move |_this, _w, cx| {
                        with_tab(cx, &k, |t| t.notice = None);
                    },
                )),
        );
    }
    if let Some(c) = confirm {
        let (entity, k, k2) = (cx.entity(), key.clone(), key.clone());
        let c2 = c.clone();
        col = col.child(
            banner(c.message.clone(), theme::warning(), theme::warning_soft())
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(kit::primary_button(
                            eid("rb-confirm", &key.1),
                            None,
                            "Run anyway",
                            move |_, window, cx| {
                                let (k, c2) = (k.clone(), c2.clone());
                                handler(&entity, move |this, _w, cx| {
                                    this.runbook_send(
                                        &k,
                                        RunbookRequest::Run {
                                            approvals: c2.approvals.clone(),
                                            options: c2.options.clone(),
                                        },
                                    );
                                    with_tab(cx, &k, |t| t.confirm = None);
                                    cx.notify();
                                })(window, cx)
                            },
                        ))
                        .child(button(
                            eid("rb-confirm-x", &key.1),
                            IconName::Close,
                            "Cancel",
                            true,
                            cx,
                            move |_this, _w, cx| {
                                with_tab(cx, &k2, |t| t.confirm = None);
                            },
                        )),
                ),
        );
    }
    if let Some((action, values)) = form {
        col = col.child(action_form(key, &action, &values, window, cx));
    }
    if let Some((title, report)) = report {
        let k = key.clone();
        col = col.child(
            kit::slab(theme::accent())
                .w_full()
                .px_2()
                .py_1()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(kit::section_label(&title))
                        .child(div().flex_1())
                        .child(button(
                            eid("rb-report-x", &key.1),
                            IconName::Close,
                            "Close the report",
                            true,
                            cx,
                            move |_this, _w, cx| {
                                with_tab(cx, &k, |t| t.report = None);
                            },
                        )),
                )
                .child(
                    div()
                        .id(eid("rb-report", &key.1))
                        .max_h(px(260.))
                        .overflow_y_scroll()
                        .child(output::markdown_block(
                            format!("rb-report-md-{}", key.1),
                            report,
                        )),
                ),
        );
    }
    col.into_any_element()
}

/// A provider action's fields: choice pills for a closed set, a text field otherwise.
fn action_form(
    key: &Key,
    action: &kubiq_proto::runbook::ActionSpec,
    values: &std::collections::BTreeMap<String, String>,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let mut rows: Vec<AnyElement> = Vec::new();
    for field in &action.fields {
        let mut row = div().flex().flex_wrap().items_center().gap_2().child(
            div()
                .w(px(120.))
                .text_size(theme::font(Family::Chrome, Role::Label))
                .text_color(theme::text_muted())
                .child(if field.required {
                    format!("{} *", field.label)
                } else {
                    field.label.clone()
                }),
        );
        if field.choices.is_empty() {
            let input = runbooks(cx)
                .tabs
                .get(key)
                .and_then(|t| t.form_inputs.get(&field.name).cloned());
            let input = input.unwrap_or_else(|| {
                let default = values.get(&field.name).cloned().unwrap_or_default();
                let f = cx.new(|cx| InputState::new(window, cx).default_value(default));
                let name = field.name.clone();
                with_tab(cx, key, |t| t.form_inputs.insert(name, f.clone()));
                f
            });
            row = row.child(div().w(px(260.)).child(Input::new(&input)));
        } else {
            for choice in &field.choices {
                let (entity, k, name, value) =
                    (cx.entity(), key.clone(), field.name.clone(), choice.clone());
                let active = values.get(&field.name) == Some(choice);
                row = row.child(kit::choice_pill(
                    eid2("rb-choice", &field.name, choice),
                    choice.clone(),
                    active,
                    move |_, window, cx| {
                        let (k, name, value) = (k.clone(), name.clone(), value.clone());
                        handler(&entity, move |_this, _w, cx| {
                            with_tab(cx, &k, |t| {
                                if let Some((_, v)) = t.form.as_mut() {
                                    v.insert(name.clone(), value.clone());
                                }
                            });
                            cx.notify();
                        })(window, cx)
                    },
                ));
            }
        }
        rows.push(row.into_any_element());
    }
    let (entity, k, id) = (cx.entity(), key.clone(), action.id.clone());
    let k2 = key.clone();
    let go = kit::primary_button(
        eid("rb-form-go", &action.id),
        None,
        action.label.clone(),
        move |_, window, cx| {
            let (k, id) = (k.clone(), id.clone());
            handler(&entity, move |this, _w, cx| {
                let Some(tab) = runbooks(cx).tabs.get(&k) else {
                    return;
                };
                let mut fields = tab.form.as_ref().map(|f| f.1.clone()).unwrap_or_default();
                let inputs: Vec<(String, gpui::Entity<InputState>)> = tab
                    .form_inputs
                    .iter()
                    .map(|(n, f)| (n.clone(), f.clone()))
                    .collect();
                let typed: Vec<(String, String)> = inputs
                    .into_iter()
                    .map(|(n, f)| (n, f.read(cx).value().to_string()))
                    .collect();
                fields.extend(typed);
                this.runbook_send(
                    &k,
                    RunbookRequest::Action {
                        action: id.clone(),
                        fields,
                    },
                );
            })(window, cx)
        },
    );
    let edge = if action.danger {
        theme::danger()
    } else {
        theme::accent()
    };
    kit::slab(edge)
        .w_full()
        .px_2()
        .py_1()
        .flex()
        .flex_col()
        .gap_1()
        .children(rows)
        .child(div().flex().gap_2().child(go).child(button(
            eid("rb-form-x", &action.id),
            IconName::Close,
            "Cancel",
            true,
            cx,
            move |_this, _w, cx| {
                with_tab(cx, &k2, |t| {
                    t.form = None;
                    t.form_inputs.clear();
                });
            },
        )))
        .into_any_element()
}

// ── body ───────────────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn body(
    key: &Key,
    text: &str,
    book: &Runbook,
    file_key: &str,
    unsaved: bool,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let mut rows: Vec<AnyElement> = Vec::new();
    for (i, block) in book.source.blocks.iter().enumerate() {
        let piece = &text[block.span.clone()];
        match &block.kind {
            BlockKind::Prose => {
                if !piece.trim().is_empty() {
                    rows.push(output::markdown_block(
                        format!("rb-prose-{}-{i}", key.1),
                        piece.to_string(),
                    ));
                }
            }
            BlockKind::Fence(f) if f.lang == OUTPUT => {
                rows.push(output_block(key, i, f, cx));
            }
            BlockKind::Fence(_) => match book.cells.iter().find(|c| c.block == i) {
                Some(cell) => rows.push(cell_row(key, book, cell, file_key, unsaved, window, cx)),
                None => rows.push(output::markdown_block(
                    format!("rb-fence-{}-{i}", key.1),
                    piece.to_string(),
                )),
            },
        }
    }
    div()
        .id(eid("rb-body", &key.1))
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .px_3()
        .py_2()
        .flex()
        .flex_col()
        .gap_2()
        .children(rows)
        .into_any_element()
}

/// A host-written `output` block: its facts on top, the excerpt selectable with a zoom, and the
/// full retained log a click away.
fn output_block(
    key: &Key,
    i: usize,
    f: &kubiq_runbook::format::Fence,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let attrs = f.attrs.clone().unwrap_or_default();
    let cell = attrs.get("cell").unwrap_or("?").to_string();
    let mut facts: Vec<String> = attrs
        .pairs
        .iter()
        .filter(|(k, _)| k != "cell" && k != "log" && k != "hash")
        .map(|(k, v)| format!("{k} {v}"))
        .collect();
    facts.extend(attrs.flags.iter().cloned());
    let facts = facts.join(" · ");
    let tone = match (
        attrs.get("exit"),
        attrs.get("verdict"),
        attrs.get("outcome"),
    ) {
        (Some("0"), _, _) | (_, Some("pass"), _) => theme::text(),
        (Some(_), _, _) | (_, Some("fail"), _) => theme::danger(),
        (_, _, Some(_)) => theme::warning(),
        _ => theme::text_muted(),
    };
    let attempt: Option<u64> = attrs.get("attempt").and_then(|a| a.parse().ok());
    let collapsed = attrs.has("collapsed");
    let mut head = div()
        .flex()
        .items_center()
        .gap_2()
        .text_size(theme::font(Family::Content, Role::Meta))
        .text_color(theme::text_muted())
        .font_family(theme::MONO_FONT)
        .child(format!("#{cell} · {facts}"));
    if let Some(attempt) = attempt {
        let (k, c) = (key.clone(), cell.clone());
        head = head.child(button(
            eid2("rb-full", &key.1, i),
            IconName::FileText,
            "Open the full output",
            true,
            cx,
            move |this, _w, _cx| {
                this.runbook_full_output(&k, &c, attempt);
            },
        ));
    }
    let mut block = kit::slab(tone)
        .w_full()
        .pl_2()
        .flex()
        .flex_col()
        .gap_1()
        .child(head);
    if !collapsed && !f.body.trim().is_empty() {
        block = block.child(output::text_block(
            format!("rb-out-{}-{i}", key.1),
            format!("#{cell} — output"),
            facts.clone(),
            f.body.clone(),
            theme::text(),
        ));
    }
    block.into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn cell_row(
    key: &Key,
    book: &Runbook,
    cell: &kubiq_runbook::model::Cell,
    file_key: &str,
    unsaved: bool,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let Some(tab) = runbooks(cx).tabs.get(key) else {
        return div().into_any_element();
    };
    let snap = tab.snapshot.clone();
    let selected = tab.selected.as_deref() == Some(cell.id.as_str());
    let ring = tab.tails.get(&cell.id).cloned();
    let ask = tab.ask.clone().filter(|(c, _, _)| *c == cell.id);
    let locked = snap.as_ref().and_then(|s| s.status.no_runs().cloned());
    let can_act = locked.is_none() && !unsaved;
    let id = cell.id.clone();

    let step = snap.as_ref().and_then(|s| s.states.step(&id)).cloned();
    let check = snap.as_ref().and_then(|s| {
        s.states
            .steps
            .iter()
            .flat_map(|st| st.checks.iter())
            .find(|c| c.cell == id)
            .cloned()
    });
    let command = snap.as_ref().and_then(|s| s.commands.get(&id)).cloned();
    let lints: Vec<String> = snap
        .as_ref()
        .map(|s| {
            s.lints
                .lints
                .iter()
                .filter(|l| l.cell.as_deref() == Some(id.as_str()))
                .map(|l| l.message.clone())
                .collect()
        })
        .unwrap_or_default();
    let running = snap.as_ref().is_some_and(|s| {
        s.attempts
            .iter()
            .any(|a| a.cell == id && a.status == "running")
    });
    let pending: Vec<kubiq_proto::runbook::Pending> = snap
        .as_ref()
        .map(|s| {
            s.pending
                .iter()
                .filter(|p| p.cells.first() == Some(&id))
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    // Title row.
    let kind = match cell.kind {
        CellKind::Step => "step",
        CellKind::Check => "check",
        CellKind::Manual => "manual check",
    };
    let mut title = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .text_size(theme::font(Family::Content, Role::Label))
        .child(
            div()
                .font_family(theme::MONO_FONT)
                .text_color(theme::text())
                .child(format!("#{id}")),
        )
        .child(
            div()
                .text_color(theme::text_muted())
                .child(format!("{kind} · {}", cell.lang)),
        );
    if let Some(step) = &step {
        let (c, w) = state_chip(step.state);
        title = title.child(kit::state_chip(w, c, 0.8));
        if !step.needs.is_empty() {
            title = title.child(
                div()
                    .text_color(theme::text_muted())
                    .child(format!("needs {}", step.needs.join(", "))),
            );
        }
        if !step.waits_on.is_empty() {
            title = title.child(
                div()
                    .text_color(theme::warning())
                    .child(format!("waits on {}", step.waits_on.join(", "))),
            );
        }
        for c in &step.checks {
            let (col, w) = check_chip(c.state);
            title = title.child(kit::state_chip(format!("{} {w}", c.cell), col, 0.7));
        }
    } else if let Some(check) = &check {
        let (c, w) = check_chip(check.state);
        title = title.child(kit::state_chip(w, c, 0.8));
        if let Some(of) = &cell.check_of {
            title = title.child(
                div()
                    .text_color(theme::text_muted())
                    .child(format!("checks {of}")),
            );
        }
    }
    if cell.readonly {
        title = title.child(kit::badge("readonly", theme::text_muted()));
    }
    if let Some(c) = &cell.confirm {
        let label = format!("confirm: {c}");
        title = title.child(kit::badge(&label, theme::warning()));
    }
    title = title.child(div().flex_1());

    // Controls.
    let k = key.clone();
    if cell.kind != CellKind::Manual {
        let (k1, c1) = (k.clone(), id.clone());
        let confirm = cell.confirm.clone();
        title = title.child(button(
            eid2("rb-run", &key.1, &id),
            IconName::Play,
            "Run (Ctrl-Enter)",
            can_act && !running,
            cx,
            move |this, _w, cx| {
                let confirm = confirm.clone();
                this.runbook_run(
                    &k1,
                    std::slice::from_ref(&c1),
                    None,
                    &move |_| confirm.clone(),
                    cx,
                );
            },
        ));
        if running {
            let (k1, c1) = (k.clone(), id.clone());
            title = title.child(button(
                eid2("rb-stop", &key.1, &id),
                IconName::CircleX,
                "Stop (I, I)",
                true,
                cx,
                move |this, _w, _cx| {
                    this.runbook_send(&k1, RunbookRequest::Stop { cell: c1.clone() });
                },
            ));
        }
    }
    let ask_button = |what: Ask, icon: IconName, tip: &'static str, cx: &mut Context<AppState>| {
        let (k1, c1) = (k.clone(), id.clone());
        button(
            eid2("rb-ask", &key.1, format!("{id}-{what:?}")),
            icon,
            tip,
            can_act,
            cx,
            move |_this, window, cx| {
                let f = cx.new(|cx| {
                    InputState::new(window, cx).placeholder(match what {
                        Ask::Pass | Ask::Fail => "Note (optional)",
                        _ => "Why",
                    })
                });
                with_tab(cx, &k1, |t| t.ask = Some((c1.clone(), what, f)));
            },
        )
    };
    match cell.kind {
        CellKind::Step => {
            title = title.child(ask_button(
                Ask::Skip,
                IconName::Ban,
                "Skip, with a reason",
                cx,
            ));
            if step
                .as_ref()
                .is_some_and(|s| !s.state.satisfies() && s.runs > 0)
            {
                title = title.child(ask_button(
                    Ask::Override,
                    IconName::TriangleAlert,
                    "Override: count it done, with a reason",
                    cx,
                ));
            }
        }
        CellKind::Manual => {
            let eligible = check
                .as_ref()
                .is_some_and(|c| c.state != CheckState::Waiting);
            if eligible {
                title = title.child(ask_button(
                    Ask::Pass,
                    IconName::Check,
                    "Pass, with a note",
                    cx,
                ));
                title = title.child(ask_button(
                    Ask::Fail,
                    IconName::Close,
                    "Fail, with a note",
                    cx,
                ));
            }
        }
        CellKind::Check => {}
    }
    if cell.readonly
        && cell.kind != CellKind::Manual
        && book.front.gate.as_deref() == Some("readonly-auto")
    {
        title = title.child(ask_button(
            Ask::AcceptReadonly,
            IconName::Eye,
            "Accept as readonly: agents may run it under readonly-auto",
            cx,
        ));
    }

    // The command: what will run, secrets masked.
    let shown = command
        .as_ref()
        .map(|c| c.display.clone())
        .unwrap_or_else(|| cell.body.clone());
    let facts = command
        .as_ref()
        .map(|c| format!("{} · hash {}", c.shell, c.hash))
        .unwrap_or_default();
    let mut body = div().flex().flex_col().gap_1().w_full().child(title);
    for lint in lints {
        body = body.child(
            div()
                .text_color(theme::danger())
                .text_size(theme::font(Family::Content, Role::Meta))
                .child(lint),
        );
    }
    body = body.child(
        div()
            .w_full()
            .bg(theme::surface())
            .border_1()
            .border_color(theme::border())
            .px_2()
            .font_family(theme::MONO_FONT)
            .child(output::text_block(
                format!("rb-cmd-{}-{id}", key.1),
                format!("#{id}"),
                facts,
                shown,
                theme::text(),
            )),
    );

    // An agent's request, as a bar.
    for p in pending {
        let changed = p.cells.iter().any(|c| {
            snap.as_ref()
                .and_then(|s| s.commands.get(c))
                .is_some_and(|cmd| p.hashes.get(c) != Some(&cmd.hash))
        });
        let what = match p.kind {
            kubiq_proto::runbook::PendingKind::Run => "run",
            kubiq_proto::runbook::PendingKind::Check => "judge",
        };
        let mut text = format!(
            "{} asks to {what} {}: {}",
            p.by,
            p.cells
                .iter()
                .map(|c| format!("#{c}"))
                .collect::<Vec<_>>()
                .join(", "),
            p.why
        );
        if changed {
            text.push_str(" — the command changed since it was asked; review it");
        }
        let (k1, cells, req) = (key.clone(), p.cells.clone(), p.id);
        let (k2, fk) = (key.clone(), file_key.to_string());
        let (k3, req3, cells3, kind3) = (key.clone(), p.id, p.cells.clone(), p.kind);
        let confirms: Vec<(String, Option<String>)> = p
            .cells
            .iter()
            .map(|c| (c.clone(), book.cell(c).and_then(|x| x.confirm.clone())))
            .collect();
        let mut bar = div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_1().child(text));
        if p.kind == kubiq_proto::runbook::PendingKind::Run {
            bar = bar.child(button(
                eid2("rb-req-run", &key.1, p.id),
                IconName::Play,
                "Run what is shown",
                can_act,
                cx,
                move |this, _w, cx| {
                    let confirms = confirms.clone();
                    let lookup = move |c: &str| {
                        confirms
                            .iter()
                            .find(|(x, _)| x == c)
                            .and_then(|(_, t)| t.clone())
                    };
                    this.runbook_run(&k1, &cells, Some(req), &lookup, cx);
                },
            ));
        }
        bar = bar
            .child(button(
                eid2("rb-req-edit", &key.1, p.id),
                IconName::FileText,
                "Edit the cell first (Source)",
                true,
                cx,
                move |this, _w, cx| {
                    this.set_view_layout(&fk, ViewLayout::Source, cx);
                    let _ = &k2;
                },
            ))
            .child(button(
                eid2("rb-req-skip", &key.1, p.id),
                IconName::Ban,
                "Skip: set the request aside",
                true,
                cx,
                move |this, _w, _cx| {
                    if kind3 == kubiq_proto::runbook::PendingKind::Run && cells3.len() == 1 {
                        this.runbook_send(
                            &k3,
                            RunbookRequest::Skip {
                                cell: cells3[0].clone(),
                                reason: "skipped the agent's request".into(),
                            },
                        );
                    }
                    this.runbook_send(&k3, RunbookRequest::Dismiss { request: req3 });
                },
            ));
        body = body.child(
            kit::slab(theme::agent_controlled())
                .w_full()
                .px_2()
                .py_1()
                .bg(theme::agent_controlled_soft())
                .text_size(theme::font(Family::Content, Role::Body))
                .child(bar),
        );
    }

    // The reason or note field.
    if let Some((_, what, field)) = ask {
        let (entity, k1, c1, f1) = (cx.entity(), key.clone(), id.clone(), field.clone());
        let k2 = key.clone();
        let label = match what {
            Ask::Skip => "Skip",
            Ask::Override => "Override",
            Ask::Pass => "Pass",
            Ask::Fail => "Fail",
            Ask::AcceptReadonly => "Accept",
        };
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().w(px(320.)).child(Input::new(&field)))
                .child(kit::primary_button(
                    eid2("rb-ask-go", &key.1, &id),
                    None,
                    label,
                    move |_, window, cx| {
                        let (k1, c1, f1) = (k1.clone(), c1.clone(), f1.clone());
                        handler(&entity, move |this, _w, cx| {
                            let text = f1.read(cx).value().to_string();
                            let cell = c1.clone();
                            let request = match what {
                                Ask::Skip => RunbookRequest::Skip { cell, reason: text },
                                Ask::Override => RunbookRequest::Override { cell, reason: text },
                                Ask::Pass => RunbookRequest::Verdict {
                                    cell,
                                    pass: true,
                                    note: text,
                                },
                                Ask::Fail => RunbookRequest::Verdict {
                                    cell,
                                    pass: false,
                                    note: text,
                                },
                                // The hash of the command on screen: the host refuses if it moved.
                                Ask::AcceptReadonly => {
                                    let hash = runbooks(cx)
                                        .tabs
                                        .get(&k1)
                                        .and_then(|t| t.snapshot.as_ref())
                                        .and_then(|s| s.commands.get(&cell))
                                        .map(|c| c.hash.clone())
                                        .unwrap_or_default();
                                    RunbookRequest::AcceptReadonly {
                                        cell,
                                        reason: text,
                                        hash,
                                    }
                                }
                            };
                            this.runbook_send(&k1, request);
                            with_tab(cx, &k1, |t| t.ask = None);
                            cx.notify();
                        })(window, cx)
                    },
                ))
                .child(button(
                    eid2("rb-ask-x", &key.1, &id),
                    IconName::Close,
                    "Cancel",
                    true,
                    cx,
                    move |_this, _w, cx| {
                        with_tab(cx, &k2, |t| t.ask = None);
                    },
                )),
        );
        let _ = window;
    }

    // The live tail: fixed height, the newest lines, what was skipped.
    if let Some(ring) = ring.filter(|_| running) {
        let mut tail = div()
            .w_full()
            .h(px(15. * 16.))
            .overflow_hidden()
            .bg(theme::app_bg())
            .px_2()
            .font_family(theme::MONO_FONT)
            .text_size(theme::font(Family::Content, Role::Dense))
            .flex()
            .flex_col()
            .justify_end();
        if ring.skipped > 0 {
            tail = tail.child(
                div()
                    .text_color(theme::text_faint())
                    .child(format!("… {} lines skipped", ring.skipped)),
            );
        }
        for line in ring.lines {
            tail = tail.child(div().text_color(theme::text()).child(line));
        }
        body = body.child(tail);
    }

    let (entity, k1, c1) = (cx.entity(), key.clone(), id.clone());
    let edge = if selected {
        theme::accent()
    } else {
        theme::border()
    };
    kit::slab(edge)
        .id(eid2("rb-row", &key.1, &id))
        .w_full()
        .pl_2()
        .py_1()
        .when(selected, |d| d.bg(theme::hover()))
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            let (k1, c1) = (k1.clone(), c1.clone());
            handler(&entity, move |_this, _w, cx| {
                with_tab(cx, &k1, |t| t.selected = Some(c1.clone()));
                cx.notify();
            })(window, cx)
        })
        .child(body)
        .into_any_element()
}

// ── keys ───────────────────────────────────────────────────────────────────────────────────

fn actions(
    root: gpui::Stateful<gpui::Div>,
    key: &Key,
    book: &Runbook,
    cx: &mut Context<AppState>,
) -> gpui::Stateful<gpui::Div> {
    let ids: Vec<String> = book.cells.iter().map(|c| c.id.clone()).collect();
    let confirms: Vec<(String, Option<String>)> = book
        .cells
        .iter()
        .map(|c| (c.id.clone(), c.confirm.clone()))
        .collect();
    let run = {
        let (k, confirms) = (key.clone(), confirms.clone());
        move |this: &mut AppState, cx: &mut Context<AppState>| {
            let Some(cell) = runbooks(cx).tabs.get(&k).and_then(|t| t.selected.clone()) else {
                return;
            };
            // An agent's request on the cell is what this run answers.
            let request = runbooks(cx)
                .tabs
                .get(&k)
                .and_then(|t| t.snapshot.as_ref())
                .and_then(|s| {
                    s.pending
                        .iter()
                        .find(|p| p.cells.contains(&cell))
                        .map(|p| p.id)
                });
            let lookup = |c: &str| {
                confirms
                    .iter()
                    .find(|(x, _)| x == c)
                    .and_then(|(_, t)| t.clone())
            };
            this.runbook_run(&k, std::slice::from_ref(&cell), request, &lookup, cx);
        }
    };
    let run = Rc::new(run);
    let (r1, r2) = (run.clone(), run.clone());
    let move_sel = {
        let (k, ids) = (key.clone(), ids.clone());
        move |cx: &mut Context<AppState>, step: isize| {
            with_tab(cx, &k, |t| {
                let at = t
                    .selected
                    .as_ref()
                    .and_then(|s| ids.iter().position(|i| i == s));
                let next = match at {
                    Some(n) => {
                        (n as isize + step).clamp(0, ids.len().saturating_sub(1) as isize) as usize
                    }
                    None => 0,
                };
                t.selected = ids.get(next).cloned();
            });
            cx.notify();
        }
    };
    let move_sel = Rc::new(move_sel);
    let (m1, m2) = (move_sel.clone(), move_sel.clone());
    let (k_stop, k_zoom) = (key.clone(), key.clone());
    root.on_action(cx.listener(move |this, _: &keys::RunAdvance, _w, cx| {
        r1(this, cx);
        m1(cx, 1);
    }))
    .on_action(cx.listener(move |this, _: &keys::RunInPlace, _w, cx| r2(this, cx)))
    .on_action(cx.listener(move |_this, _: &keys::SelectNext, _w, cx| m2(cx, 1)))
    .on_action(cx.listener(move |_this, _: &keys::SelectPrev, _w, cx| move_sel(cx, -1)))
    .on_action(cx.listener(move |this, _: &keys::InterruptChord, _w, cx| {
        let second = with_tab(cx, &k_stop, |t| {
            let second = t.chord == Some('i');
            t.chord = if second { None } else { Some('i') };
            second
        });
        if second == Some(true)
            && let Some(cell) = runbooks(cx)
                .tabs
                .get(&k_stop)
                .and_then(|t| t.selected.clone())
        {
            this.runbook_send(&k_stop, RunbookRequest::Stop { cell });
        }
    }))
    .on_action(cx.listener(move |_this, _: &keys::ZoomCell, _w, cx| {
        let Some(tab) = runbooks(cx).tabs.get(&k_zoom) else {
            return;
        };
        let Some(cell) = tab.selected.clone() else {
            return;
        };
        let shown = tab
            .snapshot
            .as_ref()
            .and_then(|s| s.commands.get(&cell))
            .map(|c| (c.display.clone(), format!("{} · hash {}", c.shell, c.hash)));
        if let Some((text, facts)) = shown {
            cells::zoom::open_text(cx, format!("#{cell}"), facts, text, None);
        }
    }))
}
