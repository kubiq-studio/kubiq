//! A saved SSH or socket host's tunnels, the window's half (`D227`). The channel is a second
//! connection beside the bus's: over `ssh` a second exec, `kubiq-drone --tunnel`; to a served host
//! (`kubiq --serve`) a second TCP/TLS connection upgraded on `/tunnel` with the same token and the
//! same trust — a pinned key or the CA chain, never a first use of its own
//! (`remote_connect::open_socket_tunnel`). The manager is the process-wide
//! [`kubiq_tunnel::global`] registry's, under a key scoped to this window and the saved host
//! ([`AppState::tunnel_key`]) — two windows attached to one host each link their own.
//!
//! - **Attach** with a far end that advertises `tunnel` (a drone's hello; a served host's `101`)
//!   and at least one enabled spec dials the channel ([`AppState::link_tunnels`]), and keeps it
//!   up: a tunnel connection that dies while the host stays attached is dialled again with a
//!   capped backoff. A reconnect links again under the listeners already bound.
//! - **A lost link** detaches the tunnels; the listeners stay, refusing, for the reconnect
//!   ([`AppState::tunnels_down`]). An `Attached` drone, which nothing reconnects, forgets them.
//! - **Disconnect**, **forget** or **closing the window** stops listening
//!   ([`AppState::forget_tunnels`], [`forget_window_tunnels`]).
//! - **An edit** in the Tunnels form saves the list, drops a manager whose specs or rate changed,
//!   and relinks if the host is attached.
//!
//! Nothing here touches the bus: tunnels never ride it. The child process stays in
//! `ssh_connect`, as the dial's does.

use super::*;
use gpui_component::input::InputState;
use kubiq_proto::carrier::CAP_TUNNEL;
use kubiq_proto::settings::{RemoteCarrier, TunnelKind, TunnelSpec};
use kubiq_tunnel::Live;

use crate::ui::tunnels::{TunnelEdit, TunnelInputs};

/// The prefix of every key a window's saved-host tunnels sit under. Prefixed, so an embedder's
/// keys (Studio's `rdp:`) never meet these.
fn window_prefix(window: WindowId) -> String {
    format!("remote:{}:", window.as_u64())
}

/// Stop every tunnel a window linked: it closed.
pub fn forget_window_tunnels(window: WindowId) {
    let prefix = window_prefix(window);
    kubiq_tunnel::global().forget_prefix(&prefix);
    dial_pins().retain(|key, _| !key.starts_with(&prefix));
}

/// The key pin a socket host's tunnel dial presents, per registry key: read by the redial closure
/// on its own thread, written by the window whenever its saved hosts change.
fn dial_pins() -> std::sync::MutexGuard<'static, HashMap<String, Option<String>>> {
    static PINS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, Option<String>>>> =
        std::sync::LazyLock::new(Default::default);
    PINS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The port each browser proxy (by registry key) last listened on, so a restart rebinds it.
fn proxy_ports() -> std::sync::MutexGuard<'static, HashMap<String, u16>> {
    static PORTS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, u16>>> =
        std::sync::LazyLock::new(Default::default);
    PORTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn set_dial_pin(key: &str, pin: Option<String>) {
    dial_pins().insert(key.to_string(), pin);
}

fn dial_pin(key: &str) -> Option<String> {
    dial_pins().get(key).cloned().flatten()
}

/// The Tunnels form of one saved SSH or socket host: a working copy of its list, an input set per spec, and
/// the rate.
pub struct TunnelForm {
    pub save_id: String,
    pub specs: Vec<TunnelSpec>,
    pub inputs: Vec<TunnelInputs>,
    pub rate: Entity<InputState>,
    pub error: Option<String>,
}

impl AppState {
    /// The registry key of a saved host's tunnels in this window.
    pub fn tunnel_key(&self, save_id: &str) -> String {
        format!("{}{save_id}", window_prefix(self.window_id))
    }

    /// The tunnels' state for a saved host, read as the page draws.
    pub fn tunnel_live(&self, save_id: &str) -> Option<Live> {
        kubiq_tunnel::global().live(&self.tunnel_key(save_id))
    }

    /// The registry key of the embedded browser's proxy for a saved host, in this window.
    fn browser_proxy_key(&self, save_id: &str) -> String {
        format!("{}proxy:{save_id}", window_prefix(self.window_id))
    }

    /// A loopback forward proxy ([`TunnelKind::Proxy`]) that browses as saved host `save_id`, for
    /// the embedded browser's "Browse via remote". Started on first use as a follower
    /// ([`kubiq_tunnel::Registry::follow`]) of whichever link carries that host's tunnels — this
    /// window's own SSH or socket dial, or an embedder's ([`kubiq_tunnel::saved_key`]) — and
    /// kept until no tab rides it or the window closes. Answers its port. `restart` replaces a
    /// running proxy instead of answering it.
    pub fn browser_proxy_port(&mut self, save_id: &str, restart: bool) -> Result<u16, String> {
        let registry = kubiq_tunnel::global();
        let key = self.browser_proxy_key(save_id);
        let listening = |key: &str| {
            registry
                .live(key)
                .and_then(|live| live.status.into_iter().next())
                .filter(|status| status.listening)
        };
        if !restart && let Some(status) = listening(&key) {
            return Ok(status.local_port);
        }
        let parent = self.tunnel_key(save_id);
        // The port this proxy last had in this window comes first: the tabs' stores are keyed by
        // remote, and WebView2 keeps a store's first `--proxy-server` for the life of its browser
        // process (`web_view::proxied_store`), so a proxy back on the same port keeps the cookies.
        // `follow` stops the listener it replaces before binding, so a restart frees the port in
        // time; one dropped by `forget` stops on another thread, hence a short wait between tries.
        // Then a few fresh ports: probing `:0` and binding again races another process for it.
        let last = proxy_ports().get(&key).copied();
        let mut error = None;
        for attempt in 0..6 {
            let port = match (attempt, last) {
                (0..3, Some(port)) => {
                    if attempt > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(60));
                    }
                    port
                }
                _ => match std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                    .and_then(|listener| listener.local_addr())
                {
                    Ok(addr) => addr.port(),
                    Err(e) => {
                        error = Some(format!("no free loopback port for the proxy: {e}"));
                        continue;
                    }
                },
            };
            let mut spec = crate::ui::tunnels::new_spec();
            spec.kind = TunnelKind::Proxy;
            spec.local_port = port;
            spec.host.clear();
            registry.follow(
                key.clone(),
                vec![spec],
                vec![parent.clone(), kubiq_tunnel::saved_key(save_id)],
            );
            if listening(&key).is_some() {
                proxy_ports().insert(key.clone(), port);
                break;
            }
            error = registry
                .live(&key)
                .and_then(|live| live.status.into_iter().next())
                .and_then(|status| status.last_error);
        }
        if listening(&key).is_none() {
            return Err(error.unwrap_or_else(|| "the proxy did not start".to_string()));
        }
        // The window's own dial links only while something rides it: now something does.
        if !registry.live(&parent).is_some_and(|live| live.linked)
            && let Some(host) = self
                .bus
                .remote_conns()
                .iter()
                .find(|conn| conn.save_id == save_id)
                .map(|conn| conn.id)
        {
            self.link_tunnels(host);
        }
        listening(&key)
            .map(|status| status.local_port)
            .ok_or_else(|| {
                registry
                    .live(&key)
                    .and_then(|live| live.status.into_iter().next())
                    .and_then(|status| status.last_error)
                    .unwrap_or_else(|| "the proxy did not start".to_string())
            })
    }

    /// Stop the browser's proxy for `save_id`: no tab rides it any more.
    pub fn drop_browser_proxy(&self, save_id: &str) {
        kubiq_tunnel::global().forget(&self.browser_proxy_key(save_id));
    }

    /// A tab left `save_id`'s proxy (turned off, or closed): when it was the last rider, stop the
    /// proxy and re-run [`Self::link_tunnels`], so a link kept up only for it is released.
    pub(super) fn release_browser_proxy(&mut self, save_id: &str) {
        let ridden = self
            .projects
            .values()
            .flat_map(|open| open.browser.tabs.iter())
            .any(|tab| {
                tab.via_remote
                    .as_ref()
                    .is_some_and(|via| via.save_id == save_id)
            });
        if ridden {
            return;
        }
        self.drop_browser_proxy(save_id);
        let host = self
            .bus
            .remote_conns()
            .iter()
            .find(|conn| conn.save_id == save_id)
            .map(|conn| conn.id);
        if let Some(host) = host {
            self.link_tunnels(host);
        }
    }

    /// Stop listening for a saved host: it was disconnected or forgotten.
    pub(super) fn forget_tunnels(&self, save_id: &str) {
        if !save_id.is_empty() {
            let key = self.tunnel_key(save_id);
            kubiq_tunnel::global().forget(&key);
            dial_pins().remove(&key);
        }
    }

    /// Carry the saved hosts' key pins to the tunnel dials this window keeps, so a redial presents
    /// the pin as filed now. Runs after any change to them: one filed here, or the host's settings
    /// landing from another window.
    pub(super) fn sync_tunnel_pins(&self) {
        let mut pins = dial_pins();
        for saved in &self.workbench.settings.host.remote_hosts {
            if let Some(pin) = pins.get_mut(&self.tunnel_key(&saved.id)) {
                *pin = saved.tls_pin.clone();
            }
        }
    }

    /// The host's link was lost: detach, keep the listeners for the reconnect.
    pub(super) fn tunnels_down(&self, save_id: &str) {
        if !save_id.is_empty() {
            kubiq_tunnel::global().down(&self.tunnel_key(save_id));
        }
    }

    /// After an SSH or socket attach: dial the tunnel channel when the far end serves it and the
    /// saved host has an enabled tunnel, and keep it dialled while the host stays attached;
    /// otherwise stop whatever listens for it.
    pub(super) fn link_tunnels(&mut self, host: HostId) {
        let Some(conn) = self.bus.remote(host) else {
            return;
        };
        let save_id = conn.save_id.clone();
        let served = conn.caps.iter().any(|cap| cap == CAP_TUNNEL);
        let Some(saved) = self
            .workbench
            .settings
            .host
            .remote_hosts
            .iter()
            .find(|saved| !save_id.is_empty() && saved.id == save_id)
            .cloned()
        else {
            return;
        };
        let registry = kubiq_tunnel::global();
        let key = self.tunnel_key(&save_id);
        let socket = match saved.carrier {
            RemoteCarrier::Ssh { .. } => false,
            RemoteCarrier::Socket => true,
            _ => return,
        };
        // The embedded browser's proxy rides this link too ([`Self::browser_proxy_port`]).
        let proxied = registry.live(&self.browser_proxy_key(&save_id)).is_some();
        if !saved.tunnels.iter().any(|spec| spec.enabled) && !proxied {
            registry.forget(&key);
            return;
        }
        if !served {
            let why = if socket {
                "this host serves no tunnels \u{2014} start it without `--tunnel-allow none`, \
                 on a build that has them"
            } else {
                "this drone serves no tunnels \u{2014} update its binary"
            };
            registry.fail(&key, why.into());
            return;
        }
        let (specs, rate) = (saved.tunnels.clone(), saved.tunnel_rate_bps);
        let dial: kubiq_tunnel::Dial = match saved.carrier {
            RemoteCarrier::Ssh {
                profile,
                drone_path,
                ..
            } => {
                let Some(profile) = self.workbench.settings.ssh_profile(profile).cloned() else {
                    return;
                };
                let config_root = self.workbench.config_root.clone();
                std::sync::Arc::new(move || {
                    ssh_connect::open_tunnel_channel(
                        &profile,
                        config_root.as_deref(),
                        drone_path.as_deref(),
                        rate,
                    )
                })
            }
            _ => {
                // The bus dial's own terms, read on every dial, the redials included: the token
                // as saved now, and the pin as the window last filed it ([`sync_tunnel_pins`]) —
                // a "Trust new key" or a "Forget key" reaches the next redial. Never a first use.
                let secret = host_secrets::key_for(&saved.id, &saved.address);
                set_dial_pin(&key, saved.tls_pin.clone());
                let (address, scheme, trust_insecure) = (
                    crate::state::remote::with_default_port(&saved.address),
                    saved.scheme,
                    saved.trust_insecure,
                );
                let pin_key = key.clone();
                std::sync::Arc::new(move || {
                    let token = host_secrets::load_token(&secret)
                        .ok_or_else(|| "no saved token for this host".to_string())?;
                    let params = super::remote_connect::DialParams {
                        address: address.clone(),
                        token,
                        scheme,
                        trust_insecure,
                        tls_pin: dial_pin(&pin_key),
                    };
                    super::remote_connect::open_socket_tunnel(&params, rate)
                })
            }
        };
        registry.link(key, specs, rate, dial, true);
    }

    /// Open the Tunnels form for a saved SSH host.
    pub fn open_tunnel_form(
        &mut self,
        save_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(saved) = self
            .workbench
            .settings
            .host
            .remote_hosts
            .iter()
            .find(|saved| saved.id == save_id)
            .cloned()
        else {
            return;
        };
        let inputs = saved
            .tunnels
            .iter()
            .map(|spec| TunnelInputs::new(spec, window, cx))
            .collect();
        let rate = cx.new(|cx| {
            InputState::new(window, cx).placeholder("bytes per second (empty: unlimited)")
        });
        if let Some(bps) = saved.tunnel_rate_bps {
            rate.update(cx, |input, cx| input.set_value(bps.to_string(), window, cx));
        }
        self.tunnel_form = Some(TunnelForm {
            save_id,
            specs: saved.tunnels,
            inputs,
            rate,
            error: None,
        });
        cx.notify();
    }

    pub fn close_tunnel_form(&mut self, cx: &mut Context<Self>) {
        self.tunnel_form = None;
        cx.notify();
    }

    pub fn add_tunnel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let spec = crate::ui::tunnels::new_spec();
        let inputs = TunnelInputs::new(&spec, window, cx);
        if let Some(form) = &mut self.tunnel_form {
            form.specs.push(spec);
            form.inputs.push(inputs);
        }
        cx.notify();
    }

    pub fn edit_tunnel(&mut self, id: &str, edit: TunnelEdit, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.tunnel_form {
            edit.apply(&mut form.specs, &mut form.inputs, id);
        }
        cx.notify();
    }

    /// Validate the form, save the list on the host's record, and relink: a changed list or rate
    /// drops the manager, and an attached host dials the channel again with the new one.
    pub fn save_tunnel_form(&mut self, cx: &mut Context<Self>) {
        let Some(mut form) = self.tunnel_form.take() else {
            return;
        };
        let fields: Vec<_> = form.inputs.iter().map(|inputs| inputs.fields(cx)).collect();
        let rate_text = form.rate.read(cx).value().to_string();
        let checked = (|| {
            crate::ui::tunnels::apply_fields(&mut form.specs, &fields)?;
            crate::ui::tunnels::check_tunnels(&form.specs)?;
            let rate: Option<u64> = crate::ui::tunnels::optional(&rate_text, "The tunnel rate")?;
            if rate == Some(0) {
                return Err("The tunnel rate is 0; leave it empty for unlimited".to_string());
            }
            Ok(rate)
        })();
        let rate = match checked {
            Ok(rate) => rate,
            Err(error) => {
                form.error = Some(error);
                self.tunnel_form = Some(form);
                cx.notify();
                return;
            }
        };
        let save_id = form.save_id.clone();
        let Some(saved) = self
            .workbench
            .settings
            .host
            .remote_hosts
            .iter_mut()
            .find(|saved| saved.id == save_id)
        else {
            cx.notify();
            return;
        };
        saved.tunnels = form.specs;
        saved.tunnel_rate_bps = rate;
        let (specs, keep) = (
            saved.tunnels.clone(),
            saved.tunnels.iter().any(|s| s.enabled),
        );
        self.remember_host_settings();

        let registry = kubiq_tunnel::global();
        let key = self.tunnel_key(&save_id);
        let dropped = registry.reconfigure(&key, &specs, rate, keep);
        if dropped || registry.live(&key).is_none_or(|live| !live.linked) {
            let attached = self
                .bus
                .remote_conns()
                .iter()
                .find(|conn| conn.save_id == save_id)
                .map(|conn| conn.id);
            match attached {
                Some(host) => self.link_tunnels(host),
                None => registry.forget(&key),
            }
        }
        cx.notify();
    }
}
