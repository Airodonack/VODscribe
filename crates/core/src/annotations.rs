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
//!
//! The people in a cast have streamer cards on the same server, one per
//! person (apps/desktop/src/lib/streamers.ts has their shape), at
//! `<BASE_URL>/streamers/<id>.json`, with an index of their accounts at
//! `<BASE_URL>/streamers.json`: `streamer_card` and `streamer_index` fetch
//! them for the page.

use std::collections::HashSet;
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

// The most a file may bring: well past what the longest VOD's annotations
// have, short of what would hold the app up (an import is one transaction
// with the library locked, and the page draws a row and a mark for each).
// The page's reader (annotations.ts) has the same limits.
/// Transcript lines.
pub const MAX_SEGMENTS: usize = 200_000;
/// A transcript line's words.
pub const MAX_WORDS: usize = 2_000;
pub const MAX_CHAPTERS: usize = 2_000;
pub const MAX_HIGHLIGHTS: usize = 5_000;
/// The people in the cast.
pub const MAX_CAST: usize = 1_000;
/// A cast member's other names, and the people a highlight names.
pub const MAX_NAMES: usize = 100;
/// Stretches of activity (a game, just chatting, an event).
pub const MAX_ACTIVITY: usize = 10_000;
/// Other points of view.
pub const MAX_POVS: usize = 1_000;
/// The contexts a VOD names (`contexts`).
pub const MAX_CONTEXTS: usize = 100;
/// Any one text in a file (a title, a line, a name), in characters.
pub const MAX_TEXT: usize = 10_000;
/// A chat file's messages.
pub const MAX_MESSAGES: usize = 2_000_000;
/// One chat message's parts (its text and emotes).
pub const MAX_PARTS: usize = 1_000;

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
    /// Other points of view: cast members' own VODs that ran alongside this
    /// one. Optional; one `Pov::read` can't take is left out.
    #[serde(default, deserialize_with = "povs", skip_serializing_if = "Vec::is_empty")]
    pub povs: Vec<Pov>,
    /// What the VOD is part of, by the ids streamer cards name their
    /// sections by ("nopixel"): a card shows a section only when the VOD it's
    /// opened in names it. Optional; one `context_id` can't take is left out.
    #[serde(default, deserialize_with = "contexts", skip_serializing_if = "Vec::is_empty")]
    pub contexts: Vec<String>,
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
    /// When the VOD started (its time 0), RFC 3339: an import keeps it as
    /// the VOD's `started_at` when nothing better says (`db::MIGRATE_V33`).
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
    /// The person's streamer card, by the id it's published under (the
    /// player's, for a character): `streamer_card`. One `streamer_id` can't
    /// take is left out.
    #[serde(default, deserialize_with = "streamer", skip_serializing_if = "Option::is_none")]
    pub streamer: Option<String>,
}

/// `id` as a streamer card's id or a context's: lowercase letters, digits,
/// `_` and `-`, a letter or digit first, at most 64; trimmed; None for
/// anything else. It names a file on the file host, so nothing else gets
/// into the path. The page's `cardId` (streamers.ts) is the same rule.
pub fn card_id(id: &str) -> Option<&str> {
    let id = id.trim();
    let fits = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-');
    let first = id.bytes().next()?;
    (id.len() <= 64 && (first.is_ascii_lowercase() || first.is_ascii_digit()) && id.bytes().all(fits)).then_some(id)
}

fn streamer<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(value.as_str().and_then(card_id).map(Into::into))
}

fn contexts<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    let mut out: Vec<String> = Vec::new();
    for id in value.as_array().into_iter().flatten().filter_map(|v| v.as_str()).filter_map(card_id) {
        if !out.iter().any(|o| o == id) {
            out.push(id.into());
        }
    }
    Ok(out)
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
    /// letters, digits and `_.-`, not only dots (which a path would read as
    /// itself or its parent), a YouTube handle with its "@" first; trimmed;
    /// None for anything else. The page's `readAccount` (annotations.ts) is
    /// the same rule.
    pub fn read(value: &serde_json::Value) -> Option<Account> {
        let platform = value.get("platform")?.as_str()?.trim().to_lowercase();
        let login = value.get("login")?.as_str()?.trim();
        if !ACCOUNT_PLATFORMS.contains(&platform.as_str()) {
            return None;
        }
        let rest = if platform == "youtube" { login.strip_prefix('@').unwrap_or(login) } else { login };
        let fits = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-');
        if rest.is_empty() || !rest.chars().all(fits) || rest.chars().all(|c| c == '.') {
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

/// Another streamer's VOD that ran alongside this one: whose (`player`, by
/// their public name, and `account`), which video, when it started (RFC
/// 3339) and how long it is (seconds). A moment `t` seconds into this VOD is
/// `started_at + t - start` seconds into it.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Pov {
    pub player: String,
    pub account: Account,
    pub video: Video,
    pub start: String,
    pub duration: f64,
}

/// A video on a site: the platform and its own id there (Twitch's "v2906990586").
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Video {
    pub platform: String,
    pub id: String,
}

impl Pov {
    /// `value` as a point of view: a player's name, an account
    /// `Account::read` takes, a video on a listed platform with an id of
    /// letters, digits and `_-`, a start that reads as a time and a length
    /// that is one; trimmed; None for anything else. The page's `readPov`
    /// (annotations.ts) is the same rule.
    pub fn read(value: &serde_json::Value) -> Option<Pov> {
        let player = value.get("player")?.as_str()?.trim();
        let account = Account::read(value.get("account")?)?;
        let video = value.get("video")?;
        let platform = video.get("platform")?.as_str()?.trim().to_lowercase();
        let id = video.get("id")?.as_str()?.trim();
        let start = value.get("start")?.as_str()?.trim();
        let duration = value.get("duration")?.as_f64()?;
        let fits = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-');
        if player.is_empty()
            || !ACCOUNT_PLATFORMS.contains(&platform.as_str())
            || id.is_empty()
            || !id.chars().all(fits)
            || crate::chat::parse_time(start).is_none()
            || !(duration.is_finite() && duration > 0.0)
        {
            return None;
        }
        Some(Pov {
            player: player.into(),
            account,
            video: Video { platform, id: id.into() },
            start: start.into(),
            duration,
        })
    }
}

fn povs<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Pov>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(value.as_array().map(|list| list.iter().filter_map(Pov::read).collect()).unwrap_or_default())
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Activity {
    pub start: f64,
    pub end: f64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// A VOD's cast, activity, other points of view and contexts, as an import left them.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct VodAnnotations {
    pub cast: Vec<CastMember>,
    pub activity: Vec<Activity>,
    pub povs: Vec<Pov>,
    pub contexts: Vec<String>,
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

/// As `parse`, from JSON already read.
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
    within_limits(&value)?;
    serde_json::from_value(value).map_err(|e| not_annotations(&e.to_string()))
}

/// `n` as it's written out: "200,000".
fn count(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(d);
    }
    out
}

/// The length of the list at `value`'s `path`, if there's one there.
fn list_len(value: &serde_json::Value, path: &str) -> usize {
    value.pointer(path).and_then(|v| v.as_array()).map_or(0, Vec::len)
}

/// Whether any text in `value` (a key or a string, at any depth) is longer
/// than `MAX_TEXT` characters.
fn has_long_text(value: &serde_json::Value) -> bool {
    let long = |s: &str| s.len() > MAX_TEXT && s.chars().count() > MAX_TEXT;
    match value {
        serde_json::Value::String(s) => long(s),
        serde_json::Value::Array(list) => list.iter().any(has_long_text),
        serde_json::Value::Object(map) => map.iter().any(|(k, v)| long(k) || has_long_text(v)),
        _ => false,
    }
}

/// The items of the list at `value`'s `path`, none if there's none.
fn items<'a>(value: &'a serde_json::Value, path: &str) -> &'a [serde_json::Value] {
    value.pointer(path).and_then(|v| v.as_array()).map_or(&[], Vec::as_slice)
}

/// Refuses an annotations file that brings more than the app takes (the
/// `MAX_` limits), saying what for the user. The page's `withinLimits`
/// (annotations.ts) is the same rule.
fn within_limits(value: &serde_json::Value) -> Result<()> {
    let too_big = |what: String| Err(anyhow::anyhow!("This annotations file is too big: {what}."));
    let lists = [
        ("/transcript/segments", MAX_SEGMENTS, "lines"),
        ("/chapters", MAX_CHAPTERS, "chapters"),
        ("/highlights", MAX_HIGHLIGHTS, "highlights"),
        ("/cast", MAX_CAST, "people in its cast"),
        ("/activity", MAX_ACTIVITY, "stretches of activity"),
        ("/povs", MAX_POVS, "other points of view"),
        ("/contexts", MAX_CONTEXTS, "contexts"),
    ];
    for (path, max, what) in lists {
        if list_len(value, path) > max {
            return too_big(format!("it has more than {} {what}", count(max)));
        }
    }
    let nested = [
        ("/transcript/segments", "/words", MAX_WORDS, "a line has more than {} words"),
        ("/highlights", "/cast", MAX_NAMES, "a highlight names more than {} people"),
        ("/cast", "/aliases", MAX_NAMES, "a cast member has more than {} other names"),
    ];
    for (path, inner, max, what) in nested {
        if items(value, path).iter().any(|item| list_len(item, inner) > max) {
            return too_big(what.replace("{}", &count(max)));
        }
    }
    if has_long_text(value) {
        return too_big(format!("a text in it is longer than {} characters", count(MAX_TEXT)));
    }
    Ok(())
}

/// Refuses a chat file that has more than the app takes (`MAX_MESSAGES`,
/// `MAX_PARTS`, `MAX_TEXT`), saying what for the user. The website's
/// `chatOf` (apps/site annotated.ts) is the same rule.
fn chat_within_limits(value: &serde_json::Value) -> Result<()> {
    let too_big = |what: String| Err(anyhow::anyhow!("This chat file is too big: {what}."));
    let messages = value.as_array().map_or(&[][..], Vec::as_slice);
    if messages.len() > MAX_MESSAGES {
        return too_big(format!("it has more than {} messages", count(MAX_MESSAGES)));
    }
    if messages.iter().any(|m| list_len(m, "/parts") > MAX_PARTS) {
        return too_big(format!("a message has more than {} parts", count(MAX_PARTS)));
    }
    if has_long_text(value) {
        return too_big(format!("a text in it is longer than {} characters", count(MAX_TEXT)));
    }
    Ok(())
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
            let not_chat = || format!("{shown} isn't a chat VODscribe can read");
            let value = serde_json::from_str(&json).with_context(not_chat)?;
            chat_within_limits(&value)?;
            Some(serde_json::from_value(value).with_context(not_chat)?)
        }
        None => None,
    };
    Ok((annotations, chat))
}

/// Whether `id` (and for Kick, `channel`) has the shape the site's own
/// VODs' have, as a VOD added by its link gets them: YouTube's eleven
/// letters, digits, `_` and `-`; Twitch's number, its "v" first or not;
/// Kick's UUID, under a channel of letters, digits, `_` and `-`. They go
/// into the paths of the requests made for the VOD (Kick's playback, on
/// web.kick.com), so a file's own can't steer those.
fn has_sites_shape(platform: &str, id: &str, channel: Option<&str>) -> bool {
    let word = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'));
    match platform {
        "youtube" => id.len() == 11 && word(id),
        "twitch" => {
            let number = twitch_number(id);
            (1..=20).contains(&number.len()) && number.bytes().all(|b| b.is_ascii_digit())
        }
        "kick" => {
            let uuid = id.len() == 36
                && id.bytes().enumerate().all(|(i, b)| match i {
                    8 | 13 | 18 | 23 => b == b'-',
                    _ => b.is_ascii_hexdigit(),
                });
            uuid && channel.is_some_and(|c| c.len() <= 100 && word(c))
        }
        _ => false,
    }
}

/// A Twitch video's id as its address has it: without the "v" yt-dlp puts
/// before a VOD's number; only a "v" before a digit goes, so a clip's slug
/// keeps its own. The page's `twitchNumber` (platform.ts) is the same rule,
/// tested with the same cases.
fn twitch_number(id: &str) -> &str {
    match id.strip_prefix('v') {
        Some(number) if number.starts_with(|c: char| c.is_ascii_digit()) => number,
        _ => id,
    }
}

/// Where a VOD of `platform` with id `id` is watched, for a VOD an import
/// adds to the library; None for a platform it can't be added for that way.
/// As the page's links (platform.ts `watchUrl`) without a time; a Kick
/// link never has one.
fn watch_url(platform: &str, id: &str, channel: Option<&str>) -> Option<String> {
    match platform {
        "youtube" => Some(format!("https://www.youtube.com/watch?v={id}")),
        "twitch" => Some(format!("https://www.twitch.tv/videos/{}", twitch_number(id))),
        "kick" => channel.map(|c| format!("https://kick.com/{}/videos/{id}", c.to_lowercase())),
        _ => None,
    }
}

/// `platform`'s name as its site writes it.
fn site_name(platform: &str) -> &str {
    match platform {
        "youtube" => "YouTube",
        "twitch" => "Twitch",
        "kick" => "Kick",
        other => other,
    }
}

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

/// A time from the file that can stand as one: finite and not before the start.
fn usable(t: f64) -> bool {
    t.is_finite() && t >= 0.0
}

/// A server named by environment variable `name`, without a trailing slash.
fn named_server(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|b| b.trim().trim_end_matches('/').to_string()).filter(|b| !b.is_empty())
}

/// The server published annotations are asked of: `BASE_ENV`'s, if set.
pub(crate) fn base_url() -> String {
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

/// Whether VODs of `platform` can have published annotations: Twitch's,
/// YouTube's and Kick's, the only ones the file host holds. The file host is
/// never asked of others: not a file on this computer (its id is this
/// computer's own), nor a link yt-dlp reads as a plain video file (whose id
/// is the file's name on whatever server it's on, a private one, say), nor
/// another site's VOD.
pub fn can_be_published(platform: &str) -> bool {
    ACCOUNT_PLATFORMS.contains(&platform)
}

/// The client for the file host: quick to give up, since nothing waits on
/// it but an offer. Plain HTTP only for a server named by `BASE_ENV`.
pub(crate) fn agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_connect(Some(std::time::Duration::from_secs(10)))
            .timeout_recv_response(Some(std::time::Duration::from_secs(20)))
            .timeout_recv_body(Some(std::time::Duration::from_secs(120)))
            .https_only(std::env::var_os(BASE_ENV).is_none() && !cfg!(test))
            // The file host answers directly; a redirect could lead anywhere, the local network included.
            .max_redirects(0)
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

pub(crate) fn look_up_at(base: &str, platform: &str, id: &str) -> Result<Found> {
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

/// The largest streamer card read: a few kilobytes is plenty.
const MAX_CARD_BYTES: u64 = 256 * 1024;
/// The largest index of streamers' accounts read (`streamer_index`).
const MAX_INDEX_BYTES: u64 = 16 * 1024 * 1024;

/// Where streamer card `id` is on server `base`, if `id` is one (`card_id`).
pub fn card_url(base: &str, id: &str) -> Option<String> {
    Some(format!("{base}/streamers/{}.json", card_id(id)?))
}

/// Where the index of streamers' accounts (each platform and login's card
/// id; the page reads it, streamers.ts) is on server `base`.
pub fn index_url(base: &str) -> String {
    format!("{base}/streamers.json")
}

/// The JSON at `url` on the file host, read up to `limit` bytes; None when
/// it isn't there (404, 410, 403); offline or anything else, an error. The
/// page reads what's in it, and shows only what it can (streamers.ts).
fn fetch_json(url: &str, limit: u64) -> Result<Option<serde_json::Value>> {
    let mut response = match agent().get(url).call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(403 | 404 | 410)) => return Ok(None),
        Err(e) => return Err(e).context("ask the file host"),
    };
    let json =
        response.body_mut().with_config().limit(limit).read_to_string().context("read the file host's answer")?;
    Ok(Some(serde_json::from_str(&json).context("read the file host's answer")?))
}

/// Streamer card `id` as published, if it is; an id that can't be one is none.
pub fn streamer_card(id: &str) -> Result<Option<serde_json::Value>> {
    streamer_card_at(&base_url(), id)
}

pub(crate) fn streamer_card_at(base: &str, id: &str) -> Result<Option<serde_json::Value>> {
    match card_url(base, id) {
        Some(url) => fetch_json(&url, MAX_CARD_BYTES),
        None => Ok(None),
    }
}

/// The index of streamers' accounts as published, if it is.
pub fn streamer_index() -> Result<Option<serde_json::Value>> {
    fetch_json(&index_url(&base_url()), MAX_INDEX_BYTES)
}

/// What the library knows of a VOD's published annotations, for offering them.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Offer {
    pub available: Availability,
    /// The version published is the one it last brought in, and its
    /// transcript is still the one it brought in.
    pub up_to_date: bool,
    /// It has a transcript made on this computer, which an import replaces.
    pub local_transcript: bool,
    /// A version other than the one it brought in is published: newer, as
    /// versions only go forward.
    pub newer: bool,
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
                    let channel = source.channel.as_deref().map(str::trim);
                    if watch_url(&source.platform, &source.id, channel).is_some()
                        && !has_sites_shape(&source.platform, &source.id, channel)
                    {
                        bail!("The annotations file's VOD isn't one {0} could have: its id or channel isn't the shape of {0}'s.", site_name(&source.platform));
                    }
                    let url = watch_url(&source.platform, &source.id, channel)
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
                        channel_login: None,
                        channel_url: None,
                        duration: Some(source.duration),
                        thumbnail: None,
                        // YYYY-MM-DD, from an ISO 8601 time.
                        published: source
                            .recorded_at
                            .as_deref()
                            .and_then(|t| t.get(..10))
                            .filter(|d| d.len() == 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-')
                            .map(str::to_string),
                        started_at: None,
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
            // When it was recorded, if nothing better says when it started.
            if let Some(at) = &source.recorded_at {
                db.set_start_if_unknown(vod_id, at)?;
            }
            db.import_transcript(vod_id, annotations)?;
            db.import_marks(vod_id, annotations)?;
            db.conn.execute(
                "INSERT OR REPLACE INTO vod_annotations (vod_id, cast_members, activity, produced, imported_at, povs, contexts)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    vod_id,
                    serde_json::to_string(&annotations.cast)?,
                    serde_json::to_string(&annotations.activity)?,
                    annotations.produced.as_ref().map(serde_json::to_string).transpose()?,
                    now(),
                    serde_json::to_string(&annotations.povs)?,
                    serde_json::to_string(&annotations.contexts)?
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
        let mut stmt = self.conn.prepare(&format!(
            "INSERT INTO segments ({}) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            crate::db::SEGMENT_COLUMNS
        ))?;
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
    /// the import, in place of the last import's; the user's own stay, and so do
    /// the imported ones they changed, which the file's copy doesn't join again.
    fn import_marks(&self, vod_id: i64, annotations: &Annotations) -> Result<()> {
        let clip = self.vod_clip(vod_id)?.context("That VOD is gone.")?;
        self.change(clip, None, |db| {
            db.conn.execute("DELETE FROM marks WHERE clip_id = ?1 AND source = ?2", params![clip, SOURCE])?;
            // What's left of an earlier import is what the user changed: theirs, in place of the import's.
            let kept: HashSet<String> = db
                .conn
                .prepare("SELECT imported_as FROM marks WHERE clip_id = ?1 AND imported_as IS NOT NULL")?
                .query_map([clip], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            let key = |kind: &str, start: f64| format!("{kind} {}", Flicks::from_seconds(start).0);
            let from_import = |kind: &str, start: f64| MarkDetail {
                source: SOURCE.into(),
                imported_as: Some(key(kind, start)),
                ..MarkDetail::default()
            };
            let new = |kind: &str, start: f64| usable(start) && !kept.contains(&key(kind, start));
            for c in annotations.chapters.iter().filter(|c| new("chapter", c.start)) {
                let title = Some(c.title.trim()).filter(|t| !t.is_empty());
                let detail = MarkDetail {
                    icon: c.icon.as_deref().and_then(one_emoji).map(Into::into),
                    ..from_import("chapter", c.start)
                };
                db.insert_mark_with(clip, vod_id, "chapter", Flicks::from_seconds(c.start), title, &detail)?;
            }
            for h in annotations.highlights.iter().filter(|h| new("highlight", h.start)) {
                let detail = MarkDetail {
                    end: Some(h.end).filter(|e| e.is_finite() && *e > h.start),
                    category: Some(h.kind.trim().to_string()).filter(|k| !k.is_empty()),
                    note: Some(h.why.trim().to_string()).filter(|w| !w.is_empty()),
                    cast: h.cast.iter().map(|n| n.trim()).filter(|n| !n.is_empty()).map(Into::into).collect(),
                    ..from_import("highlight", h.start)
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
        // Without a version from the server, or none kept from the import, there's no telling.
        let same =
            version.as_deref().zip(imported.as_deref()).map(|(v, i)| crate::annotations_filter::same_version(v, i));
        Ok(Offer {
            available: Availability::parse(available.as_deref()),
            // Never up to date with no telling, nor with lines transcribed here since:
            // what it brought in is replaced, in part or all.
            up_to_date: same == Some(true) && !local,
            local_transcript: local,
            newer: same == Some(false),
        })
    }

    /// VOD `vod_id`'s cast, activity, other points of view and contexts, if annotations were imported for it.
    pub fn vod_annotations(&self, vod_id: i64) -> Result<Option<VodAnnotations>> {
        type Row = (String, String, Option<String>, Option<String>);
        let row: Option<Row> = self
            .conn
            .query_row(
                "SELECT cast_members, activity, povs, contexts FROM vod_annotations WHERE vod_id = ?1",
                [vod_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((cast, activity, povs, contexts)) = row else { return Ok(None) };
        Ok(Some(VodAnnotations {
            cast: serde_json::from_str(&cast)?,
            activity: serde_json::from_str(&activity)?,
            // None from an import before they were kept (`db::MIGRATE_V34`, `db::MIGRATE_V38`).
            povs: povs.as_deref().map(serde_json::from_str).transpose()?.unwrap_or_default(),
            contexts: contexts.as_deref().map(serde_json::from_str).transpose()?.unwrap_or_default(),
        }))
    }
}

#[cfg(test)]
pub(crate) mod tests;
