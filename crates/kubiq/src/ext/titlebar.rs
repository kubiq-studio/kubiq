//! The titlebar-item container: controls an edition adds to the titlebar's left group.
//!
//! Each item is a function asked on every titlebar draw; it answers `None` when it has nothing to
//! show (most of the time — an edition's buttons for one kind of project exist only while the
//! active project is of that kind). The titlebar draws the answers in registration order after the remote-host split
//! button, each behind its own rule. Empty in the base.
//!
//! The order is resolved **once** and cached for the process, never walked in a draw path.

use std::sync::OnceLock;

use gpui::{AnyElement, Context, Window};

use super::{Registry, SlotId, Slotted, ids};
use crate::app::AppState;

/// The one group an item is registered in.
pub const GROUPS: &[SlotId] = &[ids::TITLEBAR_ITEM];

/// One contributed titlebar control (or a run of them in one element).
pub struct TitlebarItemSpec {
    pub id: SlotId,
    /// The control, or `None` to draw nothing. Takes the titlebar row's full height.
    pub render: fn(&AppState, &Window, &mut Context<AppState>) -> Option<AnyElement>,
}

impl Slotted for TitlebarItemSpec {
    fn id(&self) -> SlotId {
        self.id
    }
}

/// Registers an item. The one way in.
pub fn register(reg: &mut Registry<TitlebarItemSpec>, spec: TitlebarItemSpec) {
    reg.insert(ids::TITLEBAR_ITEM, spec);
}

/// The base's own items: none.
pub fn base_registry() -> Registry<TitlebarItemSpec> {
    Registry::new()
}

static RESOLVED: OnceLock<Vec<&'static TitlebarItemSpec>> = OnceLock::new();

/// Installs the composed registry, before the first window. The base binary never calls this.
///
/// # Panics
/// If the order has already been resolved.
pub fn install(reg: Registry<TitlebarItemSpec>) {
    if RESOLVED.set(resolve(reg)).is_err() {
        panic!("ext::titlebar: the titlebar-item container was already resolved before install");
    }
}

fn resolve(reg: Registry<TitlebarItemSpec>) -> Vec<&'static TitlebarItemSpec> {
    let reg: &'static mut Registry<TitlebarItemSpec> = Box::leak(Box::new(reg));
    Registry::resolve(reg, GROUPS)
}

/// Every registered item, in draw order.
pub fn items() -> &'static [&'static TitlebarItemSpec] {
    RESOLVED.get_or_init(|| resolve(base_registry()))
}
