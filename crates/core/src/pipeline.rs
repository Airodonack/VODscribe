use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::db::{Db, Status, Vod};
use crate::fetch;
use crate::tools::{self, YtDlp};
use crate::transcript::{Segmenter, Word};

/// Stream URLs in a saved info JSON expire (YouTube's after about 6 hours, others
/// sooner), so a download that starts later than this probes the VOD again.
const INFO_MAX_AGE: Duration = Duration::from_secs(30 * 60);

pub struct Tools<'a> {
    /// Not needed for local files.
    pub yt_dlp: Option<&'a YtDlp>,
    pub ffmpeg: &'a Path,
}

/// How to launch a `vodscribe-worker` process.
#[derive(Clone, Debug)]
pub struct WorkerSpec {
    pub program: PathBuf,
    /// Recorded on the VOD, so transcripts from older models can be flagged.
    pub model_id: &'static str,
    pub model_dir: PathBuf,
    /// "cpu" or "cuda".
    pub provider: &'static str,
    /// Chunks per decode call; see `asr::Parallelism::batch`.
    pub batch: usize,
    /// Extra directories for the dynamic linker (the GPU pack's libraries).
    pub lib_dirs: Vec<PathBuf>,
}

pub struct Transcription {
    pub worker: WorkerSpec,
    /// Used if `worker` fails, e.g. the GPU runs out of memory.
    pub fallback: Option<WorkerSpec>,
}

/// Something worth telling the user that isn't an error.
pub type Notice<'a> = &'a mut dyn FnMut(String);

pub fn info_path(work_dir: &Path, vod_id: i64) -> PathBuf {
    work_dir.join(format!("{vod_id}.info.json"))
}

/// Downloads a VOD's audio, resuming a partial download. Returns its path.
pub fn download(
    db: &mut Db,
    vod: &Vod,
    tools: &Tools,
    work_dir: &Path,
    video: Option<(&crate::playable::Playable, u32)>,
    cancel: &AtomicBool,
    mut report: impl FnMut(Status, f64),
) -> Result<PathBuf> {
    // A local file is transcribed where it is: there's nothing to download.
    if let Some(source) = &vod.source_path {
        return crate::local::source(source);
    }
    // A VOD whose video is kept (always, for sites without their own player;
    // on request, for the others): transcribed from that video, once it's here.
    if let Some(kept) = db.media_path(vod.id)?.map(PathBuf::from).filter(|p| p.is_file()) {
        return Ok(kept);
    }
    let keep = video.filter(|_| keeps_video(&vod.info.platform));
    fs::create_dir_all(work_dir)?;
    let info = info_path(work_dir, vod.id);
    let fresh = fs::metadata(&info)
        .and_then(|m| m.modified())
        .is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < INFO_MAX_AGE);

    db.set_status(vod.id, Status::Downloading, 0.0, None)?;
    report(Status::Downloading, 0.0);
    let yt_dlp = tools.yt_dlp.context(fetch::NO_YT_DLP)?;
    if !fresh {
        fetch::probe(yt_dlp, &vod.info.url, &info, cancel)?;
    }
    let selector = keep.map(|(playable, height)| playable.selector(height));
    let (out, wanted) = match &selector {
        Some(selector) => {
            let dir = media_dir(work_dir);
            fs::create_dir_all(&dir)?;
            (media_stem(&dir, vod.id, &vod.info.title), fetch::Wanted::Video { selector, ffmpeg: tools.ffmpeg })
        }
        None => (work_dir.join(vod.id.to_string()), fetch::Wanted::Audio),
    };
    let file = fetch::download_or_reprobe(yt_dlp, &vod.info.url, &info, &out, wanted, cancel, |f| {
        report(Status::Downloading, f)
    })?;
    let Some((playable, _)) = keep else { return Ok(file) };
    // Kept to play only if what arrived plays here: sites only claim their
    // codecs. Otherwise it's still transcribed, from the work dir, and goes
    // with the rest of the work files after.
    let streams = crate::local::codecs(tools.ffmpeg, &file)?;
    if playable.plays(&streams) {
        db.set_media_path(vod.id, Some(&file.to_string_lossy()))?;
        return Ok(file);
    }
    eprintln!("VOD {}: {streams:?} doesn't play here, so it isn't kept", vod.id);
    let ext = file.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    let moved = work_dir.join(format!("{}.{ext}", vod.id));
    fs::rename(&file, &moved)?;
    Ok(moved)
}

/// Whether a VOD from `platform` has its video kept to play here: any site but
/// those with embeds of their own (and files on this computer, played in place).
pub fn keeps_video(platform: &str) -> bool {
    !matches!(platform, "youtube" | "twitch" | "kick" | crate::local::PLATFORM)
}

/// Where kept videos go (`Paths::media`): beside the work dir, never in it, so
/// clearing a VOD's work files after transcribing can't take its video.
pub fn media_dir(work_dir: &Path) -> PathBuf {
    work_dir.with_file_name("media")
}

/// Where VOD `vod_id`'s kept video goes in `dir`, without its extension:
/// `<title> [<id>]`, so it can be found and copied out by hand, in order by
/// title, then by id: padded to six digits, so [000007] comes before
/// [000012]. The id is how the app knows whose it is (`owner`); videos kept
/// before 0.4.0 are plain `<id>.<ext>`.
pub fn media_stem(dir: &Path, vod_id: i64, title: &str) -> PathBuf {
    dir.join(format!("{} [{vod_id:06}]", crate::clips::safe_name(title)))
}

/// The VOD a work or media file belongs to: `<id>.…` (the work dir, and
/// videos kept before 0.4.0) or `<title> [<id>]` followed by its extension
/// or a download's part suffixes (`media_stem`).
fn owner(name: &str) -> Option<i64> {
    let digits = |s: &str| s.parse::<i64>().ok().filter(|_| s.bytes().all(|b| b.is_ascii_digit()));
    // The last " [digits]" followed by the end or a dot (titles have no square
    // brackets), looked for first: a title can start with digits and a dot.
    let mut found = None;
    let mut rest = name;
    while let Some(at) = rest.find(" [") {
        let after = &rest[at + 2..];
        if let Some((id, tail)) = after.split_once(']')
            && (tail.is_empty() || tail.starts_with('.'))
            && let Some(id) = digits(id)
        {
            found = Some(id);
        }
        rest = after;
    }
    found.or_else(|| name.split_once('.').and_then(|(id, _)| digits(id)))
}

/// Removes files in `media_dir` (named `<vod id>.…` or `<vod id> …`) of VODs not in `library`:
/// left by a crash, say, mid-way through removing one. Files of VODs still
/// there stay, as a paused download carries on from its parts.
pub fn tidy_media(work_dir: &Path, library: &[i64]) {
    let Ok(entries) = fs::read_dir(media_dir(work_dir)) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let id = owner(&name);
        if id.is_some_and(|id| !library.contains(&id)) && entry.path().is_file() {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Removes VOD `vod_id`'s kept video, and any part of one, from `media_dir`.
pub fn remove_media(work_dir: &Path, vod_id: i64) {
    remove_work_files(&media_dir(work_dir), vod_id);
}

/// Transcribes downloaded audio, writing segments to the index as they are
/// produced so partial results are searchable during a long run. Picks up
/// after the last indexed segment, so a paused or interrupted job resumes.
#[allow(clippy::too_many_arguments)]
pub fn transcribe(
    db: &mut Db,
    vod: &Vod,
    ffmpeg: &Path,
    audio_path: &Path,
    transcription: &Transcription,
    cancel: &AtomicBool,
    report: &mut impl FnMut(Status, f64),
    notice: Notice,
) -> Result<()> {
    // A video without sound has nothing to transcribe: done, with no lines.
    // Nothing found at all is a file ffmpeg couldn't read, not a silent one.
    let streams = crate::local::codecs(ffmpeg, audio_path)?;
    if streams.audio.is_none() && streams.video.is_none() {
        bail!("{} couldn't be read.", tools::shown(audio_path));
    }
    if streams.audio.is_none() {
        db.set_status(vod.id, Status::Done, 1.0, None)?;
        report(Status::Done, 1.0);
        return Ok(());
    }
    let attempt = |spec: &WorkerSpec, db: &mut Db, report: &mut dyn FnMut(Status, f64)| {
        run_worker(db, vod, spec, ffmpeg, audio_path, cancel, report)
    };
    // A paused or interrupted job resumes after its last segment, but never on
    // top of segments from an older model (or from before models were recorded).
    if db.transcribed_until(vod.id)? > 0.0 && !db.model(vod.id)?.is_some_and(|m| crate::model::is_current(&m)) {
        db.clear_segments(vod.id)?;
    }
    let mut used = &transcription.worker;
    db.set_model(vod.id, Some(used.model_id))?;
    if let Err(e) = attempt(used, db, report) {
        let Some(fallback) = &transcription.fallback else { return Err(e) };
        if cancel.load(Ordering::Relaxed) {
            return Err(e);
        }
        eprintln!("GPU transcription failed: {e:#}");
        notice("The graphics card ran into a problem, so the rest was transcribed with the processor.".into());
        used = fallback;
        db.set_model(vod.id, Some(used.model_id))?;
        attempt(used, db, report)?;
    }
    db.set_status(vod.id, Status::Done, 1.0, None)?;
    report(Status::Done, 1.0);
    Ok(())
}

/// Deletes everything in `work_dir` belonging to a VOD: its info JSON, the
/// audio, and any partial download fragments.
pub fn remove_work_files(work_dir: &Path, vod_id: i64) {
    let Ok(entries) = fs::read_dir(work_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if owner(&name) == Some(vod_id) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[derive(Deserialize)]
struct WorkerLine {
    #[serde(default)]
    end: Option<f64>,
    #[serde(default)]
    words: Vec<Word>,
}

/// Runs one worker over the whole file, indexing segments as they stream in.
#[allow(clippy::too_many_arguments)]
fn run_worker(
    db: &mut Db,
    vod: &Vod,
    spec: &WorkerSpec,
    ffmpeg: &Path,
    audio_path: &Path,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(Status, f64),
) -> Result<()> {
    let duration = vod.info.duration.filter(|d| *d > 0.0);
    let fraction = |t: f64| duration.map_or(0.0, |d| (t / d).min(1.0));
    let start = db.transcribed_until(vod.id)?;
    db.set_status(vod.id, Status::Transcribing, fraction(start), None)?;
    report(Status::Transcribing, fraction(start));

    let mut cmd = tools::command(&spec.program);
    cmd.arg("--model").arg(&spec.model_dir).args(["--provider", spec.provider]);
    cmd.args(["--batch", &spec.batch.to_string()]);
    cmd.arg("--ffmpeg").arg(ffmpeg).args(["--start", &format!("{start:.3}")]).arg(audio_path);
    if !spec.lib_dirs.is_empty() {
        let var = if cfg!(windows) { "PATH" } else { "LD_LIBRARY_PATH" };
        let existing = std::env::var_os(var).unwrap_or_default();
        // An empty entry would mean the working directory to the loader.
        let paths =
            spec.lib_dirs.iter().cloned().chain(std::env::split_paths(&existing)).filter(|p| !p.as_os_str().is_empty());
        cmd.env(var, std::env::join_paths(paths)?);
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("start {}", tools::shown(&spec.program)))?;
    let _tracked = tools::track(&child);

    // sherpa-onnx logs to stderr; keep the tail for error messages.
    let stderr_thread = tools::drain(child.stderr.take().unwrap());

    // Wake regularly while waiting for output, so a stop kills the worker
    // promptly instead of waiting for its next batch.
    let (tx, rx) = mpsc::channel();
    tools::send_lines(child.stdout.take().unwrap(), tx);

    let mut segmenter = Segmenter::new();
    let result = (|| -> Result<()> {
        loop {
            let line = match rx.recv_timeout(tools::STOP_POLL) {
                Ok(line) => Some(line),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            };
            if cancel.load(Ordering::Relaxed) {
                return Err(crate::Canceled.into());
            }
            let Some(line) = line else { continue };
            let line: WorkerLine = serde_json::from_str(&line).context("unreadable worker output")?;
            if let Some(end) = line.end {
                db.insert_segments(vod.id, &segmenter.push(line.words))?;
                report(Status::Transcribing, fraction(end));
            }
        }
    })();
    if result.is_err() {
        tools::kill_tree(&mut child);
        // Don't wait for the stderr reader: a process the worker started may
        // still hold the pipe open, and the error at hand is what matters.
        return result;
    }
    let status = child.wait()?;
    let stderr = stderr_thread.join().unwrap_or_default();
    if !status.success() {
        let msg = tools::last_lines(&stderr, 3);
        bail!("{}", if msg.is_empty() { format!("worker exited with {status}") } else { msg });
    }
    if let Some(last) = segmenter.finish() {
        db.insert_segments(vod.id, &[last])?;
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A video without sound is done at once, with no lines: no worker runs
    /// (the one given doesn't exist). Skipped where ffmpeg isn't installed.
    #[test]
    fn a_video_without_sound_is_done_without_transcribing() {
        let Ok(ffmpeg) = which::which("ffmpeg") else {
            eprintln!("skipped: no ffmpeg on PATH");
            return;
        };
        let dir = crate::TempDir::new("silent");
        let file = dir.join("silent.mp4");
        let made = std::process::Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=d=1:s=160x90"])
            .arg(&file)
            .status()
            .unwrap();
        assert!(made.success());
        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let (vod, _) = db.add_vod(&crate::local::probe(&ffmpeg, &file).unwrap()).unwrap();
        let none = WorkerSpec {
            program: dir.join("no-such-worker"),
            model_id: "none",
            model_dir: dir.join("none"),
            provider: "cpu",
            batch: 1,
            lib_dirs: Vec::new(),
        };
        let transcription = Transcription { worker: none, fallback: None };
        let mut seen = Vec::new();
        let (cancel, mut notice) = (AtomicBool::new(false), |_: String| {});
        transcribe(&mut db, &vod, &ffmpeg, &file, &transcription, &cancel, &mut |s, _| seen.push(s), &mut notice)
            .unwrap();
        assert_eq!(seen, [Status::Done]);
        assert_eq!(db.vod(vod.id).unwrap().unwrap().status, Status::Done);
        assert_eq!(db.transcribed_until(vod.id).unwrap(), 0.0, "no lines");
    }

    #[test]
    fn tidies_media_of_vods_gone() {
        let root = crate::TempDir::new("tidy-media");
        let work = root.join("work");
        let media = media_dir(&work);
        fs::create_dir_all(&media).unwrap();
        for name in
            ["3.mp4", "A title [4].mp4.part-Frag7.part", "29.mp4.part-Frag65.part", "Gone [30].m4a", "notes.txt"]
        {
            fs::write(media.join(name), b"x").unwrap();
        }
        tidy_media(&work, &[3, 4]);
        let mut left: Vec<String> =
            fs::read_dir(&media).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["3.mp4", "A title [4].mp4.part-Frag7.part", "notes.txt"]);
        // And a VOD's own go, named either way, but not another's with its digits.
        fs::write(media.join("Version 3.1 [3].f137.mp4"), b"x").unwrap();
        fs::write(media.join("Other [34].mp4"), b"x").unwrap();
        remove_media(&work, 3);
        let mut left: Vec<String> =
            fs::read_dir(&media).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["A title [4].mp4.part-Frag7.part", "Other [34].mp4", "notes.txt"]);
    }

    #[test]
    fn files_say_whose_they_are() {
        assert_eq!(owner("12.info.json"), Some(12));
        assert_eq!(owner("12.m4a.part"), Some(12));
        assert_eq!(owner("Version 1.2 [7].mp4"), Some(7), "not the 1 of 1.2");
        assert_eq!(owner("Day 3 [7].f399.mp4.part"), Some(7));
        assert_eq!(owner("Talk (3) [7]"), Some(7));
        assert_eq!(owner("Talk [000007].mp4"), Some(7), "padded");
        assert_eq!(owner("2024.10.15 Stream [000007].mp4"), Some(7), "not 2024");
        assert_eq!(owner("12. Intro [000007].f399.mp4.part"), Some(7), "not 12");
        assert_eq!(media_stem(Path::new("/m"), 7, "Talk [1]"), Path::new("/m/Talk (1) [000007]"));
        assert_eq!(owner("clip-12-1000-5000.mp4"), None);
        assert_eq!(owner("video-12.info.json"), None);
        assert_eq!(owner("notes.txt"), None);
        assert_eq!(owner("A [b] [x].mp4"), None);
    }
    use crate::fetch::VodInfo;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    use std::time::Instant;

    /// Every path that cleans up after a VOD leaves a local file alone.
    #[test]
    fn local_files_survive_every_cleanup() {
        let root = crate::TempDir::new("local");
        let paths = crate::Paths::new(root.join("data"));
        paths.ensure().unwrap();
        let mine = root.join("mine");
        fs::create_dir_all(&mine).unwrap();
        let source = mine.join("1.mp4");
        fs::write(&source, b"the user's video").unwrap();

        let mut db = Db::open(&paths.db()).unwrap();
        let info = VodInfo {
            platform: crate::local::PLATFORM.into(),
            platform_id: source.to_string_lossy().into_owned(),
            url: source.to_string_lossy().into_owned(),
            title: "1".into(),
            channel: None,
            duration: Some(1.0),
            thumbnail: None,
            published: None,
        };
        let vod = db.add_vod(&info).unwrap().0;
        db.set_source_path(vod.id, &source.to_string_lossy()).unwrap();
        let vod = db.vod(vod.id).unwrap().unwrap();

        let yt_dlp = YtDlp { path: "/nonexistent/yt-dlp".into(), js_runtime: None };
        let tools = Tools { yt_dlp: Some(&yt_dlp), ffmpeg: Path::new("/nonexistent/ffmpeg") };
        let audio = download(&mut db, &vod, &tools, &paths.work(), None, &AtomicBool::new(false), |_, _| {}).unwrap();
        assert_eq!(audio, source, "transcribed in place, not copied");
        db.set_audio_path(vod.id, Some(&audio.to_string_lossy())).unwrap();

        remove_work_files(&paths.work(), vod.id);
        paths.clear_cache().unwrap();
        db.forget_downloads().unwrap();
        db.delete_vod(vod.id).unwrap();
        assert_eq!(fs::read(&source).unwrap(), b"the user's video");

        fs::remove_file(&source).unwrap();
        assert!(download(&mut db, &vod, &tools, &paths.work(), None, &AtomicBool::new(false), |_, _| {}).is_err());
    }

    #[test]
    fn a_stop_kills_a_quiet_worker_promptly() {
        // A "worker" that prints nothing for 30 s.
        let dir = crate::TempDir::new("stop");
        let program = dir.join("worker.sh");
        fs::write(&program, "#!/bin/sh\nsleep 30\n").unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();

        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let info = VodInfo {
            platform: "youtube".into(),
            platform_id: "x".into(),
            url: "u".into(),
            title: "t".into(),
            channel: None,
            duration: Some(60.0),
            thumbnail: None,
            published: None,
        };
        let (vod, _) = db.add_vod(&info).unwrap();
        let spec = WorkerSpec {
            program,
            model_id: "m",
            model_dir: dir.to_path_buf(),
            provider: "cpu",
            batch: 1,
            lib_dirs: Vec::new(),
        };

        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            flag.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        let result = run_worker(&mut db, &vod, &spec, Path::new("ffmpeg"), Path::new("audio"), &cancel, &mut |_, _| {});
        assert!(result.is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "took {:?}", started.elapsed());
    }
}
