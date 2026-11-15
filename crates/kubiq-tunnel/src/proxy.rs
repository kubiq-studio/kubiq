//! The proxy kind: a standard HTTP forward proxy on the local port, so a browser pointed at it
//! browses as if it sat on the far machine.
//!
//! Each connection's first request head names where it goes. `CONNECT host:port` opens that on
//! the far side, answers `200 Connection Established` and turns the connection into a raw pipe —
//! HTTPS stays end-to-end TLS between the browser and the far server. A request with an absolute
//! `http://` URI opens its authority and goes out in origin form, its `Proxy-*`, `Connection` and
//! `Keep-Alive` headers dropped, its `Host` the URI's, and `Connection: close` added: one request
//! per connection, so a browser's next request to another host never lands on this upstream.
//! Anything else — an origin-form request a page could aim at the port directly — is a `400`.
//! What the far side's allow list refuses is a `502` naming the reason.

use std::io;

use httparse::Status;

/// The longest request head accepted.
pub(crate) const HEAD_MAX: usize = 64 * 1024;
const MAX_HEADERS: usize = 128;

/// One proxy request, parsed from its head.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ProxyRequest {
    /// The destination, unbracketed.
    pub(crate) host: String,
    pub(crate) port: u16,
    /// A `CONNECT`: answer `200` and pipe raw; nothing of the head goes upstream.
    pub(crate) connect: bool,
    /// For a forwarded request: its head rewritten to origin form, what goes upstream first.
    pub(crate) head: Vec<u8>,
}

/// Parse the request head at the start of `buf`: `None` until it is complete, then the request
/// and how many bytes of `buf` it took. An error is a `400`.
pub(crate) fn parse(buf: &[u8]) -> io::Result<Option<(ProxyRequest, usize)>> {
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut req = httparse::Request::new(&mut headers);
    let n = match req.parse(buf) {
        Ok(Status::Partial) if buf.len() > HEAD_MAX => {
            return Err(bad("the request head is too large"));
        }
        Ok(Status::Partial) => return Ok(None),
        Ok(Status::Complete(n)) => n,
        Err(e) => return Err(bad(&format!("not an HTTP request: {e}"))),
    };
    let method = req.method.unwrap_or("");
    let target = req.path.unwrap_or("");
    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = authority(target, None)?;
        return Ok(Some((
            ProxyRequest {
                host,
                port,
                connect: true,
                head: Vec::new(),
            },
            n,
        )));
    }
    let rest = target
        .get(..7)
        .filter(|s| s.eq_ignore_ascii_case("http://"))
        .map(|_| &target[7..])
        .ok_or_else(|| {
            bad(&format!(
                "{target} is not an absolute http:// URI: this port is a proxy"
            ))
        })?;
    let (auth, path) = match rest.find(['/', '?', '#']) {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    let path = match path.as_bytes().first() {
        None | Some(b'#') => "/".to_string(),
        Some(b'?') => format!("/{path}"),
        _ => path.to_string(),
    };
    let path = path.split('#').next().unwrap_or("/");
    // Credentials in the URI are not forwarded.
    let auth = auth.rsplit_once('@').map_or(auth, |(_, a)| a);
    let (host, port) = authority(auth, Some(80))?;

    let mut head = format!(
        "{method} {path} HTTP/1.{}\r\n",
        req.version.unwrap_or(1).min(1)
    )
    .into_bytes();
    push(&mut head, "Host", auth.as_bytes());
    for h in req.headers.iter() {
        let name = h.name.to_ascii_lowercase();
        if name == "host"
            || name == "connection"
            || name == "keep-alive"
            || name.starts_with("proxy-")
        {
            continue;
        }
        push(&mut head, h.name, h.value);
    }
    push(&mut head, "Connection", b"close");
    head.extend_from_slice(b"\r\n");
    Ok(Some((
        ProxyRequest {
            host,
            port,
            connect: false,
            head,
        },
        n,
    )))
}

/// `host:port` or `[v6]:port`, the port `default` when absent; the host unbracketed.
fn authority(text: &str, default: Option<u16>) -> io::Result<(String, u16)> {
    let (host, port) = match text.strip_prefix('[') {
        Some(v6) => {
            let (addr, after) = v6
                .split_once(']')
                .ok_or_else(|| bad(&format!("{text}: an unclosed [")))?;
            (addr, after.strip_prefix(':'))
        }
        None => match text.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (text, None),
        },
    };
    let port = match port {
        Some(p) => p.parse::<u16>().ok().filter(|&p| p != 0),
        None => default,
    }
    .ok_or_else(|| bad(&format!("{text} does not name a port")))?;
    if host.is_empty() || host.contains(['/', ' ', '@']) {
        return Err(bad(&format!("{text} does not name a host")));
    }
    Ok((host.to_ascii_lowercase(), port))
}

fn push(out: &mut Vec<u8>, name: &str, value: &[u8]) {
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(b": ");
    out.extend_from_slice(value);
    out.extend_from_slice(b"\r\n");
}

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, what.to_string())
}

/// A proxy's own answer: a status line, a one-line plain body, and a close.
pub(crate) fn answer(status: &str, message: &str) -> Vec<u8> {
    let body = format!("kubiq tunnel: {message}\n");
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(head: &str) -> ProxyRequest {
        let (req, n) = parse(head.as_bytes()).unwrap().unwrap();
        assert_eq!(n, head.len());
        req
    }

    #[test]
    fn connect_names_its_authority_and_sends_nothing_upstream() {
        let r = one("CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n");
        assert_eq!(
            (r.host.as_str(), r.port, r.connect),
            ("example.com", 443, true)
        );
        assert!(r.head.is_empty());
        let r = one("CONNECT [::1]:8443 HTTP/1.1\r\n\r\n");
        assert_eq!((r.host.as_str(), r.port), ("::1", 8443));
        for bad in [
            "CONNECT example.com HTTP/1.1\r\n\r\n",
            "CONNECT :443 HTTP/1.1\r\n\r\n",
            "CONNECT h:0 HTTP/1.1\r\n\r\n",
            "CONNECT [::1 HTTP/1.1\r\n\r\n",
        ] {
            assert!(parse(bad.as_bytes()).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_absolute_uri_goes_out_in_origin_form_without_proxy_headers() {
        let r = one(
            "GET http://Intranet:8080/a/b?q=1#frag HTTP/1.1\r\nHost: wrong\r\n\
             Proxy-Connection: keep-alive\r\nProxy-Authorization: Basic x\r\n\
             Connection: keep-alive\r\nKeep-Alive: 5\r\nAccept: */*\r\n\r\n",
        );
        assert_eq!(
            (r.host.as_str(), r.port, r.connect),
            ("intranet", 8080, false)
        );
        assert_eq!(
            String::from_utf8_lossy(&r.head),
            "GET /a/b?q=1 HTTP/1.1\r\nHost: Intranet:8080\r\nAccept: */*\r\nConnection: close\r\n\r\n"
        );
        let r = one("POST http://u:p@h?x HTTP/1.0\r\nContent-Length: 2\r\n\r\n");
        assert_eq!((r.host.as_str(), r.port), ("h", 80));
        assert_eq!(
            String::from_utf8_lossy(&r.head),
            "POST /?x HTTP/1.0\r\nHost: h\r\nContent-Length: 2\r\nConnection: close\r\n\r\n"
        );
        let r = one("GET http://[fe80::1]/ HTTP/1.1\r\n\r\n");
        assert_eq!((r.host.as_str(), r.port), ("fe80::1", 80));
    }

    #[test]
    fn a_partial_head_waits_and_the_body_is_left_in_the_buffer() {
        assert!(parse(b"GET http://h/ HTTP/1.1\r\nAcc").unwrap().is_none());
        let buf = b"POST http://h/ HTTP/1.1\r\nContent-Length: 3\r\n\r\nabc";
        let (_, n) = parse(buf).unwrap().unwrap();
        assert_eq!(&buf[n..], b"abc");
    }

    #[test]
    fn an_origin_form_or_https_request_is_refused() {
        for bad in [
            "GET / HTTP/1.1\r\nHost: localhost:3128\r\n\r\n",
            "GET https://h/ HTTP/1.1\r\n\r\n",
            "GET http:///x HTTP/1.1\r\n\r\n",
            "\x16\x03\x01 tls\r\n\r\n",
        ] {
            let e = parse(bad.as_bytes()).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::InvalidInput, "{bad}");
        }
        assert!(parse(&vec![b'a'; HEAD_MAX + 1]).is_err());
    }
}
