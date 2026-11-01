//! The prompt for project settings changed outside Kubiq (`D230`): a strip above the dock, for
//! the project on screen, that lists every project-layer definition and catalog MCP this machine
//! does not trust and asks before it is used.
//!
//! It is not a modal: the work underneath goes on, and a refused launch or a hidden server is what
//! sends the user here. Approving sends back the fingerprint each row was shown with, so the host
//! trusts exactly what the user saw; the wording says what that grants — a definition's write
//! access, or running an MCP's command.

use gpui::{AnyElement, Context, IntoElement, ParentElement, SharedString, Styled, div, px};
use gpui_component::{Icon, IconName, Sizable as _, Size};

use crate::app::AppState;
use crate::theme;
use crate::theme::{Family, Role};
use crate::ui::eid2;
use crate::ui::kit::{ghost_button, primary_button};
use kubiq_proto::projects::{TrustItem, TrustKind, TrustReason, UntrustedItem};

/// What one row calls its item.
fn label(item: &UntrustedItem) -> String {
    let kind = match item.kind {
        TrustKind::Definition => "Agent definition",
        TrustKind::Mcp => "MCP server",
    };
    let reason = match item.reason {
        TrustReason::Changed => "changed",
        TrustReason::New => "new",
    };
    format!("{kind} {} \u{2014} {reason}", item.id)
}

/// The strip, or nothing when the project on screen trusts everything it holds.
pub fn strip(app: &AppState, cx: &mut Context<AppState>) -> Option<AnyElement> {
    // The project this strip is drawn for, captured by every listener below: a click lands on
    // this project's items even if another one is on screen by then.
    let project_id = app.project(cx)?;
    let items = app.open_project(cx)?.untrusted.clone();
    if items.is_empty() {
        return None;
    }
    let rows = items.iter().enumerate().map(|(index, item)| {
        let target = TrustItem {
            kind: item.kind,
            id: item.id.clone(),
            fingerprint: item.fingerprint.clone(),
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(theme::font(Family::Chrome, Role::Label))
                    .text_color(theme::text())
                    .child(SharedString::from(label(item))),
            )
            .child(ghost_button(
                eid2("trust-item", index, &item.id),
                None,
                "Approve",
                cx.listener(move |this, _, _, cx| {
                    this.trust_project_items(project_id, Some(vec![target.clone()]), cx)
                }),
            ))
    });
    Some(
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .py_1()
            .bg(theme::warning_soft())
            .border_l(px(theme::accent_edge()))
            .border_color(theme::warning())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .with_size(Size::XSmall)
                            .text_color(theme::warning()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(theme::font(Family::Chrome, Role::Label))
                            .text_color(theme::text())
                            .child("Project settings changed outside Kubiq"),
                    )
                    .child(primary_button(
                        "trust-all",
                        None,
                        "Approve all",
                        cx.listener(move |this, _, _, cx| {
                            this.trust_project_items(project_id, None, cx)
                        }),
                    )),
            )
            .child(
                div()
                    .text_size(theme::font(Family::Chrome, Role::Meta))
                    .text_color(theme::text_muted())
                    .child(
                        "Approving lets an agent definition keep the write access it asks for, \
                         and lets an MCP server's command run on this machine. Check the \
                         project's .kubiq folder first.",
                    ),
            )
            .children(rows)
            .into_any_element(),
    )
}
