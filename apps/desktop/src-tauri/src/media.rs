//! Serves local VODs' files to the player's <video>, over HTTP on 127.0.0.1.
//!
//! A custom URL scheme would be simpler, but WebKitGTK won't load one into the
//! dev server's http page, and a plain HTTP server works the same everywhere.
//! It only listens on the loopback interface, every URL carries a random token
//! made at launch, requests must name the server's own host (so a web page
//! can't reach it through DNS rebinding), and it serves only the files of local
//! VODs, looked up by id, and edits' footage sections, looked up by name. Byte
//! ranges are supported so the player can seek.
//!
//! The app's own player (ffmpeg) reads a section still downloading through
//! here rather than from the file: ffmpeg opens files on Windows without
//! letting them be renamed while open, and the download's file is renamed
//! when it's done, which then failed. Each request finds the file anew, so
//! a seek after the renaming reads it where it went.
//!
//! The player's waveform (`peaks`) and frame rate are asked for here too.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result};
use tauri::{AppHandle, Manager, State};
use tiny_http::{Header, Request, Response, Server, StatusCode};

use vodscribe_core::db::Db;
use vodscribe_core::{local, peaks, tools};

use crate::{AppState, CmdResult, err, loopback};

/// Requests served at once.
const MAX_ACTIVE: usize = 32;

/// What a URL asks for: a VOD's file (`/<token>/<id>`) or a footage
/// section's (`/<footage token>/footage/<name>`, `footage::section_stem`).
/// Footage has a token of its own: its URLs go to ffmpeg's command line,
/// which other accounts on the computer can read, and must not open the VODs.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Served {
    Vod(i64),
    Footage(String),
}

pub(crate) struct MediaServer {
    /// `http://127.0.0.1:<port>`
    origin: String,
    token: String,
    /// For footage alone (`Served`).
    footage_token: String,
}

impl MediaServer {
    /// `find` gives the file asked for, if there is one.
    pub(crate) fn start(find: impl Fn(Served) -> Option<String> + Send + Sync + 'static) -> Result<Self> {
        let find = Arc::new(find);
        let server = Server::http("127.0.0.1:0").map_err(|e| anyhow::anyhow!("start the media server: {e}"))?;
        let port = server.server_addr().to_ip().context("media server address")?.port();
        let random = || -> Result<String> {
            let mut bytes = [0u8; 16];
            getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("random token: {e}"))?;
            Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
        };
        let (token, footage_token) = (random()?, random()?);
        let origin = format!("http://127.0.0.1:{port}");
        let host = format!("127.0.0.1:{port}");
        let prefixes = Arc::new((format!("/{token}/"), format!("/{footage_token}/footage/")));
        let active = Arc::new(AtomicUsize::new(0));
        std::thread::Builder::new().name("media".into()).spawn(move || {
            for request in server.incoming_requests() {
                // A video streams for a long time, so each request gets a thread,
                // up to a limit that a player seeking around never reaches.
                if active.fetch_add(1, Ordering::SeqCst) >= MAX_ACTIVE {
                    active.fetch_sub(1, Ordering::SeqCst);
                    let _ = request.respond(Response::empty(503));
                    continue;
                }
                let (find, host, prefixes, active) = (find.clone(), host.clone(), prefixes.clone(), active.clone());
                std::thread::spawn(move || {
                    serve(&*find, request, &host, &prefixes);
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        })?;
        Ok(Self { origin, token, footage_token })
    }
}

/// The URL the player loads local VOD `id` from.
#[tauri::command]
pub(crate) async fn media_url(state: State<'_, AppState>, id: i64) -> CmdResult<String> {
    let media = &state.media;
    Ok(format!("{}/{}/{id}", media.origin, media.token))
}

/// The URL the app's own player reads footage section `name` from while it
/// downloads (`footage::section_stem`).
pub(crate) fn footage_url(state: &AppState, name: &str) -> PathBuf {
    let media = &state.media;
    PathBuf::from(format!("{}/{}/footage/{name}", media.origin, media.footage_token))
}

/// The file VOD `id` plays from here: a file on this computer, or a video kept
/// from a site without an embed.
pub(crate) fn playable_file(db: &Db, id: i64) -> Option<String> {
    db.playable_path(id).ok().flatten()
}

/// VOD `id`'s loudness over time (`peaks`), ten levels a second; empty if it
/// was never read. One that plays from a file here is read now if need be.
#[tauri::command]
pub(crate) async fn peaks(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<tauri::ipc::Response> {
    let dir = state.paths.peaks();
    if let Some(levels) = peaks::load(&dir, id) {
        return Ok(tauri::ipc::Response::new(levels));
    }
    let file = playable_file(&state.db.lock().unwrap(), id);
    let (Some(file), Some(ffmpeg)) = (file, tools::find_ffmpeg(&state.paths.bin())) else {
        return Ok(tauri::ipc::Response::new(Vec::new()));
    };
    // One at a time, so opening a VOD twice reads its file once.
    static READING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let levels = tauri::async_runtime::spawn_blocking(move || {
        let _one = READING.lock().unwrap();
        if let Some(levels) = peaks::load(&dir, id) {
            return Ok(levels);
        }
        let levels = peaks::read(&ffmpeg, Path::new(&file), &Default::default())?;
        // Kept only while the VOD's still there (removing it deletes its peaks).
        let state = app.state::<AppState>();
        let db = state.db.lock().unwrap();
        if db.vod(id)?.is_some() {
            peaks::save(&dir, id, &levels)?;
        }
        anyhow::Ok(levels)
    })
    .await
    .map_err(err)?
    .map_err(err)?;
    Ok(tauri::ipc::Response::new(levels))
}

/// The frame of VOD `id`'s video at `time` seconds, as a small JPEG (for the
/// picture shown while a stretch is dragged); empty for a VOD that doesn't
/// play from a file here, or sound alone.
#[tauri::command]
pub(crate) async fn frame(state: State<'_, AppState>, id: i64, time: f64) -> CmdResult<tauri::ipc::Response> {
    let file = playable_file(&state.db.lock().unwrap(), id);
    let (Some(file), Some(ffmpeg)) = (file, tools::find_ffmpeg(&state.paths.bin())) else {
        return Ok(tauri::ipc::Response::new(Vec::new()));
    };
    if !time.is_finite() {
        return Err("Not a time in the video.".into());
    }
    let jpeg = tauri::async_runtime::spawn_blocking(move || local::frame(&ffmpeg, Path::new(&file), time, 240))
        .await
        .map_err(err)?
        .unwrap_or_default();
    Ok(tauri::ipc::Response::new(jpeg))
}

/// The frame rate of VOD `id`'s video, for stepping a frame at a time. None
/// for sound alone, or when it can't be read.
#[tauri::command]
pub(crate) async fn frame_rate(state: State<'_, AppState>, id: i64) -> CmdResult<Option<f64>> {
    let file = playable_file(&state.db.lock().unwrap(), id);
    let (Some(file), Some(ffmpeg)) = (file, tools::find_ffmpeg(&state.paths.bin())) else { return Ok(None) };
    tauri::async_runtime::spawn_blocking(move || local::frame_rate(&ffmpeg, Path::new(&file)))
        .await
        .map_err(err)?
        .map_err(err)
}

type Find = dyn Fn(Served) -> Option<String> + Send + Sync;

/// The start of a VOD's URL path, and of a footage section's.
type Prefixes = (String, String);

fn serve(find: &Find, request: Request, host: &str, prefixes: &Prefixes) {
    let response = match respond(find, &request, host, prefixes) {
        Ok(response) => response,
        Err(status) => Response::new(
            StatusCode(status),
            Vec::new(),
            Box::new(std::io::empty()) as Box<dyn Read + Send>,
            Some(0),
            None,
        ),
    };
    let _ = request.respond(response);
}

fn respond(
    find: &Find,
    request: &Request,
    host: &str,
    (vods, footage): &Prefixes,
) -> Result<Response<Box<dyn Read + Send>>, u16> {
    let header = |name: &'static str| {
        request.headers().iter().find(|h| h.field.equiv(name)).map(|h| h.value.as_str().to_string())
    };
    loopback::check(request, host)?;
    let url = request.url();
    let asked = match (url.strip_prefix(footage.as_str()), url.strip_prefix(vods.as_str())) {
        // Only a section's name: digits and dashes.
        (Some(name), _) if !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit() || b == b'-') => {
            Served::Footage(name.to_string())
        }
        (None, Some(id)) => Served::Vod(id.parse().map_err(|_| 404u16)?),
        _ => return Err(404),
    };
    let source = find(asked).ok_or(404u16)?;
    let mut file = open_regular(Path::new(&source)).ok_or(404u16)?;
    let size = file.metadata().map_err(|_| 500u16)?.len();

    let (start, end) = match header("Range").as_deref().map(|r| parse_range(r, size)) {
        None => (0, size.saturating_sub(1)),
        Some(Some(range)) => range,
        Some(None) => {
            let range = Header::from_bytes("Content-Range", format!("bytes */{size}")).unwrap();
            return Ok(Response::new(StatusCode(416), vec![range], Box::new(std::io::empty()), Some(0), None));
        }
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
    // Always send Content-Length: tiny_http otherwise switches large bodies to
    // chunked encoding, and without a length the player can't seek or show a
    // timeline.
    Ok(Response::new(StatusCode(status), headers, body, Some(len as usize), None).with_chunked_threshold(usize::MAX))
}

/// Opens `path` only if it's a regular file and not a symlink, and (on Unix)
/// only if the file opened is the one checked, so swapping the file for a link
/// to something else between the check and the open doesn't work.
fn open_regular(path: &Path) -> Option<File> {
    let checked = std::fs::symlink_metadata(path).ok()?;
    if !checked.file_type().is_file() {
        return None;
    }
    let file = File::open(path).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file.metadata().ok()?;
        if (opened.dev(), opened.ino()) != (checked.dev(), checked.ino()) {
            return None;
        }
    }
    Some(file)
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
    vodscribe_core::local::media_type(path).unwrap_or("application/octet-stream")
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

    /// Sends a raw request line and returns the whole response.
    fn raw(origin: &str, line: &str) -> String {
        use std::io::Write;
        let addr = origin.strip_prefix("http://").unwrap();
        let mut conn = std::net::TcpStream::connect(addr).unwrap();
        conn.write_all(format!("{line}\r\nHost: {addr}\r\n\r\n").as_bytes()).unwrap();
        let mut out = Vec::new();
        conn.read_to_end(&mut out).unwrap();
        String::from_utf8_lossy(&out).into_owned()
    }

    /// The response headers for `path`.
    fn head_of(origin: &str, path: &str) -> String {
        use std::io::Write;
        let addr = origin.strip_prefix("http://").unwrap();
        let mut conn = std::net::TcpStream::connect(addr).unwrap();
        conn.write_all(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
        let mut out = Vec::new();
        conn.read_to_end(&mut out).unwrap();
        let text = String::from_utf8_lossy(&out);
        text.split("\r\n\r\n").next().unwrap().to_string()
    }

    fn footage_of(media: &MediaServer, name: &str) -> PathBuf {
        PathBuf::from(format!("{}/{}/footage/{name}", media.origin, media.footage_token))
    }

    /// Footage's token (on ffmpeg's command line, which other accounts can
    /// read) opens footage alone, and the VODs' token no footage.
    #[test]
    fn each_token_opens_only_its_own() {
        let path = std::env::temp_dir().join(format!("vodscribe-media-tokens-{}.mkv", std::process::id()));
        std::fs::write(&path, b"x").unwrap();
        let file = path.to_string_lossy().into_owned();
        let media = MediaServer::start(move |_| Some(file.clone())).unwrap();
        let (vod, section) = (format!("/{}/7", media.token), format!("/{}/footage/7-0-9", media.footage_token));
        assert!(get(&media.origin, &vod, None, None).0.contains("200"));
        assert!(get(&media.origin, &section, None, None).0.contains("200"));
        assert!(get(&media.origin, &format!("/{}/7", media.footage_token), None, None).0.contains("404"), "a VOD");
        assert!(get(&media.origin, &format!("/{}/footage/7-0-9", media.token), None, None).0.contains("404"));
        assert_eq!(footage_of(&media, "7-0-9").to_string_lossy(), format!("{}{section}", media.origin));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn serves_only_local_vods_to_its_own_host_with_the_token() {
        let path = std::env::temp_dir().join(format!("vodscribe-media-{}.mp4", std::process::id()));
        // Larger than tiny_http's 32 KB threshold for switching to chunked encoding.
        let data: Vec<u8> = (0..100_000u32).map(|i| (i % 10) as u8 + b'0').collect();
        std::fs::write(&path, &data).unwrap();
        let file = path.to_string_lossy().into_owned();
        let media = MediaServer::start(move |asked| (asked == Served::Vod(7)).then(|| file.clone())).unwrap();
        let ok = format!("/{}/7", media.token);

        let (status, body) = get(&media.origin, &ok, None, None);
        assert!(status.contains("200"), "{status}");
        assert!(head_of(&media.origin, &ok).contains("Content-Length: 100000"), "players need the length to seek");
        assert_eq!(body, data);
        let (status, body) = get(&media.origin, &ok, None, Some("bytes=2-4"));
        assert!(status.contains("206"), "{status}");
        assert_eq!(body, b"234");

        assert!(get(&media.origin, &format!("/{}/8", media.token), None, None).0.contains("404"), "not a local VOD");
        assert!(get(&media.origin, &format!("/{}/clip/7", media.token), None, None).0.contains("404"), "no clips");
        assert!(get(&media.origin, &format!("/{}/footage/7-0-9", media.token), None, None).0.contains("404"), "none");
        assert!(get(&media.origin, &format!("/{}/footage/..%2f7", media.token), None, None).0.contains("404"), "name");
        assert!(get(&media.origin, "/wrongtoken/7", None, None).0.contains("404"), "wrong token");
        let (status, _) = get(&media.origin, &ok, None, Some("bytes=200000-"));
        assert!(status.contains("416"), "{status}");
        let post = raw(&media.origin, &format!("POST {ok} HTTP/1.0"));
        assert!(post.lines().next().unwrap().contains("405"), "{post}");
        assert!(get(&media.origin, &ok, Some("evil.example"), None).0.contains("403"), "other host");
        // Swapped for a link to another file: refused.
        #[cfg(unix)]
        {
            let secret = path.with_extension("secret");
            std::fs::write(&secret, b"secret").unwrap();
            std::fs::remove_file(&path).unwrap();
            std::os::unix::fs::symlink(&secret, &path).unwrap();
            assert!(get(&media.origin, &ok, None, None).0.contains("404"), "symlink");
            let _ = std::fs::remove_file(&secret);
        }
        let _ = std::fs::remove_file(&path);
    }

    /// ffmpeg reads a section through the server, allowed HTTP for it alone
    /// (`tools::ffmpeg_allow_for`), seeking in it, and finds it where it went
    /// when it's renamed between requests (as a download's file is when done).
    #[test]
    fn ffmpeg_reads_footage_through_it() {
        let ffmpeg = PathBuf::from("ffmpeg");
        if std::process::Command::new(&ffmpeg).arg("-version").output().is_err() {
            eprintln!("skipped: no ffmpeg on PATH");
            return;
        }
        let dir = std::env::temp_dir().join(format!("vodscribe-served-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (part, done) = (dir.join("s.mkv.part"), dir.join("s.mkv"));
        let made = std::process::Command::new(&ffmpeg)
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=160x90:r=30:d=4",
                "-c:v",
                "mjpeg",
                "-f",
                "matroska",
            ])
            .arg(&part)
            .status()
            .unwrap();
        assert!(made.success());
        let (p, d) = (part.clone(), done.clone());
        let media = MediaServer::start(move |asked| {
            (asked == Served::Footage("7-0-4000".into()))
                .then(|| if p.is_file() { &p } else { &d }.display().to_string())
        })
        .unwrap();
        let url = footage_of(&media, "7-0-4000");
        assert!(vodscribe_core::tools::served(&url));
        let frame = vodscribe_core::local::frame(&ffmpeg, &url, 2.5, 160).unwrap();
        assert_eq!(frame[..2], [0xff, 0xd8], "a JPEG");
        std::fs::rename(&part, &done).unwrap();
        assert!(vodscribe_core::local::frame(&ffmpeg, &url, 3.5, 160).is_ok(), "read where it went");
        // Elsewhere, files only: the same URL handed in as a file's isn't read over HTTP.
        let elsewhere = PathBuf::from(format!("http://localhost:{}/x", media.origin.rsplit(':').next().unwrap()));
        assert!(!vodscribe_core::tools::served(&elsewhere));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Serves $VODSCRIBE_MEDIA_FILE as VOD 1 for a few minutes and prints its
    /// URL, for trying the server with a real player: `-- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn serve_a_file_for_manual_testing() {
        let file = std::env::var("VODSCRIBE_MEDIA_FILE").expect("set VODSCRIBE_MEDIA_FILE");
        let media = MediaServer::start(move |asked| (asked == Served::Vod(1)).then(|| file.clone())).unwrap();
        println!("MEDIA_URL {}/{}/1", media.origin, media.token);
        std::thread::sleep(std::time::Duration::from_secs(300));
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
