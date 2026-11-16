//! The notebook engine. Pure Rust — no GPUI, no Kubiq crate, no I/O.
//!
//! - [`json`] — a JSON tree that keeps key order and number text, and prints the way the file's
//!   writer did: what makes a notebook round-trip byte for byte.
//! - [`nbformat`] — the `.ipynb` file: cells, outputs, the writer, the kernel-message → output
//!   conversion.
//! - [`doc`] — the live notebook: id-addressed cell operations, the notebook-level undo stack with
//!   its [`doc::Origin`]s, staleness, reload matched by cell id.
//! - [`output`] — what an output becomes on screen or for an agent: terminal text, the richest
//!   MIME type, images.
//! - [`env`] — the Python environment resolver's rules and kernelspec synthesis, over a
//!   [`env::Probe`] the host implements.
//! - [`wire`] — the Jupyter message (HMAC-SHA256 signed) and its ZMTP 3.0 framing.
//!
//! The host (`kubiq-host/src/notebook/`) owns the file, the kernel process and the sockets; the
//! interface draws [`doc::CellView`]s and calls [`output`]'s functions. Both re-export through
//! `kubiq-proto::notebook`.

pub mod doc;
pub mod env;
pub mod json;
pub mod nbformat;
pub mod output;
pub mod wire;

pub use doc::{CellOp, CellView, Notebook, Origin};
pub use nbformat::{CellKind, Output};

/// The ports of one kernel's five channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ports {
    pub shell: u16,
    pub iopub: u16,
    pub stdin: u16,
    pub control: u16,
    pub hb: u16,
}

/// A connection file's text, as `jupyter_client` writes one.
pub fn connection_file(ports: Ports, key: &str, kernel_name: &str) -> String {
    use json::Json;
    let n = |p: u16| Json::Num(p.to_string());
    let v = Json::sorted(vec![
        ("control_port".into(), n(ports.control)),
        ("hb_port".into(), n(ports.hb)),
        ("iopub_port".into(), n(ports.iopub)),
        ("ip".into(), Json::str("127.0.0.1")),
        ("kernel_name".into(), Json::str(kernel_name)),
        ("key".into(), Json::str(key)),
        ("shell_port".into(), n(ports.shell)),
        ("signature_scheme".into(), Json::str("hmac-sha256")),
        ("stdin_port".into(), n(ports.stdin)),
        ("transport".into(), Json::str("tcp")),
    ]);
    json::write(&v, json::Style::default())
}
