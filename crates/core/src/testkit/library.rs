//! A library for a test: the real SQLite file with every migration, in a
//! folder of its own (`Library`), and what a test puts in it the way the
//! app would (a file on this computer, a site's VOD, what's said in it).
//! Older libraries too: one at any format (`at_version`), one this version
//! made taken back to an older format (`back_to`), and the libraries and
//! settings files releases left (`Released`).

use std::path::{Path, PathBuf};

use rusqlite::Connection;

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

    /// `add_site_vod`, the VOD streamed on `channel` (yt-dlp's `channel`).
    pub fn add_site_vod_of(&self, platform: &str, id: &str, title: &str, duration: f64, channel: &str) -> Vod {
        add_site_vod_of(&self.db, platform, id, title, duration, Some(channel))
    }

    /// `words` are said in VOD `vod_id` (`said`), as a transcription writes them.
    pub fn says(&mut self, vod_id: i64, words: Vec<Word>) {
        says(&mut self.db, vod_id, words)
    }

    /// `says`, by `speaker`, as an imported VODscription names who says a line.
    pub fn says_as(&mut self, vod_id: i64, words: Vec<Word>, speaker: &str) {
        says_as(&mut self.db, vod_id, words, speaker)
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
    add_site_vod_of(db, platform, id, title, duration, None)
}

/// `Library::add_site_vod_of`, into `db`: on `channel`, if one is given.
pub fn add_site_vod_of(db: &Db, platform: &str, id: &str, title: &str, duration: f64, channel: Option<&str>) -> Vod {
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
        channel: channel.map(str::to_string),
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
    let Some(segment) = line(words) else { return };
    db.insert_segments(vod_id, &[segment]).unwrap();
}

/// `Library::says_as`, into `db`: one line of `words`, by `speaker`, as an
/// import writes it (a transcription names no one).
pub fn says_as(db: &mut Db, vod_id: i64, words: Vec<Word>, speaker: &str) {
    let Some(s) = line(words) else { return };
    let sql = format!("INSERT INTO segments ({}) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", crate::db::SEGMENT_COLUMNS);
    let words = serde_json::to_string(&s.words).unwrap();
    db.conn.execute(&sql, rusqlite::params![vod_id, s.start, s.end, s.text, words, speaker]).unwrap();
}

/// `words` as one line: their text, from the first's start to the last's end.
fn line(words: Vec<Word>) -> Option<Segment> {
    let (start, end) = (words.first()?.start, words.last()?.end);
    let text = words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
    Some(Segment { start, end, text, words })
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

/// A connection of its own to the library at `path`, open beside the app's:
/// for a test to change what the library allows, as a trigger that refuses
/// a change does, the way a full or failing disk would refuse it.
pub fn beside(path: &Path) -> Connection {
    Connection::open(path).unwrap()
}

/// The format the library on `conn` is at: how many migrations it has had
/// (`PRAGMA user_version`).
pub fn format_of(conn: &Connection) -> usize {
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap() as usize
}

/// Column `column` of `table`'s row `id` (a clip's, in `renders`) in the
/// library at `paths`, read as an older version reads it: through a
/// read-only connection of its own, not through `Db`.
pub fn old_column(paths: &Paths, table: &str, column: &str, id: i64) -> Option<String> {
    let conn = Connection::open_with_flags(paths.db(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let key = if table == "renders" { "clip_id" } else { "id" };
    conn.query_row(&format!("SELECT {column} FROM {table} WHERE {key} = ?1"), [id], |r| r.get(0)).unwrap()
}

/// A new library at `path` at format `version`, as a VODscribe whose newest
/// migration was that one made it: the first `version` migrations
/// (`db::MIGRATIONS`) run and `user_version` set, no rows. The test puts
/// rows in through the connection, as that version wrote them, and
/// `Db::open` then upgrades it.
pub fn at_version(path: &Path, version: usize) -> Connection {
    let migrations = crate::db::MIGRATIONS;
    assert!(version <= migrations.len(), "format {version} is newer than this version's {}", migrations.len());
    let conn = Connection::open(path).unwrap();
    let schema = migrations[..version].join("\n").replace("FLICKS", crate::db::FLICKS);
    conn.execute_batch(&format!("BEGIN; {schema}\n PRAGMA user_version = {version}; COMMIT;")).unwrap();
    conn
}

/// The library at `path`, made by this version, taken back to format
/// `version`, as if an older one had kept its rows: a library at that
/// format (`at_version`) with every row of each table both have, in the
/// columns both have. A column or table added since is gone, one dropped
/// since is back and empty, and the triggers and indexes are that format's;
/// a value whose meaning changed since (times in seconds before 22) is
/// copied as it is. Nothing may have the library open. The connection is
/// to the library at `version`, for a test to change rows as that version
/// would.
pub fn back_to(path: &Path, version: usize) -> Connection {
    let now = path.with_extension("now");
    let _ = std::fs::remove_file(&now);
    // Whole, with what the write-ahead log holds.
    Connection::open(path).unwrap().execute("VACUUM INTO ?1", [now.to_string_lossy()]).unwrap();
    for suffix in ["", "-wal", "-shm"] {
        let mut file = path.as_os_str().to_owned();
        file.push(suffix);
        let _ = std::fs::remove_file(file);
    }
    let conn = at_version(path, version);
    conn.execute("ATTACH ?1 AS now", [now.to_string_lossy()]).unwrap();
    let names = |sql: &str| -> Vec<(String, String)> {
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|r| r.unwrap()).collect()
    };
    let columns = |schema: &str, table: &str| -> Vec<String> {
        let mut stmt = conn.prepare("SELECT name FROM pragma_table_info(?1, ?2)").unwrap();
        stmt.query_map([table, schema], |r| r.get(0)).unwrap().map(|c| c.unwrap()).collect()
    };
    // The format's triggers keep counts and the search index as rows change;
    // the rows copied are already counted, so they're off while they go in.
    let triggers = names("SELECT name, sql FROM main.sqlite_master WHERE type = 'trigger'");
    // Tables of rows: not views, virtual tables (the search index) or their
    // own shadow tables, nor SQLite's.
    let tables = names(
        "SELECT name, type FROM pragma_table_list WHERE schema = 'main' AND type = 'table' AND name NOT LIKE 'sqlite_%'",
    );
    conn.execute_batch("PRAGMA foreign_keys = OFF; BEGIN").unwrap();
    for (name, _) in &triggers {
        conn.execute_batch(&format!("DROP TRIGGER main.\"{name}\"")).unwrap();
    }
    for (table, _) in &tables {
        let theirs = columns("now", table);
        if theirs.is_empty() {
            continue;
        }
        let both: Vec<String> =
            columns("main", table).into_iter().filter(|c| theirs.contains(c)).map(|c| format!("\"{c}\"")).collect();
        let both = both.join(", ");
        // What the format's migrations put in (the next id) gives way to the library's own.
        conn.execute_batch(&format!(
            "DELETE FROM main.\"{table}\"; INSERT INTO main.\"{table}\" ({both}) SELECT {both} FROM now.\"{table}\";"
        ))
        .unwrap();
    }
    // The search index of what was copied, made again from its rows.
    let virtual_tables = names("SELECT name, sql FROM main.sqlite_master WHERE sql LIKE 'CREATE VIRTUAL TABLE %'");
    for (index, sql) in &virtual_tables {
        let sql = sql.to_lowercase();
        if !sql.contains("using fts5") || sql.contains("fts5vocab") {
            continue;
        }
        conn.execute_batch(&format!("INSERT INTO main.\"{index}\" (\"{index}\") VALUES ('rebuild')")).unwrap();
    }
    for (_, sql) in &triggers {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("COMMIT; DETACH now").unwrap();
    std::fs::remove_file(&now).unwrap();
    conn
}

/// The library at `path`, made by this version, as a newer one left it:
/// changed by `sql`, as that version's migrations change it (a column
/// dropped, say), at the format after this version's, which `Db::open`
/// opens as it is. Nothing may have the library open.
pub fn newer(path: &Path, sql: &str) {
    let format = crate::db::MIGRATIONS.len() + 1;
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(&format!("BEGIN; {sql}\n PRAGMA user_version = {format}; COMMIT;")).unwrap();
}

/// The library and settings file a release left, as the upgrade scenarios
/// keep them (tests/e2e/fixtures: `library-<version>.sqlite`,
/// `settings-<version>.json`), copied into a folder of the test's own as the
/// app's data folder holds them. The fixtures stay as they are. A file's
/// path in the library keeps its placeholder (`{FIXTURES}`).
pub struct Released {
    /// The copy of the library, `library.sqlite`.
    pub library: PathBuf,
    /// The copy of the settings, `settings.json`.
    pub settings: PathBuf,
    dir: TempDir,
}

impl Released {
    /// The fixtures' folder, tests/e2e/fixtures.
    pub fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/fixtures")
    }

    /// The releases there are fixtures of, oldest first, as their files name
    /// them ("0.3.0", "0.5.2-dev").
    pub fn versions() -> Vec<String> {
        let mut versions: Vec<String> = std::fs::read_dir(Self::fixtures())
            .unwrap()
            .filter_map(|e| {
                let name = e.unwrap().file_name().to_string_lossy().into_owned();
                Some(name.strip_prefix("library-")?.strip_suffix(".sqlite")?.to_owned())
            })
            .collect();
        let number = |v: &str| -> Vec<u32> { v.split(['.', '-']).filter_map(|n| n.parse().ok()).collect() };
        versions.sort_by_key(|v| number(v));
        versions
    }

    /// Release `version`'s library and settings, copied.
    pub fn new(version: &str) -> Self {
        let dir = TempDir::new(&format!("released-{version}"));
        let (library, settings) = (dir.join("library.sqlite"), dir.join("settings.json"));
        let fixtures = Self::fixtures();
        std::fs::copy(fixtures.join(format!("library-{version}.sqlite")), &library).unwrap();
        std::fs::copy(fixtures.join(format!("settings-{version}.json")), &settings).unwrap();
        Released { library, settings, dir }
    }

    /// The folder the two copies are in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}
