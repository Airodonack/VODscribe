//! The player on awkward files, each made here by ffmpeg once per test run:
//! a screen recording's long GOP (in MP4 and MPEG-TS), a variable frame
//! rate, sound alone, a rotated phone video, odd sizes, footage that
//! doesn't start at 0 (B-frames, edit lists, MPEG-TS), and files cut
//! short. For each: what the probe reads (the size, rate and length the
//! app shows), the first frame, a seek, steps, the last second played to
//! the end, and the storyboard's slots against the file's real keyframes.
//!
//! Most of the videos say their own time in each frame: sixteen bars across
//! the picture, black or white, the frame's time in 600ths of a second in
//! binary (`BARCODE`), read back from the luma the player decoded (no GPU
//! needed). Skipped where there's no ffmpeg on PATH, except where
//! VODSCRIBE_TESTS_NEED_ALL is set (CI).

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use vodscribe_player::clock::{Event, Player};
use vodscribe_player::decode::{self, Frame};
use vodscribe_player::storyboard;
use vodscribe_player::timeline::Timeline;

/// The bars' unit: a frame's time is said in 600ths of a second, a whole
/// number at 24, 25, 30, 50 and 60 fps.
const TICKS: f64 = 600.0;
/// Bars across the picture: up to 109 s of times.
const BARS: u32 = 16;
/// ffmpeg's filter drawing the bars on a yuv420p picture: bar `i` (from the
/// left) is bit `15 - i` of the frame's time in ticks, white (235) or black
/// (16), the chroma grey. After any `setpts`, so it's the time the frame is
/// stored at, relative to the file's first.
const BARCODE: &str = "geq=lum='if(mod(floor(round(T*600)/pow(2\\,15-floor(X*16/W)))\\,2)\\,235\\,16)':cb=128:cr=128";

fn ffmpeg() -> &'static Path {
    Path::new("ffmpeg")
}

/// Whether a missing ffmpeg fails the test rather than skipping it (CI).
fn needed() -> bool {
    std::env::var_os("VODSCRIBE_TESTS_NEED_ALL").is_some()
}

/// Runs ffmpeg with `args` (quiet, overwriting); whether it made its output.
fn run(args: &[&str]) -> bool {
    Command::new(ffmpeg())
        .args(["-hide_banner", "-v", "error", "-y"])
        .args(args)
        .stdin(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The kinds of file, each made once per run by `file`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    /// A screen recording: 20 s at 60 fps, a keyframe every 5 s.
    LongGop,
    /// The same, in MPEG-TS, where ffmpeg seeks without an index.
    LongGopTs,
    /// 4 s, 30 fps for the first 2 s and 60 after, in an MP4 (whose rate
    /// ffmpeg reads as the average, 45) and a Matroska file (read as 60).
    VfrMp4,
    VfrMkv,
    /// Sound alone, 2 s.
    Mp3,
    M4a,
    /// 3 s at 30 fps, 256x144 stored, a display matrix saying turn it 90°
    /// (a phone's upright video as phones write it).
    RotatedMatrix,
    /// The same picture turned for real: stored 144x256.
    Transposed,
    /// Odd sizes, 1 s at 30 fps each.
    Pixel,
    Odd17x11,
    Strip4096x8,
    Uhd,
    OddHeight,
    /// 3 s at 30 fps whose first frame is at 10 s in the file.
    Offset,
    /// 3 s at 30 fps with B-frames, in an MP4 without an edit list (its
    /// first frame shows at 0.067 s, the B-frames' delay) and in MPEG-TS
    /// (which starts at 1.4 s).
    BFramesMp4,
    BFramesTs,
    /// B-frames in an MP4 as ffmpeg writes one: an edit list, its first
    /// frames' dts below 0.
    BFramesEditList,
    /// B-frames, every time 0.5 s earlier (the first frame's pts -0.5):
    /// the edit list hides the first half second, as a phone's or an
    /// editor's trimmed file does.
    NegativeStart,
    /// 5 s at 30 fps, a keyframe a second, its last 20% cut off: a
    /// Matroska recording stopped by a crash, an MP4 with its index in
    /// front (faststart) cut the same, and one with its index at the end
    /// (ffmpeg's way), cut off with it.
    TruncatedMkv,
    TruncatedMp4,
    TruncatedNoIndex,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::LongGop => "long-gop.mp4",
            Kind::LongGopTs => "long-gop.ts",
            Kind::VfrMp4 => "vfr.mp4",
            Kind::VfrMkv => "vfr.mkv",
            Kind::Mp3 => "sound.mp3",
            Kind::M4a => "sound.m4a",
            Kind::RotatedMatrix => "rotated.mp4",
            Kind::Transposed => "transposed.mp4",
            Kind::Pixel => "1x1.mkv",
            Kind::Odd17x11 => "17x11.mkv",
            Kind::Strip4096x8 => "4096x8.mp4",
            Kind::Uhd => "3840x2160.mp4",
            Kind::OddHeight => "64x37.mkv",
            Kind::Offset => "offset.mp4",
            Kind::BFramesMp4 => "bframes.mp4",
            Kind::BFramesTs => "bframes.ts",
            Kind::BFramesEditList => "bframes-edit-list.mp4",
            Kind::NegativeStart => "negative-start.mp4",
            Kind::TruncatedMkv => "truncated.mkv",
            Kind::TruncatedMp4 => "truncated.mp4",
            Kind::TruncatedNoIndex => "no-index.mp4",
        }
    }

    /// Makes the file at `out`; whether it could.
    fn make(self, dir: &Path, out: &Path) -> bool {
        let out = out.to_str().unwrap();
        // Keyframes only where `-g` says: bars changing every frame would look
        // like scene cuts.
        let x264 = ["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-sc_threshold", "0"];
        let bars = |size: &str, rate: u32, seconds: f64| {
            format!("color=black:s={size}:r={rate}:d={seconds},format=yuv420p,{BARCODE}")
        };
        let lavfi = |graph: &str, rest: &[&str]| lavfi_to(graph, rest, Path::new(out));
        // A copy of `whole`, the last 20% cut off.
        let cut = |whole: &Path| {
            let Ok(bytes) = std::fs::read(whole) else { return false };
            std::fs::write(out, &bytes[..bytes.len() * 4 / 5]).is_ok()
        };
        match self {
            Kind::LongGop | Kind::LongGopTs => {
                lavfi(&bars("256x144", 60, 20.0), &[&x264[..], &["-g", "300", "-keyint_min", "300"]].concat())
            }
            Kind::VfrMp4 | Kind::VfrMkv => lavfi(
                // Every other frame of the first 2 s dropped, the rest kept
                // at their times: 30 fps, then 60.
                &format!("color=black:s=256x144:r=60:d=4,format=yuv420p,select='gte(t\\,2)+not(mod(n\\,2))',{BARCODE}"),
                &[&x264[..], &["-fps_mode", "vfr", "-g", "30"]].concat(),
            ),
            Kind::Mp3 => lavfi("sine=f=440:d=2", &["-c:a", "libmp3lame"]),
            Kind::M4a => lavfi("sine=f=440:d=2", &["-c:a", "aac"]),
            Kind::RotatedMatrix => {
                // `-metadata:s:v rotate=90`, the old way, writes nothing in
                // ffmpeg 7 or 9; `-display_rotation` on a copy does.
                let plain = dir.join("unrotated.mp4");
                lavfi_to(&bars("256x144", 30, 3.0), &[&x264[..], &["-g", "30"]].concat(), &plain)
                    && run(&["-display_rotation", "90", "-i", plain.to_str().unwrap(), "-c", "copy", out])
            }
            Kind::Transposed => {
                lavfi(&format!("{},transpose=clock", bars("256x144", 30, 3.0)), &[&x264[..], &["-g", "30"]].concat())
            }
            // libx264 takes only even sides in 4:2:0: odd ones are VP9.
            Kind::Pixel => lavfi("color=gray:s=1x1:r=30:d=1", &["-c:v", "libvpx-vp9", "-pix_fmt", "yuv420p"]),
            Kind::Odd17x11 => lavfi("color=gray:s=17x11:r=30:d=1", &["-c:v", "libvpx-vp9", "-pix_fmt", "yuv420p"]),
            Kind::Strip4096x8 => lavfi(&bars("4096x8", 30, 1.0), &x264),
            Kind::Uhd => lavfi(&bars("3840x2160", 30, 1.0), &x264),
            Kind::OddHeight => lavfi(&bars("64x37", 30, 1.0), &["-c:v", "libvpx-vp9", "-pix_fmt", "yuv420p"]),
            Kind::Offset => {
                lavfi(&bars("256x144", 30, 3.0), &[&x264[..], &["-g", "30", "-output_ts_offset", "10"]].concat())
            }
            Kind::BFramesMp4 => {
                lavfi(&bars("256x144", 30, 3.0), &[&x264[..], &["-g", "30", "-bf", "3", "-use_editlist", "0"]].concat())
            }
            Kind::BFramesTs | Kind::BFramesEditList => {
                lavfi(&bars("256x144", 30, 3.0), &[&x264[..], &["-g", "30", "-bf", "3"]].concat())
            }
            Kind::NegativeStart => lavfi(
                &bars("256x144", 30, 3.0),
                &[&x264[..], &["-g", "30", "-bf", "3", "-avoid_negative_ts", "disabled", "-output_ts_offset", "-0.5"]]
                    .concat(),
            ),
            Kind::TruncatedMkv | Kind::TruncatedMp4 | Kind::TruncatedNoIndex => {
                let whole = dir.join(format!("whole-{}", self.name()));
                let faststart: &[&str] = if self == Kind::TruncatedMp4 { &["-movflags", "+faststart"] } else { &[] };
                lavfi_to(&bars("256x144", 30, 5.0), &[&x264[..], &["-g", "30"], faststart].concat(), &whole)
                    && cut(&whole)
            }
        }
    }
}

/// ffmpeg's lavfi `graph` encoded with `rest` into `out`.
fn lavfi_to(graph: &str, rest: &[&str], out: &Path) -> bool {
    let mut args = vec!["-f", "lavfi", "-i", graph];
    args.extend(rest);
    args.push(out.to_str().unwrap());
    run(&args)
}

/// A kind's file, made by the first test to ask for it: None if ffmpeg couldn't.
type Made = Arc<OnceLock<Option<PathBuf>>>;

/// This run's folder of files, and each kind's file once made (None: ffmpeg
/// couldn't). Folders of runs an hour old are cleared away first
/// (a test binary has no hook to clear its own when it ends).
fn file(kind: Kind) -> Option<PathBuf> {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    static MADE: OnceLock<Mutex<HashMap<Kind, Made>>> = OnceLock::new();
    let dir = DIR.get_or_init(|| {
        let base = std::env::temp_dir();
        let hour = Duration::from_secs(3600);
        for entry in std::fs::read_dir(&base).into_iter().flatten().flatten() {
            let old = entry.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|e| e > hour));
            if old && entry.file_name().to_string_lossy().starts_with("vodscribe-media-") {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
        let dir = base.join(format!("vodscribe-media-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    });
    let once = MADE.get_or_init(Mutex::default).lock().unwrap().entry(kind).or_default().clone();
    once.get_or_init(|| {
        let out = dir.join(kind.name());
        kind.make(dir, &out).then_some(out)
    })
    .clone()
}

/// The kind's file, or None for a test to return: skipped without ffmpeg,
/// failed where it's needed.
fn file_or_skip(kind: Kind) -> Option<PathBuf> {
    let found = file(kind);
    if found.is_none() {
        assert!(!needed(), "ffmpeg couldn't make {}", kind.name());
        eprintln!("skipped: no ffmpeg (or no encoder) for {}", kind.name());
    }
    found
}

/// Which way the bars run in the picture as decoded.
#[derive(Clone, Copy, Debug)]
enum Bars {
    /// Left to right across the middle row.
    Across,
    /// Top to bottom down the middle column (a picture turned clockwise).
    Down,
}

/// The time (in the file, from its first frame) a picture's bars say, read
/// from its luma (`width` by `height`).
fn said(luma: &[u8], width: u32, height: u32, bars: Bars) -> f64 {
    let (w, h, n) = (width as usize, height as usize, BARS as usize);
    let mut ticks = 0u32;
    for i in 0..n {
        let at = match bars {
            Bars::Across => h / 2 * w + (2 * i + 1) * w / (2 * n),
            Bars::Down => (2 * i + 1) * h / (2 * n) * w + w / 2,
        };
        ticks = ticks << 1 | (luma[at] > 128) as u32;
    }
    ticks as f64 / TICKS
}

/// A storyboard's JPEG as grey, `width` by `height` (decoded by ffmpeg).
fn grey(jpeg: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut child = Command::new(ffmpeg())
        .args(["-hide_banner", "-v", "error", "-f", "image2pipe", "-c:v", "mjpeg", "-i", "pipe:0"])
        .args(["-f", "rawvideo", "-pix_fmt", "gray", "pipe:1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(jpeg).unwrap();
    let grey = child.wait_with_output().unwrap().stdout;
    assert_eq!(grey.len(), width as usize * height as usize, "a {width}x{height} JPEG");
    grey
}

/// A player of `path`, and what it tells of.
struct Opened {
    player: Player,
    events: Receiver<Event>,
}

impl Opened {
    fn file(path: &Path) -> Opened {
        Opened::with(Player::open_file(ffmpeg(), path).unwrap())
    }

    fn with(player: Player) -> Opened {
        let (tx, events) = mpsc::channel();
        player.on_frame(Box::new(move |event| {
            let _ = tx.send(event);
        }));
        Opened { player, events }
    }

    /// The frame shown once the picture is at `time` in the timeline (the
    /// seek's landing, `Player::snap`).
    fn frame_at(&self, time: f64) -> Arc<Frame> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.player.shown().is_none_or(|t| (t - time).abs() > 1e-6) {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(
                self.events.recv_timeout(left).is_ok(),
                "no frame at {time:.4} s; showing {:?}, {:?}",
                self.player.shown(),
                self.player.diagnosis()
            );
        }
        self.player.frame().expect("a frame showing")
    }

    /// Seeks to `time`: the frame shown, and where it landed.
    fn seek(&self, time: f64) -> (Arc<Frame>, f64) {
        let landed = self.player.snap(time);
        self.player.seek(time);
        (self.frame_at(landed), landed)
    }

    /// Steps `frames`: the frame shown, and where it landed.
    fn step(&self, frames: i64) -> (Arc<Frame>, f64) {
        let at = self.player.shown().unwrap();
        let fps = self.player.fps().unwrap();
        let landed = self.player.snap(at + frames as f64 / fps);
        self.player.step(frames);
        (self.frame_at(landed), landed)
    }

    /// Plays to the end: false if it hadn't ended `within` seconds after.
    fn play_to_end(&self, within: f64) -> bool {
        while self.events.try_recv().is_ok() {}
        self.player.play();
        let deadline = Instant::now() + Duration::from_secs_f64(within);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(Event::Ended) => return true,
                Ok(_) => {}
                Err(_) => return false,
            }
        }
    }
}

/// What a video with bars is to read as and play as.
struct Want {
    size: (u32, u32),
    /// The rate the probe reads, which the player steps and decodes at.
    fps: f64,
    duration: f64,
    /// None for a picture too small for bars: only its size is checked.
    bars: Option<Bars>,
    /// The times of its frames as stored, from the first (what the bars say).
    frames: Vec<f64>,
    /// Its keyframes' times, from the first frame.
    keyframes: Vec<f64>,
    /// What the bars say at the file's time 0 (where a file's first frames
    /// are hidden by its edit list).
    lead: f64,
    /// How far from the frame stored at or before the time asked the one
    /// shown may be: 0 for a constant rate (it's exactly that one).
    slack: f64,
}

impl Want {
    /// A constant `fps` from 0 for `duration` s, keyframes every `gop` s.
    fn constant(size: (u32, u32), fps: f64, duration: f64, gop: f64) -> Want {
        let n = (duration * fps).round() as usize;
        let keys = (duration / gop).ceil() as usize;
        Want {
            size,
            fps,
            duration,
            bars: Some(Bars::Across),
            frames: (0..n).map(|i| i as f64 / fps).collect(),
            keyframes: (0..keys).map(|i| i as f64 * gop).collect(),
            lead: 0.0,
            slack: 0.0,
        }
    }

    /// The time of the frame stored at or before `time`.
    fn stored_at(&self, time: f64) -> f64 {
        self.frames.iter().copied().take_while(|&t| t <= time + 1e-6).last().unwrap_or(self.frames[0])
    }

    fn keyframe_at(&self, time: f64) -> f64 {
        self.keyframes.iter().copied().take_while(|&t| t <= time + 1e-6).last().unwrap_or(self.keyframes[0])
    }

    /// That `frame`, shown at `time`, is the one stored there.
    fn check(&self, frame: &Frame, time: f64, what: &str) {
        assert_eq!((frame.width, frame.height), self.size, "{what}: the frame's size");
        let Some(bars) = self.bars else { return };
        let said = said(frame.planes().0, frame.width, frame.height, bars) - self.lead;
        let stored = self.stored_at(time);
        assert!(
            (said - stored).abs() <= self.slack + 1e-6,
            "{what}: at {time:.4} s showed the frame stored at {said:.4} s, not {stored:.4} s"
        );
    }
}

/// The probe, the first frame, a seek to `mid`, steps on and back from it,
/// the last second played to the end, and the storyboard: as `want` says.
fn plays_as_it_should(path: &Path, want: &Want, mid: f64) {
    let started = Instant::now();
    let info = decode::probe(ffmpeg(), path).unwrap();
    let picture = info.picture.expect("a picture");
    assert_eq!((picture.width, picture.height), want.size, "the probe's size");
    assert!((picture.fps - want.fps).abs() < 1e-3, "the probe's rate: {}", picture.fps);
    let duration = info.duration.expect("a length");
    assert!((duration - want.duration).abs() < 0.05, "the probe's length: {duration}");

    let opened = Opened::file(path);
    let first = opened.frame_at(0.0);
    assert_eq!(first.pts, 0.0, "the first frame's time");
    want.check(&first, 0.0, "opening");

    let (frame, landed) = opened.seek(mid);
    assert!((landed - mid).abs() < 1.0 / want.fps, "a seek to {mid} landed at {landed}");
    assert!((frame.pts - landed).abs() < 1e-6, "the frame's pts {} at {landed}", frame.pts);
    want.check(&frame, landed, &format!("a seek to {mid}"));
    let mut at = landed;
    for frames in [1, 1, -1, 5, -6, -1] {
        let (frame, landed) = opened.step(frames);
        let moved = landed - at;
        assert!((moved - frames as f64 / want.fps).abs() < 1e-6, "a step of {frames} from {at:.4} moved {moved:.4} s");
        want.check(&frame, landed, &format!("a step of {frames} to {landed:.4}"));
        at = landed;
    }

    // The last second, played out: each frame of the probe's rate shows
    // (or is dropped for being late, on a busy machine), then it ends.
    let (_, from) = opened.seek(want.duration - 1.0);
    let (shown, dropped) = opened.player.counts();
    assert!(opened.play_to_end(5.0), "playing from {from:.3} s didn't end; {:?}", opened.player.diagnosis());
    let (shown_after, dropped_after) = opened.player.counts();
    let played = (shown_after - shown) + (dropped_after - dropped);
    let due = ((want.duration - from) * want.fps).round() as u64 - 1;
    assert_eq!(played, due, "frames after the one at {from:.3} s ({} dropped)", dropped_after - dropped);
    let last = opened.player.frame().unwrap();
    want.check(&last, want.duration - 1e-3, "the end");
    drop(opened);

    storyboard_maps_to_keyframes(path, want);
    eprintln!("{}: {:.2} s", path.file_name().unwrap().to_string_lossy(), started.elapsed().as_secs_f64());
}

/// Each slot of the storyboard is a real keyframe, at or before its time,
/// its picture that keyframe's; and the frame shown for every quarter
/// second is the keyframe at or before it.
fn storyboard_maps_to_keyframes(path: &Path, want: &Want) {
    let board_path = PathBuf::from(format!("{}.sb", path.display()));
    let stop = AtomicBool::new(false);
    let board = storyboard::make(ffmpeg(), path, &board_path, &stop, |_| true).unwrap().expect("a storyboard");
    assert!(board.complete());
    let layout = board.layout;
    for slot in 0..layout.slots {
        let time = layout.time(slot);
        let pts = board.slot(slot).unwrap_or_else(|| panic!("slot {slot} at {time} s has no frame"));
        let key = want.keyframe_at(time);
        assert!((pts - key).abs() < 1e-3, "slot {slot} at {time} s: {pts}, not the keyframe at {key}");
        let jpeg = board.frame(pts).unwrap();
        // Bars a few pixels wide in the storyboard's 90 px tall frame, at least.
        if let Some(bars) = want.bars.filter(|_| want.size.0.min(want.size.1) >= 64) {
            let said = said(&grey(&jpeg, layout.width, layout.height), layout.width, layout.height, bars) - want.lead;
            assert!((said - key).abs() < 1e-3, "slot {slot}'s picture is of {said} s, not {key}");
        }
    }
    // The keyframes it holds: the one at or before each slot (where
    // keyframes are closer than the slots, some aren't among them).
    let held: Vec<f64> = (0..layout.slots).map(|s| want.keyframe_at(layout.time(s))).collect();
    let mut time = 0.0;
    while time < want.duration {
        let key = held.iter().copied().filter(|&k| k <= time + 1e-6).fold(held[0], f64::max);
        let shown = board.shown_at(time, 0.0);
        assert!(shown.is_some_and(|s| (s - key).abs() < 1e-3), "shown at {time} s: {shown:?}, not {key}");
        time += 0.25;
    }
}

#[test]
fn a_long_gop_screen_recording() {
    let Some(path) = file_or_skip(Kind::LongGop) else { return };
    plays_as_it_should(&path, &Want::constant((256, 144), 60.0, 20.0, 5.0), 7.3);
}

#[test]
fn a_long_gop_in_mpeg_ts() {
    let Some(path) = file_or_skip(Kind::LongGopTs) else { return };
    plays_as_it_should(&path, &Want::constant((256, 144), 60.0, 20.0, 5.0), 7.3);
}

#[test]
fn a_variable_rate_mp4_plays_at_its_average_rate() {
    // A design limit: the player decodes at one rate, the probe's (ffmpeg's
    // average for an MP4, 45 here), and steps by it. Through the 30 fps
    // part a frame is shown twice or three times, through the 60 fps part
    // every fourth is skipped; each shown is within half a step of its
    // stored time, never more. A 2 s per-frame index would need the
    // frames' own pts from ffmpeg, and stepping by them.
    let Some(path) = file_or_skip(Kind::VfrMp4) else { return };
    plays_as_it_should(&path, &vfr(45.0), 2.7);
}

#[test]
fn a_variable_rate_mkv_plays_at_its_fastest_rate() {
    // Matroska has no average: ffmpeg reads the rate as 60, the fastest
    // part's, so each 30 fps frame shows twice and every one is shown.
    let Some(path) = file_or_skip(Kind::VfrMkv) else { return };
    plays_as_it_should(&path, &vfr(60.0), 2.7);
}

/// `Kind::VfrMp4`'s and `VfrMkv`'s frames, read at `fps`.
fn vfr(fps: f64) -> Want {
    let mut want = Want::constant((256, 144), fps, 4.0, 1.0);
    want.frames = (0..60).map(|i| i as f64 / 30.0).chain((0..120).map(|i| 2.0 + i as f64 / 60.0)).collect();
    // `-g 30`: a keyframe each 30 frames, a second apart, then half.
    want.keyframes = vec![0.0, 1.0, 2.0, 2.5, 3.0, 3.5];
    want.slack = 1.5 / fps;
    want
}

#[test]
fn a_rotated_phone_video_shows_as_stored() {
    // As the storyboard's plan chose (-noautorotate everywhere the player
    // and storyboards read): a display matrix is not followed, so the
    // probe's size and the picture are the stored 256x144, on its side.
    let Some(path) = file_or_skip(Kind::RotatedMatrix) else { return };
    plays_as_it_should(&path, &Want::constant((256, 144), 30.0, 3.0, 1.0), 1.5);
}

#[test]
fn a_video_turned_for_real_shows_upright() {
    let Some(path) = file_or_skip(Kind::Transposed) else { return };
    let mut want = Want::constant((144, 256), 30.0, 3.0, 1.0);
    want.bars = Some(Bars::Down);
    plays_as_it_should(&path, &want, 1.5);
}

#[test]
fn a_one_pixel_video() {
    let Some(path) = file_or_skip(Kind::Pixel) else { return };
    let mut want = Want::constant((1, 1), 30.0, 1.0, 1.0);
    want.bars = None;
    plays_as_it_should(&path, &want, 0.4);
}

#[test]
fn an_odd_sized_video() {
    let Some(path) = file_or_skip(Kind::Odd17x11) else { return };
    let mut want = Want::constant((17, 11), 30.0, 1.0, 1.0);
    want.bars = None;
    plays_as_it_should(&path, &want, 0.4);
}

#[test]
fn an_odd_height_in_yuv420p() {
    let Some(path) = file_or_skip(Kind::OddHeight) else { return };
    plays_as_it_should(&path, &Want::constant((64, 37), 30.0, 1.0, 1.0), 0.4);
}

#[test]
fn a_strip_4096_by_8() {
    let Some(path) = file_or_skip(Kind::Strip4096x8) else { return };
    plays_as_it_should(&path, &Want::constant((4096, 8), 30.0, 1.0, 10.0), 0.4);
}

#[test]
fn a_short_4k_video() {
    let Some(path) = file_or_skip(Kind::Uhd) else { return };
    plays_as_it_should(&path, &Want::constant((3840, 2160), 30.0, 1.0, 10.0), 0.4);
}

#[test]
fn a_file_starting_at_10_s() {
    let Some(path) = file_or_skip(Kind::Offset) else { return };
    plays_as_it_should(&path, &Want::constant((256, 144), 30.0, 3.0, 1.0), 1.5);
}

#[test]
fn b_frames_in_an_mp4_without_an_edit_list() {
    let Some(path) = file_or_skip(Kind::BFramesMp4) else { return };
    plays_as_it_should(&path, &Want::constant((256, 144), 30.0, 3.0, 1.0), 1.5);
}

#[test]
fn b_frames_in_mpeg_ts_starting_at_1_4_s() {
    let Some(path) = file_or_skip(Kind::BFramesTs) else { return };
    plays_as_it_should(&path, &Want::constant((256, 144), 30.0, 3.0, 1.0), 1.5);
}

/// Sound alone: the probe finds no picture, and the player opened on the
/// file by itself refuses it (`Player::open_file`, what the app's player
/// opens a VOD with): such a VOD plays in the page's own media element.
/// In a timeline (an edit's piece of sound alone), it plays: no frame
/// shows, the time runs, and it ends.
fn sound_alone(kind: Kind) {
    let Some(path) = file_or_skip(kind) else { return };
    let info = decode::probe(ffmpeg(), &path).unwrap();
    assert!(info.picture.is_none() && info.audio, "{info:?}");
    let duration = info.duration.expect("a length");
    // MP3 and AAC pad their first and last frames a little.
    assert!((duration - 2.0).abs() < 0.1, "the probe's length: {duration}");
    let Err(refused) = Player::open_file(ffmpeg(), &path) else { panic!("opened sound alone") };
    assert!(refused.to_string().contains("no picture"), "{refused:#}");

    let opened = Opened::with(Player::open(ffmpeg(), Timeline::file(&path, duration)).unwrap());
    assert!(opened.play_to_end(duration + 3.0), "didn't end; {:?}", opened.player.diagnosis());
    assert!(opened.player.frame().is_none() && opened.player.shown().is_none(), "a frame of sound alone");
    assert!((opened.player.time() - duration).abs() < 1e-6, "ended at {}", opened.player.time());
    drop(opened);

    let stop = AtomicBool::new(false);
    let board = PathBuf::from(format!("{}.sb", path.display()));
    assert!(storyboard::make(ffmpeg(), &path, &board, &stop, |_| true).unwrap().is_none(), "a storyboard of sound");
}

#[test]
fn an_mp3() {
    sound_alone(Kind::Mp3);
}

#[test]
fn an_m4a() {
    sound_alone(Kind::M4a);
}

/// The time the bars of the last frame ffmpeg can decode of `path` say:
/// how far a file cut short goes.
fn last_frame(path: &Path) -> f64 {
    let out = Command::new(ffmpeg())
        .args(["-hide_banner", "-v", "quiet", "-i"])
        .arg(path)
        .args(["-map", "0:v:0", "-f", "rawvideo", "-pix_fmt", "gray", "pipe:1"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let size = 256 * 144;
    assert!(out.stdout.len() >= size, "no frame of {} decodes", path.display());
    let last = &out.stdout[(out.stdout.len() / size - 1) * size..][..size];
    said(last, 256, 144, Bars::Across)
}

/// A file cut short opens with the length its header says, shows what's
/// there, and played into the cut, shows its last frame there and runs on
/// to the length said, then ends.
fn cut_short(kind: Kind) {
    let Some(path) = file_or_skip(kind) else { return };
    let reaches = last_frame(&path);
    assert!((3.0..4.9).contains(&reaches), "the cut file goes to {reaches} s");
    let info = decode::probe(ffmpeg(), &path).unwrap();
    let duration = info.duration.unwrap();
    assert!((duration - 5.0).abs() < 0.05, "the length its header says: {duration}");
    let want = Want::constant((256, 144), 30.0, 5.0, 1.0);
    let opened = Opened::file(&path);
    want.check(&opened.frame_at(0.0), 0.0, "opening");
    let (frame, landed) = opened.seek(reaches - 1.0);
    want.check(&frame, landed, "a seek before the cut");
    let (frame, landed) = opened.step(1);
    want.check(&frame, landed, "a step before the cut");
    assert!(opened.play_to_end(duration - landed + 3.0), "didn't end; {:?}", opened.player.diagnosis());
    let last = said(opened.player.frame().unwrap().planes().0, 256, 144, Bars::Across);
    assert!((last - reaches).abs() < 1e-6, "ended showing {last} s, not the last frame there, {reaches} s");
    assert!((opened.player.time() - duration).abs() < 1e-6, "ended at {}", opened.player.time());
    // A seek past the cut: no frame there, so the one showing stays,
    // and the time stands where it was sent. (A limit: the picture isn't
    // of that time, and the page isn't told. The file's own length would
    // be read only by decoding it to its end.)
    let shown = opened.player.shown();
    opened.player.seek(duration - 0.5);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(opened.player.shown(), shown, "a frame shown past the cut");
    assert!((opened.player.time() - opened.player.snap(duration - 0.5)).abs() < 1e-6);
    drop(opened);

    // The storyboard: a slot past the cut has the last keyframe there
    // (Matroska, whose index is cut off, is sought by reading) or none (an
    // MP4's index sends the seek to a keyframe that isn't there); either
    // way the thumbnail past it is that last keyframe.
    let stop = AtomicBool::new(false);
    let board_path = PathBuf::from(format!("{}.sb", path.display()));
    let board = storyboard::make(ffmpeg(), &path, &board_path, &stop, |_| true).unwrap().expect("a storyboard");
    assert!(board.complete());
    let last_key = reaches.floor();
    for slot in 0..board.layout.slots {
        let time = board.layout.time(slot);
        let got = board.slot(slot);
        let ok = if time <= last_key { got == Some(time.floor()) } else { got.is_none_or(|k| k == last_key) };
        assert!(ok, "slot {slot} at {time} s: {got:?}");
    }
    let held: Vec<f64> = (0..board.layout.slots).filter_map(|s| board.slot(s)).collect();
    let mut time = 0.0;
    while time < duration {
        let key = held.iter().copied().filter(|&k| k <= time).fold(0.0, f64::max);
        assert_eq!(board.shown_at(time, 0.0), Some(key), "shown at {time} s");
        time += 0.25;
    }
}

#[test]
fn a_matroska_recording_cut_short() {
    cut_short(Kind::TruncatedMkv);
}

#[test]
fn an_mp4_with_its_index_in_front_cut_short() {
    cut_short(Kind::TruncatedMp4);
}

/// An MP4 whose index (at its end) was cut off can't be read at all: the
/// probe says so, and it isn't opened.
#[test]
fn an_mp4_cut_before_its_index() {
    let Some(path) = file_or_skip(Kind::TruncatedNoIndex) else { return };
    let refused = decode::probe(ffmpeg(), &path).expect_err("refused");
    assert!(refused.to_string().contains("has no picture and no sound"), "{refused:#}");
    assert!(Player::open_file(ffmpeg(), &path).is_err());
}

#[test]
fn b_frames_in_an_mp4_with_an_edit_list() {
    let Some(path) = file_or_skip(Kind::BFramesEditList) else { return };
    plays_as_it_should(&path, &Want::constant((256, 144), 30.0, 3.0, 1.0), 1.5);
}

#[test]
fn an_mp4_whose_first_half_second_is_hidden() {
    let Some(path) = file_or_skip(Kind::NegativeStart) else { return };
    // 2.5 s shown, from the bars' 0.5 s; the keyframes (the bars' 0, 1
    // and 2 s) at -0.5, 0.5 and 1.5 in the file. The first is hidden: the
    // picture at 0 is decoded from it, but ffmpeg gives no frame of it to
    // the storyboard, whose first is the one at 0.5. A limit: the
    // thumbnail over the first half second is of 0.5 s, a little ahead.
    let mut want = Want::constant((256, 144), 30.0, 2.5, 1.0);
    want.lead = 0.5;
    want.frames = (0..90).map(|i| i as f64 / 30.0 - 0.5).collect();
    want.keyframes = vec![0.5, 1.5];
    plays_as_it_should(&path, &want, 1.2);
}
