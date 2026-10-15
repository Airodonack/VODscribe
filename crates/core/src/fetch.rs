use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::kick;
use crate::tools::{self, YtDlp};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VodInfo {
    /// "youtube", "twitch", "kick", or yt-dlp's extractor name for other sites.
    pub platform: String,
    pub platform_id: String,
    pub url: String,
    pub title: String,
    pub channel: Option<String>,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
    /// YYYY-MM-DD
    pub published: Option<String>,
}

fn platform_of(extractor_key: &str) -> String {
    let key = extractor_key.to_lowercase();
    for p in ["youtube", "twitch", "kick"] {
        if key.starts_with(p) {
            return p.to_string();
        }
    }
    key
}

pub fn parse_info(json: &Value) -> Result<VodInfo> {
    let s = |k: &str| json.get(k).and_then(Value::as_str).map(str::to_string);
    if s("_type").as_deref() == Some("playlist") {
        bail!("That's a playlist or channel link. Paste a link to a single VOD.");
    }
    if json.get("is_live").and_then(Value::as_bool) == Some(true) {
        bail!("That stream is still live. Add it once the VOD is available.");
    }
    // Digits only: slicing at byte positions would panic inside a multi-byte character.
    let published = s("upload_date")
        .filter(|d| d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit()))
        .map(|d| format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..]));
    let extractor = s("extractor_key").unwrap_or_default();
    // A Twitch clip's "id" is a number of its own, which Twitch's VOD lookups
    // would take for a VOD's; it goes by its slug instead (see `twitch`).
    let id = if extractor == "TwitchClips" { s("display_id").or_else(|| s("id")) } else { s("id") };
    Ok(VodInfo {
        platform: platform_of(&extractor),
        platform_id: id.context("yt-dlp returned no video id")?,
        url: s("webpage_url").or_else(|| s("original_url")).context("yt-dlp returned no URL")?,
        title: s("title").unwrap_or_else(|| "Untitled".into()),
        channel: s("channel").or_else(|| s("uploader")),
        duration: json.get("duration").and_then(Value::as_f64),
        thumbnail: s("thumbnail"),
        published,
    })
}

/// Fetches metadata and saves yt-dlp's info JSON to `info_path`, so the later
/// download reuses it instead of resolving the URL a second time.
/// `cancel` stops it (yt-dlp can take a while to look a link up).
pub fn probe(yt_dlp: &YtDlp, url: &str, info_path: &Path, cancel: &AtomicBool) -> Result<VodInfo> {
    let result = dump_json(yt_dlp.command(), url, cancel).and_then(|(json, raw)| Ok((parse_info(&json)?, raw)));
    let (info, raw) = match result {
        Ok(ok) => ok,
        // yt-dlp can't read Kick's current VOD links yet. Resolve those
        // ourselves and have yt-dlp treat the manifest as a plain HLS stream.
        Err(e) if kick::parse_vod_url(url).is_some() && !crate::is_canceled(&e) => {
            let (info, manifest) = kick::resolve(url).map_err(|kick_err| e.context(kick_err))?;
            // Checked like any link, which leaves a plain host to compare exactly.
            let manifest = crate::net::public_url(&manifest)?;
            if !is_kick_host(&manifest) {
                bail!("Kick answered with a stream somewhere other than Kick.");
            }
            // A plain HLS stream, which only the generic extractor reads; the
            // link is Kick's own answer, not one pasted in.
            (info, dump_json(yt_dlp.command_any_extractor(), &manifest, cancel)?.1)
        }
        Err(e) => return Err(e),
    };
    fs::write(info_path, raw)?;
    Ok(info)
}

/// Whether `url` (as `net::public_url` returns it) is https on kick.com or a subdomain.
fn is_kick_host(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h).split(':').next().unwrap_or_default();
    host == "kick.com" || host.ends_with(".kick.com")
}

fn dump_json(mut cmd: std::process::Command, url: &str, cancel: &AtomicBool) -> Result<(Value, Vec<u8>)> {
    let mut child = cmd
        .args(["-J", "--no-playlist", "--no-warnings", "--", url])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("run yt-dlp")?;
    let read = |mut pipe: Box<dyn std::io::Read + Send>| {
        std::thread::spawn(move || {
            let mut out = Vec::new();
            let _ = pipe.read_to_end(&mut out);
            out
        })
    };
    let stdout = read(Box::new(child.stdout.take().unwrap()));
    let stderr = read(Box::new(child.stderr.take().unwrap()));
    // Polled, so a stop takes effect while yt-dlp is still looking.
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            tools::kill_tree(&mut child);
            return Err(crate::Canceled.into());
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(tools::STOP_POLL);
    };
    let (stdout, stderr) = (stdout.join().unwrap_or_default(), stderr.join().unwrap_or_default());
    if !status.success() {
        bail!("{}", clean_error(&String::from_utf8_lossy(&stderr)));
    }
    let json = serde_json::from_slice(&stdout).context("parse yt-dlp output")?;
    Ok((json, stdout))
}

const PROGRESS: &str = "[vodscribe]";
const FILE: &str = "[vodscribe-file]";

/// What to download: the smallest stream that carries audio, to transcribe;
/// or a video to keep and play here, chosen by a yt-dlp format selection
/// (`playable::Playable::selector`), its streams merged with `ffmpeg`.
#[derive(Clone, Copy)]
pub enum Wanted<'a> {
    Audio,
    Video {
        selector: &'a str,
        ffmpeg: &'a Path,
    },
    /// Only `start`..`end` seconds of a video (a clip), in an MP4.
    Section {
        selector: &'a str,
        ffmpeg: &'a Path,
        start: f64,
        end: f64,
    },
}

/// ffmpeg's arguments for files it reads after a download (merging, fixing
/// up): the app's lists, with picture decoders, which merging needs to read
/// a video's format.
fn ffmpeg_reading_files() -> String {
    format!(
        "ffmpeg_i:-format_whitelist {} -codec_whitelist {},{} -protocol_whitelist file",
        tools::FFMPEG_FORMATS,
        tools::FFMPEG_AUDIO,
        tools::FFMPEG_PICTURE
    )
}

/// Downloads what's `wanted`, reporting a 0..1 fraction. Returns the path of
/// the downloaded file. An interrupted download resumes, and a finished one is
/// reused, when called again with the same `out_stem`.
pub fn download(
    yt_dlp: &YtDlp,
    info_path: &Path,
    out_stem: &Path,
    wanted: Wanted,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(f64),
) -> Result<PathBuf> {
    let template = format!(
        "download:{PROGRESS} %(progress.downloaded_bytes)s %(progress.total_bytes)s \
         %(progress.total_bytes_estimate)s %(progress.fragment_index)s %(progress.fragment_count)s"
    );
    let mut cmd = yt_dlp.command();
    cmd.arg("--load-info-json").arg(info_path);
    match wanted {
        // Audio-only when offered (YouTube, Twitch); otherwise the lowest quality muxed stream.
        // HLS downloads aren't remuxed: only the audio is used, and ffmpeg reads the raw stream.
        Wanted::Audio => cmd.args(["-f", "ba/w", "--fixup", "never"]),
        // Kept to play: video and audio in one MP4, which every webview reads;
        // a single file in a container none read (Flash video, QuickTime) is
        // rewrapped as one.
        // yt-dlp's own ffmpeg runs (merging, fixing up HLS) read what the site
        // sent, so they get the app's lists too, with picture decoders, which
        // merging needs to read a video's format; and HLS is fetched by
        // yt-dlp itself, never by ffmpeg.
        Wanted::Video { selector, ffmpeg } => cmd
            .args(["-f", selector, "--merge-output-format", "mp4", "--remux-video", "flv>mp4/mov>mp4"])
            .args(["--downloader", "m3u8:native"])
            .arg("--postprocessor-args")
            .arg(ffmpeg_reading_files())
            .arg("--ffmpeg-location")
            .arg(ffmpeg),
        // A section is the one download yt-dlp hands to ffmpeg, which seeks in
        // the stream itself. It reads the stream at the URL given only: over
        // https (no local files, nor plain http), and only the formats and
        // codecs the app reads anyway, HLS playlists among them.
        Wanted::Section { selector, ffmpeg, start, end } => cmd
            .args(["-f", selector, "--merge-output-format", "mp4"])
            .arg("--download-sections")
            .arg(format!("*{start:.3}-{end:.3}"))
            .arg("--downloader-args")
            .arg(format!(
                "ffmpeg_i:-protocol_whitelist https,tls,tcp -format_whitelist {},hls -codec_whitelist {},{}",
                tools::FFMPEG_FORMATS,
                tools::FFMPEG_AUDIO,
                tools::FFMPEG_PICTURE
            ))
            .arg("--postprocessor-args")
            .arg(ffmpeg_reading_files())
            .arg("--ffmpeg-location")
            .arg(ffmpeg),
    };
    let mut child = cmd
        .args(["-N", "8", "--no-mtime", "--newline", "--progress"])
        .args(["--progress-template", &template])
        // A runaway download (a stream that never ends) stops instead of filling the disk.
        .args(["--max-filesize", "40G"])
        .args(["--print", &format!("after_move:{FILE}%(filepath)s"), "-o"])
        .arg(format!("{}.%(ext)s", out_stem.display()))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("start yt-dlp")?;

    // Where progress lands depends on yt-dlp's flags, so read both streams.
    let (tx, rx) = mpsc::channel::<String>();
    let readers = [
        tools::send_lines(child.stdout.take().unwrap(), tx.clone()),
        tools::send_lines(child.stderr.take().unwrap(), tx),
    ];

    let mut path = None;
    let mut log = Vec::new();
    loop {
        // Wake regularly so a stop takes effect even while yt-dlp is quiet.
        let line = match rx.recv_timeout(tools::STOP_POLL) {
            Ok(line) => line,
            Err(mpsc::RecvTimeoutError::Timeout) => String::new(),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if cancel.load(Ordering::Relaxed) {
            tools::kill_tree(&mut child);
            return Err(crate::Canceled.into());
        }
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix(PROGRESS) {
            if let Some(f) = parse_progress(rest) {
                on_progress(f);
            }
        } else if let Some(file) = line.strip_prefix(FILE) {
            let file = PathBuf::from(file.trim());
            if file.parent() == out_stem.parent() && file.is_file() {
                path = Some(file);
            }
        } else {
            log.push(line);
        }
    }
    for r in readers {
        let _ = r.join();
    }
    let status = child.wait()?;
    if !status.success() {
        bail!("{}", clean_error(&log.join("\n")));
    }
    // The printed path is the reliable way to find the file; if it got lost,
    // the finished download is still the one file named after `out_stem`.
    path.or_else(|| finished_file(out_stem)).context("yt-dlp finished without producing a file")
}

/// The finished download for `out_stem` (`<stem>.<ext>`), ignoring yt-dlp's
/// partial and metadata files, if there's exactly one.
fn finished_file(out_stem: &Path) -> Option<PathBuf> {
    let (dir, stem) = (out_stem.parent()?, out_stem.file_name()?.to_str()?);
    let mut found = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        p.is_file()
            && name.strip_prefix(stem).and_then(|r| r.strip_prefix('.')).is_some_and(|ext| !ext.contains('.'))
            && !name.ends_with(".part")
            && !name.ends_with(".ytdl")
    });
    let file = found.next()?;
    found.next().is_none().then_some(file)
}

fn parse_progress(rest: &str) -> Option<f64> {
    let v: Vec<Option<f64>> = rest.split_whitespace().map(|s| s.parse().ok()).collect();
    let get = |i: usize| v.get(i).copied().flatten().filter(|x| *x > 0.0);
    let done = get(0);
    if let (Some(d), Some(t)) = (done, get(1).or(get(2))) {
        return Some((d / t).min(1.0));
    }
    // Fragmented (HLS) downloads often only know fragment counts.
    match (get(3), get(4)) {
        (Some(i), Some(n)) => Some((i / n).min(1.0)),
        _ => None,
    }
}

/// Turns yt-dlp's stderr into a message for people: a plain sentence for the
/// common failures, otherwise its ERROR lines (the full log goes to stderr).
pub(crate) fn clean_error(stderr: &str) -> String {
    eprintln!("yt-dlp: {}", tools::last_lines(stderr, 10));
    let errors: Vec<&str> = stderr.lines().filter_map(|l| l.strip_prefix("ERROR: ")).collect();
    let raw = if errors.is_empty() { tools::last_lines(stderr, 3) } else { errors.join("\n") };
    let msg = strip_tag(&raw);
    friendly(msg).map_or_else(|| format!("Couldn't get this video: {msg}"), str::to_string)
}

/// Drops yt-dlp's "[extractor] video-id: " prefix, when the message starts with one.
fn strip_tag(raw: &str) -> &str {
    let Some(rest) = raw.strip_prefix('[').and_then(|r| r.split_once("] ")).map(|(_, rest)| rest) else {
        return raw;
    };
    rest.split_once(": ").filter(|(id, _)| !id.contains(' ')).map_or(rest, |(_, msg)| msg)
}

/// yt-dlp got an HTTP 403 for the media. YouTube ties its media URLs to the
/// session that produced them and sometimes limits a session to tiny ranges,
/// so a download that was refused usually works from a fresh probe.
pub const REFUSED: &str = "The site refused the download. Updating the video downloader in Settings often fixes this.";

/// Whether `e` is the media being refused (`REFUSED`), which a new probe may fix.
pub fn is_refused(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.to_string() == REFUSED)
}

/// A plain sentence for the failures people actually hit, from the message
/// with its "[extractor] id:" prefix already removed.
fn friendly(msg: &str) -> Option<&'static str> {
    let m = msg.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| m.contains(w));
    let status = m.split("http error ").nth(1).and_then(|rest| rest.get(..3)).and_then(|code| code.parse::<u16>().ok());
    Some(match status {
        Some(404 | 410) => "That video isn't available. It may have been deleted or made private.",
        Some(429) => "The site is limiting downloads right now. Try again in a little while.",
        Some(403) => REFUSED,
        Some(500..=599) => "The site is having problems right now. Try again later.",
        _ if has(&["unsupported url", "is not a valid url", "no suitable extractor"]) => {
            "VODscribe can't use that link: it isn't a site the video downloader knows. Paste a link to a video page."
        }
        _ if has(&["no such option"]) => "The video downloader is out of date. Update it in Settings, then try again.",
        _ if has(&["private video", "video is private"]) => "That video is private.",
        _ if has(&[
            "members-only",
            "join this channel",
            "subscriber-only",
            "subscribers-only",
            "only available to subscribers",
        ]) =>
        {
            "That video is only for channel members or subscribers, so VODscribe can't get it."
        }
        _ if has(&["confirm your age", "age-restricted", "age restricted"]) => {
            "That video is age-restricted, so VODscribe can't get it."
        }
        _ if has(&["not a bot"]) => "YouTube is asking to check that you're not a bot. Try again in a little while.",
        // After the specific cases: yt-dlp adds its sign-in hint ("Use
        // --cookies…") to private and age-restricted videos too.
        _ if has(&["--cookies", "logged-in", "log in to", "login required", "sign in to view"]) => {
            "That site wants you signed in to get this video, and VODscribe doesn't sign in to sites."
        }
        _ if has(&["requested format is not available"]) => "That video has no audio VODscribe can download.",
        _ if has(&[
            "video unavailable",
            "video is unavailable",
            "has been removed",
            "does not exist",
            "no longer available",
        ]) =>
        {
            "That video isn't available. It may have been deleted or made private."
        }
        _ if has(&[
            "timed out",
            "name resolution",
            "failed to resolve",
            "network is unreachable",
            "connection refused",
            "connection reset",
        ]) =>
        {
            "Couldn't connect. Check your internet connection and try again."
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_403_reads_as_refused_through_context() {
        let e =
            anyhow::anyhow!("{}", clean_error("ERROR: [youtube] abc: HTTP Error 403: Forbidden")).context("download");
        assert!(is_refused(&e));
        assert!(!is_refused(&anyhow::anyhow!("{}", clean_error("ERROR: [youtube] abc: HTTP Error 404: Not Found"))));
    }

    #[test]
    fn finds_the_finished_download_without_its_printed_path() {
        let dir = crate::TempDir::new("finished");
        let stem = dir.join("12");
        for name in ["12.info.json", "12.m4a.part", "123.m4a", "12.f251.webm"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        assert_eq!(finished_file(&stem), None, "nothing finished yet");
        std::fs::write(dir.join("12.m4a"), b"x").unwrap();
        assert_eq!(finished_file(&stem), Some(dir.join("12.m4a")));
        std::fs::write(dir.join("12.webm"), b"x").unwrap();
        assert_eq!(finished_file(&stem), None, "two candidates: don't guess");
    }

    #[test]
    fn errors_read_as_plain_sentences() {
        assert_eq!(
            clean_error("[info] x\nERROR: [youtube] abc123: Video unavailable\nmore"),
            "That video isn't available. It may have been deleted or made private."
        );
        assert_eq!(clean_error("ERROR: [youtube] abc: Private video. Sign in"), "That video is private.");
        let hint = " Use --cookies-from-browser or --cookies for the authentication.";
        assert_eq!(
            clean_error(&format!("ERROR: [youtube] abc: Private video. Sign in if you've been granted access.{hint}")),
            "That video is private."
        );
        assert!(
            clean_error(&format!("ERROR: [youtube] abc: Sign in to confirm your age.{hint}"))
                .contains("age-restricted")
        );
        assert_eq!(
            clean_error("ERROR: [twitch:vod] v12: Something odd happened"),
            "Couldn't get this video: Something odd happened"
        );
        assert_eq!(clean_error("one\ntwo\nthree\nfour\n"), "Couldn't get this video: two\nthree\nfour");
        // A video id containing "404" isn't a 404, and the format error still reads right.
        assert_eq!(
            clean_error("ERROR: [twitch:vod] v2404123: Requested format is not available"),
            "That video has no audio VODscribe can download."
        );
        assert!(clean_error("ERROR: [youtube] x: HTTP Error 503: Service Unavailable").contains("having problems"));
        assert!(
            clean_error("ERROR: [youtube] x: Unable to download webpage: HTTP Error 429: Too Many Requests")
                .contains("limiting")
        );
        assert!(clean_error("ERROR: [youtube] x: HTTP Error 403: Forbidden").contains("refused"));
        assert!(
            clean_error("ERROR: No suitable extractor found for URL https://example.com/").contains("isn't a site")
        );
        assert!(
            clean_error("ERROR: [vimeo] 76979871: The web client only works when logged-in. Use --cookies, …")
                .contains("signed in")
        );
        assert!(clean_error("yt-dlp: error: no such option: --no-plugin-dirs").contains("out of date"));
        assert!(
            clean_error("ERROR: [youtube] x: Sign in to confirm you're not a bot. Use --cookies-from-browser")
                .contains("not a bot")
        );
        // Only a leading tag is stripped.
        assert_eq!(
            clean_error("ERROR: unable to open for writing: [Errno 28] No space left on device"),
            "Couldn't get this video: unable to open for writing: [Errno 28] No space left on device"
        );
    }

    #[test]
    fn progress_parsing() {
        assert_eq!(parse_progress(" 50 100 NA NA NA"), Some(0.5));
        assert_eq!(parse_progress(" 50 NA 200 NA NA"), Some(0.25));
        assert_eq!(parse_progress(" 50 NA NA 3 12"), Some(0.25));
        assert_eq!(parse_progress(" NA NA NA NA NA"), None);
    }

    /// A link's lookup stops when canceled: a stand-in yt-dlp that never answers.
    #[cfg(unix)]
    #[test]
    fn a_lookup_stops_when_canceled() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::TempDir::new("probe-cancel");
        let path = dir.join("yt-dlp");
        fs::write(&path, "#!/bin/sh\nsleep 30\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        let yt_dlp = YtDlp { path, js_runtime: None };
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let stopper = {
            let cancel = cancel.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                cancel.store(true, Ordering::Relaxed);
                std::time::Instant::now()
            })
        };
        let result = probe(&yt_dlp, "https://example.com/v/1", &dir.join("info.json"), &cancel);
        let stopped = stopper.join().unwrap();
        assert!(result.is_err_and(|e| crate::is_canceled(&e)));
        assert!(stopped.elapsed() < std::time::Duration::from_secs(1), "{:?} after the cancel", stopped.elapsed());
    }

    #[test]
    fn kick_streams_only_from_kick() {
        assert!(is_kick_host("https://web.kick.com/api/v1/stream/manifest.m3u8?init=x"));
        assert!(is_kick_host("https://kick.com/x"));
        for bad in [
            "https://evil.com?x.kick.com/m.m3u8",
            "https://evil.com#.kick.com/",
            "https://evilkick.com/",
            "http://web.kick.com/",
            "https://kick.com@evil.com/",
            "https://web.kick.com.evil.com/",
        ] {
            assert!(!is_kick_host(bad), "{bad}");
        }
    }

    #[test]
    fn info_parsing() {
        let json = serde_json::json!({
            "id": "abc", "extractor_key": "TwitchVod", "webpage_url": "https://www.twitch.tv/videos/abc",
            "title": "Stream", "uploader": "someone", "duration": 3600.5, "upload_date": "20260102"
        });
        let info = parse_info(&json).unwrap();
        assert_eq!(info.platform, "twitch");
        assert_eq!(info.channel.as_deref(), Some("someone"));
        assert_eq!(info.published.as_deref(), Some("2026-01-02"));
        assert!(parse_info(&serde_json::json!({"_type": "playlist"})).is_err());
        let clip = serde_json::json!({
            "id": "1431111279", "display_id": "CoweringRudeDiamondMikeHogu-somPr5Um-WHGc4ga",
            "extractor_key": "TwitchClips", "webpage_url": "https://www.twitch.tv/crelly/clip/CoweringRudeDiamondMikeHogu-somPr5Um-WHGc4ga"
        });
        let clip = parse_info(&clip).unwrap();
        assert_eq!(
            (clip.platform.as_str(), clip.platform_id.as_str()),
            ("twitch", "CoweringRudeDiamondMikeHogu-somPr5Um-WHGc4ga")
        );
        // Eight bytes, but not eight digits: no date, and no panic.
        for date in ["202é102", "2026-1-2"] {
            let json = serde_json::json!({ "id": "abc", "webpage_url": "https://www.twitch.tv/videos/abc", "upload_date": date });
            assert_eq!(parse_info(&json).unwrap().published, None);
        }
    }

    /// Keeps a real video in a format that plays, merged with ffmpeg:
    /// `VODSCRIBE_BIN=<data dir>/bin cargo test -p vodscribe-core -- --ignored keeps_videos_live --nocapture`.
    #[test]
    #[ignore = "network"]
    fn keeps_videos_live() {
        let bin = std::env::var("VODSCRIBE_BIN").expect("VODSCRIBE_BIN: the folder with yt-dlp");
        let yt_dlp = crate::tools::find_yt_dlp(Path::new(&bin)).expect("yt-dlp");
        let ffmpeg = crate::tools::find_ffmpeg(Path::new(&bin)).expect("ffmpeg");
        let dir = crate::TempDir::new("keep-video");
        let selector = crate::playable::Playable::default().selector(480);
        for (name, url) in [
            ("dailymotion", "https://www.dailymotion.com/video/x7tgad0"),
            ("soundcloud", "https://soundcloud.com/forss/flickermood"),
            (
                "rumble",
                "https://rumble.com/v76ea7s-president-trump-announces-major-combat-operations-against-iran.html",
            ),
        ] {
            let info = dir.join(format!("{name}.info.json"));
            probe(&yt_dlp, url, &info, &AtomicBool::new(false)).unwrap();
            let wanted = Wanted::Video { selector: &selector, ffmpeg: &ffmpeg };
            let file = download(&yt_dlp, &info, &dir.join(name), wanted, &AtomicBool::new(false), |_| {}).unwrap();
            let streams = crate::local::codecs(&ffmpeg, &file).unwrap();
            assert!(crate::playable::Playable::default().plays(&streams), "{name}: {streams:?}");
            let out = std::process::Command::new(&ffmpeg).arg("-hide_banner").arg("-i").arg(&file).output().unwrap();
            let streams: Vec<String> = String::from_utf8_lossy(&out.stderr)
                .lines()
                .filter(|l| l.contains("Stream #"))
                .map(|l| l.trim().to_string())
                .collect();
            eprintln!(
                "{name}: {} ({} bytes)\n  {}",
                file.display(),
                file.metadata().unwrap().len(),
                streams.join("\n  ")
            );
        }
    }

    /// A Kick VOD, which yt-dlp can't read itself, through the app's own Kick
    /// lookup: `VODSCRIBE_BIN=<data dir>/bin cargo test -p vodscribe-core -- --ignored kick_vod_live`.
    #[test]
    #[ignore = "network"]
    fn kick_vod_live() {
        let bin = std::env::var("VODSCRIBE_BIN").expect("VODSCRIBE_BIN: the folder with yt-dlp");
        let yt_dlp = crate::tools::find_yt_dlp(Path::new(&bin)).expect("yt-dlp");
        let dir = crate::TempDir::new("kick-live");
        let url = std::env::var("KICK_URL")
            .unwrap_or_else(|_| "https://kick.com/geega/videos/01a0db44-6218-70fb-977c-6bbe91a018d1".into());
        let info = probe(&yt_dlp, &url, &dir.join("info.json"), &AtomicBool::new(false)).unwrap();
        eprintln!("{} · {:?} · {:?}", info.title, info.channel, info.duration);
        assert_eq!(info.platform, "kick");
    }
}
