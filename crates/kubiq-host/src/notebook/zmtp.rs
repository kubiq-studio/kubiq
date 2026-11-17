//! One ZMTP 3.0 connection to a kernel's socket, over a blocking loopback `TcpStream`.
//!
//! The bytes are [`kubiq_notebook::wire`]'s; this is only the socket: connect (retrying while the
//! kernel is still binding its ports), greet, exchange READY, then whole multipart messages in and
//! out. A reader thread owns a clone of the stream and the worker writes on the original, so
//! neither waits for the other.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use kubiq_notebook::wire;

pub struct Socket {
    stream: TcpStream,
    buf: Vec<u8>,
}

impl Socket {
    /// Connect and handshake as `socket_type` (`DEALER`, `SUB`, `REQ`), until `deadline`.
    pub fn connect(port: u16, socket_type: &str, identity: Option<&[u8]>, deadline: Instant) -> io::Result<Socket> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let stream = loop {
            match TcpStream::connect_timeout(&addr, Duration::from_millis(500)) {
                Ok(stream) => break stream,
                Err(e) if Instant::now() >= deadline => return Err(e),
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        };
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(100))))?;
        let mut socket = Socket {
            stream,
            buf: Vec::new(),
        };
        socket.stream.write_all(&wire::greeting())?;
        let mut theirs = [0u8; 64];
        socket.stream.read_exact(&mut theirs)?;
        wire::check_greeting(&theirs).map_err(io::Error::other)?;
        socket.stream.write_all(&wire::ready(socket_type, identity))?;
        // Their READY.
        loop {
            let frame = socket.frame()?;
            if frame.command {
                break;
            }
        }
        socket.stream.set_read_timeout(None)?;
        Ok(socket)
    }

    pub fn try_clone(&self) -> io::Result<Socket> {
        Ok(Socket {
            stream: self.stream.try_clone()?,
            buf: Vec::new(),
        })
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.stream.set_read_timeout(timeout)
    }

    pub fn send(&mut self, parts: &[Vec<u8>]) -> io::Result<()> {
        self.stream.write_all(&wire::encode_frames(parts))
    }

    fn frame(&mut self) -> io::Result<wire::Frame> {
        loop {
            if let Some(frame) = wire::take_frame(&mut self.buf) {
                return Ok(frame);
            }
            let mut chunk = [0u8; 64 * 1024];
            let n = self.stream.read(&mut chunk)?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the kernel closed the socket"));
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }

    /// The next whole message; commands (a 3.1 peer's PING) are skipped.
    pub fn recv(&mut self) -> io::Result<Vec<Vec<u8>>> {
        let mut parts = Vec::new();
        loop {
            let frame = self.frame()?;
            if frame.command {
                continue;
            }
            parts.push(frame.body);
            if !frame.more {
                return Ok(parts);
            }
        }
    }

    pub fn shutdown(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}
