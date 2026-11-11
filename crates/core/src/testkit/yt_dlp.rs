//! A fake yt-dlp that answers as yt-dlp does to what the app asks of it: a
//! VOD's details (`-J`, from what the test says the site serves) and its
//! downloads (`fetch::download`: the audio, the whole video, or a section of
//! it), written as fake media files (`Media`) cut from the VOD's link. It
//! fails as yt-dlp does for a link the site doesn't serve, and on request
//! for one it refuses, partway or not; or holds partway at a `Gate`.

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

fn answer(run: &mut Run, site: &Site) -> i32 {
    let args = run.args();
    if run.has("-J") {
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
    if let Some(gate) = &site.hold
        && !run.hold(gate)
    {
        return 255;
    }
    run.out("[vodscribe] 1000 4000 NA NA NA\n");
    media.write(Path::new(&file));
    run.out("[vodscribe] 4000 4000 NA NA NA\n");
    run.out(&format!("[vodscribe-file]{file}\n"));
    0
}
