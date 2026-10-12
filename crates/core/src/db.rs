use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;

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
    /// Has transcript segments from a model this version no longer uses (or
    /// from before models were recorded). Finished VODs can be re-transcribed;
    /// unfinished ones start over when they resume.
    pub outdated: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct TextPart {
    pub text: String,
    pub hit: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct SearchHit {
    pub vod_id: i64,
    pub segment_id: i64,
    /// Start of the matched phrase itself, or of the segment if it couldn't be located.
    pub time: f64,
    pub parts: Vec<TextPart>,
}

pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    pub truncated: bool,
}

pub struct Db {
    conn: Connection,
}

const SCHEMA_VERSION: i32 = 4;

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

const VOD_COLUMNS: &str =
    "id, platform, platform_id, url, title, channel, duration, thumbnail, published, status, progress, error, added_at, note, model,
     EXISTS (SELECT 1 FROM segments WHERE segments.vod_id = vods.id)";

fn vod_from_row(r: &Row) -> rusqlite::Result<Vod> {
    Ok(Vod {
        id: r.get(0)?,
        info: VodInfo {
            platform: r.get(1)?,
            platform_id: r.get(2)?,
            url: r.get(3)?,
            title: r.get(4)?,
            channel: r.get(5)?,
            duration: r.get(6)?,
            thumbnail: r.get(7)?,
            published: r.get(8)?,
        },
        status: Status::parse(&r.get::<_, String>(9)?),
        progress: r.get(10)?,
        error: r.get(11)?,
        added_at: r.get(12)?,
        note: r.get(13)?,
        outdated: {
            let model: Option<String> = r.get(14)?;
            let has_segments: bool = r.get(15)?;
            has_segments && !model.is_some_and(|m| crate::model::is_current(&m))
        },
    })
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let migrations = [SCHEMA, MIGRATE_V2, MIGRATE_V3, MIGRATE_V4];
        if (version as usize) < migrations.len() {
            let pending = migrations[version as usize..].join("\n");
            conn.execute_batch(&format!("BEGIN; {pending} PRAGMA user_version = {SCHEMA_VERSION}; COMMIT;"))?;
        }
        Ok(Self { conn })
    }

    /// Inserts a VOD, or returns the existing row if it was already added.
    pub fn add_vod(&self, info: &VodInfo) -> Result<(Vod, bool)> {
        if let Some(existing) = self.find_vod(&info.platform, &info.platform_id)? {
            return Ok((existing, false));
        }
        self.conn.execute(
            "INSERT INTO vods (platform, platform_id, url, title, channel, duration, thumbnail, published, status, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'queued', ?9)",
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
        let sql = format!("SELECT {VOD_COLUMNS} FROM vods ORDER BY added_at DESC, id DESC");
        let mut stmt = self.conn.prepare(&sql)?;
        Ok(stmt.query_map([], vod_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// The oldest VOD with `status`, which is the next one for that stage.
    pub fn next_with_status(&self, status: Status) -> Result<Option<Vod>> {
        let sql = format!("SELECT {VOD_COLUMNS} FROM vods WHERE status = ?1 ORDER BY added_at, id LIMIT 1");
        Ok(self.conn.query_row(&sql, [status.as_str()], vod_from_row).optional()?)
    }

    pub fn count_with_status(&self, status: Status) -> Result<usize> {
        let n: i64 = self.conn.query_row("SELECT count(*) FROM vods WHERE status = ?1", [status.as_str()], |r| r.get(0))?;
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

    pub fn set_audio_path(&self, id: i64, path: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE vods SET audio_path = ?2 WHERE id = ?1", params![id, path])?;
        Ok(())
    }

    /// How far transcription got: the end of the last indexed segment.
    pub fn transcribed_until(&self, vod_id: i64) -> Result<f64> {
        Ok(self.conn.query_row("SELECT coalesce(max(end), 0) FROM segments WHERE vod_id = ?1", [vod_id], |r| r.get(0))?)
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

    pub fn set_thumbnail(&self, id: i64, thumbnail: &str) -> Result<()> {
        self.conn.execute("UPDATE vods SET thumbnail = ?2 WHERE id = ?1", params![id, thumbnail])?;
        Ok(())
    }

    pub fn set_note(&self, id: i64, note: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE vods SET note = ?2 WHERE id = ?1", params![id, note])?;
        Ok(())
    }

    pub fn delete_vod(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM vods WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn clear_segments(&self, vod_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM segments WHERE vod_id = ?1", [vod_id])?;
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

    /// Phrase search. The last term matches as a prefix so results update while typing.
    /// Hits are ordered by VOD (newest first), then by time within the VOD.
    pub fn search(&self, query: &str, vod_id: Option<i64>, limit: usize) -> Result<SearchResults> {
        let q = terms(query);
        if q.is_empty() {
            return Ok(SearchResults { hits: Vec::new(), truncated: false });
        }
        // Terms are alphanumeric only, so quoting them can't inject FTS syntax.
        let fts = format!("\"{}\"*", q.join(" "));
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.vod_id, s.start, s.words FROM segments_fts f
             JOIN segments s ON s.id = f.rowid
             JOIN vods v ON v.id = s.vod_id
             WHERE segments_fts MATCH ?1 AND (?2 IS NULL OR s.vod_id = ?2)
             ORDER BY v.added_at DESC, v.id DESC, s.start
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![fts, vod_id, limit as i64 + 1], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, f64>(2)?, r.get::<_, String>(3)?))
        })?;
        let mut hits = Vec::new();
        for row in rows {
            let (segment_id, vod_id, start, words) = row?;
            let words: Vec<Word> = serde_json::from_str(&words)?;
            let ranges = find_phrase(&words, &q, true);
            let time = ranges.first().map_or(start, |&(i, _)| words[i].start);
            hits.push(SearchHit { vod_id, segment_id, time, parts: highlight(&words, &ranges) });
        }
        let truncated = hits.len() > limit;
        hits.truncate(limit);
        Ok(SearchResults { hits, truncated })
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
        let path = std::env::temp_dir().join(format!("vodscribe-migrate-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
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
        drop(db);
        let _ = std::fs::remove_file(&path);
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

        let r = db.search("CLUTCH play", None, 10).unwrap();
        assert_eq!(r.hits.len(), 1);
        assert_eq!(r.hits[0].time, 13.0);
        let parts: Vec<_> = r.hits[0].parts.iter().map(|p| (p.text.as_str(), p.hit)).collect();
        assert_eq!(parts, [("That was a ", false), ("clutch play,", true), (" honestly.", false)]);

        assert_eq!(db.search("welc", None, 10).unwrap().hits.len(), 1, "prefix match");
        assert!(db.search("play clutch", None, 10).unwrap().hits.is_empty(), "phrase order");
        assert!(db.search("\" OR *", None, 10).unwrap().hits.is_empty(), "no FTS syntax injection");

        db.delete_vod(vod.id).unwrap();
        assert!(db.search("clutch", None, 10).unwrap().hits.is_empty(), "cascade clears index");
    }
}
