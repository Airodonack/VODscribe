//! Serves the app's own pages at http://localhost:<port> in release builds.
//!
//! Tauri normally serves them from tauri://localhost (http://tauri.localhost on
//! Windows), but YouTube's player refuses to play without an http(s) referrer
//! (error 153), and Twitch's only plays for an http(s) page named in its
//! `parent` parameter. `tauri dev` already serves from localhost, so this makes
//! release builds behave the same.
//!
//! The server binds a random port, on both 127.0.0.1 and [::1], before the
//! window opens, so no other program can be listening there first; answers only GET and HEAD for the
//! app's built assets; and refuses requests that don't name its own host, so a
//! web page can't reach it through DNS rebinding. The pages get the app's
//! commands through a capability limited to this exact origin.

// Debug builds compile this only for its tests; `tauri dev` doesn't start it.
#![cfg_attr(dev, allow(dead_code))]

use std::io::ErrorKind;
use std::net::{Ipv4Addr, Ipv6Addr, TcpListener};

use anyhow::{Result, bail};
use tauri::{AppHandle, Manager, Runtime};
use tiny_http::{Header, Method, Request, Response, Server};

/// The origin the app is served from, such as `http://localhost:41723`.
pub(crate) fn start<R: Runtime>(app: &AppHandle<R>) -> Result<String> {
    let (listeners, port) = bind_loopback()?;
    let host = format!("localhost:{port}");
    for listener in listeners {
        let server = Server::from_listener(listener, None).map_err(|e| anyhow::anyhow!("start the app server: {e}"))?;
        let (assets, host) = (app.asset_resolver(), host.clone());
        std::thread::Builder::new().name("app-server".into()).spawn(move || serve(&server, &host, &assets))?;
    }
    let origin = format!("http://localhost:{port}");
    allow_commands(app, &origin)?;
    Ok(origin)
}

/// One free port on both 127.0.0.1 and [::1]. Webviews resolve `localhost` to
/// either (often ::1 first), so the app holds both: otherwise another account
/// on the computer could listen on the other one and get the app's commands.
fn bind_loopback() -> Result<(Vec<TcpListener>, u16)> {
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

fn serve<R: Runtime>(server: &Server, host: &str, assets: &tauri::AssetResolver<R>) {
    for request in server.incoming_requests() {
        let allowed = accepts(&request, host);
        let path = request.url().split(['?', '#']).next().unwrap_or("/").to_string();
        let response = match allowed.then(|| assets.get(path)).flatten() {
            Some(asset) => {
                let mut response = Response::from_data(asset.bytes)
                    .with_header(header("Content-Type", &asset.mime_type))
                    .with_header(header("Cache-Control", "no-cache"))
                    .with_header(header("X-Content-Type-Options", "nosniff"));
                if let Some(csp) = asset.csp_header {
                    response.add_header(header("Content-Security-Policy", &csp));
                }
                response
            }
            None => Response::from_string("").with_status_code(if allowed { 404 } else { 403 }),
        };
        let _ = request.respond(response);
    }
}

/// Whether to answer `request`: a GET or HEAD naming exactly this server's
/// host, so a page reached through DNS rebinding (another Host) gets nothing.
fn accepts(request: &Request, host: &str) -> bool {
    matches!(request.method(), Method::Get | Method::Head)
        && request.headers().iter().any(|h| h.field.equiv("Host") && h.value.as_str() == host)
}

fn header(name: &'static str, value: &str) -> Header {
    Header::from_bytes(name, value).expect("valid header")
}

/// Gives pages from `origin` the main window's permissions, as listed in
/// capabilities/default.json (which otherwise covers only the app's own scheme).
fn allow_commands<R: Runtime>(app: &AppHandle<R>, origin: &str) -> Result<()> {
    let mut capability: serde_json::Value = serde_json::from_str(include_str!("../capabilities/default.json"))?;
    capability["identifier"] = "app-server".into();
    capability["description"] = "The main window, served by the app server".into();
    capability["remote"] = serde_json::json!({ "urls": [format!("{origin}/*")] });
    app.add_capability(capability.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_http::TestRequest;

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
        assert!(accepts(&request(Method::Get, Some(host)), host));
        assert!(accepts(&request(Method::Head, Some(host)), host));
        assert!(!accepts(&request(Method::Post, Some(host)), host));
        assert!(!accepts(&request(Method::Get, Some("evil.example:41723")), host), "DNS rebinding");
        assert!(!accepts(&request(Method::Get, Some("127.0.0.1:41723")), host), "another name for it");
        assert!(!accepts(&request(Method::Get, Some("localhost:41724")), host));
        assert!(!accepts(&request(Method::Get, None), host));
    }

    #[test]
    fn holds_one_port_on_both_loopbacks() {
        let (listeners, port) = bind_loopback().unwrap();
        let addrs: Vec<_> = listeners.iter().map(|l| l.local_addr().unwrap()).collect();
        assert!(addrs.iter().all(|a| a.port() == port && a.ip().is_loopback()));
        assert!(addrs.iter().any(|a| a.is_ipv4()));
        // Where there's IPv6, ::1 is held too, so nothing else can answer `localhost` there.
        if TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).is_ok() {
            assert!(addrs.iter().any(|a| a.is_ipv6()), "{addrs:?}");
        }
    }
}
