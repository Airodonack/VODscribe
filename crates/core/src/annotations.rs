//! Annotations: a VOD's transcript, chapters, highlights, cast and activity
//! made outside the app, as one JSON file per VOD (its shape is
//! apps/desktop/src/lib/annotations.ts), with its chat in a second file in
//! the app's own chat shape. Importing one makes them the VOD's: its
//! transcript, all of it transcribed; its chapters and highlights as marks
//! from the import (the user's own stay); its cast and activity; and its
//! chat, if given. Importing again replaces what the last import brought.
//!
//! Annotations can also be published, one file per VOD, at
//! `<BASE_URL>/annotations/<platform>/<platform id>.json`: `look_up` asks
//! whether a VOD's is there (one HEAD request naming the site and the VOD's
//! id, nothing else), and `download` fetches it to import.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::chat::ChatMessage;
use crate::db::{Availability, Db, MarkDetail, Vod, now};
use crate::fetch::VodInfo;
use crate::time::Flicks;
use crate::transcript::Word;

/// The file's `format` field, and the version of its shape this reads.
pub const FORMAT: &str = "vodscribe-annotations";
pub const VERSION: u32 = 1;

/// The `source` of the marks an import brings.
pub const SOURCE: &str = "annotations";

/// Where published annotations are served from.
pub const BASE_URL: &str = "https://dl.vodscribe.com";
/// Names another server to ask instead of `BASE_URL` (the end-to-end tests'
/// own, on this computer, over plain HTTP).
pub const BASE_ENV: &str = "VODSCRIBE_ANNOTATIONS_URL";
/// The largest published file read: a long VOD's is a few megabytes.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

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
    /// What it's about, as one emoji; shown as given (`one_emoji` keeps only that).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Highlight {
    pub start: f64,
    pub end: f64,
    pub title: String,
    pub kind: String,
    #[serde(default)]
    pub why: String,
    /// Who is in it: cast members' names, or their aliases.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cast: Vec<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct CastMember {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub role: String,
    /// The streamer behind a character, by their public name; one that isn't
    /// a name is left out rather than the file refused.
    #[serde(default, deserialize_with = "player", skip_serializing_if = "Option::is_none")]
    pub player: Option<String>,
    /// Where they stream; one `Account::read` can't take is left out.
    #[serde(default, deserialize_with = "account", skip_serializing_if = "Option::is_none")]
    pub account: Option<Account>,
}

/// A cast member's account: the site and their login there (Twitch's and
/// Kick's channel name, YouTube's "@handle" or channel id). Never a picture's
/// address: the page asks for pictures when it shows them (accounts.ts).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Account {
    pub platform: String,
    pub login: String,
}

/// The sites an account can be on.
pub const ACCOUNT_PLATFORMS: [&str; 3] = ["twitch", "kick", "youtube"];

impl Account {
    /// `value` as an account: a listed platform (any case) and a login of
    /// letters, digits and `_.-` (a YouTube handle with its "@" first),
    /// trimmed; None for anything else. The page's `readAccount`
    /// (annotations.ts) is the same rule.
    pub fn read(value: &serde_json::Value) -> Option<Account> {
        let platform = value.get("platform")?.as_str()?.trim().to_lowercase();
        let login = value.get("login")?.as_str()?.trim();
        if !ACCOUNT_PLATFORMS.contains(&platform.as_str()) {
            return None;
        }
        let rest = if platform == "youtube" { login.strip_prefix('@').unwrap_or(login) } else { login };
        let fits = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-');
        if rest.is_empty() || !rest.chars().all(fits) {
            return None;
        }
        Some(Account { platform, login: login.to_string() })
    }
}

fn player<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(value.as_str().map(str::trim).filter(|p| !p.is_empty()).map(Into::into))
}

fn account<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Account>, D::Error> {
    Ok(Account::read(&serde_json::Value::deserialize(d)?))
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
/// `icon` as a chapter's icon: one emoji (a single character as the reader
/// sees it, of no letter or digit and not plain ASCII), trimmed; None for
/// anything else, which an import leaves out rather than refusing the file.
/// The page's `oneEmoji` (annotations.ts) is the same rule.
pub fn one_emoji(icon: &str) -> Option<&str> {
    use unicode_segmentation::UnicodeSegmentation;
    let s = icon.trim();
    let plain = |c: char| c.is_alphanumeric() || c.is_control() || c.is_whitespace();
    if s.is_empty() || s.is_ascii() || s.chars().any(plain) || s.graphemes(true).count() != 1 {
        return None;
    }
    Some(s)
}

fn usable(t: f64) -> bool {
    t.is_finite() && t >= 0.0
}

/// A server named by environment variable `name`, without a trailing slash.
fn named_server(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|b| b.trim().trim_end_matches('/').to_string()).filter(|b| !b.is_empty())
}

/// The server published annotations are asked of: `BASE_ENV`'s, if set.
fn base_url() -> String {
    named_server(BASE_ENV).unwrap_or_else(|| BASE_URL.to_string())
}

/// Names another server for the cast's pictures than the page's own
/// (accounts.ts `AVATAR_BASE`): the end-to-end tests', on this computer.
pub const AVATAR_ENV: &str = "VODSCRIBE_AVATAR_URL";

/// The server the cast's pictures are asked of when `AVATAR_ENV` names one;
/// None for the page's own.
pub fn avatar_base() -> Option<String> {
    named_server(AVATAR_ENV)
}

/// Where VOD `id` of `platform`'s published annotations would be on server
/// `base`: each part with anything but letters, digits and `-._~` escaped,
/// so an id can't reach another path.
pub fn file_url(base: &str, platform: &str, id: &str) -> String {
    let escape = |part: &str| -> String {
        part.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect()
    };
    format!("{base}/annotations/{}/{}.json", escape(platform), escape(id))
}

/// Whether VODs of `platform` can have published annotations: those from a
/// site, not a file on this computer (its id is this computer's own).
pub fn can_be_published(platform: &str) -> bool {
    platform != crate::local::PLATFORM
}

/// The client for the file host: quick to give up, since nothing waits on
/// it but an offer. Plain HTTP only for a server named by `BASE_ENV`.
fn agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_connect(Some(std::time::Duration::from_secs(10)))
            .timeout_recv_response(Some(std::time::Duration::from_secs(20)))
            .timeout_recv_body(Some(std::time::Duration::from_secs(120)))
            .https_only(std::env::var_os(BASE_ENV).is_none() && !cfg!(test))
            .build()
            .into()
    })
}

/// What the file host said of a VOD's annotations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    None,
    /// There, in the version given (its ETag, or else its Last-Modified), if the server says.
    Available {
        version: Option<String>,
    },
}

/// The file's version, from a response's headers: its ETag, or else its Last-Modified.
fn version_of<B>(response: &ureq::http::Response<B>) -> Option<String> {
    let header = |name: &str| {
        response.headers().get(name).and_then(|v| v.to_str().ok()).map(str::trim).filter(|v| !v.is_empty())
    };
    header("etag").or_else(|| header("last-modified")).map(str::to_string)
}

/// Asks the file host whether VOD `id` of `platform` has published
/// annotations: one HEAD request. Not there (404, 410, 403) is `None`; a
/// failure to ask (offline, a server error) is an error, and tells nothing.
pub fn look_up(platform: &str, id: &str) -> Result<Found> {
    look_up_at(&base_url(), platform, id)
}

fn look_up_at(base: &str, platform: &str, id: &str) -> Result<Found> {
    match agent().head(file_url(base, platform, id)).call() {
        Ok(response) if response.status() == 200 => Ok(Found::Available { version: version_of(&response) }),
        Ok(response) => bail!("the file host answered {}", response.status()),
        Err(ureq::Error::StatusCode(403 | 404 | 410)) => Ok(Found::None),
        Err(e) => Err(e).context("ask the file host"),
    }
}

/// Fetches VOD `id` of `platform`'s published annotations, and the file's
/// version; fails if they're of another VOD.
pub fn download(platform: &str, id: &str) -> Result<(Annotations, Option<String>)> {
    download_from(&base_url(), platform, id)
}

fn download_from(base: &str, platform: &str, id: &str) -> Result<(Annotations, Option<String>)> {
    let mut response = match agent().get(file_url(base, platform, id)).call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(403 | 404 | 410)) => {
            bail!("The annotations for this VOD aren't there any more.")
        }
        Err(e) => return Err(e).context("Couldn't get the annotations for this VOD"),
    };
    let version = version_of(&response);
    let json = response
        .body_mut()
        .with_config()
        .limit(MAX_FILE_BYTES)
        .read_to_string()
        .context("Couldn't get the annotations for this VOD")?;
    let annotations = parse(&json)?;
    if annotations.source.platform != platform || annotations.source.id != id {
        bail!("The annotations found are of another VOD.");
    }
    Ok((annotations, version))
}

/// What the library knows of a VOD's published annotations, for offering them.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Offer {
    pub available: Availability,
    /// The version published is the one it last brought in.
    pub up_to_date: bool,
    /// It has a transcript made on this computer, which an import replaces.
    pub local_transcript: bool,
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
                let detail =
                    MarkDetail { icon: c.icon.as_deref().and_then(one_emoji).map(Into::into), ..from_import() };
                db.insert_mark_with(clip, vod_id, "chapter", Flicks::from_seconds(c.start), title, &detail)?;
            }
            for h in annotations.highlights.iter().filter(|h| usable(h.start)) {
                let detail = MarkDetail {
                    end: Some(h.end).filter(|e| e.is_finite() && *e > h.start),
                    category: Some(h.kind.trim().to_string()).filter(|k| !k.is_empty()),
                    note: Some(h.why.trim().to_string()).filter(|w| !w.is_empty()),
                    cast: h.cast.iter().map(|n| n.trim()).filter(|n| !n.is_empty()).map(Into::into).collect(),
                    ..from_import()
                };
                let title = Some(h.title.trim()).filter(|t| !t.is_empty());
                db.insert_mark_with(clip, vod_id, "highlight", Flicks::from_seconds(h.start), title, &detail)?;
            }
            Ok(())
        })
    }

    /// Keeps what the file host said of VOD `vod_id`'s annotations.
    pub fn set_annotations_found(&self, vod_id: i64, found: &Found) -> Result<()> {
        let (availability, version) = match found {
            Found::None => (Availability::None, None),
            Found::Available { version } => (Availability::Available, version.as_deref()),
        };
        self.conn.execute(
            "UPDATE vods SET annotations = ?2, annotations_version = ?3 WHERE id = ?1",
            params![vod_id, availability.as_str(), version],
        )?;
        Ok(())
    }

    /// VOD `vod_id` brought in its published annotations, in version `version`.
    pub fn set_annotations_imported(&self, vod_id: i64, version: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE vods SET annotations = ?2, annotations_version = ?3, annotations_imported = ?3 WHERE id = ?1",
            params![vod_id, Availability::Available.as_str(), version],
        )?;
        Ok(())
    }

    /// What's known of VOD `vod_id`'s published annotations (`Offer`).
    pub fn annotations_offer(&self, vod_id: i64) -> Result<Offer> {
        let (available, version, imported, local): (Option<String>, Option<String>, Option<String>, bool) =
            self.conn.query_row(
                "SELECT annotations, annotations_version, annotations_imported,
                     EXISTS (SELECT 1 FROM segments WHERE vod_id = ?1)
                     AND EXISTS (SELECT 1 FROM stretches WHERE vod_id = ?1 AND model IS NOT ?2)
                 FROM vods WHERE id = ?1",
                params![vod_id, crate::model::IMPORTED_MODEL],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
        Ok(Offer {
            available: Availability::parse(available.as_deref()),
            // Without a version from the server, there's no telling: never up to date.
            up_to_date: version.is_some() && version == imported,
            local_transcript: local,
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
            "chapters": [{ "start": 0.0, "title": "Start", "icon": " 🎮" }, { "start": 20.0, "title": "Middle", "icon": "a game" }],
            "highlights": [{ "start": 5.0, "end": 9.0, "title": "Big laugh", "kind": "funny", "why": "everyone loses it",
                              "cast": ["Marlowe", " ", "Ada "] }],
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
    fn a_chapter_icon_is_one_emoji_or_none() {
        for icon in ["🎮", " 🍳 ", "👩‍🍳", "🏳️‍🌈", "❤️", "🇫🇷"] {
            assert_eq!(one_emoji(icon), Some(icon.trim()), "{icon}");
        }
        for icon in ["", " ", "🎮🎮", "a", "🎮 game", "Game", "7", "?", ":)", "日"] {
            assert_eq!(one_emoji(icon), None, "{icon}");
        }
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
        assert_eq!(h.cast, ["Marlowe", "Ada"], "names trimmed, blanks left out");
        assert!(marks[0].detail.cast.is_empty(), "a chapter has no cast");
        assert_eq!(marks[0].detail.icon.as_deref(), Some("🎮"), "an icon, trimmed");
        assert_eq!(marks[2].detail.icon, None, "an icon that isn't one emoji is left out");
        assert!(marks.iter().all(|m| m.detail.source == SOURCE));
        let extra = db.vod_annotations(vod.id).unwrap().unwrap();
        assert_eq!(extra.cast[0].aliases, ["Mar"]);
        assert_eq!(extra.activity[1].name.as_deref(), Some("Chess"));
        assert_eq!(db.chat(vod.id).unwrap().len(), 2);
        let found = db.search("general", &Default::default(), 10).unwrap();
        assert_eq!(found.hits.len(), 1, "imported lines are searched");
    }

    #[test]
    fn a_cast_members_player_and_account_are_read_and_kept_through_an_import() {
        let mut json = serde_json::to_value(file("youtube", "abc")).unwrap();
        json["cast"] = serde_json::json!([
            { "name": "Marlowe", "role": "streamer", "player": " Marlowe Plays ",
              "account": { "platform": "Twitch", "login": " marlowe_plays " } },
            { "name": "Ada", "role": "friend", "account": { "platform": "youtube", "login": "@ada.makes" } },
            { "name": "Bo", "role": "friend", "player": 3, "account": { "platform": "vimeo", "login": "bo" } },
            { "name": "Cy", "role": "friend", "player": "", "account": { "platform": "kick", "login": "../cy" } },
            { "name": "Di", "role": "friend", "account": { "platform": "kick", "login": "@di" } },
            { "name": "Ed", "role": "friend", "account": "ed" }
        ]);
        let annotations = from_value(json).expect("a player or account that can't be read isn't fatal");
        let cast = &annotations.cast;
        assert_eq!(cast[0].player.as_deref(), Some("Marlowe Plays"), "trimmed");
        let twitch = Account { platform: "twitch".into(), login: "marlowe_plays".into() };
        assert_eq!(cast[0].account.as_ref(), Some(&twitch), "the platform in lower case, the login trimmed");
        assert_eq!(cast[1].account.as_ref().map(|a| a.login.as_str()), Some("@ada.makes"), "a YouTube handle");
        assert_eq!((cast[2].player.as_deref(), cast[2].account.as_ref()), (None, None), "an unknown platform");
        assert_eq!((cast[3].player.as_deref(), cast[3].account.as_ref()), (None, None), "a login that's a path");
        assert_eq!(cast[4].account, None, "an @ is YouTube's only");
        assert_eq!(cast[5].account, None);

        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let (vod, _) = db.import_annotations(&annotations, None).unwrap();
        let kept = db.vod_annotations(vod.id).unwrap().unwrap().cast;
        assert_eq!(kept, annotations.cast, "the import keeps them as read");
        let stored = serde_json::to_value(&kept).unwrap();
        assert_eq!(
            stored[1],
            serde_json::json!({ "name": "Ada", "aliases": [], "role": "friend",
            "account": { "platform": "youtube", "login": "@ada.makes" } }),
            "and leaves out what's not there"
        );
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
        next.chapters = vec![Chapter { start: 0.0, title: "Opening".into(), icon: None }];
        next.highlights.clear();
        next.cast.push(CastMember {
            name: "Ada".into(),
            aliases: vec![],
            role: "chatter".into(),
            player: None,
            account: None,
        });
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
        assert!(before.iter().any(|m| m.detail.cast == ["Marlowe", "Ada"]), "with the highlight's cast");
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

    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A file host on this computer answering each request with `answer`
    /// (status line and headers, and a body) for its request line, as many
    /// as come; its address, and the request lines it was sent.
    fn host(answer: fn(&str) -> (String, String)) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let seen = asked.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { return };
                let mut req = [0u8; 4096];
                let n = conn.read(&mut req).unwrap_or(0);
                let line = String::from_utf8_lossy(&req[..n]).lines().next().unwrap_or("").to_string();
                let (head, body) = answer(&line);
                seen.lock().unwrap().push(line.clone());
                let body = if line.starts_with("HEAD") { String::new() } else { body };
                let _ = write!(conn, "{head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        (format!("http://{addr}"), asked)
    }

    #[test]
    fn a_vods_file_is_at_its_site_and_id_escaped() {
        assert_eq!(
            file_url(BASE_URL, "twitch", "v2901646294"),
            "https://dl.vodscribe.com/annotations/twitch/v2901646294.json"
        );
        assert_eq!(file_url("http://h", "youtube", "a/../b?c"), "http://h/annotations/youtube/a%2F..%2Fb%3Fc.json");
        assert!(can_be_published("kick") && !can_be_published(crate::local::PLATFORM));
    }

    #[test]
    fn looking_up_says_there_not_there_or_fails() {
        let (base, asked) = host(|line| match line {
            l if l.contains("/annotations/youtube/there.json") => {
                ("HTTP/1.1 200 OK\r\nETag: \"v1\"".into(), String::new())
            }
            l if l.contains("/dated.json") => {
                ("HTTP/1.1 200 OK\r\nLast-Modified: Sat, 24 Oct 2026 10:00:00 GMT".into(), String::new())
            }
            l if l.contains("/broken.json") => ("HTTP/1.1 500 Internal Server Error".into(), String::new()),
            _ => ("HTTP/1.1 404 Not Found".into(), String::new()),
        });
        assert_eq!(look_up_at(&base, "youtube", "there").unwrap(), Found::Available { version: Some("\"v1\"".into()) });
        let dated = look_up_at(&base, "youtube", "dated").unwrap();
        assert_eq!(dated, Found::Available { version: Some("Sat, 24 Oct 2026 10:00:00 GMT".into()) });
        assert_eq!(look_up_at(&base, "twitch", "v1").unwrap(), Found::None);
        assert!(look_up_at(&base, "youtube", "broken").is_err());
        let asked = asked.lock().unwrap().clone();
        assert!(asked.iter().all(|l| l.starts_with("HEAD ")), "only asked, never fetched: {asked:?}");
        assert_eq!(asked[2], "HEAD /annotations/twitch/v1.json HTTP/1.1");
        // Nothing listening: a failure, which tells nothing.
        let gone = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        assert!(look_up_at(&format!("http://{gone}"), "youtube", "there").is_err());
    }

    #[test]
    fn a_download_is_the_vods_own_file_with_its_version() {
        let (base, _) = host(|line| {
            let platform = if line.contains("/youtube/") { "youtube" } else { "twitch" };
            let json = serde_json::to_string(&file(platform, "abc")).unwrap();
            ("HTTP/1.1 200 OK\r\nETag: \"v2\"".into(), json)
        });
        let (annotations, version) = download_from(&base, "youtube", "abc").unwrap();
        assert_eq!((annotations.source.id.as_str(), version.as_deref()), ("abc", Some("\"v2\"")));
        // A file whose source is another VOD isn't this one's.
        let err = download_from(&base, "youtube", "xyz").unwrap_err();
        assert_eq!(err.to_string(), "The annotations found are of another VOD.");
        let (_, _) = download_from(&base, "twitch", "abc").unwrap();
        let (gone, _) = host(|_| ("HTTP/1.1 404 Not Found".into(), String::new()));
        let err = download_from(&gone, "youtube", "abc").unwrap_err();
        assert_eq!(err.to_string(), "The annotations for this VOD aren't there any more.");
    }

    #[test]
    fn what_is_found_is_kept_and_an_import_of_it_is_up_to_date_till_it_changes() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let (vod, _) = db.add_vod(&crate::edits::tests::info("abc")).unwrap();
        assert_eq!(vod.annotations, Availability::Unknown);
        let offer = db.annotations_offer(vod.id).unwrap();
        assert_eq!(offer, Offer { available: Availability::Unknown, up_to_date: false, local_transcript: false });

        db.set_annotations_found(vod.id, &Found::None).unwrap();
        assert_eq!(db.vod(vod.id).unwrap().unwrap().annotations, Availability::None);
        let v1 = Found::Available { version: Some("v1".into()) };
        db.set_annotations_found(vod.id, &v1).unwrap();
        assert_eq!(db.vod(vod.id).unwrap().unwrap().annotations, Availability::Available);
        assert!(!db.annotations_offer(vod.id).unwrap().up_to_date, "found, not brought in");

        db.import_annotations(&file("youtube", "abc"), None).unwrap();
        db.set_annotations_imported(vod.id, Some("v1")).unwrap();
        let offer = db.annotations_offer(vod.id).unwrap();
        assert_eq!(offer, Offer { available: Availability::Available, up_to_date: true, local_transcript: false });
        // Asked again: the same file is still up to date; a new one isn't.
        db.set_annotations_found(vod.id, &v1).unwrap();
        assert!(db.annotations_offer(vod.id).unwrap().up_to_date);
        db.set_annotations_found(vod.id, &Found::Available { version: Some("v2".into()) }).unwrap();
        assert!(!db.annotations_offer(vod.id).unwrap().up_to_date);
        // A server that gives no version can't be told up to date.
        db.set_annotations_imported(vod.id, None).unwrap();
        assert!(!db.annotations_offer(vod.id).unwrap().up_to_date);
    }

    #[test]
    fn a_transcript_made_here_is_told_from_an_imported_one() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let (vod, _) = db.add_vod(&crate::edits::tests::info("abc")).unwrap();
        db.conn
            .execute("INSERT INTO segments (vod_id, start, end, text, words) VALUES (?1, 0, 1, 'hi', '[]')", [vod.id])
            .unwrap();
        db.insert_done(vod.id, 0.0, None, Some(crate::model::CPU_MODEL.id)).unwrap();
        assert!(db.annotations_offer(vod.id).unwrap().local_transcript);
        db.import_annotations(&file("youtube", "abc"), None).unwrap();
        assert!(!db.annotations_offer(vod.id).unwrap().local_transcript, "replaced by the import's");
    }
}
