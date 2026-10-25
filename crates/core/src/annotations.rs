//! Annotations: a VOD's transcript, chapters, highlights, cast and activity
//! made outside the app, as one JSON file per VOD (its shape is
//! apps/desktop/src/lib/annotations.ts), with its chat in a second file in
//! the app's own chat shape. Importing one makes them the VOD's: its
//! transcript, all of it transcribed; its chapters and highlights as marks
//! from the import (the user's own stay); its cast and activity; and its
//! chat, if given. Importing again replaces what the last import brought.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::chat::ChatMessage;
use crate::db::{Db, MarkDetail, Vod, now};
use crate::fetch::VodInfo;
use crate::time::Flicks;
use crate::transcript::Word;

/// The file's `format` field, and the version of its shape this reads.
pub const FORMAT: &str = "vodscribe-annotations";
pub const VERSION: u32 = 1;

/// The `source` of the marks an import brings.
pub const SOURCE: &str = "annotations";

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Annotations {
    pub format: String,
    pub version: u32,
    pub source: Source,
    #[serde(default)]
    pub produced: Option<serde_json::Value>,
    pub transcript: Transcript,
    pub chapters: Vec<Chapter>,
    pub highlights: Vec<Highlight>,
    pub cast: Vec<CastMember>,
    pub activity: Vec<Activity>,
}

/// The VOD the file is of.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Source {
    pub platform: String,
    /// The platform's own id, as the app stores it (Twitch's "v2906977264").
    pub id: String,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// Seconds.
    pub duration: f64,
    #[serde(default)]
    pub recorded_at: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Transcript {
    pub segments: Vec<AnnotatedSegment>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct AnnotatedSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
    #[serde(default)]
    pub words: Vec<Word>,
    /// A cast member's `name`.
    #[serde(default)]
    pub speaker: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Chapter {
    pub start: f64,
    pub title: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Highlight {
    pub start: f64,
    pub end: f64,
    pub title: String,
    pub kind: String,
    #[serde(default)]
    pub why: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct CastMember {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub role: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Activity {
    pub start: f64,
    pub end: f64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// A VOD's cast and activity, as an import left them.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct VodAnnotations {
    pub cast: Vec<CastMember>,
    pub activity: Vec<Activity>,
}

fn not_annotations(what: &str) -> anyhow::Error {
    anyhow::anyhow!("This isn't a VODscribe annotations file: {what}.")
}

/// Reads `json` as an annotations file, checking its format and version
/// first; the error's message is for the user (as `parseAnnotations` in the
/// page words it).
pub fn parse(json: &str) -> Result<Annotations> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|_| not_annotations("it isn't JSON"))?;
    from_value(value)
}

/// As `parse`, from JSON already read (the page's).
pub fn from_value(value: serde_json::Value) -> Result<Annotations> {
    let format = value.get("format");
    if format.and_then(|f| f.as_str()) != Some(FORMAT) {
        return Err(not_annotations(&format!("its format is {}", format.unwrap_or(&serde_json::Value::Null))));
    }
    let version = value.get("version");
    if version.and_then(|v| v.as_u64()) != Some(VERSION.into()) {
        let shown = version.map_or("none".to_string(), |v| v.to_string());
        return Err(not_annotations(&format!("version {shown} isn't {VERSION}")));
    }
    serde_json::from_value(value).map_err(|e| not_annotations(&e.to_string()))
}

/// The chat file that goes with annotations file `path`: `x.chat.json` beside `x.json`.
pub fn chat_path(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".json").filter(|s| !s.ends_with(".chat"))?;
    Some(path.with_file_name(format!("{stem}.chat.json")))
}

/// Reads annotations file `path`, and its chat: `chat` if given, else the
/// one beside it (`chat_path`) if it's there.
pub fn read(path: &Path, chat: Option<&Path>) -> Result<(Annotations, Option<Vec<ChatMessage>>)> {
    if path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".chat.json")) {
        bail!("That's the chat that goes with an annotations file: pick the annotations file beside it.");
    }
    let shown = crate::tools::shown(path);
    let json = std::fs::read_to_string(path).with_context(|| format!("read {shown}"))?;
    let annotations = parse(&json)?;
    let chat = match chat.map(Path::to_path_buf).or_else(|| chat_path(path).filter(|p| p.is_file())) {
        Some(chat) => {
            let shown = crate::tools::shown(&chat);
            let json = std::fs::read_to_string(&chat).with_context(|| format!("read {shown}"))?;
            Some(serde_json::from_str(&json).with_context(|| format!("{shown} isn't a chat VODscribe can read"))?)
        }
        None => None,
    };
    Ok((annotations, chat))
}

/// Where a VOD of `platform` with id `id` is watched, for a VOD an import
/// adds to the library; None for a platform it can't be added for that way.
fn watch_url(platform: &str, id: &str, channel: Option<&str>) -> Option<String> {
    match platform {
        "youtube" => Some(format!("https://www.youtube.com/watch?v={id}")),
        "twitch" => Some(format!("https://www.twitch.tv/videos/{}", id.strip_prefix('v').unwrap_or(id))),
        "kick" => channel.map(|c| format!("https://kick.com/{}/videos/{id}", c.to_lowercase())),
        _ => None,
    }
}

/// A time from the file that can stand as one: finite and not before the start.
fn usable(t: f64) -> bool {
    t.is_finite() && t >= 0.0
}

impl Db {
    /// Makes `annotations` VOD `source`'s (see the module's comment), with
    /// `chat` as its chat if given, all in one transaction: adding the VOD
    /// if the library doesn't have it (from a site; a file on this computer
    /// is added first). Returns the VOD, and whether it was added.
    pub fn import_annotations(&self, annotations: &Annotations, chat: Option<&[ChatMessage]>) -> Result<(Vod, bool)> {
        let source = &annotations.source;
        if !(source.duration.is_finite() && source.duration > 0.0) {
            bail!("The annotations file gives no length for its VOD.");
        }
        let (vod_id, created) = self.atomically(|db| {
            let found: Option<i64> = db
                .conn
                .query_row(
                    "SELECT id FROM vods WHERE platform = ?1 AND platform_id = ?2",
                    params![source.platform, source.id],
                    |r| r.get(0),
                )
                .optional()?;
            let (vod_id, created) = match found {
                Some(id) => {
                    // Taken out of the list but kept for clips: listed again.
                    let info = db.vod(id)?.context("just found")?.info;
                    db.add_vod(&info)?;
                    (id, false)
                }
                None => {
                    let url = watch_url(&source.platform, &source.id, source.channel.as_deref())
                        .with_context(|| match source.platform.as_str() {
                            crate::local::PLATFORM => "Add the video file first, then import its annotations.".into(),
                            "kick" => "The annotations file doesn't say the Kick VOD's channel.".to_string(),
                            other => format!("Annotations can only add a YouTube, Twitch or Kick VOD, not one from {other}: add the VOD first."),
                        })?;
                    let info = VodInfo {
                        platform: source.platform.clone(),
                        platform_id: source.id.clone(),
                        url,
                        title: source.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "Untitled".into()),
                        channel: source.channel.clone().filter(|c| !c.trim().is_empty()),
                        duration: Some(source.duration),
                        thumbnail: None,
                        // YYYY-MM-DD, from an ISO 8601 time.
                        published: source
                            .recorded_at
                            .as_deref()
                            .and_then(|t| t.get(..10))
                            .filter(|d| d.len() == 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-')
                            .map(str::to_string),
                    };
                    (db.add_vod(&info)?.0.id, true)
                }
            };
            if !db.requests(vod_id)?.is_empty() {
                bail!("That VOD is being transcribed, or waits to be: stop that first, then import its annotations.");
            }
            db.conn.execute(
                "UPDATE vods SET duration = ?2 WHERE id = ?1 AND duration IS NULL",
                params![vod_id, source.duration],
            )?;
            db.import_transcript(vod_id, annotations)?;
            db.import_marks(vod_id, annotations)?;
            db.conn.execute(
                "INSERT OR REPLACE INTO vod_annotations (vod_id, cast_members, activity, produced, imported_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    vod_id,
                    serde_json::to_string(&annotations.cast)?,
                    serde_json::to_string(&annotations.activity)?,
                    annotations.produced.as_ref().map(serde_json::to_string).transpose()?,
                    now()
                ],
            )?;
            if let Some(chat) = chat {
                db.clear_chat(vod_id)?;
                crate::db::insert_chat(&db.conn, vod_id, chat)?;
                db.finish_chat(vod_id)?;
            }
            Ok((vod_id, created))
        })?;
        // The replaced lines' and messages' words, out of the search indexes.
        self.tidy_index("segments_fts")?;
        if chat.is_some() {
            self.tidy_index("chat_fts")?;
        }
        Ok((self.vod(vod_id)?.context("just imported")?, created))
    }

    /// The file's lines become VOD `vod_id`'s transcript, all of it
    /// transcribed, by the imported "model".
    fn import_transcript(&self, vod_id: i64, annotations: &Annotations) -> Result<()> {
        self.conn.execute("DELETE FROM segments WHERE vod_id = ?1", [vod_id])?;
        let mut stmt = self.conn.prepare(
            "INSERT INTO segments (vod_id, start, end, text, words, speaker) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for s in &annotations.transcript.segments {
            if !usable(s.start) || !s.end.is_finite() || s.text.trim().is_empty() {
                continue;
            }
            let speaker = s.speaker.as_deref().map(str::trim).filter(|n| !n.is_empty());
            stmt.execute(params![
                vod_id,
                s.start,
                s.end.max(s.start),
                s.text,
                serde_json::to_string(&s.words)?,
                speaker
            ])?;
        }
        self.clear_stretches(vod_id)?;
        self.insert_done(vod_id, 0.0, None, Some(crate::model::IMPORTED_MODEL))?;
        Ok(())
    }

    /// The file's chapters and highlights become VOD `vod_id`'s marks from
    /// the import, in place of the last import's; the user's own stay.
    fn import_marks(&self, vod_id: i64, annotations: &Annotations) -> Result<()> {
        let clip = self.vod_clip(vod_id)?.context("That VOD is gone.")?;
        self.change(clip, None, |db| {
            db.conn.execute("DELETE FROM marks WHERE clip_id = ?1 AND source = ?2", params![clip, SOURCE])?;
            let from_import = || MarkDetail { source: SOURCE.into(), ..MarkDetail::default() };
            for c in annotations.chapters.iter().filter(|c| usable(c.start)) {
                let title = Some(c.title.trim()).filter(|t| !t.is_empty());
                db.insert_mark_with(clip, vod_id, "chapter", Flicks::from_seconds(c.start), title, &from_import())?;
            }
            for h in annotations.highlights.iter().filter(|h| usable(h.start)) {
                let detail = MarkDetail {
                    end: Some(h.end).filter(|e| e.is_finite() && *e > h.start),
                    category: Some(h.kind.trim().to_string()).filter(|k| !k.is_empty()),
                    note: Some(h.why.trim().to_string()).filter(|w| !w.is_empty()),
                    ..from_import()
                };
                let title = Some(h.title.trim()).filter(|t| !t.is_empty());
                db.insert_mark_with(clip, vod_id, "highlight", Flicks::from_seconds(h.start), title, &detail)?;
            }
            Ok(())
        })
    }

    /// VOD `vod_id`'s cast and activity, if annotations were imported for it.
    pub fn vod_annotations(&self, vod_id: i64) -> Result<Option<VodAnnotations>> {
        let row: Option<(String, String)> = self
            .conn
            .query_row("SELECT cast_members, activity FROM vod_annotations WHERE vod_id = ?1", [vod_id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        let Some((cast, activity)) = row else { return Ok(None) };
        Ok(Some(VodAnnotations { cast: serde_json::from_str(&cast)?, activity: serde_json::from_str(&activity)? }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{ChatPart, Kind};
    use crate::db::ChatStatus;
    use crate::stretches::Transcribed;

    fn file(platform: &str, id: &str) -> Annotations {
        let json = serde_json::json!({
            "format": FORMAT,
            "version": 1,
            "source": { "platform": platform, "id": id, "channel": "Marlowe", "title": "A stream", "duration": 60.0,
                        "recorded_at": "2026-10-01T18:00:00Z" },
            "produced": { "at": "2026-10-02T00:00:00Z", "build": "b1" },
            "transcript": { "corrected": 0, "segments": [
                { "id": 0, "start": 1.0, "end": 3.0, "text": "hello there", "speaker": "Marlowe",
                  "words": [{ "text": "hello", "start": 1.0, "end": 1.5 }, { "text": "there", "start": 1.6, "end": 3.0 }] },
                { "id": 1, "start": 10.0, "end": 12.0, "text": "general kenobi", "words": [] }
            ]},
            "chapters": [{ "start": 0.0, "title": "Start" }, { "start": 20.0, "title": "Middle" }],
            "highlights": [{ "start": 5.0, "end": 9.0, "title": "Big laugh", "kind": "funny", "why": "everyone loses it" }],
            "cast": [{ "name": "Marlowe", "aliases": ["Mar"], "role": "streamer" }],
            "activity": [{ "start": 0.0, "end": 30.0, "kind": "just chatting" }, { "start": 30.0, "end": 60.0, "kind": "game", "name": "Chess" }]
        });
        from_value(json).unwrap()
    }

    fn message(time: f64, text: &str) -> ChatMessage {
        ChatMessage {
            time,
            author: "Ada".into(),
            color: None,
            parts: vec![ChatPart::Text { text: text.into() }],
            kind: Kind::Message,
            amount: None,
            event_time: None,
        }
    }

    #[test]
    fn the_chat_is_read_from_beside_the_file() {
        let dir = std::env::temp_dir().join(format!("vodscribe-annotations-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v123.json");
        std::fs::write(&path, serde_json::to_string(&file("twitch", "v123")).unwrap()).unwrap();
        assert_eq!(chat_path(&path), Some(dir.join("v123.chat.json")));
        assert_eq!(read(&path, None).unwrap().1, None, "no chat beside it");
        let chat = r##"[{"id": 1, "time": 2.5, "author": "Ada", "color": "#ff0000",
                        "parts": [{"type": "text", "text": "hi "}, {"type": "emote", "name": "KEKW", "url": "https://x/1"}],
                        "kind": "message", "amount": null}]"##;
        std::fs::write(dir.join("v123.chat.json"), chat).unwrap();
        let (annotations, chat) = read(&path, None).unwrap();
        assert_eq!(annotations.source.id, "v123");
        let chat = chat.unwrap();
        assert_eq!((chat[0].time, chat[0].author.as_str(), chat[0].parts.len()), (2.5, "Ada", 2));
        let err = read(&dir.join("v123.chat.json"), None).unwrap_err().to_string();
        assert!(err.contains("pick the annotations file beside it"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_wrong_file_says_so() {
        let err = parse("{\"format\": \"other\"}").unwrap_err().to_string();
        assert_eq!(err, "This isn't a VODscribe annotations file: its format is \"other\".");
        let err = parse(&format!("{{\"format\": \"{FORMAT}\", \"version\": 2}}")).unwrap_err().to_string();
        assert_eq!(err, "This isn't a VODscribe annotations file: version 2 isn't 1.");
        assert!(parse("not json").unwrap_err().to_string().contains("it isn't JSON"));
        // The page's own reader takes the same file.
        let json = serde_json::to_string(&file("twitch", "v1")).unwrap();
        assert_eq!(parse(&json).unwrap(), file("twitch", "v1"));
    }

    #[test]
    fn an_import_adds_the_vod_transcribed_with_its_marks_cast_and_chat() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let chat = [message(2.0, "hi"), message(4.0, "KEKW")];
        let (vod, created) = db.import_annotations(&file("twitch", "v123"), Some(&chat)).unwrap();
        assert!(created);
        assert_eq!(vod.info.url, "https://www.twitch.tv/videos/123");
        assert_eq!(vod.info.duration, Some(60.0));
        assert_eq!(vod.info.published.as_deref(), Some("2026-10-01"));
        assert_eq!(vod.transcribed, Transcribed::All);
        assert!(!vod.outdated, "an imported transcript isn't from an older model");
        assert_eq!(vod.chat, ChatStatus::Done);
        let lines = db.lines(vod.id).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].speaker.as_deref(), Some("Marlowe"));
        assert_eq!(lines[1].speaker, None);
        assert_eq!(db.words_between(vod.id, 0.0, 60.0).unwrap().len(), 2);
        let marks = db.marks(vod.id).unwrap();
        let kinds: Vec<_> = marks.iter().map(|m| (m.kind.as_str(), m.start, m.text.as_deref())).collect();
        assert_eq!(
            kinds,
            [("chapter", 0.0, Some("Start")), ("highlight", 5.0, Some("Big laugh")), ("chapter", 20.0, Some("Middle"))]
        );
        let h = &marks[1].detail;
        assert_eq!(
            (h.end, h.category.as_deref(), h.note.as_deref()),
            (Some(9.0), Some("funny"), Some("everyone loses it"))
        );
        assert!(marks.iter().all(|m| m.detail.source == SOURCE));
        let extra = db.vod_annotations(vod.id).unwrap().unwrap();
        assert_eq!(extra.cast[0].aliases, ["Mar"]);
        assert_eq!(extra.activity[1].name.as_deref(), Some("Chess"));
        assert_eq!(db.chat(vod.id).unwrap().len(), 2);
        let found = db.search("general", &Default::default(), 10).unwrap();
        assert_eq!(found.hits.len(), 1, "imported lines are searched");
    }

    #[test]
    fn importing_again_replaces_the_import_and_keeps_the_users_marks() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let (vod, _) = db.import_annotations(&file("youtube", "abc"), None).unwrap();
        assert_eq!(vod.chat, ChatStatus::Pending, "with no chat file, the site's chat is fetched as for any VOD");
        let mine = db.add_mark(vod.id, "highlight", 30.0, Some("mine")).unwrap();
        // An imported chapter the user renames is theirs from then on.
        let middle = db.marks(vod.id).unwrap().into_iter().find(|m| m.text.as_deref() == Some("Middle")).unwrap();
        db.set_mark_text(middle.id, Some("My middle")).unwrap();

        let mut next = file("youtube", "abc");
        next.chapters = vec![Chapter { start: 0.0, title: "Opening".into() }];
        next.highlights.clear();
        next.cast.push(CastMember { name: "Ada".into(), aliases: vec![], role: "chatter".into() });
        next.transcript.segments.truncate(1);
        let (again, created) = db.import_annotations(&next, Some(&[message(1.0, "first")])).unwrap();
        assert_eq!((again.id, created), (vod.id, false));
        let texts: Vec<_> = db.marks(vod.id).unwrap().into_iter().map(|m| (m.text.unwrap(), m.detail.source)).collect();
        assert_eq!(
            texts,
            [("Opening".into(), SOURCE.into()), ("My middle".into(), "user".into()), ("mine".into(), "user".into())]
        );
        assert!(db.marks(vod.id).unwrap().iter().any(|m| m.id == mine.id));
        assert_eq!(db.lines(vod.id).unwrap().len(), 1);
        assert_eq!(db.stretches(vod.id).unwrap().len(), 1, "one done stretch, the whole VOD");
        assert_eq!(db.vod_annotations(vod.id).unwrap().unwrap().cast.len(), 2);
        assert_eq!(db.chat(vod.id).unwrap().len(), 1, "the new chat in place of none");
    }

    #[test]
    fn undo_puts_imported_marks_back_as_imported() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let (vod, _) = db.import_annotations(&file("youtube", "abc"), None).unwrap();
        let before = db.mark_states(vod.id).unwrap();
        db.add_mark(vod.id, "cut", 3.0, None).unwrap();
        db.restore_marks(vod.id, &before).unwrap();
        assert_eq!(db.mark_states(vod.id).unwrap(), before);
        // So the next import still replaces them.
        db.import_annotations(&file("youtube", "abc"), None).unwrap();
        assert_eq!(db.marks(vod.id).unwrap().len(), 3);
    }

    #[test]
    fn an_import_onto_a_vod_already_there_or_one_that_cannot_be_added() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let err = db.import_annotations(&file("local", "/videos/a.mp4"), None).unwrap_err();
        assert_eq!(err.to_string(), "Add the video file first, then import its annotations.");
        let err = db.import_annotations(&file("vimeo", "1"), None).unwrap_err();
        assert!(err.to_string().contains("not one from vimeo"));
        assert!(db.vods().unwrap().is_empty(), "nothing added");

        let (vod, _) = db.add_vod(&crate::edits::tests::info("abc")).unwrap();
        db.request(vod.id, 0.0, None).unwrap();
        let err = db.import_annotations(&file("youtube", "abc"), None).unwrap_err();
        assert!(err.to_string().starts_with("That VOD is being transcribed"));
        assert!(db.marks(vod.id).unwrap().is_empty(), "nothing imported");
        db.clear_stretches(vod.id).unwrap();
        let (imported, created) = db.import_annotations(&file("youtube", "abc"), None).unwrap();
        assert_eq!((imported.id, created), (vod.id, false));
        assert_eq!(imported.info.title, "VOD abc", "the library's title stays");
        assert_eq!(imported.transcribed, Transcribed::All);
    }
}
