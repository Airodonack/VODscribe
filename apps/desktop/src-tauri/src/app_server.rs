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

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tauri::{AppHandle, Manager, Runtime};

use crate::loopback;

/// The origin the app is served from, such as `http://localhost:41723`.
pub(crate) fn start<R: Runtime>(app: &AppHandle<R>) -> Result<String> {
    let (listeners, port) = loopback::bind_both()?;
    let host = format!("localhost:{port}");
    let assets = app.asset_resolver();
    let find: Arc<Find> = Arc::new(move |path| {
        assets.get(path.to_string()).map(|a| Page { bytes: a.bytes, mime: a.mime_type, csp: a.csp_header })
    });
    for listener in listeners {
        let (find, host) = (find.clone(), host.clone());
        std::thread::Builder::new().name("app-server".into()).spawn(move || serve(listener, &host, find))?;
    }
    let origin = format!("http://localhost:{port}");
    allow_commands(app, &origin)?;
    Ok(origin)
}

/// A file the server answers with.
struct Page {
    bytes: Vec<u8>,
    mime: String,
    csp: Option<String>,
}

/// Finds the page at a path, such as `/_app/immutable/entry/app.js`.
type Find = dyn Fn(&str) -> Option<Page> + Send + Sync;

/// Answers each connection on a thread of its own, one request per connection.
///
/// This was tiny_http, and the page then sometimes stayed empty for two
/// minutes at start (WebKitGTK and macOS's WebKit, mostly in the tests): its
/// thread pool can count a thread as free that it has already woken for an
/// earlier connection, and then leaves the new connection waiting until a
/// thread comes back, which a kept-alive connection only does when the web
/// view drops it, about two minutes later. The start's burst of script
/// preloads hit that, one preload never got its answer, and the page waited
/// on it. (tiny_http also drops a `Connection: close` set on a response.)
fn serve(listener: TcpListener, host: &str, find: Arc<Find>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let (host, find) = (host.to_string(), find.clone());
        let _ = std::thread::Builder::new().name("app-server".into()).spawn(move || {
            let _ = answer(stream, &host, &*find);
        });
    }
}

/// Reads one request from `stream`, answers it and closes the connection.
fn answer(mut stream: TcpStream, host: &str, find: &Find) -> std::io::Result<()> {
    // A connection that never sends its request doesn't hold a thread for long.
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    // The tests' builds say what was asked and when (a start that stalls
    // with the page empty: was its script asked for, and answered?).
    #[cfg(feature = "e2e")]
    let asked = std::time::Instant::now();
    let mut reader = BufReader::new(&stream).take(64 * 1024);
    let mut first = String::new();
    reader.read_line(&mut first)?;
    let mut words = first.split_whitespace();
    let (method, url) = (words.next().unwrap_or(""), words.next().unwrap_or("/"));
    let (mut line, mut hosts) = (String::new(), Vec::new());
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 || line.trim_end().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("host")
        {
            hosts.push(value.trim().to_string());
        }
    }
    let allowed = loopback::allowed(method, hosts.iter().map(String::as_str), host).is_ok();
    let path = url.split(['?', '#']).next().unwrap_or("/");
    let (status, page) = match allowed.then(|| find(path)).flatten() {
        Some(page) => ("200 OK", Some(page)),
        None if allowed => ("404 Not Found", None),
        None => ("403 Forbidden", None),
    };
    let mut head = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
    let body = match &page {
        Some(page) => {
            head += &format!(
                "Content-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nX-Content-Type-Options: nosniff\r\n",
                page.mime,
                page.bytes.len()
            );
            if let Some(csp) = &page.csp {
                head += &format!("Content-Security-Policy: {csp}\r\n");
            }
            &page.bytes[..]
        }
        None => {
            head += "Content-Length: 0\r\n";
            &[][..]
        }
    };
    head += "\r\n";
    stream.write_all(head.as_bytes())?;
    if method != "HEAD" {
        stream.write_all(body)?;
    }
    stream.flush()?;
    #[cfg(feature = "e2e")]
    eprintln!("app-server {}: {url} in {} ms", chrono_ms(), asked.elapsed().as_millis());
    Ok(())
}

/// The time of day to the millisecond (UTC), to line the log up with the tests'.
#[cfg(feature = "e2e")]
fn chrono_ms() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let s = ms / 1000 % 86_400;
    format!("{:02}:{:02}:{:02}.{:03}", s / 3600, s / 60 % 60, s % 60, ms % 1000)
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

    /// A server on 127.0.0.1 with one page, `/a.js`; its port.
    fn server() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let find: Arc<Find> = Arc::new(|path| {
            (path == "/a.js").then(|| Page { bytes: b"x".to_vec(), mime: "text/javascript".into(), csp: None })
        });
        let host = format!("localhost:{port}");
        std::thread::spawn(move || serve(listener, &host, find));
        port
    }

    fn ask(port: u16, request: &str) -> TcpStream {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(request.replace("{port}", &port.to_string()).as_bytes()).unwrap();
        stream
    }

    fn reply(mut stream: TcpStream) -> String {
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        text
    }

    #[test]
    fn answers_a_burst_of_connections_while_others_stay_open() {
        // A start's script preloads: many connections at once, the web view
        // keeping each open. Under tiny_http's pool one of them sometimes went
        // unanswered (on one or two CPUs, 29 rounds in 30).
        let port = server();
        for _ in 0..20 {
            let open: Vec<_> = (0..8).map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap()).collect();
            let burst: Vec<_> =
                (0..8).map(|_| ask(port, "GET /a.js HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n")).collect();
            for stream in burst {
                assert!(reply(stream).starts_with("HTTP/1.1 200 OK"));
            }
            drop(open);
        }
    }

    #[test]
    fn answers_once_and_closes() {
        let port = server();
        let text = reply(ask(port, "GET /a.js?v=1 HTTP/1.1\r\nhost: localhost:{port}\r\n\r\n"));
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
        assert!(text.contains("Connection: close\r\n") && text.contains("Content-Length: 1\r\n"), "{text}");
        assert!(text.ends_with("\r\n\r\nx"), "{text}");
        let head = reply(ask(port, "HEAD /a.js HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n"));
        assert!(head.starts_with("HTTP/1.1 200 OK") && head.ends_with("\r\n\r\n"), "{head}");
        let missing = reply(ask(port, "GET /b.js HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n"));
        assert!(missing.starts_with("HTTP/1.1 404"), "{missing}");
        let rebound = reply(ask(port, "GET /a.js HTTP/1.1\r\nHost: evil.example:{port}\r\n\r\n"));
        assert!(rebound.starts_with("HTTP/1.1 403"), "{rebound}");
        let post = reply(ask(port, "POST /a.js HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n"));
        assert!(post.starts_with("HTTP/1.1 403"), "{post}");
    }
}
