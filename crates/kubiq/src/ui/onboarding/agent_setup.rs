//! The onboarding's **Default agent** form: one pill per harness installed here, then one per
//! model the picked harness lists, and where to change it later. Passing the step confirms
//! (`AppState::commit_agent_setup`); the picks are `crate::state::agent_setup`.

use gpui::{
    AnyElement, Context, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::{Icon, Sizable as _, Size};

use crate::app::AppState;
use crate::state::agent_setup::AgentSetup;
use crate::theme::{self, Family, Role};
use crate::ui::kit::{KubiqIcon, choice_pill, modal_note};

pub fn form(app: &AppState, cx: &mut Context<AppState>) -> AnyElement {
    let setup = &app.workbench.agent_setup;
    let installed: Vec<_> = AgentSetup::installed(&app.workbench.agent_types).collect();

    let mut body = div().pt_3().flex().flex_col().gap_3();
    if installed.is_empty() {
        let known: Vec<&str> = app
            .workbench
            .agent_types
            .iter()
            .filter(|it| it.chat)
            .map(|it| it.label.as_str())
            .collect();
        let supported = if known.is_empty() {
            String::new()
        } else {
            format!(" Supported: {}.", known.join(", "))
        };
        body = body
            .child(
                div()
                    .text_size(theme::font(Family::Chrome, Role::Body))
                    .text_color(theme::warning())
                    .child("No agent harness installed: none was found on this machine's PATH."),
            )
            .child(modal_note(&format!(
                "Install one with its own installer, or give its command in Settings → \
                 Harnesses, then pick it in Settings → Agent definitions.{supported}"
            )));
    } else {
        body = body
            .child(modal_note(
                "The harness and model the Coordinator, Worker and Kubiq helper agents run on.",
            ))
            .child(label("Harness"))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .children(installed.iter().map(|it| {
                        let id = it.id.clone();
                        choice_pill(
                            ElementId::Name(format!("onboarding-harness-{id}").into()),
                            it.label.clone(),
                            setup.harness.as_deref() == Some(id.as_str()),
                            cx.listener(move |this, _, _, cx| {
                                this.agent_setup_pick_harness(&id, cx)
                            }),
                        )
                        .into_any_element()
                    })),
            )
            .child(label("Model"))
            .child(models(setup, cx));
    }
    body = body.child(change_later());

    div()
        .id("onboarding-agent-setup")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .child(body)
        .into_any_element()
}

fn models(setup: &AgentSetup, cx: &mut Context<AppState>) -> AnyElement {
    if setup.probing {
        return modal_note("Reading the harness's models…").into_any_element();
    }
    let default = choice_pill(
        "onboarding-model-default",
        "Harness default",
        setup.model.is_none(),
        cx.listener(|this, _, _, cx| this.agent_setup_pick_model(None, cx)),
    )
    .into_any_element();
    let pills = setup.models.iter().map(|model| {
        let id = model.id.clone();
        let tip: SharedString = model.description.clone().unwrap_or(id.clone()).into();
        choice_pill(
            ElementId::Name(format!("onboarding-model-{id}").into()),
            id.clone(),
            setup.model.as_deref() == Some(id.as_str()),
            cx.listener(move |this, _, _, cx| this.agent_setup_pick_model(Some(id.clone()), cx)),
        )
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
        })
        .into_any_element()
    });
    div()
        .flex()
        .flex_wrap()
        .gap_1()
        .child(default)
        .children(pills)
        .into_any_element()
}

fn label(text: &'static str) -> impl IntoElement {
    div()
        .text_size(theme::font(Family::Chrome, Role::Label))
        .text_color(theme::text())
        .child(text)
}

/// "Change it later in ⚙ Settings → Agent definitions", with the titlebar's own settings mark.
fn change_later() -> impl IntoElement {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .text_size(theme::font(Family::Chrome, Role::Body))
        .text_color(theme::text_muted())
        .child("Change it later in")
        .child(
            Icon::new(KubiqIcon::TitlebarSettings)
                .with_size(Size::XSmall)
                .text_color(theme::text()),
        )
        .child(
            div()
                .text_color(theme::text())
                .child("Settings → Agent definitions"),
        )
        .child("— each agent there names its own harness and model.")
}
