//! The Settings tab: the whole list — driver, planner, worker, coordinator (under the
//! coordinator driver), parallel, approve.
//!
//! **Every control is one `SetMissionField`.** The window asks, the host writes, and
//! `MissionChanged` comes back — nothing here edits the settings it is drawing.
//!
//! **Each setting says what it does in a sentence the user can act on.**

use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, px};

use kubiq_proto::ids::TaskId;
use kubiq_proto::mission::{Driver, MissionField, MissionSettings};
use kubiq_proto::work::AgentId;

use crate::app::AppState;
use crate::theme;
use crate::ui::eid2;
use crate::ui::kit::{check_box, choice_pill, setting_row};

/// The whole form.
pub fn render(
    app: &AppState,
    task_id: TaskId,
    settings: &MissionSettings,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let definitions: Vec<String> = app
        .workbench
        .settings
        .definitions_in(app.project(cx))
        .into_iter()
        .filter(|definition| !definition.disabled)
        .map(|definition| definition.id)
        .collect();

    let mut list = vec![
        setting_row(
            "Driver",
            "Auto starts ready tasks by itself until the next human task or approval. Manual \
             lists them under Next and waits for you to press \u{25b6}. Coordinator hands them \
             to a coordinator agent, which is told as each one finishes. Changeable mid-run.",
            pills(
                ("mission-driver", task_id),
                Driver::all(),
                settings.driver,
                Driver::label,
                MissionField::Driver,
                cx,
            ),
        ),
        setting_row(
            "Planner",
            "The agent that writes the plan and drafts the tasks, and that a replan goes back \
                 to. Default is the project's planner.",
            definition_pills(
                ("mission-planner", task_id),
                &definitions,
                settings.planner.as_ref(),
                MissionField::Planner,
                cx,
            ),
        ),
        setting_row(
            "Worker",
            "The agent started on each task, unless the task names one of its own. Default is \
                 the project's worker.",
            definition_pills(
                ("mission-worker", task_id),
                &definitions,
                settings.worker.as_ref(),
                MissionField::Worker,
                cx,
            ),
        ),
    ];
    if settings.driver == Driver::Coordinator {
        list.push(setting_row(
            "Coordinator",
            "The agent opened when the mission runs, to start its tasks. Default is the \
             project's coordinator.",
            definition_pills(
                ("mission-coordinator", task_id),
                &definitions,
                settings.coordinator.as_ref(),
                MissionField::Coordinator,
                cx,
            ),
        ));
        list.push(setting_row(
            "Coordinator agent",
            "An open agent to coordinate instead: it is given the coordinator's brief and told \
             as each task finishes. The one the mission opened is listed here too.",
            agent_pills(
                task_id,
                &app.coordinator_candidates(task_id),
                settings.coordinator_agent,
                cx,
            ),
        ));
    }
    list.extend([
        setting_row(
            "Parallel",
            "How many workers may run at once in auto mode, or the coordinator's limit. Each \
                 one costs an agent.",
            count(
                ("mission-parallel", task_id),
                settings.parallelism(),
                1,
                12,
                MissionField::Parallel,
                cx,
            ),
        ),
        setting_row(
            "Approve each task",
            "The default for new subtasks: a finished task waits for your Confirm before \
                 anything that depends on it starts.",
            switch(
                task_id,
                eid2("mission-set", task_id, "approve"),
                settings.approve,
                MissionField::Approve,
                cx,
            ),
        ),
    ]);

    div().flex().flex_col().child(rows(list)).into_any_element()
}

/// *none* plus one pill per open agent of the project; the held coordinator reads even when it is
/// not open now.
fn agent_pills(
    task_id: TaskId,
    candidates: &[(AgentId, String)],
    held: Option<AgentId>,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let mut pills: Vec<AnyElement> = vec![
        choice_pill(
            eid2("mission-coordinator-agent", task_id, "none"),
            "none",
            held.is_none(),
            cx.listener(move |this, _, _, cx| {
                this.set_mission_field(task_id, MissionField::CoordinatorAgent(None), cx)
            }),
        )
        .into_any_element(),
    ];
    let mut names: Vec<(AgentId, String)> = candidates.to_vec();
    if let Some(held) = held
        && !names.iter().any(|(id, _)| *id == held)
    {
        names.push((held, "opened (stopped)".to_string()));
    }
    for (agent, name) in names {
        pills.push(
            choice_pill(
                eid2("mission-coordinator-agent", task_id, agent.to_string()),
                name,
                held == Some(agent),
                cx.listener(move |this, _, _, cx| {
                    this.set_mission_field(task_id, MissionField::CoordinatorAgent(Some(agent)), cx)
                }),
            )
            .into_any_element(),
        );
    }
    div()
        .flex()
        .flex_wrap()
        .flex_none()
        .items_center()
        .gap_1p5()
        .children(pills)
        .into_any_element()
}

/// The column the rows sit in. `setting_row` draws its own rule and its own vertical rhythm, so
/// all this owes it is the padding the rest of the view has.
fn rows(children: Vec<AnyElement>) -> AnyElement {
    div()
        .px_3()
        .flex()
        .flex_none()
        .flex_col()
        .children(children)
        .into_any_element()
}

/// A yes/no setting: the tick box, written the moment it is clicked.
fn switch(
    task_id: TaskId,
    id: gpui::ElementId,
    on: bool,
    field: impl Fn(bool) -> MissionField + 'static,
    cx: &mut Context<AppState>,
) -> AnyElement {
    check_box(
        id,
        on,
        cx.listener(move |this, _, _, cx| this.set_mission_field(task_id, field(!on), cx)),
    )
    .into_any_element()
}

/// One of a short set of values, exactly one lit.
fn pills<T: Copy + PartialEq + 'static>(
    id: (&'static str, TaskId),
    values: impl IntoIterator<Item = T>,
    held: T,
    label: impl Fn(T) -> &'static str + 'static,
    field: impl Fn(T) -> MissionField + Copy + 'static,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let task_id = id.1;
    let pills: Vec<AnyElement> = values
        .into_iter()
        .map(|value| {
            let text = label(value);
            choice_pill(
                eid2(id.0, id.1, text),
                text,
                value == held,
                cx.listener(move |this, _, _, cx| {
                    this.set_mission_field(task_id, field(value), cx)
                }),
            )
            .into_any_element()
        })
        .collect();

    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .children(pills)
        .into_any_element()
}

/// A whole number between two nudges, clamped to what the field can mean.
fn count(
    id: (&'static str, TaskId),
    held: usize,
    least: usize,
    most: usize,
    field: impl Fn(usize) -> MissionField + Copy + 'static,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let task_id = id.1;
    let down = held.saturating_sub(1).max(least);
    let up = (held + 1).min(most);

    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .child(choice_pill(
            eid2(id.0, id.1, "down"),
            "\u{2212}",
            false,
            cx.listener(move |this, _, _, cx| this.set_mission_field(task_id, field(down), cx)),
        ))
        .child(
            div()
                .w(px(34.))
                .flex()
                .flex_none()
                .justify_center()
                .child(crate::ui::kit::mono(held.to_string(), theme::text())),
        )
        .child(choice_pill(
            eid2(id.0, id.1, "up"),
            "+",
            false,
            cx.listener(move |this, _, _, cx| this.set_mission_field(task_id, field(up), cx)),
        ))
        .into_any_element()
}

/// *Project default* plus one pill per agent definition; exactly one lit.
fn definition_pills(
    id: (&'static str, TaskId),
    definitions: &[String],
    held: Option<&String>,
    field: impl Fn(Option<String>) -> MissionField + Copy + 'static,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let task_id = id.1;
    let mut pills: Vec<AnyElement> = vec![
        choice_pill(
            eid2(id.0, id.1, "default"),
            "default",
            held.is_none(),
            cx.listener(move |this, _, _, cx| this.set_mission_field(task_id, field(None), cx)),
        )
        .into_any_element(),
    ];
    // A held value whose definition is gone still reads, rather than the row looking unset.
    let mut names: Vec<&String> = definitions.iter().collect();
    if let Some(held) = held
        && !names.contains(&held)
    {
        names.push(held);
    }
    for name in names {
        let pick = name.clone();
        pills.push(
            choice_pill(
                eid2(id.0, id.1, name.clone()),
                name.clone(),
                held == Some(name),
                cx.listener(move |this, _, _, cx| {
                    this.set_mission_field(task_id, field(Some(pick.clone())), cx)
                }),
            )
            .into_any_element(),
        );
    }

    div()
        .flex()
        .flex_none()
        .flex_wrap()
        .items_center()
        .gap_1p5()
        .children(pills)
        .into_any_element()
}
