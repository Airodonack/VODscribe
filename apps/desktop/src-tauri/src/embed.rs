//! A site's VOD plays in the site's own player, in its embed frame, on top;
//! the app's player draws the picture under it, frame-exact. The site's
//! player fetches; what it appends to its media buffers comes here
//! (embed_shim.js in the frame, through the page: embed.ts), and the app's
//! player plays it. Each buffer's appends are kept as runs: media that
//! follows on, after the buffer's init segment, one file each, in a folder
//! of the app's that lasts while the VOD is open (a session: what it kept
//! goes when it closes, and any left over when the app starts). The runs
//! are laid out as the player's timeline, picture with sound; where nothing
//! has been appended, the player waits.
//!
//! What comes from the frame is the site's to say, so it's checked: each
//! session has a token the page sends with each append and the close (a
//! closing VOD's last appends never reach the next one's), the times it
//! names are kept within the VOD, and what's kept is capped (`MOST`).

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use tauri::Manager;
use vodscribe_core::footage::Source;
use vodscribe_player::timeline::{Sound, Stretch, Timeline};

use crate::AppHandle;
use crate::{AppState, CmdResult, err};

/// The VOD whose embed feeds the player, if one does.
#[derive(Default)]
pub(crate) struct Embed {
    session: Mutex<Option<Session>>,
    /// The tokens given out, counted.
    made: AtomicU64,
}

struct Session {
    /// What the page sends with what's for this session.
    token: u64,
    duration: f64,
    dir: PathBuf,
    buffers: HashMap<u32, Buffer>,
    /// Files made, counted (each run's is named by it).
    files: u32,
    /// Bytes kept in all.
    bytes: u64,
}

struct Buffer {
    /// Its type as the site's player gave it (`x-type`).
    kind: String,
    /// Picture (else sound only).
    video: bool,
    ext: &'static str,
    init: Vec<u8>,
    runs: Vec<Run>,
    /// The run the last append went to.
    current: Option<usize>,
}

struct Run {
    path: PathBuf,
    /// VOD time where the file's own time is 0 (ffmpeg counts from the first frame).
    start: f64,
    /// How far its appends reach.
    end: f64,
    /// How far it reaches for certain: as of the append before the last
    /// (the last one's media may still be coming in the next).
    safe: f64,
    /// The bytes in its file.
    bytes: u64,
}

/// An append starting this near where its run ends follows on.
const FOLLOWS: f64 = 0.25;

/// The most a session keeps; past it, the runs farthest from what's being
/// appended go (the site's player fetches them again if they're played).
const MOST: u64 = 4 << 30;

/// How far past the VOD's length an append's times may reach (a site's
/// stream can run a little longer than it says).
const PAST_END: f64 = 60.0;

/// The folder sessions keep their files in, in the app's data.
fn root(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().paths.temp_embed()
}

/// Deletes what sessions left in `dir` (`root`: the app quit, or stopped,
/// while one was open).
pub(crate) fn clear(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// Whether a site's VOD's picture can be taken into the app's player (built
/// with the embed-takeover feature); the page leaves their players alone if not.
#[tauri::command]
pub(crate) fn embed_takeover() -> bool {
    cfg!(feature = "embed-takeover")
}

const NO_TAKEOVER: &str = "a site's player isn't taken over in this build";

/// Starts keeping what VOD `vod_id`'s embed appends, and opens the player
/// on it, waiting for it, at `at`: the session's token, which the page
/// sends with each append and the close. Fails if it's closed (or another
/// opened) before the player is.
#[tauri::command]
pub(crate) async fn embed_open(app: AppHandle, vod_id: i64, at: f64) -> CmdResult<u64> {
    if !embed_takeover() {
        return Err(NO_TAKEOVER.into());
    }
    let duration = {
        let state = app.state::<AppState>();
        let db = state.db.lock().unwrap();
        db.vod(vod_id).map_err(err)?.and_then(|v| v.info.duration).unwrap_or(0.0)
    };
    let state = app.state::<AppState>();
    let token = state.embed.made.fetch_add(1, Ordering::AcqRel) + 1;
    let dir = root(&app).join(token.to_string());
    std::fs::create_dir_all(&dir).map_err(err)?;
    let session = Session { token, duration, dir, buffers: HashMap::new(), files: 0, bytes: 0 };
    // One at a time: an earlier one's files go.
    if let Some(old) = state.embed.session.lock().unwrap().replace(session) {
        forget(old);
    }
    crate::player::open_embed(app.clone(), vod_id, token, timeline_of_nothing(duration), at).await?;
    // Closed, or another opened, while the player opened: it goes too.
    if !state.embed.session.lock().unwrap().as_ref().is_some_and(|s| s.token == token) {
        crate::player::close_embed(&app, token);
        return Err(crate::player::CLOSED.into());
    }
    Ok(token)
}

fn timeline_of_nothing(duration: f64) -> Timeline {
    Timeline::from_stretches(vec![Stretch {
        at: 0.0,
        length: duration.max(1.0),
        piece_id: None,
        source: None,
        from: 0.0,
        sound: None,
    }])
}

/// Deletes what `session` kept, off the caller's thread.
fn forget(session: Session) {
    std::thread::spawn(move || {
        let _ = std::fs::remove_dir_all(&session.dir);
    });
}

/// Ends session `token`, if it's the one open: its player closes, and what
/// it kept goes.
#[tauri::command]
pub(crate) async fn embed_close(app: AppHandle, token: u64) {
    let state = app.state::<AppState>();
    let session = {
        let mut guard = state.embed.session.lock().unwrap();
        if guard.as_ref().is_some_and(|s| s.token == token) { guard.take() } else { None }
    };
    if let Some(session) = session {
        forget(session);
    }
    crate::player::close_embed(&app, token);
}

/// One append: the body is its bytes; the headers say its session
/// (`x-session`, the token), its buffer (`x-buffer`), the buffer's type
/// (`x-type`), whether an init segment is in it (`x-init`), the VOD times its
/// media holds (`x-ranges`, JSON pairs), and whether it ends on a whole box
/// or block (`x-complete`). One for a session not open is dropped. Off the
/// main thread (it writes to disk); the page sends one at a time, in order.
#[tauri::command(async)]
pub(crate) fn embed_append(app: AppHandle, request: tauri::ipc::Request<'_>) -> CmdResult<()> {
    if !embed_takeover() {
        return Err(NO_TAKEOVER.into());
    }
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("An append comes as bytes.".into());
    };
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or("");
    let token: u64 = header("x-session").parse().map_err(err)?;
    let id: u32 = header("x-buffer").parse().map_err(err)?;
    let kind = header("x-type").to_string();
    let init = header("x-init") == "1";
    let complete = header("x-complete") == "1";
    let ranges: Vec<(f64, f64)> = serde_json::from_str(header("x-ranges")).unwrap_or_default();

    let state = app.state::<AppState>();
    let mut guard = state.embed.session.lock().unwrap();
    let Some(session) = guard.as_mut().filter(|s| s.token == token) else { return Ok(()) };
    let ranges = session.within(&ranges);
    if !session.append(id, &kind, init, complete, &ranges, bytes).map_err(err)? {
        return Ok(());
    }
    let timeline = layout(session);
    drop(guard);
    crate::player::set_embed_timeline(&app, token, timeline);
    Ok(())
}

impl Session {
    /// `ranges` as times of the VOD: in order, from 0, and not far past its end.
    fn within(&self, ranges: &[(f64, f64)]) -> Vec<(f64, f64)> {
        let most = if self.duration > 0.0 { self.duration + PAST_END } else { f64::MAX };
        ranges
            .iter()
            .filter(|(a, b)| a.is_finite() && b.is_finite())
            .map(|&(a, b)| (a.max(0.0), b.min(most)))
            .filter(|(a, b)| b > a)
            .collect()
    }

    /// One append to buffer `id` (of type `kind`): `bytes`, with an init
    /// segment first if `init`, holding the VOD times `ranges`, ending on a
    /// whole box or block if `complete`. Kept in its run's file; whether
    /// anything was kept.
    fn append(
        &mut self,
        id: u32,
        kind: &str,
        init: bool,
        complete: bool,
        ranges: &[(f64, f64)],
        bytes: &[u8],
    ) -> std::io::Result<bool> {
        let buffer = self.buffers.entry(id).or_insert_with(|| Buffer {
            kind: String::new(),
            video: false,
            ext: "mp4",
            init: Vec::new(),
            runs: Vec::new(),
            current: None,
        });
        // A buffer's type can change (SourceBuffer.changeType): what follows is read as the new one's.
        if buffer.kind != kind {
            buffer.kind = kind.to_string();
            buffer.video = !is_sound(kind);
            buffer.ext = if kind.contains("webm") { "webm" } else { "mp4" };
            buffer.init.clear();
            buffer.current = None;
        }
        let mut bytes = bytes;
        if init {
            // An init segment, then maybe media. The same init again (Twitch
            // sends it before each segment) goes on with the run; a new one
            // begins a new run.
            let (head, media) = split_init(bytes, buffer.ext == "webm");
            if head != buffer.init {
                buffer.init = head.to_vec();
                buffer.current = None;
            }
            bytes = media;
            if bytes.is_empty() {
                return Ok(false);
            }
        }
        if buffer.init.is_empty() {
            return Ok(false);
        }
        let start = ranges.iter().map(|r| r.0).fold(f64::INFINITY, f64::min);
        let end = ranges.iter().map(|r| r.1).fold(f64::NEG_INFINITY, f64::max);
        let follows = buffer.current.is_some_and(|i| {
            let run = &buffer.runs[i];
            ranges.is_empty() || (start >= run.end - FOLLOWS && start <= run.end + FOLLOWS)
        });
        if !follows {
            if ranges.is_empty() {
                // The rest of something not kept (no run to add it to).
                return Ok(false);
            }
            self.files += 1;
            let path = self.dir.join(format!("{id}-{}.{}", self.files, buffer.ext));
            std::fs::write(&path, &buffer.init)?;
            let bytes = buffer.init.len() as u64;
            self.bytes += bytes;
            buffer.runs.push(Run { path, start, end: start, safe: start, bytes });
            buffer.current = Some(buffer.runs.len() - 1);
        }
        let run = &mut buffer.runs[buffer.current.unwrap()];
        std::fs::OpenOptions::new().append(true).open(&run.path).and_then(|mut f| f.write_all(bytes))?;
        run.bytes += bytes.len() as u64;
        self.bytes += bytes.len() as u64;
        if !ranges.is_empty() {
            run.safe = run.end;
            run.end = run.end.max(end);
        }
        // Whole boxes or blocks: what its ranges name is all in the file. (Else
        // the last of it may come with the next append: counted then.)
        if complete {
            run.safe = run.end;
        }
        if self.bytes > MOST {
            self.shed(if start.is_finite() { start } else { 0.0 });
        }
        Ok(true)
    }

    /// Past `MOST`: deletes the runs farthest from `at` (never one an append
    /// goes to now) until what's kept is well within it again.
    fn shed(&mut self, at: f64) {
        let distance = |r: &Run| if at < r.start { r.start - at } else { (at - r.end).max(0.0) };
        let mut far: Vec<(f64, PathBuf)> = self
            .buffers
            .values()
            .flat_map(|b| b.runs.iter().enumerate().filter(|(i, _)| b.current != Some(*i)).map(|(_, r)| r))
            .map(|r| (distance(r), r.path.clone()))
            .collect();
        far.sort_by(|x, y| y.0.total_cmp(&x.0));
        for (_, path) in far {
            if self.bytes <= MOST / 5 * 4 {
                break;
            }
            for buffer in self.buffers.values_mut() {
                let Some(i) = buffer.runs.iter().position(|r| r.path == path) else { continue };
                let run = buffer.runs.remove(i);
                self.bytes -= run.bytes;
                let _ = std::fs::remove_file(&run.path);
                buffer.current = buffer.current.map(|c| if c > i { c - 1 } else { c });
            }
        }
    }
}

/// `bytes` split where the media after an init segment begins: fMP4's
/// first moof, WebM's first cluster.
fn split_init(bytes: &[u8], webm: bool) -> (&[u8], &[u8]) {
    let at = if webm {
        bytes.windows(4).position(|w| w == [0x1f, 0x43, 0xb6, 0x75]).unwrap_or(bytes.len())
    } else {
        // Box by box to the first moof; a size that doesn't fit what's here
        // (or is no size) ends it: all of it counts as the init.
        let mut o = 0;
        while o + 8 <= bytes.len() && &bytes[o + 4..o + 8] != b"moof" {
            let size = u32::from_be_bytes(bytes[o..o + 4].try_into().unwrap()) as u64;
            let size = if size == 1 && o + 16 <= bytes.len() {
                u64::from_be_bytes(bytes[o + 8..o + 16].try_into().unwrap())
            } else {
                size
            };
            match usize::try_from(size) {
                Ok(size) if size >= 8 && size <= bytes.len() - o => o += size,
                _ => return bytes.split_at(bytes.len()),
            }
        }
        o.min(bytes.len())
    };
    bytes.split_at(at)
}

/// A buffer's type that is sound alone.
fn is_sound(kind: &str) -> bool {
    let k = kind.to_ascii_lowercase();
    let sound = ["mp4a", "opus", "vorbis", "ac-3", "ec-3", "flac"].iter().any(|c| k.contains(c));
    let picture = ["avc", "vp8", "vp9", "vp09", "av01", "hev", "hvc"].iter().any(|c| k.contains(c));
    sound && !picture
}

/// The runs laid out on the VOD's time: where a picture run and a sound run
/// (if the embed has a sound buffer) both reach for certain, they play;
/// elsewhere the player waits.
fn layout(session: &Session) -> Timeline {
    let runs = |video: bool| -> Vec<&Run> {
        session
            .buffers
            .values()
            .filter(|b| b.video == video)
            .flat_map(|b| b.runs.iter())
            .filter(|r| r.safe > r.start)
            .collect()
    };
    let pictures = runs(true);
    let has_sound = session.buffers.values().any(|b| !b.video);
    let sounds = runs(false);
    // Each piece that plays: from, to, picture run, sound run.
    let mut pieces: Vec<(f64, f64, &Run, Option<&Run>)> = Vec::new();
    for p in &pictures {
        if !has_sound {
            pieces.push((p.start, p.safe, p, None));
            continue;
        }
        for s in &sounds {
            let (a, b) = (p.start.max(s.start), p.safe.min(s.safe));
            if b > a {
                pieces.push((a, b, p, Some(s)));
            }
        }
    }
    pieces.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut stretches = Vec::new();
    let mut at = 0.0;
    let gap =
        |from: f64, to: f64| Stretch { at: from, length: to - from, piece_id: None, source: None, from, sound: None };
    for (a, b, p, s) in pieces {
        let a = a.max(at);
        if b <= a {
            continue;
        }
        if a > at {
            stretches.push(gap(at, a));
        }
        stretches.push(Stretch {
            at: a,
            length: b - a,
            piece_id: None,
            source: Some(Source { path: p.path.clone(), offset: p.start, section: None }),
            from: a - p.start,
            sound: s.map(|s| Sound { path: s.path.clone(), offset: s.start }),
        });
        at = b;
    }
    let duration = session.duration.max(at);
    if duration > at {
        stretches.push(gap(at, duration));
    }
    Timeline::from_stretches(stretches)
}

#[cfg(test)]
mod tests;
