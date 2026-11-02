//! The drone-mode-kind container: what the "Configure drone mode" dialog offers besides `http`.
//!
//! The base's one kind, `http`, is built into the dialog. An edition that registers a further
//! endpoint on the host side (`Contributions::drone_endpoints`, `DroneModeKind::Ext { kind, .. }`)
//! registers the matching kind here: an id, a name, a default configuration and the fields the
//! dialog draws for it. The dialog owns the form — text boxes, a tick, a choice — and reads and
//! writes the configuration as a JSON object, which is exactly what the host stores and the
//! edition's endpoint parses; the base never names a field.
//!
//! A mode saved under a kind nothing registered stays in the list, can be enabled, disabled and
//! removed, and cannot be edited.
//!
//! The order is resolved **once** and cached for the process, never walked in a draw path.

use std::sync::OnceLock;

use serde_json::{Map, Value};

use super::{Registry, SlotId, Slotted, ids};

/// The one group a kind is registered in.
pub const GROUPS: &[SlotId] = &[ids::DRONE_KIND];

/// How the dialog draws one configuration field, and what it writes back.
#[derive(Clone, Copy, Debug)]
pub enum DroneFieldInput {
    /// A one-line text box. An empty box removes the key from the configuration.
    Text { placeholder: &'static str },
    /// A text box holding an unsigned integer, written as a JSON number. An empty box removes the
    /// key; anything that is not a whole number is refused on Save.
    Number { placeholder: &'static str },
    /// A multi-line text box, one entry per line, written as a JSON array of strings (blank lines
    /// dropped). An empty box removes the key; a missing key reads as empty, an array as its
    /// entries one per line.
    Lines { placeholder: &'static str },
    /// A tick box, written as a JSON bool. A missing key reads as unticked.
    Bool,
    /// One of a fixed set, written as a JSON string. Each pair is `(value, label)`; a missing or
    /// unknown key reads as the first.
    Choice(&'static [(&'static str, &'static str)]),
}

/// One field of a kind's configuration.
#[derive(Clone, Copy, Debug)]
pub struct DroneField {
    /// The key in the configuration object.
    pub key: &'static str,
    pub label: &'static str,
    /// One line under the label.
    pub note: &'static str,
    pub input: DroneFieldInput,
}

/// One kind of drone mode.
pub struct DroneKindSpec {
    pub id: SlotId,
    /// The host-side kind string: `DroneModeKind::Ext::kind`, and what
    /// `Contributions::drone_endpoints` registered the endpoint under.
    pub kind: &'static str,
    /// What the Add row and each mode's line call it.
    pub label: &'static str,
    /// One sentence on the dialog's form, above the fields.
    pub description: &'static str,
    /// The configuration a newly added mode starts from. An `fn`, for the reason every spec field
    /// is one: the resolved order is process-wide and immortal.
    pub default_config: fn() -> Map<String, Value>,
    /// Drawn in order.
    pub fields: &'static [DroneField],
}

impl Slotted for DroneKindSpec {
    fn id(&self) -> SlotId {
        self.id
    }
}

/// Registers a kind. The one way in, base or second edition.
pub fn register(reg: &mut Registry<DroneKindSpec>, spec: DroneKindSpec) {
    reg.insert(ids::DRONE_KIND, spec);
}

/// The base's own kinds: none. `http` is built into the dialog.
pub fn base_registry() -> Registry<DroneKindSpec> {
    Registry::new()
}

static RESOLVED: OnceLock<Vec<&'static DroneKindSpec>> = OnceLock::new();

/// Installs the composed registry, before the first window. The base binary never calls this:
/// nothing installed resolves [`base_registry`] instead.
///
/// # Panics
/// If the order has already been resolved.
pub fn install(reg: Registry<DroneKindSpec>) {
    if RESOLVED.set(resolve(reg)).is_err() {
        panic!("ext::drone: the drone-kind container was already resolved before install");
    }
}

fn resolve(reg: Registry<DroneKindSpec>) -> Vec<&'static DroneKindSpec> {
    // Leaked deliberately and exactly once, the way the rail's and the menus' are.
    let reg: &'static mut Registry<DroneKindSpec> = Box::leak(Box::new(reg));
    Registry::resolve(reg, GROUPS)
}

fn resolved() -> &'static [&'static DroneKindSpec] {
    RESOLVED.get_or_init(|| resolve(base_registry()))
}

/// Every registered kind, in the dialog's Add order.
pub fn kinds() -> &'static [&'static DroneKindSpec] {
    resolved()
}

/// The kind registered under a host-side kind string, if any.
pub fn kind_named(kind: &str) -> Option<&'static DroneKindSpec> {
    resolved().iter().copied().find(|spec| spec.kind == kind)
}
