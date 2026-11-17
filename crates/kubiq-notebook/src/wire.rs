//! The Jupyter wire format, as bytes: the signed multipart message (messaging spec 5.3) and the
//! ZMTP 3.0 framing it travels in. Pure — the host owns the sockets (`kubiq-host/src/notebook/`).
//!
//! Only what a *client* of a kernel needs: the NULL mechanism (a kernel listens on loopback and
//! signs every message instead), a DEALER for shell, control and stdin, a SUB for iopub and a REQ
//! for the heartbeat. Kubiq greets as ZMTP 3.0, so a 3.1 peer (libzmq) falls back to 3.0 rules — a
//! subscription is the message `\x01<topic>`, not a SUBSCRIBE command.

use sha2::{Digest, Sha256};

use crate::json::{self, Json, Style};

/// The protocol version Kubiq speaks.
pub const PROTOCOL_VERSION: &str = "5.3";
/// The delimiter between routing identities and the signed parts.
pub const DELIMITER: &[u8] = b"<IDS|MSG>";

/// HMAC-SHA256 over `parts`, in order.
pub fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    for part in parts {
        inner.update(part);
    }
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner);
    outer.finalize().into()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// One Jupyter message.
#[derive(Debug, Clone, PartialEq)]
pub struct JupyterMessage {
    /// Routing identities before the delimiter (an iopub topic, a ROUTER's peer).
    pub identities: Vec<Vec<u8>>,
    pub header: Json,
    pub parent_header: Json,
    pub metadata: Json,
    pub content: Json,
    pub buffers: Vec<Vec<u8>>,
}

impl JupyterMessage {
    /// A new request. `msg_id`, `session` and `date` (ISO 8601) come from the caller.
    pub fn new(msg_type: &str, content: Json, msg_id: &str, session: &str, date: &str) -> Self {
        JupyterMessage {
            identities: Vec::new(),
            header: Json::sorted(vec![
                ("date".into(), Json::str(date)),
                ("msg_id".into(), Json::str(msg_id)),
                ("msg_type".into(), Json::str(msg_type)),
                ("session".into(), Json::str(session)),
                ("username".into(), Json::str("kubiq")),
                ("version".into(), Json::str(PROTOCOL_VERSION)),
            ]),
            parent_header: Json::Obj(Vec::new()),
            metadata: Json::Obj(Vec::new()),
            content,
            buffers: Vec::new(),
        }
    }

    pub fn msg_type(&self) -> &str {
        self.header.get("msg_type").and_then(Json::as_str).unwrap_or("")
    }

    pub fn msg_id(&self) -> &str {
        self.header.get("msg_id").and_then(Json::as_str).unwrap_or("")
    }

    /// The `msg_id` of the request this answers.
    pub fn parent_id(&self) -> &str {
        self.parent_header.get("msg_id").and_then(Json::as_str).unwrap_or("")
    }

    /// The multipart frames, signed with `key` (an empty key signs nothing, as Jupyter allows).
    pub fn encode(&self, key: &[u8]) -> Vec<Vec<u8>> {
        let compact = Style {
            indent: None,
            trailing_newline: false,
        };
        let parts: Vec<Vec<u8>> = [&self.header, &self.parent_header, &self.metadata, &self.content]
            .iter()
            .map(|p| json::write(p, compact).into_bytes())
            .collect();
        let signature = if key.is_empty() {
            String::new()
        } else {
            let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
            hex(&hmac_sha256(key, &refs))
        };
        let mut frames = self.identities.clone();
        frames.push(DELIMITER.to_vec());
        frames.push(signature.into_bytes());
        frames.extend(parts);
        frames.extend(self.buffers.iter().cloned());
        frames
    }

    /// Read frames, checking the signature.
    pub fn decode(frames: &[Vec<u8>], key: &[u8]) -> Result<JupyterMessage, String> {
        let at = frames
            .iter()
            .position(|f| f == DELIMITER)
            .ok_or("no <IDS|MSG> delimiter")?;
        let rest = &frames[at + 1..];
        if rest.len() < 5 {
            return Err("too few message parts".into());
        }
        if !key.is_empty() {
            let refs: Vec<&[u8]> = rest[1..5].iter().map(Vec::as_slice).collect();
            let want = hex(&hmac_sha256(key, &refs));
            if want.as_bytes() != rest[0].as_slice() {
                return Err("bad signature".into());
            }
        }
        let part = |n: usize| -> Result<Json, String> {
            let text = std::str::from_utf8(&rest[n]).map_err(|_| "a part is not UTF-8")?;
            json::parse(text).map_err(|e| e.to_string())
        };
        Ok(JupyterMessage {
            identities: frames[..at].to_vec(),
            header: part(1)?,
            parent_header: part(2)?,
            metadata: part(3)?,
            content: part(4)?,
            buffers: rest[5..].to_vec(),
        })
    }
}

// ── ZMTP 3.0 ─────────────────────────────────────────────────────────────────────────────────

/// The 64-byte greeting: signature, version 3.0, NULL mechanism, client.
pub fn greeting() -> [u8; 64] {
    let mut g = [0u8; 64];
    g[0] = 0xFF;
    g[8] = 0x01;
    g[9] = 0x7F;
    g[10] = 3;
    g[11] = 0;
    g[12..16].copy_from_slice(b"NULL");
    g
}

/// Check a peer's greeting.
pub fn check_greeting(g: &[u8; 64]) -> Result<(), String> {
    if g[0] != 0xFF || g[9] != 0x7F {
        return Err("not a ZMTP peer".into());
    }
    if g[10] < 3 {
        return Err(format!("ZMTP {} is too old", g[10]));
    }
    if &g[12..16] != b"NULL" {
        return Err("the kernel asks for a security mechanism other than NULL".into());
    }
    Ok(())
}

fn frame_with(flags: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 9);
    if body.len() > 255 {
        out.push(flags | 0x02);
        out.extend_from_slice(&(body.len() as u64).to_be_bytes());
    } else {
        out.push(flags);
        out.push(body.len() as u8);
    }
    out.extend_from_slice(body);
    out
}

/// The READY command naming our socket type and, for a DEALER, its identity.
pub fn ready(socket_type: &str, identity: Option<&[u8]>) -> Vec<u8> {
    let mut body = vec![5u8];
    body.extend_from_slice(b"READY");
    let mut prop = |name: &str, value: &[u8]| {
        body.push(name.len() as u8);
        body.extend_from_slice(name.as_bytes());
        body.extend_from_slice(&(value.len() as u32).to_be_bytes());
        body.extend_from_slice(value);
    };
    prop("Socket-Type", socket_type.as_bytes());
    if let Some(id) = identity {
        prop("Identity", id);
    }
    frame_with(0x04, &body)
}

/// A multipart message's bytes.
pub fn encode_frames(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for (n, part) in parts.iter().enumerate() {
        let more = if n + 1 < parts.len() { 0x01 } else { 0x00 };
        out.extend(frame_with(more, part));
    }
    out
}

/// One frame read off the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub command: bool,
    pub more: bool,
    pub body: Vec<u8>,
}

/// Take one frame off the front of `buf`, or `None` when it is not all there yet.
pub fn take_frame(buf: &mut Vec<u8>) -> Option<Frame> {
    let flags = *buf.first()?;
    let (len, head) = if flags & 0x02 != 0 {
        let bytes: [u8; 8] = buf.get(1..9)?.try_into().ok()?;
        (u64::from_be_bytes(bytes) as usize, 9)
    } else {
        (*buf.get(1)? as usize, 2)
    };
    if buf.len() < head + len {
        return None;
    }
    let body = buf[head..head + len].to_vec();
    buf.drain(..head + len);
    Some(Frame {
        command: flags & 0x04 != 0,
        more: flags & 0x01 != 0,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_rfc4231_case_2() {
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", &[b"what do ya want ", b"for nothing?"])),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn message_round_trip_and_tamper() {
        let mut m = JupyterMessage::new(
            "execute_request",
            Json::sorted(vec![("code".into(), Json::str("1+1"))]),
            "m1",
            "s1",
            "2026-01-01T00:00:00Z",
        );
        m.identities = vec![b"topic".to_vec()];
        let frames = m.encode(b"key");
        let back = JupyterMessage::decode(&frames, b"key").unwrap();
        assert_eq!(back, m);
        assert_eq!(back.msg_type(), "execute_request");
        let mut bad = frames.clone();
        bad[6] = b"{\"code\":\"2+2\"}".to_vec();
        assert!(JupyterMessage::decode(&bad, b"key").is_err());
    }

    #[test]
    fn frames_short_and_long() {
        let parts = vec![vec![1u8; 3], vec![2u8; 300]];
        let mut buf = encode_frames(&parts);
        let a = take_frame(&mut buf).unwrap();
        assert!(a.more && !a.command && a.body == vec![1u8; 3]);
        let b = take_frame(&mut buf).unwrap();
        assert!(!b.more && b.body.len() == 300);
        assert!(buf.is_empty() && take_frame(&mut buf).is_none());
        let mut ready_buf = ready("DEALER", Some(b"id"));
        assert!(take_frame(&mut ready_buf).unwrap().command);
        assert!(check_greeting(&greeting()).is_ok());
    }
}
