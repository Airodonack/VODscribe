//! The specs of `annotations_filter.rs`: the helpers they share, and the files in `tests/` that hold the rest.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use super::*;

mod checks;
mod format;

/// A file host on this computer: `answer(request)` (its whole head) gives
/// the status line and headers, and the body. Returns its address and the
/// requests' first lines, with their If-None-Match if any.
pub(crate) type Answer = dyn Fn(&str) -> (String, Vec<u8>) + Send + Sync;
pub(crate) fn host(answer: Arc<Answer>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = asked.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { return };
            let mut req = [0u8; 4096];
            let n = conn.read(&mut req).unwrap_or(0);
            let req = String::from_utf8_lossy(&req[..n]).to_string();
            let line = req.lines().next().unwrap_or("").to_string();
            let since = req.lines().find_map(|l| {
                l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("if-none-match")).map(|(_, v)| v.trim())
            });
            seen.lock().unwrap().push(match since {
                Some(etag) => format!("{line} since {etag}"),
                None => line.clone(),
            });
            let (head, body) = answer(&req);
            let body = if line.starts_with("HEAD") { Vec::new() } else { body };
            let _ = write!(conn, "{head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = conn.write_all(&body);
        }
    });
    (format!("http://{addr}"), asked)
}
