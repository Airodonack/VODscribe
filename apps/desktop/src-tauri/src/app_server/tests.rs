//! The app server's specs (app_server.rs): the app's pages served at
//! http://localhost on a port held on both loopback addresses, as a
//! release build serves them (YouTube's and Twitch's players play only in
//! an http page), only to requests that name it, and the app's commands
//! given to that origin alone. Each test starts the app on Tauri's mock
//! runtime with its real configuration and permissions, its pages built
//! in, and serves them with `start`, as the app's start does.

use super::*;

use std::borrow::Cow;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpStream};
use std::time::Duration;

use serde_json::{Value, json};
use tauri::utils::assets::{AssetKey, AssetsIter, CspHash};

use crate::{Rt, WebviewWindow};

/// The app's built pages, as a release build has them in its binary.
struct Built;

const PAGES: &[(&str, &[u8])] =
    &[("/index.html", b"<!doctype html><html><head></head><body></body></html>"), ("/_app/entry.js", b"export {};")];

impl tauri::Assets<Rt> for Built {
    fn get(&self, key: &AssetKey) -> Option<Cow<'_, [u8]>> {
        PAGES.iter().find(|(path, _)| *path == key.as_ref()).map(|(_, bytes)| Cow::Borrowed(*bytes))
    }

    fn iter(&self) -> Box<AssetsIter<'_>> {
        Box::new(PAGES.iter().map(|(path, bytes)| (Cow::Borrowed(*path), Cow::Borrowed(*bytes))))
    }

    fn csp_hashes(&self, _: &AssetKey) -> Box<dyn Iterator<Item = CspHash<'_>> + '_> {
        Box::new(std::iter::empty())
    }
}

/// The app as a release build starts, its pages served (`start`): the app,
/// its main window, and the origin it's served from.
fn served() -> (tauri::App<Rt>, WebviewWindow, String) {
    let mut context = crate::context();
    // As a release build: the pages built in, not a dev server's.
    context.config_mut().build.dev_url = None;
    context.set_assets(Box::new(Built));
    let app = tauri::test::mock_builder().invoke_handler(crate::commands()).build(context).unwrap();
    let origin = start(app.handle()).unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    (app, window, origin)
}

/// The port of `origin` (`http://localhost:<port>`).
fn port(origin: &str) -> u16 {
    origin.rsplit(':').next().unwrap().parse().unwrap()
}

/// `request` sent to `ip` at `origin`'s port, `{host}` in it its host.
fn ask(origin: &str, ip: IpAddr, request: &str) -> TcpStream {
    let mut stream = TcpStream::connect((ip, port(origin))).unwrap();
    let host = origin.strip_prefix("http://").unwrap();
    stream.write_all(request.replace("{host}", host).as_bytes()).unwrap();
    stream
}

/// The whole answer to `stream`.
fn reply(mut stream: TcpStream) -> String {
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut text = String::new();
    stream.read_to_string(&mut text).unwrap();
    text
}

/// `request` to `origin` on 127.0.0.1: its answer.
fn answer_to(origin: &str, request: &str) -> String {
    reply(ask(origin, Ipv4Addr::LOCALHOST.into(), request))
}

/// A command (`embed_takeover`, which every page may call) called from a
/// page at `url` in the main window, as the web view passes it on.
fn call_from(window: &WebviewWindow, url: &str) -> Result<Value, Value> {
    let request = tauri::webview::InvokeRequest {
        cmd: "embed_takeover".into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: url.parse().unwrap(),
        body: tauri::ipc::InvokeBody::Json(json!({})),
        headers: Default::default(),
        invoke_key: tauri::test::INVOKE_KEY.into(),
    };
    tauri::test::get_ipc_response(window, request).map(|body| body.deserialize::<Value>().unwrap())
}

/// The app is served from http://localhost at a port of its own, held on
/// 127.0.0.1 and on ::1 where there's IPv6 (app_server.rs: "YouTube's
/// player refuses to play without an http(s) referrer (error 153), and
/// Twitch's only plays for an http(s) page named in its `parent`
/// parameter"; "The server binds a random port, on both 127.0.0.1 and
/// [::1], before the window opens, so no other program can be listening
/// there first"; 0.1.1: "Fixes YouTube and Twitch playback in the
/// installed app.").
#[test]
fn the_app_is_served_from_localhost_at_a_port_held_on_both_loopbacks() {
    let (_app, _window, origin) = served();
    assert!(origin.starts_with("http://localhost:") && port(&origin) > 0, "{origin}");
    let get = "GET /_app/entry.js HTTP/1.1\r\nHost: {host}\r\n\r\n";
    let v4 = reply(ask(&origin, Ipv4Addr::LOCALHOST.into(), get));
    assert!(v4.starts_with("HTTP/1.1 200 OK") && v4.ends_with("export {};"), "{v4}");
    if TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).is_ok() {
        let v6 = reply(ask(&origin, Ipv6Addr::LOCALHOST.into(), get));
        assert!(v6.starts_with("HTTP/1.1 200 OK") && v6.ends_with("export {};"), "{v6}");
    }
}

/// Each of the app's built pages is answered with its type, never kept by
/// the web view without asking again, and never read as another type; the
/// page with the app's security policy; a GET's query names no other file
/// (app_server.rs: "answers only GET and HEAD for the app's built assets";
/// `answer`).
#[test]
fn the_app_server_answers_the_apps_built_pages_as_they_are() {
    let (_app, _window, origin) = served();
    let script = answer_to(&origin, "GET /_app/entry.js?v=1 HTTP/1.1\r\nHost: {host}\r\n\r\n");
    assert!(script.starts_with("HTTP/1.1 200 OK\r\n") && script.ends_with("\r\n\r\nexport {};"), "{script}");
    assert!(script.contains("Content-Type: text/javascript"), "{script}");
    assert!(script.contains("Cache-Control: no-cache\r\n"), "{script}");
    assert!(script.contains("X-Content-Type-Options: nosniff\r\n"), "{script}");
    let head = answer_to(&origin, "HEAD /_app/entry.js HTTP/1.1\r\nHost: {host}\r\n\r\n");
    assert!(head.starts_with("HTTP/1.1 200 OK") && head.ends_with("\r\n\r\n"), "{head}");
    let page = answer_to(&origin, "GET /index.html HTTP/1.1\r\nHost: {host}\r\n\r\n");
    assert!(page.starts_with("HTTP/1.1 200 OK") && page.contains("Content-Type: text/html"), "{page}");
    let policy = page.lines().find_map(|l| l.strip_prefix("Content-Security-Policy: "));
    assert!(policy.is_some_and(|p| p.contains("default-src")), "the app's security policy: {page}");
}

/// A request naming another host (a web page reaching the server through
/// DNS rebinding), or that isn't a read, gets nothing (app_server.rs:
/// "refuses requests that don't name its own host, so a web page can't
/// reach it through DNS rebinding"; "answers only GET and HEAD").
#[test]
fn the_app_server_refuses_another_host_and_anything_but_a_read() {
    let (_app, _window, origin) = served();
    let port = port(&origin);
    let rebound = answer_to(&origin, &format!("GET /_app/entry.js HTTP/1.1\r\nHost: evil.example:{port}\r\n\r\n"));
    assert!(rebound.starts_with("HTTP/1.1 403") && !rebound.contains("export"), "{rebound}");
    let by_address = answer_to(&origin, &format!("GET /_app/entry.js HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"));
    assert!(by_address.starts_with("HTTP/1.1 403"), "{by_address}");
    let post = answer_to(&origin, "POST /_app/entry.js HTTP/1.1\r\nHost: {host}\r\n\r\n");
    assert!(post.starts_with("HTTP/1.1 403"), "{post}");
}

/// A start's burst of script preloads is answered whole while the web view
/// keeps other connections open (app_server.rs `MAX_ACTIVE`: "far more
/// than a web view opens to one host (about six)"; loopback.rs `serve`:
/// under tiny_http "the page then sometimes stayed empty for two minutes at
/// start … one preload never got its answer").
#[test]
fn answers_a_burst_of_connections_while_others_stay_open() {
    let (_app, _window, origin) = served();
    for _ in 0..20 {
        let open: Vec<_> = (0..8).map(|_| TcpStream::connect(("127.0.0.1", port(&origin))).unwrap()).collect();
        let get = "GET /_app/entry.js HTTP/1.1\r\nHost: {host}\r\n\r\n";
        let burst: Vec<_> = (0..8).map(|_| ask(&origin, Ipv4Addr::LOCALHOST.into(), get)).collect();
        for stream in burst {
            assert!(reply(stream).starts_with("HTTP/1.1 200 OK"));
        }
        drop(open);
    }
}

/// The page served gets the app's commands, and a page from any other
/// origin, another port on this computer included, gets none (app_server.rs:
/// "The pages get the app's commands through a capability limited to this
/// exact origin"; `allow_commands`).
#[test]
fn the_page_served_gets_the_apps_commands_and_no_other_origin_does() {
    let (_app, window, origin) = served();
    assert_eq!(call_from(&window, &format!("{origin}/")), Ok(json!(cfg!(feature = "embed-takeover"))));
    let next = format!("http://localhost:{}/", port(&origin).wrapping_add(1));
    assert!(call_from(&window, &next).is_err(), "a page from {next} called the app's commands");
    assert!(call_from(&window, "http://evil.example/").is_err(), "a web page called the app's commands");
}
