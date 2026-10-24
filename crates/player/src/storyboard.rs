//! Storyboards: small frames of a file every few seconds, for the
//! timeline's thumbnail while hovering or dragging and for the library's
//! picture of a file. Made by the ffmpeg the app ships, as `decode` reads
//! frames, but only keyframes: each slot (every `Layout::interval` seconds)
//! takes the keyframe at or before it, which costs a decode of one picture,
//! not of everything up to the time.
//!
//! The slots are filled halves first (½, then ¼ and ¾, then the eighths…),
//! so a frame near anywhere exists within a second of starting, and the rest
//! fill in between. Each is appended to one file per source as a record, so
//! a storyboard made partway (the app quit, the job stopped) carries on from
//! where it was, and a crash loses at most the last, torn record.
//!
//! The file: a header (`MAGIC`, `VERSION`, the source's `Fingerprint`, its
//! `Layout`), then records `[slot u32][pts f64][len u32][jpeg]`, little-endian.
//! A slot whose keyframe is stored already (keyframes further apart than the
//! interval) is a record with no bytes and that keyframe's pts; one with no
//! frame at all (past the last), no bytes and a NaN pts.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result, bail};
use vodscribe_core::local::Picture;
use vodscribe_core::tools;

use crate::decode;

/// A frame's height in pixels: the timeline's thumbnail is 44 CSS pixels
/// tall (88 at twice the density), the library row's 45.
const HEIGHT: u32 = 90;
/// Slots are at least this far apart, in seconds…
const SHORTEST_INTERVAL: f64 = 2.0;
/// …and at most this many: past two hours they spread out, so a file's
/// storyboard stays near 15 MB however long it is.
const MOST_SLOTS: u32 = 3600;
/// Slots fetched by one ffmpeg: each its own input, sought to its time. Eight
/// was about two and a half times as fast as one ffmpeg per frame (1080p
/// H.264: 32 ms a frame against 74), at 210 MB of memory against 64; sixteen
/// gained little more (26 ms) for 340 MB.
const BATCH: usize = 8;
const JPEG_QUALITY: &str = "5";

const MAGIC: [u8; 4] = *b"VSSB";
const VERSION: u32 = 1;
/// A record's header: slot, pts, length.
const RECORD: u64 = 4 + 8 + 4;

/// What a source file was when its storyboard was made: a different one is
/// made again.
#[derive(Clone, Debug, PartialEq)]
pub struct Fingerprint {
    pub path: String,
    pub size: u64,
    /// Nanoseconds since 1970 it was last changed.
    pub modified: u64,
}

impl Fingerprint {
    pub fn of(path: &Path) -> Result<Fingerprint> {
        let meta = std::fs::metadata(path).with_context(|| format!("read {}", tools::shown(path)))?;
        let modified = meta.modified().ok().and_then(|m| m.duration_since(UNIX_EPOCH).ok());
        Ok(Fingerprint {
            path: path.to_string_lossy().into_owned(),
            size: meta.len(),
            modified: modified.map_or(0, |d| d.as_nanos() as u64),
        })
    }
}

/// A storyboard's frame size and slots.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    /// Seconds between slots.
    pub interval: f64,
    pub slots: u32,
    /// The file's length in seconds.
    pub duration: f64,
}

impl Layout {
    /// For a picture of `picture`'s shape (as stored: the player doesn't
    /// turn rotated phone videos either) and `duration` seconds.
    pub fn of(picture: Picture, duration: f64) -> Layout {
        let width = ((HEIGHT as f64 * picture.width as f64 / picture.height.max(1) as f64 / 2.0).round() as u32 * 2)
            .clamp(2, HEIGHT * 8);
        let duration = duration.max(0.0);
        let interval = SHORTEST_INTERVAL.max(duration / MOST_SLOTS as f64);
        let slots = ((duration / interval).ceil() as u32).clamp(1, MOST_SLOTS);
        Layout { width, height: HEIGHT, interval, slots, duration }
    }

    /// Seconds into the file of slot `slot`.
    pub fn time(&self, slot: u32) -> f64 {
        slot as f64 * self.interval
    }

    /// The slot for the library's picture: an eighth of the way in (made
    /// fourth, within the first batch).
    pub fn picture_slot(&self) -> u32 {
        order(self.slots).get(3).copied().unwrap_or(0).min(self.slots - 1)
    }
}

/// Slots `0..slots` in the order they're made: the middle, then the
/// quarters, then the eighths, and so on down, so wherever the pointer is
/// a frame not far off is made early.
fn order(slots: u32) -> Vec<u32> {
    let mut seen = vec![false; slots as usize];
    let mut out = Vec::with_capacity(slots as usize);
    let mut parts = 2u64;
    while out.len() < slots as usize && parts <= 4 * slots as u64 {
        for j in (1..parts).step_by(2) {
            let slot = (j * slots as u64 / parts) as usize;
            if !seen[slot] {
                seen[slot] = true;
                out.push(slot as u32);
            }
        }
        parts *= 2;
    }
    out.extend((0..slots).filter(|&s| !seen[s as usize]));
    out
}

/// A frame stored: where its bytes are.
#[derive(Clone, Copy, Debug)]
struct Stored {
    pts: f64,
    offset: u64,
    len: u32,
}

/// A storyboard file as read: what's in it so far.
#[derive(Debug)]
pub struct Storyboard {
    path: PathBuf,
    pub fingerprint: Fingerprint,
    pub layout: Layout,
    /// Each slot's frame (its pts), once made: None for one not made, NaN for one with none.
    slots: Vec<Option<f64>>,
    /// The frames, by pts.
    frames: Vec<Stored>,
    /// Where the next record starts: the file is read up to here.
    end: u64,
}

impl Storyboard {
    /// Reads the storyboard at `path`, as far as its records are whole.
    pub fn open(path: &Path) -> Result<Storyboard> {
        let mut reader = BufReader::new(File::open(path)?);
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)?;
        if magic != MAGIC || read_u32(&mut reader)? != VERSION {
            bail!("not a storyboard of this version");
        }
        let len = read_u32(&mut reader)? as usize;
        if len > 64 * 1024 {
            bail!("not a storyboard");
        }
        let mut name = vec![0u8; len];
        reader.read_exact(&mut name)?;
        let fingerprint = Fingerprint {
            path: String::from_utf8_lossy(&name).into_owned(),
            size: read_u64(&mut reader)?,
            modified: read_u64(&mut reader)?,
        };
        let (width, height) = (read_u32(&mut reader)?, read_u32(&mut reader)?);
        let interval = read_f64(&mut reader)?;
        let slots = read_u32(&mut reader)?;
        let duration = read_f64(&mut reader)?;
        if slots == 0 || slots > MOST_SLOTS || interval.is_nan() || interval <= 0.0 {
            bail!("not a storyboard");
        }
        let layout = Layout { width, height, interval, slots, duration };
        let end = reader.stream_position()?;
        let mut board = Storyboard {
            path: path.to_path_buf(),
            fingerprint,
            layout,
            slots: vec![None; slots as usize],
            frames: Vec::new(),
            end,
        };
        board.read_records(reader)?;
        Ok(board)
    }

    /// Reads what's been appended since it was read (it's being made).
    pub fn refresh(&mut self) -> Result<()> {
        let reader = BufReader::new(File::open(&self.path)?);
        self.read_records(reader)
    }

    fn read_records(&mut self, mut reader: BufReader<File>) -> Result<()> {
        let size = reader.get_ref().metadata()?.len();
        reader.seek(SeekFrom::Start(self.end))?;
        while self.end + RECORD <= size {
            let slot = read_u32(&mut reader)?;
            let pts = read_f64(&mut reader)?;
            let len = read_u32(&mut reader)?;
            if self.end + RECORD + len as u64 > size || slot >= self.layout.slots {
                // Torn: only the writer cuts it.
                break;
            }
            self.note(slot, pts, self.end + RECORD, len);
            reader.seek_relative(len as i64)?;
            self.end += RECORD + len as u64;
        }
        Ok(())
    }

    fn note(&mut self, slot: u32, pts: f64, offset: u64, len: u32) {
        self.slots[slot as usize] = Some(pts);
        if len > 0 {
            let at = self.frames.partition_point(|f| f.pts < pts);
            if self.frames.get(at).is_none_or(|f| f.pts != pts) {
                self.frames.insert(at, Stored { pts, offset, len });
            }
        }
    }

    /// Whether every slot is made.
    pub fn complete(&self) -> bool {
        self.slots.iter().all(Option::is_some)
    }

    /// How many slots are made.
    pub fn made(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// Whether it was made of `fingerprint`'s file as it is.
    pub fn of(&self, fingerprint: &Fingerprint) -> bool {
        self.fingerprint == *fingerprint
    }

    /// The pts of the frame stored nearest `time` (seconds into the file), if any is.
    pub fn nearest(&self, time: f64) -> Option<f64> {
        let at = self.frames.partition_point(|f| f.pts < time);
        let after = self.frames.get(at);
        let before = at.checked_sub(1).and_then(|i| self.frames.get(i));
        match (before, after) {
            (Some(b), Some(a)) => Some(if time - b.pts <= a.pts - time { b.pts } else { a.pts }),
            (Some(f), None) | (None, Some(f)) => Some(f.pts),
            (None, None) => None,
        }
    }

    /// The pts of the frame to show for `time` (seconds into the file): the
    /// keyframe stored at or before it, however far back, as the picture
    /// there is decoded from it (the first one, for a time before it). That
    /// keyframe is known once the slots either side of `time` are made;
    /// before then, while it's being made, the frame stored nearest within
    /// `near` seconds (or the interval) stands in, if there is one.
    pub fn shown_at(&self, time: f64, near: f64) -> Option<f64> {
        let slots = self.layout.slots as usize;
        let slot = ((time / self.layout.interval).floor().max(0.0) as usize).min(slots - 1);
        let made = |s: usize| s >= slots || self.slots[s].is_some();
        if made(slot) && made(slot + 1) {
            let at = self.frames.partition_point(|f| f.pts <= time);
            return self.frames.get(at.saturating_sub(1)).map(|f| f.pts);
        }
        let pts = self.nearest(time)?;
        ((pts - time).abs() <= self.layout.interval.max(near)).then_some(pts)
    }

    /// The pts of slot `slot`'s frame, once it's made and has one.
    pub fn slot(&self, slot: u32) -> Option<f64> {
        self.slots.get(slot as usize).copied().flatten().filter(|p| !p.is_nan())
    }

    /// The JPEG of the frame stored at `pts` exactly.
    pub fn frame(&self, pts: f64) -> Result<Vec<u8>> {
        let at = self.frames.partition_point(|f| f.pts < pts);
        let stored = self.frames.get(at).filter(|f| f.pts == pts).context("no frame there")?;
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(stored.offset))?;
        let mut bytes = vec![0u8; stored.len as usize];
        file.read_exact(&mut bytes)?;
        Ok(bytes)
    }
}

fn read_u32(r: &mut impl Read) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64(r: &mut impl Read) -> std::io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn read_f64(r: &mut impl Read) -> std::io::Result<f64> {
    Ok(f64::from_bits(read_u64(r)?))
}

/// A storyboard being made: its file open to append to.
struct Writer {
    file: File,
    board: Storyboard,
}

impl Writer {
    /// The storyboard at `path` of `fingerprint`'s file to carry on with,
    /// a torn last record cut off; or a new one in its place if there's
    /// none, or it's of the file as it was, or of another `layout`.
    fn open(path: &Path, fingerprint: &Fingerprint, layout: Layout) -> Result<Writer> {
        if let Ok(board) = Storyboard::open(path)
            && board.of(fingerprint)
            && board.layout == layout
        {
            let file = File::options().write(true).open(path)?;
            file.set_len(board.end)?;
            let mut writer = Writer { file, board };
            writer.file.seek(SeekFrom::End(0))?;
            return Ok(writer);
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut head = Vec::new();
        head.extend_from_slice(&MAGIC);
        head.extend_from_slice(&VERSION.to_le_bytes());
        head.extend_from_slice(&(fingerprint.path.len() as u32).to_le_bytes());
        head.extend_from_slice(fingerprint.path.as_bytes());
        head.extend_from_slice(&fingerprint.size.to_le_bytes());
        head.extend_from_slice(&fingerprint.modified.to_le_bytes());
        head.extend_from_slice(&layout.width.to_le_bytes());
        head.extend_from_slice(&layout.height.to_le_bytes());
        head.extend_from_slice(&layout.interval.to_le_bytes());
        head.extend_from_slice(&layout.slots.to_le_bytes());
        head.extend_from_slice(&layout.duration.to_le_bytes());
        // Written aside and renamed, so a reader never finds half a header.
        let aside = path.with_extension("sb-new");
        std::fs::write(&aside, &head)?;
        std::fs::rename(&aside, path)?;
        let board = Storyboard::open(path)?;
        let mut file = File::options().write(true).open(path)?;
        file.seek(SeekFrom::End(0))?;
        Ok(Writer { file, board })
    }

    /// Records slot `slot`'s frame: `jpeg` at `pts`, or None if it has
    /// none. A keyframe stored already isn't stored twice.
    fn add(&mut self, slot: u32, frame: Option<(f64, &[u8])>) -> Result<()> {
        let (pts, jpeg) = match frame {
            Some((pts, _)) if self.board.frames.iter().any(|f| f.pts == pts) => (pts, &[][..]),
            Some((pts, jpeg)) => (pts, jpeg),
            None => (f64::NAN, &[][..]),
        };
        let mut record = Vec::with_capacity(RECORD as usize + jpeg.len());
        record.extend_from_slice(&slot.to_le_bytes());
        record.extend_from_slice(&pts.to_le_bytes());
        record.extend_from_slice(&(jpeg.len() as u32).to_le_bytes());
        record.extend_from_slice(jpeg);
        // One write, so a reader sees it whole or not at all (as torn).
        self.file.write_all(&record)?;
        let offset = self.board.end + RECORD;
        self.board.note(slot, pts, offset, jpeg.len() as u32);
        self.board.end += record.len() as u64;
        Ok(())
    }
}

/// Makes (or carries on making) the storyboard of `source` in `file`.
/// After each batch of slots, `between` is shown it so far, and says
/// whether to go on (it may wait first: the player's playing). None if
/// the source has no picture or no known length; stopped by `stop` or
/// `between`, `Canceled`.
pub fn make(
    ffmpeg: &Path,
    source: &Path,
    file: &Path,
    stop: &AtomicBool,
    mut between: impl FnMut(&Storyboard) -> bool,
) -> Result<Option<Storyboard>> {
    let fingerprint = Fingerprint::of(source)?;
    // Done already: no probe needed.
    if let Ok(board) = Storyboard::open(file)
        && board.of(&fingerprint)
        && board.complete()
    {
        return Ok(Some(board));
    }
    let info = decode::probe(ffmpeg, source)?;
    let (Some(picture), Some(duration)) = (info.picture, info.duration.filter(|d| *d > 0.0)) else {
        return Ok(None);
    };
    let layout = Layout::of(picture, duration);
    let mut writer = Writer::open(file, &fingerprint, layout)?;
    let waiting: Vec<u32> =
        order(layout.slots).into_iter().filter(|&s| writer.board.slots[s as usize].is_none()).collect();
    for batch in waiting.chunks(BATCH) {
        if stop.load(Ordering::Relaxed) {
            return Err(vodscribe_core::Canceled.into());
        }
        let times: Vec<f64> = batch.iter().map(|&s| layout.time(s)).collect();
        let frames = match grab(ffmpeg, source, layout, &times) {
            Ok(frames) => frames,
            Err(e) => {
                // One seek point ffmpeg can't read spoils the batch: each on
                // its own, unless the file changed under it.
                if Fingerprint::of(source).ok().as_ref() != Some(&fingerprint) {
                    return Err(e.context("the file changed while its storyboard was made"));
                }
                times
                    .iter()
                    .map(|&t| grab(ffmpeg, source, layout, &[t]).ok().and_then(|mut f| f.pop().flatten()))
                    .collect()
            }
        };
        for (&slot, frame) in batch.iter().zip(&frames) {
            writer.add(slot, frame.as_ref().map(|(pts, jpeg)| (*pts, jpeg.as_slice())))?;
        }
        if !between(&writer.board) {
            return Err(vodscribe_core::Canceled.into());
        }
    }
    Ok(Some(writer.board))
}

/// A frame read: its pts, and its JPEG.
type Grabbed = (f64, Vec<u8>);

/// The keyframe at or before each of `times` (seconds into `source`),
/// `layout`'s size, as a JPEG with its pts: one ffmpeg, an input sought to
/// each time, each input's first frame passed through `showinfo` (which
/// says its pts) and the frames put in order of input. In MPEG-TS, where a
/// seek lands on the keyframe after the time (`decode::SEEK_BACK`), each
/// input is sought that much earlier and its last keyframe up to the time
/// taken; none there (keyframes further apart), the slot has none.
fn grab(ffmpeg: &Path, source: &Path, layout: Layout, times: &[f64]) -> Result<Vec<Option<Grabbed>>> {
    let late = decode::seeks_late(source);
    let seeks: Vec<f64> =
        times.iter().map(|t| if late { (t - decode::SEEK_BACK).max(0.0) } else { t.max(0.0) }).collect();
    let mut cmd = tools::command(ffmpeg);
    cmd.args(["-hide_banner", "-nostdin", "-loglevel", "info"]);
    for seek in &seeks {
        tools::ffmpeg_allow_for(&mut cmd, tools::FFMPEG_PICTURE, source)
            .args(["-threads", "1", "-skip_frame", "nokey", "-noautorotate", "-noaccurate_seek"])
            .args(["-ss", &format!("{seek:.6}"), "-i"])
            .arg(source);
    }
    let until: Option<Vec<f64>> = late.then(|| times.iter().zip(&seeks).map(|(t, seek)| t.max(0.0) - seek).collect());
    cmd.args(["-filter_complex", &graph(layout, times.len(), until.as_deref())])
        .args(["-map", "[out]", "-fps_mode", "passthrough", "-filter_complex_threads", "1", "-threads", "1"])
        .args(["-c:v", "mjpeg", "-q:v", JPEG_QUALITY, "-f", "image2pipe", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = cmd.spawn().with_context(|| format!("run {}", tools::shown(ffmpeg)))?;
    let _tracked = tools::track(&child);
    let out = child.wait_with_output()?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        let last = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        bail!("ffmpeg couldn't read its frames: {last}");
    }
    let shown = showinfo(&String::from_utf8_lossy(&out.stderr), &seeks, filters(late));
    let jpegs = split_jpegs(&out.stdout);
    let found: Vec<usize> = (0..times.len()).filter(|&i| shown[i].is_some()).collect();
    if found.len() != jpegs.len() {
        bail!("ffmpeg gave {} frames for {} found", jpegs.len(), found.len());
    }
    let mut frames = vec![None; times.len()];
    for (i, jpeg) in found.into_iter().zip(jpegs) {
        frames[i] = shown[i].map(|pts| (pts, jpeg.to_vec()));
    }
    Ok(frames)
}

/// How many filters `graph` gives each input: its showinfo is the second.
fn filters(late: bool) -> usize {
    if late { 6 } else { 4 }
}

/// The filter graph for `inputs` inputs: each one's first frame shown
/// (`showinfo`), scaled, and stamped with its input's number so
/// `interleave` puts them out in that order. With `until`, each input's
/// frames up to `until[k]` seconds from where it was sought are shown
/// instead and the last taken (`reverse` gives it the first one's pts:
/// shown before). `filters` to each, so the showinfo of input `k` is the
/// graph's filter `filters * k + 1`.
fn graph(layout: Layout, inputs: usize, until: Option<&[f64]>) -> String {
    let mut graph = String::new();
    for k in 0..inputs {
        // Up to a millisecond past, so a keyframe at the time itself is taken.
        let pick = match until {
            Some(until) => format!("trim=end={:.6},showinfo,reverse,trim=end_frame=1", until[k] + 1e-3),
            None => "trim=end_frame=1,showinfo".to_string(),
        };
        graph += &format!("[{k}:v:0]{pick},scale={}:{},setpts={k}/TB[v{k}];", layout.width, layout.height);
    }
    for k in 0..inputs {
        graph += &format!("[v{k}]");
    }
    graph + &format!("interleave=nb_inputs={inputs}[out]")
}

/// Each input's frame's pts in the file, from what `showinfo` said: its
/// time from where it was sought to (`seeks`), as ffmpeg keeps a sought
/// input's times from there, with `filters` filters to each input. Where
/// an input's showinfo tells of more than one, the last (`graph`'s `until`).
fn showinfo(said: &str, seeks: &[f64], filters: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; seeks.len()];
    for line in said.lines() {
        let Some(rest) = line.split("Parsed_showinfo_").nth(1) else { continue };
        let Some(n) = rest.split(|c: char| !c.is_ascii_digit()).next().and_then(|n| n.parse::<usize>().ok()) else {
            continue;
        };
        let Some(pts) = rest.split("pts_time:").nth(1).and_then(|p| p.split_whitespace().next()?.parse::<f64>().ok())
        else {
            continue;
        };
        if !rest.contains(" n:") || n % filters != 1 {
            continue;
        }
        let k = n / filters;
        if k < seeks.len() {
            // Kept to the microsecond: the same keyframe from two seeks is the same pts.
            let pts = ((seeks[k].max(0.0) + pts) * 1e6).round() / 1e6;
            out[k] = Some(out[k].map_or(pts, |p: f64| p.max(pts)));
        }
    }
    out
}

/// The JPEGs one after another in `bytes`, each from its start-of-image
/// marker to its end-of-image one, read by its segments (an end marker's
/// bytes can't be inside the compressed data, where 0xFF is always
/// followed by 0 or a restart marker).
fn split_jpegs(bytes: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut at = 0;
    'images: while at + 1 < bytes.len() {
        if bytes[at] != 0xFF || bytes[at + 1] != 0xD8 {
            break;
        }
        let start = at;
        let mut i = at + 2;
        loop {
            // The next marker: 0xFF (any fill), then its code.
            while i < bytes.len() && bytes[i] != 0xFF {
                i += 1;
            }
            while i < bytes.len() && bytes[i] == 0xFF {
                i += 1;
            }
            let Some(&code) = bytes.get(i) else { break 'images };
            i += 1;
            match code {
                0xD9 => {
                    out.push(&bytes[start..i]);
                    at = i;
                    continue 'images;
                }
                0x01 | 0xD0..=0xD7 => {}
                _ => {
                    let Some(len) = bytes.get(i..i + 2).map(|b| u16::from_be_bytes([b[0], b[1]]) as usize) else {
                        break 'images;
                    };
                    i += len;
                    if code == 0xDA {
                        // The compressed data, to the next marker that isn't a stuffed byte or a restart.
                        while i + 1 < bytes.len()
                            && !(bytes[i] == 0xFF && bytes[i + 1] != 0 && !(0xD0..=0xD7).contains(&bytes[i + 1]))
                        {
                            i += 1;
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vodscribe_core::local::Colour;

    /// A fresh folder for a test, removed when it's dropped.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("vodscribe-sb-{name}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fingerprint() -> Fingerprint {
        Fingerprint { path: "/videos/a.mp4".into(), size: 1234, modified: 5678 }
    }

    fn layout(slots: u32) -> Layout {
        Layout { width: 160, height: 90, interval: 2.0, slots, duration: slots as f64 * 2.0 }
    }

    /// A JPEG's shape, enough for `split_jpegs`: start, a segment, the
    /// compressed data (with a stuffed 0xFF and a restart), end.
    fn jpeg(fill: u8) -> Vec<u8> {
        vec![
            0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x04, fill, fill, 0xFF, 0xDA, 0x00, 0x02, fill, 0xFF, 0x00, 0xFF, 0xD3, fill,
            0xFF, 0xD9,
        ]
    }

    #[test]
    fn slots_are_made_halves_first() {
        assert_eq!(order(16)[..7], [8, 4, 12, 2, 6, 10, 14]);
        for n in [1, 2, 3, 5, 16, 300, 3600] {
            let mut all = order(n);
            assert_eq!(all.len(), n as usize, "each slot once");
            all.sort();
            assert_eq!(all, (0..n).collect::<Vec<_>>());
        }
        assert_eq!(order(1), [0]);
    }

    #[test]
    fn slots_are_two_seconds_apart_up_to_two_hours_then_spread() {
        let picture = Picture { width: 1920, height: 1080, fps: 30.0, colour: Colour::UNTAGGED };
        let hour = Layout::of(picture, 3600.0);
        assert_eq!((hour.width, hour.height, hour.interval, hour.slots), (160, 90, 2.0, 1800));
        let five = Layout::of(picture, 5.0 * 3600.0);
        assert_eq!((five.interval, five.slots), (5.0, 3600));
        assert_eq!(Layout::of(picture, 0.5).slots, 1);
        // Stored as stored: a phone's upright video is narrow.
        assert_eq!(
            Layout::of(Picture { width: 1080, height: 1920, fps: 30.0, colour: Colour::UNTAGGED }, 60.0).width,
            50
        );
        // An eighth of the way in: slot 225 of 1800, at 450 s.
        assert_eq!(hour.time(hour.picture_slot()), 450.0);
    }

    #[test]
    fn records_read_back_and_a_keyframe_is_stored_once() {
        let dir = TempDir::new("records");
        let path = dir.0.join("a.sb");
        let mut writer = Writer::open(&path, &fingerprint(), layout(4)).unwrap();
        writer.add(2, Some((4.0, &jpeg(1)))).unwrap();
        writer.add(1, Some((2.0, &jpeg(2)))).unwrap();
        // Keyframes further apart than the slots: slot 3 shows slot 2's.
        writer.add(3, Some((4.0, &jpeg(1)))).unwrap();
        let board = Storyboard::open(&path).unwrap();
        assert!(board.of(&fingerprint()) && board.layout == layout(4));
        assert_eq!(board.made(), 3);
        assert!(!board.complete());
        assert_eq!(board.frames.len(), 2, "the same keyframe isn't stored twice");
        assert_eq!(board.slot(3), Some(4.0));
        assert_eq!(board.frame(2.0).unwrap(), jpeg(2));
        assert_eq!(board.frame(4.0).unwrap(), jpeg(1));
        assert_eq!(board.nearest(0.0), Some(2.0));
        assert_eq!(board.nearest(3.1), Some(4.0));
        assert_eq!(board.nearest(99.0), Some(4.0));
        assert_eq!(std::fs::metadata(&path).unwrap().len(), board.end);
        // Slots 1 to 3 made: anywhere from 2 s on shows the keyframe at or
        // before it, however far; before slot 1's, slot 0 isn't made yet, so
        // only a frame within the interval (or `near`) stands in.
        assert_eq!(board.shown_at(3.9, 0.0), Some(2.0));
        assert_eq!(board.shown_at(7.9, 0.0), Some(4.0));
        assert_eq!(board.shown_at(99.0, 0.0), Some(4.0));
        assert_eq!(board.shown_at(1.0, 0.0), Some(2.0));
        assert_eq!(board.shown_at(-1.0, 0.0), None);
        assert_eq!(board.shown_at(-1.0, 5.0), Some(2.0));
        // A slot with no frame at all is made, with none.
        writer.add(0, None).unwrap();
        let board = Storyboard::open(&path).unwrap();
        assert!(board.complete());
        assert_eq!(board.slot(0), None);
        // Made, before the first keyframe: the first.
        assert_eq!(board.shown_at(0.5, 0.0), Some(2.0));
    }

    #[test]
    fn a_long_gop_shows_the_keyframe_at_or_before_however_far() {
        // A keyframe every 5 s (a screen recording), slots every 2: slots
        // 0-2 take the one at 0, 3-4 the one at 5, 5-7 at 10, 8-9 at 15.
        let dir = TempDir::new("gop");
        let path = dir.0.join("a.sb");
        let mut writer = Writer::open(&path, &fingerprint(), layout(10)).unwrap();
        for slot in order(10) {
            let key = (slot as f64 * 2.0 / 5.0).floor() * 5.0;
            writer.add(slot, Some((key, &jpeg(key as u8)))).unwrap();
        }
        let board = Storyboard::open(&path).unwrap();
        assert_eq!(board.frames.len(), 4);
        for (time, key) in
            [(0.0, 0.0), (2.6, 0.0), (4.99, 0.0), (5.0, 5.0), (7.6, 5.0), (12.6, 10.0), (17.6, 15.0), (19.9, 15.0)]
        {
            assert_eq!(board.shown_at(time, 0.0), Some(key), "at {time} s");
        }
        // Not the nearest, which is the next keyframe: a frame from after
        // the pointer, which Play there wouldn't reach for seconds.
        assert_eq!(board.nearest(4.0), Some(5.0));
    }

    #[test]
    fn a_torn_record_is_cut_and_what_was_made_carries_on() {
        let dir = TempDir::new("torn");
        let path = dir.0.join("a.sb");
        let mut writer = Writer::open(&path, &fingerprint(), layout(4)).unwrap();
        writer.add(2, Some((4.0, &jpeg(1)))).unwrap();
        let whole = writer.board.end;
        drop(writer);
        // A crash mid-record: half of it written.
        let mut record = 1u32.to_le_bytes().to_vec();
        record.extend_from_slice(&2.0f64.to_le_bytes());
        record.extend_from_slice(&100u32.to_le_bytes());
        record.extend_from_slice(&[0xFF; 10]);
        std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(&record).unwrap();
        // Read, the torn one isn't there; carried on, it's cut.
        assert_eq!(Storyboard::open(&path).unwrap().made(), 1);
        let mut writer = Writer::open(&path, &fingerprint(), layout(4)).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), whole);
        assert_eq!(writer.board.slot(2), Some(4.0), "what was made stays");
        writer.add(1, Some((2.0, &jpeg(2)))).unwrap();
        let board = Storyboard::open(&path).unwrap();
        assert_eq!((board.made(), board.frame(2.0).unwrap()), (2, jpeg(2)));
        // A reader holding it reads on from where it was.
        let mut reader = Storyboard::open(&path).unwrap();
        writer.add(0, Some((0.0, &jpeg(3)))).unwrap();
        reader.refresh().unwrap();
        assert_eq!(reader.made(), 3);
        // Of the file as it was, it starts over.
        let changed = Fingerprint { size: 99, ..fingerprint() };
        assert_eq!(Writer::open(&path, &changed, layout(4)).unwrap().board.made(), 0);
    }

    #[test]
    fn jpegs_are_split_by_their_markers() {
        let mut bytes = jpeg(1);
        bytes.extend(jpeg(0xD9));
        bytes.extend(jpeg(3));
        let parts = split_jpegs(&bytes);
        assert_eq!(parts, [jpeg(1).as_slice(), jpeg(0xD9).as_slice(), jpeg(3).as_slice()]);
        // Cut short: the whole ones only.
        assert_eq!(split_jpegs(&bytes[..bytes.len() - 3]).len(), 2);
    }

    #[test]
    fn each_inputs_pts_is_read_from_showinfo() {
        let said = "\
[Parsed_showinfo_1 @ 0x7f] config in time_base: 1/15360, frame_rate: 30/1
[Parsed_showinfo_5 @ 0x7f] n:   0 pts: 153600 pts_time:-1      duration:    512
[Parsed_showinfo_5 @ 0x7f] color_range:unknown
[Parsed_showinfo_1 @ 0x7f] n:   0 pts:      0 pts_time:0       duration:    512";
        assert_eq!(showinfo(said, &[0.0, 19.0, 30.0], 4), [Some(0.0), Some(18.0), None]);
        // MPEG-TS's: each input's keyframes up to its time, the last kept.
        let said = "\
[Parsed_showinfo_1 @ 0x7f] n:   0 pts:      0 pts_time:0       duration:    512
[Parsed_showinfo_7 @ 0x7f] n:   0 pts:  45000 pts_time:0.5     duration:    512
[Parsed_showinfo_1 @ 0x7f] n:   1 pts:  90000 pts_time:1       duration:    512
[Parsed_showinfo_7 @ 0x7f] n:   1 pts: 135000 pts_time:1.5     duration:    512";
        assert_eq!(showinfo(said, &[0.0, 3.0], 6), [Some(1.0), Some(4.5)]);
    }

    /// A 40 s file with a keyframe each second, if there's an ffmpeg to make one.
    fn sample(dir: &Path) -> Option<(PathBuf, PathBuf)> {
        let ffmpeg = tools::find_ffmpeg(Path::new("/nonexistent"))?;
        let path = dir.join("sample.mp4");
        let made = std::process::Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x180:rate=30:duration=40",
            ])
            .args(["-c:v", "libx264", "-preset", "ultrafast", "-g", "30", "-pix_fmt", "yuv420p", "-y"])
            .arg(&path)
            .status()
            .ok()?;
        made.success().then_some((ffmpeg, path))
    }

    #[test]
    fn a_files_storyboard_is_made_from_its_keyframes_and_carries_on_once_stopped() {
        let dir = TempDir::new("make");
        let Some((ffmpeg, source)) = sample(&dir.0) else {
            eprintln!("no ffmpeg: skipped");
            return;
        };
        let file = dir.0.join("sample.sb");
        let stop = AtomicBool::new(false);
        // Stopped after the first batch.
        let mut seen = Vec::new();
        let first = make(&ffmpeg, &source, &file, &stop, |board| {
            seen.push(board.made());
            false
        });
        assert!(first.as_ref().is_err_and(vodscribe_core::is_canceled), "{:?}", first.map(|b| b.map(|b| b.made())));
        assert_eq!(seen, [BATCH]);
        let board = Storyboard::open(&file).unwrap();
        assert_eq!((board.layout.width, board.layout.height, board.layout.slots), (160, 90, 20));
        // Halves first: the middle (20 s) and the picture's slot (5 s) are in.
        assert_eq!(board.slot(10), Some(20.0));
        assert_eq!(board.slot(board.layout.picture_slot()), Some(4.0));
        // Carried on: the rest, none again.
        let mut batches = 0;
        let board = make(&ffmpeg, &source, &file, &stop, |_| {
            batches += 1;
            true
        })
        .unwrap()
        .unwrap();
        assert_eq!(batches, (20 - BATCH).div_ceil(BATCH));
        assert!(board.complete());
        assert_eq!(Storyboard::open(&file).unwrap().made(), 20, "no slot twice");
        // Each slot's frame is the keyframe at or before it, a whole second.
        for slot in 0..20 {
            let pts = board.slot(slot).unwrap();
            assert!(pts <= board.layout.time(slot) + 1e-6 && pts.fract() == 0.0, "slot {slot}: {pts}");
        }
        let jpeg = board.frame(board.nearest(7.4).unwrap()).unwrap();
        assert_eq!(&jpeg[..2], [0xFF, 0xD8]);
        // Made: found done without a probe or a frame read.
        assert!(make(&ffmpeg, &source, &file, &stop, |_| panic!("nothing to make")).unwrap().unwrap().complete());
    }
}
