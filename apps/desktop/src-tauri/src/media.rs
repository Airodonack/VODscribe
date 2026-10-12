//! Serves local VODs' files to the player's <video>, over HTTP on 127.0.0.1.
//!
//! A custom URL scheme would be simpler, but WebKitGTK won't load one into the
//! dev server's http page, and a plain HTTP server works the same everywhere.
//! It only listens on the loopback interface, every URL carries a random token
//! made at launch, requests must name the server's own host (so a web page
//! can't reach it through DNS rebinding), and it serves only the files of local
//! VODs, looked up by id. Byte ranges are supported so the player can seek.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use tauri::State;
use tiny_http::{Header, Request, Response, Server, StatusCode};

use crate::{AppState, CmdResult};

pub(crate) struct MediaServer {
    /// `http://127.0.0.1:<port>`
    origin: String,
    token: String,
}

impl MediaServer {
    /// `find` gives the file of local VOD `id`, if it is one.
    pub(crate) fn start(find: impl Fn(i64) -> Option<String> + Send + Sync + 'static) -> Result<Self> {
        let find = Arc::new(find);
        let server = Server::http("127.0.0.1:0").map_err(|e| anyhow::anyhow!("start the media server: {e}"))?;
        let port = server.server_addr().to_ip().context("media server address")?.port();
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("random token: {e}"))?;
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let origin = format!("http://127.0.0.1:{port}");
        let (host, prefix) = (format!("127.0.0.1:{port}"), format!("/{token}/"));
        std::thread::Builder::new().name("media".into()).spawn(move || {
            for request in server.incoming_requests() {
                let (find, host, prefix) = (find.clone(), host.clone(), prefix.clone());
                // A video streams for a long time; don't hold up other requests.
                std::thread::spawn(move || serve(&*find, request, &host, &prefix));
            }
        })?;
        Ok(Self { origin, token })
    }
}

/// The URL the player loads local VOD `id` from.
#[tauri::command]
pub(crate) async fn media_url(state: State<'_, AppState>, id: i64) -> CmdResult<String> {
    let media = &state.media;
    Ok(format!("{}/{}/{id}", media.origin, media.token))
}

type Find = dyn Fn(i64) -> Option<String> + Send + Sync;

fn serve(find: &Find, request: Request, host: &str, prefix: &str) {
    let response = match respond(find, &request, host, prefix) {
        Ok(response) => response,
        Err(status) => Response::new(StatusCode(status), Vec::new(), Box::new(std::io::empty()) as Box<dyn Read + Send>, Some(0), None),
    };
    let _ = request.respond(response);
}

fn respond(find: &Find, request: &Request, host: &str, prefix: &str) -> Result<Response<Box<dyn Read + Send>>, u16> {
    let header = |name: &'static str| request.headers().iter().find(|h| h.field.equiv(name)).map(|h| h.value.as_str().to_string());
    if header("Host").as_deref() != Some(host) {
        return Err(403);
    }
    let id: i64 = request.url().strip_prefix(prefix).and_then(|id| id.parse().ok()).ok_or(404u16)?;
    let source = find(id).ok_or(404u16)?;
    let mut file = File::open(&source).map_err(|_| 404u16)?;
    let size = file.metadata().map_err(|_| 500u16)?.len();

    let (start, end) = match header("Range").as_deref().map(|r| parse_range(r, size)) {
        None => (0, size.saturating_sub(1)),
        Some(Some(range)) => range,
        Some(None) => return Err(416),
    };
    let len = if size == 0 { 0 } else { end - start + 1 };
    file.seek(SeekFrom::Start(start)).map_err(|_| 500u16)?;
    let mut headers = vec![
        Header::from_bytes("Content-Type", content_type(Path::new(&source))).unwrap(),
        Header::from_bytes("Accept-Ranges", "bytes").unwrap(),
        Header::from_bytes("Cache-Control", "no-store").unwrap(),
    ];
    let status = if header("Range").is_some() {
        headers.push(Header::from_bytes("Content-Range", format!("bytes {start}-{end}/{size}")).unwrap());
        206
    } else {
        200
    };
    let body: Box<dyn Read + Send> = Box::new(file.take(len));
    Ok(Response::new(StatusCode(status), headers, body, Some(len as usize), None))
}

/// A single `bytes=` range within a file of `size` bytes, as (first, last).
fn parse_range(header: &str, size: u64) -> Option<(u64, u64)> {
    let spec = header.strip_prefix("bytes=")?;
    if spec.contains(',') || size == 0 {
        return None;
    }
    let (first, last) = spec.split_once('-')?;
    let (first, last) = match (first.trim(), last.trim()) {
        ("", n) => (size.saturating_sub(n.parse().ok()?), size - 1),
        (a, "") => (a.parse().ok()?, size - 1),
        (a, b) => (a.parse().ok()?, b.parse::<u64>().ok()?.min(size - 1)),
    };
    (first <= last && first < size).then_some((first, last))
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("mp4" | "m4v") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mkv") => "video/x-matroska",
        Some("mov") => "video/quicktime",
        Some("ts") => "video/mp2t",
        Some("avi") => "video/x-msvideo",
        Some("flv") => "video/x-flv",
        Some("mp3") => "audio/mpeg",
        Some("m4a" | "aac") => "audio/mp4",
        Some("wav") => "audio/wav",
        Some("ogg" | "opus") => "audio/ogg",
        Some("flac") => "audio/flac",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Requests `path` from the server with a raw HTTP/1.0 request, returning
    /// the status line and body.
    fn get(origin: &str, path: &str, host: Option<&str>, range: Option<&str>) -> (String, Vec<u8>) {
        use std::io::Write;
        let addr = origin.strip_prefix("http://").unwrap();
        let mut conn = std::net::TcpStream::connect(addr).unwrap();
        let mut req = format!("GET {path} HTTP/1.0\r\nHost: {}\r\n", host.unwrap_or(addr));
        if let Some(r) = range {
            req += &format!("Range: {r}\r\n");
        }
        conn.write_all(format!("{req}\r\n").as_bytes()).unwrap();
        let mut out = Vec::new();
        conn.read_to_end(&mut out).unwrap();
        let split = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&out[..split]).lines().next().unwrap().to_string();
        (head, out[split + 4..].to_vec())
    }

    #[test]
    fn serves_only_local_vods_to_its_own_host_with_the_token() {
        let path = std::env::temp_dir().join(format!("vodscribe-media-{}.mp4", std::process::id()));
        std::fs::write(&path, b"0123456789").unwrap();
        let file = path.to_string_lossy().into_owned();
        let media = MediaServer::start(move |id| (id == 7).then(|| file.clone())).unwrap();
        let ok = format!("/{}/7", media.token);

        let (status, body) = get(&media.origin, &ok, None, None);
        assert!(status.contains("200"), "{status}");
        assert_eq!(body, b"0123456789");
        let (status, body) = get(&media.origin, &ok, None, Some("bytes=2-4"));
        assert!(status.contains("206"), "{status}");
        assert_eq!(body, b"234");

        assert!(get(&media.origin, &format!("/{}/8", media.token), None, None).0.contains("404"), "not a local VOD");
        assert!(get(&media.origin, "/wrongtoken/7", None, None).0.contains("404"), "wrong token");
        assert!(get(&media.origin, &ok, Some("evil.example"), None).0.contains("403"), "other host");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn parses_byte_ranges() {
        assert_eq!(parse_range("bytes=0-", 100), Some((0, 99)));
        assert_eq!(parse_range("bytes=10-19", 100), Some((10, 19)));
        assert_eq!(parse_range("bytes=90-500", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("bytes=5-2", 100), None);
        assert_eq!(parse_range("bytes=0-1,5-6", 100), None);
        assert_eq!(parse_range("items=0-1", 100), None);
    }
}
