use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::chat::{ChatMessage, ChatPart, Kind};
use crate::fetch::VodInfo;
use crate::settings::ClipEncoding;
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
    /// For a file on this computer, its path. It's read in place, never
    /// changed: the user's own file where it is, or the app's copy of one
    /// received from another app's drop (`received`).
    pub source_path: Option<String>,
    /// Its file is the app's own copy, received from another app's drop
    /// (`Paths::source_received`), deleted with it.
    pub received: bool,
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
    /// It brought in published annotations, and another version of them is
    /// published since (`annotations::Offer::newer`).
    pub annotations_newer: bool,
    /// Its download is under way, waiting, paused or failed part way, with a
    /// part file on this computer and nothing kept yet: the app shows that
    /// file as it is (`pipeline::mark_partial`). False as read from the library.
    pub partial: bool,
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
    /// Where the library's files are (`Paths`): every path the library
    /// gives is built from these, a row's id and what it records of the
    /// file (its extension); none is stored.
    pub(crate) files: crate::Paths,
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
pub(crate) const FLICKS: &str = "705600000";

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

/// Version 33: when a VOD started (`started_at`), as an RFC 3339 time in UTC
/// to the second, so VODs of one moment from different streams line up. The
/// best known, in this order: from its chat (`set_chat_start`: each message's
/// time sent less its time into the VOD, the median; to the second), else
/// from the site's own metadata when it was added (`VodInfo::started_at`:
/// when the site made the video, which can be tens of seconds off the first
/// frame), else an annotations file's `recorded_at` (`set_start_if_unknown`).
/// A chat always replaces the others; the others never replace anything.
/// Null when nothing says.
const MIGRATE_V33: &str = "
ALTER TABLE vods ADD COLUMN started_at TEXT;";

/// Version 34: an import's other points of view (`povs`: other streamers'
/// VODs that ran alongside, annotations.rs `Pov`, as a JSON array), on the
/// VOD's annotations row. Null for an import from before: none known.
const MIGRATE_V34: &str = "
ALTER TABLE vod_annotations ADD COLUMN povs TEXT;";

/// Version 35: which of an import's chapters or highlights a mark was brought
/// in as (`imported_as`: its kind and start in the file), kept when the user
/// changes it, so the next import doesn't add that one again beside theirs.
/// Null for the user's own marks and for marks imported before.
const MIGRATE_V35: &str = "
ALTER TABLE marks ADD COLUMN imported_as TEXT;";

/// Version 36: a kept stretch (a source realized from a VOD, `add_realized`)
/// was copied without who speaks each line and what each chat message is (a
/// sub, a gift, a tip), how much and when it happened. Where the VOD it was
/// kept from is still in the library (keeping a later stretch failed, so the
/// VOD stayed), those come back from its rows; where the VOD is gone, so are
/// they. A kept stretch's `platform_id` is `<vod>:<start>-<end>:<when>`.
const MIGRATE_V36: &str = "
CREATE TEMP TABLE kept AS
    SELECT id, CAST(substr(platform_id, 1, instr(platform_id, ':') - 1) AS INTEGER) AS vod_id,
           CAST(substr(span, 1, instr(span, '-') - 1) AS REAL) AS start,
           CAST(substr(span, instr(span, '-') + 1) AS REAL) AS end
    FROM (SELECT id, platform_id,
                 substr(platform_id, instr(platform_id, ':') + 1,
                        instr(substr(platform_id, instr(platform_id, ':') + 1), ':') - 1) AS span
          FROM vods WHERE platform = 'realized');
UPDATE segments SET speaker = (
    SELECT o.speaker FROM kept JOIN segments AS o ON o.vod_id = kept.vod_id
    WHERE kept.id = segments.vod_id AND o.speaker IS NOT NULL AND o.end > kept.start AND o.start < kept.end
      AND abs(max(o.start - kept.start, 0) - segments.start) < 0.000001
      AND abs(min(o.end - kept.start, kept.end - kept.start) - segments.end) < 0.000001
    ORDER BY o.start LIMIT 1)
WHERE speaker IS NULL AND vod_id IN (SELECT id FROM kept);
UPDATE chat SET (kind, amount, event_time) = (
    SELECT o.kind, o.amount, o.event_time - kept.start FROM kept JOIN chat AS o ON o.vod_id = kept.vod_id
    WHERE kept.id = chat.vod_id AND o.kind <> 'message' AND o.author = chat.author AND o.text = chat.text
      AND abs(o.time - kept.start - chat.time) < 0.000001
    ORDER BY o.id LIMIT 1)
WHERE kind = 'message' AND vod_id IN (SELECT id FROM kept) AND EXISTS (
    SELECT 1 FROM kept JOIN chat AS o ON o.vod_id = kept.vod_id
    WHERE kept.id = chat.vod_id AND o.kind <> 'message' AND o.author = chat.author AND o.text = chat.text
      AND abs(o.time - kept.start - chat.time) < 0.000001);
DROP TABLE kept;";

/// Version 37: the channel's login or handle as the site gave it
/// (`channel_login`, `VodInfo::channel_login`), beside its display name
/// (`channel`), which can differ. Null for a VOD added before: unknown.
const MIGRATE_V37: &str = "
ALTER TABLE vods ADD COLUMN channel_login TEXT;";

/// Version 38: the contexts an import names (`contexts`: annotations.rs
/// `Annotations::contexts`, as a JSON array), on the VOD's annotations row,
/// which say which sections of a streamer card show. Null for an import from
/// before: none known.
/// Version 40: the channel's page as the site gave it (`channel_url`,
/// `VodInfo::channel_url`), for a site whose channels' addresses can't be
/// built from a login. Kept on the row because the info it comes from is
/// removed with the VOD's work files once it's transcribed. Null for a VOD
/// added before, filled from its info where that is still kept
/// (`pipeline::fill_channels`).
const MIGRATE_V40: &str = "
ALTER TABLE vods ADD COLUMN channel_url TEXT;";

const MIGRATE_V38: &str = "
ALTER TABLE vod_annotations ADD COLUMN contexts TEXT;";

/// Version 41: the codec and quality a clip's (or edit's) file was made
/// with (`Clip::encoding`, as JSON), so a change of either in Settings makes
/// it again as a change of format does. Null for a file made before: known
/// only by its extension.
const MIGRATE_V41: &str = "
ALTER TABLE renders ADD COLUMN encoding TEXT;";

/// Version 42: a deleted clip's id is never given to a new one, as a VOD's
/// isn't (v14), so what still holds it (an undo step, the clipboard, the
/// page, the clip's picture and files, all named by it) can't reach the new
/// clip. Edits and VODs' own clips are rows of `clips` too, and a render is
/// keyed by its clip, so the one counter covers them all. `clip_ids` holds
/// the next id; `Db::new_clip` takes it, and the trigger moves it past any id
/// inserted another way (an older version's, after a downgrade).
const MIGRATE_V42: &str = "
CREATE TABLE clip_ids (next INTEGER NOT NULL);
INSERT INTO clip_ids SELECT coalesce(max(id), 0) + 1 FROM clips;
CREATE TRIGGER clips_ids AFTER INSERT ON clips BEGIN
    UPDATE clip_ids SET next = max(next, NEW.id + 1);
END;";

/// Version 43: which make of a clip (or edit) its file is (`Paths::clip_make`):
/// each make writes a file of its own. Null for one made before, at the path
/// its id alone builds, which it keeps; so nothing is moved.
const MIGRATE_V43: &str = "
ALTER TABLE renders ADD COLUMN make INTEGER;";

/// Version 44: the stretches' `note` (v27) goes; nothing ever read or wrote
/// it. A version before this one opened on the library reads and writes the
/// stretches by their columns' names, none of them `note`.
const MIGRATE_V44: &str = "
ALTER TABLE stretches DROP COLUMN note;";

/// Version 39: no path to a file of the library's own is read from the
/// library; each is built from its kind and its row's id (`Paths`), and the
/// row keeps what can't be derived: the extension, which says too that the
/// file is there. Only a VOD's file on this computer, outside the library,
/// keeps its path (`source_path`). The old columns still get the full path
/// written beside the extension, for 0.7.1 opened on this library, and
/// `library::settle` moves what they record that isn't where it's built.
const MIGRATE_BUILT_PATHS: &str = "
ALTER TABLE vods ADD COLUMN video_ext TEXT;
ALTER TABLE vods ADD COLUMN audio_ext TEXT;
ALTER TABLE vods ADD COLUMN received_ext TEXT;
ALTER TABLE stretches ADD COLUMN audio_ext TEXT;
ALTER TABLE renders ADD COLUMN file_ext TEXT;";

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
    "id, platform, platform_id, url, title, channel, channel_login, channel_url, duration, thumbnail, published, started_at, added_at,
     note, thumb, source_path, video_ext, received_ext, chat, chat_error, annotations, annotations_version, annotations_imported,
     EXISTS (SELECT 1 FROM segments WHERE segments.vod_id = vods.id) AS has_segments,
     EXISTS (SELECT 1 FROM clips WHERE clips.whole_of = vods.id) AS listed,
     (SELECT json_group_array(json_array(s.id, s.vod_id, s.start, s.end, s.status, s.progress, s.reached, s.error, s.model))
      FROM stretches s WHERE s.vod_id = vods.id) AS stretches";

pub(crate) fn vod_from_row(r: &Row, files: &crate::Paths) -> rusqlite::Result<Vod> {
    let id: i64 = r.get("id")?;
    let video = ext_of(r.get("video_ext")?);
    let received = ext_of(r.get("received_ext")?);
    let source_path = match &received {
        Some(ext) => Some(files.source_received(id, ext).to_string_lossy().into_owned()),
        None => r.get("source_path")?,
    };
    // What plays here, by its file type.
    let played = source_path.clone().or_else(|| video.as_ref().map(|ext| format!("video.{ext}")));
    let duration: Option<f64> = r.get("duration")?;
    let stretches = crate::stretches::from_json(&r.get::<_, String>("stretches")?)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into()))?;
    let shown = crate::stretches::summarize(&stretches, duration, r.get("has_segments")?);
    Ok(Vod {
        id,
        info: VodInfo {
            platform: r.get("platform")?,
            platform_id: r.get("platform_id")?,
            url: r.get("url")?,
            title: r.get("title")?,
            channel: r.get("channel")?,
            channel_login: r.get("channel_login")?,
            channel_url: r.get("channel_url")?,
            duration,
            thumbnail: r.get("thumbnail")?,
            published: r.get("published")?,
            started_at: r.get("started_at")?,
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
        source_path,
        received: received.is_some(),
        media: video.is_some(),
        audio_only: played
            .and_then(|p| crate::local::media_type(Path::new(&p)))
            .is_some_and(|mime| mime.starts_with("audio/")),
        outdated: shown.outdated,
        chat: ChatStatus::parse(r.get::<_, Option<String>>("chat")?.as_deref()),
        chat_error: r.get("chat_error")?,
        listed: r.get("listed")?,
        annotations: Availability::parse(r.get::<_, Option<String>>("annotations")?.as_deref()),
        // Versions as the file host names them (`same_version`), not as given.
        annotations_newer: match (
            r.get::<_, Option<String>>("annotations_version")?,
            r.get::<_, Option<String>>("annotations_imported")?,
        ) {
            (Some(seen), Some(imported)) => !crate::annotations_filter::same_version(&seen, &imported),
            _ => false,
        },
        partial: false,
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
    /// What its file was made with (`set_made_as`); None if it isn't made, or
    /// was made before this was recorded.
    pub encoding: Option<ClipEncoding>,
    /// On a VOD's clip bar, whether it still plays just its stretch of that
    /// VOD: one piece of it, nothing struck, not in a group (as
    /// `edits::cut_whole`), not changed in the editor. A drag of the stretch
    /// from the VOD gives only such a clip (`Db::vod_clip_playing`).
    pub plain: bool,
}

/// Whose clip bar a clip is on.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ClipBar {
    Vod { id: i64 },
    Edit { id: i64 },
}

impl ClipBar {
    /// Whose bar a clip is on, from its `bar_of` and that clip's `whole_of`:
    /// a VOD's bar is its own clip's (`whole_of` set), an edit's the edit's.
    /// None for a listed clip (no `bar_of`).
    pub(crate) fn of(bar_of: Option<i64>, bar_whole_of: Option<i64>) -> Option<ClipBar> {
        match (bar_of, bar_whole_of) {
            (_, Some(vod)) => Some(ClipBar::Vod { id: vod }),
            (Some(edit), None) => Some(ClipBar::Edit { id: edit }),
            (None, None) => None,
        }
    }
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
    /// A highlight's end, when it has one: in seconds of the timeline it's
    /// on, as the mark's start is.
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
    /// Which of an import's chapters or highlights it was brought in as, kept
    /// after the user changes it (`MIGRATE_V35`); the page has no use for it.
    #[serde(skip)]
    pub imported_as: Option<String>,
}

impl Default for MarkDetail {
    fn default() -> Self {
        Self {
            end: None,
            category: None,
            note: None,
            cast: Vec::new(),
            icon: None,
            source: USER_MARK.into(),
            imported_as: None,
        }
    }
}

/// The `source` of a mark made in the app.
pub const USER_MARK: &str = "user";

/// The columns of `marks` that `mark_detail` reads, in its order.
pub(crate) const MARK_DETAIL_COLUMNS: &str = "end_at, category, note, source, people, icon, imported_as";

/// A mark's detail from row `r`, its `MARK_DETAIL_COLUMNS` from column `from`
/// on; its end in seconds of its VOD.
pub(crate) fn mark_detail(r: &rusqlite::Row, from: usize) -> rusqlite::Result<MarkDetail> {
    Ok(MarkDetail {
        end: r.get::<_, Option<i64>>(from)?.map(|f| Flicks(f).seconds()),
        category: r.get(from + 1)?,
        note: r.get(from + 2)?,
        source: r.get(from + 3)?,
        // A list the app wrote; one it can't read is no one.
        cast: r.get::<_, Option<String>>(from + 4)?.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default(),
        icon: r.get(from + 5)?,
        imported_as: r.get(from + 6)?,
    })
}

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
    "c.id, i.vod_id, i.start, i.end, c.title, r.status, r.file_ext, r.error, c.shown, c.revision,
     c.bar_of, (SELECT whole_of FROM clips b WHERE b.id = c.bar_of), r.file_path, r.encoding, r.make,
     coalesce((SELECT count(*) = 1 AND max(json_array_length(p.struck)) = 0
                      AND max(p.vod_id) IS (SELECT whole_of FROM clips b WHERE b.id = c.bar_of)
               FROM items p WHERE p.clip_id = c.id), 0)";
/// Joins a clip (`c`) to its first piece (`i`) and its render (`r`). The
/// first piece in timeline order, inside a group if its timeline starts with
/// one (a clip pasted or dropped at its start): a group has no stretch to read.
pub(crate) const CLIP_JOIN: &str = "JOIN items i ON i.id = (
       WITH RECURSIVE walk(id, vod_id, path) AS (
         SELECT id, vod_id, printf('%010d.%015d', position, id) FROM items WHERE clip_id = c.id AND parent IS NULL
         UNION ALL
         SELECT t.id, t.vod_id, walk.path || '/' || printf('%010d.%015d', t.position, t.id)
         FROM items t JOIN walk ON t.parent = walk.id
       )
       SELECT id FROM walk WHERE vod_id IS NOT NULL ORDER BY path LIMIT 1)
     JOIN renders r ON r.clip_id = c.id";

pub(crate) fn clip_from_row(r: &Row, files: &crate::Paths) -> rusqlite::Result<Clip> {
    let id = r.get(0)?;
    let mut clip = Clip {
        id,
        vod_id: r.get(1)?,
        start: Flicks(r.get(2)?).seconds(),
        end: Flicks(r.get::<_, Option<i64>>(3)?.unwrap_or(0)).seconds(),
        title: r.get(4)?,
        status: ClipStatus::parse(&r.get::<_, String>(5)?),
        file_path: clip_file_of(files, id, r.get(6)?, r.get(14)?, r.get(12)?),
        error: r.get(7)?,
        shown: r.get(8)?,
        revision: r.get(9)?,
        bar: ClipBar::of(r.get(10)?, r.get(11)?),
        length: 0.0,
        encoding: r.get::<_, Option<String>>(13)?.and_then(|e| serde_json::from_str(&e).ok()),
        plain: r.get(15)?,
    };
    clip.length = clip.end - clip.start;
    Ok(clip)
}

/// The library's migrations, in the order `Db::open` runs them: migration n
/// brings the schema to version n + 1, and `user_version` is how many have
/// run. A test builds a library at any version from the first n
/// (`testkit::library::at_version`); tests/e2e/fixtures/upgrade-matrix.mjs
/// counts them here.
pub(crate) const MIGRATIONS: &[&str] = &[
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
    MIGRATE_V33,
    MIGRATE_V34,
    MIGRATE_V35,
    MIGRATE_V36,
    MIGRATE_V37,
    MIGRATE_V38,
    MIGRATE_BUILT_PATHS,
    MIGRATE_V40,
    MIGRATE_V41,
    MIGRATE_V42,
    MIGRATE_V43,
    MIGRATE_V44,
];

/// How many copies of the library from before upgrades are kept.
const KEPT_COPIES: usize = 2;

/// How long a copy from before an upgrade is kept: time to find an upgrade
/// gone wrong, but not for good, as a copy keeps what's been deleted since
/// (the privacy page says so).
const COPIES_KEPT_FOR: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);

/// Whether `name` is a copy of the library from before an upgrade
/// (`keep_before_upgrade`).
fn is_copy(name: &str) -> bool {
    name.strip_prefix("library-v").and_then(|n| n.strip_suffix(".sqlite")).is_some_and(|v| v.parse::<i32>().is_ok())
}

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
            is_copy(&e.file_name().to_string_lossy()).then_some(())?;
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
        let old =
            entry.metadata().and_then(|m| m.modified()).is_ok_and(|made| made.elapsed().is_ok_and(|age| age > max_age));
        if is_copy(&entry.file_name().to_string_lossy()) && old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Moves the library that `paths` lays out out of the way, so a new one is
/// made in its place: into a folder `library-broken-<date>` (`-2`… if
/// taken) beside it, in the same layout, never deleted, for recovering or
/// sending with a bug report. Its file goes, with its WAL and shared memory
/// files and its copies from before upgrades (`keep_before_upgrade`), and
/// so do its own folders and files (`Paths::library_files`): its videos,
/// clips and the rest, as a new library's ids start again at 1. Left in
/// place, the next start's tidies would delete them as no VOD's (and the
/// copies as old), and its first VODs and clips would write over them.
/// All or nothing: if one can't be moved, what was goes back, and the
/// error names a file another app has open (`in_use`), the one reason
/// Windows refuses, or, if something couldn't go back, what's left set
/// aside. Returns the folder its file went to.
pub fn set_aside(paths: &crate::Paths) -> Result<PathBuf> {
    set_aside_with(paths, &|from, to| std::fs::rename(from, to), &in_use)
}

/// `set_aside`, moving each file or folder with `rename` and asking
/// `in_use` which file another app has open when a move fails.
fn set_aside_with(
    paths: &crate::Paths,
    rename: &dyn Fn(&Path, &Path) -> std::io::Result<()>,
    in_use: &dyn Fn(&Path) -> Option<PathBuf>,
) -> Result<PathBuf> {
    let date = date_of(now());
    let name = (1..)
        .map(|n| if n == 1 { format!("library-broken-{date}") } else { format!("library-broken-{date}-{n}") })
        .find(|name| {
            let aside = paths.aside(name);
            !aside.root.exists() && !aside.local.exists()
        })
        .context("no free name")?;
    let aside = paths.aside(&name);
    // Built from the path as the system has it: a name made from a lossy
    // copy (`display`) of a path that isn't UTF-8 isn't the file's.
    let side = |db: &Path, suffix: &str| {
        let mut name = db.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    };
    let mut moves = vec![(paths.db(), aside.db())];
    moves.extend(["-wal", "-shm"].map(|suffix| (side(&paths.db(), suffix), side(&aside.db(), suffix))));
    let copies = paths.db().parent().and_then(|dir| std::fs::read_dir(dir).ok());
    for entry in copies.into_iter().flatten().flatten() {
        if is_copy(&entry.file_name().to_string_lossy()) {
            moves.push((entry.path(), aside.db().with_file_name(entry.file_name())));
        }
    }
    moves.extend(paths.library_files().into_iter().zip(aside.library_files()));
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    // The file another app has open, if that's what stopped a move.
    let mut open = None;
    let moving = (|| {
        std::fs::create_dir_all(&aside.root)?;
        std::fs::create_dir_all(&aside.local)?;
        for (from, to) in moves {
            // Only the library's file must be there.
            if from != paths.db() && std::fs::symlink_metadata(&from).is_err() {
                continue;
            }
            if let Err(e) = rename(&from, &to) {
                open = in_use(&from);
                return Err(e)
                    .with_context(|| format!("move {} to {}", crate::tools::shown(&from), crate::tools::shown(&to)));
            }
            moved.push((from, to));
        }
        anyhow::Ok(())
    })();
    let Err(e) = moving else { return Ok(aside.root) };
    let stuck: Vec<String> = moved
        .iter()
        .rev()
        .filter(|(from, to)| rename(to, from).is_err())
        .map(|(_, to)| crate::tools::shown(to))
        .collect();
    if stuck.is_empty() {
        let _ = std::fs::remove_dir(&aside.local);
        let _ = std::fs::remove_dir(&aside.root);
        return Err(match open {
            Some(open) => anyhow::anyhow!(
                "{} is open in another app. Close that app, then start a new library again.",
                crate::tools::shown(&open)
            ),
            None => e,
        });
    }
    // Part of the library is set aside: a new start would begin without
    // it, so it's named, to be moved back by hand first. The paths are
    // whole, as the split layout sets aside into two folders; the cause is
    // only what went wrong, as the in-use sentence's own advice (start
    // again) is the opposite of this one's.
    let cause = match open {
        Some(open) => format!("{} is open in another app", crate::tools::shown(&open)),
        None => format!("{e:#}"),
    };
    Err(anyhow::anyhow!(
        "The library is partly set aside: {} couldn't be moved back after this failed: {cause}. \
         Move them back where they were before trying again.",
        stuck.join(", ")
    ))
}

/// The first file at or under `path` that another app has open, on
/// Windows, where a folder holding one can't be moved: opened sharing
/// nothing, it's refused. Elsewhere an open file doesn't stop a move.
fn in_use(path: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const ERROR_SHARING_VIOLATION: i32 = 32;
        let mut left = vec![path.to_path_buf()];
        while let Some(path) = left.pop() {
            let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
            if meta.is_dir() {
                left.extend(std::fs::read_dir(&path).into_iter().flatten().flatten().map(|e| e.path()));
            } else if std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&path)
                .is_err_and(|e| e.raw_os_error() == Some(ERROR_SHARING_VIOLATION))
            {
                return Some(path);
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
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

/// `secs` since 1970 as an RFC 3339 time in UTC, to the nearest second
/// (`2026-10-14T18:00:05Z`).
pub fn utc_time(secs: f64) -> String {
    let secs = secs.round() as i64;
    let of_day = secs.rem_euclid(86400);
    format!("{}T{:02}:{:02}:{:02}Z", date_of(secs), of_day / 3600, of_day / 60 % 60, of_day % 60)
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

/// Between the parts of a long job on the library (a removed VOD's chat
/// erased a thousand messages at a time), lets a thread waiting for the
/// library's lock take it. A yield doesn't: the lock isn't fair, and the
/// job takes it back before the waiting thread has woken, so a quit or a
/// command could wait out the whole job (an erasure held the quit for over
/// three seconds, 2026-11-05). A millisecond is that thread's time to wake.
pub fn give_way() {
    std::thread::sleep(std::time::Duration::from_millis(1));
}

/// An extension column (`video_ext`, `file_ext`…) as the library reads it:
/// only one the app could have written, 1 to 10 ASCII letters and digits,
/// in lower case. Anything else (a separator, a `..`, from a library someone
/// else made) is read as none, the file unknown, so no path it builds can
/// leave the library's folders, and nothing is deleted for it.
pub(crate) fn ext_of(ext: Option<String>) -> Option<String> {
    ext.filter(|e| (1..=10).contains(&e.len()) && e.bytes().all(|b| b.is_ascii_alphanumeric()))
        .map(|e| e.to_ascii_lowercase())
}

/// Clip `id`'s file as its render row has it: built from its extension
/// (`file_ext`) and which make it is (`make`, `Paths::clip_make`). A row `library::settle` left as 0.7.1 wrote it, with only
/// the full path (`file_path`): the file in 0.7.1's clip folder
/// (`Paths::old_clips`) where it couldn't be linked in, else where its
/// extension would build it, so a file gone reads as gone (and the clip
/// comes off its bar, `forget_gone_clips`) rather than as none at all.
pub(crate) fn clip_file_of(
    files: &crate::Paths,
    id: i64,
    ext: Option<String>,
    make: Option<i64>,
    old: Option<String>,
) -> Option<String> {
    if let Some(ext) = ext_of(ext) {
        return Some(shown(&files.clip_make(id, make.unwrap_or(0), &ext)));
    }
    let old = old?;
    if files.old_clips.as_deref().is_some_and(|dir| crate::library::named_inside(Path::new(&old), dir)) {
        return Some(old);
    }
    let ext = ext_of(Path::new(&old).extension().and_then(|e| e.to_str()).map(str::to_owned))?;
    Some(shown(&files.clip_file(id, &ext)))
}

/// The extension of clip (or edit) `id`'s file `file`, and which make it is:
/// it must be where `Paths::clip_make` builds one, as `built_ext` asks.
pub(crate) fn built_make(files: &crate::Paths, id: i64, file: &str) -> Result<(String, i64)> {
    let stem = Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let make = match stem.strip_prefix(&format!("{id}-")) {
        Some(n) => n.parse().with_context(|| format!("{file} isn't a make of clip {id}"))?,
        None => 0,
    };
    Ok((built_ext(file, |ext| files.clip_make(id, make, ext))?, make))
}

/// The extension of `file`, which must be `built(ext)`: where the library
/// keeps a file of its kind (`Paths`). Writers give the path they wrote,
/// built there; any other is refused, so no row can mean a file elsewhere.
pub(crate) fn built_ext(file: &str, built: impl Fn(&str) -> PathBuf) -> Result<String> {
    let path = Path::new(file);
    let ext = path.extension().and_then(|e| e.to_str()).with_context(|| format!("{file} has no extension"))?;
    let at = built(ext);
    anyhow::ensure!(at == path, "{file} isn't where the library keeps it ({})", at.display());
    Ok(ext.to_owned())
}

/// `path` as the library gives paths: a string, as the page and the
/// players take them.
pub(crate) fn shown(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub(crate) fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Db {
    /// Opens (making or upgrading it) the library at `path`, its files in
    /// the folder it's in.
    pub fn open(path: &Path) -> Result<Self> {
        let files = crate::Paths::new(path.parent().unwrap_or(Path::new("")));
        Self::open_with(path, files)
    }

    /// Opens (making or upgrading it) the library `paths` lays out.
    pub fn open_in(paths: &crate::Paths) -> Result<Self> {
        Self::open_with(&paths.db(), paths.clone())
    }

    fn open_with(path: &Path, files: crate::Paths) -> Result<Self> {
        // Files recorded where they were before 0.7.2 are moved by the app
        // once, as it starts (`library::settle`), not at every open: a job's
        // own connection opened mid-session would move a file under another
        // job that has its path.
        Self::open_inner(path, files).with_context(|| format!("open the library at {}", crate::tools::shown(path)))
    }

    /// A second connection to the library at `path`, for reading only, opened
    /// after `open` (which makes and upgrades it). The library is in WAL mode,
    /// so reads here never wait for writes on the other connection (chat being
    /// saved, a VOD being deleted), and the other way around.
    pub fn open_reader(paths: &crate::Paths) -> Result<Self> {
        let path = paths.db();
        let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("open the library at {} for reading", crate::tools::shown(&path)))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Self { conn, files: paths.clone() })
    }

    /// Takes the library's write lock and keeps it: nothing more is written,
    /// on this connection or another, till the process ends. For quitting,
    /// so jobs whose processes are stopped then don't record a failure.
    pub fn stop_writes(&self) -> Result<()> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        Ok(())
    }

    fn open_inner(path: &Path, files: crate::Paths) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // Transactions take the write lock when they begin, waiting (as above)
        // for another connection's to end. Begun deferred, one that reads and
        // then writes fails "database is locked" at once if another connection
        // (the worker's) has written since its read.
        conn.set_transaction_behavior(rusqlite::TransactionBehavior::Immediate);
        // A test's library is never flushed to the disk: a flush is what
        // survives a power cut, which no test has, and with one at each
        // commit every test waited on the disk, so a disk that stalled for
        // seconds (CI's Windows runners) failed what was waiting then.
        #[cfg(any(test, feature = "testkit"))]
        conn.execute_batch("PRAGMA synchronous = OFF;")?;
        // secure_delete zeroes freed pages, so a removed transcript doesn't linger in the file.
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA secure_delete = ON;")?;
        let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if (version as usize) < MIGRATIONS.len() {
            // A copy as it was first, so an upgrade that turns out wrong can be
            // undone; no copy, no upgrade.
            if version > 0 {
                keep_before_upgrade(&conn, path, version)?;
            }
            let pending = MIGRATIONS[version as usize..].join("\n").replace("FLICKS", FLICKS);
            let latest = MIGRATIONS.len();
            conn.execute_batch(&format!("BEGIN; {pending}"))?;
            if version < 22 {
                struck_to_flicks(&conn)?;
            }
            conn.execute_batch(&format!("PRAGMA user_version = {latest}; COMMIT;"))?;
        } else if version as usize > MIGRATIONS.len() {
            // A newer VODscribe made this library (the user went back a version).
            // The migrations after 0.5.0's only add, or drop what no version reads (V44, the
            // stretches' `note`), so carry on rather than refuse to start. Going back to a version
            // before 0.5.0 isn't supported: V20, V22 and V23 drop tables and columns it reads.
            eprintln!("{}: library format {version} is newer than this version knows", path.display());
        }
        tidy_copies(path, COPIES_KEPT_FOR);
        Ok(Self { conn, files })
    }

    /// Inserts a VOD, or returns the existing row if it was already added.
    pub fn add_vod(&self, info: &VodInfo) -> Result<(Vod, bool)> {
        if let Some(existing) = self.find_vod(&info.platform, &info.platform_id)? {
            if existing.listed {
                return Ok((existing, false));
            }
            // Removed and erased, its row kept for the session as a gone
            // source nothing uses (an undo may yet): what's left goes, and
            // it's added anew rather than listed again empty.
            if self.is_gone(existing.id)? && self.uses(existing.id)? <= 0 {
                self.delete_vod(existing.id)?;
            } else {
                // Taken out of the list but kept for other clips: listed again.
                self.atomically(|db| db.add_vod_clip(existing.id, &existing.info.title))?;
                return Ok((self.vod(existing.id)?.expect("just found"), false));
            }
        }
        // New VODs go to the top of the library: their own clip does (`add_vod_clip`).
        let id = self.atomically(|db| {
            db.conn.execute(
                "INSERT INTO vods (id, platform, platform_id, url, title, channel, channel_login, channel_url, duration, thumbnail, published, started_at, status, added_at)
                 VALUES ((SELECT max(next, (SELECT coalesce(max(id), 0) + 1 FROM vods)) FROM vod_ids),
                         ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'queued', ?12)",
                params![
                    info.platform, info.platform_id, info.url, info.title, info.channel, info.channel_login, info.channel_url,
                    info.duration, info.thumbnail, info.published, info.started_at, now()
                ],
            )?;
            let id = db.conn.last_insert_rowid();
            db.add_vod_clip(id, &info.title)?;
            Ok(id)
        })?;
        Ok((self.vod(id)?.expect("just inserted"), true))
    }

    /// The VOD of `platform`'s `platform_id` in the library, listed or not.
    pub fn find_vod(&self, platform: &str, platform_id: &str) -> Result<Option<Vod>> {
        let sql = format!("SELECT {VOD_COLUMNS} FROM vods WHERE platform = ?1 AND platform_id = ?2");
        Ok(self.conn.query_row(&sql, params![platform, platform_id], |r| vod_from_row(r, &self.files)).optional()?)
    }

    pub fn vod(&self, id: i64) -> Result<Option<Vod>> {
        let sql = format!("SELECT {VOD_COLUMNS} FROM vods WHERE id = ?1");
        Ok(self.conn.query_row(&sql, [id], |r| vod_from_row(r, &self.files)).optional()?)
    }

    /// Every VOD, as a source: those in the library's list, in its order,
    /// then those only other clips use.
    pub fn vods(&self) -> Result<Vec<Vod>> {
        let sql = format!(
            "SELECT {VOD_COLUMNS} FROM vods LEFT JOIN vod_order o ON o.vod_id = vods.id
             ORDER BY o.position IS NULL, o.position, id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        Ok(stmt.query_map([], |r| vod_from_row(r, &self.files))?.collect::<rusqlite::Result<_>>()?)
    }

    /// The VODs in the library's list, in its order.
    pub fn listed_vods(&self) -> Result<Vec<Vod>> {
        let sql = format!(
            "SELECT {VOD_COLUMNS} FROM vods JOIN vod_order o ON o.vod_id = vods.id ORDER BY o.position, id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        Ok(stmt.query_map([], |r| vod_from_row(r, &self.files))?.collect::<rusqlite::Result<_>>()?)
    }

    /// Adds a clip of VOD `vod_id` from `start` to `end` seconds, after its
    /// others on the clip bar (a span of the VOD, `vod_span`: its end cut at
    /// the VOD's); `shown` if it goes in the bar. A snippet: a clip of one
    /// piece, hanging off the VOD's own clip.
    pub fn add_clip(&self, vod_id: i64, start: f64, end: f64, title: &str, shown: bool) -> Result<Clip> {
        let bar = self.vod_clip(vod_id)?.context("That VOD is gone.")?;
        let (start, end) = self.vod_span(vod_id, start, end)?;
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
        self.conn
            .query_row(&sql, [id], |r| clip_from_row(r, &self.files))
            .optional()?
            .map(|c| self.measured(c))
            .transpose()
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
        Ok(stmt.query_map([vod_id], |r| clip_from_row(r, &self.files))?.collect::<rusqlite::Result<_>>()?)
    }

    /// The clips on listed clip `id`'s clip bar (an edit's), in their order.
    pub fn bar_clips(&self, id: i64) -> Result<Vec<Clip>> {
        let sql =
            format!("SELECT {CLIP_COLUMNS} FROM clips c {CLIP_JOIN} WHERE c.bar_of = ?1 ORDER BY c.bar_position, c.id");
        let mut stmt = self.conn.prepare(&sql)?;
        let clips: Vec<Clip> =
            stmt.query_map([id], |r| clip_from_row(r, &self.files))?.collect::<rusqlite::Result<_>>()?;
        clips.into_iter().map(|c| self.measured(c)).collect()
    }

    /// Clips whose file is still to be made (or was being made when the app stopped).
    pub fn unfinished_clips(&self) -> Result<Vec<Clip>> {
        let sql = format!(
            "SELECT {CLIP_COLUMNS} FROM clips c {CLIP_JOIN}
             WHERE c.bar_of IS NOT NULL AND r.status IN ('waiting', 'cutting') ORDER BY c.id"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        Ok(stmt.query_map([], |r| clip_from_row(r, &self.files))?.collect::<rusqlite::Result<_>>()?)
    }

    /// Moves clip `id` to `start`..`end` seconds of its VOD (`vod_span`); its
    /// file is to be made again.
    pub fn set_clip_range(&self, id: i64, start: f64, end: f64) -> Result<()> {
        let clip = self.clip(id)?.context("That clip is gone.")?;
        let (start, end) = self.vod_span(clip.vod_id, start, end)?;
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

    /// Takes clip `id` off the clip bar, kept with its file (a cut held for a
    /// paste): like one only dragged out, it's forgotten at the next start
    /// (`forget_hidden_clips`) unless something puts it back first.
    pub fn hide_clip(&self, id: i64) -> Result<()> {
        let bar: Option<i64> =
            self.conn.query_row("SELECT bar_of FROM clips WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten();
        let Some(bar) = bar else { return Ok(()) };
        self.change(bar, None, |db| {
            db.conn.execute("UPDATE clips SET shown = 0 WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// Notes that clip `id`'s file was dragged out of the app: it's someone
    /// else's now, so it stays when the clip goes.
    pub fn hand_out_clip(&self, id: i64) -> Result<()> {
        // (2, `hand_out_clip_file`, says more, and stays.)
        self.conn.execute("UPDATE renders SET handed_out = max(handed_out, 1) WHERE clip_id = ?1", [id])?;
        Ok(())
    }

    /// Notes that clip `id`'s own file itself (or edit `id`'s video) went to
    /// another app, as none could be made under a second name (a disk that
    /// takes no hard links, such as exFAT): from now the app never deletes
    /// it (`clip_file_handed_out`). It's handed out too (`clip_handed_out`).
    pub fn hand_out_clip_file(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE renders SET handed_out = 2 WHERE clip_id = ?1", [id])?;
        Ok(())
    }

    /// Whether clip `id`'s own file itself went to another app
    /// (`hand_out_clip_file`).
    pub fn clip_file_handed_out(&self, id: i64) -> Result<bool> {
        let out: Option<i64> =
            self.conn.query_row("SELECT handed_out FROM renders WHERE clip_id = ?1", [id], |r| r.get(0)).optional()?;
        Ok(out == Some(2))
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

    /// Forgets the clips not in the clip bar (made only to be dragged out);
    /// gives each one's id, its file and whether that was handed out to
    /// another app, for the caller to delete what's the app's: no file for
    /// one whose own file itself went (`hand_out_clip_file`), as it stays.
    pub fn forget_hidden_clips(&self) -> Result<Vec<(i64, Option<String>, bool)>> {
        let mut stmt = self.conn.prepare("SELECT id FROM clips WHERE bar_of IS NOT NULL AND shown = 0")?;
        let ids: Vec<i64> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        let mut gone = Vec::new();
        for id in ids {
            let given = self.clip_file_handed_out(id)?;
            let file = self.clip(id)?.and_then(|c| c.file_path).filter(|_| !given);
            gone.push((id, file, self.clip_handed_out(id)?));
        }
        self.conn.execute("DELETE FROM clips WHERE bar_of IS NOT NULL AND shown = 0", [])?;
        Ok(gone)
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
        let (ext, make) = file.map(|f| built_make(&self.files, id, f)).transpose()?.unzip();
        let n = self.conn.execute(
            "UPDATE renders SET status = ?4, file_ext = coalesce(?5, file_ext), file_path = coalesce(?7, file_path), error = ?6,
                 make = CASE WHEN ?5 IS NULL THEN make ELSE ?8 END
             WHERE clip_id = ?1 AND EXISTS (
                 SELECT 1 FROM items WHERE clip_id = ?1 AND parent IS NULL AND start = ?2 AND end = ?3
                 ORDER BY position, id LIMIT 1)",
            params![id, Flicks::from_seconds(start).0, Flicks::from_seconds(end).0, status.as_str(), ext, error, file, make],
        )?;
        Ok(n > 0)
    }

    /// Where clip (or edit) `id`'s next make goes, as `ext`: a file of its
    /// own, after the one it has (`Paths::clip_make`). Recorded once it's
    /// made (`set_clip_file`, `set_edit_render`); one not recorded (it failed,
    /// or its range moved) leaves the next make the same path.
    pub fn next_clip_file(&self, id: i64, ext: &str) -> Result<PathBuf> {
        let make: Option<i64> = self
            .conn
            .query_row("SELECT make FROM renders WHERE clip_id = ?1", [id], |r| r.get(0))
            .optional()?
            .flatten();
        Ok(self.files.clip_make(id, make.unwrap_or(0) + 1, ext))
    }

    /// What clip `id`'s file (or edit `id`'s video) was made with, if that's
    /// recorded (`set_made_as`).
    pub fn made_as(&self, id: i64) -> Result<Option<ClipEncoding>> {
        let made: Option<Option<String>> =
            self.conn.query_row("SELECT encoding FROM renders WHERE clip_id = ?1", [id], |r| r.get(0)).optional()?;
        Ok(made.flatten().and_then(|e| serde_json::from_str(&e).ok()))
    }

    /// Records that clip `id`'s file (or edit `id`'s video) was made with
    /// `encoding`: None for one copied from a file made before that was
    /// recorded.
    pub fn set_made_as(&self, id: i64, encoding: Option<ClipEncoding>) -> Result<()> {
        let encoding = encoding.map(|e| serde_json::to_string(&e)).transpose()?;
        self.conn.execute("UPDATE renders SET encoding = ?2 WHERE clip_id = ?1", params![id, encoding])?;
        Ok(())
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
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, vod_id, kind, at, text, {MARK_DETAIL_COLUMNS} FROM marks
             WHERE clip_id = (SELECT id FROM clips WHERE whole_of = ?1)
             ORDER BY at, id"
        ))?;
        let rows = stmt.query_map([vod_id], |r| {
            Ok(Mark {
                id: r.get(0)?,
                vod_id: r.get(1)?,
                kind: r.get(2)?,
                start: Flicks(r.get(3)?).seconds(),
                text: r.get(4)?,
                detail: mark_detail(r, 5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Puts a mark on VOD `vod_id`'s own clip at `start` seconds of the VOD,
    /// if that's within it (`clips::check_mark`).
    pub fn add_mark(&self, vod_id: i64, kind: &str, start: f64, text: Option<&str>) -> Result<Mark> {
        let clip = self.vod_clip(vod_id)?.context("That VOD is gone.")?;
        crate::clips::check_mark(start, self.vod_duration(vod_id)?).map_err(anyhow::Error::msg)?;
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
            "INSERT INTO marks (clip_id, vod_id, kind, at, text, end_at, category, note, source, people, icon, imported_as,
                created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
                detail.imported_as,
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

    /// Moves mark `id` to `start` seconds of its VOD, if that's within it
    /// (`clips::check_mark`); false if it's gone.
    pub fn move_mark(&self, id: i64, start: f64) -> Result<bool> {
        let vod = self.conn.query_row("SELECT vod_id FROM marks WHERE id = ?1", [id], |r| r.get(0)).optional()?;
        let Some(vod) = vod else { return Ok(false) };
        crate::clips::check_mark(start, self.vod_duration(vod)?).map_err(anyhow::Error::msg)?;
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
        let sql = "SELECT source_path, received_ext, video_ext FROM vods WHERE id = ?1";
        let row: Option<(Option<String>, Option<String>, Option<String>)> =
            self.conn.query_row(sql, [id], |r| Ok((r.get(0)?, ext_of(r.get(1)?), ext_of(r.get(2)?)))).optional()?;
        let Some((source, received, video)) = row else { return Ok(None) };
        let path = match (received, source, video) {
            (Some(ext), _, _) => Some(shown(&self.files.source_received(id, &ext))),
            (None, Some(source), _) => Some(source),
            (None, None, video) => video.map(|ext| shown(&self.files.source_video(id, &ext))),
        };
        Ok(path)
    }

    /// VOD `id`'s kept video (see `Vod::media`).
    pub fn media_path(&self, id: i64) -> Result<Option<String>> {
        let ext: Option<String> =
            self.conn.query_row("SELECT video_ext FROM vods WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten();
        Ok(ext_of(ext).map(|ext| shown(&self.files.source_video(id, &ext))))
    }

    /// Records VOD `id`'s kept video, at `path` (`Paths::source_video`), or
    /// that it has none.
    pub fn set_media_path(&self, id: i64, path: Option<&str>) -> Result<()> {
        let ext = path.map(|p| built_ext(p, |ext| self.files.source_video(id, ext))).transpose()?;
        // The full path too, for 0.7.1 opening this library (`library`).
        self.conn.execute("UPDATE vods SET video_ext = ?2, media_path = ?3 WHERE id = ?1", params![id, ext, path])?;
        Ok(())
    }

    /// VOD `id`'s whole audio: downloaded to transcribe it, or else the file
    /// it plays from (its own, or its kept video), which holds it.
    pub fn audio_path(&self, id: i64) -> Result<Option<String>> {
        let ext: Option<String> =
            self.conn.query_row("SELECT audio_ext FROM vods WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten();
        match ext_of(ext) {
            Some(ext) => Ok(Some(shown(&self.files.source_audio(id, &ext)))),
            None => self.playable_path(id),
        }
    }

    /// Records where VOD `id`'s file on this computer is: outside the
    /// library, the only path it keeps.
    pub fn set_source_path(&self, id: i64, path: &str) -> Result<()> {
        self.conn.execute("UPDATE vods SET source_path = ?2, received_ext = NULL WHERE id = ?1", params![id, path])?;
        Ok(())
    }

    /// Adds file `info`, just received from another app's drop at `info.url`
    /// (in `Paths::received`), as a new VOD, never one already there: the
    /// name it arrived under is free again once it's renamed for its VOD
    /// (`set_received`), and the next drop may be given it. So its identity
    /// is "received:<id>", which no file's path is, and its url the path
    /// it's kept at (`Paths::source_received`); till it's moved there it
    /// plays where it arrived (`source_path`).
    pub fn add_received(&self, info: &VodInfo) -> Result<Vod> {
        let arrived = info.url.clone();
        // Under an identity no row keeps past this transaction, so none is found.
        let info = VodInfo { platform_id: format!("received:{arrived}"), ..info.clone() };
        self.atomically(|db| {
            let (vod, _) = db.add_vod(&info)?;
            let ext = Path::new(&arrived).extension().map(|e| e.to_string_lossy().into_owned());
            let url = ext.map_or_else(|| arrived.clone(), |ext| shown(&db.files.source_received(vod.id, &ext)));
            db.conn.execute(
                "UPDATE vods SET platform_id = ?2, url = ?3, source_path = ?4, received_ext = NULL WHERE id = ?1",
                params![vod.id, format!("received:{}", vod.id), url, arrived],
            )?;
            Ok(db.vod(vod.id)?.expect("just added"))
        })
    }

    /// Records that VOD `id`'s file is the app's own copy, received from
    /// another app's drop, at `path` (`Paths::source_received`).
    pub fn set_received(&self, id: i64, path: &str) -> Result<()> {
        let ext = built_ext(path, |ext| self.files.source_received(id, ext))?;
        let sql = "UPDATE vods SET source_path = ?3, received_ext = ?2 WHERE id = ?1";
        self.conn.execute(sql, params![id, ext, path])?;
        Ok(())
    }

    /// Records VOD `id`'s whole audio at `path`: downloaded
    /// (`Paths::source_audio`), or its own file or kept video, which are
    /// known already and so not recorded (`audio_path` gives them); or that
    /// it has none downloaded.
    pub fn set_audio_path(&self, id: i64, path: Option<&str>) -> Result<()> {
        let own = match path {
            Some(p) => self.playable_path(id)?.as_deref() == Some(p),
            None => false,
        };
        let path = path.filter(|_| !own);
        let ext = path.map(|p| built_ext(p, |ext| self.files.source_audio(id, ext))).transpose()?;
        self.conn.execute("UPDATE vods SET audio_ext = ?2, audio_path = ?3 WHERE id = ?1", params![id, ext, path])?;
        Ok(())
    }

    /// Where the library's files are.
    pub fn files(&self) -> &crate::Paths {
        &self.files
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

    /// How many of VOD `vod_id`'s chat messages and transcript segments are
    /// left to erase (`delete_some_of`): how far an erasure is.
    pub fn erase_left(&self, vod_id: i64) -> Result<usize> {
        let count = |table: &str| -> Result<usize> {
            let sql = format!("SELECT count(*) FROM {table} WHERE vod_id = ?1");
            Ok(self.conn.query_row(&sql, [vod_id], |r| r.get::<_, i64>(0))? as usize)
        };
        Ok(count("chat")? + count("segments")?)
    }

    /// Rewrites both search indexes without the entries of rows deleted
    /// since (`tidy_index`), and empties the WAL of the old pages: a VOD's
    /// words gone from the files, with its row (`delete_vod`) or without.
    pub fn tidy_indexes(&self) -> Result<()> {
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
        self.tidy_indexes()
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
                tx.prepare(&format!("INSERT INTO segments ({SEGMENT_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"))?;
            for s in segments {
                // A transcription names no speaker.
                let speaker: Option<&str> = None;
                stmt.execute(params![vod_id, s.start, s.end, s.text, serde_json::to_string(&s.words)?, speaker])?;
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

    /// VOD `vod_id`'s channel's login and page, where the library has none
    /// yet (a VOD added before it kept them: `MIGRATE_V37`, `MIGRATE_V40`).
    pub fn fill_channel(&self, vod_id: i64, login: Option<&str>, page: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE vods SET channel_login = coalesce(channel_login, ?2), channel_url = coalesce(channel_url, ?3) WHERE id = ?1",
            params![vod_id, login, page],
        )?;
        Ok(())
    }

    /// VOD `vod_id`'s channel's login and page as its site names them now
    /// (`pipeline::refresh_details`): each one named replaces the one kept;
    /// one not named leaves it as it is, never guessed or cleared.
    pub fn refresh_channel(&self, vod_id: i64, login: Option<&str>, page: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE vods SET channel_login = coalesce(?2, channel_login), channel_url = coalesce(?3, channel_url) WHERE id = ?1",
            params![vod_id, login, page],
        )?;
        Ok(())
    }

    /// VOD `vod_id` started at `secs` (Unix seconds), by its chat
    /// (`chat::vod_start`): better than anything else says, so it replaces
    /// it (see `MIGRATE_V33`).
    pub fn set_chat_start(&self, vod_id: i64, secs: f64) -> Result<()> {
        self.conn.execute("UPDATE vods SET started_at = ?2 WHERE id = ?1", params![vod_id, utc_time(secs)])?;
        Ok(())
    }

    /// VOD `vod_id` started at `time` (RFC 3339), by an annotations file:
    /// kept only if nothing better said when (see `MIGRATE_V33`), and only a
    /// time that reads as one.
    pub fn set_start_if_unknown(&self, vod_id: i64, time: &str) -> Result<()> {
        let Some(secs) = crate::chat::parse_time(time) else { return Ok(()) };
        self.conn.execute(
            "UPDATE vods SET started_at = ?2 WHERE id = ?1 AND started_at IS NULL",
            params![vod_id, utc_time(secs)],
        )?;
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
                    at: None,
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

/// Every column of a transcript line, as each INSERT INTO segments names
/// them: the transcriber's, an import's and a kept stretch's copy all go
/// through this list, so a column added later can't be left out of one
/// (a kept stretch lost who was speaking that way).
pub(crate) const SEGMENT_COLUMNS: &str = "vod_id, start, end, text, words, speaker";

/// Every column of a chat message, as each INSERT INTO chat names them (see
/// `SEGMENT_COLUMNS`; a kept stretch's chat lost its subs, gifts and tips).
pub(crate) const CHAT_COLUMNS: &str = "vod_id, time, author, color, text, parts, kind, amount, event_time";

/// A VOD's chat in the library, as `chat::fetch` saves it. Holds the library's
/// lock only while it reads or writes, not while the fetch waits on the network.
/// Inserts `messages` as VOD `vod_id`'s, on `conn` (in a transaction the caller holds).
pub(crate) fn insert_chat(conn: &Connection, vod_id: i64, messages: &[ChatMessage]) -> Result<()> {
    let mut stmt =
        conn.prepare(&format!("INSERT INTO chat ({CHAT_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"))?;
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
    /// The VOD's start by each message saved (`ChatMessage::start`), for
    /// `chat::vod_start` once the fetch is done. Only this fetch's: one that
    /// carries on from an earlier one goes by the messages it fetched itself.
    pub starts: Vec<f64>,
}

impl crate::chat::ChatStore for LibraryChat<'_> {
    fn resume(&mut self) -> Result<Option<Vec<f64>>> {
        self.db.lock().unwrap().chat_slices(self.vod_id)
    }

    fn clear(&mut self) -> Result<()> {
        // A part at a time, letting go of the library between them, so a busy
        // chat fetched again doesn't hold everything else up.
        while self.db.lock().unwrap().clear_some_chat(self.vod_id, 1000)? > 0 {
            give_way();
        }
        self.starts.clear();
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
        self.starts.extend(messages.iter().filter_map(ChatMessage::start));
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
mod tests;
