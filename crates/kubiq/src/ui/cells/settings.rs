//! The "Cells" settings section (App > System): the preferences of the cell documents, notebooks
//! and runbooks, in one place (`state::cells::CellsSettings`, kept in the interface's settings
//! blob). Each kind contributes its rows to [`render`]; the runbook's are [`runbook_rows`].

use gpui::{AnyElement, Context, IntoElement, ParentElement as _, Styled as _, Window, div};
use gpui_component::{Icon, IconName};
use kubiq_proto::notebook::RunPolicy;

use crate::app::AppState;
use crate::ext::settings::{SectionCtx, SectionGate, SettingsContainer, SettingsSectionSpec};
use crate::ext::{Registry, ids};
use crate::state::cells::OUTPUT_LINES;
use crate::ui::eid;
use crate::ui::kit::{check_box, choice_pill, column, heading, setting_row};

pub fn register(reg: &mut Registry<SettingsSectionSpec>) {
    crate::ext::settings::register(
        reg,
        SettingsSectionSpec {
            id: ids::CELLS_SETTINGS,
            container: SettingsContainer::App,
            group: ids::SETTINGS_APP_SYSTEM,
            label: "Cells",
            icon: || Icon::new(IconName::SquareTerminal),
            gate: SectionGate::Always,
            tier: crate::ext::settings::Tier::Stable,
            keywords: &[
                "notebook", "jupyter", "kernel", "runbook", "cells", "outputs", "agent policy",
            ],
            count: None,
            on_show: None,
            render,
        },
    );
}

fn pills(items: Vec<AnyElement>) -> AnyElement {
    div().flex().gap_1().children(items).into_any_element()
}

fn render(ctx: &SectionCtx<'_>, window: &Window, cx: &mut Context<AppState>) -> AnyElement {
    let settings = ctx.app.cells_settings().clone();
    let policy: Vec<AnyElement> = [
        ("Ask", RunPolicy::Ask),
        ("Allow", RunPolicy::Allow),
        ("Deny", RunPolicy::Deny),
    ]
    .into_iter()
    .map(|(label, value)| {
        choice_pill(
            eid("cells-policy", label),
            label,
            settings.notebook_policy == value,
            cx.listener(move |app, _, _window, cx| {
                app.update_cells_settings(|s| s.notebook_policy = value, cx);
            }),
        )
        .into_any_element()
    })
    .collect();
    let lines: Vec<AnyElement> = OUTPUT_LINES
        .into_iter()
        .map(|n| {
            choice_pill(
                eid("cells-lines", n),
                n.to_string(),
                settings.output_lines == n,
                cx.listener(move |app, _, _window, cx| {
                    app.update_cells_settings(|s| s.output_lines = n, cx);
                }),
            )
            .into_any_element()
        })
        .collect();
    let mut rows = vec![
        heading(
            "Cells",
            "Notebooks and runbooks: how agents may run their cells, and how outputs are kept and \
             drawn. Kept in the interface's settings, not in the project.",
        ),
        setting_row(
            "Notebook agent policy",
            "What a notebook allows an agent's runs until it is given its own: ask in the \
             notebook's header, allow, or deny. A harness's permission mode never implies allow.",
            pills(policy),
        ),
        setting_row(
            "Save notebooks without outputs",
            "Writes cells without their outputs and execution counts — smaller files and clean \
             diffs. The open notebook keeps its outputs.",
            check_box(
                "cells-strip",
                settings.strip_outputs,
                cx.listener(|app, _, _window, cx| {
                    app.update_cells_settings(|s| s.strip_outputs = !s.strip_outputs, cx);
                }),
            )
            .into_any_element(),
        ),
        setting_row(
            "Output lines in a cell",
            "Lines of a text output drawn in place; the zoom shows all of it.",
            pills(lines),
        ),
    ];
    rows.extend(runbook_rows(ctx, window, cx));
    column(rows)
}

/// The runbook's rows (`CellsSettings::runbook`). They reach the host with each run
/// (`RunOptions`); a document's own `timeout=` wins over the default timeout.
pub fn runbook_rows(ctx: &SectionCtx<'_>, _window: &Window, cx: &mut Context<AppState>) -> Vec<AnyElement> {
    let runbook = ctx.app.cells_settings().runbook.clone();
    let caps: Vec<AnyElement> = [("Default (16 MiB)", None), ("4 MiB", Some(4)), ("64 MiB", Some(64)), ("256 MiB", Some(256))]
        .into_iter()
        .map(|(label, value)| {
            choice_pill(
                eid("cells-rb-cap", label),
                label,
                runbook.log_cap_mib == value,
                cx.listener(move |app, _, _window, cx| {
                    app.update_cells_settings(|s| s.runbook.log_cap_mib = value, cx);
                }),
            )
            .into_any_element()
        })
        .collect();
    let timeouts: Vec<AnyElement> = [("None", None), ("1 min", Some(60)), ("10 min", Some(600)), ("1 h", Some(3600))]
        .into_iter()
        .map(|(label, value)| {
            choice_pill(
                eid("cells-rb-timeout", label),
                label,
                runbook.timeout_secs == value,
                cx.listener(move |app, _, _window, cx| {
                    app.update_cells_settings(|s| s.runbook.timeout_secs = value, cx);
                }),
            )
            .into_any_element()
        })
        .collect();
    vec![
        setting_row(
            "Runbook log cap",
            "How much of one run's output is kept in its log; past it the log says it was capped \
             and the run goes on. The excerpt in the document is always the first and last lines.",
            pills(caps),
        ),
        setting_row(
            "Runbook cell timeout",
            "How long a cell that names no timeout= may run before it is stopped.",
            pills(timeouts),
        ),
    ]
}
