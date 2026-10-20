//! A site's VOD plays in the site's own player, in its embed frame, on top;
//! the app's player draws the picture under it, frame-exact. The site's
//! player fetches; what it appends to its media buffers comes here
//! (embed_shim.js in the frame, through the page: embed.ts), and the app's
//! player plays it. Each buffer's appends are kept as runs: media that
//! follows on, after the buffer's init segment, one file each, in a folder
//! for the VOD that lasts while it plays. The runs are laid out as the
//! player's timeline, picture with sound; where nothing has been appended,
//! the player waits.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{AppHandle, Manager};
use vodscribe_core::footage::Source;
use vodscribe_player::timeline::{Sound, Stretch, Timeline};

use crate::{AppState, CmdResult, err};

/// The VOD whose embed feeds the player, if one does.
#[derive(Default)]
pub(crate) struct Embed {
    session: Mutex<Option<Session>>,
}

struct Session {
    vod_id: i64,
    duration: f64,
    dir: PathBuf,
    buffers: HashMap<u32, Buffer>,
    files: u32,
}

struct Buffer {
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
}

/// An append starting this near where its run ends follows on.
const FOLLOWS: f64 = 0.25;

/// Starts keeping what VOD `vod_id`'s embed appends, and opens the player
/// on it, waiting for it, at `at`.
#[tauri::command]
pub(crate) async fn embed_open(app: AppHandle, vod_id: i64, at: f64) -> CmdResult<()> {
    let duration = {
        let state = app.state::<AppState>();
        let db = state.db.lock().unwrap();
        db.vod(vod_id).map_err(err)?.and_then(|v| v.info.duration).unwrap_or(0.0)
    };
    // One VOD's at a time: whatever an earlier one left goes.
    let root = std::env::temp_dir().join("vodscribe-embed");
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join(vod_id.to_string());
    std::fs::create_dir_all(&dir).map_err(err)?;
    *app.state::<AppState>().embed.session.lock().unwrap() =
        Some(Session { vod_id, duration, dir, buffers: HashMap::new(), files: 0 });
    crate::player::open_embed(app.clone(), vod_id, timeline_of_nothing(duration), at).await?;
    Ok(())
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

/// Stops keeping appends, and drops what was kept.
#[tauri::command]
pub(crate) fn embed_close(app: AppHandle) {
    if let Some(session) = app.state::<AppState>().embed.session.lock().unwrap().take() {
        let _ = std::fs::remove_dir_all(&session.dir);
    }
}

/// One append: the body is its bytes; the headers say its buffer
/// (`x-buffer`), the buffer's type (`x-type`), whether an init segment is in
/// it (`x-init`), the VOD times its media holds (`x-ranges`, JSON pairs), and
/// whether it ends on a whole box or block (`x-complete`). Off the main
/// thread (it writes to disk); the page sends one at a time, in order.
#[tauri::command(async)]
pub(crate) fn embed_append(app: AppHandle, request: tauri::ipc::Request<'_>) -> CmdResult<()> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("An append comes as bytes.".into());
    };
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or("");
    let id: u32 = header("x-buffer").parse().map_err(err)?;
    let kind = header("x-type").to_string();
    let init = header("x-init") == "1";
    let complete = header("x-complete") == "1";
    let ranges: Vec<(f64, f64)> = serde_json::from_str(header("x-ranges")).unwrap_or_default();

    let state = app.state::<AppState>();
    let mut guard = state.embed.session.lock().unwrap();
    let Some(session) = guard.as_mut() else { return Ok(()) };
    if !session.append(id, &kind, init, complete, &ranges, bytes).map_err(err)? {
        return Ok(());
    }
    let timeline = layout(session);
    let vod_id = session.vod_id;
    drop(guard);
    crate::player::set_embed_timeline(&app, vod_id, timeline);
    Ok(())
}

impl Session {
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
            video: !is_sound(kind),
            ext: if kind.contains("webm") { "webm" } else { "mp4" },
            init: Vec::new(),
            runs: Vec::new(),
            current: None,
        });
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
            buffer.runs.push(Run { path, start, end: start, safe: start });
            buffer.current = Some(buffer.runs.len() - 1);
        }
        let run = &mut buffer.runs[buffer.current.unwrap()];
        std::fs::OpenOptions::new().append(true).open(&run.path).and_then(|mut f| f.write_all(bytes))?;
        if !ranges.is_empty() {
            run.safe = run.end;
            run.end = run.end.max(end);
        }
        // Whole boxes or blocks: what its ranges name is all in the file. (Else
        // the last of it may come with the next append: counted then.)
        if complete {
            run.safe = run.end;
        }
        Ok(true)
    }
}

/// `bytes` split where the media after an init segment begins: fMP4's
/// first moof, WebM's first cluster.
fn split_init(bytes: &[u8], webm: bool) -> (&[u8], &[u8]) {
    let at = if webm {
        bytes.windows(4).position(|w| w == [0x1f, 0x43, 0xb6, 0x75]).unwrap_or(bytes.len())
    } else {
        let mut o = 0;
        while o + 8 <= bytes.len() && &bytes[o + 4..o + 8] != b"moof" {
            let size = u32::from_be_bytes(bytes[o..o + 4].try_into().unwrap()) as usize;
            let size = if size == 1 && o + 16 <= bytes.len() {
                u64::from_be_bytes(bytes[o + 8..o + 16].try_into().unwrap()) as usize
            } else {
                size
            };
            if size < 8 {
                o = bytes.len();
                break;
            }
            o += size;
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
mod tests {
    use super::*;

    /// An MP4 box: its size, its type, `body`.
    fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((8 + body.len()) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    fn session(name: &str, duration: f64) -> Session {
        let dir = std::env::temp_dir().join(format!("vodscribe-embed-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Session { vod_id: 1, duration, dir, buffers: HashMap::new(), files: 0 }
    }

    #[test]
    fn an_init_segment_is_split_from_the_media_after_it() {
        let init = [mp4_box(b"ftyp", b"isom"), mp4_box(b"moov", b"tracks")].concat();
        let media = [mp4_box(b"moof", b"x"), mp4_box(b"mdat", b"frames")].concat();
        let both = [init.clone(), media.clone()].concat();
        assert_eq!(split_init(&both, false), (&init[..], &media[..]));
        // WebM: up to the first cluster.
        let webm = [&[0x1a, 0x45, 0xdf, 0xa3, 0x80][..], &[0x1f, 0x43, 0xb6, 0x75, 0x81, 0x00]].concat();
        assert_eq!(split_init(&webm, true).0, &webm[..5]);
    }

    #[test]
    fn sound_buffers_are_told_from_picture_ones() {
        assert!(is_sound(r#"audio/mp4; codecs="mp4a.40.2""#));
        assert!(is_sound(r#"audio/webm; codecs="opus""#));
        assert!(!is_sound(r#"video/mp4; codecs="avc1.640032""#));
        // Twitch names its sound buffer video/mp4.
        assert!(is_sound(r#"video/mp4;codecs="mp4a.40.2""#));
    }

    #[test]
    fn appends_that_follow_on_make_one_run_and_a_jump_starts_another() {
        let mut s = session("runs", 60.0);
        let init = [mp4_box(b"ftyp", b"isom"), mp4_box(b"moov", b"v")].concat();
        let seg = mp4_box(b"moof", b"m");
        let video = r#"video/mp4; codecs="avc1""#;
        // Twitch sends the same init before each segment: one run.
        assert!(s.append(1, video, true, false, &[(0.0, 2.0)], &[init.clone(), seg.clone()].concat()).unwrap());
        assert!(s.append(1, video, true, false, &[(2.0, 4.0)], &[init.clone(), seg.clone()].concat()).unwrap());
        let runs = &s.buffers[&1].runs;
        assert_eq!(runs.len(), 1);
        // What the last append named may not all be in yet: safe as of the one before.
        assert_eq!((runs[0].start, runs[0].end, runs[0].safe), (0.0, 4.0, 2.0));
        // Its file holds the init once, then both segments.
        assert_eq!(std::fs::read(&runs[0].path).unwrap(), [init.clone(), seg.clone(), seg.clone()].concat());
        // A jump ahead: a run of its own, with the init again at its start.
        assert!(s.append(1, video, false, true, &[(30.0, 32.0)], &seg).unwrap());
        let runs = &s.buffers[&1].runs;
        assert_eq!(runs.len(), 2);
        // Ending on a whole box: all of it counts at once.
        assert_eq!((runs[1].start, runs[1].end, runs[1].safe), (30.0, 32.0, 32.0));
        assert_eq!(std::fs::read(&runs[1].path).unwrap(), [init, seg].concat());
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn the_timeline_plays_where_picture_and_sound_both_are_and_waits_elsewhere() {
        let mut s = session("layout", 20.0);
        let init = [mp4_box(b"ftyp", b"isom"), mp4_box(b"moov", b"t")].concat();
        let seg = mp4_box(b"moof", b"m");
        let (video, audio) = (r#"video/mp4; codecs="avc1""#, r#"audio/mp4; codecs="mp4a.40.2""#);
        s.append(1, video, true, true, &[(0.0, 10.0)], &[init.clone(), seg.clone()].concat()).unwrap();
        s.append(2, audio, true, true, &[(0.0, 8.0)], &[init, seg].concat()).unwrap();
        let timeline = layout(&s);
        let st = timeline.stretches();
        assert_eq!(st.len(), 2);
        assert_eq!((st[0].at, st[0].length, st[0].from), (0.0, 8.0, 0.0));
        assert!(st[0].source.is_some() && st[0].sound.is_some());
        // Past the sound, it waits, to the VOD's end.
        assert_eq!((st[1].at, st[1].length), (8.0, 12.0));
        assert!(st[1].source.is_none());
        let _ = std::fs::remove_dir_all(&s.dir);
    }
}
