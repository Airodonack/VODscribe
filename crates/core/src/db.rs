use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::chat::{ChatMessage, ChatPart, Kind};
use crate::fetch::VodInfo;
use crate::transcript::{Segment, Word, find_phrase, terms};

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Waiting to download.
    Queued,
    Downloading,
    /// Downloaded, waiting to transcribe.
    Ready,
    Transcribing,
    Paused,
    Done,
    Error,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Queued => "queued",
            Status::Downloading => "downloading",
            Status::Ready => "ready",
            Status::Transcribing => "transcribing",
            Status::Paused => "paused",
            Status::Done => "done",
            Status::Error => "error",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "queued" => Status::Queued,
            "downloading" => Status::Downloading,
            "ready" => Status::Ready,
            "transcribing" => Status::Transcribing,
            "paused" => Status::Paused,
            "done" => Status::Done,
            _ => Status::Error,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct Vod {
    pub id: i64,
    #[serde(flatten)]
    pub info: VodInfo,
    pub status: Status,
    /// 0..1 within the current status.
    pub progress: f64,
    pub error: Option<String>,
    pub added_at: i64,
    pub note: Option<String>,
    /// File name of the local thumbnail copy in `Paths::thumbs` (the UI fetches it
    /// with the `thumbnail` command). `None` until fetched; `""` if the VOD has no
    /// usable image, so it isn't tried again.
    pub thumb: Option<String>,
    /// For a file on this computer, its path. It's read in place, never changed.
    pub source_path: Option<String>,
    /// Has transcript segments from a model this version no longer uses (or
    /// from before models were recorded). Finished VODs can be re-transcribed;
    /// unfinished ones start over when they resume.
    pub outdated: bool,
    pub chat: ChatStatus,
}

/// Where a VOD's chat replay is.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChatStatus {
    /// Not fetched yet (or fetching).
    Pending,
    Done,
    /// The site has no chat replay for this VOD.
    None,
    /// Fetching failed; `chat_error` says why, and it's tried again later.
    Failed,
}

impl ChatStatus {
    fn as_str(self) -> Option<&'static str> {
        match self {
            ChatStatus::Pending => None,
            ChatStatus::Done => Some("done"),
            ChatStatus::None => Some("none"),
            ChatStatus::Failed => Some("failed"),
        }
    }

    fn parse(s: Option<&str>) -> Self {
        match s {
            Some("done") => ChatStatus::Done,
            Some("none") => ChatStatus::None,
            Some("failed") => ChatStatus::Failed,
            _ => ChatStatus::Pending,
        }
    }
}

/// What a search looks through. New kinds of filter go here as fields, with
/// defaults that leave a search unfiltered by them.
#[derive(Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct SearchFilter {
    /// Only this VOD.
    pub vod_id: Option<i64>,
    /// What was said (the transcript).
    pub speech: bool,
    /// What chat wrote.
    pub chat: bool,
}

impl Default for SearchFilter {
    fn default() -> Self {
        SearchFilter { vod_id: None, speech: true, chat: true }
    }
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Speech,
    Chat,
}

/// A transcript row's chat: its first messages, and how many it has in all.
#[derive(Serialize, Clone, Debug)]
pub struct ChatWindow {
    pub messages: Vec<ChatLine>,
    pub total: u32,
}

/// The columns `chat_line_row` reads.
const CHAT_LINE: &str = "id, time, author, color, parts, kind, amount";

/// A row of `CHAT_LINE`, its parts still JSON.
struct ChatLineRow(i64, f64, String, Option<String>, String, String, Option<f64>);

fn chat_line_row(r: &Row) -> rusqlite::Result<ChatLineRow> {
    Ok(ChatLineRow(r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
}

impl ChatLineRow {
    fn into_line(self) -> Result<ChatLine> {
        let ChatLineRow(id, time, author, color, parts, kind, amount) = self;
        Ok(ChatLine { id, time, author, color, parts: serde_json::from_str(&parts)?, kind: Kind::parse(&kind), amount })
    }
}

/// A chat message as the transcript view shows it.
#[derive(Serialize, Clone, Debug)]
pub struct ChatLine {
    /// Unique in the library; what `chat_matches` returns.
    pub id: i64,
    pub time: f64,
    pub author: String,
    pub color: Option<String>,
    pub parts: Vec<ChatPart>,
    pub kind: Kind,
    pub amount: Option<f64>,
}

#[derive(Serialize, Clone, Debug)]
pub struct TextPart {
    pub text: String,
    pub hit: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct SearchHit {
    pub vod_id: i64,
    pub source: Source,
    /// The segment's or chat message's id; unique within its source.
    pub id: i64,
    /// Start of the matched phrase itself, or of the segment if it couldn't be
    /// located. For chat, when the message was sent.
    pub time: f64,
    pub parts: Vec<TextPart>,
    /// For chat, who wrote it, and their name color.
    pub author: Option<String>,
    pub color: Option<String>,
}

pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    pub truncated: bool,
}

pub struct Db {
    conn: Connection,
}

const SCHEMA: &str = r#"
CREATE TABLE vods (
    id INTEGER PRIMARY KEY,
    platform TEXT NOT NULL,
    platform_id TEXT NOT NULL,
    url TEXT NOT NULL,
    title TEXT NOT NULL,
    channel TEXT,
    duration REAL,
    thumbnail TEXT,
    published TEXT,
    status TEXT NOT NULL,
    progress REAL NOT NULL DEFAULT 0,
    error TEXT,
    added_at INTEGER NOT NULL,
    UNIQUE (platform, platform_id)
);
CREATE TABLE segments (
    id INTEGER PRIMARY KEY,
    vod_id INTEGER NOT NULL REFERENCES vods (id) ON DELETE CASCADE,
    start REAL NOT NULL,
    end REAL NOT NULL,
    text TEXT NOT NULL,
    words TEXT NOT NULL
);
CREATE INDEX segments_by_vod ON segments (vod_id, start);
CREATE VIRTUAL TABLE segments_fts USING fts5 (
    text, content = 'segments', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER segments_ai AFTER INSERT ON segments BEGIN
    INSERT INTO segments_fts (rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER segments_ad AFTER DELETE ON segments BEGIN
    INSERT INTO segments_fts (segments_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
"#;

/// v2: a note shown with a VOD that isn't an error, such as why it ran on the CPU.
const MIGRATE_V2: &str = "ALTER TABLE vods ADD COLUMN note TEXT;";
/// v3: the finished download, so a resumed or retried VOD skips straight to transcription.
const MIGRATE_V3: &str = "ALTER TABLE vods ADD COLUMN audio_path TEXT;";
/// v4: the speech model that produced the transcript.
const MIGRATE_V4: &str = "ALTER TABLE vods ADD COLUMN model TEXT;";
/// v5: the library's order, which the user can rearrange. Starts newest first.
const MIGRATE_V5: &str = "
ALTER TABLE vods ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
UPDATE vods SET position = (
    SELECT count(*) FROM vods AS v WHERE v.added_at > vods.added_at OR (v.added_at = vods.added_at AND v.id > vods.id)
);";
/// v6: deleting a transcript removes its words from the search index right away,
/// rather than leaving them in index pages until a merge.
const MIGRATE_V6: &str = "INSERT INTO segments_fts (segments_fts, rank) VALUES ('secure-delete', 1);";
/// v7: the file name of the VOD's local thumbnail (see `thumbs`).
const MIGRATE_V7: &str = "ALTER TABLE vods ADD COLUMN thumb TEXT;";
/// v8: for a file on this computer (see `local`), where it is.
const MIGRATE_V8: &str = "ALTER TABLE vods ADD COLUMN source_path TEXT;";
/// v9: chat replays (see `chat`), searchable like the transcript, and whether
/// each VOD's has been fetched.
const MIGRATE_V9: &str = "
ALTER TABLE vods ADD COLUMN chat TEXT;
ALTER TABLE vods ADD COLUMN chat_error TEXT;
CREATE TABLE chat (
    id INTEGER PRIMARY KEY,
    vod_id INTEGER NOT NULL REFERENCES vods (id) ON DELETE CASCADE,
    time REAL NOT NULL,
    author TEXT NOT NULL,
    color TEXT,
    text TEXT NOT NULL,
    parts TEXT NOT NULL
);
CREATE INDEX chat_by_vod ON chat (vod_id, time);
CREATE VIRTUAL TABLE chat_fts USING fts5 (
    text, content = 'chat', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER chat_ai AFTER INSERT ON chat BEGIN
    INSERT INTO chat_fts (rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER chat_ad AFTER DELETE ON chat BEGIN
    INSERT INTO chat_fts (chat_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
INSERT INTO chat_fts (chat_fts, rank) VALUES ('secure-delete', 1);";
/// v10: what each chat message is (a sub, a gift, a tip, or just a message;
/// see `chat::Kind`), and how much. Chat fetched before has neither, so it's
/// fetched again.
const MIGRATE_V10: &str = "
ALTER TABLE chat ADD COLUMN kind TEXT NOT NULL DEFAULT 'message';
ALTER TABLE chat ADD COLUMN amount REAL;
UPDATE vods SET chat = NULL WHERE chat = 'done';";
/// v11: chat saved as it's fetched: where an unfinished fetch got to (see
/// `chat::ChatStore`), and when a message's event happened, if not when it was sent.
const MIGRATE_V11: &str = "
ALTER TABLE vods ADD COLUMN chat_resume REAL;
ALTER TABLE chat ADD COLUMN event_time REAL;";
/// v12: a VOD the user stopped says "Stopped", as the app has written since
/// 0.1.0's "Canceled" (the app shows either as stopped, not failed).
const MIGRATE_V12: &str = "
UPDATE vods SET error = 'Stopped' WHERE status = 'error' AND error = 'Canceled';";
/// v13: the search indexes stop rewriting each deleted row's entries as it's
/// deleted ("secure-delete", 25 times slower: 6.2 s for a 127k-message chat);
/// instead, `tidy_index` rewrites an index without them once a VOD's rows are
/// gone (0.3 s for 2.7M messages), so deleted words still don't stay in the file.
const MIGRATE_V13: &str = "
INSERT INTO segments_fts (segments_fts, rank) VALUES ('secure-delete', 0);
INSERT INTO chat_fts (chat_fts, rank) VALUES ('secure-delete', 0);";

/// The columns `vod_from_row` reads, by name, so adding a column can't shift the others.
const VOD_COLUMNS: &str =
    "id, platform, platform_id, url, title, channel, duration, thumbnail, published, status, progress, error, added_at,
     note, model, thumb, source_path, chat, EXISTS (SELECT 1 FROM segments WHERE segments.vod_id = vods.id) AS has_segments";

fn vod_from_row(r: &Row) -> rusqlite::Result<Vod> {
    Ok(Vod {
        id: r.get("id")?,
        info: VodInfo {
            platform: r.get("platform")?,
            platform_id: r.get("platform_id")?,
            url: r.get("url")?,
            title: r.get("title")?,
            channel: r.get("channel")?,
            duration: r.get("duration")?,
            thumbnail: r.get("thumbnail")?,
            published: r.get("published")?,
        },
        status: Status::parse(&r.get::<_, String>("status")?),
        progress: r.get("progress")?,
        error: r.get("error")?,
        added_at: r.get("added_at")?,
        note: r.get("note")?,
        thumb: r.get("thumb")?,
        source_path: r.get("source_path")?,
        outdated: {
            let model: Option<String> = r.get("model")?;
            let has_segments: bool = r.get("has_segments")?;
            has_segments && !model.is_some_and(|m| crate::model::is_current(&m))
        },
        chat: ChatStatus::parse(r.get::<_, Option<String>>("chat")?.as_deref()),
    })
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_inner(path).with_context(|| format!("open the library at {}", path.display()))
    }

    /// A second connection to the library at `path`, for reading only, opened
    /// after `open` (which makes and upgrades it). The library is in WAL mode,
    /// so reads here never wait for writes on the other connection (chat being
    /// saved, a VOD being deleted), and the other way around.
    pub fn open_reader(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("open the library at {} for reading", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Self { conn })
    }

    fn open_inner(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // secure_delete zeroes freed pages, so a removed transcript doesn't linger in the file.
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA secure_delete = ON;")?;
        let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        // Migration n brings the schema to version n + 1; user_version is how many have run.
        let migrations = [
            SCHEMA,
            MIGRATE_V2,
            MIGRATE_V3,
            MIGRATE_V4,
            MIGRATE_V5,
            MIGRATE_V6,
            MIGRATE_V7,
            MIGRATE_V8,
            MIGRATE_V9,
            MIGRATE_V10,
            MIGRATE_V11,
            MIGRATE_V12,
            MIGRATE_V13,
        ];
        if (version as usize) < migrations.len() {
            let pending = migrations[version as usize..].join("\n");
            let latest = migrations.len();
            conn.execute_batch(&format!("BEGIN; {pending} PRAGMA user_version = {latest}; COMMIT;"))?;
        } else if version as usize > migrations.len() {
            // A newer VODscribe made this library (the user went back a version).
            // Migrations so far only add, so carry on rather than refuse to start.
            eprintln!("{}: library format {version} is newer than this version knows", path.display());
        }
        Ok(Self { conn })
    }

    /// Inserts a VOD, or returns the existing row if it was already added.
    pub fn add_vod(&self, info: &VodInfo) -> Result<(Vod, bool)> {
        if let Some(existing) = self.find_vod(&info.platform, &info.platform_id)? {
            return Ok((existing, false));
        }
        // New VODs go to the top of the library.
        self.conn.execute(
            "INSERT INTO vods (platform, platform_id, url, title, channel, duration, thumbnail, published, status, added_at, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'queued', ?9, (SELECT coalesce(min(position), 0) - 1 FROM vods))",
            params![
                info.platform, info.platform_id, info.url, info.title, info.channel,
                info.duration, info.thumbnail, info.published, now()
            ],
        )?;
        Ok((self.vod(self.conn.last_insert_rowid())?.expect("just inserted"), true))
    }

    fn find_vod(&self, platform: &str, platform_id: &str) -> Result<Option<Vod>> {
        let sql = format!("SELECT {VOD_COLUMNS} FROM vods WHERE platform = ?1 AND platform_id = ?2");
        Ok(self.conn.query_row(&sql, params![platform, platform_id], vod_from_row).optional()?)
    }

    pub fn vod(&self, id: i64) -> Result<Option<Vod>> {
        let sql = format!("SELECT {VOD_COLUMNS} FROM vods WHERE id = ?1");
        Ok(self.conn.query_row(&sql, [id], vod_from_row).optional()?)
    }

    pub fn vods(&self) -> Result<Vec<Vod>> {
        let sql = format!("SELECT {VOD_COLUMNS} FROM vods ORDER BY position, id DESC");
        let mut stmt = self.conn.prepare(&sql)?;
        Ok(stmt.query_map([], vod_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// Runs `f`'s writes as one transaction: all of them happen, or none do.
    pub fn atomically<T>(&self, f: impl FnOnce(&Db) -> Result<T>) -> Result<T> {
        let tx = self.conn.unchecked_transaction()?;
        let out = f(self)?;
        tx.commit()?;
        Ok(out)
    }

    /// Puts the library in the order of `ids`, which should list every VOD.
    pub fn reorder(&mut self, ids: &[i64]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare("UPDATE vods SET position = ?2 WHERE id = ?1")?;
            for (i, id) in ids.iter().enumerate() {
                stmt.execute(params![id, i as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The lowest VOD in the library with `status`, which is the next one for that stage.
    /// New VODs go on top, so by default the queue runs oldest first.
    pub fn next_with_status(&self, status: Status) -> Result<Option<Vod>> {
        let sql = format!("SELECT {VOD_COLUMNS} FROM vods WHERE status = ?1 ORDER BY position DESC, id LIMIT 1");
        Ok(self.conn.query_row(&sql, [status.as_str()], vod_from_row).optional()?)
    }

    /// Like `next_with_status`, but only VODs that are files on this computer.
    pub fn next_local_with_status(&self, status: Status) -> Result<Option<Vod>> {
        let sql = format!(
            "SELECT {VOD_COLUMNS} FROM vods WHERE status = ?1 AND source_path IS NOT NULL ORDER BY position DESC, id LIMIT 1"
        );
        Ok(self.conn.query_row(&sql, [status.as_str()], vod_from_row).optional()?)
    }

    pub fn count_with_status(&self, status: Status) -> Result<usize> {
        let n: i64 =
            self.conn.query_row("SELECT count(*) FROM vods WHERE status = ?1", [status.as_str()], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Returns VODs left mid-stage by a crash or quit to that stage's queue.
    /// Transcription resumes from its last indexed segment.
    pub fn requeue_interrupted(&self) -> Result<()> {
        self.conn.execute_batch(
            "UPDATE vods SET status = 'queued' WHERE status = 'downloading';
             UPDATE vods SET status = 'ready' WHERE status = 'transcribing';",
        )?;
        Ok(())
    }

    /// Forgets every finished download, after the work dir has been cleared.
    /// VODs waiting to transcribe go back to waiting to download.
    pub fn forget_downloads(&self) -> Result<()> {
        self.conn.execute_batch(
            "UPDATE vods SET audio_path = NULL;
             UPDATE vods SET status = 'queued', progress = 0 WHERE status = 'ready';",
        )?;
        Ok(())
    }

    pub fn audio_path(&self, id: i64) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT audio_path FROM vods WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten())
    }

    pub fn set_source_path(&self, id: i64, path: &str) -> Result<()> {
        self.conn.execute("UPDATE vods SET source_path = ?2 WHERE id = ?1", params![id, path])?;
        Ok(())
    }

    pub fn set_audio_path(&self, id: i64, path: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE vods SET audio_path = ?2 WHERE id = ?1", params![id, path])?;
        Ok(())
    }

    /// How far transcription got: the end of the last indexed segment.
    pub fn transcribed_until(&self, vod_id: i64) -> Result<f64> {
        Ok(self
            .conn
            .query_row("SELECT coalesce(max(end), 0) FROM segments WHERE vod_id = ?1", [vod_id], |r| r.get(0))?)
    }

    pub fn set_status(&self, id: i64, status: Status, progress: f64, error: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE vods SET status = ?2, progress = ?3, error = ?4 WHERE id = ?1",
            params![id, status.as_str(), progress, error],
        )?;
        Ok(())
    }

    pub fn model(&self, id: i64) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT model FROM vods WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten())
    }

    pub fn set_model(&self, id: i64, model: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE vods SET model = ?2 WHERE id = ?1", params![id, model])?;
        Ok(())
    }

    pub fn set_thumb(&self, id: i64, thumb: &str) -> Result<()> {
        self.conn.execute("UPDATE vods SET thumb = ?2 WHERE id = ?1", params![id, thumb])?;
        Ok(())
    }

    pub fn set_note(&self, id: i64, note: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE vods SET note = ?2 WHERE id = ?1", params![id, note])?;
        Ok(())
    }

    /// Deletes up to `limit` of VOD `vod_id`'s chat messages and transcript
    /// segments; how many it deleted, 0 when there are none left. A VOD with a
    /// busy chat has a hundred thousand rows and more, and erasing them (see
    /// `secure_delete`) takes seconds, so they go a part at a time, letting
    /// everything else use the library in between.
    pub fn delete_some_of(&self, vod_id: i64, limit: usize) -> Result<usize> {
        let chat = self.clear_some_chat(vod_id, limit)?;
        if chat > 0 {
            return Ok(chat);
        }
        // Segments cost more each, with their words: a third as many at a time.
        self.clear_some_segments(vod_id, (limit / 3).max(1))
    }

    /// Deletes up to `limit` of VOD `vod_id`'s chat messages; how many, 0 when
    /// none are left. A busy chat takes seconds to delete, so it goes a part at
    /// a time, between which others can use the library (see `LibraryChat::clear`).
    pub fn clear_some_chat(&self, vod_id: i64, limit: usize) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM chat WHERE id IN (SELECT id FROM chat WHERE vod_id = ?1 LIMIT ?2)",
            params![vod_id, limit as i64],
        )?)
    }

    /// Deletes up to `limit` of VOD `vod_id`'s transcript segments; how many, 0
    /// when none are left. A long transcript takes a second or more to erase,
    /// so it goes a part at a time (see `delete_some_of`).
    pub fn clear_some_segments(&self, vod_id: i64, limit: usize) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM segments WHERE id IN (SELECT id FROM segments WHERE vod_id = ?1 LIMIT ?2)",
            params![vod_id, limit as i64],
        )?)
    }

    /// Rewrites search index `index` ("chat_fts" or "segments_fts") without
    /// the entries of rows deleted since, which otherwise stay in the file
    /// (secure_delete then zeroes the pages they were on). After a VOD's rows
    /// are deleted; a no-op when there's nothing to tidy.
    fn tidy_index(&self, index: &str) -> Result<()> {
        self.conn.execute(&format!("INSERT INTO {index} ({index}) VALUES ('optimize')"), [])?;
        Ok(())
    }

    /// Removes VOD `id`, once its chat and transcript are erased (`delete_some_of`;
    /// any saved since, by a fetch or a transcription finishing, go with it),
    /// and the search indexes' entries for them, tidied after it's gone.
    pub fn delete_vod(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM vods WHERE id = ?1", [id])?;
        self.tidy_index("chat_fts")?;
        self.tidy_index("segments_fts")?;
        // Copy the change into the main file and empty the WAL, which still
        // holds the old pages. A read on another connection at the time keeps
        // it from finishing, so it's tried a few times.
        for _ in 0..5 {
            let busy: i64 = self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0)).unwrap_or(1);
            if busy == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Ok(())
    }

    pub fn clear_segments(&self, vod_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM segments WHERE vod_id = ?1", [vod_id])?;
        // The old transcript's words, out of the index too.
        self.tidy_index("segments_fts")?;
        Ok(())
    }

    pub fn insert_segments(&mut self, vod_id: i64, segments: &[Segment]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt =
                tx.prepare("INSERT INTO segments (vod_id, start, end, text, words) VALUES (?1, ?2, ?3, ?4, ?5)")?;
            for s in segments {
                stmt.execute(params![vod_id, s.start, s.end, s.text, serde_json::to_string(&s.words)?])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Each segment's start time and text, without the per-word timings that
    /// `segments` also parses. This is what the transcript view needs.
    pub fn lines(&self, vod_id: i64) -> Result<Vec<(f64, String)>> {
        let mut stmt = self.conn.prepare("SELECT start, text FROM segments WHERE vod_id = ?1 ORDER BY start")?;
        let rows = stmt.query_map([vod_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn segments(&self, vod_id: i64) -> Result<Vec<Segment>> {
        let mut stmt =
            self.conn.prepare("SELECT start, end, text, words FROM segments WHERE vod_id = ?1 ORDER BY start")?;
        let rows = stmt.query_map([vod_id], |r| {
            Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (start, end, text, words) = row?;
            out.push(Segment { start, end, text, words: serde_json::from_str(&words)? });
        }
        Ok(out)
    }

    /// Replaces VOD `vod_id`'s chat with `messages` and records it as fetched.
    pub fn set_chat(&mut self, vod_id: i64, messages: &[ChatMessage]) -> Result<()> {
        self.clear_chat(vod_id)?;
        self.add_chat(vod_id, messages, 0.0)?;
        self.finish_chat(vod_id)
    }

    /// Where an unfinished fetch of VOD `vod_id`'s chat got to (see `chat::ChatStore`).
    pub fn chat_resume(&self, vod_id: i64) -> Result<Option<f64>> {
        Ok(self
            .conn
            .query_row("SELECT chat_resume FROM vods WHERE id = ?1", [vod_id], |r| r.get(0))
            .optional()?
            .flatten())
    }

    /// Forgets VOD `vod_id`'s chat and where a fetch of it got to.
    pub fn clear_chat(&self, vod_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM chat WHERE vod_id = ?1", [vod_id])?;
        self.conn.execute("UPDATE vods SET chat_resume = NULL WHERE id = ?1", [vod_id])?;
        Ok(())
    }

    /// Forgets VOD `vod_id`'s chat messages from `time` on.
    pub fn drop_chat_from(&self, vod_id: i64, time: f64) -> Result<()> {
        self.conn.execute("DELETE FROM chat WHERE vod_id = ?1 AND time >= ?2", params![vod_id, time])?;
        Ok(())
    }

    /// Adds `messages` to VOD `vod_id`'s chat and records that its fetch reached `position`, together.
    pub fn add_chat(&mut self, vod_id: i64, messages: &[ChatMessage], position: f64) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO chat (vod_id, time, author, color, text, parts, kind, amount, event_time)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for m in messages {
                let parts = serde_json::to_string(&m.parts)?;
                let kind = m.kind.as_str();
                stmt.execute(params![
                    vod_id,
                    m.time,
                    m.author,
                    m.color,
                    m.text(),
                    parts,
                    kind,
                    m.amount,
                    m.event_time
                ])?;
            }
        }
        tx.execute("UPDATE vods SET chat_resume = ?2 WHERE id = ?1", params![vod_id, position])?;
        tx.commit()?;
        Ok(())
    }

    /// Records VOD `vod_id`'s chat as fetched in full.
    pub fn finish_chat(&self, vod_id: i64) -> Result<()> {
        self.conn
            .execute("UPDATE vods SET chat = 'done', chat_error = NULL, chat_resume = NULL WHERE id = ?1", [vod_id])?;
        Ok(())
    }

    /// VOD `vod_id`'s subs, as (id, message).
    pub fn chat_subs(&self, vod_id: i64) -> Result<Vec<(i64, ChatMessage)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, time, author, text, amount, event_time FROM chat WHERE vod_id = ?1 AND kind = 'sub' ORDER BY time",
        )?;
        let rows = stmt.query_map([vod_id], |r| {
            Ok((
                r.get(0)?,
                ChatMessage {
                    time: r.get(1)?,
                    author: r.get(2)?,
                    color: None,
                    parts: vec![ChatPart::Text { text: r.get(3)? }],
                    kind: Kind::Sub,
                    amount: r.get(4)?,
                    event_time: r.get(5)?,
                },
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Makes these chat messages plain messages.
    pub fn demote_chat(&self, ids: &[i64]) -> Result<()> {
        // One statement, so one commit: a busy chat can have hundreds.
        self.conn.execute(
            "UPDATE chat SET kind = 'message' WHERE id IN (SELECT value FROM json_each(?1))",
            [serde_json::to_string(ids)?],
        )?;
        Ok(())
    }

    /// Records that VOD `vod_id` has no chat replay (`ChatStatus::None`), that
    /// fetching it failed (`Failed`, with why), or that it should be fetched again (`Pending`).
    pub fn set_chat_status(&self, vod_id: i64, status: ChatStatus, error: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE vods SET chat = ?2, chat_error = ?3 WHERE id = ?1",
            params![vod_id, status.as_str(), error],
        )?;
        Ok(())
    }

    /// VODs whose chat hasn't been fetched (or failed), in library order.
    pub fn chat_pending(&self) -> Result<Vec<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM vods WHERE chat IS NULL OR chat = 'failed' ORDER BY position, id DESC")?;
        Ok(stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }

    /// VOD `vod_id`'s chat, oldest first. All of it, which can be a lot: the
    /// transcript view asks for what it shows instead (`chat_windows`).
    pub fn chat(&self, vod_id: i64) -> Result<Vec<ChatLine>> {
        let sql = format!("SELECT {CHAT_LINE} FROM chat WHERE vod_id = ?1 ORDER BY time, id");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([vod_id], chat_line_row)?;
        rows.map(|r| r?.into_line()).collect()
    }

    /// For each stretch of VOD `vod_id` (from, to: seconds, the end left out),
    /// its first `cap` messages and how many it has. The transcript view's rows,
    /// each one line and the chat while it was said.
    pub fn chat_windows(&self, vod_id: i64, ranges: &[(f64, f64)], cap: usize) -> Result<Vec<ChatWindow>> {
        let sql = format!(
            "SELECT {CHAT_LINE} FROM chat WHERE vod_id = ?1 AND time >= ?2 AND time < ?3 ORDER BY time, id LIMIT ?4"
        );
        let mut lines = self.conn.prepare(&sql)?;
        let mut count =
            self.conn.prepare("SELECT count(*) FROM chat WHERE vod_id = ?1 AND time >= ?2 AND time < ?3")?;
        let mut out = Vec::with_capacity(ranges.len());
        for &(from, to) in ranges {
            let messages = lines
                .query_map(params![vod_id, from, to, cap as i64], chat_line_row)?
                .map(|r| r?.into_line())
                .collect::<Result<Vec<_>>>()?;
            let total = if messages.len() < cap {
                messages.len() as u32
            } else {
                count.query_row(params![vod_id, from, to], |r| r.get(0))?
            };
            out.push(ChatWindow { messages, total });
        }
        Ok(out)
    }

    /// How many of VOD `vod_id`'s chat messages fall in each of `bins` equal
    /// stretches of its `duration`, for the minimap.
    pub fn chat_histogram(&self, vod_id: i64, duration: f64, bins: usize) -> Result<Vec<u32>> {
        let mut out = vec![0u32; bins];
        if duration <= 0.0 || bins == 0 {
            return Ok(out);
        }
        let mut stmt = self.conn.prepare(
            "SELECT CAST(time * ?2 / ?3 AS INTEGER) AS bin, count(*) FROM chat WHERE vod_id = ?1 GROUP BY bin",
        )?;
        let rows =
            stmt.query_map(params![vod_id, bins as f64, duration], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, u32>(1)?)))?;
        for row in rows {
            let (bin, n) = row?;
            out[(bin.max(0) as usize).min(bins - 1)] += n;
        }
        Ok(out)
    }

    /// How many chat messages VOD `vod_id` has.
    pub fn chat_count(&self, vod_id: i64) -> Result<u32> {
        Ok(self.conn.query_row("SELECT count(*) FROM chat WHERE vod_id = ?1", [vod_id], |r| r.get(0))?)
    }

    /// VOD `vod_id`'s subs, gifts and tips, oldest first. Few, next to the rest of chat.
    pub fn chat_events(&self, vod_id: i64) -> Result<Vec<ChatLine>> {
        let sql = format!("SELECT {CHAT_LINE} FROM chat WHERE vod_id = ?1 AND kind != 'message' ORDER BY time, id");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([vod_id], chat_line_row)?;
        rows.map(|r| r?.into_line()).collect()
    }

    /// The emotes in VOD `vod_id`'s chat, as (name, image URL), each once.
    pub fn chat_emotes(&self, vod_id: i64) -> Result<Vec<(String, String)>> {
        // Only messages with an emote, by a cheap look at their stored parts.
        let mut stmt = self.conn.prepare("SELECT parts FROM chat WHERE vod_id = ?1 AND parts LIKE '%\"emote\"%'")?;
        let rows = stmt.query_map([vod_id], |r| r.get::<_, String>(0))?;
        let mut seen = std::collections::HashMap::new();
        for parts in rows {
            for part in serde_json::from_str::<Vec<ChatPart>>(&parts?)? {
                if let ChatPart::Emote { name, url } = part {
                    seen.entry(name).or_insert(url);
                }
            }
        }
        let mut out: Vec<_> = seen.into_iter().collect();
        out.sort();
        Ok(out)
    }

    /// The ids and times of VOD `vod_id`'s chat messages that contain any of `terms`, each
    /// a whole word or phrase (an emote by its name), matched as search matches
    /// words: ignoring case and accents. For a timeline layer (see `layers`).
    pub fn chat_matches(&self, vod_id: i64, terms: &[String]) -> Result<Vec<(i64, f64)>> {
        // Terms are alphanumeric only, so quoting them can't inject FTS syntax.
        let phrases: Vec<String> = terms
            .iter()
            .map(|t| crate::transcript::terms(t))
            .filter(|t| !t.is_empty())
            .map(|t| format!("\"{}\"", t.join(" ")))
            .collect();
        if phrases.is_empty() {
            return Ok(Vec::new());
        }
        let (lo, hi) = self.id_range("chat", Some(vod_id))?;
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.time FROM chat_fts f JOIN chat c ON c.id = f.rowid
             WHERE chat_fts MATCH ?1 AND f.rowid BETWEEN ?3 AND ?4 AND c.vod_id = ?2 ORDER BY c.time, c.id",
        )?;
        let found = stmt.query_map(params![phrases.join(" OR "), vod_id, lo, hi], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(found.collect::<rusqlite::Result<_>>()?)
    }

    /// The lowest and highest row ids of VOD `vod_id`'s rows in `table`, or
    /// every id for none. The full-text indexes don't know which VOD a row is
    /// from; a range of ids lets a search for one VOD look at its rows only,
    /// rather than every match in the library before keeping its own (a
    /// 2.7M-message library: 161 ms for "a" in one VOD, 33 ms with this).
    fn id_range(&self, table: &str, vod_id: Option<i64>) -> Result<(i64, i64)> {
        let Some(vod_id) = vod_id else { return Ok((i64::MIN, i64::MAX)) };
        let sql = format!("SELECT min(id), max(id) FROM {table} WHERE vod_id = ?1");
        let (lo, hi): (Option<i64>, Option<i64>) =
            self.conn.query_row(&sql, [vod_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok((lo.unwrap_or(1), hi.unwrap_or(0)))
    }

    /// Phrase search. The last term matches as a prefix so results update
    /// while typing, once it's two letters or more: one letter begins too many
    /// words to look through as a prefix, so it matches as a word.
    /// Hits are ordered by VOD, in library order, then by time within the VOD.
    pub fn search(&self, query: &str, filter: &SearchFilter, limit: usize) -> Result<SearchResults> {
        let q = terms(query);
        if q.is_empty() || !(filter.speech || filter.chat) {
            return Ok(SearchResults { hits: Vec::new(), truncated: false });
        }
        // Terms are alphanumeric only, so quoting them can't inject FTS syntax.
        let prefix = q.last().is_some_and(|t| t.chars().count() >= 2);
        let fts = format!("\"{}\"{}", q.join(" "), if prefix { "*" } else { "" });
        // (library position, VOD id, hit), to merge the sources in order.
        let mut found: Vec<(i64, i64, SearchHit)> = Vec::new();
        if filter.speech {
            let mut stmt = self.conn.prepare(
                "SELECT s.id, s.vod_id, s.start, s.words, v.position FROM segments_fts f
                 JOIN segments s ON s.id = f.rowid
                 JOIN vods v ON v.id = s.vod_id
                 WHERE segments_fts MATCH ?1 AND f.rowid BETWEEN ?4 AND ?5 AND (?2 IS NULL OR s.vod_id = ?2)
                 ORDER BY v.position, v.id DESC, s.start
                 LIMIT ?3",
            )?;
            let (lo, hi) = self.id_range("segments", filter.vod_id)?;
            let rows = stmt.query_map(params![fts, filter.vod_id, limit as i64 + 1, lo, hi], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, f64>(2)?, r.get::<_, String>(3)?, r.get(4)?))
            })?;
            for row in rows {
                let (id, vod_id, start, words, position) = row?;
                let words: Vec<Word> = serde_json::from_str(&words)?;
                let ranges = find_phrase(&words, &q, prefix);
                let time = ranges.first().map_or(start, |&(i, _)| words[i].start);
                let parts = highlight(&words, &ranges);
                let hit = SearchHit { vod_id, source: Source::Speech, id, time, parts, author: None, color: None };
                found.push((position, vod_id, hit));
            }
        }
        if filter.chat {
            let mut stmt = self.conn.prepare(
                "SELECT c.id, c.vod_id, c.time, c.text, c.author, c.color, v.position FROM chat_fts f
                 JOIN chat c ON c.id = f.rowid
                 JOIN vods v ON v.id = c.vod_id
                 WHERE chat_fts MATCH ?1 AND f.rowid BETWEEN ?4 AND ?5 AND (?2 IS NULL OR c.vod_id = ?2)
                 ORDER BY v.position, v.id DESC, c.time
                 LIMIT ?3",
            )?;
            let (lo, hi) = self.id_range("chat", filter.vod_id)?;
            let rows = stmt.query_map(params![fts, filter.vod_id, limit as i64 + 1, lo, hi], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get(6)?,
                ))
            })?;
            for row in rows {
                let (id, vod_id, time, text, author, color, position) = row?;
                // Chat has no word timings; every word is at the message's time.
                let words: Vec<Word> =
                    text.split_whitespace().map(|w| Word { text: w.to_string(), start: time, end: time }).collect();
                let parts = highlight(&words, &find_phrase(&words, &q, prefix));
                let hit = SearchHit { vod_id, source: Source::Chat, id, time, parts, author: Some(author), color };
                found.push((position, vod_id, hit));
            }
        }
        // The same order as each query: library position, newest VOD first, then time.
        found.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.time.total_cmp(&b.2.time)));
        let truncated = found.len() > limit;
        let hits = found.into_iter().take(limit).map(|(_, _, hit)| hit).collect();
        Ok(SearchResults { hits, truncated })
    }
}

/// A VOD's chat in the library, as `chat::fetch` saves it. Holds the library's
/// lock only while it reads or writes, not while the fetch waits on the network.
pub struct LibraryChat<'a> {
    pub db: &'a std::sync::Mutex<Db>,
    pub vod_id: i64,
    /// Told after each batch is saved.
    pub on_save: Box<dyn FnMut() + 'a>,
}

impl crate::chat::ChatStore for LibraryChat<'_> {
    fn resume(&mut self) -> Result<Option<f64>> {
        self.db.lock().unwrap().chat_resume(self.vod_id)
    }

    fn clear(&mut self) -> Result<()> {
        // A part at a time, letting go of the library between them, so a busy
        // chat fetched again doesn't hold everything else up.
        while self.db.lock().unwrap().clear_some_chat(self.vod_id, 1000)? > 0 {
            std::thread::yield_now();
        }
        let db = self.db.lock().unwrap();
        db.clear_chat(self.vod_id)?;
        // The old messages' words out of the index too (some may be gone from
        // the platform since, removed by moderators).
        db.tidy_index("chat_fts")
    }

    fn drop_from(&mut self, time: f64) -> Result<()> {
        self.db.lock().unwrap().drop_chat_from(self.vod_id, time)
    }

    fn save(&mut self, messages: &[ChatMessage], position: f64) -> Result<()> {
        self.db.lock().unwrap().add_chat(self.vod_id, messages, position)?;
        (self.on_save)();
        Ok(())
    }

    fn subs(&mut self) -> Result<Vec<(i64, ChatMessage)>> {
        self.db.lock().unwrap().chat_subs(self.vod_id)
    }

    fn demote(&mut self, ids: &[i64]) -> Result<()> {
        self.db.lock().unwrap().demote_chat(ids)
    }
}

/// Renders words as text runs, marking the matched ranges.
fn highlight(words: &[Word], ranges: &[(usize, usize)]) -> Vec<TextPart> {
    let mut parts: Vec<TextPart> = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let hit = ranges.iter().any(|&(a, b)| i >= a && i <= b);
        let sep = if i == 0 { "" } else { " " };
        match parts.last_mut() {
            Some(p) if p.hit == hit => {
                p.text.push_str(sep);
                p.text.push_str(&w.text);
            }
            // Keep spaces out of highlighted runs so the marks hug the words.
            Some(p) if hit => {
                p.text.push_str(sep);
                parts.push(TextPart { text: w.text.clone(), hit });
            }
            _ => parts.push(TextPart { text: format!("{sep}{}", w.text), hit }),
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::{Segment, Segmenter};

    /// How search results mark their matches, against the cases the website's
    /// demo is tested against too (crates/core/tests/phrase-search.json):
    /// matched runs in [brackets].
    #[test]
    fn shared_highlight_cases() {
        #[derive(serde::Deserialize)]
        struct Case {
            about: String,
            words: Vec<String>,
            ranges: Vec<(usize, usize)>,
            marked: String,
        }
        let cases: Vec<Case> = serde_json::from_str(include_str!("../tests/phrase-search.json")).unwrap();
        for c in cases {
            let words: Vec<Word> = c.words.into_iter().map(|text| Word { text, start: 0.0, end: 0.0 }).collect();
            let marked: String = highlight(&words, &c.ranges)
                .into_iter()
                .map(|p| if p.hit { format!("[{}]", p.text) } else { p.text })
                .collect();
            assert_eq!(marked, c.marked, "{}", c.about);
        }
    }

    fn info(id: &str) -> VodInfo {
        VodInfo {
            platform: "youtube".into(),
            platform_id: id.into(),
            url: format!("https://youtu.be/{id}"),
            title: "t".into(),
            channel: None,
            duration: Some(60.0),
            thumbnail: None,
            published: None,
        }
    }

    fn words(text: &str, start: f64) -> Vec<Word> {
        text.split(' ')
            .enumerate()
            .map(|(i, t)| Word { text: t.into(), start: start + i as f64, end: start + i as f64 + 0.5 })
            .collect()
    }

    #[test]
    fn migrates_v1_databases() {
        let dir = crate::TempDir::new("migrate");
        let path = dir.join("library.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(&format!("{SCHEMA} PRAGMA user_version = 1;")).unwrap();
            conn.execute(
                "INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES ('kick', 'x', 'u', 't', 'done', 0)",
                [],
            )
            .unwrap();
        }
        let mut db = Db::open(&path).unwrap();
        let vod = db.vods().unwrap().remove(0);
        db.conn.execute("UPDATE vods SET position = 5", []).unwrap();
        let later = db.add_vod(&info("later")).unwrap().0;
        assert_eq!(db.vods().unwrap()[0].id, later.id, "added on top of migrated VODs");
        db.delete_vod(later.id).unwrap();
        assert_eq!(vod.note, None);
        assert!(!vod.outdated, "no transcript yet");
        db.insert_segments(vod.id, &[Segment { start: 0.0, end: 1.0, text: "hi".into(), words: Vec::new() }]).unwrap();
        assert!(db.vod(vod.id).unwrap().unwrap().outdated, "transcribed before models were recorded");
        db.set_model(vod.id, Some(crate::model::CPU_MODEL.id)).unwrap();
        assert!(!db.vod(vod.id).unwrap().unwrap().outdated);
        db.set_model(vod.id, Some("sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8")).unwrap();
        assert!(db.vod(vod.id).unwrap().unwrap().outdated, "older model");
        db.set_note(vod.id, Some("ran on CPU")).unwrap();
        assert_eq!(db.vod(vod.id).unwrap().unwrap().note.as_deref(), Some("ran on CPU"));
    }

    #[test]
    fn chat_from_before_kinds_is_fetched_again() {
        let dir = crate::TempDir::new("migrate-v10");
        let path = dir.join("library.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            let before = [
                SCHEMA, MIGRATE_V2, MIGRATE_V3, MIGRATE_V4, MIGRATE_V5, MIGRATE_V6, MIGRATE_V7, MIGRATE_V8, MIGRATE_V9,
            ];
            conn.execute_batch(&format!("{} PRAGMA user_version = 9;", before.join("\n"))).unwrap();
            conn.execute_batch(
                "INSERT INTO vods (platform, platform_id, url, title, status, added_at, chat) VALUES
                     ('kick', 'a', 'u', 't', 'done', 0, 'done'), ('local', 'b', 'u', 't', 'done', 0, 'none');
                 INSERT INTO chat (vod_id, time, author, text, parts) VALUES (1, 1.0, 'v', 'hi', '[]');",
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let chat: Vec<_> = db.vods().unwrap().iter().map(|v| v.chat).collect();
        assert!(chat.contains(&ChatStatus::Pending) && chat.contains(&ChatStatus::None));
        assert_eq!(db.chat(1).unwrap()[0].kind, Kind::Message, "kept until fetched again");
    }

    #[test]
    fn canceled_vods_say_stopped() {
        let dir = crate::TempDir::new("migrate-v12");
        let path = dir.join("library.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            let before = [
                SCHEMA,
                MIGRATE_V2,
                MIGRATE_V3,
                MIGRATE_V4,
                MIGRATE_V5,
                MIGRATE_V6,
                MIGRATE_V7,
                MIGRATE_V8,
                MIGRATE_V9,
                MIGRATE_V10,
                MIGRATE_V11,
            ];
            conn.execute_batch(&format!("{} PRAGMA user_version = 11;", before.join("\n"))).unwrap();
            conn.execute_batch(
                "INSERT INTO vods (platform, platform_id, url, title, status, error, added_at) VALUES
                     ('kick', 'a', 'u', 't', 'error', 'Canceled', 0), ('kick', 'b', 'u', 't', 'error', 'No audio', 0);",
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let errors: Vec<_> = db.vods().unwrap().into_iter().map(|v| v.error).collect();
        assert!(errors.contains(&Some("Stopped".into())) && errors.contains(&Some("No audio".into())));
    }

    #[test]
    fn deleted_transcripts_leave_the_file() {
        let dir = crate::TempDir::new("delete");
        let path = dir.join("library.sqlite");
        let mut db = Db::open(&path).unwrap();
        // Another VOD stays, sharing the search indexes with the one removed.
        let kept = db.add_vod(&info("kept")).unwrap().0;
        let vod = db.add_vod(&info("x")).unwrap().0;
        let secret = "zanzibarquokka";
        for (i, id) in [kept.id, vod.id, kept.id].into_iter().enumerate() {
            let text = if id == vod.id { secret.to_string() } else { format!("plain words {i}") };
            db.insert_segments(
                id,
                &[Segment { start: i as f64, end: 1.0, text: text.clone(), words: words(&text, 0.0) }],
            )
            .unwrap();
            db.add_chat(id, &[message(i as f64, "v", &format!("chat{text}"))], i as f64).unwrap();
        }
        assert_eq!(db.search(secret, &SearchFilter::default(), 10).unwrap().hits.len(), 1);
        while db.delete_some_of(vod.id, 1000).unwrap() > 0 {}
        // A batch saved after the erase, by a fetch that hadn't stopped yet.
        db.add_chat(vod.id, &[message(9.0, "v", "late quokkazanzibar")], 9.0).unwrap();
        db.delete_vod(vod.id).unwrap();
        assert_eq!(db.search("words", &SearchFilter::default(), 10).unwrap().hits.len(), 4, "the other VOD's stay");
        drop(db);
        let bytes =
            [std::fs::read(&path).unwrap(), std::fs::read(path.with_extension("sqlite-wal")).unwrap_or_default()]
                .concat();
        assert!(!bytes.windows(8).any(|w| w == &secret.as_bytes()[6..]), "deleted text still on disk");
        assert!(!bytes.windows(8).any(|w| w == b"zanzibar"), "a late batch's text still on disk");
    }

    #[test]
    fn status_names_round_trip() {
        use Status::*;
        for status in [Queued, Downloading, Ready, Transcribing, Paused, Done, Error] {
            assert_eq!(Status::parse(status.as_str()), status);
        }
    }

    #[test]
    fn keeps_the_library_order() {
        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let a = db.add_vod(&info("a")).unwrap().0.id;
        let b = db.add_vod(&info("b")).unwrap().0.id;
        let ids = |db: &Db| db.vods().unwrap().iter().map(|v| v.id).collect::<Vec<_>>();
        assert_eq!(ids(&db), [b, a], "newest first");
        db.reorder(&[a, b]).unwrap();
        assert_eq!(ids(&db), [a, b]);
        let c = db.add_vod(&info("c")).unwrap().0.id;
        assert_eq!(ids(&db), [c, a, b], "added on top");
        assert_eq!(db.next_with_status(Status::Queued).unwrap().unwrap().id, b, "the queue runs from the bottom");
        db.reorder(&[b, c, a]).unwrap();
        assert_eq!(db.next_with_status(Status::Queued).unwrap().unwrap().id, a);
    }

    #[test]
    fn interrupted_jobs_return_to_their_queues() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let [down, trans, ready, done] = ["d", "t", "r", "x"].map(|id| db.add_vod(&info(id)).unwrap().0.id);
        db.set_status(down, Status::Downloading, 0.4, None).unwrap();
        db.set_status(trans, Status::Transcribing, 0.6, None).unwrap();
        db.set_status(ready, Status::Ready, 0.0, None).unwrap();
        db.set_status(done, Status::Done, 1.0, None).unwrap();
        let status = |id| db.vod(id).unwrap().unwrap().status;

        // A quit mid-stage: each VOD goes back to wait for the stage it was in.
        db.requeue_interrupted().unwrap();
        assert_eq!(
            [status(down), status(trans), status(ready), status(done)],
            [Status::Queued, Status::Ready, Status::Ready, Status::Done]
        );

        // "Free up space" deleted the audio: VODs waiting to transcribe must download again.
        db.set_audio_path(ready, Some("/work/r.m4a")).unwrap();
        db.forget_downloads().unwrap();
        assert_eq!(db.audio_path(ready).unwrap(), None);
        assert_eq!([status(trans), status(ready), status(done)], [Status::Queued, Status::Queued, Status::Done]);
    }

    #[test]
    fn local_files_queue_separately() {
        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let [link, file_a, file_b] = ["l", "a", "b"].map(|id| db.add_vod(&info(id)).unwrap().0.id);
        db.set_source_path(file_a, "/videos/a.mp4").unwrap();
        db.set_source_path(file_b, "/videos/b.mp4").unwrap();
        // Bottom of the list first, as for links, but never a link.
        db.reorder(&[file_a, file_b, link]).unwrap();
        assert_eq!(db.next_with_status(Status::Queued).unwrap().unwrap().id, link);
        assert_eq!(db.next_local_with_status(Status::Queued).unwrap().unwrap().id, file_b);
        db.reorder(&[file_b, file_a, link]).unwrap();
        assert_eq!(db.next_local_with_status(Status::Queued).unwrap().unwrap().id, file_a);
        db.set_status(file_a, Status::Done, 1.0, None).unwrap();
        db.set_status(file_b, Status::Done, 1.0, None).unwrap();
        assert!(db.next_local_with_status(Status::Queued).unwrap().is_none());
    }

    #[test]
    fn index_and_search() {
        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let (vod, created) = db.add_vod(&info("a")).unwrap();
        assert!(created);
        assert!(!db.add_vod(&info("a")).unwrap().1);

        let mut seg = Segmenter::new();
        let mut segs = seg.push(words("Welcome back everyone.", 0.0));
        segs.extend(seg.push(words("That was a clutch play, honestly.", 10.0)));
        db.insert_segments(vod.id, &segs).unwrap();

        let r = db.search("CLUTCH play", &SearchFilter::default(), 10).unwrap();
        assert_eq!(r.hits.len(), 1);
        assert_eq!(r.hits[0].time, 13.0);
        let parts: Vec<_> = r.hits[0].parts.iter().map(|p| (p.text.as_str(), p.hit)).collect();
        assert_eq!(parts, [("That was a ", false), ("clutch play,", true), (" honestly.", false)]);

        assert_eq!(db.search("welc", &SearchFilter::default(), 10).unwrap().hits.len(), 1, "prefix match");
        assert!(db.search("play clutch", &SearchFilter::default(), 10).unwrap().hits.is_empty(), "phrase order");
        assert!(db.search("\" OR *", &SearchFilter::default(), 10).unwrap().hits.is_empty(), "no FTS syntax injection");

        db.delete_vod(vod.id).unwrap();
        assert!(db.search("clutch", &SearchFilter::default(), 10).unwrap().hits.is_empty(), "cascade clears index");
    }

    fn message(time: f64, author: &str, text: &str) -> ChatMessage {
        ChatMessage {
            time,
            author: author.into(),
            color: Some("#ff0000".into()),
            parts: vec![ChatPart::Text { text: text.into() }],
            kind: Kind::Message,
            amount: None,
            event_time: None,
        }
    }

    #[test]
    fn one_letter_matches_a_word_and_a_vods_search_keeps_to_it() {
        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let a = db.add_vod(&info("a")).unwrap().0.id;
        let b = db.add_vod(&info("b")).unwrap().0.id;
        // Saved in turns, so b's messages sit between a's ids.
        db.add_chat(a, &[message(1.0, "v", "an apple a day")], 1.0).unwrap();
        db.add_chat(b, &[message(2.0, "v", "apple pie")], 2.0).unwrap();
        db.add_chat(a, &[message(3.0, "v", "apples again")], 3.0).unwrap();
        let chat = |query: &str, vod_id: Option<i64>| {
            let filter = SearchFilter { vod_id, speech: false, chat: true };
            db.search(query, &filter, 10).unwrap().hits.iter().map(|h| (h.vod_id, h.time)).collect::<Vec<_>>()
        };
        assert_eq!(chat("a", None), [(a, 1.0)], "one letter: the word, not every word starting with it");
        assert_eq!(chat("ap", Some(a)), [(a, 1.0), (a, 3.0)], "a's own, though b's lie between them");
        assert_eq!(chat("ap", Some(b)), [(b, 2.0)]);
        let ids: Vec<f64> = db.chat_matches(a, &["apple".into()]).unwrap().into_iter().map(|(_, t)| t).collect();
        assert_eq!(ids, [1.0], "matches keep to the VOD too");
    }

    #[test]
    fn reads_dont_wait_for_writes() {
        let dir = crate::TempDir::new("reader");
        let path = dir.join("library.sqlite");
        let db = Db::open(&path).unwrap();
        db.add_vod(&info("a")).unwrap();
        let reader = Db::open_reader(&path).unwrap();
        // A write under way on the other connection, not yet committed.
        db.conn.execute_batch("BEGIN IMMEDIATE; DELETE FROM vods;").unwrap();
        let started = std::time::Instant::now();
        assert_eq!(reader.vods().unwrap().len(), 1, "what was there before the write");
        assert!(started.elapsed() < std::time::Duration::from_millis(500), "didn't wait for the writer");
        db.conn.execute_batch("COMMIT;").unwrap();
        assert!(reader.vods().unwrap().is_empty(), "and the write once it's done");
        assert!(reader.add_vod(&info("b")).is_err(), "it only reads");
    }

    #[test]
    fn a_chat_fetched_again_is_cleared_in_parts_and_resubs_demoted_at_once() {
        use crate::chat::ChatStore;
        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let a = db.add_vod(&info("a")).unwrap().0.id;
        let b = db.add_vod(&info("b")).unwrap().0.id;
        let many: Vec<_> = (0..2500).map(|i| message(i as f64, "v", "hi")).collect();
        db.add_chat(a, &many, 2500.0).unwrap();
        let mut subs = message(1.0, "s", "resub");
        subs.kind = Kind::Sub;
        db.add_chat(b, &[subs.clone(), subs.clone(), subs], 2.0).unwrap();
        let lock = std::sync::Mutex::new(db);
        let mut store = LibraryChat { db: &lock, vod_id: b, on_save: Box::new(|| {}) };
        let ids: Vec<i64> = store.subs().unwrap().into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids.len(), 3);
        store.demote(&ids[..2]).unwrap();
        assert_eq!(store.subs().unwrap().len(), 1);
        let mut store = LibraryChat { db: &lock, vod_id: a, on_save: Box::new(|| {}) };
        store.clear().unwrap();
        let db = lock.lock().unwrap();
        assert_eq!(db.chat_count(a).unwrap(), 0);
        assert_eq!(db.chat_resume(a).unwrap(), None);
        assert_eq!(db.chat_count(b).unwrap(), 3, "another VOD's chat stays");
    }

    #[test]
    fn stores_and_searches_chat() {
        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let a = db.add_vod(&info("a")).unwrap().0.id;
        let b = db.add_vod(&info("b")).unwrap().0.id;
        assert_eq!(db.vod(a).unwrap().unwrap().chat, ChatStatus::Pending);
        assert_eq!(db.chat_pending().unwrap(), [b, a], "library order");

        let mut seg = Segmenter::new();
        let mut segs = seg.push(words("what a clutch play", 20.0));
        segs.extend(seg.finish());
        db.insert_segments(a, &segs).unwrap();
        let emote =
            ChatPart::Emote { name: "barkylove".into(), url: "https://files.kick.com/emotes/1/fullsize".into() };
        let chat = [
            message(25.0, "viewer", "CLUTCH PLAY!!"),
            ChatMessage { parts: vec![emote.clone()], ..message(5.0, "fan", "") },
        ];
        db.set_chat(a, &chat).unwrap();
        db.set_chat(b, &[message(1.0, "other", "clutch play elsewhere")]).unwrap();
        assert_eq!(db.vod(a).unwrap().unwrap().chat, ChatStatus::Done);
        assert!(db.chat_pending().unwrap().is_empty());

        let lines = db.chat(a).unwrap();
        assert_eq!(lines.iter().map(|l| l.time).collect::<Vec<_>>(), [5.0, 25.0], "oldest first");

        // What the transcript view asks for: rows' windows, counts per stretch, events.
        db.add_chat(a, &[message(15.0, "late", "arrived later"), message(16.0, "later", "and again")], 0.0).unwrap();
        let windows = db.chat_windows(a, &[(-1e12, 10.0), (10.0, 20.0), (20.0, 1e12)], 1).unwrap();
        let shown: Vec<_> =
            windows.iter().map(|w| (w.messages.iter().map(|m| m.time).collect::<Vec<_>>(), w.total)).collect();
        assert_eq!(shown, [(vec![5.0], 1), (vec![15.0], 2), (vec![25.0], 1)], "the first of each, and how many");
        assert_eq!(db.chat_histogram(a, 30.0, 3).unwrap(), [1, 2, 1]);
        assert_eq!(db.chat_count(a).unwrap(), 4);
        assert_eq!(
            db.chat_emotes(a).unwrap(),
            [("barkylove".to_string(), "https://files.kick.com/emotes/1/fullsize".to_string())]
        );
        db.set_chat(a, &chat).unwrap();
        let lines = db.chat(a).unwrap();
        assert_eq!(lines[1].kind, Kind::Message);
        assert_eq!(lines[0].parts, [emote]);

        let search = |filter: SearchFilter| {
            let hits = db.search("clutch play", &filter, 10).unwrap().hits;
            hits.into_iter().map(|h| (h.vod_id, h.source, h.time)).collect::<Vec<_>>()
        };
        assert_eq!(
            search(SearchFilter::default()),
            [(b, Source::Chat, 1.0), (a, Source::Speech, 22.0), (a, Source::Chat, 25.0)],
            "library order (newest on top), then time"
        );
        assert_eq!(search(SearchFilter { chat: false, ..Default::default() }), [(a, Source::Speech, 22.0)]);
        assert_eq!(
            search(SearchFilter { speech: false, vod_id: Some(a), ..Default::default() }),
            [(a, Source::Chat, 25.0)]
        );
        assert!(search(SearchFilter { speech: false, chat: false, ..Default::default() }).is_empty());
        let hit = db.search("barkylove", &SearchFilter::default(), 10).unwrap().hits.remove(0);
        assert_eq!((hit.author.as_deref(), hit.color.as_deref()), (Some("fan"), Some("#ff0000")));
        let parts: Vec<_> = db
            .search("clutch", &SearchFilter { speech: false, vod_id: Some(a), ..Default::default() }, 10)
            .unwrap()
            .hits[0]
            .parts
            .iter()
            .map(|p| (p.text.clone(), p.hit))
            .collect();
        assert_eq!(parts, [("CLUTCH".to_string(), true), (" PLAY!!".to_string(), false)]);

        // Layers match whole words and phrases, in any of their terms.
        let times = |terms: &[&str]| {
            let terms: Vec<String> = terms.iter().map(|t| t.to_string()).collect();
            db.chat_matches(a, &terms).unwrap().into_iter().map(|(_, time)| time).collect::<Vec<_>>()
        };
        assert_eq!(times(&["clutch play"]), [25.0]);
        assert_eq!(times(&["BARKYLOVE", "nothing"]), [5.0], "an emote by name, any case");
        assert_eq!(times(&["clut"]), Vec::<f64>::new(), "whole words, not prefixes");
        assert_eq!(times(&["play", "barkylove"]), [5.0, 25.0]);
        assert_eq!(times(&["\" OR *", ""]), Vec::<f64>::new(), "no FTS syntax injection");

        // Fetching again replaces the chat; a failure is retried; deleting the VOD clears it.
        db.set_chat(a, &[message(30.0, "viewer", "gg")]).unwrap();
        assert_eq!(db.chat(a).unwrap().len(), 1);
        db.set_chat_status(b, ChatStatus::Failed, Some("Kick isn't responding")).unwrap();
        assert_eq!(db.chat_pending().unwrap(), [b]);
        while db.delete_some_of(a, 1).unwrap() > 0 {}
        assert!(db.chat(a).unwrap().is_empty() && db.lines(a).unwrap().is_empty(), "deleted a part at a time");
        assert!(db.vod(a).unwrap().is_some(), "the VOD itself stays until delete_vod");
        db.delete_vod(a).unwrap();
        assert!(db.search("gg", &SearchFilter::default(), 10).unwrap().hits.is_empty());
    }

    #[test]
    fn a_chat_search_is_limited_too() {
        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let a = db.add_vod(&info("a")).unwrap().0.id;
        let chat: Vec<_> = (0..5).map(|i| message(i as f64, "v", "hype")).collect();
        db.set_chat(a, &chat).unwrap();
        let r = db.search("hype", &SearchFilter::default(), 3).unwrap();
        assert_eq!((r.hits.len(), r.truncated), (3, true));
    }
}
