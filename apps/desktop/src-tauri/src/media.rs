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
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use tauri::{Manager, State};

use vodscribe_core::db::Db;
use vodscribe_core::{local, peaks, tools};

use crate::AppHandle;
use crate::loopback::{self, Request, Response};
use crate::{AppState, CmdResult, err};

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
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).context("start the media server")?;
        let port = listener.local_addr().context("media server address")?.port();
        let random = || -> Result<String> {
            let mut bytes = [0u8; 16];
            getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("random token: {e}"))?;
            Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
        };
        let (token, footage_token) = (random()?, random()?);
        let origin = format!("http://127.0.0.1:{port}");
        let host = format!("127.0.0.1:{port}");
        let prefixes = (format!("/{token}/"), format!("/{footage_token}/footage/"));
        // A video streams for a long time, so each request gets a thread, up
        // to a limit that a player seeking around never reaches.
        let answer =
            move |request: &Request| respond(&*find, request, &host, &prefixes).unwrap_or_else(Response::empty);
        std::thread::Builder::new()
            .name("media".into())
            .spawn(move || loopback::serve(listener, "media", MAX_ACTIVE, 0, Arc::new(answer)))?;
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
/// for sound alone, or when it can't be read. Kept in the library once read
/// (`Db::set_rate`): the grid the record snaps the VOD's times to.
#[tauri::command]
pub(crate) async fn frame_rate(state: State<'_, AppState>, id: i64) -> CmdResult<Option<f64>> {
    let file = playable_file(&state.db.lock().unwrap(), id);
    let (Some(file), Some(ffmpeg)) = (file, tools::find_ffmpeg(&state.paths.bin())) else { return Ok(None) };
    let fps = tauri::async_runtime::spawn_blocking(move || local::frame_rate(&ffmpeg, Path::new(&file)))
        .await
        .map_err(err)?
        .map_err(err)?;
    let _ = state.db.lock().unwrap().set_rate(id, fps.and_then(vodscribe_core::time::Rate::from_fps));
    Ok(fps)
}

type Find = dyn Fn(Served) -> Option<String> + Send + Sync;

/// The start of a VOD's URL path, and of a footage section's.
type Prefixes = (String, String);

fn respond(find: &Find, request: &Request, host: &str, (vods, footage): &Prefixes) -> Result<Response, u16> {
    loopback::check(request, host)?;
    let url = request.url.as_str();
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

    let range = request.header("Range");
    let (start, end) = match range.map(|r| parse_range(r, size)) {
        None => (0, size.saturating_sub(1)),
        Some(Some(range)) => range,
        Some(None) => return Ok(Response::empty(416).header("Content-Range", format!("bytes */{size}"))),
    };
    let len = if size == 0 { 0 } else { end - start + 1 };
    file.seek(SeekFrom::Start(start)).map_err(|_| 500u16)?;
    let status = if range.is_some() { 206 } else { 200 };
    let mut response = Response::reader(status, Box::new(file.take(len)), len)
        .header("Content-Type", content_type(Path::new(&source)))
        .header("Accept-Ranges", "bytes")
        .header("Cache-Control", "no-store");
    if range.is_some() {
        response = response.header("Content-Range", format!("bytes {start}-{end}/{size}"));
    }
    Ok(response)
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
mod tests;
