//! The onboarding flows the base ships, and their words.
//!
//! A flow is data (`crate::state::onboarding`); this is the one list of them, built on demand so a
//! flow can depend on the window — the IDE walk visits exactly the modes the rail is drawing.
//! Adding a flow or a step is an entry here; nothing else changes.
//!
//! The markdown is authored in the help tree, under `help/onboarding/`, as `hidden: true` pages:
//! the packer validates them and keeps them out of the nav, the catalogue and the bundle, and they
//! are compiled in here, so a step never waits for — or lacks — a help bundle. The dialog drops
//! their frontmatter.

use gpui::App;

use crate::app::AppState;
use crate::state::RailMode;
use crate::state::onboarding::{Flow, FormKind, IDE, Step, StepAction, TOUR, Trigger, WELCOME};
use crate::state::ui_id;

const WELCOME_MD: &str = include_str!("../../../../../help/onboarding/welcome.md");
const TOUR_MD: &str = include_str!("../../../../../help/onboarding/tour.md");
const DONE_MD: &str = include_str!("../../../../../help/onboarding/done.md");
const IDE_MD: &str = include_str!("../../../../../help/onboarding/ide.md");
const IDE_DONE_MD: &str = include_str!("../../../../../help/onboarding/ide-done.md");

/// Every flow, in the order the launch check considers them.
pub fn all(app: &AppState, cx: &App) -> Vec<Flow> {
    let mut welcome = welcome();
    // The Default agent step closes the welcome, and only while none is configured.
    if !app.default_agent_configured() {
        welcome
            .steps
            .push(Step::form("Default agent", FormKind::DefaultAgent));
    }
    vec![welcome, tour(), ide(app, cx)]
}

/// The first run: what Kubiq is, the colours, the tour as one step, and the way on.
fn welcome() -> Flow {
    Flow {
        id: WELCOME,
        title: "Welcome".into(),
        trigger: Trigger::Start,
        then: None,
        steps: vec![
            Step::markdown("Welcome", WELCOME_MD).naming([ui_id::TITLEBAR_OVERFLOW]),
            Step::form("Your colours", FormKind::Palette),
            Step::sub_flow("A quick tour", TOUR, TOUR_MD, "Start the tour"),
            Step::markdown("You're set", DONE_MD).naming([
                ui_id::TITLEBAR_POINT_AT,
                ui_id::TITLEBAR_HELP,
                ui_id::TITLEBAR_OVERFLOW,
            ]),
        ],
    }
}

/// One titlebar button, in the words in-place help already has for it — one sentence per control,
/// whichever way the reader arrives at it.
fn button(id: ui_id::UiId) -> Step {
    let (label, blurb) = ui_id::describe(&id).map_or(("", ""), |info| (info.label, info.blurb));
    Step::highlight(label, id, blurb)
}

/// The main screen's areas and every titlebar button, one highlight each — run as a sub-flow of
/// the welcome.
fn tour() -> Flow {
    Flow {
        id: TOUR,
        title: "Tour".into(),
        trigger: Trigger::Manual,
        then: None,
        steps: vec![
            Step::highlight(
                "The rail",
                ui_id::RAIL,
                "Modes at the top change what the window shows; the badges under them are the \
                 projects open in this window.",
            ),
            Step::highlight(
                "Projects",
                ui_id::TITLEBAR_PROJECT,
                "The project on screen. Click it to switch to another, or to open one that is not \
                 here yet.",
            ),
            Step::highlight(
                "Add a project",
                ui_id::TITLEBAR_NEW_PROJECT,
                "Open a folder on this machine as a project in this window. The chevron beside \
                 it clones a repository or opens a project on a remote host.",
            )
            .naming([ui_id::TITLEBAR_NEW_PROJECT_MENU]),
            Step::highlight(
                "Back and forward",
                ui_id::TITLEBAR_NAV_BACK,
                "Return to where this window was standing — the mode, the tab and the place in it \
                 — and undo that step again (⌃- and ⌃⇧-).",
            )
            .naming([ui_id::TITLEBAR_NAV_FORWARD]),
            Step::highlight(
                "The command field",
                ui_id::TITLEBAR_COMMAND,
                "One field for finding a file, searching the project and running a command — ⌘P \
                 to go to a file, ⌘⏎ to search.",
            ),
            Step::highlight(
                "Panel switches",
                ui_id::TITLEBAR_REGION_LEFT,
                "Show or hide the panels on the window's left, along its bottom and on its right.",
            )
            .naming([ui_id::TITLEBAR_REGION_BOTTOM, ui_id::TITLEBAR_REGION_RIGHT]),
            Step::highlight(
                "New agent",
                ui_id::TITLEBAR_NEW_AGENT,
                "Start an agent: harness, account, model and permission mode in one step. Its \
                 chevron lists the agents whose tab you hid. Shown once a project is open.",
            )
            .naming([ui_id::TITLEBAR_HIDDEN_AGENTS]),
            Step::highlight(
                "New terminal",
                ui_id::TITLEBAR_NEW_TERMINAL,
                "A shell in the project's folder, in a pane of its own; the chevron runs the other \
                 shells and harnesses this machine offers. Shown once a project is open.",
            )
            .naming([ui_id::TITLEBAR_NEW_TERMINAL_MENU]),
            button(ui_id::TITLEBAR_SEARCH),
            Step::highlight(
                "Notifications",
                ui_id::TITLEBAR_NOTIFICATIONS,
                "What agents and the host have reported. A count means something is waiting for \
                 you.",
            ),
            button(ui_id::TITLEBAR_FEEDBACK),
            button(ui_id::TITLEBAR_HELP),
            Step::highlight(
                "Click for instant help",
                ui_id::TITLEBAR_POINT_AT,
                "Click it, then point at any control: a box marks it and a balloon says what it \
                 does. Click it again or press Esc to leave — ⇧F1 does the same from the keyboard.",
            ),
            button(ui_id::TITLEBAR_TOOLS),
            Step::highlight(
                "Settings and More",
                ui_id::TITLEBAR_SETTINGS,
                "Accounts, agents, connectors and the rest of Kubiq's settings. The chevron \
                 beside it, More, holds remote hosts, export, capture, help and Show onboarding.",
            )
            .naming([ui_id::TITLEBAR_OVERFLOW]),
            button(ui_id::TITLEBAR_THEME),
            button(ui_id::TITLEBAR_ABOUT),
            Step::highlight(
                "The workspace",
                ui_id::WORKSPACE,
                "The mode's own screen, with panels docked around it. Drag a panel's tab to move \
                 it; the titlebar's panel switches show and hide the edges.",
            )
            .naming([
                ui_id::TITLEBAR_REGION_LEFT,
                ui_id::TITLEBAR_REGION_BOTTOM,
                ui_id::TITLEBAR_REGION_RIGHT,
            ]),
            Step::highlight(
                "The status bar",
                ui_id::STATUS_BAR,
                "Counts for the mode you are in, the update when one is ready, and the size \
                 control for the whole window.",
            ),
        ],
    }
}

/// The first visit to the IDE with a project: the rail, then every project mode it draws.
fn ide(app: &AppState, cx: &App) -> Flow {
    let mut steps = vec![
        Step::markdown("Project modes", IDE_MD),
        Step::highlight(
            "Modes",
            ui_id::RAIL_MODES,
            "Every screen this project offers. Click one, or press ⌃ and its number.",
        )
        .entering(StepAction::EnterMode(RailMode::IDE)),
    ];
    steps.extend(
        RailMode::project_modes()
            .filter(|mode| {
                mode.spec().is_some_and(|spec| spec.needs_project) && app.mode_enabled(*mode, cx)
            })
            .map(|mode| {
                let id = ui_id::rail_mode(mode);
                let text = ui_id::describe(&id)
                    .map(|info| info.blurb)
                    .unwrap_or_else(|| mode.note());
                Step::highlight(mode.label(), id, text).entering(StepAction::EnterMode(mode))
            }),
    );
    steps.push(
        Step::markdown("Back to the IDE", IDE_DONE_MD)
            .entering(StepAction::EnterMode(RailMode::IDE)),
    );
    Flow {
        id: IDE,
        title: "Project modes".into(),
        trigger: Trigger::EnterMode(RailMode::IDE),
        then: None,
        steps,
    }
}
