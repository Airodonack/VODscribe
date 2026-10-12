//! What the app's two local HTTP servers share: the app server
//! (app_server.rs, the app's pages) and the media server (media.rs, local
//! video files). Both answer only on this computer's loopback addresses.

use std::io::ErrorKind;
use std::net::{Ipv4Addr, Ipv6Addr, TcpListener};

use anyhow::{Result, bail};
use tiny_http::{Method, Request};

/// One free port on both 127.0.0.1 and [::1]. Webviews resolve `localhost` to
/// either (often ::1 first), so the app holds both: otherwise another account
/// on the computer could listen on the other one and get the app's commands.
// Only release builds serve the app over HTTP (app_server.rs).
#[cfg_attr(dev, allow(dead_code))]
pub(crate) fn bind_both() -> Result<(Vec<TcpListener>, u16)> {
    for _ in 0..20 {
        let v4 = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = v4.local_addr()?.port();
        match TcpListener::bind((Ipv6Addr::LOCALHOST, port)) {
            Ok(v6) => return Ok((vec![v4, v6], port)),
            // Taken on ::1, perhaps on purpose: try another port.
            Err(e) if e.kind() == ErrorKind::AddrInUse => continue,
            // No IPv6 here, so `localhost` can only mean 127.0.0.1.
            Err(_) => return Ok((vec![v4], port)),
        }
    }
    bail!("no free port for the app server")
}

/// Whether to answer `request`, or the status to refuse it with: it must be a
/// GET or HEAD (else 405), naming exactly this server's `host` (else 403), so
/// a page that reaches the server through DNS rebinding, with its own name in
/// the Host header, gets nothing.
pub(crate) fn check(request: &Request, host: &str) -> Result<(), u16> {
    if !matches!(request.method(), Method::Get | Method::Head) {
        return Err(405);
    }
    if !request.headers().iter().any(|h| h.field.equiv("Host") && h.value.as_str() == host) {
        return Err(403);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_http::{Header, TestRequest};

    fn request(method: Method, host: Option<&str>) -> Request {
        let mut r = TestRequest::new().with_method(method).with_path("/");
        if let Some(host) = host {
            r = r.with_header(Header::from_bytes("Host", host).unwrap());
        }
        r.into()
    }

    #[test]
    fn answers_only_its_own_host_and_reads() {
        let host = "localhost:41723";
        assert_eq!(check(&request(Method::Get, Some(host)), host), Ok(()));
        assert_eq!(check(&request(Method::Head, Some(host)), host), Ok(()));
        assert_eq!(check(&request(Method::Post, Some(host)), host), Err(405));
        assert_eq!(check(&request(Method::Get, Some("evil.example:41723")), host), Err(403), "DNS rebinding");
        assert_eq!(check(&request(Method::Get, Some("127.0.0.1:41723")), host), Err(403), "another name for it");
        assert_eq!(check(&request(Method::Get, Some("localhost:41724")), host), Err(403));
        assert_eq!(check(&request(Method::Get, None), host), Err(403));
    }

    #[test]
    fn holds_one_port_on_both_loopbacks() {
        let (listeners, port) = bind_both().unwrap();
        let addrs: Vec<_> = listeners.iter().map(|l| l.local_addr().unwrap()).collect();
        assert!(addrs.iter().all(|a| a.port() == port && a.ip().is_loopback()));
        assert!(addrs.iter().any(|a| a.is_ipv4()));
        // Where there's IPv6, ::1 is held too, so nothing else can answer `localhost` there.
        if TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).is_ok() {
            assert!(addrs.iter().any(|a| a.is_ipv6()), "{addrs:?}");
        }
    }
}
