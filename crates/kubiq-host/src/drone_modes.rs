//! Drone mode: a running Kubiq serving remote clients from inside its own process (`D232`).
//!
//! What should run is [`HostSettings::drone_modes`](kubiq_proto::settings::HostSettings); this
//! service owns what *is* running and makes the one match the other on every
//! [`DroneModes::reconcile`]. A remote served this way is an ordinary client of the same
//! [`Hub`] the windows attach to (`D28`), exactly as under `kubiq --serve`.
//!
//! Two shapes of mode. [`DroneModeKind::Http`] is the base listener ([`crate::remote::listen`]),
//! started here. [`DroneModeKind::Ext`] names a kind an edition registered at boot — a
//! [`DroneEndpoint`] in the [`Registry`] `kubiq-app` hands the coordinator — whose configuration
//! the base never parses.
//!
//! Nothing here touches the store: minting a missing token is the coordinator's, through
//! `Settings::update_host`, before it reconciles. Every state change is said to every window as
//! [`Message::DroneModeStatus`]; a mode that fails is reported, never fatal.

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard};

use kubiq_proto::bus::{Hub, Mailbox};
use kubiq_proto::messages::Message;
use kubiq_proto::settings::{DroneMode, DroneModeKind, DroneModeState};

use crate::remote;

/// An edition's kind of drone mode, registered by name (Studio's `rdp`).
pub trait DroneEndpoint: Send + Sync {
    /// The `kind` an [`DroneModeKind::Ext`] mode names to select this endpoint.
    fn kind(&self) -> &str;

    /// Start serving and return at once — called on the coordinator's thread, so anything that
    /// waits (an accept, a channel open) belongs on a thread of the endpoint's own. `Err` is a
    /// sentence the settings page shows as [`DroneModeState::Failed`].
    fn start(&self, launch: Launch<'_>) -> Result<Started, String>;
}

/// What an endpoint is started with.
pub struct Launch<'a> {
    /// The mode's id, for log lines.
    pub id: &'a str,
    /// [`DroneModeKind::Ext::config`], unparsed — [`DroneModeKind::ext_config`] reads it.
    pub config: &'a serde_json::Map<String, serde_json::Value>,
    /// The process's one hub. A remote that should be a full workbench client attaches with
    /// `hub.connect()`, or hands a byte stream to [`crate::carrier::pump`] with a clone.
    pub hub: &'a Hub,
    /// Where to report how many remotes are attached.
    pub clients: Clients,
}

/// A started endpoint: what to show, and how to stop it.
pub struct Started {
    /// Where it is bound, in the endpoint's own words.
    pub addr: String,
    /// What the other end pastes or dials. May be empty for a kind with nothing to paste.
    pub connect_string: String,
    pub running: Box<dyn Running>,
}

/// A running endpoint. [`Running::stop`] stops it: the port or channel is released, and whether
/// attached remotes are closed is the endpoint's to say (the `http` kind closes them).
pub trait Running: Send {
    fn stop(self: Box<Self>);
}

/// The attached-remotes counter an endpoint reports into. Cheap to clone; call
/// [`Clients::set`] with the absolute count each time it moves, from any thread.
#[derive(Clone)]
pub struct Clients(Arc<dyn Fn(u32) + Send + Sync>);

impl Clients {
    pub fn set(&self, count: u32) {
        (self.0)(count)
    }
}

/// The edition-supplied kinds, by name. Empty in the base.
#[derive(Default)]
pub struct Registry {
    endpoints: Vec<Arc<dyn DroneEndpoint>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one. A second registration under a kind already held **replaces** it.
    pub fn register(&mut self, endpoint: Box<dyn DroneEndpoint>) {
        let kind = endpoint.kind().to_string();
        self.endpoints.retain(|held| held.kind() != kind);
        self.endpoints.push(Arc::from(endpoint));
    }

    pub fn get(&self, kind: &str) -> Option<Arc<dyn DroneEndpoint>> {
        self.endpoints
            .iter()
            .find(|held| held.kind() == kind)
            .cloned()
    }

    /// Every registered kind, in registration order — what a settings page may offer.
    pub fn kinds(&self) -> Vec<String> {
        self.endpoints
            .iter()
            .map(|held| held.kind().to_string())
            .collect()
    }
}

/// One mode's state: the run's generation, so a counter left over from a stopped run cannot write
/// into the next one; the state; and the latest attached count that run reported — kept while the
/// state still reads `Starting`, so a remote attaching before `Serving` is said is not lost.
type Shared = Arc<Mutex<(u64, DroneModeState, u32)>>;

struct Slot {
    mode: DroneMode,
    state: Shared,
    running: Option<Box<dyn Running>>,
}

/// The running drone modes.
pub struct DroneModes {
    /// `None` serves nothing: every enabled mode fails with a sentence saying so. Tests that need
    /// no listener build it this way.
    hub: Option<Hub>,
    kinds: Registry,
    everyone: Mailbox,
    slots: BTreeMap<String, Slot>,
}

impl DroneModes {
    pub fn new(hub: Option<Hub>, kinds: Registry) -> Self {
        Self {
            hub,
            kinds,
            everyone: Mailbox::nowhere(),
            slots: BTreeMap::new(),
        }
    }

    /// Where status goes: every window. The coordinator binds it before the first reconcile.
    pub fn bind(&mut self, everyone: Mailbox) {
        self.everyone = everyone;
    }

    /// Make what runs match `modes`: a mode gone from the list is stopped and said `Stopped`
    /// once; a new or changed one (`enabled` or `kind`, never just the name) is stopped and, when
    /// enabled, started again. An unchanged mode is left alone — a `Failed` one included, which
    /// [`Self::restart`] is for.
    pub fn reconcile(&mut self, modes: &[DroneMode]) {
        let gone: Vec<String> = self
            .slots
            .keys()
            .filter(|id| !modes.iter().any(|mode| &mode.id == *id))
            .cloned()
            .collect();
        for id in gone {
            if let Some(mut slot) = self.slots.remove(&id) {
                stop(&mut slot);
                set(&self.everyone, &id, &slot.state, DroneModeState::Stopped);
            }
        }
        for mode in modes {
            let same = self.slots.get(&mode.id).is_some_and(|slot| {
                slot.mode.enabled == mode.enabled && slot.mode.kind == mode.kind
            });
            if same {
                if let Some(slot) = self.slots.get_mut(&mode.id) {
                    slot.mode = mode.clone();
                }
                continue;
            }
            self.slots
                .entry(mode.id.clone())
                .or_insert_with(|| Slot {
                    mode: mode.clone(),
                    state: Arc::new(Mutex::new((0, DroneModeState::Stopped, 0))),
                    running: None,
                })
                .mode = mode.clone();
            self.restart(&mode.id);
        }
    }

    /// Stop the mode if it runs and start it again if it is enabled. An id this service does not
    /// hold says nothing, and answers `false`.
    pub fn restart(&mut self, id: &str) -> bool {
        let Some(mut slot) = self.slots.remove(id) else {
            return false;
        };
        stop(&mut slot);
        if slot.mode.enabled {
            set(&self.everyone, id, &slot.state, DroneModeState::Starting);
            let state = match self.start(&slot) {
                Ok(started) => {
                    slot.running = Some(started.running);
                    tracing::info!("drone mode {id}: serving on {}", started.addr);
                    // `set` fills in the count this run has reported so far.
                    DroneModeState::Serving {
                        addr: started.addr,
                        connect_string: started.connect_string,
                        clients: 0,
                    }
                }
                Err(reason) => {
                    tracing::warn!("drone mode {id}: {reason}");
                    DroneModeState::Failed { reason }
                }
            };
            set(&self.everyone, id, &slot.state, state);
        } else {
            set(&self.everyone, id, &slot.state, DroneModeState::Stopped);
        }
        self.slots.insert(id.to_string(), slot);
        true
    }

    /// One [`Message::DroneModeStatus`] per held mode, for a window that just attached.
    pub fn statuses(&self) -> Vec<Message> {
        self.slots
            .iter()
            .map(|(id, slot)| Message::DroneModeStatus {
                id: id.clone(),
                state: lock(&slot.state).1.clone(),
            })
            .collect()
    }

    /// One mode's state, if held.
    pub fn state(&self, id: &str) -> Option<DroneModeState> {
        self.slots.get(id).map(|slot| lock(&slot.state).1.clone())
    }

    fn start(&self, slot: &Slot) -> Result<Started, String> {
        let Some(hub) = &self.hub else {
            return Err("this host serves no remote clients".to_string());
        };
        let clients = self.counter(&slot.mode.id, &slot.state);
        match &slot.mode.kind {
            DroneModeKind::Http {
                bind,
                port,
                tls_cert,
                tls_key,
                tunnel_allow,
                token,
            } => start_http(
                hub,
                HttpConfig {
                    bind,
                    port: *port,
                    tls_cert: tls_cert.as_deref(),
                    tls_key: tls_key.as_deref(),
                    tunnel_allow,
                    token,
                },
                clients,
            ),
            DroneModeKind::Ext { kind, config } => match self.kinds.get(kind) {
                Some(endpoint) => endpoint.start(Launch {
                    id: &slot.mode.id,
                    config,
                    hub,
                    clients,
                }),
                None => Err(format!("this build has no `{kind}` drone mode")),
            },
        }
    }

    /// The counter handed to one run of one mode: it records the count for that run only, and
    /// once the state reads `Serving`, says each change. The status is sent under the state's
    /// lock, so two counts reported at once reach the windows in the order they were recorded.
    fn counter(&self, id: &str, state: &Shared) -> Clients {
        // `restart` has already opened this run's generation with its `Starting`.
        let generation = lock(state).0;
        let state = state.clone();
        let everyone = self.everyone.clone();
        let id = id.to_string();
        Clients(Arc::new(move |count| {
            let mut held = lock(&state);
            if held.0 != generation {
                return;
            }
            held.2 = count;
            if let DroneModeState::Serving { clients, .. } = &mut held.1
                && *clients != count
            {
                *clients = count;
                everyone.send(Message::DroneModeStatus {
                    id: id.clone(),
                    state: held.1.clone(),
                });
            }
        }))
    }
}

impl Drop for DroneModes {
    fn drop(&mut self) {
        for slot in self.slots.values_mut() {
            stop(slot);
        }
    }
}

fn stop(slot: &mut Slot) {
    if let Some(running) = slot.running.take() {
        running.stop();
    }
}

/// Write a new state. `Starting` opens a new generation with no one attached; every other state
/// belongs to the one it follows, and a `Serving` takes the count that run has reported so far.
/// Said to every window under the same lock a counter sends under, so the two never cross.
fn set(everyone: &Mailbox, id: &str, state: &Shared, mut next: DroneModeState) {
    let mut held = lock(state);
    if next == DroneModeState::Starting {
        held.0 += 1;
        held.2 = 0;
    }
    if let DroneModeState::Serving { clients, .. } = &mut next {
        *clients = held.2;
    }
    held.1 = next.clone();
    everyone.send(Message::DroneModeStatus {
        id: id.to_string(),
        state: next,
    });
}

fn lock(state: &Shared) -> MutexGuard<'_, (u64, DroneModeState, u32)> {
    state.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// [`DroneModeKind::Http`]'s fields, borrowed.
struct HttpConfig<'a> {
    bind: &'a str,
    port: u16,
    tls_cert: Option<&'a str>,
    tls_key: Option<&'a str>,
    tunnel_allow: &'a [String],
    token: &'a str,
}

/// The base kind: `kubiq --serve`'s listener on the mode's own address and persisted token.
fn start_http(hub: &Hub, config: HttpConfig<'_>, clients: Clients) -> Result<Started, String> {
    let ip: IpAddr = config
        .bind
        .trim()
        .parse()
        .map_err(|_| format!("`{}` is not an IP address to bind", config.bind))?;
    let bind = SocketAddr::new(ip, config.port);
    if config.token.is_empty() {
        return Err("no token was minted for this mode".to_string());
    }
    let tunnels = remote::tunnel_allow(config.tunnel_allow)
        .map_err(|why| format!("tunnel allow list: {why}"))?;
    let tls = match (given(config.tls_cert), given(config.tls_key)) {
        (None, None) => None,
        (Some(cert), Some(key)) => Some((read_pem(cert)?, read_pem(key)?)),
        (Some(_), None) => return Err("a TLS certificate wants its key beside it".to_string()),
        (None, Some(_)) => return Err("a TLS key wants its certificate beside it".to_string()),
    };
    let serving = remote::listen(
        hub.clone(),
        remote::Listen {
            bind,
            token: config.token.to_string(),
            tls,
            tunnels,
            watch: Some(Box::new(move |count| clients.set(count))),
        },
    )
    .map_err(|error| format!("could not listen on {bind}: {error}"))?;
    Ok(Started {
        addr: serving.addr.to_string(),
        connect_string: serving.connect_string(),
        running: Box::new(HttpRunning(serving.listening)),
    })
}

/// A PEM file no real certificate chain or key comes near.
const PEM_MAX: u64 = 1024 * 1024;

/// Read a PEM file on the coordinator's thread, safely: only a regular file under [`PEM_MAX`] is
/// opened — a FIFO, a device or a directory is refused from its metadata, which never blocks,
/// because opening one would hang the thread every pane's keystrokes pass through.
fn read_pem(path: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let meta =
        std::fs::metadata(path).map_err(|error| format!("could not read {path}: {error}"))?;
    if !meta.is_file() {
        return Err(format!("{path} is not a regular file"));
    }
    if meta.len() > PEM_MAX {
        return Err(format!("{path} is too large to be a PEM file"));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(PEM_MAX + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("could not read {path}: {error}"))?;
    if bytes.len() as u64 > PEM_MAX {
        return Err(format!("{path} is too large to be a PEM file"));
    }
    Ok(bytes)
}

/// A path field as typed: blank is unset.
fn given(path: Option<&str>) -> Option<&str> {
    path.map(str::trim).filter(|path| !path.is_empty())
}

struct HttpRunning(remote::Listening);

impl Running for HttpRunning {
    fn stop(self: Box<Self>) {
        self.0.stop();
    }
}
