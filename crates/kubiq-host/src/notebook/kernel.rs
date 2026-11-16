//! The kernel manager: one Jupyter kernel process and its five channels.
//!
//! [`start`] does the whole lifecycle's first half and blocks while it does, so the worker runs it
//! on a thread of its own: five free loopback ports and a random HMAC key written to a connection
//! file (`<config root>/runtime/kernels/kernel-<id>.json`, mode 0600), the kernelspec's argv spawned
//! in its own process group with the notebook's folder as its working directory, then ready =
//! a heartbeat answer and a `kernel_info_reply`, with iopub proven subscribed by a message on it.
//! Every socket then gets a reader thread that forwards whole messages to the worker as
//! [`KernelEvent`]s, tagged so a restarted kernel's stragglers are recognisable; an exit watcher
//! reports a kernel that dies on its own with its status and the tail of what it printed.
//!
//! The wire protocol is Kubiq's own over [`super::zmtp`] (`kubiq_notebook::wire`), not a library:
//! see the notebook feature document for why.

use std::io::Read;
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kubiq_notebook::Ports;
use kubiq_notebook::env::{InterruptMode, KernelSpec};
use kubiq_notebook::json::Json;
use kubiq_notebook::wire::JupyterMessage;

use super::zmtp::Socket;

/// How long a kernel may take to come up. `uv run --with ipykernel` may have to fetch it first.
pub const READY_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a shutdown waits for the kernel to leave before it is killed.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);
/// How much of the kernel's own output is kept for a `KernelDied`.
const TAIL_BYTES: usize = 4000;

/// Which channel a message came in on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Shell,
    Iopub,
    Stdin,
    Control,
}

/// What a kernel's threads tell the worker.
#[derive(Debug)]
pub enum KernelEvent {
    Message(Channel, JupyterMessage),
    /// The process ended (or could not be read any longer).
    Exited { code: Option<i32>, tail: String },
}

/// A tagged event: `tag` is the generation [`start`] was given.
pub type Tagged = (u64, KernelEvent);

pub struct Launch {
    pub spec: KernelSpec,
    pub cwd: PathBuf,
    /// `<config root>/runtime/kernels`.
    pub runtime_dir: PathBuf,
}

pub struct Kernel {
    key: Vec<u8>,
    session: String,
    counter: u64,
    shell: Socket,
    control: Socket,
    stdin: Socket,
    /// Read halves, kept to be shut down.
    readers: Vec<Socket>,
    child: Arc<Mutex<Child>>,
    connection_file: PathBuf,
    interrupt_mode: InterruptMode,
    /// The process group (Unix) or Job object (Windows) the kernel runs in.
    group: kubiq_cells::process::Group,
    /// Windows' interrupt event; `None` elsewhere.
    event: Option<kubiq_cells::process::InterruptEvent>,
    /// `language_info.name` from the kernel's info reply.
    pub language: Option<String>,
}

/// Random bytes: the OS's where it offers them, else the standard library's per-process random
/// hasher seeds (themselves drawn from the OS).
pub fn random_bytes(n: usize) -> Vec<u8> {
    #[cfg(unix)]
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let mut buf = vec![0u8; n];
        if f.read_exact(&mut buf).is_ok() {
            return buf;
        }
    }
    use std::hash::{BuildHasher, Hasher};
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_usize(out.len());
        out.extend_from_slice(&h.finish().to_le_bytes());
    }
    out.truncate(n);
    out
}

fn free_ports() -> std::io::Result<Ports> {
    // Held together so the five are distinct, then released for the kernel to bind.
    let listeners: Vec<TcpListener> = (0..5)
        .map(|_| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)))
        .collect::<Result<_, _>>()?;
    let port = |n: usize| listeners[n].local_addr().map(|a| a.port());
    Ok(Ports {
        shell: port(0)?,
        iopub: port(1)?,
        stdin: port(2)?,
        control: port(3)?,
        hb: port(4)?,
    })
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(text.as_bytes())
}

/// Delete connection files a crashed host left behind: none of their kernels is ours any more.
pub fn sweep(runtime_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(runtime_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("kernel-") && name.ends_with(".json") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn keep_tail(tail: &Arc<Mutex<String>>, mut from: impl Read + Send + 'static) {
    let tail = Arc::clone(tail);
    let _ = std::thread::Builder::new()
        .name("kubiq-kernel-output".into())
        .spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(n) = from.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let mut t = tail.lock().unwrap_or_else(|e| e.into_inner());
                t.push_str(&String::from_utf8_lossy(&chunk[..n]));
                if t.len() > TAIL_BYTES {
                    let mut cut = t.len() - TAIL_BYTES;
                    while !t.is_char_boundary(cut) {
                        cut += 1;
                    }
                    t.drain(..cut);
                }
            }
        });
}

fn exited(child: &Arc<Mutex<Child>>) -> Option<Option<i32>> {
    match child.lock().unwrap_or_else(|e| e.into_inner()).try_wait() {
        Ok(Some(status)) => Some(status.code()),
        Ok(None) => None,
        Err(_) => Some(None),
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

impl Kernel {
    fn message(&mut self, msg_type: &str, content: Json) -> JupyterMessage {
        self.counter += 1;
        let id = format!("{}_{}", self.session, self.counter);
        JupyterMessage::new(msg_type, content, &id, &self.session, &now())
    }

    fn send(&mut self, channel: Channel, msg: &JupyterMessage) -> Result<(), String> {
        let frames = msg.encode(&self.key);
        let socket = match channel {
            Channel::Control => &mut self.control,
            Channel::Stdin => &mut self.stdin,
            _ => &mut self.shell,
        };
        socket.send(&frames).map_err(|e| format!("kernel socket: {e}"))
    }

    /// Send `execute_request`; returns its `msg_id`, which every output names as its parent.
    pub fn execute(&mut self, code: &str) -> Result<String, String> {
        let msg = self.message(
            "execute_request",
            Json::sorted(vec![
                ("allow_stdin".into(), Json::Bool(true)),
                ("code".into(), Json::str(code)),
                ("silent".into(), Json::Bool(false)),
                ("stop_on_error".into(), Json::Bool(true)),
                ("store_history".into(), Json::Bool(true)),
                ("user_expressions".into(), Json::Obj(Vec::new())),
            ]),
        );
        self.send(Channel::Shell, &msg)?;
        Ok(msg.msg_id().to_string())
    }

    /// Answer an `input_request` whose header was `parent`.
    pub fn input_reply(&mut self, parent: Json, value: &str) -> Result<(), String> {
        let mut msg = self.message("input_reply", Json::sorted(vec![("value".into(), Json::str(value))]));
        msg.parent_header = parent;
        self.send(Channel::Stdin, &msg)
    }

    /// Interrupt as the kernelspec asks: SIGINT to the process group, or `interrupt_request`.
    pub fn interrupt(&mut self) -> Result<(), String> {
        if self.interrupt_mode == InterruptMode::Signal {
            // Windows: the event the kernel was handed in `JPY_INTERRUPT_EVENT`; Unix: SIGINT to
            // the group the kernel leads.
            let sent = match &self.event {
                Some(event) => event.fire(),
                None => self.group.interrupt(),
            };
            if sent.is_ok() {
                return Ok(());
            }
        }
        let msg = self.message("interrupt_request", Json::Obj(Vec::new()));
        self.send(Channel::Control, &msg)
    }

    /// Ask the kernel to leave, wait [`SHUTDOWN_GRACE`], then kill it. The connection file goes.
    pub fn shutdown(mut self) {
        let msg = self.message(
            "shutdown_request",
            Json::sorted(vec![("restart".into(), Json::Bool(false))]),
        );
        let _ = self.send(Channel::Control, &msg);
        let until = Instant::now() + SHUTDOWN_GRACE;
        while exited(&self.child).is_none() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
        if exited(&self.child).is_none() {
            // The whole tree: a kernel's own children (a multiprocessing pool) go with it.
            let _ = self.group.kill_tree();
            let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
            let _ = child.kill();
            let _ = child.wait();
        }
        for socket in self.readers.iter().chain([&self.shell, &self.control, &self.stdin]) {
            socket.shutdown();
        }
        let _ = std::fs::remove_file(&self.connection_file);
    }
}

fn reader(mut socket: Socket, channel: Channel, key: Vec<u8>, tag: u64, events: flume::Sender<Tagged>) {
    let _ = std::thread::Builder::new()
        .name("kubiq-kernel-read".into())
        .spawn(move || {
            while let Ok(frames) = socket.recv() {
                match JupyterMessage::decode(&frames, &key) {
                    Ok(msg) => {
                        if events.send((tag, KernelEvent::Message(channel, msg))).is_err() {
                            break;
                        }
                    }
                    Err(error) => tracing::warn!("a kernel message was dropped: {error}"),
                }
            }
        });
}

/// Launch a kernel and wait until it answers. Blocks for up to [`READY_TIMEOUT`].
pub fn start(launch: Launch, tag: u64, events: flume::Sender<Tagged>) -> Result<Kernel, String> {
    let ports = free_ports().map_err(|e| format!("No free port: {e}"))?;
    let key_text: String = random_bytes(32).iter().map(|b| format!("{b:02x}")).collect();
    let id = kubiq_proto::ids::NotebookRunId::generate().to_string().to_lowercase();
    let connection_file = launch.runtime_dir.join(format!("kernel-{id}.json"));
    write_private(
        &connection_file,
        &kubiq_notebook::connection_file(ports, &key_text, &launch.spec.display_name),
    )
    .map_err(|e| format!("Connection file not written: {e}"))?;
    let argv = launch
        .spec
        .launch_argv(&connection_file.to_string_lossy(), "");
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .current_dir(&launch.cwd)
        .envs(launch.spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let event = kubiq_cells::process::InterruptEvent::new();
    if let Some(event) = &event {
        cmd.envs(event.env());
    }
    let (mut child, group) = match kubiq_cells::process::spawn(&mut cmd) {
        Ok(spawned) => spawned,
        Err(e) => {
            let _ = std::fs::remove_file(&connection_file);
            return Err(format!("Kernel not started: {}: {e}", argv[0]));
        }
    };
    let tail = Arc::new(Mutex::new(String::new()));
    if let Some(out) = child.stdout.take() {
        keep_tail(&tail, out);
    }
    if let Some(err) = child.stderr.take() {
        keep_tail(&tail, err);
    }
    let child = Arc::new(Mutex::new(child));
    // A failed start takes the whole tree down: under `uv run` the leader is uv, and the kernel is
    // its child.
    let fail = |what: String, child: &Arc<Mutex<Child>>, group: Option<&kubiq_cells::process::Group>| {
        if let Some(group) = group {
            let _ = group.kill_tree();
        }
        let mut c = child.lock().unwrap_or_else(|e| e.into_inner());
        let _ = c.kill();
        let _ = c.wait();
        let _ = std::fs::remove_file(&connection_file);
        let tail = tail.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if tail.trim().is_empty() {
            what
        } else {
            format!("{what}\n{}", tail.trim_end())
        }
    };
    let deadline = Instant::now() + READY_TIMEOUT;
    let session = format!("kubiq-{id}");
    let identity = session.as_bytes().to_vec();

    // The shell socket proves the kernel is listening; poll its exit meanwhile.
    let shell = loop {
        if let Some(code) = exited(&child) {
            return Err(fail(format!("Kernel exited during start: status {code:?}"), &child, Some(&group)));
        }
        if Instant::now() >= deadline {
            return Err(fail("Kernel did not start: timed out".into(), &child, Some(&group)));
        }
        match Socket::connect(ports.shell, "DEALER", Some(&identity), Instant::now() + Duration::from_secs(1)) {
            Ok(socket) => break socket,
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    };
    let connect = |port, kind, id: Option<&[u8]>| {
        Socket::connect(port, kind, id, deadline).map_err(|e| format!("Kernel socket: {e}"))
    };
    let sockets = (|| {
        let stdin = connect(ports.stdin, "DEALER", Some(&identity))?;
        let control = connect(ports.control, "DEALER", None)?;
        let mut iopub = connect(ports.iopub, "SUB", None)?;
        iopub.send(&[vec![1u8]]).map_err(|e| format!("Kernel socket: {e}"))?;
        let mut hb = connect(ports.hb, "REQ", None)?;
        hb.send(&[Vec::new(), b"ping".to_vec()]).map_err(|e| format!("Heartbeat: {e}"))?;
        hb.set_read_timeout(Some(Duration::from_secs(10))).ok();
        hb.recv().map_err(|e| format!("Heartbeat: {e}"))?;
        hb.shutdown();
        Ok::<_, String>((stdin, control, iopub))
    })();
    let (stdin, control, mut iopub) = match sockets {
        Ok(s) => s,
        Err(e) => return Err(fail(e, &child, Some(&group))),
    };
    let key = key_text.into_bytes();
    let mut kernel = Kernel {
        key: key.clone(),
        session,
        counter: 0,
        shell,
        control,
        stdin,
        readers: Vec::new(),
        child: Arc::clone(&child),
        connection_file: connection_file.clone(),
        interrupt_mode: launch.spec.interrupt_mode,
        group,
        event,
        language: None,
    };

    // kernel_info until it answers and iopub has said anything (proof the subscription is live).
    let mut ready = false;
    kernel.shell.set_read_timeout(Some(Duration::from_secs(2))).ok();
    iopub.set_read_timeout(Some(Duration::from_millis(500))).ok();
    while !ready && Instant::now() < deadline {
        if let Some(code) = exited(&child) {
            return Err(fail(format!("Kernel exited during start: status {code:?}"), &child, Some(&kernel.group)));
        }
        let info = kernel.message("kernel_info_request", Json::Obj(Vec::new()));
        if kernel.send(Channel::Shell, &info).is_err() {
            break;
        }
        let Ok(frames) = kernel.shell.recv() else { continue };
        let Ok(reply) = JupyterMessage::decode(&frames, &key) else { continue };
        kernel.language = reply
            .content
            .get("language_info")
            .and_then(|l| l.get("name"))
            .and_then(Json::as_str)
            .map(str::to_string);
        for _ in 0..4 {
            if iopub.recv().is_ok() {
                ready = true;
                break;
            }
        }
    }
    if !ready {
        kernel.shutdown();
        return Err(fail("Kernel did not answer: timed out".into(), &child, None));
    }
    kernel.shell.set_read_timeout(None).ok();
    iopub.set_read_timeout(None).ok();
    // Anything else the start's own requests caused is stale; the readers start clean.
    let read = |s: &Socket| s.try_clone().map_err(|e| format!("Kernel socket: {e}"));
    let halves = (|| Ok::<_, String>((read(&kernel.shell)?, read(&kernel.stdin)?, read(&kernel.control)?)))();
    let (shell_r, stdin_r, control_r) = match halves {
        Ok(h) => h,
        Err(e) => {
            kernel.shutdown();
            return Err(e);
        }
    };
    kernel.readers = [&shell_r, &stdin_r, &control_r, &iopub]
        .iter()
        .filter_map(|s| s.try_clone().ok())
        .collect();
    reader(shell_r, Channel::Shell, key.clone(), tag, events.clone());
    reader(stdin_r, Channel::Stdin, key.clone(), tag, events.clone());
    reader(control_r, Channel::Control, key.clone(), tag, events.clone());
    reader(iopub, Channel::Iopub, key, tag, events.clone());

    let watched = Arc::clone(&child);
    let _ = std::thread::Builder::new()
        .name("kubiq-kernel-exit".into())
        .spawn(move || loop {
            if let Some(code) = exited(&watched) {
                let tail = tail.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let _ = events.send((tag, KernelEvent::Exited { code, tail }));
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        });
    Ok(kernel)
}
