//! A fake yt-dlp that answers as yt-dlp does to what the app asks of it: a
//! VOD's details (`-J`, from what the test says the site serves) and its
//! downloads (`fetch::download`: the audio, the whole video, or a section of
//! it), written as fake media files (`Media`) cut from the VOD's link. Each
//! is written as it comes to a part file beside it (`<file>.part`), as
//! yt-dlp writes, and renamed into place once whole, so a download stopped
//! or held partway leaves its part file. It fails as yt-dlp does for a link
//! the site doesn't serve, and on request for one it refuses, partway or
//! not; or holds partway at a `Gate`. A YouTube video's chat replay
//! (`--write-subs --sub-langs live_chat`, `chat::fetch`) is written a line at
//! a time to its part file, as yt-dlp writes it, then renamed into place;
//! a video the site serves no chat of has none, and nothing is written.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::media::Media;
use super::tools::{self, Fake, Gate, Run};

/// The fake yt-dlp at a path, faked until it's dropped.
pub struct YtDlp {
    fake: Fake,
    site: Arc<Mutex<Site>>,
}

/// What the fake site serves, and what its downloads do.
#[derive(Clone, Default)]
struct Site {
    /// Each link's details, as yt-dlp's `-J` gives them.
    serves: HashMap<String, Value>,
    /// Links whose downloads the site refuses (403).
    refuses: Vec<String>,
    /// Each download waits here once it's begun, before its progress.
    hold: Option<Gate>,
    /// Each look-up of a link's details (`-J`) waits here before it answers.
    details_hold: Option<Gate>,
    /// Each link's chat replay, as the lines of yt-dlp's `live_chat.json`.
    chats: HashMap<String, Vec<String>>,
    /// Each chat replay's download waits here once its first line is written.
    chat_hold: Option<Gate>,
}

impl YtDlp {
    /// Fakes yt-dlp at `path`.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        let site = Arc::new(Mutex::new(Site::default()));
        let served = site.clone();
        let fake = tools::fake(path, move |run| {
            let site = served.lock().unwrap().clone();
            answer(run, &site)
        });
        YtDlp { fake, site }
    }

    /// Fakes the yt-dlp the app manages in `bin_dir` (`Paths::bin`).
    pub fn in_bin(bin_dir: &Path) -> Self {
        Self::at(crate::tools::managed_yt_dlp(bin_dir))
    }

    /// Ends its runs, now and to come (`Fake::end`).
    pub fn end(&self) {
        self.fake.end();
    }

    pub fn path(&self) -> &Path {
        self.fake.path()
    }

    /// The site serves `info` (yt-dlp's details, `details`) at its
    /// `webpage_url`.
    pub fn serves(&self, info: Value) {
        let url = info["webpage_url"].as_str().expect("details with a webpage_url").to_string();
        self.site.lock().unwrap().serves.insert(url, info);
    }

    /// The site refuses downloads of `url` from now (HTTP 403), its details
    /// still served.
    pub fn refuses(&self, url: &str) {
        self.site.lock().unwrap().refuses.push(url.into());
    }

    /// Each download from now waits at `gate` once it's begun, before it
    /// says how far it is.
    pub fn holds(&self, gate: &Gate) {
        self.site.lock().unwrap().hold = Some(gate.clone());
    }

    /// Each look-up of a link's details from now (`-J`: an add, a refresh)
    /// waits at `gate` before it answers, as a slow site does.
    pub fn holds_details(&self, gate: &Gate) {
        self.site.lock().unwrap().details_hold = Some(gate.clone());
    }

    /// The site has a chat replay for video `url`: `lines`, each a moment's
    /// messages as yt-dlp's `live_chat.json` has them (`chat_line`).
    pub fn serves_chat(&self, url: &str, lines: Vec<String>) {
        self.site.lock().unwrap().chats.insert(url.into(), lines);
    }

    /// Each chat replay's download from now waits at `gate` once its first
    /// line is written, as a long chat takes its time.
    pub fn holds_chat(&self, gate: &Gate) {
        self.site.lock().unwrap().chat_hold = Some(gate.clone());
    }
}

/// A line of a YouTube chat replay (`YtDlp::serves_chat`): `author` says
/// `text` `at` seconds into the video.
pub fn chat_line(at: f64, author: &str, text: &str) -> String {
    json!({ "replayChatItemAction": {
        "videoOffsetTimeMsec": format!("{}", (at * 1000.0).round() as i64),
        "actions": [{ "addChatItemAction": { "item": { "liveChatTextMessageRenderer": {
            "authorName": { "simpleText": author },
            "message": { "runs": [{ "text": text }] },
        } } } }],
    } })
    .to_string()
}

/// A chat replay's download: its lines to `<-o>.live_chat.json.part` as
/// they come, renamed once whole. None at all for a video without one.
fn write_chat(run: &mut Run, site: &Site) -> i32 {
    let url = run.args().last().cloned().unwrap_or_default();
    let Some(lines) = site.chats.get(&url) else {
        run.err("WARNING: There are no subtitles for the requested languages\n");
        return 0;
    };
    let file = PathBuf::from(format!("{}.live_chat.json", run.after("-o").unwrap_or_default()));
    let part = PathBuf::from(format!("{}.part", file.display()));
    let mut out = std::fs::File::create(&part).unwrap();
    for (i, line) in lines.iter().enumerate() {
        std::io::Write::write_all(&mut out, format!("{line}\n").as_bytes()).unwrap();
        if i == 0
            && let Some(gate) = &site.chat_hold
            && !run.hold(gate)
        {
            return 255;
        }
        if run.stopped() {
            return 255;
        }
    }
    drop(out);
    if let Err(e) = std::fs::rename(&part, &file) {
        run.err(&format!("ERROR: Unable to rename file: {e}\n"));
        return 1;
    }
    0
}

/// yt-dlp's details of a VOD of `platform` (youtube, twitch, kick) with
/// `id`, at `url`, titled `title`, `duration` seconds long; add to them
/// (`channel`, `upload_date`…) as the site would.
pub fn details(platform: &str, id: &str, url: &str, title: &str, duration: f64) -> Value {
    let extractor = match platform {
        "youtube" => "Youtube",
        "twitch" => "TwitchVod",
        "kick" => "Kick",
        other => other,
    };
    json!({
        "id": id,
        "title": title,
        "duration": duration,
        "webpage_url": url,
        "extractor_key": extractor,
    })
}

/// What a download's part file holds till it's whole: no media yet.
const BEING_WRITTEN: &[u8] = b"FAKE MEDIA (being downloaded)\n";

fn answer(run: &mut Run, site: &Site) -> i32 {
    let args = run.args();
    if run.has("--write-subs") && run.after("--sub-langs").as_deref() == Some("live_chat") {
        return write_chat(run, site);
    }
    if run.has("-J") {
        if let Some(gate) = &site.details_hold
            && !run.hold(gate)
        {
            return 255;
        }
        let url = args.last().cloned().unwrap_or_default();
        return match site.serves.get(&url) {
            Some(info) => {
                run.out(&format!("{info}\n"));
                0
            }
            None => {
                run.err(&format!("ERROR: Unsupported URL: {url}\n"));
                1
            }
        };
    }
    let Some(info_path) = run.after("--load-info-json") else {
        run.err("ERROR: fake yt-dlp: nothing it knows was asked\n");
        return 2;
    };
    let Some(info) = std::fs::read(&info_path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else {
        run.err(&format!("ERROR: Cannot read {info_path}\n"));
        return 1;
    };
    let url = info["webpage_url"].as_str().unwrap_or_default().to_string();
    let duration = info["duration"].as_f64().unwrap_or(0.0);
    if site.refuses.contains(&url) {
        run.err(&format!("ERROR: [{}] {}: HTTP Error 403: Forbidden\n", info["extractor_key"], info["id"]));
        return 1;
    }
    // What's downloaded: a section, the audio alone, or the whole video.
    let section = run.after("--download-sections").and_then(|s| {
        let (from, to) = s.strip_prefix('*')?.split_once('-')?;
        Some((from.parse::<f64>().ok()?, to.parse::<f64>().ok()?))
    });
    let audio = run.has("--fixup") && run.after("--fixup").as_deref() == Some("never");
    let matroska = args.iter().any(|a| a.contains("-f matroska"));
    let (from, to) = section.unwrap_or((0.0, duration));
    let mut media = if audio { Media::sound(to - from) } else { Media::video(to - from) };
    media.cut_from = Some((url, from));
    let ext = match (audio, matroska) {
        (true, _) => "m4a",
        (false, true) => "mkv",
        (false, false) => "mp4",
    };
    if matroska {
        media.container = "matroska".into();
    }
    let template = run.after("-o").unwrap_or_default();
    let file = template.replace("%(ext)s", ext);
    // Its first part, before it says how far it is; the rest as it ends.
    let part = PathBuf::from(format!("{file}.part"));
    if let Some(dir) = part.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(&part, BEING_WRITTEN).unwrap();
    if let Some(gate) = &site.hold
        && !run.hold(gate)
    {
        return 255;
    }
    run.out("[vodscribe] 1000 4000 NA NA NA\n");
    if run.stopped() {
        return 255;
    }
    media.write(&part);
    if let Err(e) = std::fs::rename(&part, &file) {
        run.err(&format!("ERROR: Unable to rename file: {e}\n"));
        return 1;
    }
    run.out("[vodscribe] 4000 4000 NA NA NA\n");
    run.out(&format!("[vodscribe-file]{file}\n"));
    0
}
