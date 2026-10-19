//! A way in for the end-to-end tests (tests/e2e) where the webview has no
//! WebDriver: macOS's WKWebView. Built only with the `e2e` feature, never in a
//! release (CI checks the installers for it), and started only when the test
//! sets VODSCRIBE_E2E_PORT and VODSCRIBE_E2E_TOKEN.
//!
//! It listens on 127.0.0.1 for what WebDriver's "execute script" does: run a
//! function body in the page, with arguments, and answer with what it returns
//! (awaited). The page's answer comes back as an event, so the app needs no
//! command or permission for it. Every request carries the token.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Listener, Manager};

type Waiting = Arc<Mutex<HashMap<u64, mpsc::Sender<String>>>>;

#[derive(serde::Deserialize)]
struct Run {
    script: String,
    #[serde(default)]
    args: Vec<serde_json::Value>,
    /// How long to wait for the page's answer; it may not have loaded yet.
    #[serde(default = "default_wait")]
    wait_ms: u64,
}

fn default_wait() -> u64 {
    30_000
}

#[derive(serde::Deserialize)]
struct Answer {
    id: u64,
}

pub(crate) fn start(app: &AppHandle) {
    let (Ok(port), Ok(token)) = (std::env::var("VODSCRIBE_E2E_PORT"), std::env::var("VODSCRIBE_E2E_TOKEN")) else {
        return;
    };
    let port: u16 = port.parse().expect("VODSCRIBE_E2E_PORT: a port");
    assert!(token.len() >= 16, "VODSCRIBE_E2E_TOKEN: at least 16 characters");
    let server = tiny_http::Server::http(("127.0.0.1", port)).expect("the e2e port");
    let waiting: Waiting = Arc::default();
    {
        let waiting = waiting.clone();
        app.listen_any("e2e-answer", move |event| {
            let payload = event.payload().to_owned();
            if let Ok(Answer { id }) = serde_json::from_str(&payload)
                && let Some(to) = waiting.lock().unwrap().remove(&id)
            {
                let _ = to.send(payload);
            }
        });
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let mut next = 0u64;
        for mut request in server.incoming_requests() {
            let authorized = request
                .headers()
                .iter()
                .any(|h| h.field.equiv("Authorization") && h.value.as_str() == format!("Bearer {token}"));
            if !authorized {
                let _ = request.respond(tiny_http::Response::empty(403));
                continue;
            }
            let (status, body) = match request.url() {
                "/run" => {
                    next += 1;
                    let mut body = String::new();
                    let _ = request.as_reader().read_to_string(&mut body);
                    run(&app, &waiting, next, &body)
                }
                "/quit" => {
                    let _ = request.respond(tiny_http::Response::empty(200));
                    app.exit(0);
                    return;
                }
                _ => (404, String::new()),
            };
            let json = tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap();
            let _ = request.respond(tiny_http::Response::from_string(body).with_status_code(status).with_header(json));
        }
    });
}

/// Runs `body` (a `Run`) in the main window and waits for its answer:
/// `{"id", "ok", "value"}`, `value` being what it returned or the error.
fn run(app: &AppHandle, waiting: &Waiting, id: u64, body: &str) -> (u16, String) {
    let run: Run = match serde_json::from_str(body) {
        Ok(run) => run,
        Err(e) => return (400, e.to_string()),
    };
    let Some(window) = app.get_webview_window("main") else { return (503, "no window yet".into()) };
    let (to, answer) = mpsc::channel();
    waiting.lock().unwrap().insert(id, to);
    let args = serde_json::to_string(&run.args).unwrap();
    // The body goes in as source, as WebDriver's does; `arguments` are its own.
    let script = format!(
        r#"(async () => {{
  let answer;
  try {{
    const value = await (async function () {{ {body} }}).apply(null, {args});
    answer = {{ id: {id}, ok: true, value: value === undefined ? null : value }};
    JSON.stringify(answer);
  }} catch (e) {{
    answer = {{ id: {id}, ok: false, value: String((e && e.message) || e) }};
  }}
  await window.__TAURI_INTERNALS__.invoke("plugin:event|emit", {{ event: "e2e-answer", payload: answer }});
}})();"#,
        body = run.script,
    );
    if let Err(e) = window.eval(&script) {
        waiting.lock().unwrap().remove(&id);
        return (500, e.to_string());
    }
    match answer.recv_timeout(Duration::from_millis(run.wait_ms)) {
        Ok(answer) => (200, answer),
        Err(_) => {
            waiting.lock().unwrap().remove(&id);
            (504, "the page didn't answer".into())
        }
    }
}
