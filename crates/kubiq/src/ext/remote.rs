//! The remote-block container: what the application settings' **Remote** section draws, per tab.
//!
//! The section is one settings entry with three tabs (`RemoteTab`): Remotes, Host mode and
//! Settings. Each tab is the column of blocks registered for it, in registration order. The base
//! registers its own — saved hosts, SSH profiles and their drones on Remotes, the drone modes on
//! Host mode, the drone list URL on Settings (`ui::settings::remote_blocks`) — and an edition adds
//! its own connection types the same way (an edition's targets on Remotes).
//!
//! A block is drawn against the same [`SectionCtx`] a settings section is, so a former section
//! moves here by changing its registration and nothing else.
//!
//! The order is resolved **once** and cached for the process, never walked in a draw path.

use std::sync::OnceLock;

use gpui::{AnyElement, Context, Window};

use super::settings::{SectionCtx, Tier};
use super::{Registry, SlotId, Slotted};
use crate::app::AppState;
pub use crate::state::settings::RemoteTab;

/// One block of the Remote section.
pub struct RemoteBlockSpec {
    pub id: SlotId,
    /// The tab it is drawn on. [`register`] files it under that tab's group.
    pub tab: RemoteTab,
    /// A tier the Experimental section has off leaves the block out.
    pub tier: Tier,
    /// Run when the section is shown — the asks worth making only because the page is on screen.
    pub on_show: Option<fn(&mut AppState, &mut Context<AppState>)>,
    /// The body: a plain column, the container owns the scroll — the block's **list page**.
    pub render: fn(&SectionCtx<'_>, &Window, &mut Context<AppState>) -> AnyElement,
    /// The block's drill-in editor, while one is open (`ui::kit::detail_page`): `Some` replaces
    /// the whole tab — the strip, the other blocks — with it. `None` for a block with no editor.
    pub detail: Option<fn(&SectionCtx<'_>, &Window, &mut Context<AppState>) -> Option<AnyElement>>,
}

impl Slotted for RemoteBlockSpec {
    fn id(&self) -> SlotId {
        self.id
    }
}

/// Registers a block under its tab. The one way in, base or second edition.
pub fn register(reg: &mut Registry<RemoteBlockSpec>, spec: RemoteBlockSpec) {
    reg.insert(spec.tab.group(), spec);
}

/// The base's own blocks.
pub fn base_registry() -> Registry<RemoteBlockSpec> {
    let mut reg = Registry::new();
    crate::ui::settings::remote_blocks(&mut reg);
    reg
}

static RESOLVED: OnceLock<Vec<&'static RemoteBlockSpec>> = OnceLock::new();

/// Installs the composed registry, before the first window. The base binary never calls this:
/// nothing installed resolves [`base_registry`] instead.
///
/// # Panics
/// If the order has already been resolved.
pub fn install(reg: Registry<RemoteBlockSpec>) {
    if RESOLVED.set(resolve(reg)).is_err() {
        panic!("ext::remote: the remote-block container was already resolved before install");
    }
}

fn resolve(reg: Registry<RemoteBlockSpec>) -> Vec<&'static RemoteBlockSpec> {
    // Leaked deliberately and exactly once, the way the other containers' are.
    let reg: &'static mut Registry<RemoteBlockSpec> = Box::leak(Box::new(reg));
    let groups: Vec<SlotId> = RemoteTab::ALL.iter().map(|tab| tab.group()).collect();
    Registry::resolve(reg, &groups)
}

fn resolved() -> &'static [&'static RemoteBlockSpec] {
    RESOLVED.get_or_init(|| resolve(base_registry()))
}

/// Every block, in tab then registration order.
pub fn blocks() -> &'static [&'static RemoteBlockSpec] {
    resolved()
}

/// The blocks one tab draws, in order.
pub fn tab_blocks(tab: RemoteTab) -> impl Iterator<Item = &'static RemoteBlockSpec> {
    resolved()
        .iter()
        .copied()
        .filter(move |spec| spec.tab == tab)
}
