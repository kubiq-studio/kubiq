//! The window's half: a loopback listener per enabled [`TunnelSpec`], spliced to the attached mux.
//!
//! Listeners are bound once, at [`TunnelManager::start`], and survive the link: a connection
//! accepted with no live mux under them is refused with a sentence — a `502` for the HTTP kind, a
//! close for TCP — and recorded as the spec's last error, until [`TunnelManager::attach`] gives
//! them a new one. The HTTP kind answers `403` to a request whose `Host` is not the loopback — the
//! DNS-rebinding guard in [`crate::http`]. The proxy kind reads each connection's first request
//! head before it opens anything, since that head names the destination ([`crate::proxy`]).

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use kubiq_proto::settings::{TunnelKind, TunnelSpec};
use parking_lot::Mutex;
use rustls::pki_types::ServerName;

use crate::http::{self, Rewrite};
use crate::mux::Mux;
use crate::pipe::{self, Counters};
use crate::{proxy, tls};

/// How often an idle listener looks at the stop flag.
const ACCEPT_POLL: Duration = Duration::from_millis(50);
/// How long a proxy client may take to send its first request head.
const PROXY_HEAD_TIMEOUT: Duration = Duration::from_secs(30);

/// One spec's state, as the settings page or a status line would draw it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TunnelStatus {
    /// The [`TunnelSpec::id`].
    pub id: String,
    pub local_port: u16,
    /// Whether the loopback listener is bound.
    pub listening: bool,
    /// Whether a live link is attached under it.
    pub linked: bool,
    /// Connections accepted, ever.
    pub conns: u64,
    /// Connections open now.
    pub active: u64,
    /// Bytes read from local clients, sent toward the far machine.
    pub bytes_up: u64,
    /// Bytes from the far machine written to local clients.
    pub bytes_down: u64,
    /// The latest failure — a bind, a refused dial, a dropped link — as a sentence.
    pub last_error: Option<String>,
}

struct Tunnel {
    spec: TunnelSpec,
    counters: Arc<Counters>,
    listening: AtomicBool,
}

/// Where a following manager finds its link, asked once per accepted connection.
pub type Source = Arc<dyn Fn() -> Option<Mux> + Send + Sync>;

struct Shared {
    mux: Mutex<Option<Mux>>,
    /// A following manager's link, borrowed per connection from whoever owns it — never closed
    /// from here.
    source: Option<Source>,
    /// Whether the source answered with a live link at the last connection.
    followed: AtomicBool,
    stop: AtomicBool,
}

/// Every forwarded port of one saved host. Dropped, it stops listening and closes its link.
pub struct TunnelManager {
    shared: Arc<Shared>,
    tunnels: Vec<Arc<Tunnel>>,
    /// The accept loops, joined by [`Self::stop`] so the ports are free when it returns.
    listeners: Mutex<Vec<thread::JoinHandle<()>>>,
}

impl TunnelManager {
    /// Bind `127.0.0.1:local_port` for every enabled spec. A port that will not bind is reported
    /// in that spec's status and does not stop the others. `mux` may come later, by
    /// [`Self::attach`].
    pub fn start(specs: &[TunnelSpec], mux: Option<Mux>) -> TunnelManager {
        Self::bind(specs, mux, None)
    }

    /// Bind like [`Self::start`], but own no link: every accepted connection asks `source` for
    /// one — another manager's ([`crate::Registry::follow`]) — and a stop leaves it open.
    pub fn follow(specs: &[TunnelSpec], source: Source) -> TunnelManager {
        Self::bind(specs, None, Some(source))
    }

    fn bind(specs: &[TunnelSpec], mux: Option<Mux>, source: Option<Source>) -> TunnelManager {
        let shared = Arc::new(Shared {
            mux: Mutex::new(mux),
            source,
            followed: AtomicBool::new(false),
            stop: AtomicBool::new(false),
        });
        let mut tunnels = Vec::new();
        let mut listeners = Vec::new();
        for spec in specs.iter().filter(|s| s.enabled) {
            let tunnel = Arc::new(Tunnel {
                spec: spec.clone(),
                counters: Arc::new(Counters::default()),
                listening: AtomicBool::new(false),
            });
            match TcpListener::bind((Ipv4Addr::LOCALHOST, spec.local_port)) {
                Ok(listener) => {
                    tunnel.listening.store(true, Ordering::Relaxed);
                    let (shared, tunnel) = (shared.clone(), tunnel.clone());
                    let spawned = thread::Builder::new()
                        .name(format!("kubiq-tunnel-listen-{}", spec.local_port))
                        .spawn(move || accept_loop(shared, tunnel, listener));
                    match spawned {
                        Ok(handle) => listeners.push(handle),
                        Err(e) => {
                            tunnels.push(fail(spec, format!("could not start the listener: {e}")));
                            continue;
                        }
                    }
                }
                Err(e) => tunnel.counters.error(format!(
                    "could not listen on 127.0.0.1:{}: {e}",
                    spec.local_port
                )),
            }
            tunnels.push(tunnel);
        }
        TunnelManager {
            shared,
            tunnels,
            listeners: Mutex::new(listeners),
        }
    }

    /// Put `mux` under every listener, closing the one it replaces.
    pub fn attach(&self, mux: Mux) {
        let old = self.shared.mux.lock().replace(mux);
        if let Some(old) = old {
            old.close();
        }
    }

    /// Close the link; the listeners stay and refuse until the next [`Self::attach`].
    pub fn detach(&self) {
        let old = self.shared.mux.lock().take();
        if let Some(old) = old {
            old.close();
        }
    }

    /// Whether a live link is attached.
    pub fn linked(&self) -> bool {
        self.mux().is_some() || self.shared.followed.load(Ordering::Relaxed)
    }

    /// The attached link, while it is alive — for a [`Self::follow`]er to borrow.
    pub fn mux(&self) -> Option<Mux> {
        self.shared.mux.lock().clone().filter(Mux::is_alive)
    }

    /// Every enabled spec's state, in the order given to [`Self::start`].
    pub fn status(&self) -> Vec<TunnelStatus> {
        let linked = self.linked();
        self.tunnels
            .iter()
            .map(|t| TunnelStatus {
                id: t.spec.id.clone(),
                local_port: t.spec.local_port,
                listening: t.listening.load(Ordering::Relaxed),
                linked,
                conns: t.counters.conns.load(Ordering::Relaxed),
                active: t.counters.active.load(Ordering::Relaxed),
                bytes_up: t.counters.up.load(Ordering::Relaxed),
                bytes_down: t.counters.down.load(Ordering::Relaxed),
                last_error: t.counters.last_error.lock().clone(),
            })
            .collect()
    }

    /// Stop listening and close the link. Connections in flight reset with it. Returns once every
    /// listener is closed — within one [`ACCEPT_POLL`] — so a new manager can bind the same ports
    /// straight away.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.detach();
        let listeners = std::mem::take(&mut *self.listeners.lock());
        for handle in listeners {
            let _ = handle.join();
        }
    }
}

impl Drop for TunnelManager {
    fn drop(&mut self) {
        self.stop();
    }
}

fn fail(spec: &TunnelSpec, error: String) -> Arc<Tunnel> {
    let counters = Arc::new(Counters::default());
    counters.error(error);
    Arc::new(Tunnel {
        spec: spec.clone(),
        counters,
        listening: AtomicBool::new(false),
    })
}

fn accept_loop(shared: Arc<Shared>, tunnel: Arc<Tunnel>, listener: TcpListener) {
    if let Err(e) = listener.set_nonblocking(true) {
        tunnel.counters.error(format!("the listener failed: {e}"));
    }
    while !shared.stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((socket, _)) => {
                // An accepted socket inherits the listener's non-blocking flag on some platforms.
                let _ = socket.set_nonblocking(false);
                let _ = socket.set_nodelay(true);
                let (shared, tunnel) = (shared.clone(), tunnel.clone());
                let _ = thread::Builder::new()
                    .name("kubiq-tunnel-conn".into())
                    .spawn(move || connection(&shared, &tunnel, socket));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(ACCEPT_POLL),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => {
                tunnel.counters.error(format!("the listener failed: {e}"));
                thread::sleep(ACCEPT_POLL * 4);
            }
        }
    }
    tunnel.listening.store(false, Ordering::Relaxed);
}

struct Active<'a>(&'a Counters);

impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::Relaxed);
    }
}

fn connection(shared: &Shared, tunnel: &Tunnel, socket: TcpStream) {
    let spec = &tunnel.spec;
    let counters = &tunnel.counters;
    counters.conns.fetch_add(1, Ordering::Relaxed);
    counters.active.fetch_add(1, Ordering::Relaxed);
    let _active = Active(counters);

    let mut mux = shared.mux.lock().clone().filter(Mux::is_alive);
    if let Some(source) = &shared.source {
        mux = source().filter(Mux::is_alive);
        shared.followed.store(mux.is_some(), Ordering::Relaxed);
    }
    let Some(mux) = mux else {
        refuse(
            socket,
            spec.kind,
            "the tunnel link is down: no drone is attached to forward this port".into(),
            counters,
        );
        return;
    };
    if spec.kind == TunnelKind::Proxy {
        proxy_connection(&mux, socket, counters);
        return;
    }
    let stream = match mux.open(&spec.host, spec.port) {
        Ok(stream) => stream,
        Err(e) => {
            refuse(
                socket,
                spec.kind,
                format!("{}:{} was refused: {e}", spec.host, spec.port),
                counters,
            );
            return;
        }
    };
    match spec.kind {
        TunnelKind::Tcp | TunnelKind::Proxy => {
            pipe::pipe(&mux, socket, stream, None, None, Some(counters.clone()))
        }
        TunnelKind::Http => {
            let (mut requests, responses) = http::rewriters(Rewrite::new(spec));
            if spec.upstream_tls {
                let server = match ServerName::try_from(spec.host.clone()) {
                    Ok(name) => name,
                    Err(e) => {
                        stream.reset(None);
                        refuse(
                            socket,
                            spec.kind,
                            format!("{} is not a TLS server name: {e}", spec.host),
                            counters,
                        );
                        return;
                    }
                };
                tls::pipe_tls(
                    &mux,
                    socket,
                    stream,
                    (requests, responses),
                    tls::client_config(spec.insecure),
                    server,
                    counters.clone(),
                );
            } else {
                let transform: pipe::Transform =
                    Box::new(move |input, out| requests.feed(input, out));
                pipe::pipe(
                    &mux,
                    socket,
                    stream,
                    Some(transform),
                    Some(responses),
                    Some(counters.clone()),
                );
            }
        }
    }
}

/// The proxy kind: read the first request head, open what it names, then splice. A `CONNECT` is
/// answered `200` here and piped raw; a forwarded request goes out rewritten to origin form.
fn proxy_connection(mux: &Mux, mut socket: TcpStream, counters: &Arc<Counters>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let _ = socket.set_read_timeout(Some(PROXY_HEAD_TIMEOUT));
    let parsed = loop {
        match proxy::parse(&buf) {
            Ok(Some(parsed)) => break Ok(parsed),
            Ok(None) => {}
            Err(e) => break Err(e.to_string()),
        }
        match socket.read(&mut chunk) {
            // Closed, or silent past the timeout, before a whole head: nothing to answer.
            Ok(0) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    };
    let _ = socket.set_read_timeout(None);
    let (request, used) = match parsed {
        Ok(parsed) => parsed,
        Err(e) => return refuse_with(socket, Some("400 Bad Request"), e, counters),
    };
    let stream = match mux.open(&request.host, request.port) {
        Ok(stream) => stream,
        Err(e) => {
            let error = format!("{}:{} was refused: {e}", request.host, request.port);
            return refuse_with(socket, Some("502 Bad Gateway"), error, counters);
        }
    };
    if request.connect
        && socket
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .is_err()
    {
        stream.reset(None);
        return;
    }
    let mut first = request.head;
    first.extend_from_slice(&buf[used..]);
    if !first.is_empty() {
        counters.up.fetch_add(first.len() as u64, Ordering::Relaxed);
        if stream.send(&first).is_err() {
            return;
        }
    }
    pipe::pipe(mux, socket, stream, None, None, Some(counters.clone()));
}

/// Turn a local connection away, saying why: a `502` for HTTP and the proxy, a plain close for
/// TCP.
fn refuse(socket: TcpStream, kind: TunnelKind, error: String, counters: &Counters) {
    let status = (kind != TunnelKind::Tcp).then_some("502 Bad Gateway");
    refuse_with(socket, status, error, counters);
}

/// Turn a local connection away with `status` (an HTTP answer) or, without one, a plain close.
fn refuse_with(mut socket: TcpStream, status: Option<&str>, error: String, counters: &Counters) {
    counters.error(error.clone());
    if let Some(status) = status {
        let _ = socket.write_all(&proxy::answer(status, &error));
    }
    let _ = socket.shutdown(Shutdown::Write);
    // Drain what the client already sent, so the close is a FIN and not a reset that eats the
    // answer.
    let _ = socket.set_read_timeout(Some(Duration::from_millis(500)));
    let mut sink = [0u8; 4096];
    while matches!(socket.read(&mut sink), Ok(n) if n > 0) {}
    let _ = socket.shutdown(Shutdown::Both);
}
