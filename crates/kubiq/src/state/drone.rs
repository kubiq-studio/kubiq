//! The "Configure drone mode" dialog's state, and the pure conversions its form needs (`D232`).
//!
//! What runs is `HostSettings::drone_modes`, written whole through `SetSettings`; how each mode is
//! doing is the last `DroneModeStatus` the host said for it. This holds the second, the dialog's
//! own chrome, and the form being edited — and the pure half of that form, which turns what was
//! typed into a [`DroneModeKind`] so a test can pin it with no window.

use std::collections::HashMap;
use std::net::IpAddr;

use gpui::Entity;
use gpui_component::input::{InputState, TextareaState};
use kubiq_proto::settings::{DroneModeKind, DroneModeState};
use serde_json::{Map, Value};

use crate::ext::drone::{DroneFieldInput, DroneKindSpec};

/// `http`'s default bind, and the first of the three choices.
pub const LOOPBACK: &str = "127.0.0.1";
/// The second choice: every interface.
pub const ANY_INTERFACE: &str = "0.0.0.0";
/// `http`'s default port.
pub const DEFAULT_PORT: u16 = 7420;

/// How the bind address is picked on the `http` form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindChoice {
    /// "This machine only" — `127.0.0.1`.
    Loopback,
    /// "Local network" — `0.0.0.0`.
    Lan,
    /// Any other address, typed.
    Custom,
}

impl BindChoice {
    /// The choice a saved bind address reads as.
    pub fn of(bind: &str) -> Self {
        match bind {
            LOOPBACK => Self::Loopback,
            ANY_INTERFACE => Self::Lan,
            _ => Self::Custom,
        }
    }
}

/// Whether binding here puts the listener on a network rather than on this machine alone. An
/// address that is not an IP literal counts as exposed: the warning errs toward showing.
pub fn bind_exposes(bind: &str) -> bool {
    bind.trim()
        .parse::<IpAddr>()
        .map(|ip| !ip.is_loopback())
        .unwrap_or(true)
}

/// What the `http` form holds, as typed.
#[derive(Clone, Debug)]
pub struct HttpForm {
    pub bind: BindChoice,
    /// Read only when [`Self::bind`] is [`BindChoice::Custom`].
    pub bind_custom: String,
    pub port: String,
    pub tls_cert: String,
    pub tls_key: String,
    /// Comma or space separated.
    pub tunnel_allow: String,
}

impl HttpForm {
    /// The form as a mode kind. `token` is the one the mode already holds — empty for a new mode,
    /// so the host mints one.
    pub fn to_kind(&self, token: String) -> Result<DroneModeKind, String> {
        let bind = match self.bind {
            BindChoice::Loopback => LOOPBACK.to_string(),
            BindChoice::Lan => ANY_INTERFACE.to_string(),
            BindChoice::Custom => {
                let typed = self.bind_custom.trim();
                if typed.parse::<IpAddr>().is_err() {
                    return Err("The bind address must be an IP address.".to_string());
                }
                typed.to_string()
            }
        };
        let port = match self.port.trim() {
            "" => DEFAULT_PORT,
            typed => typed
                .parse::<u16>()
                .map_err(|_| "The port must be a number from 0 to 65535.".to_string())?,
        };
        let (cert, key) = (non_empty(&self.tls_cert), non_empty(&self.tls_key));
        if cert.is_some() != key.is_some() {
            return Err("TLS needs both a certificate and a key, or neither.".to_string());
        }
        Ok(DroneModeKind::Http {
            bind,
            port,
            tls_cert: cert,
            tls_key: key,
            tunnel_allow: self
                .tunnel_allow
                .split([',', ' ', '\n'])
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect(),
            token,
        })
    }
}

fn non_empty(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// An edition kind's configuration from its form: `base` (the mode's configuration, which already
/// carries every tick and choice and any key this form does not name) with each text and number
/// field replaced by what was typed.
pub fn ext_config(
    spec: &DroneKindSpec,
    base: &Map<String, Value>,
    typed: &HashMap<&str, String>,
) -> Result<Map<String, Value>, String> {
    let mut config = base.clone();
    for field in spec.fields {
        let text = typed.get(field.key).map(|text| text.trim()).unwrap_or("");
        match field.input {
            DroneFieldInput::Text { .. } => {
                if text.is_empty() {
                    config.remove(field.key);
                } else {
                    config.insert(field.key.to_string(), Value::String(text.to_string()));
                }
            }
            DroneFieldInput::Number { .. } => {
                if text.is_empty() {
                    config.remove(field.key);
                } else {
                    let number = text
                        .parse::<u64>()
                        .map_err(|_| format!("{} must be a whole number.", field.label))?;
                    config.insert(field.key.to_string(), Value::from(number));
                }
            }
            DroneFieldInput::Lines { .. } => {
                let lines: Vec<Value> = text
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(|line| Value::String(line.to_string()))
                    .collect();
                if lines.is_empty() {
                    config.remove(field.key);
                } else {
                    config.insert(field.key.to_string(), Value::Array(lines));
                }
            }
            DroneFieldInput::Bool | DroneFieldInput::Choice(_) => {}
        }
    }
    Ok(config)
}

/// A field's current text, as the form's box starts out: strings as they are, numbers printed.
pub fn field_text(config: &Map<String, Value>, key: &str) -> String {
    match config.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// The dialog, while it is up, and what the host last said about each mode.
#[derive(Default)]
pub struct DroneDialog {
    pub open: bool,
    /// Last `DroneModeStatus` per mode id. Kept past the dialog closing: the titlebar's badge
    /// reads it.
    pub status: HashMap<String, DroneModeState>,
    /// The form, while one mode is being added or edited; the list otherwise.
    pub edit: Option<DroneEdit>,
}

impl DroneDialog {
    pub fn apply_status(&mut self, id: String, state: DroneModeState) {
        self.status.insert(id, state);
    }

    /// Whether any mode is serving right now — what the titlebar's badge shows.
    pub fn any_serving(&self) -> bool {
        self.status
            .values()
            .any(|state| matches!(state, DroneModeState::Serving { .. }))
    }
}

/// The form for one mode.
pub struct DroneEdit {
    pub id: String,
    pub is_new: bool,
    pub enabled: bool,
    pub name: Entity<InputState>,
    pub body: EditBody,
    /// The last Save's refusal, worded for the form.
    pub error: Option<String>,
}

/// The fields a kind draws.
pub enum EditBody {
    Http {
        bind: BindChoice,
        bind_custom: Entity<InputState>,
        port: Entity<InputState>,
        tls_cert: Entity<InputState>,
        tls_key: Entity<InputState>,
        tunnel_allow: Entity<InputState>,
        /// Carried through unchanged: the host owns it.
        token: String,
    },
    Ext {
        spec: &'static DroneKindSpec,
        /// Every tick and choice lives here, with any key the form does not name.
        config: Map<String, Value>,
        /// One box per text and number field.
        inputs: Vec<(&'static str, ExtBox)>,
    },
}

/// The box behind one text, number or lines field of an edition kind's form.
#[derive(Clone)]
pub enum ExtBox {
    Line(Entity<InputState>),
    Lines(Entity<TextareaState>),
}

impl ExtBox {
    pub fn value(&self, cx: &gpui::App) -> String {
        match self {
            Self::Line(input) => input.read(cx).value().to_string(),
            Self::Lines(area) => area.read(cx).value().to_string(),
        }
    }
}
