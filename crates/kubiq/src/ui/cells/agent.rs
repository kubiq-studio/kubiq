//! The bound-agent control a cell document's header carries: which of the project's agents hears
//! this document's events (a run it asked for finished, a run approved, a question on a cell).
//! Binding decides only who is told — every hosted agent may use the document's MCP server.
//!
//! A chip naming the bound agent; expanded, one row per agent in the project, Unbind, and New
//! agent… (the new-agent dialog; bind it from this list once it runs).

use std::rc::Rc;

use gpui::{AnyElement, Context, IntoElement, ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div};
use gpui_component::IconName;
use kubiq_proto::work::AgentId;

use crate::app::AppState;
use crate::theme::{self, Family, Role};
use crate::ui::{eid, handler, kit};

pub type OnPick = Rc<dyn Fn(&mut AppState, Option<AgentId>, &mut Context<AppState>)>;

/// A *New agent…* from a document's control: the binding owed to the agent the dialog starts,
/// taken by `AppState::start_new_agent` once its id exists — the doc-binding precedent of
/// annotation mode's *New agent…*.
#[derive(Default)]
pub struct PendingBind(pub Option<OnPick>);

impl gpui::Global for PendingBind {}

/// The binding owed to a just-minted agent, if a document's *New agent…* asked for one.
pub fn take_pending(cx: &mut gpui::App) -> Option<OnPick> {
    cx.default_global::<PendingBind>().0.take()
}

/// The chip, and when `expanded` the list under it. `on_toggle` opens and closes the list.
pub fn bound_agent(
    app: &AppState,
    id: &str,
    bound: Option<AgentId>,
    expanded: bool,
    on_toggle: Rc<dyn Fn(&mut AppState, &mut Window, &mut Context<AppState>)>,
    on_pick: OnPick,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let agents: Vec<(AgentId, String)> = app
        .open_project(cx)
        .map(|open| {
            open.work
                .agents
                .iter()
                .map(|a| (a.id, a.title_or_handle().to_string()))
                .collect()
        })
        .unwrap_or_default();
    let name = bound
        .and_then(|b| agents.iter().find(|(id, _)| *id == b).map(|(_, n)| n.clone()))
        .unwrap_or_else(|| if bound.is_some() { "agent".into() } else { "No agent".into() });
    let entity = cx.entity();
    let toggle = on_toggle.clone();
    let chip = kit::ghost_button(
        eid(id, "bound"),
        Some(IconName::Bot),
        format!("Bound: {name}"),
        move |_, window, cx| handler(&entity, {
            let toggle = toggle.clone();
            move |this, window, cx| toggle(this, window, cx)
        })(window, cx),
    )
    .tooltip(|window, cx| {
        gpui_component::tooltip::Tooltip::new("The agent told about this document's events").build(window, cx)
    })
    .into_any_element();
    if !expanded {
        return chip;
    }
    let mut rows: Vec<AnyElement> = Vec::new();
    for (agent, label) in agents {
        let pick = on_pick.clone();
        let entity = cx.entity();
        rows.push(
            kit::choice_pill(eid(id, agent), label, Some(agent) == bound, move |_, window, cx| {
                let pick = pick.clone();
                handler(&entity, move |this, _window, cx| pick(this, Some(agent), cx))(window, cx)
            })
            .into_any_element(),
        );
    }
    if bound.is_some() {
        let pick = on_pick.clone();
        let entity = cx.entity();
        rows.push(
            kit::ghost_button(eid(id, "unbind"), None, "Unbind", move |_, window, cx| {
                let pick = pick.clone();
                handler(&entity, move |this, _window, cx| pick(this, None, cx))(window, cx)
            })
            .into_any_element(),
        );
    }
    let entity = cx.entity();
    let pick = on_pick.clone();
    rows.push(
        kit::ghost_button(eid(id, "new"), Some(IconName::Plus), "New agent…", move |_, window, cx| {
            let pick = pick.clone();
            handler(&entity, move |this, window, cx| {
                // Bound once the dialog starts it (`take_pending`).
                cx.default_global::<PendingBind>().0 = Some(pick.clone());
                this.open_new_agent(Default::default(), window, cx);
            })(window, cx)
        })
        .into_any_element(),
    );
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(chip)
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .text_size(theme::font(Family::Chrome, Role::Label))
                .children(rows),
        )
        .into_any_element()
}
