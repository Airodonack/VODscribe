use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::Paths;
use crate::db::{Db, Status, Vod};
use crate::fetch;
use crate::stretches::Stretch;
use crate::tools::{self, YtDlp};
use crate::transcript::{Segment, Segmenter, Word};

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

/// Where `audio_for` found a stretch's audio.
#[derive(Debug, PartialEq)]
pub enum Source {
    /// The VOD's whole audio: its file on this computer or its kept video
    /// (`downloaded` false), or audio in the work dir, found there from
    /// another request or downloaded now (`downloaded`).
    Whole { path: PathBuf, downloaded: bool },
    /// A section downloaded for the stretch alone, its 0 at `start`.
    Section { path: PathBuf, start: f64 },
}

impl Source {
    /// The audio to transcribe from.
    pub fn audio(self) -> Audio {
        match self {
            Source::Whole { path, .. } => Audio { path, offset: 0.0 },
            Source::Section { path, start } => Audio { path, offset: start },
        }
    }
}

/// Deletes request `stretch_id`'s section of audio, and any part of one.
pub fn remove_section(paths: &Paths, vod_id: i64, stretch_id: i64) {
    let stem = paths.source_section_stem(vod_id, stretch_id);
    remove_work_prefix(&paths.work(), &stem.file_name().unwrap_or_default().to_string_lossy());
}

/// Gets the audio to transcribe `stretch` from, the first way that works:
///
/// 1. a file on this computer, or the VOD's kept video, read in place (the
///    worker reads just the stretch of it);
/// 2. the VOD's whole audio, already in the work dir for another request;
/// 3. a section of the VOD for the stretch alone (yt-dlp's
///    `--download-sections`): sites serve audio over https or HLS, which
///    ffmpeg fetches only the needed parts of;
/// 4. the VOD's whole audio, downloaded as for a request for all of it,
///    when the section fails for any reason but a stop. A request for all
///    of it goes straight here.
///
/// Footage already here (sections downloaded for edits, `footage::find`)
/// could serve too, ahead of 3; it's left for when the takeover's footage
/// registers in the same lookup.
#[allow(clippy::too_many_arguments)]
pub fn audio_for(
    db: &mut Db,
    vod: &Vod,
    stretch: &Stretch,
    tools: &Tools,
    paths: &Paths,
    video: Option<(&crate::playable::Playable, u32)>,
    cancel: &AtomicBool,
    mut report: impl FnMut(Status, f64),
) -> Result<Source> {
    // Lines it would cut through are transcribed again whole.
    let stretch = &db.snap_stretch(stretch.id)?;
    // A local file is transcribed where it is: there's nothing to download.
    if let Some(source) = &vod.source_path {
        return Ok(Source::Whole { path: crate::local::source(source)?, downloaded: false });
    }
    // A VOD whose video is kept (always, for sites without their own player;
    // on request, for the others): transcribed from that video, once it's here.
    if let Some(kept) = db.media_path(vod.id)?.map(PathBuf::from).filter(|p| p.is_file()) {
        return Ok(Source::Whole { path: kept, downloaded: false });
    }
    if let Some(whole) = db.audio_path(vod.id)?.map(PathBuf::from).filter(|p| p.is_file()) {
        return Ok(Source::Whole { path: whole, downloaded: false });
    }
    db.set_stretch_status(stretch.id, Status::Downloading, 0.0, None)?;
    report(Status::Downloading, 0.0);
    let end = stretch.end.or(vod.info.duration.filter(|_| stretch.start > 0.0));
    if let Some(end) = end.filter(|_| !stretch.is_whole()) {
        match download_section(vod, stretch.id, (stretch.start, end), tools, paths, cancel, &mut report) {
            Ok(path) => return Ok(Source::Section { path, start: stretch.start }),
            Err(e) if crate::is_canceled(&e) => return Err(e),
            Err(e) => {
                eprintln!(
                    "VOD {}: its section {}-{end} didn't download, so all of it does: {e:#}",
                    vod.id, stretch.start
                );
                remove_section(paths, vod.id, stretch.id);
            }
        }
    }
    let whole = stretch.is_whole();
    let path = download(db, vod, tools, paths, video.filter(|_| whole), cancel, report)?;
    Ok(Source::Whole { path, downloaded: true })
}

/// Where VOD `vod`'s info JSON is (`Paths::source_info`), probed again
/// first if it's older than its URLs last.
fn fresh_info(yt_dlp: &YtDlp, vod: &Vod, paths: &Paths, cancel: &AtomicBool) -> Result<PathBuf> {
    fs::create_dir_all(paths.work())?;
    let info = paths.source_info(vod.id);
    let fresh = fs::metadata(&info)
        .and_then(|m| m.modified())
        .is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < INFO_MAX_AGE);
    if !fresh {
        fetch::probe(yt_dlp, &vod.info.url, &info, cancel)?;
    }
    Ok(info)
}

/// Downloads `start`..`end` seconds of a VOD's audio alone, for request
/// `stretch_id`, in Matroska, its 0 at `start`. Returns its path.
fn download_section(
    vod: &Vod,
    stretch_id: i64,
    (start, end): (f64, f64),
    tools: &Tools,
    paths: &Paths,
    cancel: &AtomicBool,
    report: &mut impl FnMut(Status, f64),
) -> Result<PathBuf> {
    let yt_dlp = tools.yt_dlp.context(fetch::NO_YT_DLP)?;
    let info = fresh_info(yt_dlp, vod, paths, cancel)?;
    let selector = fetch::audio_selector();
    let wanted = fetch::Wanted::Section {
        selector: &selector,
        ffmpeg: tools.ffmpeg,
        start,
        end,
        matroska: true,
        sound_only: true,
    };
    let out = paths.source_section_stem(vod.id, stretch_id);
    fetch::download_or_reprobe(yt_dlp, &vod.info.url, &info, &out, wanted, cancel, |f| report(Status::Downloading, f))
}

/// Downloads a VOD's whole audio, resuming a partial download, or, with
/// `video`, its video to keep (sites without their own player). Returns its path.
fn download(
    db: &mut Db,
    vod: &Vod,
    tools: &Tools,
    paths: &Paths,
    video: Option<(&crate::playable::Playable, u32)>,
    cancel: &AtomicBool,
    mut report: impl FnMut(Status, f64),
) -> Result<PathBuf> {
    let keep = video.filter(|_| keeps_video(&vod.info.platform));
    let yt_dlp = tools.yt_dlp.context(fetch::NO_YT_DLP)?;
    let info = fresh_info(yt_dlp, vod, paths, cancel)?;
    let selector = keep.map(|(playable, height)| playable.selector(height));
    let (out, wanted) = match &selector {
        Some(selector) => {
            fs::create_dir_all(paths.media())?;
            (paths.source_video_stem(vod.id), fetch::Wanted::Video { selector, ffmpeg: tools.ffmpeg })
        }
        None => (paths.source_audio_stem(vod.id), fetch::Wanted::Audio),
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
    let moved = paths.source_audio(vod.id, &ext);
    fs::rename(&file, &moved)?;
    Ok(moved)
}

/// The sites whose own players the app embeds. The page's hasSitePlayer
/// (platform.ts) is checked to list the same.
pub const SITE_PLAYERS: [&str; 3] = ["youtube", "twitch", "kick"];

/// Whether a VOD from `platform` has its video kept to play here: any site but
/// those with embeds of their own (and files on this computer, played in place).
pub fn keeps_video(platform: &str) -> bool {
    !SITE_PLAYERS.contains(&platform) && platform != crate::local::PLATFORM
}

/// The VOD a work or media file belongs to: `<id>.…` (the work dir, and
/// videos kept before 0.4.0) or `<title> [<id>]` followed by its extension
/// or a download's part suffixes (`Paths::source_video_stem`).
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

/// Removes files in `Paths::media` (named `<vod id>.…` or `<vod id> …`) of VODs not in `library`:
/// left by a crash, say, mid-way through removing one. Files of VODs still
/// there stay, as a paused download carries on from its parts.
pub fn tidy_media(paths: &Paths, library: &[i64]) {
    let Ok(entries) = fs::read_dir(paths.media()) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let id = owner(&name);
        if id.is_some_and(|id| !library.contains(&id)) && entry.path().is_file() {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Fills in the channel's login and page of `vods` added before the library
/// kept them, from the info JSON still kept for them (`Paths::source_info`;
/// one is removed once its VOD is transcribed): only the VOD's own (the same
/// site and video), and only what's missing. Nothing kept, nothing known.
pub fn fill_channels(db: &Db, paths: &Paths, vods: &[Vod]) {
    let lacking = vods
        .iter()
        .filter(|v| v.source_path.is_none() && (v.info.channel_login.is_none() || v.info.channel_url.is_none()));
    for vod in lacking {
        let Ok(raw) = fs::read(paths.source_info(vod.id)) else { continue };
        let Ok(info) =
            serde_json::from_slice(&raw).map_err(anyhow::Error::from).and_then(|json| fetch::parse_info(&json))
        else {
            continue;
        };
        if (info.platform.as_str(), info.platform_id.as_str())
            != (vod.info.platform.as_str(), vod.info.platform_id.as_str())
        {
            continue;
        }
        let _ = db.fill_channel(vod.id, info.channel_login.as_deref(), info.channel_url.as_deref());
    }
}

/// A site's VOD's "Refresh details" (its row's menu): asks VOD `vod_id`'s
/// site about it again and keeps what the site now names of its channel,
/// its login and page (`Db::refresh_channel`). The site's own API answers
/// first (`channel_by_api`), in under a second; failing that, or for a site
/// with none, yt-dlp does (`fetch::probe`), which takes seconds.
/// Nothing else changes: not its title, length or anything the library has
/// of it besides. The look-up's info JSON goes to a file of its own
/// (`Paths::temp_probe`), removed after, so a download or transcription
/// under way, and any later, find the VOD's own (`Paths::source_info`) as
/// it was. Fails, changing nothing, when the site can't be asked or says
/// the video's gone, or answers with another video. The VOD as it is then.
///
/// Later it may refresh the video's other details from the site too, such
/// as its description, which VODscribe doesn't show yet.
pub fn refresh_details(
    db: &std::sync::Mutex<Db>,
    yt_dlp: &YtDlp,
    paths: &Paths,
    vod_id: i64,
    cancel: &AtomicBool,
) -> Result<Vod> {
    let gone = || anyhow::anyhow!("That VOD is gone.");
    let vod = db.lock().unwrap().vod(vod_id)?.ok_or_else(gone)?;
    if vod.source_path.is_some() || vod.info.platform == crate::local::PLATFORM {
        bail!("A file on this computer has no site to ask.");
    }
    let channel = match channel_by_api(&vod.info, cancel) {
        Some(Ok(channel)) => channel,
        // yt-dlp can't read Kick's VOD links yet: `fetch::probe` would ask
        // it, fail, then ask Kick again.
        Some(Err(e)) if vod.info.platform == "kick" => return Err(e),
        // The site's answer is the same either way, slower: the video gone
        // says so in yt-dlp's words.
        fast => {
            if let Some(Err(e)) = fast
                && cfg!(debug_assertions)
            {
                eprintln!("refresh {} {} by its API: {e:#}", vod.info.platform, vod.info.platform_id);
            }
            fs::create_dir_all(paths.work())?;
            let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_nanos();
            let probe = paths.temp_probe(nanos);
            let probed = fetch::probe(yt_dlp, &vod.info.url, &probe, cancel);
            let _ = fs::remove_file(&probe);
            let info = probed?;
            if (info.platform.as_str(), info.platform_id.as_str())
                != (vod.info.platform.as_str(), vod.info.platform_id.as_str())
            {
                bail!("The site answered with another video, so nothing changed.");
            }
            (info.channel_login, info.channel_url)
        }
    };
    let db = db.lock().unwrap();
    db.refresh_channel(vod_id, channel.0.as_deref(), channel.1.as_deref())?;
    db.vod(vod_id)?.ok_or_else(gone)
}

/// VOD `vod`'s channel, its login and page, from its site's own API, which
/// answers in under a second where yt-dlp takes seconds; each checks it
/// answered for this video. None for a site or a video it can't be asked
/// about, so yt-dlp is. Each gives what yt-dlp would of the channel, or
/// fails, so a refresh keeps the same either way:
/// - Kick (`kick::resolve`): the login, its slug, and no page. Its failure
///   is the refresh's: yt-dlp can't read Kick's VOD links yet.
/// - Twitch, a VOD (`twitch::vod_channel`): the login, and no page, as
///   yt-dlp names none for a Twitch VOD. Not a clip: yt-dlp names whoever
///   clipped it, which isn't kept.
/// - YouTube (`youtube::channel`, oEmbed): the handle and its page. A
///   channel with no plain handle fails, and yt-dlp is asked.
fn channel_by_api(vod: &fetch::VodInfo, cancel: &AtomicBool) -> Option<Result<(Option<String>, Option<String>)>> {
    let id = vod.platform_id.as_str();
    match vod.platform.as_str() {
        "kick" if crate::kick::parse_vod_url(&vod.url).is_some() => {
            Some(crate::kick::resolve(&vod.url).and_then(|(info, _)| {
                anyhow::ensure!(info.platform_id == id, "The site answered with another video, so nothing changed.");
                Ok((info.channel_login, info.channel_url))
            }))
        }
        "twitch" if !crate::twitch::is_clip(id) => {
            Some(crate::twitch::vod_channel(id, cancel).map(|login| (Some(login), None)))
        }
        "youtube" if crate::youtube::is_video_id(id) => {
            Some(crate::youtube::channel(id, cancel).map(|(handle, page)| (Some(handle), Some(page))))
        }
        _ => None,
    }
}

/// Removes VOD `vod_id`'s kept video, and any part of one, from `Paths::media`.
pub fn remove_media(paths: &Paths, vod_id: i64) {
    remove_work_files(&paths.media(), vod_id);
}

/// The file VOD `vod`'s download is being written to, as it is: the biggest
/// of yt-dlp's part files (`fetch::part_files`) at either of `download`'s
/// stems, the kept video's (`Paths::source_video_stem`) and the audio's
/// (`Paths::source_audio_stem`). None when no download has begun, or it has
/// finished.
pub fn partial_download(paths: &Paths, vod: &Vod) -> Option<PathBuf> {
    let stems = [paths.source_video_stem(vod.id), paths.source_audio_stem(vod.id)];
    stems.iter().flat_map(|stem| fetch::part_files(stem)).max_by_key(|(_, len)| *len).map(|(path, _)| path)
}

/// The file VOD `id` is on this computer as, finished or not: the one it
/// plays from (`Db::playable_path`), else its download so far
/// (`partial_download`).
pub fn file_on_disk(db: &Db, paths: &Paths, id: i64) -> Result<Option<PathBuf>> {
    if let Some(path) = db.playable_path(id)? {
        return Ok(Some(path.into()));
    }
    Ok(db.vod(id)?.and_then(|vod| partial_download(paths, &vod)))
}

/// Sets `vod.partial`: whether its download so far is a file to show. Only
/// a download that's begun and not finished is looked for on disk, so a
/// library's list reads a folder for those alone.
pub fn mark_partial(paths: &Paths, vod: &mut Vod) {
    vod.partial = vod.source_path.is_none()
        && !vod.media
        && matches!(vod.status, Status::Queued | Status::Downloading | Status::Paused | Status::Error)
        && partial_download(paths, vod).is_some();
}

/// Audio to transcribe a stretch from: a file, and the VOD's time at its 0
/// (a section's start; 0 for the VOD's whole audio).
pub struct Audio {
    pub path: PathBuf,
    pub offset: f64,
}

/// Transcribes `stretch` of the audio, writing segments to the index as they
/// are produced, in place of the lines it transcribes over, so partial
/// results are searchable during a long run. Picks up where it reached, so
/// a paused or interrupted job resumes.
#[allow(clippy::too_many_arguments)]
pub fn transcribe(
    db: &mut Db,
    vod: &Vod,
    stretch: &Stretch,
    ffmpeg: &Path,
    audio: &Audio,
    transcription: &Transcription,
    cancel: &AtomicBool,
    report: &mut impl FnMut(Status, f64),
    notice: Notice,
) -> Result<()> {
    // A video without sound has nothing to transcribe: done, with no lines.
    // Nothing found at all is a file ffmpeg couldn't read, not a silent one.
    let streams = crate::local::codecs(ffmpeg, &audio.path)?;
    if streams.audio.is_none() && streams.video.is_none() {
        bail!("{} couldn't be read.", tools::shown(&audio.path));
    }
    if streams.audio.is_none() {
        db.set_stretch_model(stretch.id, Some(transcription.worker.model_id))?;
        db.finish_stretch(stretch.id)?;
        report(Status::Done, 1.0);
        return Ok(());
    }
    let attempt = |spec: &WorkerSpec, db: &mut Db, report: &mut dyn FnMut(Status, f64)| {
        run_worker(db, vod, stretch.id, spec, ffmpeg, audio, cancel, report)
    };
    // A paused or interrupted job resumes where it reached, but never after
    // lines from an older model (or from before models were recorded): it
    // starts again from its start, over them.
    if stretch.from() > stretch.start && !stretch.model.as_deref().is_some_and(crate::model::is_current) {
        db.set_reached(stretch.id, None)?;
    }
    let mut used = &transcription.worker;
    db.set_stretch_model(stretch.id, Some(used.model_id))?;
    let replaced = match attempt(used, db, report) {
        Ok(replaced) => replaced,
        Err(e) => {
            let Some(fallback) = &transcription.fallback else { return Err(e) };
            if cancel.load(Ordering::Relaxed) {
                return Err(e);
            }
            eprintln!("GPU transcription failed: {e:#}");
            notice("The graphics card ran into a problem, so the rest was transcribed with the processor.".into());
            used = fallback;
            db.set_stretch_model(stretch.id, Some(used.model_id))?;
            // What the failed run replaced isn't known: counted as some.
            attempt(used, db, report)?.max(1)
        }
    };
    db.finish_stretch(stretch.id)?;
    // The lines transcribed over, out of the search index too.
    if replaced > 0 {
        db.tidy_segments()?;
    }
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

/// Deletes the files in `work_dir` named `prefix` and then a dot or a dash
/// (a download's pieces: `clip-12.info.json`, `clip-12-1000-2000.mp4.part`),
/// not another's whose name only starts the same (`clip-123.info.json`).
pub fn remove_work_prefix(work_dir: &Path, prefix: &str) {
    let Ok(entries) = fs::read_dir(work_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.strip_prefix(prefix).is_some_and(|rest| rest.starts_with(['.', '-'])) && entry.path().is_file() {
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

/// Runs one worker over request `stretch_id`'s stretch of `audio`, from
/// where it reached, writing segments as they stream in over the lines
/// there (`Db::replace_segments`). Returns how many lines it replaced.
#[allow(clippy::too_many_arguments)]
fn run_worker(
    db: &mut Db,
    vod: &Vod,
    stretch_id: i64,
    spec: &WorkerSpec,
    ffmpeg: &Path,
    audio: &Audio,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(Status, f64),
) -> Result<usize> {
    let stretch = db.stretch(stretch_id)?.context("That request is gone.")?;
    let (start, end) = (stretch.from(), stretch.end);
    // Progress within the stretch.
    let length = end.or(vod.info.duration).map(|e| e - stretch.start).filter(|l| *l > 0.0);
    let fraction = |t: f64| length.map_or(0.0, |l| ((t - stretch.start) / l).clamp(0.0, 1.0));
    db.set_stretch_status(stretch_id, Status::Transcribing, fraction(start), None)?;
    report(Status::Transcribing, fraction(start));

    let mut cmd = tools::command(&spec.program);
    cmd.arg("--model").arg(&spec.model_dir).args(["--provider", spec.provider]);
    cmd.args(["--batch", &spec.batch.to_string()]);
    // It stops when its stdin closes (`_app_alive` below).
    cmd.arg("--exit-on-stdin-close");
    // The worker's times are the file's: a section's 0 is its start.
    cmd.arg("--ffmpeg").arg(ffmpeg).args(["--start", &format!("{:.3}", start - audio.offset)]);
    if let Some(end) = end {
        cmd.args(["--end", &format!("{:.3}", end - audio.offset)]);
    }
    cmd.arg(&audio.path);
    if !spec.lib_dirs.is_empty() {
        let var = if cfg!(windows) { "PATH" } else { "LD_LIBRARY_PATH" };
        let existing = std::env::var_os(var).unwrap_or_default();
        // An empty entry would mean the working directory to the loader.
        let paths =
            spec.lib_dirs.iter().cloned().chain(std::env::split_paths(&existing)).filter(|p| !p.as_os_str().is_empty());
        cmd.env(var, std::env::join_paths(paths)?);
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = tools::spawn(&mut cmd).with_context(|| format!("start {}", tools::shown(&spec.program)))?;
    let _tracked = child.track();
    // Never written to: the worker stops when it closes, which it does
    // however this process ends, even where quitting's `stop_tracked` never
    // runs (a crash, a kill) or runs before the worker is tracked.
    let _app_alive = child.stdin.take();

    // sherpa-onnx logs to stderr; keep the tail for error messages.
    let stderr_thread = tools::drain(child.stderr.take().unwrap());

    // Wake regularly while waiting for output, so a stop kills the worker
    // promptly instead of waiting for its next batch.
    let (tx, rx) = mpsc::channel();
    tools::send_lines(child.stdout.take().unwrap(), tx);

    let mut segmenter = Segmenter::new();
    // The old lines from here on are still to be transcribed over.
    let mut over = start;
    let mut replaced = 0;
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
            if let Some(batch_end) = line.end {
                let batch_end = batch_end + audio.offset;
                let words = line
                    .words
                    .into_iter()
                    .map(|w| Word { start: w.start + audio.offset, end: w.end + audio.offset, ..w })
                    .collect();
                let segments = segmenter.push(words);
                // Everything before the words held back is written: a stop
                // here resumes from them, even where they began batches ago
                // (a batch of silence since) and the old lines after them are
                // gone already.
                let reached = segmenter.pending_start().map_or(batch_end, |held| held.max(start));
                replaced += db.replace_segments(vod.id, stretch_id, (over, Some(batch_end)), &segments, reached)?;
                over = over.max(batch_end);
                report(Status::Transcribing, fraction(batch_end));
            }
        }
    })();
    if result.is_err() {
        child.kill_tree();
        // On a stop, the words held back are written too (the lines they
        // replace already went), so it ends, or resumes, where the audio
        // was transcribed to. Failing to (the app quitting) loses nothing:
        // it resumes from them.
        if let Some(held) = segmenter.finish().filter(|_| cancel.load(Ordering::Relaxed)) {
            let _ = db.replace_segments(vod.id, stretch_id, (over, Some(over)), &[held], over);
        }
        // Don't wait for the stderr reader: a process the worker started may
        // still hold the pipe open, and the error at hand is what matters.
        return result.map(|()| replaced);
    }
    let status = child.wait()?;
    let stderr = stderr_thread.join().unwrap_or_default();
    if !status.success() {
        let msg = tools::last_lines(&stderr, 3);
        bail!("{}", if msg.is_empty() { format!("worker exited with {status}") } else { msg });
    }
    let last: Vec<Segment> = segmenter.finish().into_iter().collect();
    let reached = end.unwrap_or_else(|| last.last().map_or(over, |s| s.end.max(over)));
    replaced += db.replace_segments(vod.id, stretch_id, (over, end), &last, reached)?;
    Ok(replaced)
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    use std::time::Instant;

    use super::*;
    use crate::fetch::VodInfo;

    #[test]
    fn videos_are_kept_for_sites_without_players_of_their_own() {
        for platform in SITE_PLAYERS {
            assert!(!keeps_video(platform), "{platform}");
        }
        assert!(!keeps_video(crate::local::PLATFORM), "files are played in place");
        assert!(keeps_video("rumble") && keeps_video("dailymotion"));
    }

    /// The page keeps its own copies of these (it can't ask for them before
    /// drawing): they must say what the app does.
    #[test]
    fn the_page_agrees_on_site_players_and_the_default_height() {
        let page = include_str!("../../../apps/desktop/src/lib/platform.ts");
        let players = SITE_PLAYERS.map(|p| format!("{p:?}")).join(", ");
        assert!(page.contains(&format!("[{players}].includes(vod.platform)")), "hasSitePlayer lists {players}");
        let height = crate::settings::DEFAULT_VIDEO_HEIGHT;
        assert!(page.contains(&format!("export const DEFAULT_HEIGHT = {height};")), "DEFAULT_HEIGHT is {height}");
    }

    #[test]
    fn work_files_go_by_their_prefix_only() {
        let dir = crate::TempDir::new("prefix");
        for name in ["clip-12.info.json", "clip-12-1000-2000.mp4.part", "clip-123.info.json", "clip-12x"] {
            fs::write(dir.join(name), b"").unwrap();
        }
        remove_work_prefix(&dir, "clip-12");
        let mut left: Vec<_> =
            fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["clip-123.info.json", "clip-12x"]);
    }

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
        let stretch = db.request(vod.id, 0.0, None).unwrap();
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
        let mut report = |s, _| seen.push(s);
        let audio = Audio { path: file, offset: 0.0 };
        transcribe(&mut db, &vod, &stretch, &ffmpeg, &audio, &transcription, &cancel, &mut report, &mut notice)
            .unwrap();
        assert_eq!(seen, [Status::Done]);
        let vod = db.vod(vod.id).unwrap().unwrap();
        assert_eq!((vod.status, vod.transcribed), (Status::Done, crate::stretches::Transcribed::All));
        assert_eq!(db.transcribed_until(vod.id).unwrap(), 0.0, "no lines");
    }

    #[test]
    fn fills_in_a_channel_from_the_info_kept() {
        let root = crate::TempDir::new("fill-channels");
        let paths = Paths::new(root.to_path_buf());
        fs::create_dir_all(paths.work()).unwrap();
        let db = Db::open(Path::new(":memory:")).unwrap();
        let add = |id: &str| {
            let info = VodInfo { platform: "youtube".into(), ..crate::edits::tests::info(id) };
            db.add_vod(&info).unwrap().0
        };
        let (kept, other, none) = (add("aaa"), add("bbb"), add("ccc"));
        let info = |id: &str| {
            serde_json::json!({ "id": id, "extractor_key": "Youtube", "webpage_url": format!("https://www.youtube.com/watch?v={id}"),
                "uploader_id": "@quill", "uploader_url": "https://www.youtube.com/@quill" })
            .to_string()
        };
        fs::write(paths.source_info(kept.id), info("aaa")).unwrap();
        // Another video's info under this VOD's name isn't its.
        fs::write(paths.source_info(other.id), info("zzz")).unwrap();
        fill_channels(&db, &paths, &db.vods().unwrap());
        let channel = |id: i64| {
            let info = db.vod(id).unwrap().unwrap().info;
            (info.channel_login, info.channel_url)
        };
        assert_eq!(channel(kept.id), (Some("@quill".into()), Some("https://www.youtube.com/@quill".into())));
        assert_eq!(channel(other.id), (None, None));
        assert_eq!(channel(none.id), (None, None), "nothing kept, nothing known");
    }

    /// Refresh details takes the channel's login and page from what the
    /// site says now (a stand-in yt-dlp answering with `answer` beside it),
    /// only for the VOD's own video, and only what's named; the VOD's own
    /// info JSON, which the pipeline reads, stays as it was, and nothing of
    /// the look-up is left in the work folder.
    #[test]
    fn refreshing_details_takes_the_channel_the_site_names() {
        let root = crate::TempDir::new("refresh-details");
        let paths = Paths::new(root.join("data"));
        fs::create_dir_all(paths.work()).unwrap();
        let answer = root.join("answer.json");
        let path = root.join("yt-dlp");
        fs::write(&path, format!("#!/bin/sh\ncat '{}' || exit 1\n", answer.display())).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        let yt_dlp = YtDlp { path, js_runtime: None };
        let db = std::sync::Mutex::new(Db::open(Path::new(":memory:")).unwrap());
        let info = VodInfo {
            platform: "youtube".into(),
            url: "https://www.youtube.com/watch?v=aaa".into(),
            title: "As added".into(),
            ..crate::edits::tests::info("aaa")
        };
        let vod = db.lock().unwrap().add_vod(&info).unwrap().0;
        // What the pipeline found when it was added, and reads for a download.
        fs::write(paths.source_info(vod.id), b"{\"kept\":true}").unwrap();
        let say = |id: &str, channel: serde_json::Value| {
            let mut json = serde_json::json!({ "id": id, "extractor_key": "Youtube", "title": "Renamed since",
                "webpage_url": format!("https://www.youtube.com/watch?v={id}"), "duration": 99.0 });
            json.as_object_mut().unwrap().extend(channel.as_object().unwrap().clone());
            fs::write(&answer, json.to_string()).unwrap();
        };
        let no = AtomicBool::new(false);
        let refresh = || refresh_details(&db, &yt_dlp, &paths, vod.id, &no);
        let channel = || {
            let info = db.lock().unwrap().vod(vod.id).unwrap().unwrap().info;
            (info.channel_login, info.channel_url)
        };

        // Nothing named, nothing guessed: what's kept stays as it is.
        say("aaa", serde_json::json!({ "channel": "Quill" }));
        refresh().unwrap();
        assert_eq!(channel(), (None, None));

        // Another video's info isn't this VOD's.
        say("zzz", serde_json::json!({ "uploader_id": "@other", "uploader_url": "https://www.youtube.com/@other" }));
        assert!(refresh().unwrap_err().to_string().contains("another video"));
        assert_eq!(channel(), (None, None));

        // Its own: the channel's login and page, and nothing else of it.
        say("aaa", serde_json::json!({ "uploader_id": "@quill", "uploader_url": "https://www.youtube.com/@quill" }));
        let refreshed = refresh().unwrap();
        assert_eq!(
            (refreshed.info.channel_login.as_deref(), refreshed.info.channel_url.as_deref()),
            (Some("@quill"), Some("https://www.youtube.com/@quill"))
        );
        assert_eq!((refreshed.info.title.as_str(), refreshed.info.duration), ("As added", info.duration));

        // A page off the VOD's site isn't one; the login named still is. And
        // what the site no longer names stays.
        say("aaa", serde_json::json!({ "uploader_id": "@quill2", "uploader_url": "https://evil.example/@quill" }));
        refresh().unwrap();
        assert_eq!(channel(), (Some("@quill2".into()), Some("https://www.youtube.com/@quill".into())));

        // The site can't be asked: an error, and nothing changes.
        fs::remove_file(&answer).unwrap();
        assert!(refresh().is_err());
        assert_eq!(channel(), (Some("@quill2".into()), Some("https://www.youtube.com/@quill".into())));

        assert_eq!(fs::read(paths.source_info(vod.id)).unwrap(), b"{\"kept\":true}");
        let left: Vec<_> = fs::read_dir(paths.work()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, [paths.source_info(vod.id).file_name().unwrap()], "nothing of the look-ups left");

        // A file on this computer has no site to ask.
        let file = VodInfo { platform: crate::local::PLATFORM.into(), ..crate::edits::tests::info("f") };
        let file = db.lock().unwrap().add_vod(&file).unwrap().0;
        assert!(refresh_details(&db, &yt_dlp, &paths, file.id, &no).is_err());
    }

    #[test]
    fn tidies_media_of_vods_gone() {
        let root = crate::TempDir::new("tidy-media");
        let paths = Paths::new(root.to_path_buf());
        let media = paths.media();
        fs::create_dir_all(&media).unwrap();
        for name in
            ["3.mp4", "A title [4].mp4.part-Frag7.part", "29.mp4.part-Frag65.part", "Gone [30].m4a", "notes.txt", "77"]
        {
            fs::write(media.join(name), b"x").unwrap();
        }
        // Folders stay, even named as a VOD's file would be.
        fs::create_dir_all(media.join("31.mp4")).unwrap();
        fs::create_dir_all(media.join("Folder [32]")).unwrap();
        tidy_media(&paths, &[3, 4]);
        let mut left: Vec<String> =
            fs::read_dir(&media).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["3.mp4", "31.mp4", "77", "A title [4].mp4.part-Frag7.part", "Folder [32]", "notes.txt"]);
        // And a VOD's own go, named either way, but not another's with its digits.
        fs::write(media.join("Version 3.1 [3].f137.mp4"), b"x").unwrap();
        fs::write(media.join("Other [34].mp4"), b"x").unwrap();
        remove_media(&paths, 3);
        let mut left: Vec<String> =
            fs::read_dir(&media).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(
            left,
            ["31.mp4", "77", "A title [4].mp4.part-Frag7.part", "Folder [32]", "Other [34].mp4", "notes.txt"]
        );
    }

    #[test]
    fn a_download_so_far_is_its_biggest_part_file() {
        let root = crate::TempDir::new("partial");
        let paths = Paths::new(root.to_path_buf());
        let (work, media) = (paths.work(), paths.media());
        fs::create_dir_all(&media).unwrap();
        let db = Db::open(&root.join("library.sqlite")).unwrap();
        let info = |id: &str| VodInfo {
            platform: "youtube".into(),
            platform_id: id.into(),
            url: format!("https://www.youtube.com/watch?v={id}"),
            title: "A talk".into(),
            channel: None,
            channel_login: None,
            channel_url: None,
            duration: Some(60.0),
            thumbnail: None,
            published: None,
            started_at: None,
        };
        let (vod, _) = db.add_vod(&info("one")).unwrap();
        let (other, _) = db.add_vod(&info("two")).unwrap();
        let stretch = db.request(vod.id, 0.0, None).unwrap();
        db.set_stretch_status(stretch.id, Status::Paused, 0.3, None).unwrap();
        let listed = |db: &Db, id: i64| {
            let mut vod = db.vod(id).unwrap().unwrap();
            mark_partial(&paths, &mut vod);
            vod
        };

        // Nothing begun: no file, nothing to show.
        assert_eq!(file_on_disk(&db, &paths, vod.id).unwrap(), None);
        assert!(!listed(&db, vod.id).partial);

        // The audio's parts in the work dir and a kept video's in media: the
        // biggest, as it is; another VOD's (its id a prefix of none) aren't.
        let id = vod.id;
        fs::create_dir_all(&work).unwrap();
        fs::write(work.join(format!("{id}.f140.m4a.part")), [0; 10]).unwrap();
        fs::write(media.join(format!("{id}.f137.mp4.part")), [0; 30]).unwrap();
        fs::write(work.join(format!("{id}.info.json")), [0; 99]).unwrap();
        fs::write(work.join(format!("{id}0.m4a.part")), [0; 99]).unwrap();
        fs::write(work.join(format!("{}.m4a.part", other.id)), [0; 99]).unwrap();
        let shown = media.join(format!("{id}.f137.mp4.part"));
        assert_eq!(file_on_disk(&db, &paths, id).unwrap(), Some(shown));
        assert!(listed(&db, id).partial);
        fs::write(work.join(format!("{id}.f251.webm.part")), [0; 50]).unwrap();
        assert_eq!(file_on_disk(&db, &paths, id).unwrap(), Some(work.join(format!("{id}.f251.webm.part"))));

        // Not asked for (the other VOD), its parts aren't looked for in the list.
        assert!(!listed(&db, other.id).partial);

        // Kept, it's the kept file, and the list doesn't call it partial.
        let kept = media.join(format!("{id}.mp4"));
        fs::write(&kept, b"x").unwrap();
        db.set_media_path(id, Some(&kept.to_string_lossy())).unwrap();
        assert_eq!(file_on_disk(&db, &paths, id).unwrap(), Some(kept));
        assert!(!listed(&db, id).partial);
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
        assert_eq!(owner("clip-12-1000-5000.mp4"), None);
        assert_eq!(owner("video-12.info.json"), None);
        assert_eq!(owner("notes.txt"), None);
        assert_eq!(owner("A [b] [x].mp4"), None);
    }

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
            channel_login: None,
            channel_url: None,
            duration: Some(1.0),
            thumbnail: None,
            published: None,
            started_at: None,
        };
        let vod = db.add_vod(&info).unwrap().0;
        db.set_source_path(vod.id, &source.to_string_lossy()).unwrap();
        let vod = db.vod(vod.id).unwrap().unwrap();
        let stretch = db.request(vod.id, 0.0, None).unwrap();

        let yt_dlp = YtDlp { path: "/nonexistent/yt-dlp".into(), js_runtime: None };
        let tools = Tools { yt_dlp: Some(&yt_dlp), ffmpeg: Path::new("/nonexistent/ffmpeg") };
        let no = AtomicBool::new(false);
        let found = audio_for(&mut db, &vod, &stretch, &tools, &paths, None, &no, |_, _| {}).unwrap();
        assert_eq!(found, Source::Whole { path: source.clone(), downloaded: false }, "transcribed in place");
        db.set_audio_path(vod.id, Some(&source.to_string_lossy())).unwrap();

        remove_work_files(&paths.work(), vod.id);
        paths.clear_cache().unwrap();
        db.forget_downloads().unwrap();
        db.delete_vod(vod.id).unwrap();
        assert_eq!(fs::read(&source).unwrap(), b"the user's video");

        fs::remove_file(&source).unwrap();
        assert!(audio_for(&mut db, &vod, &stretch, &tools, &paths, None, &no, |_, _| {}).is_err());
    }

    /// A VOD whose video is kept is transcribed from that file, with nothing
    /// downloaded (the tools here don't exist), until the file is gone.
    #[test]
    fn a_kept_video_is_used_rather_than_downloaded_again() {
        let root = crate::TempDir::new("kept");
        let paths = crate::Paths::new(root.join("data"));
        paths.ensure().unwrap();
        let mut db = Db::open(&paths.db()).unwrap();
        let info = VodInfo {
            platform: "rumble".into(),
            platform_id: "v1".into(),
            url: "https://rumble.com/v1-x.html".into(),
            title: "x".into(),
            channel: None,
            channel_login: None,
            channel_url: None,
            duration: Some(1.0),
            thumbnail: None,
            published: None,
            started_at: None,
        };
        let vod = db.add_vod(&info).unwrap().0;
        let stretch = db.request(vod.id, 0.0, None).unwrap();
        let kept = paths.source_video(vod.id, "mp4");
        fs::create_dir_all(kept.parent().unwrap()).unwrap();
        fs::write(&kept, b"video").unwrap();
        db.set_media_path(vod.id, Some(&kept.to_string_lossy())).unwrap();
        let yt_dlp = YtDlp { path: "/nonexistent/yt-dlp".into(), js_runtime: None };
        let tools = Tools { yt_dlp: Some(&yt_dlp), ffmpeg: Path::new("/nonexistent/ffmpeg") };
        let no = AtomicBool::new(false);
        let found = audio_for(&mut db, &vod, &stretch, &tools, &paths, None, &no, |_, _| {}).unwrap();
        assert_eq!(found, Source::Whole { path: kept.clone(), downloaded: false });
        fs::remove_file(&kept).unwrap();
        assert!(audio_for(&mut db, &vod, &stretch, &tools, &paths, None, &no, |_, _| {}).is_err(), "downloads again");
    }

    /// A "yt-dlp" that writes its arguments to `args` beside it, and
    /// downloads (a file saying so) unless asked for a section and `sections` is false.
    fn fake_yt_dlp(dir: &Path, sections: bool) -> YtDlp {
        let path = dir.join(if sections { "yt-dlp-sections" } else { "yt-dlp-whole" });
        let refuse = if sections { "" } else { "--download-sections) echo 'ERROR: no' >&2; exit 1;;" };
        let script = format!(
            "#!/bin/sh\necho \"$@\" > '{args}'\nprev=\nfor a in \"$@\"; do\n  case \"$a\" in {refuse} esac\n  \
             [ \"$prev\" = -o ] && out=$a\n  prev=$a\ndone\nfile=$(echo \"$out\" | sed 's/%(ext)s/m4a/')\n\
             echo audio > \"$file\"\necho \"[vodscribe-file]$file\"\n",
            args = dir.join("args").display()
        );
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        YtDlp { path, js_runtime: None }
    }

    /// A stretch gets a section of its own, its 0 at its start; if that
    /// fails, the whole audio, which then serves the VOD's other stretches;
    /// a request for all of it gets the whole audio straight away.
    #[test]
    fn a_stretch_downloads_its_section_or_else_the_whole_audio() {
        let root = crate::TempDir::new("sections");
        let paths = crate::Paths::new(root.join("data"));
        paths.ensure().unwrap();
        let work = paths.work();
        fs::create_dir_all(&work).unwrap();
        let mut db = Db::open(&paths.db()).unwrap();
        let info = VodInfo {
            platform: "twitch".into(),
            platform_id: "v1".into(),
            url: "https://www.twitch.tv/videos/1".into(),
            title: "x".into(),
            channel: None,
            channel_login: None,
            channel_url: None,
            duration: Some(600.0),
            thumbnail: None,
            published: None,
            started_at: None,
        };
        let vod = db.add_vod(&info).unwrap().0;
        // Probed a moment ago: not probed again.
        fs::write(paths.source_info(vod.id), b"{}").unwrap();
        let no = AtomicBool::new(false);
        let ffmpeg = Path::new("/nonexistent/ffmpeg");
        let args = || fs::read_to_string(root.join("args")).unwrap();

        let sections = fake_yt_dlp(&root, true);
        let tools = Tools { yt_dlp: Some(&sections), ffmpeg };
        let part = db.request(vod.id, 100.0, Some(130.0)).unwrap();
        let found = audio_for(&mut db, &vod, &part, &tools, &paths, None, &no, |_, _| {}).unwrap();
        let section = work.join(format!("{}.s{}.m4a", vod.id, part.id));
        assert_eq!(found, Source::Section { path: section, start: 100.0 });
        assert!(args().contains("--download-sections *100.000-130.000"), "{}", args());
        assert!(args().contains("ffmpeg_o:-ss 0 -f matroska -vn"), "{}", args());

        let whole = db.request(vod.id, 0.0, None).unwrap();
        let found = audio_for(&mut db, &vod, &whole, &tools, &paths, None, &no, |_, _| {}).unwrap();
        assert_eq!(found, Source::Whole { path: work.join(format!("{}.m4a", vod.id)), downloaded: true });
        assert!(!args().contains("--download-sections"), "all of it: {}", args());
        fs::remove_file(work.join(format!("{}.m4a", vod.id))).unwrap();

        let refused = fake_yt_dlp(&root, false);
        let tools = Tools { yt_dlp: Some(&refused), ffmpeg };
        let found = audio_for(&mut db, &vod, &part, &tools, &paths, None, &no, |_, _| {}).unwrap();
        let downloaded = work.join(format!("{}.m4a", vod.id));
        assert_eq!(found, Source::Whole { path: downloaded.clone(), downloaded: true }, "the section failed");
        db.set_audio_path(vod.id, Some(&downloaded.to_string_lossy())).unwrap();
        let none = YtDlp { path: "/nonexistent/yt-dlp".into(), js_runtime: None };
        let tools = Tools { yt_dlp: Some(&none), ffmpeg };
        let other = db.request(vod.id, 300.0, Some(310.0)).unwrap();
        let found = audio_for(&mut db, &vod, &other, &tools, &paths, None, &no, |_, _| {}).unwrap();
        assert_eq!(found, Source::Whole { path: downloaded, downloaded: false }, "already here");
    }

    /// A stretch of the Twitch test VOD (67 s long) downloads as a section
    /// of its own, sound alone, its 0 at the stretch's start. `VODSCRIBE_BIN`
    /// is a folder with yt-dlp (and ffmpeg, or one on PATH).
    #[test]
    #[ignore = "network"]
    fn a_twitch_stretch_downloads_as_a_section_live() {
        let bin = std::env::var("VODSCRIBE_BIN").expect("VODSCRIBE_BIN: the folder with yt-dlp");
        let yt_dlp = tools::find_yt_dlp(Path::new(&bin)).expect("yt-dlp");
        let ffmpeg = tools::find_ffmpeg(Path::new(&bin)).expect("ffmpeg");
        let root = crate::TempDir::new("section-live");
        let paths = crate::Paths::new(root.join("data"));
        paths.ensure().unwrap();
        let work = paths.work();
        fs::create_dir_all(&work).unwrap();
        let mut db = Db::open(&paths.db()).unwrap();
        let no = AtomicBool::new(false);
        let probe = root.join("probe.json");
        let info = fetch::probe(&yt_dlp, "https://www.twitch.tv/videos/2901646294", &probe, &no).unwrap();
        let vod = db.add_vod(&info).unwrap().0;
        fs::rename(&probe, paths.source_info(vod.id)).unwrap();
        let stretch = db.request(vod.id, 30.0, Some(50.0)).unwrap();
        let tools = Tools { yt_dlp: Some(&yt_dlp), ffmpeg: &ffmpeg };
        let started = Instant::now();
        let found = audio_for(&mut db, &vod, &stretch, &tools, &paths, None, &no, |_, _| {}).unwrap();
        let Source::Section { path, start } = found else { panic!("not a section: {found:?}") };
        eprintln!("{} in {:?}", path.display(), started.elapsed());
        assert_eq!(start, 30.0);
        let streams = crate::local::codecs(&ffmpeg, &path).unwrap();
        assert!(streams.audio.is_some() && streams.video.is_none(), "{streams:?}");
        let mut samples = 0;
        crate::audio::decode(&ffmpeg, &path, 0.0, None, &no, |s| {
            samples += s.len();
            Ok(())
        })
        .unwrap();
        let secs = samples as f64 / crate::audio::SAMPLE_RATE as f64;
        eprintln!("{secs:.2} s of audio");
        assert!((secs - 20.0).abs() < 1.5, "{secs} s");
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
            channel_login: None,
            channel_url: None,
            duration: Some(60.0),
            thumbnail: None,
            published: None,
            started_at: None,
        };
        let (vod, _) = db.add_vod(&info).unwrap();
        let stretch = db.request(vod.id, 0.0, None).unwrap();
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
        let result = run_worker(
            &mut db,
            &vod,
            stretch.id,
            &spec,
            Path::new("ffmpeg"),
            &Audio { path: "audio".into(), offset: 0.0 },
            &cancel,
            &mut |_, _| {},
        );
        assert!(result.is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "took {:?}", started.elapsed());
    }

    /// A "worker" that writes its arguments to `args` beside it, prints
    /// `lines` (whatever it's asked), then waits `then` seconds.
    fn fake_worker(dir: &Path, lines: &[String], then: u32) -> WorkerSpec {
        let program = dir.join(format!("worker-{}.sh", lines.len()));
        let mut script = format!("#!/bin/sh\necho \"$@\" > '{}'\n", dir.join("args").display());
        for line in lines {
            script += &format!("echo '{line}'\n");
        }
        script += &format!("sleep {then}\n");
        fs::write(&program, script).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        WorkerSpec {
            program,
            model_id: crate::model::CPU_MODEL.id,
            model_dir: dir.to_path_buf(),
            provider: "cpu",
            batch: 1,
            lib_dirs: Vec::new(),
        }
    }

    /// A worker's batch: the audio transcribed up to `end`, and `words` as (text, start).
    fn batch(end: f64, words: &[(&str, f64)]) -> String {
        let words: Vec<Word> =
            words.iter().map(|&(text, start)| Word { text: text.into(), start, end: start + 0.3 }).collect();
        serde_json::json!({ "end": end, "words": words }).to_string()
    }

    fn texts(db: &Db, vod: i64) -> Vec<String> {
        db.segments(vod).unwrap().into_iter().map(|s| s.text).collect()
    }

    fn line(text: &str, start: f64, end: f64) -> crate::transcript::Segment {
        crate::transcript::Segment { start, end, text: text.into(), words: Vec::new() }
    }

    /// A stretch widens to the lines its edges cut through, and its lines
    /// replace the old ones as they come: stopped part-way, the old lines
    /// beyond where it got stay; ended, it's done as far as it got.
    #[test]
    fn a_stretch_snaps_to_lines_and_replaces_them_as_it_goes() {
        let dir = crate::TempDir::new("replace");
        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let info = VodInfo {
            platform: "youtube".into(),
            platform_id: "x".into(),
            url: "u".into(),
            title: "t".into(),
            channel: None,
            channel_login: None,
            channel_url: None,
            duration: Some(60.0),
            thumbnail: None,
            published: None,
            started_at: None,
        };
        let (vod, _) = db.add_vod(&info).unwrap();
        let old =
            [line("old a", 0.0, 5.0), line("old b", 8.0, 12.0), line("old c", 20.0, 25.0), line("old d", 40.0, 45.0)];
        db.insert_segments(vod.id, &old).unwrap();
        let whole = db.request(vod.id, 0.0, None).unwrap().id;
        db.set_stretch_model(whole, Some(crate::model::CPU_MODEL.id)).unwrap();
        db.finish_stretch(whole).unwrap();

        // 10-22 cuts through "old b" and "old c": it's 8-25.
        let ask = db.request(vod.id, 10.0, Some(22.0)).unwrap().id;
        let snapped = db.snap_stretch(ask).unwrap();
        assert_eq!((snapped.start, snapped.end), (8.0, Some(25.0)));
        db.set_stretch_model(ask, Some(crate::model::CPU_MODEL.id)).unwrap();
        // One batch to 16 s, a line and two words held back; then it's stopped.
        let first = batch(16.0, &[("New", 9.0), ("first", 9.3), ("line.", 9.6), ("And", 14.0), ("more", 14.5)]);
        let spec = fake_worker(&dir, &[first], 30);
        // Stopped once that batch is in, not after a fixed wait: a script just
        // written may take most of a second to start on macOS.
        let cancel = AtomicBool::new(false);
        let audio = Audio { path: "audio".into(), offset: 0.0 };
        let stopped = run_worker(&mut db, &vod, ask, &spec, Path::new("ffmpeg"), &audio, &cancel, &mut |_, done| {
            if done > 0.0 {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert!(stopped.is_err());
        let args = fs::read_to_string(dir.join("args")).unwrap();
        assert!(args.contains("--exit-on-stdin-close"), "{args}");
        assert_eq!(
            args.trim().split(' ').skip(9).collect::<Vec<_>>(),
            ["--start", "8.000", "--end", "25.000", "audio"]
        );
        // "old b" went with the first batch; "old c", beyond it, stays; the
        // words held back are written at the stop.
        assert_eq!(texts(&db, vod.id), ["old a", "New first line.", "And more", "old c", "old d"]);
        assert_eq!(db.stretch(ask).unwrap().unwrap().reached, Some(16.0));
        // Ended there: done to 16 s, one with the rest by the same model.
        db.end_request(ask).unwrap();
        let done: Vec<_> = db.stretches(vod.id).unwrap().into_iter().map(|s| (s.start, s.end, s.status)).collect();
        assert_eq!(done, [(0.0, None, Status::Done)]);

        // Again, to the end of it: what it wrote before is replaced too.
        let again = db.request(vod.id, 8.0, Some(25.0)).unwrap().id;
        db.snap_stretch(again).unwrap();
        let spec = fake_worker(&dir, &[batch(25.0, &[("Said", 10.0), ("over", 10.5), ("again.", 11.0)])], 0);
        let no = AtomicBool::new(false);
        let replaced =
            run_worker(&mut db, &vod, again, &spec, Path::new("ffmpeg"), &audio, &no, &mut |_, _| {}).unwrap();
        assert_eq!(replaced, 3);
        assert_eq!(texts(&db, vod.id), ["old a", "Said over again.", "old d"]);
    }

    /// Words held back for the next line, then a batch with none (silence):
    /// the old lines there went with it, so a run that fails then (the GPU
    /// running out, say, before the processor takes over) carries on from
    /// the held words, not past them, or they'd never be written.
    #[test]
    fn a_failed_run_resumes_from_the_words_it_held_back() {
        let dir = crate::TempDir::new("held-back");
        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let info = VodInfo {
            platform: "youtube".into(),
            platform_id: "x".into(),
            url: "u".into(),
            title: "t".into(),
            channel: None,
            channel_login: None,
            channel_url: None,
            duration: Some(60.0),
            thumbnail: None,
            published: None,
            started_at: None,
        };
        let (vod, _) = db.add_vod(&info).unwrap();
        db.insert_segments(vod.id, &[line("old a", 0.0, 5.0), line("old b", 15.0, 17.0)]).unwrap();
        let ask = db.request(vod.id, 0.0, None).unwrap().id;
        db.set_stretch_model(ask, Some(crate::model::CPU_MODEL.id)).unwrap();
        let program = dir.join("failing-worker.sh");
        let script = format!(
            "#!/bin/sh\necho '{}'\necho '{}'\necho 'out of memory' >&2\nexit 1\n",
            batch(20.0, &[("A", 2.0), ("line.", 2.5), ("Held", 15.0), ("back", 15.5)]),
            batch(30.0, &[])
        );
        fs::write(&program, script).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let spec = WorkerSpec { program, ..fake_worker(&dir, &[], 0) };
        let audio = Audio { path: "audio".into(), offset: 0.0 };
        let no = AtomicBool::new(false);
        assert!(run_worker(&mut db, &vod, ask, &spec, Path::new("ffmpeg"), &audio, &no, &mut |_, _| {}).is_err());
        assert_eq!(texts(&db, vod.id), ["A line."], "\"old b\" went with the first batch");
        assert_eq!(db.stretch(ask).unwrap().unwrap().reached, Some(15.0), "from the words held back");
    }

    /// A section's words are in its own time, from 0: they're moved to the
    /// VOD's, and the worker is asked for the stretch in the section's.
    #[test]
    fn a_sections_lines_are_in_the_vods_time() {
        let dir = crate::TempDir::new("section-offset");
        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let info = VodInfo {
            platform: "twitch".into(),
            platform_id: "x".into(),
            url: "u".into(),
            title: "t".into(),
            channel: None,
            channel_login: None,
            channel_url: None,
            duration: Some(600.0),
            thumbnail: None,
            published: None,
            started_at: None,
        };
        let (vod, _) = db.add_vod(&info).unwrap();
        let ask = db.request(vod.id, 100.0, Some(130.0)).unwrap().id;
        let spec = fake_worker(&dir, &[batch(30.0, &[("Hello", 1.0), ("there", 2.0), ("friend.", 3.0)])], 0);
        let audio = Audio { path: "section.mka".into(), offset: 100.0 };
        let mut seen = Vec::new();
        let no = AtomicBool::new(false);
        run_worker(&mut db, &vod, ask, &spec, Path::new("ffmpeg"), &audio, &no, &mut |_, p| seen.push(p)).unwrap();
        let args = fs::read_to_string(dir.join("args")).unwrap();
        assert!(args.contains("--start 0.000 --end 30.000 section.mka"), "{args}");
        let segments = db.segments(vod.id).unwrap();
        assert_eq!((segments[0].start, segments[0].words[2].start), (101.0, 103.0));
        assert_eq!(seen, [0.0, 1.0], "progress within the stretch");
    }
}
