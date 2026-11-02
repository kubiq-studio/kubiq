//! The remote listener: lets a UI on another machine attach to this host over TCP.
//!
//! Everything this module does is transport. Per rule 1 in `_docs/tech/architecture.md`, the only
//! thing a connection needs from the bus is [`kubiq_proto::bus::Hub::connect`] — it mints a
//! `Client`, registers it in the routing table and tells the coordinator it exists. So a
//! connection here is nothing but: attach, pump frames off the socket onto the client, pump
//! messages off the client onto the socket, and drop the client when either side stops. No message
//! family is special-cased, and none should ever be added here.
//!
//! **Why `std::net::TcpListener` and not `tiny_http`.** `tiny_http` is used elsewhere in this
//! workspace for a one-shot loopback request (the OAuth redirect), but it cannot hand back the raw
//! socket after answering — it owns the connection for the life of one response. This listener has
//! to answer one HTTP-shaped request and then keep the same socket for the life of a session,
//! reading and writing raw length-prefixed frames on it. That needs the socket itself, not a
//! library built around request/response. Do not "simplify" this to `tiny_http` later — it cannot
//! do the upgrade this needs.
//!
//! **TLS is the same handshake inside a session.** `serve_tls` wraps each accepted socket in a
//! rustls server session first; everything past the handshake — the HTTP upgrade, the pumps —
//! runs over a [`Socket`] that does not know which it is. A UI dials `https` exactly when the
//! host was started with `--tls-cert`/`--tls-key`, and `http` otherwise.
//!
//! **The tunnel channel is a second upgrade, `/tunnel`** (`D227`), on a connection of its own with
//! the same token: forwarded ports for a window attached over `/attach`, served by
//! [`kubiq_tunnel::serve`] under an allow list ([`loopback_tunnels`] unless `--tunnel-allow` says
//! otherwise). Nothing on it reaches the bus. When it answers, the attach's `101` says so in
//! [`CAPS_HEADER`].
//!
//! **A listener can be stopped** (`D232`). [`listen`] hands back a [`Listening`] beside the
//! address: [`Listening::stop`] ends the accept loop, which drops the socket and frees the port,
//! and shuts down every connection it accepted — attached windows and tunnel links alike — so a
//! remote window sees its carrier drop and the host reaps its panes as for any departed client.
//! `kubiq-app --serve` keeps no handle and never stops; a desktop drone mode does.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use kubiq_proto::bus::Hub;

use crate::carrier::{Closer, Socket, pump};

/// A header claiming more than this before the blank line that ends it is refused before any more
/// of it is read — an unauthenticated peer gets no chance to make this host buffer without bound.
const MAX_HEADER: usize = 8 * 1024;

/// How long a peer has to finish its handshake. The header block is read a byte at a time from an
/// unauthenticated socket, so without a deadline a peer that connects and then says nothing pins a
/// connection thread for the life of the process — the `MAX_HEADER` cap bounds the memory that
/// costs and nothing bounds the time. Cleared the moment the upgrade is written, because a session
/// is idle between frames by design.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// How often an idle accept loop looks at its stop flag. The socket is non-blocking so that a
/// stopped listener is noticed without a self-connect, which an unspecified bind on Windows cannot
/// make; a connection waiting this long to be accepted is invisible next to its own handshake.
const ACCEPT_POLL: Duration = Duration::from_millis(25);

/// What starting the listener hands back: where it ended up bound, the token a client must
/// present to attach, and whether the socket speaks TLS.
pub struct Serving {
    pub addr: SocketAddr,
    pub token: String,
    pub tls: bool,
    /// Whether `/tunnel` answers.
    pub tunnels: bool,
    /// The way to stop it, and to count who is attached. Dropping it stops nothing.
    pub listening: Listening,
}

impl Serving {
    /// The string a window pastes into its connect modal: `http[s]://<ip>:<port>?token=<token>`,
    /// with an unspecified bind replaced by [`advertised_ip`].
    pub fn connect_string(&self) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        let ip = advertised_ip(self.addr);
        let host = match ip {
            IpAddr::V6(v6) => format!("[{v6}]"),
            IpAddr::V4(v4) => v4.to_string(),
        };
        format!(
            "{scheme}://{host}:{}?token={}",
            self.addr.port(),
            self.token
        )
    }
}

/// The address to tell a remote to dial.
///
/// Bound to one interface, that address is the answer. Bound to all of them, `0.0.0.0` is useless
/// to paste, so ask the routing table which source address it would use to leave this machine —
/// a connected UDP socket assigns one without sending a packet or needing a reachable peer, which
/// is why the address it is pointed at does not matter and no traffic is generated. A machine with
/// no route out has nothing to advertise, and the bind address is returned unchanged.
pub fn advertised_ip(addr: SocketAddr) -> IpAddr {
    if !addr.ip().is_unspecified() {
        return addr.ip();
    }
    UdpSocket::bind(("0.0.0.0", 0))
        .and_then(|socket| {
            socket.connect(("192.0.2.1", 53))?;
            socket.local_addr()
        })
        .map(|local| local.ip())
        .unwrap_or_else(|_| addr.ip())
}

/// Told how many windows are attached, each time that number moves.
pub type ClientsWatch = Box<dyn Fn(u32) + Send + Sync>;

/// A running listener, from the caller's side: stop it, or ask how many windows are attached.
/// Cloneable; every clone is the same listener. Dropping one stops nothing.
#[derive(Clone)]
pub struct Listening(Arc<Shared>);

struct Shared {
    stopped: AtomicBool,
    next: AtomicU64,
    /// Every accepted socket still open, by a listener-local number: a clone of it to shut down,
    /// and whether it is an attached window rather than a handshake or a tunnel link.
    conns: Mutex<HashMap<u64, (TcpStream, bool)>>,
    watch: Option<ClientsWatch>,
    /// Held across reading the count and telling `watch`, so two threads whose sockets moved at
    /// once cannot report their counts in the opposite order.
    notifying: Mutex<()>,
    /// The accept thread, which owns the socket: [`Listening::stop`] waits for it so the port is
    /// free when `stop` returns.
    accepting: Mutex<Option<thread::JoinHandle<()>>>,
}

/// How long [`Listening::stop`] waits for the accept loop to notice and drop the socket — a few
/// [`ACCEPT_POLL`]s, so a loop asleep between polls is always caught.
const STOP_WAIT: Duration = Duration::from_millis(250);

impl Listening {
    fn new(watch: Option<ClientsWatch>) -> Self {
        Listening(Arc::new(Shared {
            stopped: AtomicBool::new(false),
            next: AtomicU64::new(0),
            conns: Mutex::new(HashMap::new()),
            watch,
            notifying: Mutex::new(()),
            accepting: Mutex::new(None),
        }))
    }

    /// Stop accepting, free the port, and close every connection this listener accepted. Returns
    /// once the accept loop has ended and dropped the socket (bounded by [`STOP_WAIT`]), so the same
    /// port can be bound again straight away; each closed window's client goes as a dropped
    /// socket's does, so the host reaps its panes. Idempotent.
    pub fn stop(&self) {
        self.0.stopped.store(true, Ordering::SeqCst);
        let conns: Vec<(TcpStream, bool)> = self.lock().drain().map(|(_, conn)| conn).collect();
        for (socket, _) in conns {
            let _ = socket.shutdown(Shutdown::Both);
        }
        let accepting = self
            .0
            .accepting
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take();
        if let Some(accepting) = accepting {
            let deadline = std::time::Instant::now() + STOP_WAIT;
            while !accepting.is_finished() && std::time::Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            if accepting.is_finished() {
                let _ = accepting.join();
            } else {
                tracing::warn!("remote: the accept loop did not stop within {STOP_WAIT:?}");
            }
        }
    }

    /// Whether [`Self::stop`] was called.
    pub fn is_stopped(&self) -> bool {
        self.0.stopped.load(Ordering::SeqCst)
    }

    /// Windows attached through this listener right now. Tunnel links are not counted.
    pub fn clients(&self) -> u32 {
        self.lock()
            .values()
            .filter(|(_, attached)| *attached)
            .count() as u32
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, (TcpStream, bool)>> {
        self.0
            .conns
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// File an accepted socket. One accepted after a stop is shut down at once.
    fn track(&self, stream: &TcpStream) -> Option<Tracked> {
        let clone = stream.try_clone().ok()?;
        let id = self.0.next.fetch_add(1, Ordering::Relaxed);
        self.lock().insert(id, (clone, false));
        if self.is_stopped() {
            self.lock().remove(&id);
            let _ = stream.shutdown(Shutdown::Both);
            return None;
        }
        Some(Tracked {
            listening: self.clone(),
            id,
        })
    }

    fn notify(&self) {
        if let Some(watch) = &self.0.watch {
            let _order = self
                .0
                .notifying
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            watch(self.clients());
        }
    }
}

/// One accepted socket's row in [`Listening`], removed when its thread ends.
struct Tracked {
    listening: Listening,
    id: u64,
}

impl Tracked {
    /// The upgrade to `/attach` was written: this socket is a window now.
    fn attached(&self) {
        if let Some(row) = self.listening.lock().get_mut(&self.id) {
            row.1 = true;
        }
        self.listening.notify();
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        let was = self.listening.lock().remove(&self.id);
        if matches!(was, Some((_, true))) {
            self.listening.notify();
        }
    }
}

/// Everything [`listen`] needs to start one listener.
pub struct Listen {
    pub bind: SocketAddr,
    /// The bearer token a client presents. [`generate_token`] mints one.
    pub token: String,
    /// `(cert_pem, key_pem)` — the chain, leaf first, and the private key. `None` is plaintext.
    pub tls: Option<(Vec<u8>, Vec<u8>)>,
    /// What `/tunnel` may dial; `None` answers it as a path this host does not serve.
    pub tunnels: Option<kubiq_tunnel::AllowList>,
    /// Told the attached count each time it moves.
    pub watch: Option<ClientsWatch>,
}

/// Start a listener on a thread of its own and return immediately. [`serve`] and [`serve_tls`] are
/// this with a fresh token; a desktop drone mode (`D232`) calls it with its persisted one and keeps
/// [`Serving::listening`] to stop it.
pub fn listen(hub: Hub, options: Listen) -> io::Result<Serving> {
    let config = match &options.tls {
        Some((cert_pem, key_pem)) => Some(tls_config(cert_pem, key_pem)?),
        None => None,
    };
    let listener = TcpListener::bind(options.bind)?;
    let addr = listener.local_addr()?;
    listener.set_nonblocking(true)?;
    let listening = Listening::new(options.watch);
    let serving_tunnels = options.tunnels.is_some();
    let accept_token = options.token.clone();
    let tls = config.is_some();
    let looping = listening.clone();
    let tunnels = options.tunnels;

    let accepting = thread::Builder::new()
        .name("kubiq-remote-listen".to_string())
        .spawn(move || accept_loop(listener, hub, accept_token, config, tunnels, looping))?;
    *listening
        .0
        .accepting
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = Some(accepting);

    Ok(Serving {
        addr,
        token: options.token,
        tls,
        tunnels: serving_tunnels,
        listening,
    })
}

/// The response header an attach's `101` names this host's carrier capabilities in — the socket
/// carrier's counterpart of a drone hello's `capabilities` ([`kubiq_proto::carrier::CAP_TUNNEL`]
/// when `/tunnel` answers). Absent is none.
pub const CAPS_HEADER: &str = "Kubiq-Capabilities";

/// What a served host's tunnel channel dials when `--tunnel-allow` names nothing: this machine's
/// own loopback and nothing else. A served host is a whole workbench on the network, so its
/// default is narrower than a drone's (which admits anything the `ssh` login could reach anyway).
/// Literal addresses, never a glob over them: `127.*` would admit a DNS name like `127.x.example`.
pub fn loopback_tunnels() -> kubiq_tunnel::AllowList {
    kubiq_tunnel::AllowList::parse(["localhost:*", "127.0.0.1:*", "[::1]:*"])
        .expect("the loopback allow list")
}

/// `--tunnel-allow` values to the tunnel channel's allow list: none at all is
/// [`loopback_tunnels`], `none` turns the channel off, and anything else is comma-separated
/// `host:port` globs, `*` admitting everything (`kubiq_tunnel::AllowList::parse`).
pub fn tunnel_allow(values: &[String]) -> Result<Option<kubiq_tunnel::AllowList>, String> {
    let patterns: Vec<&str> = values
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty())
        .collect();
    if patterns.is_empty() {
        return Ok(Some(loopback_tunnels()));
    }
    if patterns
        .iter()
        .any(|pattern| pattern.eq_ignore_ascii_case("none"))
    {
        return if patterns.len() == 1 {
            Ok(None)
        } else {
            Err("`none` cannot be combined with other patterns".to_string())
        };
    }
    kubiq_tunnel::AllowList::parse(patterns).map(Some)
}

/// Start the listener on a thread of its own and return immediately.
///
/// `hub` is cloned into the accept loop; the caller keeps its own clone for local windows exactly
/// as it does today — this adds a second kind of attacher, not a different `Hub`. `tunnels`, when
/// given, opens the tunnel channel on `/tunnel` (`D227`) and is what it may dial; `None` answers
/// `/tunnel` as a path this host does not serve.
pub fn serve(
    hub: Hub,
    bind: SocketAddr,
    tunnels: Option<kubiq_tunnel::AllowList>,
) -> io::Result<Serving> {
    listen(
        hub,
        Listen {
            bind,
            token: generate_token(),
            tls: None,
            tunnels,
            watch: None,
        },
    )
}

/// Start the listener with TLS: every accepted socket runs its handshake before the HTTP
/// upgrade, and a plaintext dial against it reads nothing but TLS bytes.
///
/// `cert_pem` is the chain, leaf first; `key_pem` the private key — PEM, as `--tls-cert` and
/// `--tls-key` hand them over. A key that matches none of the parsed private-key shapes, or a
/// chain with no certificate in it, refuses here rather than after binding.
pub fn serve_tls(
    hub: Hub,
    bind: SocketAddr,
    cert_pem: &[u8],
    key_pem: &[u8],
    tunnels: Option<kubiq_tunnel::AllowList>,
) -> io::Result<Serving> {
    listen(
        hub,
        Listen {
            bind,
            token: generate_token(),
            tls: Some((cert_pem.to_vec(), key_pem.to_vec())),
            tunnels,
            watch: None,
        },
    )
}

/// The server half of a TLS listener from PEM, refused before anything is bound.
fn tls_config(cert_pem: &[u8], key_pem: &[u8]) -> io::Result<Arc<rustls::ServerConfig>> {
    let invalid = |what: &str| io::Error::new(io::ErrorKind::InvalidInput, what.to_string());
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut &*cert_pem)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| invalid("could not read the TLS certificate"))?;
    if certs.is_empty() {
        return Err(invalid("the TLS certificate file holds no certificate"));
    }
    let key = rustls_pemfile::private_key(&mut &*key_pem)
        .map_err(|_| invalid("could not read the TLS key"))?
        .ok_or_else(|| invalid("the TLS key file holds no private key"))?;
    let config = rustls::ServerConfig::builder_with_provider(tls_provider())
        .with_safe_default_protocol_versions()
        .map_err(|error| invalid(&error.to_string()))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|error| invalid(&error.to_string()))?;
    Ok(Arc::new(config))
}

/// The crypto provider, named rather than inherited — the connector family's `tls::provider`
/// names `ring` for the same reason, and two halves of one handshake must not disagree about it.
fn tls_provider() -> Arc<rustls::crypto::CryptoProvider> {
    use std::sync::OnceLock;
    static PROVIDER: OnceLock<Arc<rustls::crypto::CryptoProvider>> = OnceLock::new();
    PROVIDER
        .get_or_init(|| Arc::new(rustls::crypto::ring::default_provider()))
        .clone()
}

/// 256 bits, rendered URL-safe with no padding — long enough to paste into a connection string and
/// short enough to actually paste.
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes[..]);
    base64_url_no_pad(&bytes)
}

/// A tiny base64url encoder so this module needs no new dependency for what is, in the end, one
/// call site. Standard base64url alphabet, no `=` padding.
fn base64_url_no_pad(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((bytes.len() * 4).div_ceil(3));
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(n & 0x3f) as usize] as char);
        }
    }
    out
}

/// Compare two byte strings without early-returning on a length mismatch or the first differing
/// byte, so how long the comparison takes leaks nothing about how much of a guessed token was
/// right. A length check up front, or a `Vec::iter().eq()`, would both let a timing side-channel
/// narrow the token byte by byte.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let len_ok = a.len() == b.len();
    // Walk both up to the longer length so the loop body itself never depends on which is longer;
    // out-of-bounds reads are replaced with 0, which only ever costs a guaranteed mismatch.
    let n = a.len().max(b.len());
    let mut diff: u8 = if len_ok { 0 } else { 1 };
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= x ^ y;
    }
    diff == 0
}

/// Accept connections until the listener itself fails (the process is going down). One hostile or
/// slow peer is confined to its own thread, per the loop body below, so it can never block this
/// one or the coordinator behind `hub`.
///
/// With a TLS config, each socket runs its handshake on its connection thread before anything is
/// read from it — a plaintext dial against a TLS listener stalls there until the handshake
/// deadline, then is dropped, having read nothing it could mistake for an answer.
fn accept_loop(
    listener: TcpListener,
    hub: Hub,
    token: String,
    tls: Option<Arc<rustls::ServerConfig>>,
    tunnels: Option<kubiq_tunnel::AllowList>,
    listening: Listening,
) {
    // Ends when the listener is stopped; dropping `listener` here is what frees the port.
    while !listening.is_stopped() {
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL);
                continue;
            }
            Err(error) => {
                tracing::warn!("remote: accept failed: {error}");
                thread::sleep(ACCEPT_POLL);
                continue;
            }
        };
        // The listener is non-blocking for the stop flag's sake; a session's socket must not be.
        if stream.set_nonblocking(false).is_err() {
            continue;
        }
        let Some(tracked) = listening.track(&stream) else {
            continue;
        };
        let hub = hub.clone();
        let token = token.clone();
        let tls = tls.clone();
        let tunnels = tunnels.clone();
        thread::Builder::new()
            .name("kubiq-remote-conn".to_string())
            .spawn(move || handle_accepted(stream, hub, token, tls, tunnels, tracked))
            .ok();
    }
}

/// One accepted socket, from the handshake through the life of the session. `tracked` is its row
/// in the listener's table, removed as this returns.
fn handle_accepted(
    stream: TcpStream,
    hub: Hub,
    token: String,
    tls: Option<Arc<rustls::ServerConfig>>,
    tunnels: Option<kubiq_tunnel::AllowList>,
    tracked: Tracked,
) {
    let tracked = &tracked;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT));

    match tls {
        None => {
            let reader = match stream.try_clone() {
                Ok(clone) => clone,
                Err(_) => return,
            };
            // Clearing the deadline on any clone clears it for all of them: socket timeouts
            // are set on the socket, not the file descriptor.
            let clearer = match stream.try_clone() {
                Ok(clone) => clone,
                Err(_) => return,
            };
            // A clone kept only so the writer can `shutdown` the kernel socket. Dropping one
            // half does not send EOF while the reader still holds the other.
            let closer = match stream.try_clone() {
                Ok(clone) => clone,
                Err(_) => return,
            };
            handle_connection(
                Box::new(reader),
                Box::new(stream),
                Box::new(move || {
                    let _ = clearer.set_read_timeout(None);
                }),
                closer,
                hub,
                &token,
                tunnels,
                tracked,
            );
        }
        Some(config) => {
            let mut conn = match rustls::ServerConnection::new(config) {
                Ok(conn) => conn,
                Err(_) => return,
            };
            let mut stream = stream;
            while conn.is_handshaking() {
                if conn.complete_io(&mut stream).is_err() {
                    return;
                }
            }
            let closer = match stream.try_clone() {
                Ok(clone) => clone,
                Err(_) => return,
            };
            // One session for both pumps, never locked across a wait on the socket — so neither
            // direction holds the other up (`kubiq_tunnel::TlsDuplex`).
            let shared = kubiq_tunnel::TlsDuplex::new(conn, stream);
            let clearer = shared.clone();
            handle_connection(
                Box::new(shared.clone()),
                Box::new(shared),
                Box::new(move || {
                    let _ = clearer.socket().set_read_timeout(None);
                }),
                closer,
                hub,
                &token,
                tunnels,
                tracked,
            );
        }
    }
}

/// One connection, from the HTTP upgrade through the life of the session.
///
/// `on_upgrade` runs the moment the `101` is written: the handshake deadline was the
/// handshake's, not the session's, and a client that says nothing for an hour is an idle
/// window, not a stalled peer.
///
/// `/attach` attaches a client to the bus; `/tunnel`, when `tunnels` is set, serves the tunnel
/// channel (`D227`) on the connection instead — same token, and nothing on it reaches the bus.
#[allow(clippy::too_many_arguments)]
fn handle_connection(
    mut reader: Box<dyn Socket>,
    mut writer: Box<dyn Socket>,
    on_upgrade: Box<dyn FnOnce() + Send>,
    closer: TcpStream,
    hub: Hub,
    token: &str,
    tunnels: Option<kubiq_tunnel::AllowList>,
    tracked: &Tracked,
) {
    let request = match read_request(&mut reader) {
        Ok(request) => request,
        Err(_) => return,
    };

    match request {
        Request::Attach { token: presented } => {
            if !constant_time_eq(presented.as_bytes(), token.as_bytes()) {
                let _ = write_response(&mut writer, 401, "Unauthorized", "bad or missing token");
                return;
            }
            let caps: &[&str] = if tunnels.is_some() {
                &[kubiq_proto::carrier::CAP_TUNNEL]
            } else {
                &[]
            };
            if write_response_line(&mut writer, "101 Switching Protocols", caps).is_err() {
                return;
            }
            // The deadline was the handshake's, not the session's: a client that says nothing for
            // an hour is an idle window, not a stalled peer.
            on_upgrade();
            tracked.attached();
            pump(reader, writer, Box::new(SocketCloser(closer)), hub);
        }
        Request::Tunnel { token: presented } => {
            // Off is indistinguishable from a path this host never served.
            let Some(allow) = tunnels else {
                let _ = write_response(&mut writer, 404, "Not Found", "not found");
                return;
            };
            if !constant_time_eq(presented.as_bytes(), token.as_bytes()) {
                let _ = write_response(&mut writer, 401, "Unauthorized", "bad or missing token");
                return;
            }
            if writer
                .write_all(
                    b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: kubiq-tunnel\r\n\
                      Connection: Upgrade\r\n\r\n",
                )
                .is_err()
            {
                return;
            }
            // The peer holds the token; from here it is a link, idle between streams by design.
            on_upgrade();
            let shut = closer.try_clone().ok();
            let mux = match kubiq_tunnel::Mux::drone(
                reader,
                writer,
                Some(Box::new(move || {
                    if let Some(sock) = shut {
                        let _ = sock.shutdown(Shutdown::Both);
                    }
                })),
            ) {
                Ok(mux) => mux,
                Err(error) => {
                    tracing::warn!(target: "tunnel", "remote: the tunnel hello failed: {error}");
                    let _ = closer.shutdown(Shutdown::Both);
                    return;
                }
            };
            kubiq_tunnel::serve(&mux, &allow);
            mux.close();
            let _ = closer.shutdown(Shutdown::Both);
        }
        Request::Root => {
            let _ = write_response(
                &mut writer,
                200,
                "OK",
                "this is a Kubiq host. attach at /attach?token=<token>.",
            );
        }
        Request::Other => {
            let _ = write_response(&mut writer, 404, "Not Found", "not found");
        }
        Request::TooLarge => {
            let _ = write_response(&mut writer, 400, "Bad Request", "header too large");
        }
    }
}

enum Request {
    /// `GET /attach?token=<token>` with an `Upgrade: kubiq` header.
    Attach { token: String },
    /// `GET /tunnel?token=<token>` with an `Upgrade: kubiq-tunnel` header.
    Tunnel { token: String },
    /// A plain `GET /`.
    Root,
    /// Anything else recognisable as an HTTP request.
    Other,
    /// The header block ran past [`MAX_HEADER`] before a blank line ended it.
    TooLarge,
}

/// Read one HTTP request's header block with a hard cap, and pick out only what this handshake
/// needs: the request line's path, and whether an `Upgrade: kubiq` header was sent. The body, if
/// any, is never read — none of these requests has one worth reading.
///
/// Reads one byte at a time, directly off the socket, deliberately not through a `BufReader`: a
/// buffered reader can pull well past the header block's end in one underlying read, and anything
/// past the blank line is the first wire frame once this handshake upgrades — bytes this function
/// must leave on the socket for [`pump`] to read, not swallow into a buffer that is about to be
/// dropped. One byte per syscall is a one-time cost on a connection that then runs for a whole
/// session.
fn read_request(stream: &mut dyn Read) -> io::Result<Request> {
    let mut total = 0usize;
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        let n = read_capped_line(stream, &mut line, MAX_HEADER - total)?;
        match n {
            None => return Ok(Request::TooLarge),
            Some(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            Some(read) => total += read,
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        lines.push(trimmed.to_string());
        if total >= MAX_HEADER {
            return Ok(Request::TooLarge);
        }
    }

    let Some(request_line) = lines.first() else {
        return Ok(Request::Other);
    };
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(path)) = (parts.next(), parts.next()) else {
        return Ok(Request::Other);
    };
    if method != "GET" {
        return Ok(Request::Other);
    }

    if path == "/" {
        return Ok(Request::Root);
    }

    let (query, protocol, tunnel) = if let Some(query) = path.strip_prefix("/attach?") {
        (query, "kubiq", false)
    } else if let Some(query) = path.strip_prefix("/tunnel?") {
        (query, "kubiq-tunnel", true)
    } else {
        return Ok(Request::Other);
    };
    let upgraded = lines.iter().skip(1).any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.trim().eq_ignore_ascii_case("upgrade")
                && value.trim().eq_ignore_ascii_case(protocol)
        })
    });
    if !upgraded {
        return Ok(Request::Other);
    }
    let token = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
        .unwrap_or("")
        .to_string();
    Ok(if tunnel {
        Request::Tunnel { token }
    } else {
        Request::Attach { token }
    })
}

/// Read one line, refusing to grow the buffer past `budget` bytes. `Ok(None)` is "ran out of
/// budget before a newline"; `Ok(Some(0))` is a clean EOF with nothing read at all.
fn read_capped_line(
    reader: &mut dyn Read,
    line: &mut String,
    budget: usize,
) -> io::Result<Option<usize>> {
    let mut byte = [0u8; 1];
    let mut read = 0usize;
    loop {
        if read >= budget {
            return Ok(None);
        }
        match reader.read(&mut byte) {
            Ok(0) => return Ok(Some(read)),
            Ok(_) => {
                read += 1;
                line.push(byte[0] as char);
                if byte[0] == b'\n' {
                    return Ok(Some(read));
                }
            }
            Err(error) => return Err(error),
        }
    }
}

fn write_response(stream: &mut dyn Write, code: u16, reason: &str, body: &str) -> io::Result<()> {
    let response = format!(
        "HTTP/1.1 {code} {reason}\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );
    stream.write_all(response.as_bytes())
}

/// The attach's `101`, naming `caps` in [`CAPS_HEADER`] when there are any.
fn write_response_line(stream: &mut dyn Write, status: &str, caps: &[&str]) -> io::Result<()> {
    let caps = if caps.is_empty() {
        String::new()
    } else {
        format!("{CAPS_HEADER}: {}\r\n", caps.join(", "))
    };
    stream.write_all(
        format!("HTTP/1.1 {status}\r\nUpgrade: kubiq\r\nConnection: Upgrade\r\n{caps}\r\n")
            .as_bytes(),
    )
}

/// Ending a TCP session's read direction: shut the kernel socket down while a handle still
/// exists. Dropping a `TlsDuplex` handle or one TCP clone does not send EOF while the reader holds
/// the other half, so the host would never see `Gone` and would not reap panes.
struct SocketCloser(TcpStream);

impl Closer for SocketCloser {
    fn close(&self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}
