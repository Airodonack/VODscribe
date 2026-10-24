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
use std::net::{Ipv4Addr, TcpListener};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Listener, Manager};

use crate::loopback::{self, Response};

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
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).expect("the e2e port");
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
    let (app, next) = (app.clone(), AtomicU64::new(0));
    let answer = move |request: &loopback::Request| {
        if !request.headers("Authorization").any(|h| h == format!("Bearer {token}")) {
            return Response::empty(403);
        }
        let (status, body) = match request.url.as_str() {
            "/run" => {
                let id = next.fetch_add(1, Ordering::SeqCst) + 1;
                run(&app, &waiting, id, &String::from_utf8_lossy(&request.body))
            }
            "/quit" => {
                let app = app.clone();
                return Response::empty(200).then(move || app.exit(0));
            }
            _ => (404, String::new()),
        };
        Response::bytes(status, body.into_bytes()).header("Content-Type", "application/json")
    };
    std::thread::spawn(move || loopback::serve(listener, "e2e", usize::MAX, loopback::MAX_BODY, Arc::new(answer)));
}

/// What the app's player is at (`player::Seen`), for the tests' page dump
/// (tests/e2e/lib/dump.mjs). Async, so it runs off the main thread: a
/// stuck player can't hold the window up while it's asked.
#[tauri::command]
pub(crate) async fn e2e_player_state(app: AppHandle) -> crate::player::Seen {
    app.state::<crate::AppState>().player.seen()
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
