//! A fake yt-dlp that answers as yt-dlp does to what the app asks of it: a
//! VOD's details (`-J`, from what the test says the site serves), its
//! downloads (`fetch::download`: the audio, the whole video, or a section of
//! it), written as fake media files (`Media`) cut from the VOD's link, and
//! its version (`--version`, `tools::update_yt_dlp`). A YouTube video's chat
//! replay (`--write-subs --sub-langs live_chat`, `chat::fetch`) is written a
//! line at a time to its part file, as yt-dlp writes it, then renamed into
//! place; a video the site serves no chat of has none, and nothing is written.
//!
//! A download is written as it comes, a quarter at a time, to a part file
//! beside it (`<file>.part`), as yt-dlp writes, and renamed into place once
//! whole, so one stopped or held partway leaves its part file with what was
//! in. Its size is its bitrate (`tbr` in the details) over its length, else
//! 4000 bytes. It says how far it is after each quarter; for a section,
//! which yt-dlp hands to ffmpeg, the app also gauges that from the part
//! file's growth against the bitrate it prints first. A part
//! file left from before is carried on from where it ends (a section's is
//! started over), and a file already whole isn't downloaded again, as
//! yt-dlp does. It fails as yt-dlp does for
//! a link the site doesn't serve, and on request for one it refuses; or
//! holds partway at a `Gate`.

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
#[derive(Clone)]
struct Site {
    /// Each link's details, as yt-dlp's `-J` gives them.
    serves: HashMap<String, Value>,
    /// Each link's chat replay, as the lines of yt-dlp's `live_chat.json`.
    chats: HashMap<String, Vec<String>>,
    /// Each chat replay's download waits here once its first line is written.
    chat_hold: Option<Gate>,
    /// Links whose downloads the site refuses (403).
    refuses: Vec<String>,
    /// Each download waits here once this far in (0 to 1), before it says so.
    hold: Option<(f64, Gate)>,
    /// Each look-up of a link's details (`-J`) waits here before it answers.
    details_hold: Option<Gate>,
    /// What `--version` says.
    version: String,
}

impl Default for Site {
    fn default() -> Self {
        Site {
            serves: HashMap::new(),
            chats: HashMap::new(),
            refuses: Vec::new(),
            hold: None,
            chat_hold: None,
            details_hold: None,
            version: VERSION.into(),
        }
    }
}

/// The version a fake yt-dlp says it is till told otherwise (`is_version`).
pub const VERSION: &str = "2025.10.22";

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

    /// The site refuses downloads of `url` from now (HTTP 403), its details
    /// still served.
    pub fn refuses(&self, url: &str) {
        self.site.lock().unwrap().refuses.push(url.into());
    }

    /// Each download from now waits at `gate` once it's begun, its first
    /// quarter written, before it says how far it is (`holds_at(0.25, …)`).
    pub fn holds(&self, gate: &Gate) {
        self.holds_at(0.25, gate);
    }

    /// Each download from now waits at `gate` once `at` (0 to 1) of it is
    /// written to its part file (to the quarter it's in), before it says
    /// so, until the gate opens or the app stops it.
    pub fn holds_at(&self, at: f64, gate: &Gate) {
        self.site.lock().unwrap().hold = Some((at, gate.clone()));
    }

    /// Each look-up of a link's details from now (`-J`: an add, a refresh)
    /// waits at `gate` before it answers, as a slow site does.
    pub fn holds_details(&self, gate: &Gate) {
        self.site.lock().unwrap().details_hold = Some(gate.clone());
    }

    /// From now it says it's `version` (`--version`), as after an update.
    pub fn is_version(&self, version: &str) {
        self.site.lock().unwrap().version = version.into();
    }
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

/// What a download's part file holds till it's whole: no media yet, as
/// many bytes as are in.
const BEING_WRITTEN: &[u8] = b"FAKE MEDIA (being downloaded)\n";

/// A download's size where its details give no bitrate (`tbr`).
const SIZE: u64 = 4000;

fn answer(run: &mut Run, site: &Site) -> i32 {
    let args = run.args();
    if run.has("--version") {
        run.out(&format!("{}\n", site.version));
        return 0;
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
    if run.has("--write-subs") && run.after("--sub-langs").as_deref() == Some("live_chat") {
        return write_chat(run, site);
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
    // Whole already: "has already been downloaded".
    if Path::new(&file).is_file() {
        run.out(&format!("[vodscribe-file]{file}\n"));
        return 0;
    }
    // Its bitrate once it's chosen, as `--print before_dl:` asks, and its size.
    let tbr = info["tbr"].as_f64().filter(|r| *r > 0.0);
    for print in args.iter().filter_map(|a| a.strip_prefix("before_dl:")) {
        let rate = tbr.map_or("NA".to_string(), |r| r.to_string());
        run.out(&format!("{}\n", print.replace("%(tbr)s", &rate)));
    }
    let size = tbr.map_or(SIZE, |r| (r * 125.0 * (to - from)) as u64).max(BEING_WRITTEN.len() as u64);
    let part = PathBuf::from(format!("{file}.part"));
    if let Some(dir) = part.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    // Carried on from where a download stopped before; a section, which
    // ffmpeg writes, starts over.
    let had = match section {
        None => std::fs::metadata(&part).map_or(0, |m| m.len()),
        Some(_) => 0,
    };
    let mut held = false;
    for quarter in [0.25, 0.5, 0.75, 1.0] {
        let bytes = (size as f64 * quarter) as u64;
        if bytes <= had {
            continue;
        }
        write_part(&part, bytes);
        if !held
            && let Some((at, gate)) = &site.hold
            && *at <= quarter
        {
            held = true;
            if !run.hold(gate) {
                return 255;
            }
        }
        if run.stopped() {
            return 255;
        }
        run.out(&format!("[vodscribe] {bytes} {size} NA NA NA\n"));
    }
    media.write(&part);
    if let Err(e) = std::fs::rename(&part, &file) {
        run.err(&format!("ERROR: Unable to rename file: {e}\n"));
        return 1;
    }
    run.out(&format!("[vodscribe-file]{file}\n"));
    0
}

/// The part file at `part`, `bytes` long: no media yet.
fn write_part(part: &Path, bytes: u64) {
    let filler: Vec<u8> = BEING_WRITTEN.iter().copied().cycle().take(bytes as usize).collect();
    std::fs::write(part, filler).unwrap();
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
