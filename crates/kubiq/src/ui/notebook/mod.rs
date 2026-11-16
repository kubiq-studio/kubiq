//! The notebook viewer: a `.ipynb` drawn as cells from the host's document (`D204` viewer seam,
//! registered by the base).
//!
//! **The tab's buffer is not the document.** The host reads the file with no cap and owns it
//! (`kubiq-host/src/notebook/`); this viewer mirrors its cell views (`state::notebook`) and sends
//! cell edits, runs and kernel requests as `Message::Notebook`. ⌘S on the tab asks the host to
//! write (`AppState::save_file`'s notebook branch), so the buffer's possibly capped read is never
//! written back.
//!
//! One row per cell: an execution-count gutter and the run state, then the source — a code
//! editor, or markdown rendered (double-click edits it) — then the outputs, each selectable with a
//! zoom. The header carries the kernel, its controls, the agent policy and the bound agent; a
//! banner offers the `ipykernel` choices when no interpreter has it. Keys: `ui::cells::keys`.

use std::rc::Rc;

use gpui::{
    AnyElement, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use gpui::prelude::FluentBuilder as _;
use gpui_component::IconName;
use gpui_component::input::{Editor, EditorState, Input, InputEvent, InputState};
use kubiq_cells::RunState;
use kubiq_notebook::output as out;
use kubiq_proto::notebook::{
    CellKind, CellOp, CellView, EnvAction, KernelChoice, KernelState, NotebookRequest, Output, RunPolicy,
};

use crate::app::AppState;
use crate::ext::viewer::{self, Claim, ViewerCtx, ViewerSpec};
use crate::ext::{Registry, ids};
use crate::state::editor::{FileLanguage, ViewLayout};
use crate::state::notebook::{Key, Mode, Tab, notebooks};
use crate::theme::{self, Family, Role};
use crate::ui::cells::{self, keys, output};
use crate::ui::{eid, eid2, handler, kit};

pub fn register(reg: &mut Registry<ViewerSpec>) {
    viewer::register(
        reg,
        ViewerSpec {
            id: ids::NOTEBOOK_VIEWER,
            label: "Notebook",
            claims,
            layouts: &[],
            default_layout: ViewLayout::Preview,
            language: Some(FileLanguage::Json),
            live_reload: false,
            render,
            menu: None,
            header_left: None,
            header_right: None,
        },
    );
}

pub fn claims(path: &str, _head: Option<&str>) -> Claim {
    if path.to_lowercase().ends_with(".ipynb") {
        Claim::Default
    } else {
        Claim::No
    }
}

/// Run `f` on the tab, if it is there.
fn with_tab<R>(cx: &mut gpui::App, key: &Key, f: impl FnOnce(&mut Tab) -> R) -> Option<R> {
    notebooks(cx).tabs.get_mut(key).map(f)
}

fn render(ctx: &ViewerCtx<'_>, window: &mut Window, cx: &mut Context<AppState>) -> AnyElement {
    let Some(project) = ctx.app.project(cx) else {
        return div().into_any_element();
    };
    let key: Key = (project, ctx.file.path.clone());
    // First draw: subscribe, and ask for the kernels.
    let first = !notebooks(cx).tabs.get(&key).is_some_and(|t| t.opened);
    if first {
        let focus = cx.focus_handle();
        let tab = notebooks(cx).tabs.entry(key.clone()).or_default();
        tab.opened = true;
        tab.focus = Some(focus);
        let default_policy = ctx.app.cells_settings().notebook_policy;
        ctx.app.notebook_send(&key, NotebookRequest::Open { default_policy });
        ctx.app.notebook_send(&key, NotebookRequest::ResolveKernels { run_project: false });
    }
    sync_editors(&key, window, cx);
    output::set_inline_lines(ctx.app.cells_settings().output_lines);

    let focus = notebooks(cx).tabs.get(&key).and_then(|t| t.focus.clone());
    let editors: Vec<Entity<EditorState>> = notebooks(cx)
        .tabs
        .get(&key)
        .map(|t| t.editors.values().map(|e| e.0.clone()).collect())
        .unwrap_or_default();
    let editing = editors
        .iter()
        .any(|e| gpui::Focusable::focus_handle(e.read(cx), cx).is_focused(window));
    with_tab(cx, &key, |t| t.mode = if editing { Mode::Edit } else { Mode::Command });

    let header = header(ctx.app, &key, cx);
    let body = cells_list(&key, window, cx);
    let mut root = div()
        .id(eid("nb", &key.1))
        .key_context(format!(
            "{} mode={}",
            keys::CONTEXT,
            if editing { "edit" } else { "command" }
        ).as_str())
        .size_full()
        .flex()
        .flex_col()
        .bg(theme::pane_bg())
        .child(header)
        .child(body);
    if let Some(focus) = &focus {
        root = root.track_focus(focus);
    }
    let root = actions(root, &key, cx);
    let mut layers = div().size_full().relative().child(root);
    if let Some(zoom) = cells::zoom::overlay(window, cx) {
        layers = layers.child(zoom);
    }
    layers.into_any_element()
}

/// Build an editor for every code and raw cell (and every markdown cell being edited) that lacks
/// one, and bring an editor's text up to the host's where the person is not typing in it.
fn sync_editors(key: &Key, window: &mut Window, cx: &mut Context<AppState>) {
    let Some(tab) = notebooks(cx).tabs.get(key) else { return };
    let wanted: Vec<(String, String, CellKind)> = tab
        .cells
        .iter()
        .filter(|c| c.kind != CellKind::Markdown || tab.editing_md.contains(&c.id))
        .filter(|c| !tab.editors.contains_key(&c.id))
        .map(|c| (c.id.clone(), c.source.clone(), c.kind))
        .collect();
    for (id, source, kind) in wanted {
        let editor = cx.new(|cx| {
            let state = EditorState::new(window, cx)
                .line_number(false)
                .soft_wrap(true)
                .default_value(source);
            match kind {
                CellKind::Code => state.language("python"),
                CellKind::Markdown => state.language("markdown"),
                CellKind::Raw => state,
            }
        });
        let (k, cell) = (key.clone(), id.clone());
        let sub = cx.subscribe_in(&editor, window, move |this: &mut AppState, editor: &Entity<EditorState>, event: &InputEvent, _window, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let source = editor.read(cx).value().to_string();
            let same = notebooks(cx)
                .tabs
                .get(&k)
                .and_then(|t| t.cells.iter().find(|c| c.id == cell))
                .is_some_and(|c| c.source == source);
            if !same {
                this.notebook_edit(&k, vec![CellOp::Source { id: cell.clone(), source }], cx);
            }
        });
        with_tab(cx, key, |t| t.editors.insert(id, (editor, sub)));
    }
    // The host's text, where the editor is not the one being typed in (cursor-safe).
    let incoming: Vec<(String, String)> = with_tab(cx, key, |t| t.incoming.drain().collect()).unwrap_or_default();
    for (id, source) in incoming {
        let Some(editor) = notebooks(cx).tabs.get(key).and_then(|t| t.editors.get(&id)).map(|e| e.0.clone()) else {
            continue;
        };
        let focused = gpui::Focusable::focus_handle(editor.read(cx), cx).is_focused(window);
        if !focused && editor.read(cx).value().as_ref() != source {
            editor.update(cx, |state, cx| state.set_value(source, window, cx));
        }
    }
}

// ── header ─────────────────────────────────────────────────────────────────────────────────

fn kernel_colour(state: &KernelState) -> (gpui::Rgba, &'static str) {
    match state {
        KernelState::Off => (theme::text_faint(), "no kernel"),
        KernelState::Starting => (theme::info(), "starting"),
        KernelState::Idle => (theme::success(), "idle"),
        KernelState::Busy => (theme::warning(), "busy"),
        KernelState::Dead { .. } => (theme::danger(), "dead"),
    }
}

fn button(
    key: &Key,
    name: &str,
    icon: IconName,
    tip: &'static str,
    cx: &mut Context<AppState>,
    f: impl Fn(&mut AppState, &Key, &mut Window, &mut Context<AppState>) + 'static,
) -> AnyElement {
    let (entity, key2) = (cx.entity(), key.clone());
    let f = Rc::new(f);
    kit::icon_button_tip(eid2("nb-btn", &key.1, name), icon, tip, true, move |_, window, cx| {
        let (f, k) = (f.clone(), key2.clone());
        handler(&entity, move |this, window, cx| f(this, &k, window, cx))(window, cx)
    })
    .into_any_element()
}

fn header(app: &AppState, key: &Key, cx: &mut Context<AppState>) -> AnyElement {
    let Some(tab) = notebooks(cx).tabs.get(key) else { return div().into_any_element() };
    let (dot, word) = kernel_colour(&tab.kernel);
    let label = tab.kernel_label.clone().unwrap_or_else(|| "Pick a kernel".into());
    let policy = tab.policy;
    let (bound, agents_open, picker) = (tab.bound, tab.agents_open, tab.picker);
    let dirty = tab.dirty;
    let candidates = tab.candidates.clone();
    let notice = tab.notice.clone();
    let conflict = tab.conflict;
    let questions = tab.questions.clone();
    // A project interpreter listed by path is unknown until the person lets Kubiq run it.
    let unprobed = candidates.iter().any(|c| c.python.is_some() && c.ipykernel.is_none());
    let lacks_ipykernel = !candidates.is_empty()
        && !unprobed
        && candidates.iter().all(|c| c.ipykernel != Some(true));
    let first_python = candidates.iter().find_map(|c| c.python.clone());
    let ephemeral = candidates.iter().any(|c| c.id == "uv:ephemeral");

    let entity = cx.entity();
    let k = key.clone();
    let kernel_chip = kit::ghost_button(eid("nb-kernel", &key.1), None, format!("{label} \u{b7} {word}"), move |_, window, cx| {
        let k = k.clone();
        handler(&entity, move |this, _w, cx| {
            let open = with_tab(cx, &k, |t| {
                t.picker = !t.picker;
                t.picker
            });
            if open == Some(true) {
                this.notebook_send(&k, NotebookRequest::ResolveKernels { run_project: false });
            }
            cx.notify();
        })(window, cx)
    })
    .child(kit::status_dot(dot, theme::transparent()))
    .into_any_element();

    let toolbar = vec![
        kernel_chip,
        button(key, "run", IconName::Play, "Run cell (Shift-Enter)", cx, |this, k, _w, cx| {
            if let Some(id) = notebooks(cx).tabs.get(k).and_then(Tab::selected_id) {
                this.notebook_run(k, &id, cx);
            }
        }),
        button(key, "interrupt", IconName::CircleX, "Interrupt (I, I)", cx, |this, k, _w, _cx| {
            this.notebook_send(k, NotebookRequest::Interrupt)
        }),
        button(key, "restart", IconName::Redo, "Restart kernel (0, 0)", cx, |this, k, _w, _cx| {
            this.notebook_send(k, NotebookRequest::Restart { run_all: false })
        }),
        button(key, "runall", IconName::ChevronDown, "Restart and run all", cx, |this, k, _w, _cx| {
            this.notebook_send(k, NotebookRequest::Restart { run_all: true })
        }),
        button(key, "clear", IconName::Delete, "Clear all outputs", cx, |this, k, _w, cx| {
            this.notebook_edit(k, vec![CellOp::ClearOutputs { id: None }], cx)
        }),
        button(key, "undo", IconName::Undo, "Undo cell operation (Z)", cx, |this, k, _w, _cx| {
            this.notebook_send(k, NotebookRequest::Undo)
        }),
        button(key, "save", IconName::Inbox, if dirty { "Save (unsaved changes)" } else { "Save" }, cx, |this, k, w, cx| {
            this.save_notebook(k, w, cx)
        }),
    ];

    let mut policy_pills: Vec<AnyElement> = vec![
        div()
            .text_size(theme::font(Family::Chrome, Role::Label))
            .text_color(theme::text_muted())
            .child("Agents may run:")
            .into_any_element(),
    ];
    for (p, label) in [(RunPolicy::Ask, "ask"), (RunPolicy::Allow, "allow"), (RunPolicy::Deny, "deny")] {
        let (entity, k) = (cx.entity(), key.clone());
        policy_pills.push(
            kit::choice_pill(eid2("nb-policy", &key.1, label), label, policy == p, move |_, window, cx| {
                let k = k.clone();
                handler(&entity, move |this, _w, _cx| {
                    this.notebook_send(&k, NotebookRequest::SetPolicy { policy: p })
                })(window, cx)
            })
            .into_any_element(),
        );
    }
    let (k1, k2) = (key.clone(), key.clone());
    let agent = cells::agent::bound_agent(
        app,
        &format!("nb-agent-{}", key.1),
        bound,
        agents_open,
        Rc::new(move |_this, _w, cx| {
            with_tab(cx, &k1, |t| t.agents_open = !t.agents_open);
            cx.notify();
        }),
        Rc::new(move |this, agent, cx| {
            this.notebook_send(&k2, NotebookRequest::Bind { agent });
            with_tab(cx, &k2, |t| t.agents_open = false);
            cx.notify();
        }),
        cx,
    );

    let mut col = div()
        .flex()
        .flex_col()
        .w_full()
        .border_b_1()
        .border_color(theme::border())
        .bg(theme::surface())
        .child(div().flex().items_center().gap_1().px_2().h(px(32.)).children(toolbar))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .pb_1()
                .flex_wrap()
                .children(policy_pills)
                .child(agent),
        );

    if picker {
        let mut rows: Vec<AnyElement> = Vec::new();
        for c in &candidates {
            let (entity, k, id) = (cx.entity(), key.clone(), c.id.clone());
            let note = match c.ipykernel {
                Some(false) => " (no ipykernel)",
                _ => "",
            };
            rows.push(
                kit::ghost_button(eid2("nb-cand", &key.1, &c.id), None, format!("{:?} \u{b7} {}{note}", c.group, c.label), move |_, window, cx| {
                    let (k, id) = (k.clone(), id.clone());
                    handler(&entity, move |this, _w, cx| {
                        this.notebook_send(&k, NotebookRequest::StartKernel { choice: KernelChoice::Candidate(id.clone()) });
                        with_tab(cx, &k, |t| t.picker = false);
                        cx.notify();
                    })(window, cx)
                })
                .into_any_element(),
            );
        }
        if rows.is_empty() {
            rows.push(div().text_color(theme::text_faint()).child("Looking for interpreters\u{2026}").into_any_element());
        }
        col = col.child(div().flex().flex_col().px_2().pb_1().children(rows));
    }

    let mut banner = |colour: gpui::Rgba, text: String, actions: Vec<AnyElement>| {
        col = std::mem::replace(&mut col, div()).child(
            kit::slab(colour)
                .flex()
                .flex_col()
                .gap_1()
                .px_2()
                .py_1()
                .child(div().text_size(theme::font(Family::Chrome, Role::Label)).child(text))
                .child(div().flex().gap_1().flex_wrap().children(actions)),
        );
    };
    if unprobed {
        let (entity, k) = (cx.entity(), key.clone());
        let detect = kit::ghost_button(eid("nb-detect", &key.1), None, "Detect kernels", move |_, window, cx| {
            let k = k.clone();
            handler(&entity, move |this, _w, _cx| {
                this.notebook_send(&k, NotebookRequest::ResolveKernels { run_project: true })
            })(window, cx)
        })
        .into_any_element();
        banner(
            theme::info(),
            "This project has its own Python. Detecting kernels runs it (and uv) to check for \
             ipykernel; picking one from the kernel list runs only that one."
                .into(),
            vec![detect],
        );
    }
    if lacks_ipykernel {
        let mut actions = Vec::new();
        let (entity, k) = (cx.entity(), key.clone());
        let python = first_python.clone();
        actions.push(
            kit::ghost_button(eid("nb-add-ipk", &key.1), None, "Add ipykernel to the project", move |_, window, cx| {
                let (k, python) = (k.clone(), python.clone());
                handler(&entity, move |this, _w, _cx| {
                    this.notebook_send(&k, NotebookRequest::Env { action: EnvAction::InstallIpykernel { python: python.clone() } })
                })(window, cx)
            })
            .into_any_element(),
        );
        if ephemeral {
            let (entity, k) = (cx.entity(), key.clone());
            actions.push(
                kit::ghost_button(eid("nb-uvrun", &key.1), None, "Run without installing (uv)", move |_, window, cx| {
                    let k = k.clone();
                    handler(&entity, move |this, _w, _cx| {
                        this.notebook_send(&k, NotebookRequest::StartKernel { choice: KernelChoice::Candidate("uv:ephemeral".into()) })
                    })(window, cx)
                })
                .into_any_element(),
            );
        }
        let (entity, k) = (cx.entity(), key.clone());
        actions.push(
            kit::ghost_button(eid("nb-venv", &key.1), None, "Create a project environment (uv venv)", move |_, window, cx| {
                let k = k.clone();
                handler(&entity, move |this, _w, _cx| {
                    this.notebook_send(&k, NotebookRequest::Env { action: EnvAction::CreateVenv })
                })(window, cx)
            })
            .into_any_element(),
        );
        banner(theme::warning(), "No interpreter here has ipykernel. Nothing is installed without a click:".into(), actions);
    }
    for (run, agent, cells_asked) in questions {
        let listed: Vec<String> = cells_asked
            .iter()
            .map(|(id, src)| format!("{id}: {}", src.lines().next().unwrap_or("")))
            .collect();
        let mut actions = Vec::new();
        for (name, allow, session) in [("Allow", true, false), ("Allow for this session", true, true), ("Deny", false, false)] {
            let (entity, k) = (cx.entity(), key.clone());
            actions.push(
                kit::ghost_button(eid2("nb-q", run, name), None, name, move |_, window, cx| {
                    let k = k.clone();
                    handler(&entity, move |this, _w, _cx| {
                        this.notebook_send(&k, NotebookRequest::DecideRun { run, allow, session })
                    })(window, cx)
                })
                .into_any_element(),
            );
        }
        banner(theme::agent_controlled(), format!("Agent {agent} asks to run:\n{}", listed.join("\n")), actions);
    }
    if conflict {
        banner(theme::danger(), "The file changed on disk; saving overwrites it.".into(), Vec::new());
    }
    if let Some(notice) = notice {
        let (entity, k) = (cx.entity(), key.clone());
        let dismiss = kit::ghost_button(eid("nb-dismiss", &key.1), None, "Dismiss", move |_, window, cx| {
            let k = k.clone();
            handler(&entity, move |_this, _w, cx| {
                with_tab(cx, &k, |t| t.notice = None);
                cx.notify();
            })(window, cx)
        })
        .into_any_element();
        banner(theme::info(), notice, vec![dismiss]);
    }
    col.into_any_element()
}

// ── cells ──────────────────────────────────────────────────────────────────────────────────

fn cells_list(key: &Key, window: &mut Window, cx: &mut Context<AppState>) -> AnyElement {
    let Some(tab) = notebooks(cx).tabs.get(key) else { return div().into_any_element() };
    if !tab.loaded {
        return div().p_4().text_color(theme::text_faint()).child("Reading the notebook\u{2026}").into_any_element();
    }
    let cells: Vec<CellView> = tab.cells.clone();
    let mut rows: Vec<AnyElement> = Vec::new();
    for (n, cell) in cells.iter().enumerate() {
        rows.push(cell_row(key, n, cell, window, cx));
    }
    let (entity, k) = (cx.entity(), key.clone());
    let n = cells.len();
    rows.push(
        div()
            .flex()
            .gap_1()
            .p_2()
            .child(kit::ghost_button(eid("nb-add-code", &key.1), Some(IconName::Plus), "Code", {
                let (entity, k) = (entity.clone(), k.clone());
                move |_, window, cx| {
                    let k = k.clone();
                    handler(&entity, move |this, _w, cx| this.notebook_insert(&k, n, CellKind::Code, String::new(), cx))(window, cx)
                }
            }))
            .child(kit::ghost_button(eid("nb-add-md", &key.1), Some(IconName::Plus), "Markdown", move |_, window, cx| {
                let k = k.clone();
                handler(&entity, move |this, _w, cx| this.notebook_insert(&k, n, CellKind::Markdown, String::new(), cx))(window, cx)
            }))
            .into_any_element(),
    );
    div()
        .id(eid("nb-cells", &key.1))
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .children(rows)
        .into_any_element()
}

fn cell_row(key: &Key, n: usize, cell: &CellView, _window: &mut Window, cx: &mut Context<AppState>) -> AnyElement {
    let Some(tab) = notebooks(cx).tabs.get(key) else { return div().into_any_element() };
    let selected = tab.selected == n;
    let state = tab.run_state(cell);
    let origin = tab.runs.get(&cell.id).and_then(|r| r.1.clone());
    let editor = tab.editors.get(&cell.id).map(|e| e.0.clone());
    let collapsed = tab.collapsed.contains(&cell.id);
    let editing_md = tab.editing_md.contains(&cell.id);
    let prompt = tab
        .prompt
        .clone()
        .filter(|p| p.1 == cell.id)
        .map(|p| (p, tab.prompt_field.clone()));
    let (chip_colour, chip_word) = cells::run_chip(state);
    let count = match (cell.kind, cell.execution_count, state) {
        (CellKind::Code, _, RunState::Queued | RunState::Running) => "[*]".to_string(),
        (CellKind::Code, Some(n), _) => format!("[{n}]"),
        (CellKind::Code, None, _) => "[ ]".to_string(),
        _ => String::new(),
    };

    let gutter = div()
        .w(px(56.))
        .flex_none()
        .flex()
        .flex_col()
        .items_end()
        .pr_2()
        .gap_1()
        .text_size(theme::font(Family::Content, Role::Meta))
        .text_color(theme::text_muted())
        .font_family(theme::MONO_FONT)
        .child(count)
        .when_some(Some(state).filter(|s| *s != RunState::Idle), |d, _| {
            d.child(kit::state_chip(chip_word, chip_colour, 0.8))
        })
        .when_some(origin, |d, who| {
            d.child(div().text_color(theme::agent_controlled()).child(who))
        });

    let id = cell.id.clone();
    let source: AnyElement = match (cell.kind, editor) {
        (_, Some(editor)) => div()
            .w_full()
            .border_1()
            .border_color(if selected { theme::border_focus() } else { theme::border() })
            .bg(theme::surface())
            .font_family(theme::MONO_FONT)
            .child(Editor::new(&editor))
            .into_any_element(),
        (CellKind::Markdown, None) => {
            let (entity, k, id2) = (cx.entity(), key.clone(), id.clone());
            div()
                .id(eid("nb-md", &id))
                .w_full()
                .child(output::markdown_block(format!("nb-md-text-{id}"), if cell.source.is_empty() { "*Empty markdown cell — double-click to edit*".into() } else { cell.source.clone() }))
                .on_click(move |event, window, cx| {
                    if event.click_count() >= 2 {
                        let (k, id2) = (k.clone(), id2.clone());
                        handler(&entity, move |_this, _w, cx| {
                            with_tab(cx, &k, |t| t.editing_md.insert(id2.clone()));
                            cx.notify();
                        })(window, cx)
                    }
                })
                .into_any_element()
        }
        _ => div().into_any_element(),
    };

    let mut body = div().flex_1().min_w(px(0.)).flex().flex_col().gap_1().child(source);
    if cell.kind == CellKind::Markdown && editing_md {
        let (entity, k, id2) = (cx.entity(), key.clone(), id.clone());
        body = body.child(kit::ghost_button(eid("nb-md-done", &id), None, "Done", move |_, window, cx| {
            let (k, id2) = (k.clone(), id2.clone());
            handler(&entity, move |_this, _w, cx| {
                with_tab(cx, &k, |t| {
                    t.editing_md.remove(&id2);
                    t.editors.remove(&id2);
                });
                cx.notify();
            })(window, cx)
        }));
    }
    if !cell.outputs.is_empty() {
        if collapsed {
            body = body.child(div().text_color(theme::text_faint()).child(format!("{} outputs hidden (O)", cell.outputs.len())));
        } else {
            for (i, o) in cell.outputs.iter().enumerate() {
                body = body.child(output_view(&id, i, o, cx));
            }
        }
    }
    if let Some(((run, _, text, password), field)) = prompt {
        let field = field.unwrap_or_else(|| {
            let f = cx.new(|cx| {
                let s = InputState::new(_window, cx).placeholder(if password { "password" } else { "input" });
                if password { s.masked(true) } else { s }
            });
            with_tab(cx, key, |t| t.prompt_field = Some(f.clone()));
            f
        });
        let (entity, k, f2) = (cx.entity(), key.clone(), field.clone());
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().font_family(theme::MONO_FONT).child(text))
                .child(div().w(px(260.)).child(Input::new(&field)))
                .child(kit::primary_button(eid("nb-stdin", run), None, "Send", move |_, window, cx| {
                    let (k, f2) = (k.clone(), f2.clone());
                    handler(&entity, move |this, _w, cx| {
                        let value = f2.read(cx).value().to_string();
                        this.notebook_send(&k, NotebookRequest::InputReply { run, value });
                        with_tab(cx, &k, |t| {
                            t.prompt = None;
                            t.prompt_field = None;
                        });
                        cx.notify();
                    })(window, cx)
                })),
        );
    }

    let (entity, k) = (cx.entity(), key.clone());
    let edge = if selected { theme::accent() } else { theme::transparent() };
    kit::slab(edge)
        .id(eid("nb-row", &id))
        .flex()
        .w_full()
        .py_1()
        .pr_2()
        .when(selected, |d| d.bg(theme::hover()))
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            let k = k.clone();
            handler(&entity, move |_this, _w, cx| {
                with_tab(cx, &k, |t| t.selected = n);
                cx.notify();
            })(window, cx)
        })
        .child(gutter)
        .child(body)
        .into_any_element()
}

fn output_view(cell: &str, i: usize, o: &Output, cx: &mut Context<AppState>) -> AnyElement {
    let id = format!("nb-out-{cell}-{i}");
    let title = format!("Cell {cell} \u{b7} output {}", i + 1);
    match o {
        Output::Stream { name, text } => {
            let text = out::terminal_text(text);
            let lines = text.lines().count();
            let tone = if name == "stderr" { theme::danger() } else { theme::text() };
            output::text_block(id, title, format!("{name} \u{b7} {lines} lines"), text, tone)
        }
        Output::Error { ename, evalue, traceback } => {
            let text = out::terminal_text(&traceback.join("\n"));
            let text = if text.trim().is_empty() { format!("{ename}: {evalue}") } else { text };
            output::text_block(id, title, format!("error \u{b7} {ename}"), text, theme::danger())
        }
        Output::Data { bundle, .. } => match out::richest(bundle) {
            Some((mime @ ("image/png" | "image/jpeg" | "image/gif" | "image/svg+xml"), data)) => {
                match output::image(cx, mime, data) {
                    Some(image) => output::image_block(id, title, image, 480.),
                    None => output::placeholder(id, &[mime], None),
                }
            }
            Some(("text/markdown", md)) => output::markdown_block(id, md.to_string()),
            Some((mime, text)) => {
                let text = if mime == "text/plain" { out::terminal_text(text) } else { text.to_string() };
                output::text_block(id, title, mime.to_string(), text, theme::text())
            }
            None => {
                let mimes: Vec<&str> = bundle.iter().map(|(m, _)| m.as_str()).collect();
                let plain = bundle.iter().find(|(m, _)| m == "text/plain").map(|(_, v)| v.as_str());
                output::placeholder(id, &mimes, plain)
            }
        },
    }
}

// ── keys ───────────────────────────────────────────────────────────────────────────────────

fn selected(cx: &mut gpui::App, key: &Key) -> Option<(usize, String, CellKind, String)> {
    let t = notebooks(cx).tabs.get(key)?;
    let c = t.cells.get(t.selected)?;
    Some((t.selected, c.id.clone(), c.kind, c.source.clone()))
}

fn focus_list(key: &Key, window: &mut Window, cx: &mut gpui::App) {
    if let Some(f) = notebooks(cx).tabs.get(key).and_then(|t| t.focus.clone()) {
        window.focus(&f, cx);
    }
}

fn chord(cx: &mut gpui::App, key: &Key, c: char) -> bool {
    with_tab(cx, key, |t| {
        if t.chord == Some(c) {
            t.chord = None;
            true
        } else {
            t.chord = Some(c);
            false
        }
    })
    .unwrap_or(false)
}

fn actions(root: gpui::Stateful<gpui::Div>, key: &Key, cx: &mut Context<AppState>) -> gpui::Stateful<gpui::Div> {
    let k = key.clone();
    macro_rules! on {
        ($action:ty, $body:expr) => {{
            let k = k.clone();
            cx.listener(move |this: &mut AppState, _: &$action, window: &mut Window, cx: &mut Context<AppState>| {
                #[allow(clippy::redundant_closure_call)]
                ($body)(this, &k, window, cx);
                cx.notify();
            })
        }};
    }
    root.on_action(on!(keys::RunAdvance, |this: &mut AppState, k: &Key, window: &mut Window, cx: &mut Context<AppState>| {
        if let Some((n, id, _, _)) = selected(cx, k) {
            this.notebook_run(k, &id, cx);
            let len = notebooks(cx).tabs.get(k).map_or(0, |t| t.cells.len());
            if n + 1 >= len {
                this.notebook_insert(k, len, CellKind::Code, String::new(), cx);
            } else {
                with_tab(cx, k, |t| t.selected = n + 1);
            }
            focus_list(k, window, cx);
        }
    }))
    .on_action(on!(keys::RunInPlace, |this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        if let Some((_, id, _, _)) = selected(cx, k) {
            this.notebook_run(k, &id, cx);
        }
    }))
    .on_action(on!(keys::RunInsert, |this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        if let Some((n, id, _, _)) = selected(cx, k) {
            this.notebook_run(k, &id, cx);
            this.notebook_insert(k, n + 1, CellKind::Code, String::new(), cx);
        }
    }))
    .on_action(on!(keys::CommandMode, |_this: &mut AppState, k: &Key, window: &mut Window, cx: &mut Context<AppState>| {
        focus_list(k, window, cx);
    }))
    .on_action(on!(keys::EditMode, |_this: &mut AppState, k: &Key, window: &mut Window, cx: &mut Context<AppState>| {
        if let Some((_, id, kind, _)) = selected(cx, k) {
            if kind == CellKind::Markdown {
                with_tab(cx, k, |t| t.editing_md.insert(id.clone()));
            }
            if let Some(e) = notebooks(cx).tabs.get(k).and_then(|t| t.editors.get(&id)).map(|e| e.0.clone()) {
                let f = gpui::Focusable::focus_handle(e.read(cx), cx);
                window.focus(&f, cx);
            }
        }
    }))
    .on_action(on!(keys::InsertAbove, |this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        let n = selected(cx, k).map_or(0, |s| s.0);
        this.notebook_insert(k, n, CellKind::Code, String::new(), cx);
    }))
    .on_action(on!(keys::InsertBelow, |this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        let n = selected(cx, k).map_or(0, |s| s.0 + 1);
        this.notebook_insert(k, n, CellKind::Code, String::new(), cx);
    }))
    .on_action(on!(keys::DeleteChord, |this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        if chord(cx, k, 'd') && let Some((_, id, _, _)) = selected(cx, k) {
            this.notebook_edit(k, vec![CellOp::Delete { id }], cx);
        }
    }))
    .on_action(on!(keys::ToMarkdown, |this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        if let Some((_, id, _, _)) = selected(cx, k) {
            this.notebook_edit(k, vec![CellOp::Kind { id, kind: CellKind::Markdown }], cx);
        }
    }))
    .on_action(on!(keys::ToCode, |this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        if let Some((_, id, _, _)) = selected(cx, k) {
            this.notebook_edit(k, vec![CellOp::Kind { id, kind: CellKind::Code }], cx);
        }
    }))
    .on_action(on!(keys::ToRaw, |this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        if let Some((_, id, _, _)) = selected(cx, k) {
            this.notebook_edit(k, vec![CellOp::Kind { id, kind: CellKind::Raw }], cx);
        }
    }))
    .on_action(on!(keys::SelectNext, |_this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        with_tab(cx, k, |t| t.selected = (t.selected + 1).min(t.cells.len().saturating_sub(1)));
    }))
    .on_action(on!(keys::SelectPrev, |_this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        with_tab(cx, k, |t| t.selected = t.selected.saturating_sub(1));
    }))
    .on_action(on!(keys::CopyCell, |_this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        if let Some((_, _, kind, source)) = selected(cx, k) {
            with_tab(cx, k, |t| t.clipboard = Some((kind, source)));
        }
    }))
    .on_action(on!(keys::CutCell, |this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        if let Some((_, id, kind, source)) = selected(cx, k) {
            with_tab(cx, k, |t| t.clipboard = Some((kind, source)));
            this.notebook_edit(k, vec![CellOp::Delete { id }], cx);
        }
    }))
    .on_action(on!(keys::PasteCell, |this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        let clip = notebooks(cx).tabs.get(k).and_then(|t| t.clipboard.clone());
        if let Some((kind, source)) = clip {
            let n = selected(cx, k).map_or(0, |s| s.0 + 1);
            this.notebook_insert(k, n, kind, source, cx);
        }
    }))
    .on_action(on!(keys::UndoCells, |this: &mut AppState, k: &Key, _w: &mut Window, _cx: &mut Context<AppState>| {
        this.notebook_send(k, NotebookRequest::Undo);
    }))
    .on_action(on!(keys::InterruptChord, |this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        if chord(cx, k, 'i') {
            this.notebook_send(k, NotebookRequest::Interrupt);
        }
    }))
    .on_action(on!(keys::RestartChord, |this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        if chord(cx, k, '0') {
            this.notebook_send(k, NotebookRequest::Restart { run_all: false });
        }
    }))
    .on_action(on!(keys::ToggleOutput, |_this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        if let Some((_, id, _, _)) = selected(cx, k) {
            with_tab(cx, k, |t| {
                if !t.collapsed.remove(&id) {
                    t.collapsed.insert(id);
                }
            });
        }
    }))
    .on_action(on!(keys::ZoomCell, |_this: &mut AppState, k: &Key, _w: &mut Window, cx: &mut Context<AppState>| {
        let Some(t) = notebooks(cx).tabs.get(k) else { return };
        let Some(c) = t.cells.get(t.selected).cloned() else { return };
        let text = c
            .outputs
            .iter()
            .map(out::agent_text)
            .collect::<Vec<_>>()
            .join("\n");
        let (body, facts, lang) = if text.trim().is_empty() {
            (c.source.clone(), "source".to_string(), Some("python"))
        } else {
            (text, "outputs".to_string(), None)
        };
        cells::zoom::open_text(cx, format!("Cell {}", c.id), facts, body, lang);
    }))
    .on_action(on!(keys::SaveCells, |this: &mut AppState, k: &Key, window: &mut Window, cx: &mut Context<AppState>| {
        this.save_notebook(k, window, cx);
    }))
}

#[allow(dead_code)]
fn _focus_type(_: FocusHandle) {}
