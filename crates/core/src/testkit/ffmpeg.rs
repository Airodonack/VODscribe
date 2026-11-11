//! A fake ffmpeg that answers as ffmpeg does to what the app asks of it:
//! a file's details (`local::probe`), a cut or a render (`clips::cut`,
//! `render`, through `tools::encode`, its progress included), a still
//! frame, a thumbnail. It reads and writes fake media files (`Media`):
//! a cut of `-ss` 10 and `-t` 10 from a 60-second file is a 10-second file
//! that says it's 10 s on of that one. It fails as ffmpeg does where the
//! app could meet it: a file that isn't there or isn't media, a stream map
//! that matches nothing, an encoder this build lacks (and its list of
//! encoders, `-encoders`, says so); and on request partway, or holds
//! partway (a still once its input is read) at a `Gate`. A file it makes is
//! there from the run's start, as ffmpeg's is, but isn't media till the
//! run's end; what it's made with (`-crf`, `-profile:v`, `-pix_fmt`,
//! `-tag:v`) is kept in it. It keeps what each run was asked (`runs`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::media::{Media, Video};
use super::tools::{self, Fake, Gate, Run};

/// The fake ffmpeg at a path, faked until it's dropped.
pub struct Ffmpeg {
    fake: Fake,
    plan: Arc<Mutex<Plan>>,
    /// Each run's arguments, in order.
    runs: Arc<Mutex<Vec<Vec<String>>>>,
}

/// What a run that writes a file does, besides its work.
#[derive(Clone, Default)]
struct Plan {
    /// Fails once this far through (0 to 1), saying this.
    fail: Option<(f64, String)>,
    /// Waits at the gate once this far through.
    hold: Option<(f64, Gate)>,
    /// Encoders it doesn't have (libx265, dnxhd…).
    lacks: Vec<String>,
    /// Its list of encoders waits at this gate.
    list_hold: Option<Gate>,
    /// A still waits at this gate, its input read.
    still_hold: Option<Gate>,
}

impl Ffmpeg {
    /// Fakes ffmpeg at `path`.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        let plan = Arc::new(Mutex::new(Plan::default()));
        let runs = Arc::new(Mutex::new(Vec::new()));
        let (planned, ran) = (plan.clone(), runs.clone());
        let fake = tools::fake(path, move |run| {
            ran.lock().unwrap().push(run.args());
            let plan = planned.lock().unwrap().clone();
            answer(run, &plan)
        });
        Ffmpeg { fake, plan, runs }
    }

    /// Fakes the ffmpeg the app finds in `bin_dir` (`Paths::bin`), as it
    /// finds the one it downloaded.
    pub fn in_bin(bin_dir: &Path) -> Self {
        Self::at(crate::deps::managed_path(bin_dir, "ffmpeg"))
    }

    /// Ends its runs, now and to come (`Fake::end`).
    pub fn end(&self) {
        self.fake.end();
    }

    /// Where it is.
    pub fn path(&self) -> &Path {
        self.fake.path()
    }

    /// Each file it makes from now fails `at` (0 to 1) of the way, as
    /// ffmpeg does when a source breaks off: what it wrote is left as it
    /// was, `message` goes to its errors, and it exits with 1.
    pub fn fails_at(&self, at: f64, message: &str) {
        self.plan.lock().unwrap().fail = Some((at, message.into()));
    }

    /// Each file it makes from now waits `at` (0 to 1) of the way until
    /// `gate` opens, or the app stops it.
    pub fn holds_at(&self, at: f64, gate: &Gate) {
        self.plan.lock().unwrap().hold = Some((at, gate.clone()));
    }

    /// From now it lacks `encoder` (libx265, dnxhd, prores_ks), as a build
    /// without it does.
    pub fn lacks(&self, encoder: &str) {
        self.plan.lock().unwrap().lacks.push(encoder.into());
    }

    /// From now its list of encoders (`-encoders`) waits until `gate`
    /// opens, or the app stops it, as a first run under a virus scan waits.
    pub fn holds_list(&self, gate: &Gate) {
        self.plan.lock().unwrap().list_hold = Some(gate.clone());
    }

    /// Each still it makes from now (a clip's picture) waits, its input
    /// read, until `gate` opens, or the app stops it: as ffmpeg has the
    /// file open while it decodes the frame.
    pub fn holds_stills(&self, gate: &Gate) {
        self.plan.lock().unwrap().still_hold = Some(gate.clone());
    }

    /// From now it works as it should: no failure, hold or missing encoder.
    pub fn works(&self) {
        *self.plan.lock().unwrap() = Plan::default();
    }

    /// Its file replaced by another ffmpeg's, as an update or another
    /// download does: the app sees a new ffmpeg at the same path.
    pub fn updated(&self) {
        let was = std::fs::read(self.path()).unwrap();
        std::fs::write(self.path(), [was.as_slice(), b"updated\n"].concat()).unwrap();
    }

    /// The arguments of each run so far, in order.
    pub fn runs(&self) -> Vec<Vec<String>> {
        self.runs.lock().unwrap().clone()
    }
}

/// An input: its path and where it's read from (`-ss` before it).
struct Input {
    path: String,
    seek: f64,
}

/// ffmpeg's answer to `run`.
fn answer(run: &mut Run, plan: &Plan) -> i32 {
    if run.has("-encoders") {
        if let Some(gate) = &plan.list_hold
            && !run.hold(gate)
        {
            return 255;
        }
        let listed =
            ENCODERS.lines().filter(|l| !plan.lacks.iter().any(|e| l.split_whitespace().nth(1) == Some(e.as_str())));
        run.out(&listed.map(|l| format!("{l}\n")).collect::<String>());
        return 0;
    }
    let args = run.args();
    let mut inputs: Vec<Input> = Vec::new();
    // An -ss before an input is where that input is read from; one after
    // the last is the output's, and changes nothing here.
    let (mut seek, mut format) = (0.0, None);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-ss" => seek = number(args.get(i + 1)),
            "-f" => format = args.get(i + 1).cloned(),
            // A render's chapters, which are no media.
            "-i" if format.as_deref() == Some("ffmetadata") => {}
            "-i" => inputs.push(Input { path: args.get(i + 1).cloned().unwrap_or_default(), seek }),
            _ => {
                i += 1;
                continue;
            }
        }
        if args[i] == "-i" {
            (seek, format) = (0.0, None);
        }
        i += 2;
    }
    let mut read = Vec::new();
    for input in &inputs {
        match Media::read(Path::new(&input.path)) {
            Some(media) => read.push(media),
            None if !Path::new(&input.path).exists() => {
                run.err(&format!("{}: No such file or directory\n", input.path));
                return 1;
            }
            None => {
                run.err(&format!("{}: Invalid data found when processing input\n", input.path));
                return 1;
            }
        }
    }
    let Some(first) = read.first() else {
        run.err("Output file does not contain any stream\n");
        return 1;
    };
    // Asked nothing of it: its details, and an error for no output.
    let last = args.last().cloned().unwrap_or_default();
    if inputs.last().is_some_and(|i| i.path == last) {
        let details = first.details(&inputs[0].path);
        run.err(&details);
        return 1;
    }
    for flag in ["-c:v", "-c:a"] {
        if let Some(encoder) = run.after(flag).filter(|e| plan.lacks.contains(e)) {
            run.err(&format!("Unknown encoder '{encoder}'\n"));
            return 1;
        }
    }
    // What's mapped: the first input's picture and sound unless told otherwise.
    let maps: Vec<String> = args.windows(2).filter(|w| w[0] == "-map").map(|w| w[1].clone()).collect();
    let wants = |kind: &str| maps.is_empty() || maps.iter().any(|m| m.starts_with(&format!("0:{kind}")));
    for map in &maps {
        let has = match map.trim_end_matches('?') {
            m if m.starts_with("0:v") => first.video.is_some(),
            m if m.starts_with("0:a") => first.audio.is_some(),
            _ => true,
        };
        if !has && !map.ends_with('?') {
            run.err(&format!("Stream map '{map}' matches no streams.\n"));
            return 1;
        }
    }
    let picture = !run.has("-vn") && wants("v") && first.video.is_some();
    let sound = wants("a") && first.audio.is_some();

    // A still: a JPEG to stdout, or one frame as a PNG.
    if run.after("-frames:v").as_deref() == Some("1") {
        if first.video.is_none() {
            run.err("Output file does not contain any stream\n");
            return 1;
        }
        if let Some(gate) = &plan.still_hold
            && !run.hold(gate)
        {
            return 255;
        }
        if last == "pipe:1" {
            run.out_bytes(JPEG);
        } else {
            std::fs::write(&last, PNG).unwrap();
        }
        return 0;
    }

    let available = (first.duration - inputs[0].seek).max(0.0);
    let duration = run.after("-t").map_or(available, |t| number(Some(&t)).min(available));
    let codec = |flag: &str, was: Option<&str>| match run.after(flag).as_deref() {
        None | Some("copy") => was.map(str::to_string),
        Some("libx264") => Some("h264".into()),
        Some("libx265") => Some("hevc".into()),
        Some("prores_ks") => Some("prores".into()),
        Some(other) => Some(other.to_string()),
    };
    // Encoded again, its picture is as the options asked; copied, as it was.
    let encoded = run.after("-c:v").is_some_and(|c| c != "copy");
    let asked = |flag: &str, was: &Option<String>| if encoded { run.after(flag) } else { was.clone() };
    let video = first.video.as_ref().filter(|_| picture).map(|v| Video {
        codec: codec("-c:v", Some(&v.codec)).unwrap_or_default(),
        // Made even, as the app's scale asks.
        width: v.width / 2 * 2,
        height: v.height / 2 * 2,
        fps: v.fps,
        crf: asked("-crf", &v.crf),
        profile: asked("-profile:v", &v.profile),
        pix_fmt: asked("-pix_fmt", &v.pix_fmt),
        tag: asked("-tag:v", &v.tag),
    });
    let audio = if sound { codec("-c:a", first.audio.as_deref()) } else { None };
    let cut_from = match &first.cut_from {
        Some((source, start)) => (source.clone(), start + inputs[0].seek),
        None => (inputs[0].path.clone(), inputs[0].seek),
    };
    let container = run.after("-f").unwrap_or_else(|| "mp4".into());
    let made = Media { duration, video, audio, container, cut_from: Some(cut_from) };

    let progress = run.after("-progress").as_deref() == Some("pipe:1");
    let us = |fraction: f64| (duration * fraction * 1e6).round() as u64;
    // Its output is there from the start, not yet media, as ffmpeg's is
    // while it writes: a run stopped or failed partway leaves it.
    if let Some(dir) = Path::new(&last).parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(&last, BEING_WRITTEN).unwrap();
    let mut before = 0.0;
    for step in [0.25, 0.5, 0.75, 1.0] {
        // Due in this step: past the one before (or at the very start).
        let due = |at: f64| at <= step && (at > before || before == 0.0);
        if let Some((_, gate)) = plan.hold.as_ref().filter(|(at, _)| due(*at))
            && !run.hold(gate)
        {
            return 255;
        }
        if let Some((_, message)) = plan.fail.as_ref().filter(|(at, _)| due(*at)) {
            // What was written so far stays, as ffmpeg leaves it.
            run.err(&format!("{message}\n"));
            return 1;
        }
        if run.stopped() {
            return 255;
        }
        if progress && step < 1.0 {
            run.out(&format!("out_time_us={}\nprogress=continue\n", us(step)));
        }
        before = step;
    }
    made.write(Path::new(&last));
    if progress {
        run.out(&format!("out_time_us={}\nprogress=end\n", us(1.0)));
    }
    0
}

/// What a file it's making holds till it's whole: no media yet.
const BEING_WRITTEN: &[u8] = b"FAKE MEDIA (being written)\n";

/// What `ffmpeg -encoders` lists, as a full build does, but shorter: the
/// encoders the app asks for, and a few beside.
const ENCODERS: &str = "Encoders:
 V..... = Video
 A..... = Audio
 S..... = Subtitle
 .F.... = Frame-level multithreading
 ..S... = Slice-level multithreading
 ...X.. = Codec is experimental
 ....B. = Supports draw_horiz_band
 .....D = Supports direct rendering method 1
 ------
 VFS..D dnxhd                VC3/DNxHD
 V....D libx264              libx264 H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10 (codec h264)
 V....D libx265              libx265 H.265 / HEVC (codec hevc)
 VFS..D mjpeg                MJPEG (Motion JPEG)
 V....D png                  PNG (Portable Network Graphics) image
 VF.... prores               Apple ProRes
 VFS... prores_ks            Apple ProRes (iCodec Pro) (codec prores)
 A....D aac                  AAC (Advanced Audio Coding)
 A....D pcm_s16le            PCM signed 16-bit little-endian";

fn number(arg: Option<&String>) -> f64 {
    arg.and_then(|a| a.parse().ok()).unwrap_or(0.0)
}

/// A one-pixel PNG, as a thumbnail is.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49,
    0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d,
    0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// The start and end of a JPEG, as a still frame's bytes begin and end.
const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0xff, 0xd9];
