//! The sites' own APIs as a test's network answers them (`Net`), for what a
//! test needs of a site beyond yt-dlp's details: what each says, in the
//! shape the site gives it, for one video of the test's own, so tests on
//! the same site's host don't meet.
//!
//! - Twitch's GraphQL API: a VOD's comments, paged as Twitch pages them, and
//!   its cheermotes (`TwitchChat`, `Net::twitch_comments`), and a clip
//!   (`Net::twitch_clip`).
//! - Kick's API: a VOD's playback answer, the playlists it names, and its
//!   channel's chat history, paged back as Kick pages it (`KickVod`,
//!   `Net::kick_vod`).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

use super::net::{Net, Reply, Request};

/// The GraphQL request `request` is, if it's about video `id`: its query.
fn about(request: &Request, id: &str) -> Option<String> {
    let body = request.json()?;
    (body.pointer("/variables/id").and_then(Value::as_str) == Some(id))
        .then(|| body["query"].as_str().unwrap_or_default().to_string())
}

/// A comment as Twitch's GraphQL API gives it: `at` seconds into the VOD,
/// by `author`, saying `text`, with no colour and no emotes. Set its
/// `createdAt`, `message.userColor` or `message.fragments` as Twitch would.
pub fn twitch_comment(at: f64, author: &str, text: &str) -> Value {
    json!({
        "contentOffsetSeconds": at,
        "commenter": { "displayName": author },
        "message": { "userColor": null, "fragments": [{ "text": text, "emote": null }] },
    })
}

/// What Twitch answers for a comments page it won't give, as it did on
/// 2026-10-24 for the test VOD (67 s long) from 69 s on.
pub fn twitch_service_error() -> Value {
    json!({ "errors": [{ "message": "service error", "path": ["video", "comments"] }],
        "data": { "video": { "comments": null } },
        "extensions": { "durationMilliseconds": 41, "requestID": "01M5Q9JGMYCF91S411PH3FN0ES" } })
}

/// A VOD's chat as Twitch's GraphQL API serves it (`Net::twitch_comments`):
/// its comments (`twitch_comment`), a page from an offset holding the next
/// 100 from that second on and saying whether there are more; and its
/// answer to the ask for the cheermotes. Each comment is given an id, unless
/// it has one.
#[derive(Clone)]
pub struct TwitchChat {
    comments: Arc<Vec<Value>>,
    /// The answer to the cheermotes' ask; None: it fails (500).
    cheermotes: Option<Value>,
    /// Pages from more than a second past this are refused (`twitch_service_error`).
    length: Option<f64>,
    /// Every page says there are more, even past the last comment.
    says_more: bool,
    /// Whether the nth comments page asked (from 0) is refused instead.
    fails: Arc<dyn Fn(usize) -> bool + Send + Sync>,
    asked: Arc<AtomicUsize>,
}

impl TwitchChat {
    /// The chat of `comments`, in any order; the channel has no cheermotes
    /// but the built-in ones.
    pub fn of(comments: Vec<Value>) -> Self {
        TwitchChat {
            comments: Arc::new(comments),
            cheermotes: Some(json!({ "data": { "cheerConfig": { "groups": [] }, "video": null } })),
            length: None,
            says_more: false,
            fails: Arc::new(|_| false),
            asked: Arc::default(),
        }
    }

    /// Twitch answers the ask for the cheermotes with `answer`
    /// (`cheerConfig` and the channel's `cheerGroups`).
    pub fn listing(mut self, answer: Value) -> Self {
        self.cheermotes = Some(answer);
        self
    }

    /// Twitch fails the ask for the cheermotes (500).
    pub fn listing_none(mut self) -> Self {
        self.cheermotes = None;
        self
    }

    /// The VOD is `seconds` long: Twitch refuses the pages from more than a
    /// second past its end, as it does (`twitch_service_error`).
    pub fn ending_at(mut self, seconds: f64) -> Self {
        self.length = Some(seconds);
        self
    }

    /// Every page says there are more, even the empty ones past the last
    /// comment, as Twitch's sometimes do.
    pub fn always_saying_more(mut self) -> Self {
        self.says_more = true;
        self
    }

    /// The nth comments page asked for (from 0) is refused when `fails(n)`,
    /// as Twitch refuses one now and then (`twitch_service_error`).
    pub fn failing(mut self, fails: impl Fn(usize) -> bool + Send + Sync + 'static) -> Self {
        self.fails = Arc::new(fails);
        self
    }

    /// How many comments pages have been asked for so far, refused ones
    /// included.
    pub fn pages_asked(&self) -> usize {
        self.asked.load(Ordering::Relaxed)
    }

    /// The answer to `query` (a comments page from `offset`, or the
    /// cheermotes) for VOD `id`.
    fn answer(&self, id: &str, query: &str, offset: f64) -> Option<Reply> {
        if query.contains("cheerConfig") {
            return Some(self.cheermotes.as_ref().map_or_else(|| Reply::status(500), Reply::json));
        }
        if !query.contains("comments(") {
            return None;
        }
        let n = self.asked.fetch_add(1, Ordering::Relaxed);
        if (self.fails)(n) || self.length.is_some_and(|l| offset > l + 1.0) {
            return Some(Reply::json(&twitch_service_error()));
        }
        let from: Vec<Value> = self
            .comments
            .iter()
            .enumerate()
            .filter(|(_, c)| c["contentOffsetSeconds"].as_f64().unwrap_or(0.0).floor() >= offset)
            .map(|(i, c)| {
                let mut node = c.clone();
                if node.get("id").is_none() {
                    node["id"] = json!(format!("{id}-{i}"));
                }
                json!({ "node": node })
            })
            .collect();
        let more = self.says_more || from.len() > 100;
        let edges: Vec<Value> = from.into_iter().take(100).collect();
        Some(Reply::json(&json!({ "data": { "video": {
            "comments": { "edges": edges, "pageInfo": { "hasNextPage": more } },
        } } })))
    }
}

/// A Kick VOD as Kick's API serves it (`Net::kick_vod`): its playback answer,
/// naming its channel, its length and a master playlist of two qualities,
/// whose playlists say when it started; and its channel's chat history
/// (`kick_message`), paged back from a cursor 25 at a time, newest first.
#[derive(Clone)]
pub struct KickVod {
    id: String,
    channel: u64,
    duration: f64,
    started: Option<f64>,
    history: Arc<Vec<Value>>,
    /// Whether the nth history page asked (from 0) fails instead (500).
    fails: Arc<dyn Fn(usize) -> bool + Send + Sync>,
    asked: Arc<AtomicUsize>,
}

impl KickVod {
    /// VOD `id` (Kick's, a UUID), `duration` seconds long, on the channel
    /// numbered `channel` (its chat history's), with no chat; its
    /// playlists say no time, so its start is the time in its id.
    pub fn new(id: &str, channel: u64, duration: f64) -> Self {
        KickVod {
            id: id.into(),
            channel,
            duration,
            started: None,
            history: Arc::default(),
            fails: Arc::new(|_| false),
            asked: Arc::default(),
        }
    }

    /// Its link, as its page is: on the channel `barky`.
    pub fn url(&self) -> String {
        format!("https://kick.com/barky/videos/{}", self.id)
    }

    /// Its master playlist's link, as its playback answer gives it.
    pub fn master(&self) -> String {
        format!("https://stream.kick.com/test/{}/master.m3u8", self.id)
    }

    /// The link of its playlist of `quality` (`1080p60`, `360p30`).
    pub fn playlist(&self, quality: &str) -> String {
        format!("https://stream.kick.com/test/{}/{quality}/playlist.m3u8", self.id)
    }

    /// Its playlists' first segment was at `at` (Unix seconds), when it
    /// started.
    pub fn started(mut self, at: f64) -> Self {
        self.started = Some(at);
        self
    }

    /// Its channel's chat history is `messages` (`kick_message`), in any
    /// order; each is given an id, unless it has one.
    pub fn chat(mut self, messages: Vec<Value>) -> Self {
        self.history = Arc::new(messages);
        self
    }

    /// The nth history page asked for (from 0) fails when `fails(n)` (500),
    /// as a site does for a moment.
    pub fn failing(mut self, fails: impl Fn(usize) -> bool + Send + Sync + 'static) -> Self {
        self.fails = Arc::new(fails);
        self
    }

    /// The history page before `cursor` (microseconds).
    fn page(&self, cursor: i64) -> Reply {
        if (self.fails)(self.asked.fetch_add(1, Ordering::Relaxed)) {
            return Reply::status(500);
        }
        let sent = |m: &Value| m["created_at"].as_str().and_then(crate::chat::parse_time).unwrap_or(0.0);
        let mut older: Vec<(i64, Value)> = self
            .history
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let mut m = m.clone();
                if m.get("id").is_none() {
                    m["id"] = json!(format!("{}-{i}", self.channel));
                }
                ((sent(&m) * 1e6).round() as i64, m)
            })
            .filter(|(at, _)| *at < cursor)
            .collect();
        older.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
        older.truncate(25);
        let next = older.last().map(|(at, _)| at.to_string());
        let messages: Vec<Value> = older.into_iter().map(|(_, m)| m).collect();
        Reply::json(&json!({ "data": { "messages": messages, "cursor": next } }))
    }
}

/// A message as Kick's chat history gives it: sent at `at` (Unix seconds,
/// to the microsecond) by `sender`, in Kick's green.
pub fn kick_message(at: f64, sender: &str, content: &str) -> Value {
    let whole = at.floor();
    let micros = ((at - whole) * 1e6).round() as i64;
    let sent = crate::db::utc_time(whole).replace('Z', &format!(".{micros:06}Z"));
    json!({ "content": content, "created_at": sent,
        "sender": { "username": sender, "identity": { "color": "#53FC18" } } })
}

impl Net {
    /// Twitch's GraphQL API has VOD `id`'s chat (its number, without
    /// yt-dlp's "v"): `said`, each `(seconds, who, text)` (`TwitchChat`).
    pub fn twitch_chat(&self, id: &str, said: &[(f64, &str, &str)]) -> &Self {
        let comments = said.iter().map(|(at, who, text)| twitch_comment(*at, who, text)).collect();
        self.twitch_comments(id, &TwitchChat::of(comments))
    }

    /// Twitch's GraphQL API serves `chat` as VOD `id`'s (its number,
    /// without yt-dlp's "v"). Kept, `chat` counts the pages asked for.
    pub fn twitch_comments(&self, id: &str, chat: &TwitchChat) -> &Self {
        let (id, chat) = (id.to_string(), chat.clone());
        self.replies(crate::chat::TWITCH_GQL, move |request| {
            let query = about(request, &id)?;
            let offset = request.json()?.pointer("/variables/offset").and_then(Value::as_f64).unwrap_or(0.0);
            chat.answer(&id, &query, offset)
        })
    }

    /// Twitch's GraphQL API knows clip `slug`: `duration` seconds long, cut
    /// from `from`, a VOD's number and where in it the clip starts, while
    /// Twitch keeps that VOD (None once it's gone).
    pub fn twitch_clip(&self, slug: &str, from: Option<(&str, f64)>, duration: f64) -> &Self {
        let slug = slug.to_string();
        let clip = json!({
            "durationSeconds": duration,
            "videoOffsetSeconds": from.map(|(_, at)| at),
            "video": from.map(|(video, _)| json!({ "id": video })),
            "broadcaster": { "id": "1000" },
        });
        self.replies(crate::chat::TWITCH_GQL, move |request| {
            let body = request.json()?;
            let query = body["query"].as_str()?;
            let ours = body.pointer("/variables/slug").and_then(Value::as_str) == Some(slug.as_str());
            (ours && query.contains("durationSeconds")).then(|| Reply::json(&json!({ "data": { "clip": clip } })))
        })
    }

    /// Kick's API serves `vod` (`KickVod`): its playback answer, its
    /// playlists, and its channel's chat history.
    pub fn kick_vod(&self, vod: &KickVod) -> &Self {
        let playback = json!({
            "playback_url": { "vod": vod.master() },
            "video_session": {
                "creator_id": vod.channel.to_string(),
                "video_title": "A stream",
                "video_series": "Barky",
                "video_duration": vod.duration,
            },
        });
        self.serves(&format!("https://web.kick.com/api/v1/stream/{}/playback", vod.id), playback.to_string());
        let master = format!(
            "#EXTM3U\n#EXT-X-STREAM-INF:VIDEO=\"1080p60\",RESOLUTION=1920x1080,BANDWIDTH=8000000\n{}\n\
             #EXT-X-STREAM-INF:VIDEO=\"360p30\",RESOLUTION=640x360,BANDWIDTH=630000\n{}\n",
            vod.playlist("1080p60"),
            vod.playlist("360p30")
        );
        self.serves(&vod.master(), master);
        let date =
            vod.started.map_or(String::new(), |at| format!("#EXT-X-PROGRAM-DATE-TIME:{}\n", crate::db::utc_time(at)));
        for quality in ["1080p60", "360p30"] {
            self.serves(&vod.playlist(quality), format!("#EXTM3U\n{date}#EXTINF:10.000,\n0.ts\n"));
        }
        let vod = vod.clone();
        let history = format!("https://web.kick.com/api/v1/chat/{}/history", vod.channel);
        self.replies(&history, move |request| {
            let cursor = request.url.split_once("cursor=")?.1.split('&').next()?.parse().ok()?;
            Some(vod.page(cursor))
        })
    }
}
