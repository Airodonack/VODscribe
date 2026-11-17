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

use std::net::TcpListener;
use std::sync::Arc;

use anyhow::Result;
use tauri::{AppHandle, Manager, Runtime};

use crate::loopback::{self, Request, Response};

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
        std::thread::Builder::new().name("app-server".into()).spawn(move || serve(listener, host, find))?;
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

/// Requests answered at once: far more than a web view opens to one host
/// (about six), and a bound on the threads another program can make the
/// app start by connecting.
const MAX_ACTIVE: usize = 64;

fn serve(listener: TcpListener, host: String, find: Arc<Find>) {
    let answer = Arc::new(move |request: &Request| answer(request, &host, &*find));
    loopback::serve(listener, "app-server", MAX_ACTIVE, 0, answer);
}

fn answer(request: &Request, host: &str, find: &Find) -> Response {
    // The tests' builds say what was asked and when (a start that stalls
    // with the page empty: was its script asked for, and answered?).
    #[cfg(feature = "e2e")]
    let (asked, url) = (std::time::Instant::now(), request.url.clone());
    let allowed = loopback::check(request, host).is_ok();
    let path = request.url.split(['?', '#']).next().unwrap_or("/");
    let response = match allowed.then(|| find(path)).flatten() {
        Some(page) => {
            let mut response = Response::bytes(200, page.bytes)
                .header("Content-Type", page.mime)
                .header("Cache-Control", "no-cache")
                .header("X-Content-Type-Options", "nosniff");
            if let Some(csp) = page.csp {
                response = response.header("Content-Security-Policy", csp);
            }
            response
        }
        None => Response::empty(if allowed { 404 } else { 403 }),
    };
    #[cfg(feature = "e2e")]
    let response =
        response.then(move || eprintln!("app-server {}: {url} in {} ms", chrono_ms(), asked.elapsed().as_millis()));
    response
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
mod tests;
