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

use anyhow::Result;
use tauri::{AppHandle, Manager, Runtime};
use tiny_http::{Header, Response, Server};

use crate::loopback;

/// The origin the app is served from, such as `http://localhost:41723`.
pub(crate) fn start<R: Runtime>(app: &AppHandle<R>) -> Result<String> {
    let (listeners, port) = loopback::bind_both()?;
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

fn serve<R: Runtime>(server: &Server, host: &str, assets: &tauri::AssetResolver<R>) {
    for request in server.incoming_requests() {
        let allowed = loopback::check(&request, host).is_ok();
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
