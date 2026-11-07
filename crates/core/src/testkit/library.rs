//! A library for a test: the real SQLite file with every migration, in a
//! folder of its own (`Library`), and what a test puts in it the way the
//! app would (a file on this computer, a site's VOD, what's said in it).

use std::path::PathBuf;

use super::{Media, TempDir};
use crate::Paths;
use crate::db::{Db, Vod};
use crate::fetch::VodInfo;
use crate::transcript::{Segment, Word};

/// A library in a folder that goes when it's dropped.
pub struct Library {
    pub paths: Paths,
    pub db: Db,
    /// Where the test's own files go (a VOD's file on this computer):
    /// outside the library's folder, as a user's are.
    pub files: PathBuf,
    // Last, so the library closes before its folder goes.
    _dir: TempDir,
}

impl Library {
    /// An empty library, its folders made (`Paths::ensure`).
    pub fn new() -> Self {
        let dir = TempDir::new("library");
        let paths = Paths::new(dir.join("data"));
        paths.ensure().unwrap();
        let db = Db::open_in(&paths).unwrap();
        let files = dir.join("files");
        std::fs::create_dir_all(&files).unwrap();
        Library { paths, db, files, _dir: dir }
    }

    /// A file on this computer, `name` (with its extension), holding
    /// `media`, added as the app adds one (`local::probe`): titled with its
    /// name, read where it is.
    pub fn add_file(&self, name: &str, media: &Media) -> Vod {
        add_file(&self.db, &self.files, name, media)
    }

    /// A VOD of `platform` (youtube, twitch, kick) with `id`, titled `title`
    /// and `duration` seconds long, as yt-dlp's details give it (`yt_dlp::details`).
    pub fn add_site_vod(&self, platform: &str, id: &str, title: &str, duration: f64) -> Vod {
        add_site_vod(&self.db, platform, id, title, duration)
    }

    /// `words` are said in VOD `vod_id` (`said`), as a transcription writes them.
    pub fn says(&mut self, vod_id: i64, words: Vec<Word>) {
        says(&mut self.db, vod_id, words)
    }
}

impl Default for Library {
    fn default() -> Self {
        Self::new()
    }
}

/// `Library::add_file`, into `db`, the file written in `files`.
pub fn add_file(db: &Db, files: &std::path::Path, name: &str, media: &Media) -> Vod {
    let path = files.join(name);
    media.write(&path);
    // As `local::probe` names it: the path as the system resolves it.
    let path = dunce::canonicalize(&path).unwrap().to_string_lossy().into_owned();
    let title = std::path::Path::new(name).file_stem().unwrap().to_string_lossy().into_owned();
    let info = VodInfo {
        platform: crate::local::PLATFORM.into(),
        platform_id: path.clone(),
        url: path.clone(),
        title,
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(media.duration),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let (vod, _) = db.add_vod(&info).unwrap();
    db.set_source_path(vod.id, &path).unwrap();
    db.vod(vod.id).unwrap().unwrap()
}

/// `Library::add_site_vod`, into `db`.
pub fn add_site_vod(db: &Db, platform: &str, id: &str, title: &str, duration: f64) -> Vod {
    let url = match platform {
        "youtube" => format!("https://www.youtube.com/watch?v={id}"),
        "twitch" => format!("https://www.twitch.tv/videos/{id}"),
        "kick" => format!("https://kick.com/video/{id}"),
        other => format!("https://{other}.example/{id}"),
    };
    let info = VodInfo {
        platform: platform.into(),
        platform_id: id.into(),
        url,
        title: title.into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(duration),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    db.add_vod(&info).unwrap().0
}

/// `Library::says`, into `db`: one line of `words`.
pub fn says(db: &mut Db, vod_id: i64, words: Vec<Word>) {
    let (Some(first), Some(last)) = (words.first(), words.last()) else { return };
    let text = words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
    let segment = Segment { start: first.start, end: last.end, text, words: words.clone() };
    db.insert_segments(vod_id, &[segment]).unwrap();
}

/// `text`'s words said from `start` seconds on, half a second each.
pub fn said(start: f64, text: &str) -> Vec<Word> {
    text.split_whitespace()
        .enumerate()
        .map(|(i, word)| {
            let at = start + i as f64 * 0.5;
            Word { text: word.into(), start: at, end: at + 0.5 }
        })
        .collect()
}
