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

/// The VODs that files in `dir` (the work or media folder) belong to, as
/// `remove_work_files` finds them.
pub fn file_owners(dir: &Path) -> std::collections::HashSet<i64> {
    let Ok(entries) = fs::read_dir(dir) else { return Default::default() };
    entries.flatten().filter_map(|e| owner(&e.file_name().to_string_lossy())).collect()
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
mod tests;
