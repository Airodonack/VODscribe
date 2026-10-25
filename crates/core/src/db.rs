use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::chat::{ChatMessage, ChatPart, Kind};
use crate::fetch::VodInfo;
use crate::stretches::Transcribed;
use crate::time::{Flicks, Rate};
use crate::transcript::{Segment, Word};

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
    /// No request: nothing of the VOD waits or runs, and not all of it is
    /// transcribed. Only a `Vod`'s status, never a stretch's.
    None,
}

impl Status {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Status::Queued => "queued",
            Status::Downloading => "downloading",
            Status::Ready => "ready",
            Status::Transcribing => "transcribing",
            Status::Paused => "paused",
            Status::Done => "done",
            Status::Error => "error",
            Status::None => "none",
        }
    }

    pub(crate) fn parse(s: &str) -> Self {
        match s {
            "queued" => Status::Queued,
            "downloading" => Status::Downloading,
            "ready" => Status::Ready,
            "transcribing" => Status::Transcribing,
            "paused" => Status::Paused,
            "done" => Status::Done,
            "none" => Status::None,
            _ => Status::Error,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct Vod {
    pub id: i64,
    #[serde(flatten)]
    pub info: VodInfo,
    /// The status of the stretch the row shows (`stretches::summarize`):
    /// running first, then the first waiting, then paused, then the latest
    /// failed; with no request, Done if all of it is transcribed, else None.
    pub status: Status,
    /// 0..1 within the current status, within that stretch.
    pub progress: f64,
    pub error: Option<String>,
    /// That stretch's id; None when there's no request.
    pub stretch: Option<i64>,
    /// How much of it is transcribed: none, some or all.
    pub transcribed: Transcribed,
    /// How many requests there are besides the one shown.
    pub waiting: usize,
    pub added_at: i64,
    pub note: Option<String>,
    /// File name of the local thumbnail copy in `Paths::thumbs` (the UI fetches it
    /// with the `thumbnail` command). `None` until fetched; `""` if the VOD has no
    /// usable image, so it isn't tried again.
    pub thumb: Option<String>,
    /// For a file on this computer, its path. It's read in place, never changed.
    pub source_path: Option<String>,
    /// A video (or audio) kept to play here, from a site other than YouTube,
    /// Twitch and Kick (`Paths::media`); the app plays it like a local file.
    pub media: bool,
    /// What plays here (a local file, or the kept video) is audio alone, by its
    /// file type: the player needs no picture.
    pub audio_only: bool,
    /// Has transcript segments from a model this version no longer uses (or
    /// from before models were recorded): a stretch done, or begun, with one.
    /// Finished VODs can be re-transcribed; unfinished ones start over when
    /// they resume.
    pub outdated: bool,
    pub chat: ChatStatus,
    /// Why fetching the chat failed, when it did (`ChatStatus::Failed`); what
    /// was fetched before it stopped is kept.
    pub chat_error: Option<String>,
    /// In the library's list. One taken out stays a source, unlisted, while
    /// other clips use it; its work waits until it's listed again.
    pub listed: bool,
    /// Whether the file host has annotations for it (`annotations::look_up`),
    /// as last asked: the library offers them when it has.
    pub annotations: Availability,
}

/// Whether annotations are published for a VOD, as the app last found out.
#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Availability {
    /// Never asked, or the asking failed (offline, an error).
    #[default]
    Unknown,
    None,
    Available,
}

impl Availability {
    pub(crate) fn as_str(self) -> Option<&'static str> {
        match self {
            Availability::Unknown => None,
            Availability::None => Some("none"),
            Availability::Available => Some("available"),
        }
    }

    pub(crate) fn parse(s: Option<&str>) -> Self {
        match s {
            Some("none") => Availability::None,
            Some("available") => Availability::Available,
            _ => Availability::Unknown,
        }
    }
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
    /// Fetching failed; `chat_error` says why, and it's tried again later,
    /// carrying on from where it stopped. What it fetched is kept, and shown.
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
    /// The window a match's pieces must fit in, in seconds, unless `within:`
    /// is typed; None for the default.
    pub within: Option<f64>,
    /// Who, unless `from:` is typed in the alternative: as `from:` does.
    pub from: Option<String>,
}

impl Default for SearchFilter {
    fn default() -> Self {
        SearchFilter { vod_id: None, speech: true, chat: true, within: None, from: None }
    }
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
        Ok(ChatLine {
            id,
            time,
            author,
            color,
            parts: serde_json::from_str(&parts)?,
            kind: Kind::parse(&kind),
            amount,
            platform: None,
        })
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
    /// The site its VOD is from, where the list mixes VODs' (an edit's chat).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
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
    /// Where the match starts: its first matched word. For chat, when the
    /// message was sent.
    pub time: f64,
    /// Where the match ends: its last matched word's end (for chat, `time`).
    pub end: f64,
    /// How many lines the match runs over, from the first matched word's to
    /// the last's: `parts` is their words, one after another.
    pub lines: usize,
    pub parts: Vec<TextPart>,
    /// For chat, who wrote it, and their name color.
    pub author: Option<String>,
    pub color: Option<String>,
}

pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    pub truncated: bool,
    /// What the typed text set (`query.rs`), for the page to show.
    pub query: crate::query::QueryInfo,
}

pub struct Db {
    pub(crate) conn: Connection,
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
/// v14: a deleted VOD's id is never given to a new one, so work still running
/// for the deleted VOD (its chat or thumbnail being fetched) can't land on the
/// new one. `vod_ids` holds the next id; `add_vod` takes it, and the trigger
/// moves it past any id inserted another way.
const MIGRATE_V14: &str = "
CREATE TABLE vod_ids (next INTEGER NOT NULL);
INSERT INTO vod_ids SELECT coalesce(max(id), 0) + 1 FROM vods;
CREATE TRIGGER vods_ids AFTER INSERT ON vods BEGIN
    UPDATE vod_ids SET next = max(next, NEW.id + 1);
END;";
/// v15: a chat is fetched in slices at once (`chat::in_slices`), so an
/// unfinished fetch records where each slice got to, as a JSON array.
/// `chat_resume` (one position) is only cleared now: a fetch an older version
/// left unfinished starts over, and so does one an older version touched since
/// (`chat_slices`).
const MIGRATE_V15: &str = "ALTER TABLE vods ADD COLUMN chat_slices TEXT;";
/// v16: the video kept to play here, for sites other than YouTube, Twitch and
/// Kick (`Paths::media`).
const MIGRATE_V16: &str = "ALTER TABLE vods ADD COLUMN media_path TEXT;";
/// v17: clips, ranges of a VOD made into video files of their own (`clips`).
const MIGRATE_V17: &str = "
CREATE TABLE clips (
    id INTEGER PRIMARY KEY,
    vod_id INTEGER NOT NULL REFERENCES vods (id) ON DELETE CASCADE,
    start REAL NOT NULL,
    end REAL NOT NULL,
    title TEXT NOT NULL,
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'waiting',
    file_path TEXT,
    error TEXT,
    shown INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX clips_by_vod ON clips (vod_id, position);";
/// v18: marks on a VOD's timeline. A "cut" is a point between two frames,
/// splitting the VOD into stretches that can each become a clip; `end`,
/// `text` and `source` leave room for marks of other kinds.
const MIGRATE_V18: &str = "
CREATE TABLE marks (
    id INTEGER PRIMARY KEY,
    vod_id INTEGER NOT NULL REFERENCES vods (id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    start REAL NOT NULL,
    end REAL,
    text TEXT,
    source TEXT NOT NULL DEFAULT 'user',
    created_at INTEGER NOT NULL
);
CREATE INDEX marks_by_vod ON marks (vod_id, start);";
/// v19: edits, each one video made of stretches of clips, one after another
/// (`crate::edits`). `version` counts changes, and `rendered_version` is the
/// one the rendered file is of.
const MIGRATE_V19: &str = "
CREATE TABLE edits (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'waiting',
    file_path TEXT,
    error TEXT,
    rendered_version INTEGER
);
CREATE TABLE edit_pieces (
    id INTEGER PRIMARY KEY,
    edit_id INTEGER NOT NULL REFERENCES edits (id) ON DELETE CASCADE,
    clip_id INTEGER NOT NULL REFERENCES clips (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    start REAL NOT NULL,
    end REAL NOT NULL,
    struck TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX edit_pieces_by_edit ON edit_pieces (edit_id, position);
CREATE INDEX edit_pieces_by_clip ON edit_pieces (clip_id);";
/// v20: an edit's pieces are their own stretches of VODs, with a title,
/// rather than of clips (so one can change without another); clips record
/// whether their file was dragged out of the app (`handed_out`), as then it's
/// another app's to keep.
const MIGRATE_V20: &str = "
CREATE TABLE pieces (
    id INTEGER PRIMARY KEY,
    edit_id INTEGER NOT NULL REFERENCES edits (id) ON DELETE CASCADE,
    vod_id INTEGER NOT NULL REFERENCES vods (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    start REAL NOT NULL,
    end REAL NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    struck TEXT NOT NULL DEFAULT '[]'
);
INSERT INTO pieces (id, edit_id, vod_id, position, start, end, title, struck)
    SELECT p.id, p.edit_id, c.vod_id, p.position, max(p.start, c.start), min(p.end, c.end), c.title, p.struck
    FROM edit_pieces p JOIN clips c ON c.id = p.clip_id;
DROP TABLE edit_pieces;
ALTER TABLE pieces RENAME TO edit_pieces;
CREATE INDEX edit_pieces_by_edit ON edit_pieces (edit_id, position);
ALTER TABLE clips ADD COLUMN handed_out INTEGER NOT NULL DEFAULT 0;";

/// v21: edits record whether their rendered video was dragged out of the app
/// (`handed_out`), as clips do: a render to a new name then leaves it be.
const MIGRATE_V21: &str = "ALTER TABLE edits ADD COLUMN handed_out INTEGER NOT NULL DEFAULT 0;";

/// Flicks in a second (`crate::time::PER_SECOND`), for the migration's SQL.
const FLICKS: &str = "705600000";

/// v22: the clip model (`crate::edits`). `clips` holds every document: a
/// VOD's own clip over the whole VOD (`whole_of`, with the VOD's place in
/// the library's list, which `vods.position` held), the edits (listed,
/// `position`), and the clip bar's clips, now snippets hanging off their
/// VOD's clip (`bar_of`). `items` holds their pieces (and groups, later),
/// with times in flicks; the struck stretches, JSON seconds till now, are
/// put in flicks by `Db::open` in the same transaction. Marks belong to a
/// clip (the VOD's own, for those made so far) and keep the VOD moment
/// they're at. Clips keep their ids (their pictures are named by them);
/// VODs' clips and edits get ids above them. VODs get their frame rate.
const MIGRATE_V22: &str = "
ALTER TABLE clips RENAME TO clips_old;
CREATE TABLE clips (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0,
    fps_num INTEGER,
    fps_den INTEGER,
    position INTEGER,
    bar_of INTEGER REFERENCES clips (id) ON DELETE CASCADE,
    bar_position INTEGER,
    whole_of INTEGER REFERENCES vods (id) ON DELETE CASCADE,
    shown INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL DEFAULT 'waiting',
    file_path TEXT,
    error TEXT,
    rendered_revision INTEGER,
    handed_out INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX clips_bar ON clips (bar_of, bar_position);
CREATE UNIQUE INDEX clips_whole ON clips (whole_of) WHERE whole_of IS NOT NULL;
INSERT INTO clips (id, title, created_at, bar_position, shown, status, file_path, error, handed_out)
    SELECT id, title, created_at, position, shown, status, file_path, error, handed_out FROM clips_old;
INSERT INTO clips (id, title, created_at, position, whole_of)
    SELECT (SELECT coalesce(max(id), 0) FROM clips_old) + id, title, added_at, position, id FROM vods;
UPDATE clips SET bar_of = (SELECT t.id FROM clips t JOIN clips_old o ON t.whole_of = o.vod_id WHERE o.id = clips.id)
    WHERE id IN (SELECT id FROM clips_old);
INSERT INTO clips (id, title, created_at, position, revision, status, file_path, error, rendered_revision, handed_out)
    SELECT (SELECT coalesce(max(id), 0) FROM clips_old) + (SELECT coalesce(max(id), 0) FROM vods) + id,
           title, created_at, position, version, status, file_path, error, rendered_version, handed_out
    FROM edits;
CREATE TABLE items (
    id INTEGER PRIMARY KEY,
    clip_id INTEGER NOT NULL REFERENCES clips (id) ON DELETE CASCADE,
    parent INTEGER REFERENCES items (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    kind TEXT NOT NULL,
    vod_id INTEGER REFERENCES vods (id) ON DELETE CASCADE,
    start INTEGER,
    end INTEGER,
    struck TEXT NOT NULL DEFAULT '[]',
    title TEXT NOT NULL DEFAULT '',
    origin INTEGER,
    fps_num INTEGER,
    fps_den INTEGER
);
CREATE INDEX items_by_clip ON items (clip_id, parent, position);
CREATE INDEX items_by_vod ON items (vod_id);
INSERT INTO items (id, clip_id, position, kind, vod_id, start, end, struck, title)
    SELECT p.id, (SELECT coalesce(max(id), 0) FROM clips_old) + (SELECT coalesce(max(id), 0) FROM vods) + p.edit_id,
           p.position, 'piece', p.vod_id, CAST(round(p.start * FLICKS) AS INTEGER), CAST(round(p.end * FLICKS) AS INTEGER),
           p.struck, p.title
    FROM edit_pieces p;
INSERT INTO items (clip_id, position, kind, vod_id, start, end, title)
    SELECT id, 0, 'piece', vod_id, CAST(round(start * FLICKS) AS INTEGER), CAST(round(end * FLICKS) AS INTEGER), title
    FROM clips_old;
INSERT INTO items (clip_id, position, kind, vod_id, start, end)
    SELECT id, 0, 'piece', whole_of, 0, NULL FROM clips WHERE whole_of IS NOT NULL;
ALTER TABLE marks RENAME TO marks_old;
CREATE TABLE marks (
    id INTEGER PRIMARY KEY,
    clip_id INTEGER NOT NULL REFERENCES clips (id) ON DELETE CASCADE,
    vod_id INTEGER NOT NULL REFERENCES vods (id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    at INTEGER NOT NULL,
    text TEXT,
    source TEXT NOT NULL DEFAULT 'user',
    created_at INTEGER NOT NULL
);
CREATE INDEX marks_by_clip ON marks (clip_id, at);
INSERT INTO marks (id, clip_id, vod_id, kind, at, text, source, created_at)
    SELECT m.id, c.id, m.vod_id, m.kind, CAST(round(m.start * FLICKS) AS INTEGER), m.text, m.source, m.created_at
    FROM marks_old m JOIN clips c ON c.whole_of = m.vod_id;
DROP TABLE marks_old;
DROP TABLE edit_pieces;
DROP TABLE edits;
DROP TABLE clips_old;
ALTER TABLE vods DROP COLUMN position;
ALTER TABLE vods ADD COLUMN fps_num INTEGER;
ALTER TABLE vods ADD COLUMN fps_den INTEGER;
CREATE VIEW vod_order AS SELECT whole_of AS vod_id, position FROM clips WHERE whole_of IS NOT NULL;";

/// Version 23: what's been rendered of a clip (its status, file, error, the
/// revision it's of, and whether its file was dragged out) is kept apart
/// from the clip: it's what was made from the document, not part of it, and
/// writing it changes no revision.
const MIGRATE_V23: &str = "
CREATE TABLE renders (
    clip_id INTEGER PRIMARY KEY REFERENCES clips (id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'waiting',
    file_path TEXT,
    error TEXT,
    revision INTEGER,
    handed_out INTEGER NOT NULL DEFAULT 0
);
INSERT INTO renders (clip_id, status, file_path, error, revision, handed_out)
    SELECT id, status, file_path, error, rendered_revision, handed_out FROM clips;
ALTER TABLE clips DROP COLUMN status;
ALTER TABLE clips DROP COLUMN file_path;
ALTER TABLE clips DROP COLUMN error;
ALTER TABLE clips DROP COLUMN rendered_revision;
ALTER TABLE clips DROP COLUMN handed_out;";

/// Version 25: a source can be gone (`gone`): its files, transcript and
/// chat erased while clips still have pieces of it, which are holes.
const MIGRATE_V25: &str = "ALTER TABLE vods ADD COLUMN gone INTEGER NOT NULL DEFAULT 0;";

/// Version 26: what a clip's rendered file was made of (`Db::made_of`: its
/// title, what plays and its chapters), so a change that leaves those as
/// they were (a highlight, a cut) doesn't leave it out of date.
const MIGRATE_V26: &str = "ALTER TABLE renders ADD COLUMN made_of TEXT;";

/// Version 27: transcription by stretches (`stretches.rs`). A `done` row is
/// a transcribed range, any other a request, `reached` how far it has
/// written lines; `audio` a section downloaded for it alone. Each VOD's
/// transcription so far becomes stretches: a finished one, done from 0 to
/// the end; one under way, waiting, paused or failed, a request for all of
/// it in that state (queued ones were added when adding meant
/// transcribing); a stopped one, done as far as its lines go, or nothing.
/// The VOD's own status, progress, error and model stay, unread.
const MIGRATE_V27: &str = "
CREATE TABLE stretches (
    id INTEGER PRIMARY KEY,
    vod_id INTEGER NOT NULL REFERENCES vods (id) ON DELETE CASCADE,
    start REAL NOT NULL,
    end REAL,
    status TEXT NOT NULL,
    progress REAL NOT NULL DEFAULT 0,
    reached REAL,
    error TEXT,
    model TEXT,
    note TEXT,
    audio TEXT
);
CREATE INDEX stretches_by_vod ON stretches (vod_id, start);
INSERT INTO stretches (vod_id, start, end, status, progress, model)
    SELECT id, 0, NULL, 'done', 1, model FROM vods WHERE status = 'done' ORDER BY id;
INSERT INTO stretches (vod_id, start, end, status, progress, reached, error, model)
    SELECT id, 0, NULL, status, progress, (SELECT max(end) FROM segments WHERE segments.vod_id = vods.id), error, model
    FROM vods
    WHERE status IN ('queued', 'downloading', 'ready', 'transcribing', 'paused')
       OR (status = 'error' AND error IS NOT 'Stopped')
    ORDER BY id;
INSERT INTO stretches (vod_id, start, end, status, progress, model)
    SELECT id, 0, (SELECT max(end) FROM segments WHERE segments.vod_id = vods.id), 'done', 1, model FROM vods
    WHERE status = 'error' AND error IS 'Stopped' AND EXISTS (SELECT 1 FROM segments WHERE segments.vod_id = vods.id)
    ORDER BY id;";

/// Version 28: search by who and by the rarest word (`search.rs`). Chat's
/// authors indexed, any case, for `from:` and its suggestions; the search
/// indexes' word counts readable as tables, to pick the word a search looks
/// up first. Nothing to fill: the counts are read from the indexes as asked.
const MIGRATE_V28: &str = "
CREATE INDEX chat_by_author ON chat (author COLLATE NOCASE, vod_id);
CREATE VIRTUAL TABLE segments_vocab USING fts5vocab (segments_fts, 'row');
CREATE VIRTUAL TABLE chat_vocab USING fts5vocab (chat_fts, 'row');";

/// Version 29: annotations imported from a file (`annotations.rs`). A
/// highlight's end, its kind (`category`: funny, hype…) and why it's one
/// (`note`), on its mark; who says a line (`speaker`), on the line; a VOD's
/// cast and what it does when (`activity`), each read whole, as JSON in a row
/// of the VOD's own.
const MIGRATE_V29: &str = "
ALTER TABLE marks ADD COLUMN end_at INTEGER;
ALTER TABLE marks ADD COLUMN category TEXT;
ALTER TABLE marks ADD COLUMN note TEXT;
ALTER TABLE segments ADD COLUMN speaker TEXT;
CREATE TABLE vod_annotations (
    vod_id INTEGER PRIMARY KEY REFERENCES vods (id) ON DELETE CASCADE,
    cast_members TEXT NOT NULL,
    activity TEXT NOT NULL,
    produced TEXT,
    imported_at INTEGER NOT NULL
);";

/// Version 30: who is in an imported highlight (`people`: its cast's names,
/// as a JSON array), on its mark.
const MIGRATE_V30: &str = "
ALTER TABLE marks ADD COLUMN people TEXT;";

/// Version 31: what an imported chapter is about, as one emoji (`icon`), on its mark.
const MIGRATE_V31: &str = "
ALTER TABLE marks ADD COLUMN icon TEXT;";

/// Version 32: whether annotations are published for a VOD
/// (`annotations`: none or available; unknown until asked), the published
/// file's version as last seen (its ETag or Last-Modified), and the version
/// the VOD last brought in from there, to tell an update from what it has.
const MIGRATE_V32: &str = "
ALTER TABLE vods ADD COLUMN annotations TEXT;
ALTER TABLE vods ADD COLUMN annotations_version TEXT;
ALTER TABLE vods ADD COLUMN annotations_imported TEXT;";

/// Version 24: each VOD (a source) counts the pieces of clips that use it,
/// its own clip's included, kept by triggers in the transaction of every
/// change to `items` (cascades too). A source at zero is unused: no entry
/// lists it and no clip holds any of it.
const MIGRATE_V24: &str = "
ALTER TABLE vods ADD COLUMN pieces INTEGER NOT NULL DEFAULT 0;
UPDATE vods SET pieces = (SELECT count(*) FROM items WHERE items.vod_id = vods.id);
CREATE TRIGGER items_counted AFTER INSERT ON items WHEN NEW.vod_id IS NOT NULL BEGIN
    UPDATE vods SET pieces = pieces + 1 WHERE id = NEW.vod_id;
END;
CREATE TRIGGER items_uncounted AFTER DELETE ON items WHEN OLD.vod_id IS NOT NULL BEGIN
    UPDATE vods SET pieces = pieces - 1 WHERE id = OLD.vod_id;
END;
CREATE TRIGGER items_recounted AFTER UPDATE OF vod_id ON items WHEN OLD.vod_id IS NOT NEW.vod_id BEGIN
    UPDATE vods SET pieces = pieces - 1 WHERE id = OLD.vod_id;
    UPDATE vods SET pieces = pieces + 1 WHERE id = NEW.vod_id;
END;";

/// The columns `vod_from_row` reads, by name, so adding a column can't shift the others.
/// Its stretches come as one JSON array (`stretches::from_json` reads it), for its status.
pub(crate) const VOD_COLUMNS: &str =
    "id, platform, platform_id, url, title, channel, duration, thumbnail, published, added_at,
     note, thumb, source_path, media_path, chat, chat_error, annotations, EXISTS (SELECT 1 FROM segments WHERE segments.vod_id = vods.id) AS has_segments,
     EXISTS (SELECT 1 FROM clips WHERE clips.whole_of = vods.id) AS listed,
     (SELECT json_group_array(json_array(s.id, s.vod_id, s.start, s.end, s.status, s.progress, s.reached, s.error, s.model))
      FROM stretches s WHERE s.vod_id = vods.id) AS stretches";

pub(crate) fn vod_from_row(r: &Row) -> rusqlite::Result<Vod> {
    let duration: Option<f64> = r.get("duration")?;
    let stretches = crate::stretches::from_json(&r.get::<_, String>("stretches")?)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into()))?;
    let shown = crate::stretches::summarize(&stretches, duration, r.get("has_segments")?);
    Ok(Vod {
        id: r.get("id")?,
        info: VodInfo {
            platform: r.get("platform")?,
            platform_id: r.get("platform_id")?,
            url: r.get("url")?,
            title: r.get("title")?,
            channel: r.get("channel")?,
            duration,
            thumbnail: r.get("thumbnail")?,
            published: r.get("published")?,
        },
        status: shown.status,
        progress: shown.progress,
        error: shown.error,
        stretch: shown.stretch,
        transcribed: shown.transcribed,
        waiting: shown.waiting,
        added_at: r.get("added_at")?,
        note: r.get("note")?,
        thumb: r.get("thumb")?,
        source_path: r.get("source_path")?,
        media: r.get::<_, Option<String>>("media_path")?.is_some(),
        audio_only: [r.get::<_, Option<String>>("source_path")?, r.get("media_path")?]
            .into_iter()
            .flatten()
            .next()
            .and_then(|p| crate::local::media_type(Path::new(&p)))
            .is_some_and(|mime| mime.starts_with("audio/")),
        outdated: shown.outdated,
        chat: ChatStatus::parse(r.get::<_, Option<String>>("chat")?.as_deref()),
        chat_error: r.get("chat_error")?,
        listed: r.get("listed")?,
        annotations: Availability::parse(r.get::<_, Option<String>>("annotations")?.as_deref()),
    })
}

/// Where a clip's file is.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClipStatus {
    /// To be made, once the VOD's video can be reached.
    Waiting,
    Cutting,
    Ready,
    Error,
}

impl ClipStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ClipStatus::Waiting => "waiting",
            ClipStatus::Cutting => "cutting",
            ClipStatus::Ready => "ready",
            ClipStatus::Error => "error",
        }
    }

    pub(crate) fn parse(s: &str) -> Self {
        match s {
            "waiting" => ClipStatus::Waiting,
            "cutting" => ClipStatus::Cutting,
            "ready" => ClipStatus::Ready,
            _ => ClipStatus::Error,
        }
    }
}

/// A range of a VOD, kept as a video file of its own (`crate::clips`).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Clip {
    pub id: i64,
    pub vod_id: i64,
    pub start: f64,
    pub end: f64,
    pub title: String,
    pub status: ClipStatus,
    pub file_path: Option<String>,
    pub error: Option<String>,
    /// In the clip bar: saved there, rather than only dragged out of the window.
    pub shown: bool,
    /// The revision it's at (`Db::change`).
    pub revision: i64,
    /// The clip bar it's on: a VOD's, or an edit's.
    pub bar: Option<ClipBar>,
    /// How long it plays: on an edit's clip bar, where it may hold several
    /// pieces (`start`..`end` are its first one's); else `end - start`.
    pub length: f64,
}

/// Whose clip bar a clip is on.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ClipBar {
    Vod { id: i64 },
    Edit { id: i64 },
}

/// A mark on a VOD's timeline: a cut (`kind` "cut", between two frames), a
/// chapter's start ("chapter": it runs to the next one) or a highlight
/// ("highlight", a moment), with its title or note.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Mark {
    pub id: i64,
    pub vod_id: i64,
    pub kind: String,
    pub start: f64,
    pub text: Option<String>,
    #[serde(flatten)]
    pub detail: MarkDetail,
}

/// What a mark brings besides its kind, moment and text: an imported
/// highlight's end, kind, why and cast, an imported chapter's icon
/// (`annotations.rs`), and where it came from.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct MarkDetail {
    /// A highlight's end, in seconds of the VOD, when it has one.
    pub end: Option<f64>,
    /// A highlight's kind: funny, hype, drama…
    pub category: Option<String>,
    /// Why it's a highlight.
    pub note: Option<String>,
    /// Who is in a highlight: cast members' names (or aliases), as the import gave them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cast: Vec<String>,
    /// What a chapter is about, as one emoji.
    pub icon: Option<String>,
    /// "user" for a mark made here; "annotations" for one an import brought,
    /// which the next import replaces.
    pub source: String,
}

impl Default for MarkDetail {
    fn default() -> Self {
        Self { end: None, category: None, note: None, cast: Vec::new(), icon: None, source: USER_MARK.into() }
    }
}

/// The `source` of a mark made in the app.
pub const USER_MARK: &str = "user";

/// A line of a VOD's transcript as the transcript view shows it: no word timings.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TranscriptLine {
    pub start: f64,
    pub end: f64,
    pub text: String,
    /// Who says it, when an import named them (a cast member's name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// Where a mark is: on a VOD's own timeline, or in an edit (its clip).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MarkDoc {
    Vod(i64),
    Edit(i64),
}

/// A mark as `Db::mark_states` takes it and `Db::restore_marks` puts it back:
/// what it is, not which row it was.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkState {
    pub kind: String,
    pub start: f64,
    pub text: Option<String>,
    pub detail: MarkDetail,
}

/// A snippet's columns: the clip's, and its first piece's stretch.
pub(crate) const CLIP_COLUMNS: &str =
    "c.id, i.vod_id, i.start, i.end, c.title, r.status, r.file_path, r.error, c.shown, c.revision,
     c.bar_of, (SELECT whole_of FROM clips b WHERE b.id = c.bar_of)";
/// Joins a clip (`c`) to its first piece (`i`) and its render (`r`).
pub(crate) const CLIP_JOIN: &str = "JOIN items i ON i.id = (SELECT id FROM items WHERE clip_id = c.id AND parent IS NULL ORDER BY position, id LIMIT 1)
     JOIN renders r ON r.clip_id = c.id";

pub(crate) fn clip_from_row(r: &Row) -> rusqlite::Result<Clip> {
    let mut clip = Clip {
        id: r.get(0)?,
        vod_id: r.get(1)?,
        start: Flicks(r.get(2)?).seconds(),
        end: Flicks(r.get::<_, Option<i64>>(3)?.unwrap_or(0)).seconds(),
        title: r.get(4)?,
        status: ClipStatus::parse(&r.get::<_, String>(5)?),
        file_path: r.get(6)?,
        error: r.get(7)?,
        shown: r.get(8)?,
        revision: r.get(9)?,
        bar: match (r.get::<_, Option<i64>>(10)?, r.get::<_, Option<i64>>(11)?) {
            (_, Some(vod)) => Some(ClipBar::Vod { id: vod }),
            (Some(edit), None) => Some(ClipBar::Edit { id: edit }),
            _ => None,
        },
        length: 0.0,
    };
    clip.length = clip.end - clip.start;
    Ok(clip)
}

/// How many copies of the library from before upgrades are kept.
const KEPT_COPIES: usize = 2;

/// How long a copy from before an upgrade is kept: time to find an upgrade
/// gone wrong, but not for good, as a copy keeps what's been deleted since
/// (the privacy page says so).
const COPIES_KEPT_FOR: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);

/// Copies the library at `path` (format `version`, open on `conn`) to
/// `library-v<version>.sqlite` beside it before it's upgraded, whole and as
/// it stands (VACUUM INTO), and removes all but the newest copies.
fn keep_before_upgrade(conn: &Connection, path: &Path, version: i32) -> Result<()> {
    let Some(dir) = path.parent().filter(|_| path.is_file()) else { return Ok(()) };
    let copy = dir.join(format!("library-v{version}.sqlite"));
    let _ = std::fs::remove_file(&copy);
    let made = conn.execute("VACUUM INTO ?1", [copy.to_string_lossy()]).and_then(|_| {
        // VACUUM INTO doesn't carry the format's number over.
        Connection::open(&copy)?.execute_batch(&format!("PRAGMA user_version = {version};"))
    });
    made.with_context(|| format!("keep a copy of the library at {} before upgrading it", crate::tools::shown(&copy)))?;
    // The newest kept, by when they were made (the one just made among them).
    let mut copies: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_prefix("library-v")?.strip_suffix(".sqlite")?.parse::<i32>().ok()?;
            Some((e.metadata().ok()?.modified().ok()?, e.path()))
        })
        .collect();
    copies.sort_by_key(|(made, _)| std::cmp::Reverse(*made));
    for (_, old) in copies.into_iter().skip(KEPT_COPIES) {
        let _ = std::fs::remove_file(old);
    }
    Ok(())
}

/// Removes the copies from before upgrades (`keep_before_upgrade`) beside
/// the library at `path` that were made more than `max_age` ago.
fn tidy_copies(path: &Path, max_age: std::time::Duration) {
    let Some(entries) = path.parent().and_then(|dir| std::fs::read_dir(dir).ok()) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let copy =
            name.strip_prefix("library-v").and_then(|n| n.strip_suffix(".sqlite")).and_then(|v| v.parse::<i32>().ok());
        let old =
            entry.metadata().and_then(|m| m.modified()).is_ok_and(|made| made.elapsed().is_ok_and(|age| age > max_age));
        if copy.is_some() && old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Moves the library at `path` out of the way (with its WAL and shared
/// memory files), to `library-broken-<date>.sqlite` beside it (`-2`… if
/// taken), so a new one is made in its place: never deleted, for recovering
/// or sending with a bug report. Returns where it went.
pub fn set_aside(path: &Path) -> Result<PathBuf> {
    let dir = path.parent().context("the library has no folder")?;
    let date = date_of(now());
    let mut n = 1;
    let aside = loop {
        let name =
            if n == 1 { format!("library-broken-{date}.sqlite") } else { format!("library-broken-{date}-{n}.sqlite") };
        let aside = dir.join(name);
        if !aside.exists() {
            break aside;
        }
        n += 1;
    };
    std::fs::rename(path, &aside).with_context(|| format!("move the library to {}", crate::tools::shown(&aside)))?;
    for suffix in ["-wal", "-shm"] {
        let side = PathBuf::from(format!("{}{suffix}", path.display()));
        if side.exists() {
            let _ = std::fs::rename(&side, format!("{}{suffix}", aside.display()));
        }
    }
    Ok(aside)
}

/// `secs` since 1970 as a UTC date, YYYY-MM-DD (Howard Hinnant's civil_from_days).
fn date_of(secs: i64) -> String {
    let z = secs.div_euclid(86400) + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// v22: the struck stretches, kept as JSON pairs of seconds till then, as
/// pairs of flicks.
fn struck_to_flicks(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("SELECT id, struck FROM items WHERE struck != '[]'")?;
    let rows: Vec<(i64, String)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    for (id, struck) in rows {
        let seconds: Vec<(f64, f64)> = serde_json::from_str(&struck).unwrap_or_default();
        let flicks = crate::edits::struck_json(&crate::edits::struck_flicks(&seconds))?;
        conn.execute("UPDATE items SET struck = ?2 WHERE id = ?1", params![id, flicks])?;
    }
    Ok(())
}

pub(crate) fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_inner(path).with_context(|| format!("open the library at {}", crate::tools::shown(path)))
    }

    /// A second connection to the library at `path`, for reading only, opened
    /// after `open` (which makes and upgrades it). The library is in WAL mode,
    /// so reads here never wait for writes on the other connection (chat being
    /// saved, a VOD being deleted), and the other way around.
    pub fn open_reader(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("open the library at {} for reading", crate::tools::shown(path)))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Self { conn })
    }

    /// Takes the library's write lock and keeps it: nothing more is written,
    /// on this connection or another, till the process ends. For quitting,
    /// so jobs whose processes are stopped then don't record a failure.
    pub fn stop_writes(&self) -> Result<()> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        Ok(())
    }

    fn open_inner(path: &Path) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // Transactions take the write lock when they begin, waiting (as above)
        // for another connection's to end. Begun deferred, one that reads and
        // then writes fails "database is locked" at once if another connection
        // (the worker's) has written since its read.
        conn.set_transaction_behavior(rusqlite::TransactionBehavior::Immediate);
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
            MIGRATE_V14,
            MIGRATE_V15,
            MIGRATE_V16,
            MIGRATE_V17,
            MIGRATE_V18,
            MIGRATE_V19,
            MIGRATE_V20,
            MIGRATE_V21,
            MIGRATE_V22,
            MIGRATE_V23,
            MIGRATE_V24,
            MIGRATE_V25,
            MIGRATE_V26,
            MIGRATE_V27,
            MIGRATE_V28,
            MIGRATE_V29,
            MIGRATE_V30,
            MIGRATE_V31,
            MIGRATE_V32,
        ];
        if (version as usize) < migrations.len() {
            // A copy as it was first, so an upgrade that turns out wrong can be
            // undone; no copy, no upgrade.
            if version > 0 {
                keep_before_upgrade(&conn, path, version)?;
            }
            let pending = migrations[version as usize..].join("\n").replace("FLICKS", FLICKS);
            let latest = migrations.len();
            conn.execute_batch(&format!("BEGIN; {pending}"))?;
            if version < 22 {
                struck_to_flicks(&conn)?;
            }
            conn.execute_batch(&format!("PRAGMA user_version = {latest}; COMMIT;"))?;
        } else if version as usize > migrations.len() {
            // A newer VODscribe made this library (the user went back a version).
            // Migrations so far only add, so carry on rather than refuse to start.
            eprintln!("{}: library format {version} is newer than this version knows", path.display());
        }
        tidy_copies(path, COPIES_KEPT_FOR);
        Ok(Self { conn })
    }

    /// Inserts a VOD, or returns the existing row if it was already added.
    pub fn add_vod(&self, info: &VodInfo) -> Result<(Vod, bool)> {
        if let Some(existing) = self.find_vod(&info.platform, &info.platform_id)? {
            // Taken out of the list but kept for other clips: listed again.
            if !existing.listed {
                self.atomically(|db| db.add_vod_clip(existing.id, &existing.info.title))?;
                return Ok((self.vod(existing.id)?.expect("just found"), false));
            }
            return Ok((existing, false));
        }
        // New VODs go to the top of the library: their own clip does (`add_vod_clip`).
        let id = self.atomically(|db| {
            db.conn.execute(
                "INSERT INTO vods (id, platform, platform_id, url, title, channel, duration, thumbnail, published, status, added_at)
                 VALUES ((SELECT max(next, (SELECT coalesce(max(id), 0) + 1 FROM vods)) FROM vod_ids),
                         ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'queued', ?9)",
                params![
                    info.platform, info.platform_id, info.url, info.title, info.channel,
                    info.duration, info.thumbnail, info.published, now()
                ],
            )?;
            let id = db.conn.last_insert_rowid();
            db.add_vod_clip(id, &info.title)?;
            Ok(id)
        })?;
        Ok((self.vod(id)?.expect("just inserted"), true))
    }

    fn find_vod(&self, platform: &str, platform_id: &str) -> Result<Option<Vod>> {
        let sql = format!("SELECT {VOD_COLUMNS} FROM vods WHERE platform = ?1 AND platform_id = ?2");
        Ok(self.conn.query_row(&sql, params![platform, platform_id], vod_from_row).optional()?)
    }

    pub fn vod(&self, id: i64) -> Result<Option<Vod>> {
        let sql = format!("SELECT {VOD_COLUMNS} FROM vods WHERE id = ?1");
        Ok(self.conn.query_row(&sql, [id], vod_from_row).optional()?)
    }

    /// Every VOD, as a source: those in the library's list, in its order,
    /// then those only other clips use.
    pub fn vods(&self) -> Result<Vec<Vod>> {
        let sql = format!(
            "SELECT {VOD_COLUMNS} FROM vods LEFT JOIN vod_order o ON o.vod_id = vods.id
             ORDER BY o.position IS NULL, o.position, id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        Ok(stmt.query_map([], vod_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// The VODs in the library's list, in its order.
    pub fn listed_vods(&self) -> Result<Vec<Vod>> {
        let sql = format!(
            "SELECT {VOD_COLUMNS} FROM vods JOIN vod_order o ON o.vod_id = vods.id ORDER BY o.position, id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        Ok(stmt.query_map([], vod_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// Adds a clip of VOD `vod_id` from `start` to `end` seconds, after its
    /// others on the clip bar; `shown` if it goes in the bar. A snippet: a
    /// clip of one piece, hanging off the VOD's own clip.
    pub fn add_clip(&self, vod_id: i64, start: f64, end: f64, title: &str, shown: bool) -> Result<Clip> {
        let bar = self.vod_clip(vod_id)?.context("That VOD is gone.")?;
        let add = |db: &Db| {
            let id = db.new_clip(title, crate::edits::Place::Bar(bar))?;
            db.conn.execute("UPDATE clips SET shown = ?2 WHERE id = ?1", params![id, shown])?;
            let range = (Flicks::from_seconds(start), Flicks::from_seconds(end));
            db.insert_piece(id, vod_id, range, "", &[])?;
            db.copy_vod_marks(id, vod_id, range.0, range.1)?;
            Ok(id)
        };
        // One only dragged out isn't in the bar: no change to the VOD's clip.
        let id = if shown { self.change(bar, None, add)? } else { self.atomically(add)? };
        Ok(self.clip(id)?.expect("just inserted"))
    }

    pub fn clip(&self, id: i64) -> Result<Option<Clip>> {
        let sql = format!("SELECT {CLIP_COLUMNS} FROM clips c {CLIP_JOIN} WHERE c.id = ?1");
        self.conn.query_row(&sql, [id], clip_from_row).optional()?.map(|c| self.measured(c)).transpose()
    }

    /// `clip` with its length: its pieces' played stretches, for one on an
    /// edit's clip bar.
    fn measured(&self, mut clip: Clip) -> Result<Clip> {
        clip.length = match clip.bar {
            Some(ClipBar::Edit { .. }) => {
                self.edit_pieces(clip.id)?.iter().flat_map(|p| p.spans()).map(|(a, b)| b - a).sum()
            }
            _ => clip.end - clip.start,
        };
        Ok(clip)
    }

    /// VOD `vod_id`'s clips (the snippets on its clip bar), in their order.
    pub fn clips(&self, vod_id: i64) -> Result<Vec<Clip>> {
        let sql = format!(
            "SELECT {CLIP_COLUMNS} FROM clips c {CLIP_JOIN}
             WHERE c.bar_of = (SELECT id FROM clips WHERE whole_of = ?1) ORDER BY c.bar_position, c.id"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        Ok(stmt.query_map([vod_id], clip_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// The clips on listed clip `id`'s clip bar (an edit's), in their order.
    pub fn bar_clips(&self, id: i64) -> Result<Vec<Clip>> {
        let sql =
            format!("SELECT {CLIP_COLUMNS} FROM clips c {CLIP_JOIN} WHERE c.bar_of = ?1 ORDER BY c.bar_position, c.id");
        let mut stmt = self.conn.prepare(&sql)?;
        let clips: Vec<Clip> = stmt.query_map([id], clip_from_row)?.collect::<rusqlite::Result<_>>()?;
        clips.into_iter().map(|c| self.measured(c)).collect()
    }

    /// Clips whose file is still to be made (or was being made when the app stopped).
    pub fn unfinished_clips(&self) -> Result<Vec<Clip>> {
        let sql = format!(
            "SELECT {CLIP_COLUMNS} FROM clips c {CLIP_JOIN}
             WHERE c.bar_of IS NOT NULL AND r.status IN ('waiting', 'cutting') ORDER BY c.id"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        Ok(stmt.query_map([], clip_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// Moves clip `id` to `start`..`end` seconds; its file is to be made again.
    pub fn set_clip_range(&self, id: i64, start: f64, end: f64) -> Result<()> {
        self.change(id, None, |db| {
            db.conn.execute(
                "UPDATE items SET start = ?2, end = ?3 WHERE id = (SELECT id FROM items WHERE clip_id = ?1 AND parent IS NULL ORDER BY position, id LIMIT 1)",
                params![id, Flicks::from_seconds(start).0, Flicks::from_seconds(end).0],
            )?;
            db.conn.execute("UPDATE renders SET status = 'waiting', error = NULL WHERE clip_id = ?1", [id])?;
            Ok(())
        })
    }

    /// Puts clip `id` in the clip bar.
    pub fn show_clip(&self, id: i64) -> Result<()> {
        let bar: Option<i64> =
            self.conn.query_row("SELECT bar_of FROM clips WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten();
        let Some(bar) = bar else { return Ok(()) };
        self.change(bar, None, |db| {
            db.conn.execute("UPDATE clips SET shown = 1 WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// Notes that clip `id`'s file was dragged out of the app: it's someone
    /// else's now, so it stays when the clip goes.
    pub fn hand_out_clip(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE renders SET handed_out = 1 WHERE clip_id = ?1", [id])?;
        Ok(())
    }

    /// Clip `id`'s file is a new one, the app's own (the one handed out stays where it went).
    pub fn take_back_clip(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE renders SET handed_out = 0 WHERE clip_id = ?1", [id])?;
        Ok(())
    }

    /// Whether clip `id`'s file was dragged out of the app (`hand_out_clip`).
    pub fn clip_handed_out(&self, id: i64) -> Result<bool> {
        let out =
            self.conn.query_row("SELECT handed_out FROM renders WHERE clip_id = ?1", [id], |r| r.get(0)).optional()?;
        Ok(out.unwrap_or(false))
    }

    /// Forgets the clips not in the clip bar (made only to be dragged out),
    /// leaving their files; gives their ids.
    pub fn forget_hidden_clips(&self) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare("DELETE FROM clips WHERE bar_of IS NOT NULL AND shown = 0 RETURNING id")?;
        let ids = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }

    pub fn rename_clip(&self, id: i64, title: &str) -> Result<()> {
        self.change(id, None, |db| {
            db.conn.execute("UPDATE clips SET title = ?2 WHERE id = ?1", params![id, title])?;
            Ok(())
        })
    }

    /// Records where clip `id`'s file is, unless its range has moved since
    /// `start`..`end` seconds was cut (it's being made again). True if recorded.
    pub fn set_clip_file(
        &self,
        id: i64,
        start: f64,
        end: f64,
        status: ClipStatus,
        file: Option<&str>,
        error: Option<&str>,
    ) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE renders SET status = ?4, file_path = coalesce(?5, file_path), error = ?6
             WHERE clip_id = ?1 AND EXISTS (
                 SELECT 1 FROM items WHERE clip_id = ?1 AND parent IS NULL AND start = ?2 AND end = ?3
                 ORDER BY position, id LIMIT 1)",
            params![id, Flicks::from_seconds(start).0, Flicks::from_seconds(end).0, status.as_str(), file, error],
        )?;
        Ok(n > 0)
    }

    /// Deletes snippet `id`: a change to the clip whose bar it's in, if it's
    /// shown there.
    pub fn delete_clip(&self, id: i64) -> Result<()> {
        let delete = |db: &Db| {
            db.conn.execute("DELETE FROM clips WHERE id = ?1", [id])?;
            Ok(())
        };
        let bar: Option<i64> = self
            .conn
            .query_row("SELECT CASE WHEN shown THEN bar_of END FROM clips WHERE id = ?1", [id], |r| r.get(0))
            .optional()?
            .flatten();
        match bar {
            Some(bar) => self.change(bar, None, delete),
            None => self.atomically(delete),
        }
    }

    /// VOD `vod_id`'s marks (its own clip's), in time order.
    pub fn marks(&self, vod_id: i64) -> Result<Vec<Mark>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, vod_id, kind, at, text, end_at, category, note, source, people, icon FROM marks
             WHERE clip_id = (SELECT id FROM clips WHERE whole_of = ?1)
             ORDER BY at, id",
        )?;
        let rows = stmt.query_map([vod_id], |r| {
            Ok(Mark {
                id: r.get(0)?,
                vod_id: r.get(1)?,
                kind: r.get(2)?,
                start: Flicks(r.get(3)?).seconds(),
                text: r.get(4)?,
                detail: MarkDetail {
                    end: r.get::<_, Option<i64>>(5)?.map(|f| Flicks(f).seconds()),
                    category: r.get(6)?,
                    note: r.get(7)?,
                    source: r.get(8)?,
                    // A list the app wrote; one it can't read is no one.
                    cast: r
                        .get::<_, Option<String>>(9)?
                        .and_then(|j| serde_json::from_str(&j).ok())
                        .unwrap_or_default(),
                    icon: r.get(10)?,
                },
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Puts a mark on VOD `vod_id`'s own clip at `start` seconds of the VOD.
    pub fn add_mark(&self, vod_id: i64, kind: &str, start: f64, text: Option<&str>) -> Result<Mark> {
        let clip = self.vod_clip(vod_id)?.context("That VOD is gone.")?;
        let at = Flicks::from_seconds(start);
        let id = self.change(clip, None, |db| db.insert_mark(clip, vod_id, kind, at, text))?;
        Ok(Mark {
            id,
            vod_id,
            kind: kind.into(),
            start: at.seconds(),
            text: text.map(Into::into),
            detail: MarkDetail::default(),
        })
    }

    /// Inserts a mark of VOD `vod_id` on clip `clip` at `at`; its id.
    pub(crate) fn insert_mark(
        &self,
        clip: i64,
        vod_id: i64,
        kind: &str,
        at: Flicks,
        text: Option<&str>,
    ) -> Result<i64> {
        self.insert_mark_with(clip, vod_id, kind, at, text, &MarkDetail::default())
    }

    /// Inserts a mark as `insert_mark` does, with `detail`; its id.
    pub(crate) fn insert_mark_with(
        &self,
        clip: i64,
        vod_id: i64,
        kind: &str,
        at: Flicks,
        text: Option<&str>,
        detail: &MarkDetail,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO marks (clip_id, vod_id, kind, at, text, end_at, category, note, source, people, icon, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                clip,
                vod_id,
                kind,
                at.0,
                text,
                detail.end.map(|e| Flicks::from_seconds(e).0),
                detail.category,
                detail.note,
                detail.source,
                (!detail.cast.is_empty()).then(|| serde_json::to_string(&detail.cast)).transpose()?,
                detail.icon,
                now()
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Runs `f` on mark `id` as a change to the clip it's on; false, and
    /// nothing done, if it's gone.
    fn change_mark(&self, id: i64, f: impl FnOnce(&Db) -> Result<()>) -> Result<bool> {
        let clip = self.conn.query_row("SELECT clip_id FROM marks WHERE id = ?1", [id], |r| r.get(0)).optional()?;
        let Some(clip) = clip else { return Ok(false) };
        self.change(clip, None, f)?;
        Ok(true)
    }

    /// Sets mark `id`'s title or note (none, if None); false if it's gone.
    pub fn set_mark_text(&self, id: i64, text: Option<&str>) -> Result<bool> {
        self.change_mark(id, |db| {
            // An imported mark changed here is the user's: a later import leaves it.
            db.conn.execute("UPDATE marks SET text = ?2, source = ?3 WHERE id = ?1", params![id, text, USER_MARK])?;
            Ok(())
        })
    }

    /// Moves mark `id` to `start` seconds; false if it's gone.
    pub fn move_mark(&self, id: i64, start: f64) -> Result<bool> {
        self.change_mark(id, |db| {
            let at = Flicks::from_seconds(start).0;
            db.conn.execute("UPDATE marks SET at = ?2, source = ?3 WHERE id = ?1", params![id, at, USER_MARK])?;
            Ok(())
        })
    }

    /// The document mark `id` is in, if it's there: its VOD's (the VOD's own
    /// clip) or an edit's.
    pub fn mark_doc(&self, id: i64) -> Result<Option<MarkDoc>> {
        Ok(self
            .conn
            .query_row(
                "SELECT m.clip_id, c.whole_of FROM marks m JOIN clips c ON c.id = m.clip_id WHERE m.id = ?1",
                [id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?)),
            )
            .optional()?
            .map(|(clip, vod)| match vod {
                Some(vod) => MarkDoc::Vod(vod),
                None => MarkDoc::Edit(clip),
            }))
    }

    /// The VOD mark `id` is on, if it's there.
    pub fn mark_vod(&self, id: i64) -> Result<Option<i64>> {
        Ok(self.conn.query_row("SELECT vod_id FROM marks WHERE id = ?1", [id], |r| r.get(0)).optional()?)
    }

    /// VOD `vod_id`'s marks, to be put back later with `restore_marks` (for undo).
    pub fn mark_states(&self, vod_id: i64) -> Result<Vec<MarkState>> {
        Ok(self
            .marks(vod_id)?
            .into_iter()
            .map(|m| MarkState { kind: m.kind, start: m.start, text: m.text, detail: m.detail })
            .collect())
    }

    /// Makes VOD `vod_id`'s marks those of `marks` (made anew, so with new ids).
    pub fn restore_marks(&self, vod_id: i64, marks: &[MarkState]) -> Result<()> {
        let clip = self.vod_clip(vod_id)?.context("That VOD is gone.")?;
        self.change(clip, None, |db| {
            db.conn.execute("DELETE FROM marks WHERE clip_id = ?1", [clip])?;
            for m in marks {
                db.insert_mark_with(
                    clip,
                    vod_id,
                    &m.kind,
                    Flicks::from_seconds(m.start),
                    m.text.as_deref(),
                    &m.detail,
                )?;
            }
            Ok(())
        })
    }

    pub fn delete_mark(&self, id: i64) -> Result<()> {
        self.change_mark(id, |db| {
            db.conn.execute("DELETE FROM marks WHERE id = ?1", [id])?;
            Ok(())
        })?;
        Ok(())
    }

    /// Runs `f`'s writes as one transaction: all of them happen, or none do.
    /// Inside another, it's part of that one.
    pub fn atomically<T>(&self, f: impl FnOnce(&Db) -> Result<T>) -> Result<T> {
        if !self.conn.is_autocommit() {
            return f(self);
        }
        let tx = self.conn.unchecked_transaction()?;
        let out = f(self)?;
        tx.commit()?;
        Ok(out)
    }

    /// Puts the library in the order of `ids`, which should list every VOD.
    pub fn reorder(&mut self, ids: &[i64]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare("UPDATE clips SET position = ?2 WHERE whole_of = ?1")?;
            for (i, id) in ids.iter().enumerate() {
                stmt.execute(params![id, i as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// VOD `id`'s frame rate, once its video has been read (`set_rate`).
    pub fn rate(&self, id: i64) -> Result<Option<Rate>> {
        let row = self
            .conn
            .query_row("SELECT fps_num, fps_den FROM vods WHERE id = ?1", [id], |r| {
                Ok((r.get::<_, Option<u32>>(0)?, r.get::<_, Option<u32>>(1)?))
            })
            .optional()?;
        Ok(row.and_then(|(num, den)| Some(Rate::new(num?, den?))).filter(|r| r.num > 0 && r.den > 0))
    }

    /// Records VOD `id`'s frame rate, as read from its video (None: sound alone).
    pub fn set_rate(&self, id: i64, rate: Option<Rate>) -> Result<()> {
        self.conn.execute(
            "UPDATE vods SET fps_num = ?2, fps_den = ?3 WHERE id = ?1",
            params![id, rate.map(|r| r.num), rate.map(|r| r.den)],
        )?;
        Ok(())
    }

    /// The file VOD `id` plays from here, if any: its file on this computer,
    /// or else the video kept for it.
    pub fn playable_path(&self, id: i64) -> Result<Option<String>> {
        let sql = "SELECT coalesce(source_path, media_path) FROM vods WHERE id = ?1";
        Ok(self.conn.query_row(sql, [id], |r| r.get(0)).optional()?.flatten())
    }

    /// VOD `id`'s kept video (see `Vod::media`).
    pub fn media_path(&self, id: i64) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT media_path FROM vods WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten())
    }

    pub fn set_media_path(&self, id: i64, path: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE vods SET media_path = ?2 WHERE id = ?1", params![id, path])?;
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
    /// busy chat has a hundred thousand rows and more: about half a second to
    /// erase (a few ms a thousand), so they go a part at a time, letting
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
    /// none are left (a part at a time: see `delete_some_of`, `LibraryChat::clear`).
    pub fn clear_some_chat(&self, vod_id: i64, limit: usize) -> Result<usize> {
        self.clear_some("chat", vod_id, limit)
    }

    /// Deletes up to `limit` of VOD `vod_id`'s transcript segments; how many, 0
    /// when none are left (a part at a time: see `delete_some_of`).
    pub fn clear_some_segments(&self, vod_id: i64, limit: usize) -> Result<usize> {
        self.clear_some("segments", vod_id, limit)
    }

    /// Deletes up to `limit` of VOD `vod_id`'s rows in `table` (one of the two above).
    fn clear_some(&self, table: &str, vod_id: i64, limit: usize) -> Result<usize> {
        let sql = format!("DELETE FROM {table} WHERE id IN (SELECT id FROM {table} WHERE vod_id = ?1 LIMIT ?2)");
        Ok(self.conn.execute(&sql, params![vod_id, limit as i64])?)
    }

    /// Rewrites search index `index` ("chat_fts" or "segments_fts") without
    /// the entries of rows deleted since, which otherwise stay in the file
    /// (secure_delete then zeroes the pages they were on). After a VOD's rows
    /// are deleted; a no-op when there's nothing to tidy.
    pub(crate) fn tidy_index(&self, index: &str) -> Result<()> {
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

    /// VOD `vod_id`'s lines as (start, end, text), without the per-word
    /// timings that `segments` also parses: what the transcript view needs.
    pub fn lines(&self, vod_id: i64) -> Result<Vec<TranscriptLine>> {
        self.lines_between(vod_id, f64::MIN, None)
    }

    /// VOD `vod_id`'s lines starting from `from` to before `to` (None: on to
    /// the end), as `lines` gives them: the ones a stretch being transcribed
    /// replaces (`replace_segments`), so the view can replace them in turn.
    pub fn lines_between(&self, vod_id: i64, from: f64, to: Option<f64>) -> Result<Vec<TranscriptLine>> {
        let mut stmt = self.conn.prepare(
            "SELECT start, end, text, speaker FROM segments
             WHERE vod_id = ?1 AND start >= ?2 AND (?3 IS NULL OR start < ?3)
             ORDER BY start",
        )?;
        let rows = stmt.query_map(params![vod_id, from, to], |r| {
            Ok(TranscriptLine { start: r.get(0)?, end: r.get(1)?, text: r.get(2)?, speaker: r.get(3)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The words VOD `vod_id`'s transcript has from `start` to `end`.
    pub fn words_between(&self, vod_id: i64, start: f64, end: f64) -> Result<Vec<Word>> {
        let mut stmt = self
            .conn
            .prepare("SELECT words FROM segments WHERE vod_id = ?1 AND end > ?2 AND start < ?3 ORDER BY start")?;
        let mut out = Vec::new();
        for words in stmt.query_map(params![vod_id, start, end], |r| r.get::<_, String>(0))? {
            let words: Vec<Word> = serde_json::from_str(&words?)?;
            out.extend(words.into_iter().filter(|w| w.end > start && w.start < end));
        }
        Ok(out)
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
        self.add_chat(vod_id, messages, &[])?;
        self.finish_chat(vod_id)
    }

    /// Where an unfinished fetch of VOD `vod_id`'s chat got to, slice by slice (see `chat::ChatStore`).
    /// None if a version before 0.3.0 has fetched since (it writes `chat_resume`, which this one
    /// only ever clears): it cleared the chat but not the slices, which no longer describe it.
    pub fn chat_slices(&self, vod_id: i64) -> Result<Option<Vec<f64>>> {
        let row: Option<(Option<String>, Option<f64>)> = self
            .conn
            .query_row("SELECT chat_slices, chat_resume FROM vods WHERE id = ?1", [vod_id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        Ok(match row {
            Some((Some(json), None)) => serde_json::from_str(&json).ok(),
            _ => None,
        })
    }

    /// Forgets VOD `vod_id`'s chat and where a fetch of it got to.
    pub fn clear_chat(&self, vod_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM chat WHERE vod_id = ?1", [vod_id])?;
        self.conn.execute("UPDATE vods SET chat_resume = NULL, chat_slices = NULL WHERE id = ?1", [vod_id])?;
        Ok(())
    }

    /// Forgets VOD `vod_id`'s chat messages from `from` up to `to` (seconds).
    pub fn drop_chat_range(&self, vod_id: i64, from: f64, to: f64) -> Result<()> {
        self.conn
            .execute("DELETE FROM chat WHERE vod_id = ?1 AND time >= ?2 AND time < ?3", params![vod_id, from, to])?;
        Ok(())
    }

    /// Adds `messages` to VOD `vod_id`'s chat and records that its fetch's slices reached `positions`, together.
    pub fn add_chat(&mut self, vod_id: i64, messages: &[ChatMessage], positions: &[f64]) -> Result<()> {
        let tx = self.conn.transaction()?;
        insert_chat(&tx, vod_id, messages)?;
        tx.execute(
            "UPDATE vods SET chat_slices = ?2 WHERE id = ?1",
            params![vod_id, serde_json::to_string(positions)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Records VOD `vod_id`'s chat as fetched in full.
    pub fn finish_chat(&self, vod_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE vods SET chat = 'done', chat_error = NULL, chat_resume = NULL, chat_slices = NULL WHERE id = ?1",
            [vod_id],
        )?;
        Ok(())
    }

    /// VOD `vod_id`'s messages of `kind` (subs, gifts), as (id, message), in time order.
    pub fn chat_of_kind(&self, vod_id: i64, kind: Kind) -> Result<Vec<(i64, ChatMessage)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, time, author, text, amount, event_time FROM chat WHERE vod_id = ?1 AND kind = ?2 ORDER BY time, id",
        )?;
        let rows = stmt.query_map(params![vod_id, kind.as_str()], |r| {
            Ok((
                r.get(0)?,
                ChatMessage {
                    time: r.get(1)?,
                    author: r.get(2)?,
                    color: None,
                    parts: vec![ChatPart::Text { text: r.get(3)? }],
                    kind,
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
            .prepare("SELECT id FROM vods JOIN vod_order o ON o.vod_id = vods.id WHERE chat IS NULL OR chat = 'failed' ORDER BY o.position, id DESC")?;
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
    pub(crate) fn id_range(&self, table: &str, vod_id: Option<i64>) -> Result<(i64, i64)> {
        let Some(vod_id) = vod_id else { return Ok((i64::MIN, i64::MAX)) };
        let sql = format!("SELECT min(id), max(id) FROM {table} WHERE vod_id = ?1");
        let (lo, hi): (Option<i64>, Option<i64>) =
            self.conn.query_row(&sql, [vod_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok((lo.unwrap_or(1), hi.unwrap_or(0)))
    }
}

/// A VOD's chat in the library, as `chat::fetch` saves it. Holds the library's
/// lock only while it reads or writes, not while the fetch waits on the network.
/// Inserts `messages` as VOD `vod_id`'s, on `conn` (in a transaction the caller holds).
pub(crate) fn insert_chat(conn: &Connection, vod_id: i64, messages: &[ChatMessage]) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT INTO chat (vod_id, time, author, color, text, parts, kind, amount, event_time)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    for m in messages {
        let parts = serde_json::to_string(&m.parts)?;
        stmt.execute(params![
            vod_id,
            m.time,
            m.author,
            m.color,
            m.text(),
            parts,
            m.kind.as_str(),
            m.amount,
            m.event_time
        ])?;
    }
    Ok(())
}

pub struct LibraryChat<'a> {
    pub db: &'a std::sync::Mutex<Db>,
    pub vod_id: i64,
    /// Told after each batch is saved.
    pub on_save: Box<dyn FnMut() + 'a>,
}

impl crate::chat::ChatStore for LibraryChat<'_> {
    fn resume(&mut self) -> Result<Option<Vec<f64>>> {
        self.db.lock().unwrap().chat_slices(self.vod_id)
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

    fn drop_range(&mut self, from: f64, to: f64) -> Result<()> {
        self.db.lock().unwrap().drop_chat_range(self.vod_id, from, to)
    }

    fn save(&mut self, messages: &[ChatMessage], positions: &[f64]) -> Result<()> {
        self.db.lock().unwrap().add_chat(self.vod_id, messages, positions)?;
        (self.on_save)();
        Ok(())
    }

    fn of_kind(&mut self, kind: Kind) -> Result<Vec<(i64, ChatMessage)>> {
        self.db.lock().unwrap().chat_of_kind(self.vod_id, kind)
    }

    fn demote(&mut self, ids: &[i64]) -> Result<()> {
        self.db.lock().unwrap().demote_chat(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::{Segment, Segmenter};

    #[test]
    fn marks_are_put_back_as_they_were() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&crate::edits::tests::info("x")).unwrap().0.id;
        let cut = db.add_mark(vod, "cut", 5.0, None).unwrap();
        db.add_mark(vod, "chapter", 1.0, Some("Intro")).unwrap();
        let before = db.mark_states(vod).unwrap();
        assert_eq!(db.mark_vod(cut.id).unwrap(), Some(vod));

        db.move_mark(cut.id, 7.0).unwrap();
        db.add_mark(vod, "highlight", 3.0, Some("note")).unwrap();
        db.restore_marks(vod, &before).unwrap();
        assert_eq!(db.mark_states(vod).unwrap(), before);
    }

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
            let marked: String = crate::search::highlight(&words, &c.ranges)
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
    fn copies_from_before_upgrades_go_after_thirty_days() {
        let dir = crate::TempDir::new("old-copies");
        let path = dir.join("library.sqlite");
        drop(Db::open(&path).unwrap());
        let (old, new, other) =
            (dir.join("library-v3.sqlite"), dir.join("library-v9.sqlite"), dir.join("notes.sqlite"));
        for p in [&old, &new, &other] {
            std::fs::write(p, b"copy").unwrap();
        }
        let long_ago = SystemTime::now() - std::time::Duration::from_secs(31 * 24 * 3600);
        for p in [&old, &other] {
            std::fs::File::options().write(true).open(p).unwrap().set_modified(long_ago).unwrap();
        }
        drop(Db::open(&path).unwrap());
        assert!(!old.exists(), "a copy 31 days old goes");
        assert!(new.exists(), "a new one stays");
        assert!(other.exists(), "only the library's copies");
    }

    #[test]
    fn an_upgrade_keeps_a_copy_from_before_and_only_the_last_two() {
        let dir = crate::TempDir::new("before-upgrade");
        let path = dir.join("library.sqlite");
        let version = |p: &Path| -> i32 {
            Connection::open(p).unwrap().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap()
        };
        // Copies left by earlier upgrades, 3 the older.
        for v in [3, 9] {
            std::fs::write(dir.join(format!("library-v{v}.sqlite")), b"old").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(&format!("{SCHEMA} PRAGMA user_version = 1;")).unwrap();
            conn.execute(
                "INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES ('kick', 'x', 'u', 't', 'done', 0)",
                [],
            )
            .unwrap();
        }
        drop(Db::open(&path).unwrap());
        let copy = dir.join("library-v1.sqlite");
        assert_eq!(version(&copy), 1, "the copy is as it was before");
        let vods: i64 =
            Connection::open(&copy).unwrap().query_row("SELECT count(*) FROM vods", [], |r| r.get(0)).unwrap();
        assert_eq!(vods, 1);
        assert!(version(&path) > 1, "and the library upgraded");
        // The two newest kept: the one just made and 9; 3, the oldest, goes.
        let mut kept: Vec<String> = std::fs::read_dir(&*dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("library-v"))
            .collect();
        kept.sort();
        assert_eq!(kept, ["library-v1.sqlite", "library-v9.sqlite"]);
        // Opened again, nothing to upgrade: no copy made.
        std::fs::remove_file(&copy).unwrap();
        drop(Db::open(&path).unwrap());
        assert!(!copy.exists());
    }

    /// Adding files while the worker writes on its own connection: a
    /// transaction that reads, then writes after another connection's
    /// commit, waits its turn instead of failing "database is locked".
    #[test]
    fn a_transaction_that_reads_then_writes_waits_for_other_writers() {
        let dir = crate::TempDir::new("read-then-write");
        let path = dir.join("library.sqlite");
        let (adder, worker) = (Db::open(&path).unwrap(), Db::open(&path).unwrap());
        let (vod, _) = worker.add_vod(&info("a")).unwrap();
        let stretch = worker.request(vod.id, 0.0, None).unwrap();
        let mut writing = None;
        adder
            .atomically(|db| {
                db.vod(vod.id)?;
                // The worker finishes a VOD between the read and the write.
                writing = Some(std::thread::spawn(move || worker.finish_stretch(stretch.id)));
                std::thread::sleep(std::time::Duration::from_millis(300));
                db.add_vod(&info("b"))
            })
            .unwrap();
        writing.unwrap().join().unwrap().unwrap();
        assert_eq!(adder.vod(vod.id).unwrap().unwrap().status, Status::Done);
    }

    #[test]
    fn a_broken_library_is_set_aside_not_deleted() {
        let dir = crate::TempDir::new("set-aside");
        let path = dir.join("library.sqlite");
        std::fs::write(&path, b"not a database").unwrap();
        assert!(Db::open(&path).is_err());
        std::fs::write(dir.join("library.sqlite-wal"), b"wal").unwrap();
        let aside = set_aside(&path).unwrap();
        let name = aside.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("library-broken-") && name.ends_with(".sqlite"), "{name}");
        assert_eq!(std::fs::read(&aside).unwrap(), b"not a database");
        assert!(dir.join(format!("{name}-wal")).exists(), "its WAL goes with it");
        assert!(!path.exists());
        // A new library opens in its place; a second one set aside the same day gets -2.
        drop(Db::open(&path).unwrap());
        let again = set_aside(&path).unwrap();
        assert!(again.to_string_lossy().ends_with("-2.sqlite"), "{}", again.display());
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(1790632794), "2026-09-28");
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
        db.conn.execute("UPDATE clips SET position = 5", []).unwrap();
        let later = db.add_vod(&info("later")).unwrap().0;
        assert_eq!(db.vods().unwrap()[0].id, later.id, "added on top of migrated VODs");
        db.delete_vod(later.id).unwrap();
        assert_eq!(vod.note, None);
        assert!(!vod.outdated, "no transcript yet");
        assert_eq!((vod.status, vod.transcribed), (Status::Done, Transcribed::All), "done from 0 to the end");
        db.insert_segments(vod.id, &[Segment { start: 0.0, end: 1.0, text: "hi".into(), words: Vec::new() }]).unwrap();
        assert!(db.vod(vod.id).unwrap().unwrap().outdated, "transcribed before models were recorded");
        let done = db.stretches(vod.id).unwrap()[0].id;
        db.set_stretch_model(done, Some(crate::model::CPU_MODEL.id)).unwrap();
        assert!(!db.vod(vod.id).unwrap().unwrap().outdated);
        db.set_stretch_model(done, Some("sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8")).unwrap();
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
    fn pieces_of_clips_become_their_own() {
        let dir = crate::TempDir::new("migrate-v19");
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
                MIGRATE_V12,
                MIGRATE_V13,
                MIGRATE_V14,
                MIGRATE_V15,
                MIGRATE_V16,
                MIGRATE_V17,
                MIGRATE_V18,
                MIGRATE_V19,
            ];
            conn.execute_batch(&format!("{} PRAGMA user_version = 19;", before.join("\n"))).unwrap();
            conn.execute_batch(
                "INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES ('kick', 'a', 'u', 't', 'done', 0);
                 INSERT INTO clips (vod_id, start, end, title, position, created_at) VALUES (1, 10.0, 20.0, 'Lunch', 1, 0);
                 INSERT INTO edits (title, position, created_at) VALUES ('e', 0, 0);
                 INSERT INTO edit_pieces (edit_id, clip_id, position, start, end, struck)
                     VALUES (1, 1, 0, 5.0, 18.0, '[[12.0,13.0]]');",
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let edit = db.edits().unwrap()[0].id;
        let pieces = db.edit_pieces(edit).unwrap();
        assert_eq!(pieces.len(), 1);
        let p = &pieces[0];
        assert_eq!((p.vod_id, p.start, p.end, p.title.as_str()), (1, 10.0, 18.0, "Lunch"), "within the clip it was of");
        assert_eq!(p.struck, [(12.0, 13.0)]);
        // The clip's gone; the piece stays.
        db.delete_clip(1).unwrap();
        assert_eq!(db.edit_pieces(edit).unwrap().len(), 1);
    }

    /// v22: VODs, clips and edits become clips of one table, in their order,
    /// with their pieces and marks in flicks.
    #[test]
    fn vods_clips_and_edits_become_clips() {
        let dir = crate::TempDir::new("migrate-v22");
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
                MIGRATE_V12,
                MIGRATE_V13,
                MIGRATE_V14,
                MIGRATE_V15,
                MIGRATE_V16,
                MIGRATE_V17,
                MIGRATE_V18,
                MIGRATE_V19,
                MIGRATE_V20,
                MIGRATE_V21,
            ];
            conn.execute_batch(&format!("{} PRAGMA user_version = 21;", before.join("\n"))).unwrap();
            conn.execute_batch(
                "INSERT INTO vods (id, platform, platform_id, url, title, duration, status, added_at, position) VALUES
                     (1, 'kick', 'a', 'u', 'First', 600.0, 'done', 0, 2), (2, 'kick', 'b', 'u', 'Second', 300.0, 'done', 0, 0);
                 INSERT INTO clips (id, vod_id, start, end, title, position, created_at, status, file_path, shown, handed_out) VALUES
                     (1, 1, 10.0, 20.5, 'Lunch', 2, 0, 'ready', '/v/lunch.mp4', 1, 1),
                     (2, 1, 30.0, 40.0, 'Dinner', 1, 0, 'waiting', NULL, 1, 0),
                     (3, 2, 1.0, 2.0, 'Dragged', 1, 0, 'ready', '/v/d.mp4', 0, 0);
                 INSERT INTO marks (id, vod_id, kind, start, text, created_at) VALUES
                     (7, 1, 'chapter', 65.25, 'The raid', 0), (8, 2, 'highlight', 3.0, NULL, 0);
                 INSERT INTO edits (id, title, position, created_at, version, status, file_path, rendered_version) VALUES
                     (1, 'Best of', 1, 0, 4, 'ready', '/v/best.mp4', 4);
                 INSERT INTO edit_pieces (id, edit_id, vod_id, position, start, end, title, struck) VALUES
                     (5, 1, 1, 1, 100.0, 130.0, 'b', '[]'), (4, 1, 2, 0, 5.0, 18.0, 'a', '[[12.0,13.5]]');",
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        use crate::edits::LibraryItem::{Edit, Vod};
        let edit = db.edits().unwrap().remove(0);
        assert_eq!(db.library_order().unwrap(), [Vod(2), Edit(edit.id), Vod(1)], "the order kept");
        assert_eq!(
            (edit.title.as_str(), edit.current, edit.file_path.as_deref()),
            ("Best of", true, Some("/v/best.mp4"))
        );
        let pieces = db.edit_pieces(edit.id).unwrap();
        assert_eq!(
            pieces.iter().map(|p| (p.id, p.vod_id, p.start, p.end)).collect::<Vec<_>>(),
            [(4, 2, 5.0, 18.0), (5, 1, 100.0, 130.0)]
        );
        assert_eq!(pieces[0].struck, [(12.0, 13.5)], "in flicks now, the same in seconds");
        assert_eq!(
            db.piece_row(4).unwrap().unwrap().struck,
            [(Flicks::from_seconds(12.0), Flicks::from_seconds(13.5))]
        );
        // The clip bar's clips keep their ids, order and files; the one only dragged out is hidden still.
        let bar: Vec<_> = db.clips(1).unwrap().into_iter().map(|c| (c.id, c.start, c.end, c.title, c.shown)).collect();
        assert_eq!(bar, [(2, 30.0, 40.0, "Dinner".into(), true), (1, 10.0, 20.5, "Lunch".into(), true)]);
        assert!(db.clip_handed_out(1).unwrap() && !db.clip_handed_out(2).unwrap());
        assert_eq!(db.clip(1).unwrap().unwrap().file_path.as_deref(), Some("/v/lunch.mp4"));
        assert_eq!(db.clips(2).unwrap().iter().map(|c| (c.id, c.shown)).collect::<Vec<_>>(), [(3, false)]);
        // Each VOD has its clip, over all of it.
        let own = db.vod_clip(1).unwrap().unwrap();
        assert_eq!(db.edit_pieces(own).unwrap().iter().map(|p| (p.start, p.end)).collect::<Vec<_>>(), [(0.0, 600.0)]);
        assert!(own != edit.id && db.vod_clip(2).unwrap().unwrap() != edit.id);
        // Marks are the VODs' clips' own, where they were.
        let marks: Vec<_> = db.marks(1).unwrap().into_iter().map(|m| (m.id, m.kind, m.start, m.text)).collect();
        assert_eq!(marks, [(7, "chapter".into(), 65.25, Some("The raid".into()))]);
        assert_eq!(db.marks(2).unwrap().iter().map(|m| m.id).collect::<Vec<_>>(), [8]);
        assert_eq!(db.vods().unwrap().iter().map(|v| v.id).collect::<Vec<_>>(), [2, 1], "VODs list in the order too");
        // Each VOD counts what uses it: its clip, its snippets (shown or not) and the edit's piece.
        assert_eq!((db.uses(1).unwrap(), db.uses(2).unwrap()), (4, 3));
        // Nothing of the old tables is left, nor the order on VODs.
        let tables: Vec<String> = {
            let mut stmt =
                db.conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").unwrap();
            stmt.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
        };
        assert!(
            !tables.iter().any(|t| t == "edits" || t == "edit_pieces" || t == "clips_old" || t == "marks_old"),
            "{tables:?}"
        );
        assert!(db.conn.execute("SELECT position FROM vods", []).is_err());
        // Deleting a VOD takes its clip, the bar and its marks.
        db.delete_vod(1).unwrap();
        assert!(
            db.vod_clip(1).unwrap().is_none() && db.clips(1).unwrap().is_empty() && db.marks(1).unwrap().is_empty()
        );
        assert_eq!(db.clip(2).unwrap(), None);
        assert_eq!(db.edit_pieces(edit.id).unwrap().len(), 1, "the edit keeps its piece of the other VOD");
    }

    #[test]
    fn transcriptions_so_far_become_stretches() {
        let dir = crate::TempDir::new("migrate-v27");
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
                MIGRATE_V12,
                MIGRATE_V13,
                MIGRATE_V14,
                MIGRATE_V15,
                MIGRATE_V16,
                MIGRATE_V17,
                MIGRATE_V18,
                MIGRATE_V19,
                MIGRATE_V20,
                MIGRATE_V21,
                MIGRATE_V22,
                MIGRATE_V23,
                MIGRATE_V24,
                MIGRATE_V25,
                MIGRATE_V26,
            ];
            conn.execute_batch(&format!("{} PRAGMA user_version = 26;", before.join("\n").replace("FLICKS", FLICKS)))
                .unwrap();
            conn.execute_batch(&format!(
                "INSERT INTO vods (id, platform, platform_id, url, title, duration, status, progress, error, model, added_at) VALUES
                     (1, 'kick', 'done', 'u', 't', 600, 'done', 1, NULL, '{model}', 0),
                     (2, 'kick', 'paused', 'u', 't', 600, 'paused', 0.2, NULL, '{model}', 0),
                     (3, 'kick', 'queued', 'u', 't', 600, 'queued', 0, NULL, NULL, 0),
                     (4, 'kick', 'stopped', 'u', 't', 600, 'error', 0, 'Stopped', NULL, 0),
                     (5, 'kick', 'failed', 'u', 't', 600, 'error', 0, 'HTTP 404', NULL, 0),
                     (6, 'kick', 'running', 'u', 't', 600, 'transcribing', 0.5, NULL, '{model}', 0),
                     (7, 'kick', 'stopped-lines', 'u', 't', 600, 'error', 0, 'Stopped', '{model}', 0);
                 INSERT INTO segments (vod_id, start, end, text, words) VALUES
                     (1, 0, 5, 'a', '[]'), (2, 100, 120, 'b', '[]'), (6, 280, 300, 'c', '[]'), (7, 40, 50, 'd', '[]');",
                model = crate::model::CPU_MODEL.id
            ))
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let vod = |id| db.vod(id).unwrap().unwrap();
        let stretches = |id| -> Vec<_> {
            db.stretches(id).unwrap().into_iter().map(|s| (s.start, s.end, s.status, s.reached)).collect()
        };
        assert_eq!(stretches(1), [(0.0, None, Status::Done, None)]);
        assert_eq!((vod(1).status, vod(1).transcribed, vod(1).outdated), (Status::Done, Transcribed::All, false));
        assert_eq!(stretches(2), [(0.0, None, Status::Paused, Some(120.0))]);
        assert_eq!((vod(2).status, vod(2).progress, vod(2).transcribed), (Status::Paused, 0.2, Transcribed::Some));
        assert_eq!(stretches(3), [(0.0, None, Status::Queued, None)], "added when adding meant transcribing");
        assert_eq!((vod(3).status, vod(3).transcribed), (Status::Queued, Transcribed::None));
        assert_eq!(stretches(4), [], "stopped, with nothing to keep");
        assert_eq!((vod(4).status, vod(4).error), (Status::None, None));
        assert_eq!((vod(5).status, vod(5).error.as_deref()), (Status::Error, Some("HTTP 404")));
        assert_eq!(stretches(6), [(0.0, None, Status::Transcribing, Some(300.0))]);
        assert_eq!(stretches(7), [(0.0, Some(50.0), Status::Done, None)], "stopped, its lines kept");
        assert_eq!((vod(7).status, vod(7).transcribed), (Status::None, Transcribed::Some));
        assert_eq!(db.stretches(1).unwrap()[0].model.as_deref(), Some(crate::model::CPU_MODEL.id));
    }

    #[test]
    fn search_reads_word_counts_and_chat_authors_after_upgrading() {
        let dir = crate::TempDir::new("migrate-v28");
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
                MIGRATE_V12,
                MIGRATE_V13,
                MIGRATE_V14,
                MIGRATE_V15,
                MIGRATE_V16,
                MIGRATE_V17,
                MIGRATE_V18,
                MIGRATE_V19,
                MIGRATE_V20,
                MIGRATE_V21,
                MIGRATE_V22,
                MIGRATE_V23,
                MIGRATE_V24,
                MIGRATE_V25,
                MIGRATE_V26,
                MIGRATE_V27,
            ];
            conn.execute_batch(&format!("{} PRAGMA user_version = 27;", before.join("\n").replace("FLICKS", FLICKS)))
                .unwrap();
            conn.execute_batch(
                "INSERT INTO vods (id, platform, platform_id, url, title, channel, duration, status, added_at) VALUES
                     (1, 'twitch', '1', 'u', 't', 'Streamer', 600, 'done', 0);
                 INSERT INTO segments (vod_id, start, end, text, words) VALUES
                     (1, 0, 1, 'Clutch play', '[]'), (1, 5, 6, 'what a play', '[]');
                 INSERT INTO chat (vod_id, time, author, text, parts) VALUES
                     (1, 1, 'Bob', 'PLAY', '[]'), (1, 2, 'bobby', 'hi', '[]'), (1, 3, 'alice', 'hi', '[]');",
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let version: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, 32);
        // The counts are the indexes' own, words already there included: nothing to fill.
        let lines = |table: &str, term: &str| -> i64 {
            let sql = format!("SELECT doc FROM {table} WHERE term = ?1");
            db.conn.query_row(&sql, [term], |r| r.get(0)).optional().unwrap().unwrap_or(0)
        };
        assert_eq!((lines("segments_vocab", "play"), lines("segments_vocab", "clutch")), (2, 1));
        assert_eq!((lines("chat_vocab", "hi"), lines("chat_vocab", "play"), lines("chat_vocab", "nope")), (2, 1, 0));
        // `from:` and its suggestions go by the author index.
        let plan = |sql: &str| -> String {
            let mut stmt = db.conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
            let rows = stmt.query_map([], |r| r.get::<_, String>(3)).unwrap();
            rows.map(|r| r.unwrap()).collect::<Vec<_>>().join("; ")
        };
        assert!(plan("SELECT id FROM chat WHERE author COLLATE NOCASE IN ('bob')").contains("chat_by_author"));
        assert!(
            plan("SELECT author FROM chat WHERE author COLLATE NOCASE >= 'b' AND author COLLATE NOCASE < 'c'")
                .contains("chat_by_author")
        );
        let names: Vec<String> = db.author_suggestions("BO", None).unwrap().into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["Bob", "bobby"]);
    }

    #[test]
    fn marks_from_before_highlights_had_a_cast_read_with_none() {
        let dir = crate::TempDir::new("migrate-v29");
        let path = dir.join("library.sqlite");
        let vod = {
            let db = Db::open(&path).unwrap();
            let (vod, _) = db.add_vod(&crate::edits::tests::info("abc")).unwrap();
            db.add_mark(vod.id, "highlight", 5.0, Some("Big laugh")).unwrap();
            vod.id
        };
        // Back to version 29: the column 30 adds, gone.
        Connection::open(&path)
            .unwrap()
            .execute_batch(concat!(
                "ALTER TABLE marks DROP COLUMN icon; ALTER TABLE marks DROP COLUMN people; ",
                "ALTER TABLE vods DROP COLUMN annotations; ALTER TABLE vods DROP COLUMN annotations_version; ",
                "ALTER TABLE vods DROP COLUMN annotations_imported; PRAGMA user_version = 29;"
            ))
            .unwrap();
        let db = Db::open(&path).unwrap();
        let version: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, 32);
        let marks = db.marks(vod).unwrap();
        assert_eq!((marks[0].text.as_deref(), marks[0].detail.cast.len()), (Some("Big laugh"), 0));
        // And a cast, once there is one, is kept and read back.
        let clip = db.vod_clip(vod).unwrap().unwrap();
        let detail = MarkDetail { cast: vec!["Marlowe".into(), "Ada".into()], ..MarkDetail::default() };
        db.insert_mark_with(clip, vod, "highlight", Flicks::from_seconds(9.0), None, &detail).unwrap();
        assert_eq!(db.marks(vod).unwrap()[1].detail.cast, ["Marlowe", "Ada"]);
    }

    #[test]
    fn marks_from_before_chapters_had_an_icon_read_with_none() {
        let dir = crate::TempDir::new("migrate-v30");
        let path = dir.join("library.sqlite");
        let vod = {
            let db = Db::open(&path).unwrap();
            let (vod, _) = db.add_vod(&crate::edits::tests::info("abc")).unwrap();
            db.add_mark(vod.id, "chapter", 5.0, Some("The game")).unwrap();
            vod.id
        };
        // Back to version 30: the column 31 adds, gone.
        Connection::open(&path)
            .unwrap()
            .execute_batch(concat!(
                "ALTER TABLE marks DROP COLUMN icon; ALTER TABLE vods DROP COLUMN annotations; ",
                "ALTER TABLE vods DROP COLUMN annotations_version; ALTER TABLE vods DROP COLUMN annotations_imported; ",
                "PRAGMA user_version = 30;"
            ))
            .unwrap();
        let db = Db::open(&path).unwrap();
        let version: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, 32);
        let marks = db.marks(vod).unwrap();
        assert_eq!((marks[0].text.as_deref(), marks[0].detail.icon.as_deref()), (Some("The game"), None));
        // And an icon, once there is one, is kept and read back.
        let clip = db.vod_clip(vod).unwrap().unwrap();
        let detail = MarkDetail { icon: Some("🎮".into()), ..MarkDetail::default() };
        db.insert_mark_with(clip, vod, "chapter", Flicks::from_seconds(9.0), None, &detail).unwrap();
        assert_eq!(db.marks(vod).unwrap()[1].detail.icon.as_deref(), Some("🎮"));
    }

    #[test]
    fn vods_from_before_annotations_were_looked_up_are_unknown() {
        let dir = crate::TempDir::new("migrate-v31");
        let path = dir.join("library.sqlite");
        let vod = {
            let db = Db::open(&path).unwrap();
            db.add_vod(&crate::edits::tests::info("abc")).unwrap().0.id
        };
        // Back to version 31: the columns 32 adds, gone.
        Connection::open(&path)
            .unwrap()
            .execute_batch(concat!(
                "ALTER TABLE vods DROP COLUMN annotations; ALTER TABLE vods DROP COLUMN annotations_version; ",
                "ALTER TABLE vods DROP COLUMN annotations_imported; PRAGMA user_version = 31;"
            ))
            .unwrap();
        let db = Db::open(&path).unwrap();
        let version: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, 32);
        assert_eq!(db.vod(vod).unwrap().unwrap().annotations, Availability::Unknown);
        let listed = db.vods().unwrap();
        assert_eq!(listed[0].annotations, Availability::Unknown);
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
        // As v12 left them (since v27 the VOD's own error is unread).
        let mut stmt = db.conn.prepare("SELECT error FROM vods").unwrap();
        let errors: Vec<Option<String>> = stmt.query_map([], |r| r.get(0)).unwrap().map(|e| e.unwrap()).collect();
        assert!(errors.contains(&Some("Stopped".into())) && errors.contains(&Some("No audio".into())));
    }

    #[test]
    fn ids_are_never_reused() {
        let dir = crate::TempDir::new("ids");
        let path = dir.join("library.sqlite");
        let before: Vec<_> = {
            // A library from before v14, whose newest VOD was deleted.
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
                MIGRATE_V12,
                MIGRATE_V13,
            ];
            conn.execute_batch(&format!("{} PRAGMA user_version = 13;", before.join("\n"))).unwrap();
            conn.execute_batch(
                "INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES
                     ('kick', 'a', 'u', 't', 'done', 0), ('kick', 'b', 'u', 't', 'done', 0);",
            )
            .unwrap();
            conn.query_row("SELECT max(id) FROM vods", [], |r| r.get::<_, i64>(0)).into_iter().collect()
        };
        let db = Db::open(&path).unwrap();
        let a = db.add_vod(&info("a2")).unwrap().0;
        assert!(a.id > before[0]);
        db.delete_vod(a.id).unwrap();
        let b = db.add_vod(&info("b2")).unwrap().0;
        assert!(b.id > a.id, "the deleted VOD's id isn't given out again");
        // Even after a VOD added without taking an id (as an older version would).
        db.conn
            .execute("INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES ('kick', 'c', 'u', 't', 'done', 0)", [])
            .unwrap();
        let older = db.conn.last_insert_rowid();
        db.delete_vod(older).unwrap();
        assert!(db.add_vod(&info("d2")).unwrap().0.id > older);
    }

    #[test]
    fn audio_alone_by_file_type() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = |name: &str| db.add_vod(&info(name)).unwrap().0.id;
        let (song, film, kept, nothing) = (vod("a"), vod("b"), vod("c"), vod("d"));
        db.set_source_path(song, "/music/song.MP3").unwrap();
        db.set_source_path(film, "/videos/film.mkv").unwrap();
        db.set_media_path(kept, Some("/data/media/3.m4a")).unwrap();
        let audio_only = |id| db.vod(id).unwrap().unwrap().audio_only;
        assert!(audio_only(song) && audio_only(kept));
        assert!(!audio_only(film) && !audio_only(nothing));
        db.set_media_path(kept, Some("/data/media/3.mp4")).unwrap();
        assert!(!audio_only(kept));
    }

    #[test]
    fn slices_written_over_by_an_older_version_are_dropped() {
        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        db.add_chat(vod, &[], &[10.0, f64::MAX]).unwrap();
        assert_eq!(db.chat_slices(vod).unwrap(), Some(vec![10.0, f64::MAX]));
        // 0.2.1 cleared the chat and fetched a little, saving its own position.
        db.conn.execute("UPDATE vods SET chat_resume = 5.0 WHERE id = ?1", [vod]).unwrap();
        assert_eq!(db.chat_slices(vod).unwrap(), None, "start over");
        db.clear_chat(vod).unwrap();
        db.add_chat(vod, &[], &[3.0]).unwrap();
        assert_eq!(db.chat_slices(vod).unwrap(), Some(vec![3.0]), "and carry on after that");
    }

    #[test]
    fn marks_come_in_time_order_and_go_with_their_vod() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let b = db.add_mark(vod, "cut", 20.0, None).unwrap();
        let a = db.add_mark(vod, "cut", 10.0, None).unwrap();
        let starts = |db: &Db| db.marks(vod).unwrap().into_iter().map(|m| m.start).collect::<Vec<_>>();
        assert_eq!(starts(&db), [10.0, 20.0]);
        assert!(db.move_mark(a.id, 30.0).unwrap());
        assert_eq!(starts(&db), [20.0, 30.0]);
        db.delete_mark(b.id).unwrap();
        assert_eq!(starts(&db), [30.0]);
        assert!(!db.move_mark(b.id, 1.0).unwrap(), "gone");
        let chapter = db.add_mark(vod, "chapter", 5.0, Some("Intro")).unwrap();
        assert!(db.set_mark_text(chapter.id, Some("Lunch")).unwrap());
        let marks = db.marks(vod).unwrap();
        assert_eq!((marks[0].kind.as_str(), marks[0].text.as_deref()), ("chapter", Some("Lunch")));
        assert!(db.set_mark_text(chapter.id, None).unwrap());
        assert_eq!(db.marks(vod).unwrap()[0].text, None);
        assert!(!db.set_mark_text(b.id, None).unwrap(), "gone");
        db.delete_vod(vod).unwrap();
        assert!(db.marks(vod).unwrap().is_empty());
    }

    #[test]
    fn clips_keep_their_order_and_follow_their_range() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let a = db.add_clip(vod, 10.0, 20.0, "first", true).unwrap();
        let b = db.add_clip(vod, 5.0, 8.0, "second", false).unwrap();
        assert_eq!(a.status, ClipStatus::Waiting);
        assert!(!b.shown, "dragged out only");
        db.show_clip(b.id).unwrap();
        assert!(db.clip(b.id).unwrap().unwrap().shown);
        let titles = |db: &Db| db.clips(vod).unwrap().into_iter().map(|c| c.title).collect::<Vec<_>>();
        assert_eq!(titles(&db), ["first", "second"], "in the order they were made");
        assert_eq!(db.unfinished_clips().unwrap().len(), 2);

        // A cut that finishes after its range moved isn't recorded.
        db.set_clip_range(a.id, 11.0, 20.0).unwrap();
        assert!(!db.set_clip_file(a.id, 10.0, 20.0, ClipStatus::Ready, Some("/c/a.mp4"), None).unwrap());
        assert!(db.set_clip_file(a.id, 11.0, 20.0, ClipStatus::Ready, Some("/c/a.mp4"), None).unwrap());
        let a = db.clip(a.id).unwrap().unwrap();
        assert_eq!((a.status, a.file_path.as_deref()), (ClipStatus::Ready, Some("/c/a.mp4")));
        // Moved again: made again, at the same file.
        db.set_clip_range(a.id, 12.0, 20.0).unwrap();
        let a = db.clip(a.id).unwrap().unwrap();
        assert_eq!((a.status, a.file_path.as_deref()), (ClipStatus::Waiting, Some("/c/a.mp4")));
        db.rename_clip(b.id, "renamed").unwrap();
        db.delete_clip(a.id).unwrap();
        assert_eq!(titles(&db), ["renamed"]);
        // One made only to drag out is forgotten later; its file was handed out.
        let c = db.add_clip(vod, 1.0, 2.0, "dragged", false).unwrap();
        assert!(!db.clip_handed_out(c.id).unwrap());
        db.hand_out_clip(c.id).unwrap();
        assert!(db.clip_handed_out(c.id).unwrap());
        assert_eq!(db.forget_hidden_clips().unwrap(), [c.id]);
        assert_eq!(titles(&db), ["renamed"]);
        // They go with their VOD.
        db.delete_vod(vod).unwrap();
        assert!(db.clip(b.id).unwrap().is_none());
    }

    #[test]
    fn lines_all_or_between() {
        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let line = |start: f64, text: &str| Segment { start, end: start + 1.0, text: text.into(), words: Vec::new() };
        db.insert_segments(vod, &[line(0.0, "one"), line(2.5, "two")]).unwrap();
        db.insert_segments(vod, &[line(4.0, "three")]).unwrap();
        let texts = db.lines(vod).unwrap().into_iter().map(|l| l.text).collect::<Vec<_>>();
        assert_eq!(texts, ["one", "two", "three"]);
        let between =
            |from, to| db.lines_between(vod, from, to).unwrap().into_iter().map(|l| l.text).collect::<Vec<_>>();
        assert_eq!(between(2.5, Some(4.0)), ["two"], "from its start, to before its end");
        assert_eq!(between(1.0, None), ["two", "three"], "on to the end");
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
            db.add_chat(id, &[message(i as f64, "v", &format!("chat{text}"))], &[i as f64]).unwrap();
        }
        assert_eq!(db.search(secret, &SearchFilter::default(), 10).unwrap().hits.len(), 1);
        while db.delete_some_of(vod.id, 1000).unwrap() > 0 {}
        // A batch saved after the erase, by a fetch that hadn't stopped yet.
        db.add_chat(vod.id, &[message(9.0, "v", "late quokkazanzibar")], &[9.0]).unwrap();
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
        for status in [Queued, Downloading, Ready, Transcribing, Paused, Done, Error, Status::None] {
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
    }

    #[test]
    fn interrupted_jobs_return_to_their_queues() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let [down, trans, ready, done] = ["d", "t", "r", "x"].map(|id| db.add_vod(&info(id)).unwrap().0.id);
        let ask = |id, status, progress| {
            let stretch = db.request(id, 0.0, None).unwrap().id;
            db.set_stretch_status(stretch, status, progress, None).unwrap();
        };
        ask(down, Status::Downloading, 0.4);
        ask(trans, Status::Transcribing, 0.6);
        ask(ready, Status::Ready, 0.0);
        db.finish_stretch(db.request(done, 0.0, None).unwrap().id).unwrap();
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
        let db = Db::open(Path::new(":memory:")).unwrap();
        let [link, file_a, file_b] = ["l", "a", "b"].map(|id| db.add_vod(&info(id)).unwrap().0.id);
        db.set_source_path(file_a, "/videos/a.mp4").unwrap();
        db.set_source_path(file_b, "/videos/b.mp4").unwrap();
        let [_, a, b] = [link, file_a, file_b].map(|id| db.request(id, 0.0, None).unwrap().id);
        let next = |local| db.next_stretch(Status::Queued, local).unwrap().map(|(v, _)| v.id);
        // In the order asked, as for links, but never a link.
        assert_eq!(next(false), Some(link));
        assert_eq!(next(true), Some(file_a));
        db.finish_stretch(a).unwrap();
        assert_eq!(next(true), Some(file_b));
        db.finish_stretch(b).unwrap();
        assert_eq!(next(true), None);
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

    #[test]
    fn an_edits_chat_is_its_pieces_vods_where_they_play() {
        let mut db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let at = [5.0, 11.0, 13.0, 15.0, 31.0];
        let messages: Vec<_> = at.iter().map(|&t| message(t, "a", &format!("at {t}"))).collect();
        db.add_chat(vod, &messages, &[]).unwrap();
        let edit = db.add_edit("e").unwrap().id;
        // 10-20 s with 12-14 struck (plays 0-2, 2-8), then 30-35 s (8-13).
        db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
        db.add_piece(edit, vod, (30.0, 35.0), "b", None).unwrap();
        let first = db.piece_rows(edit).unwrap()[0].id;
        db.set_piece_struck(first, &[(12.0, 14.0)]).unwrap();
        let w = db.edit_chat_windows(edit, &[(0.0, 13.0), (2.0, 9.5)], 2).unwrap();
        let times = |i: usize| w[i].messages.iter().map(|m| m.time).collect::<Vec<_>>();
        assert_eq!((times(0), w[0].total), (vec![1.0, 3.0], 3), "11 and 15 s; 31 s counted; 13 s struck");
        assert_eq!((times(1), w[1].total), (vec![3.0, 9.0], 2));
        assert_eq!(w[0].messages[0].platform.as_deref(), Some("youtube"));
        let (count, histogram) = db.edit_chat_histogram(edit, 13.0, 13).unwrap();
        assert_eq!(count, 3);
        assert_eq!((histogram[1], histogram[3], histogram[9]), (1, 1, 1));
        // Before the page knows the edit's length, it still learns there's chat.
        assert_eq!(db.edit_chat_histogram(edit, 0.0, 13).unwrap(), (3, vec![0; 13]));
        // Its events, where they play: a sub at 16 s (4 s in) and one at 13 s (struck).
        let sub = |time| ChatMessage { kind: Kind::Sub, amount: Some(1.0), ..message(time, "b", "subbed") };
        db.add_chat(vod, &[sub(16.0), sub(13.0)], &[]).unwrap();
        let events = db.edit_chat_events(edit).unwrap();
        assert_eq!(events.iter().map(|e| e.time).collect::<Vec<_>>(), [4.0]);
        assert_eq!(events[0].platform.as_deref(), Some("youtube"));
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
        db.add_chat(a, &[message(1.0, "v", "an apple a day")], &[1.0]).unwrap();
        db.add_chat(b, &[message(2.0, "v", "apple pie")], &[2.0]).unwrap();
        db.add_chat(a, &[message(3.0, "v", "apples again")], &[3.0]).unwrap();
        let chat = |query: &str, vod_id: Option<i64>| {
            let filter = SearchFilter { vod_id, speech: false, chat: true, ..Default::default() };
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
        db.add_chat(a, &many, &[2500.0]).unwrap();
        let mut subs = message(1.0, "s", "resub");
        subs.kind = Kind::Sub;
        db.add_chat(b, &[subs.clone(), subs.clone(), subs], &[2.0]).unwrap();
        let lock = std::sync::Mutex::new(db);
        let mut store = LibraryChat { db: &lock, vod_id: b, on_save: Box::new(|| {}) };
        let ids: Vec<i64> = store.of_kind(Kind::Sub).unwrap().into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids.len(), 3);
        store.demote(&ids[..2]).unwrap();
        assert_eq!(store.of_kind(Kind::Sub).unwrap().len(), 1);
        let mut store = LibraryChat { db: &lock, vod_id: a, on_save: Box::new(|| {}) };
        store.clear().unwrap();
        let db = lock.lock().unwrap();
        assert_eq!(db.chat_count(a).unwrap(), 0);
        assert_eq!(db.chat_slices(a).unwrap(), None);
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
        db.add_chat(a, &[message(15.0, "late", "arrived later"), message(16.0, "later", "and again")], &[0.0]).unwrap();
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
        assert_eq!(db.vod(b).unwrap().unwrap().chat_error.as_deref(), Some("Kick isn't responding"), "said why");
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
