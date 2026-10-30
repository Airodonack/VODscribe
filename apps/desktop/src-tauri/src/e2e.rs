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
            // The answer's text, as the page wrote it (`run`).
            let Ok(payload) = serde_json::from_str::<String>(event.payload()) else { return };
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
                return Response::empty(200).then(move || {
                    crate::trace_exit("quit asked for (test hook)");
                    app.exit(0)
                });
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

/// Makes the main window's page `width` by `height` (logical pixels), as a
/// small screen or a person's resize would (tests/e2e/scenarios/legend.mjs).
/// The window manager may give it a little more or less; the test reads what
/// it got from the page.
#[tauri::command]
pub(crate) fn e2e_set_inner_size(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or("no window")?;
    window.set_size(tauri::LogicalSize::new(width, height)).map_err(|e| e.to_string())
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
    answer = JSON.stringify({{ id: {id}, ok: true, value: value === undefined ? null : value }});
  }} catch (e) {{
    answer = JSON.stringify({{ id: {id}, ok: false, value: String((e && e.message) || e) }});
  }}
  // Sent as text: an object would be read into the app's JSON values, whose
  // keys come out sorted by name, and a test comparing an object's text
  // would see its keys in another order than WebDriver gives them.
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

/// A Twitch chat the tests left in the data folder (`e2e-chat.json`), fetched
/// in place of Twitch's through the app's own fetch, so a chat's paging,
/// saving and failing can be tested without the network: `{ "messages": [{
/// "time", "author", "text" }], "fail_after": pages or null }`, served 100 to a
/// page from an offset as Twitch's GraphQL API serves a VOD's. From the
/// `fail_after`th comments page on, each is refused with "service error", as
/// Twitch refuses one now and then. None without the file.
pub(crate) fn chat(
    root: &std::path::Path,
    info: &vodscribe_core::fetch::VodInfo,
    store: &mut dyn vodscribe_core::chat::ChatStore,
    cancel: &std::sync::atomic::AtomicBool,
    on_progress: &mut dyn FnMut(f64),
) -> Option<anyhow::Result<bool>> {
    #[derive(serde::Deserialize)]
    struct Message {
        time: f64,
        author: String,
        text: String,
        /// When it was sent, as Twitch's `createdAt` says it.
        #[serde(default)]
        created_at: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct Fixture {
        messages: Vec<Message>,
        fail_after: Option<usize>,
    }
    let text = std::fs::read_to_string(root.join("e2e-chat.json")).ok()?;
    let fixture: Fixture = match serde_json::from_str(&text) {
        Ok(fixture) => fixture,
        Err(e) => return Some(Err(e.into())),
    };
    let asked = AtomicU64::new(0);
    let ask = |body: &serde_json::Value, _: &std::sync::atomic::AtomicBool| {
        // The cheermotes' list: the built-in ones do.
        let Some(offset) = body.pointer("/variables/offset").and_then(serde_json::Value::as_f64) else {
            return Ok(serde_json::json!({ "data": {} }));
        };
        let n = asked.fetch_add(1, Ordering::SeqCst) as usize;
        if fixture.fail_after.is_some_and(|after| n >= after) {
            return Ok(serde_json::json!({ "errors": [{ "message": "service error" }], "data": { "video": null } }));
        }
        let from: Vec<_> = fixture.messages.iter().enumerate().filter(|(_, m)| m.time.floor() >= offset).collect();
        let edges: Vec<_> = from
            .iter()
            .take(100)
            .map(|(id, m)| {
                serde_json::json!({ "node": { "id": id.to_string(), "contentOffsetSeconds": m.time, "createdAt": m.created_at,
                    "commenter": { "displayName": m.author },
                    "message": { "userColor": null, "fragments": [{ "text": m.text, "emote": null }] } } })
            })
            .collect();
        // A page at a time, as slowly as Twitch's, so the test sees chat arrive.
        std::thread::sleep(Duration::from_millis(50));
        Ok(serde_json::json!({ "data": { "video": { "comments": {
            "edges": edges, "pageInfo": { "hasNextPage": from.len() > 100 } } } } }))
    };
    Some(vodscribe_core::chat::fetch_twitch_from(info, store, cancel, on_progress, &ask))
}
