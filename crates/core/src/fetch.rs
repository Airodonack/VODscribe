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
    let published = s("upload_date")
        .filter(|d| d.len() == 8)
        .map(|d| format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..]));
    Ok(VodInfo {
        platform: platform_of(&s("extractor_key").unwrap_or_default()),
        platform_id: s("id").context("yt-dlp returned no video id")?,
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
pub fn probe(yt_dlp: &YtDlp, url: &str, info_path: &Path) -> Result<VodInfo> {
    let result = dump_json(yt_dlp, url).and_then(|(json, raw)| Ok((parse_info(&json)?, raw)));
    let (info, raw) = match result {
        Ok(ok) => ok,
        // yt-dlp can't read Kick's current VOD links yet. Resolve those
        // ourselves and have yt-dlp treat the manifest as a plain HLS stream.
        Err(e) if kick::parse_vod_url(url).is_some() => {
            let (info, manifest) = kick::resolve(url).map_err(|kick_err| e.context(kick_err))?;
            (info, dump_json(yt_dlp, &manifest)?.1)
        }
        Err(e) => return Err(e),
    };
    fs::write(info_path, raw)?;
    Ok(info)
}

fn dump_json(yt_dlp: &YtDlp, url: &str) -> Result<(Value, Vec<u8>)> {
    let out = yt_dlp
        .command()
        .args(["-J", "--no-playlist", "--no-warnings", "--", url])
        .stdin(Stdio::null())
        .output()
        .context("run yt-dlp")?;
    if !out.status.success() {
        bail!("{}", clean_error(&String::from_utf8_lossy(&out.stderr)));
    }
    let json = serde_json::from_slice(&out.stdout).context("parse yt-dlp output")?;
    Ok((json, out.stdout))
}

const PROGRESS: &str = "[vodscribe]";
const FILE: &str = "[vodscribe-file]";

/// Downloads the smallest stream that carries audio, reporting a 0..1 fraction.
/// Returns the path of the downloaded file. An interrupted download resumes,
/// and a finished one is reused, when called again with the same `out_stem`.
pub fn download_audio(
    yt_dlp: &YtDlp,
    info_path: &Path,
    out_stem: &Path,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(f64),
) -> Result<PathBuf> {
    let template = format!(
        "download:{PROGRESS} %(progress.downloaded_bytes)s %(progress.total_bytes)s \
         %(progress.total_bytes_estimate)s %(progress.fragment_index)s %(progress.fragment_count)s"
    );
    let mut child = yt_dlp
        .command()
        .arg("--load-info-json")
        .arg(info_path)
        // Audio-only when offered (YouTube, Twitch); otherwise the lowest quality muxed stream.
        .args(["-f", "ba/w", "-N", "8", "--no-mtime", "--newline", "--progress"])
        // Skip remuxing HLS downloads: only the audio is used, and ffmpeg reads the raw stream.
        .args(["--fixup", "never"])
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
            let _ = child.kill();
            let _ = child.wait();
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
    path.context("yt-dlp finished without producing a file")
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

/// Reduces yt-dlp's stderr to its ERROR lines, which are the user-relevant part.
fn clean_error(stderr: &str) -> String {
    let errors: Vec<&str> = stderr
        .lines()
        .filter_map(|l| l.strip_prefix("ERROR: "))
        .collect();
    if errors.is_empty() {
        tools::last_lines(stderr, 3)
    } else {
        errors.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_prefer_yt_dlps_error_lines() {
        assert_eq!(clean_error("[info] x\nERROR: Video unavailable\nmore"), "Video unavailable");
        assert_eq!(clean_error("one\ntwo\nthree\nfour\n"), "two\nthree\nfour");
    }

    #[test]
    fn progress_parsing() {
        assert_eq!(parse_progress(" 50 100 NA NA NA"), Some(0.5));
        assert_eq!(parse_progress(" 50 NA 200 NA NA"), Some(0.25));
        assert_eq!(parse_progress(" 50 NA NA 3 12"), Some(0.25));
        assert_eq!(parse_progress(" NA NA NA NA NA"), None);
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
    }
}
