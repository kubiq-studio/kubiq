//! The "Configure drone mode" dialog's actions (`D232`): open and close, the list's per-mode verbs,
//! and the add/edit form with its Save.
//!
//! What runs is `HostSettings::drone_modes`, so every change here ends in `remember_host_settings`
//! — the host starts and stops modes to match the write. The dialog itself is `ui::drone_modes`;
//! how each mode is doing arrives as `DroneModeStatus` and lands in [`Self::receive_drone`].

use super::*;
use crate::ext::drone::{DroneFieldInput, DroneKindSpec};
use crate::state::drone::{
    BindChoice, DroneEdit, EditBody, ExtBox, HttpForm, bind_exposes, ext_config, field_text,
};
use gpui_component::input::TextareaState;
use kubiq_proto::settings::{DroneMode, DroneModeKind, DroneModeState};
use serde_json::{Map, Value};

impl AppState {
    /// Whether the way in to the dialog is drawn: serving is an experimental feature, but a mode
    /// that is enabled keeps listening whatever the tier says, so it is never left invisible and
    /// unstoppable.
    pub fn drone_modes_reachable(&self) -> bool {
        self.offers(crate::state::settings::Tier::Experimental)
            || self
                .workbench
                .settings
                .host
                .drone_modes
                .iter()
                .any(|mode| mode.enabled)
    }

    /// Raise the dialog on the list.
    pub fn open_drone_modes(&mut self, cx: &mut Context<Self>) {
        if !self.drone_modes_reachable() {
            return;
        }
        self.workbench.drone_modes.open = true;
        self.workbench.drone_modes.edit = None;
        cx.notify();
    }

    /// Close the dialog, and with it a form left half-filled.
    pub fn close_drone_modes(&mut self, cx: &mut Context<Self>) {
        self.workbench.drone_modes.open = false;
        self.workbench.drone_modes.edit = None;
        cx.notify();
    }

    /// Whether the dialog is showing its form, which is what Escape peels first.
    pub fn drone_form_open(&self) -> bool {
        self.workbench.drone_modes.edit.is_some()
    }

    /// The drone-mode family, host to window. Answers with the message when it is another family's.
    pub fn receive_drone(
        &mut self,
        host: HostRef,
        message: Message,
        cx: &mut Context<Self>,
    ) -> Option<Message> {
        match message {
            Message::DroneModeStatus { id, state } => {
                // Only this process's own modes: a remote host's are its own business, and would
                // light this window's badge for a listener that is not here.
                if host == HostRef::Local {
                    self.workbench.drone_modes.apply_status(id, state);
                    cx.notify();
                }
                None
            }
            other => Some(other),
        }
    }

    fn drone_modes_mut(&mut self) -> &mut Vec<DroneMode> {
        &mut self.workbench.settings.host.drone_modes
    }

    /// Enable or disable one mode: the host starts or stops it to match.
    pub fn toggle_drone_mode(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(mode) = self.drone_modes_mut().iter_mut().find(|mode| mode.id == id) {
            mode.enabled = !mode.enabled;
            self.remember_host_settings();
            cx.notify();
        }
    }

    /// Forget one mode: the host stops it and its token goes with the record.
    pub fn remove_drone_mode(&mut self, id: &str, cx: &mut Context<Self>) {
        self.drone_modes_mut().retain(|mode| mode.id != id);
        self.remember_host_settings();
        cx.notify();
    }

    /// Stop and start one mode again — the retry after a failure.
    pub fn restart_drone_mode(&mut self, id: &str) {
        self.bus
            .send(Message::RestartDroneMode { id: id.to_string() });
    }

    /// Mint a new token for one `http` mode. Every remote attached on the old one is closed.
    pub fn regenerate_drone_token(&mut self, id: &str) {
        self.bus
            .send(Message::RegenerateDroneToken { id: id.to_string() });
    }

    /// Put one serving mode's connect string on the clipboard.
    pub fn copy_drone_connect_string(&self, id: &str, cx: &mut Context<Self>) {
        if let Some(DroneModeState::Serving { connect_string, .. }) =
            self.workbench.drone_modes.status.get(id)
        {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(connect_string.clone()));
        }
    }

    // ── the form ────────────────────────────────────────────────────

    fn drone_input(
        placeholder: &str,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder.to_string()));
        input.update(cx, |state, cx| state.set_value(value, window, cx));
        input
    }

    fn ext_inputs(
        spec: &'static DroneKindSpec,
        config: &Map<String, Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<(&'static str, ExtBox)> {
        spec.fields
            .iter()
            .filter_map(|field| match field.input {
                DroneFieldInput::Text { placeholder } | DroneFieldInput::Number { placeholder } => {
                    let value = field_text(config, field.key);
                    Some((
                        field.key,
                        ExtBox::Line(Self::drone_input(placeholder, &value, window, cx)),
                    ))
                }
                DroneFieldInput::Lines { placeholder } => {
                    let value = field_text(config, field.key);
                    let area = cx.new(|cx| {
                        TextareaState::new(window, cx)
                            .placeholder(placeholder.to_string())
                            .auto_grow(3, 8)
                    });
                    area.update(cx, |state, cx| state.set_value(value, window, cx));
                    Some((field.key, ExtBox::Lines(area)))
                }
                DroneFieldInput::Bool | DroneFieldInput::Choice(_) => None,
            })
            .collect()
    }

    /// Open the form on a new mode: `http` for `None`, otherwise a registered edition kind.
    pub fn begin_add_drone_mode(
        &mut self,
        kind: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = format!("drone-{}", uuid::Uuid::new_v4().simple());
        let (name, body) = match kind.and_then(crate::ext::drone::kind_named) {
            Some(spec) => {
                let config = (spec.default_config)();
                let inputs = Self::ext_inputs(spec, &config, window, cx);
                (
                    spec.label,
                    EditBody::Ext {
                        spec,
                        config,
                        inputs,
                    },
                )
            }
            None => (
                "HTTP",
                EditBody::Http {
                    bind: BindChoice::Loopback,
                    bind_custom: Self::drone_input("IP address", "", window, cx),
                    port: Self::drone_input("7420", "", window, cx),
                    tls_cert: Self::drone_input("Path to a PEM chain", "", window, cx),
                    tls_key: Self::drone_input("Path to a PEM key", "", window, cx),
                    tunnel_allow: Self::drone_input("Empty is this machine only", "", window, cx),
                    token: String::new(),
                },
            ),
        };
        self.workbench.drone_modes.edit = Some(DroneEdit {
            id,
            is_new: true,
            enabled: true,
            name: Self::drone_input("Name", name, window, cx),
            body,
            error: None,
        });
        cx.notify();
    }

    /// Open the form on a saved mode. A kind nothing registered has no form and opens nothing.
    pub fn begin_edit_drone_mode(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mode) = self
            .drone_modes_mut()
            .iter()
            .find(|mode| mode.id == id)
            .cloned()
        else {
            return;
        };
        let body = match &mode.kind {
            DroneModeKind::Http {
                bind,
                port,
                tls_cert,
                tls_key,
                tunnel_allow,
                token,
            } => {
                let choice = BindChoice::of(bind);
                let custom = if choice == BindChoice::Custom {
                    bind
                } else {
                    ""
                };
                EditBody::Http {
                    bind: choice,
                    bind_custom: Self::drone_input("IP address", custom, window, cx),
                    port: Self::drone_input("7420", &port.to_string(), window, cx),
                    tls_cert: Self::drone_input(
                        "Path to a PEM chain",
                        tls_cert.as_deref().unwrap_or(""),
                        window,
                        cx,
                    ),
                    tls_key: Self::drone_input(
                        "Path to a PEM key",
                        tls_key.as_deref().unwrap_or(""),
                        window,
                        cx,
                    ),
                    tunnel_allow: Self::drone_input(
                        "Empty is this machine only",
                        &tunnel_allow.join(", "),
                        window,
                        cx,
                    ),
                    token: token.clone(),
                }
            }
            DroneModeKind::Ext { kind, config } => {
                let Some(spec) = crate::ext::drone::kind_named(kind) else {
                    return;
                };
                let inputs = Self::ext_inputs(spec, config, window, cx);
                EditBody::Ext {
                    spec,
                    config: config.clone(),
                    inputs,
                }
            }
        };
        self.workbench.drone_modes.edit = Some(DroneEdit {
            id: mode.id,
            is_new: false,
            enabled: mode.enabled,
            name: Self::drone_input("Name", &mode.name, window, cx),
            body,
            error: None,
        });
        cx.notify();
    }

    /// Back to the list, dropping the form.
    pub fn cancel_drone_edit(&mut self, cx: &mut Context<Self>) {
        self.workbench.drone_modes.edit = None;
        cx.notify();
    }

    /// Pick the `http` form's bind choice.
    pub fn set_drone_bind(&mut self, choice: BindChoice, cx: &mut Context<Self>) {
        if let Some(DroneEdit {
            body: EditBody::Http { bind, .. },
            ..
        }) = &mut self.workbench.drone_modes.edit
        {
            *bind = choice;
            cx.notify();
        }
    }

    /// Set a tick or a choice on an edition kind's form.
    pub fn set_drone_ext_value(&mut self, key: &'static str, value: Value, cx: &mut Context<Self>) {
        if let Some(DroneEdit {
            body: EditBody::Ext { config, .. },
            ..
        }) = &mut self.workbench.drone_modes.edit
        {
            config.insert(key.to_string(), value);
            cx.notify();
        }
    }

    /// Whether the form as it stands would serve on a network with nothing but the token between
    /// the world and the workbench — the one thing the form warns about.
    pub fn drone_form_exposed_without_tls(&self, cx: &App) -> bool {
        let Some(DroneEdit {
            body:
                EditBody::Http {
                    bind,
                    bind_custom,
                    tls_cert,
                    tls_key,
                    ..
                },
            ..
        }) = &self.workbench.drone_modes.edit
        else {
            return false;
        };
        let exposed = match bind {
            BindChoice::Loopback => false,
            BindChoice::Lan => true,
            BindChoice::Custom => bind_exposes(bind_custom.read(cx).value().as_ref()),
        };
        let tls = !tls_cert.read(cx).value().trim().is_empty()
            && !tls_key.read(cx).value().trim().is_empty();
        exposed && !tls
    }

    /// Save the form: refused with a sentence on the form, or written through `SetSettings`.
    pub fn save_drone_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.workbench.drone_modes.edit.as_ref() else {
            return;
        };
        let name = edit.name.read(cx).value().trim().to_string();
        let built = if name.is_empty() {
            Err("Give the mode a name.".to_string())
        } else {
            match &edit.body {
                EditBody::Http {
                    bind,
                    bind_custom,
                    port,
                    tls_cert,
                    tls_key,
                    tunnel_allow,
                    token,
                } => HttpForm {
                    bind: *bind,
                    bind_custom: bind_custom.read(cx).value().to_string(),
                    port: port.read(cx).value().to_string(),
                    tls_cert: tls_cert.read(cx).value().to_string(),
                    tls_key: tls_key.read(cx).value().to_string(),
                    tunnel_allow: tunnel_allow.read(cx).value().to_string(),
                }
                .to_kind(token.clone()),
                EditBody::Ext {
                    spec,
                    config,
                    inputs,
                } => {
                    let typed = inputs
                        .iter()
                        .map(|(key, input)| (*key, input.value(cx)))
                        .collect();
                    ext_config(spec, config, &typed).map(|config| DroneModeKind::Ext {
                        kind: spec.kind.to_string(),
                        config,
                    })
                }
            }
        };
        let (id, enabled) = (edit.id.clone(), edit.enabled);
        match built {
            Err(error) => {
                if let Some(edit) = &mut self.workbench.drone_modes.edit {
                    edit.error = Some(error);
                }
            }
            Ok(kind) => {
                let mode = DroneMode {
                    id,
                    name,
                    enabled,
                    kind,
                };
                let modes = self.drone_modes_mut();
                match modes.iter_mut().find(|held| held.id == mode.id) {
                    Some(held) => *held = mode,
                    None => modes.push(mode),
                }
                self.workbench.drone_modes.edit = None;
                self.remember_host_settings();
            }
        }
        cx.notify();
    }
}
