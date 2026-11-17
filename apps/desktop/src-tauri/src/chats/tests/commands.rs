//! The specs of `chats.rs`'s commands, as the page calls them: what the
//! transcript view, its minimap and header ask of a VOD's chat and of an
//! edit's, the emotes it shows, the chat fetched again after a failure, and
//! the messages a timeline layer's words match. A chat is put in the library
//! as a fetch leaves it (`Db::set_chat`), or fetched from Twitch's API on the
//! kit's network, which a test's links and ids keep to their own.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{Value, json};
use vodscribe_core::chat::{ChatMessage, ChatPart, Kind};
use vodscribe_core::db::{ChatStatus, Vod};
use vodscribe_core::testkit::sites::{self, TwitchChat};
use vodscribe_core::testkit::{Net, Reply, eventually};

use crate::testkit::TestApp;

/// A message of `author`, `time` seconds in, saying `text`.
fn said(time: f64, author: &str, text: &str) -> ChatMessage {
    ChatMessage {
        time,
        author: author.into(),
        color: None,
        parts: vec![ChatPart::Text { text: text.into() }],
        kind: Kind::Message,
        amount: None,
        event_time: None,
        at: None,
    }
}

/// A sub, gift or tip of `amount`, `time` seconds in.
fn event(time: f64, author: &str, kind: Kind, amount: f64) -> ChatMessage {
    ChatMessage { kind, amount: Some(amount), ..said(time, author, "an event") }
}

/// A message saying `text` and showing the emotes `emotes` (name, image).
fn showing(time: f64, author: &str, emotes: &[(&str, &str)]) -> ChatMessage {
    let mut message = said(time, author, "hi ");
    message.parts.extend(emotes.iter().map(|(name, url)| ChatPart::Emote { name: (*name).into(), url: (*url).into() }));
    message
}

/// VOD `vod`'s chat, as a fetch leaves it.
fn has_chat(app: &TestApp, vod: &Vod, messages: &[ChatMessage]) {
    app.db().set_chat(vod.id, messages).unwrap();
}

/// A video file of 100 seconds, "Day <n>.mp4".
fn a_vod(app: &TestApp, n: u32) -> Vod {
    app.add_video_file(&format!("Day {n}.mp4"), 100.0)
}

/// `command`'s answer, which the page gets as JSON.
fn ask(app: &TestApp, command: &str, args: Value) -> Value {
    app.invoke(command, args).unwrap()
}

/// What the page reads of a chat line: (second, author, text of its first part).
fn lines(windows: &Value, window: usize) -> Vec<(f64, String, String)> {
    windows[window]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["time"].as_f64().unwrap(),
                m["author"].as_str().unwrap().into(),
                m["parts"][0]["text"].as_str().unwrap_or_default().into(),
            )
        })
        .collect()
}

fn times(lines: &[(f64, String, String)]) -> Vec<f64> {
    lines.iter().map(|l| l.0).collect()
}

/// The numbers of an overview's histogram.
fn histogram(overview: &Value) -> Vec<u64> {
    overview["histogram"].as_array().unwrap().iter().map(|n| n.as_u64().unwrap()).collect()
}

/// The (second, kind, amount) of an overview's events.
fn events(overview: &Value) -> Vec<(f64, String, f64)> {
    overview["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["time"].as_f64().unwrap(), e["kind"].as_str().unwrap().into(), e["amount"].as_f64().unwrap()))
        .collect()
}

// A VOD's overview: what the minimap and the header need.

/// Its count is every message the chat has (chats.rs `chat_overview`: "What
/// the minimap and header need of VOD `vod_id`'s chat"; CHANGELOG.md, 0.2.0:
/// "A timeline beside the transcript shows how busy chat was over the whole
/// VOD.").
#[test]
fn a_vods_overview_counts_its_messages() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    has_chat(&app, &vod, &[said(5.0, "ada", "hi"), said(10.0, "bo", "o7"), event(20.0, "cy", Kind::Sub, 1.0)]);
    let overview = ask(&app, "chat_overview", json!({ "vodId": vod.id, "duration": 100.0, "bins": 4 }));
    assert_eq!(overview["count"], 3);
}

/// Its histogram says how many messages fall in each of `bins` equal
/// stretches of the duration (chats.rs `ChatOverview`: "Messages in each of
/// `bins` equal stretches of `duration`").
#[test]
fn a_vods_overview_says_how_busy_each_stretch_of_its_length_is() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    let chat = [5.0, 10.0, 30.0, 60.0, 99.0].map(|t| said(t, "ada", "hi"));
    has_chat(&app, &vod, &chat);
    let overview = ask(&app, "chat_overview", json!({ "vodId": vod.id, "duration": 100.0, "bins": 4 }));
    assert_eq!(histogram(&overview), [2, 1, 1, 1]);
}

/// A message that runs past the video's end still counts, in the last
/// stretch (CHANGELOG.md, 0.6.0 Fixes: "Their last messages can run a second
/// or two past the video").
#[test]
fn a_message_past_the_end_of_the_vod_counts_in_its_last_stretch() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    has_chat(&app, &vod, &[said(10.0, "ada", "hi"), said(101.5, "bo", "late")]);
    let overview = ask(&app, "chat_overview", json!({ "vodId": vod.id, "duration": 100.0, "bins": 4 }));
    assert_eq!(histogram(&overview), [1, 0, 0, 1]);
    assert_eq!(overview["count"], 2);
}

/// Its events are the subs, gifts and tips, oldest first, and no plain
/// message (chats.rs `ChatOverview`: "The subs, gifts and tips"; CHANGELOG.md,
/// 0.2.0: "Subs, gifts and tips (bits, KICKs, Super Chats and memberships)
/// are marked in chat"). Asked with no `events`, they come.
#[test]
fn a_vods_overview_lists_its_subs_gifts_and_tips_oldest_first() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    let chat = [
        event(40.0, "ada", Kind::Sub, 1.0),
        said(30.0, "bo", "hello"),
        event(70.0, "cy", Kind::Gift, 5.0),
        event(20.0, "di", Kind::Tip, 500.0),
    ];
    has_chat(&app, &vod, &chat);
    let overview = ask(&app, "chat_overview", json!({ "vodId": vod.id, "duration": 100.0, "bins": 4 }));
    let wanted = [(20.0, "tip".to_string(), 500.0), (40.0, "sub".into(), 1.0), (70.0, "gift".into(), 5.0)];
    assert_eq!(events(&overview), wanted);
}

/// A view that is refreshing chat while it comes in asks for no events, and
/// gets the count and the histogram without them (chats.rs `ChatOverview`:
/// "not while a fetch is refreshing the rest (`events: false`)").
#[test]
fn an_overview_asked_for_no_events_leaves_them_out() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    has_chat(&app, &vod, &[said(10.0, "ada", "hi"), event(40.0, "bo", Kind::Sub, 1.0)]);
    let overview =
        ask(&app, "chat_overview", json!({ "vodId": vod.id, "duration": 100.0, "bins": 4, "events": false }));
    assert_eq!(overview["events"], Value::Null);
    assert_eq!(overview["count"], 2);
    assert_eq!(histogram(&overview), [1, 1, 0, 0]);
}

/// A VOD with no chat has a count of none, a histogram of empty stretches and
/// no events.
#[test]
fn a_vod_with_no_chat_has_an_empty_overview() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    let overview = ask(&app, "chat_overview", json!({ "vodId": vod.id, "duration": 100.0, "bins": 4 }));
    assert_eq!(overview, json!({ "count": 0, "histogram": [0, 0, 0, 0], "events": [] }));
}

/// One VOD's chat isn't another's: its overview counts its own messages only
/// (chats.rs `chat_overview`: "VOD `vod_id`'s chat").
#[test]
fn an_overview_counts_only_its_own_vods_chat() {
    let app = TestApp::new();
    let (first, second) = (a_vod(&app, 1), a_vod(&app, 2));
    has_chat(&app, &first, &[said(10.0, "ada", "hi")]);
    has_chat(&app, &second, &[said(10.0, "bo", "hi"), said(20.0, "cy", "hi"), event(30.0, "di", Kind::Tip, 1.0)]);
    let overview = ask(&app, "chat_overview", json!({ "vodId": first.id, "duration": 100.0, "bins": 4 }));
    assert_eq!(overview["count"], 1);
    assert_eq!(overview["events"], json!([]));
}

// A VOD's windows: the chat each row of the transcript shows.

/// Each stretch gets its first `cap` messages and how many it has, so a
/// stretch busier than `cap` says so (chats.rs `chat_windows`: "For each
/// stretch (from, to: seconds), its first `cap` messages and how many it
/// has"; CHANGELOG.md, 0.2.0: "each line with the messages sent while it was
/// said").
#[test]
fn each_stretch_gets_its_first_messages_and_how_many_it_has() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    let chat = [5.0, 10.0, 20.0, 35.0].map(|t| said(t, "ada", &format!("at {t}")));
    has_chat(&app, &vod, &chat);
    let windows =
        ask(&app, "chat_windows", json!({ "vodId": vod.id, "ranges": [[0.0, 30.0], [30.0, 60.0]], "cap": 2 }));
    assert_eq!(times(&lines(&windows, 0)), [5.0, 10.0]);
    assert_eq!(windows[0]["total"], 3);
    assert_eq!(times(&lines(&windows, 1)), [35.0]);
    assert_eq!(windows[1]["total"], 1);
}

/// A stretch holds the messages from its start up to, but not at, its end, so
/// a message at the join is in the next stretch only (db.rs `chat_windows`:
/// "(from, to: seconds, the end left out)").
#[test]
fn a_stretchs_end_is_left_out_of_it() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    has_chat(&app, &vod, &[said(30.0, "ada", "on the join")]);
    let windows =
        ask(&app, "chat_windows", json!({ "vodId": vod.id, "ranges": [[0.0, 30.0], [30.0, 60.0]], "cap": 5 }));
    assert_eq!(times(&lines(&windows, 0)), Vec::<f64>::new());
    assert_eq!(times(&lines(&windows, 1)), [30.0]);
}

/// A stretch's messages come in the order they were sent (db.rs
/// `chat_windows`: "its first `cap` messages", the chat "while it was said").
#[test]
fn a_stretchs_messages_come_in_the_order_they_were_sent() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    has_chat(&app, &vod, &[said(20.0, "ada", "third"), said(5.0, "bo", "first"), said(12.0, "cy", "second")]);
    let windows = ask(&app, "chat_windows", json!({ "vodId": vod.id, "ranges": [[0.0, 30.0]], "cap": 5 }));
    let said: Vec<String> = lines(&windows, 0).into_iter().map(|l| l.2).collect();
    assert_eq!(said, ["first", "second", "third"]);
}

/// A stretch where chat was quiet, or a VOD with no chat, has no messages and
/// a total of none; each stretch asked for is answered, in order.
#[test]
fn a_quiet_stretch_has_no_messages_and_a_total_of_none() {
    let app = TestApp::new();
    let (chatty, silent) = (a_vod(&app, 1), a_vod(&app, 2));
    has_chat(&app, &chatty, &[said(50.0, "ada", "hi")]);
    for vod in [&chatty, &silent] {
        let ranges = json!([[0.0, 10.0], [10.0, 20.0], [20.0, 30.0]]);
        let windows = ask(&app, "chat_windows", json!({ "vodId": vod.id, "ranges": ranges, "cap": 5 }));
        assert_eq!(windows.as_array().unwrap().len(), 3);
        for window in 0..3 {
            assert_eq!(windows[window], json!({ "messages": [], "total": 0 }));
        }
    }
}

/// A message is given as the view shows it: who wrote it, in what colour, its
/// text and emotes in order, and for a sub, gift or tip its kind and amount
/// (CHANGELOG.md, 0.2.0: "each line with the messages sent while it was said,
/// emotes and all"). Its id is the one `chat_matches` gives.
#[test]
fn a_message_comes_with_its_author_colour_emotes_kind_and_amount() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    let image = "https://cdn.7tv.app/emote/chats-line/1x.webp";
    let mut tip = showing(10.0, "ada", &[("Kappa", image)]);
    tip.color = Some("#ff0000".into());
    tip.kind = Kind::Tip;
    tip.amount = Some(500.0);
    has_chat(&app, &vod, &[tip]);
    let windows = ask(&app, "chat_windows", json!({ "vodId": vod.id, "ranges": [[0.0, 30.0]], "cap": 5 }));
    let line = &windows[0]["messages"][0];
    let id = line["id"].as_i64().unwrap();
    let wanted = json!({
        "id": id, "time": 10.0, "author": "ada", "color": "#ff0000",
        "parts": [{ "type": "text", "text": "hi " }, { "type": "emote", "name": "Kappa", "url": image }],
        "kind": "tip", "amount": 500.0,
    });
    assert_eq!(line, &wanted);
}

// An edit's overview and windows: each piece's VOD's chat where it plays.

/// Two VODs with chat, A (a Twitch VOD) and B (a Kick VOD), and the edit
/// playing A's 10 to 20 and then B's 40 to 60: A's second 10 is the edit's 0,
/// B's 40 its 10. Its VODs' chat: A's at 5, 15 (a sub) and 25, B's at 12, 50
/// (a tip) and 95.
fn an_edit_of_two_vods(app: &TestApp) -> i64 {
    let a = app.add_site_vod("twitch", "9300001", "A", 100.0);
    let b = app.add_site_vod("kick", "9300002", "B", 100.0);
    has_chat(app, &a, &[said(5.0, "ada", "a5"), event(15.0, "bo", Kind::Sub, 1.0), said(25.0, "cy", "a25")]);
    has_chat(app, &b, &[said(12.0, "di", "b12"), event(50.0, "ed", Kind::Tip, 300.0), said(95.0, "fay", "b95")]);
    app.edit_of(&[(a.id, 10.0, 20.0), (b.id, 40.0, 60.0)])
}

/// An edit's chat is each piece's VOD's chat where that piece plays: its count
/// is those messages, and its histogram is over the edit's own length
/// (CHANGELOG.md, 0.5.0: "An edit's transcript has chat beside it: each
/// piece's VOD's chat where that piece plays, and the minimap shows how busy
/// it is over the edit. Its header has the line count").
#[test]
fn an_edits_chat_is_each_pieces_chat_where_the_piece_plays() {
    let app = TestApp::new();
    let edit = an_edit_of_two_vods(&app);
    let overview = ask(&app, "edit_chat_overview", json!({ "id": edit, "duration": 30.0, "bins": 3 }));
    assert_eq!(overview["count"], 2);
    // A's sub at 15 is the edit's 5; B's tip at 50 its 20.
    assert_eq!(histogram(&overview), [1, 0, 1]);
}

/// Its subs, gifts and tips show where they play, at their time in the edit
/// (CHANGELOG.md, 0.5.0: "subs, gifts and tips, and the Timeline layers of
/// each piece's VOD, show where they play"; edits.rs `edit_chat_events`).
#[test]
fn an_edits_events_are_where_they_play_in_the_edit() {
    let app = TestApp::new();
    let edit = an_edit_of_two_vods(&app);
    let overview = ask(&app, "edit_chat_overview", json!({ "id": edit, "duration": 30.0, "bins": 3 }));
    assert_eq!(events(&overview), [(5.0, "sub".to_string(), 1.0), (20.0, "tip".into(), 300.0)]);
}

/// Asked for no events, an edit's overview leaves them out as a VOD's does.
#[test]
fn an_edits_overview_asked_for_no_events_leaves_them_out() {
    let app = TestApp::new();
    let edit = an_edit_of_two_vods(&app);
    let overview = ask(&app, "edit_chat_overview", json!({ "id": edit, "duration": 30.0, "bins": 3, "events": false }));
    assert_eq!(overview["events"], Value::Null);
    assert_eq!(overview["count"], 2);
}

/// An edit just opened, whose length isn't known yet, still says whether it
/// has chat at all (edits.rs `edit_chat_histogram`: "Counted even before the
/// length is known (an edit just opened): the count says whether it has chat
/// at all").
#[test]
fn an_edit_whose_length_is_not_known_yet_still_counts_its_chat() {
    let app = TestApp::new();
    let edit = an_edit_of_two_vods(&app);
    let overview = ask(&app, "edit_chat_overview", json!({ "id": edit, "duration": 0.0, "bins": 3 }));
    assert_eq!(overview["count"], 2);
    assert_eq!(histogram(&overview), [0, 0, 0]);
}

/// An edit of VODs with no chat has none: a count of none, empty stretches, no
/// events.
#[test]
fn an_edit_of_vods_with_no_chat_has_none() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    let edit = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let overview = ask(&app, "edit_chat_overview", json!({ "id": edit, "duration": 10.0, "bins": 2 }));
    assert_eq!(overview, json!({ "count": 0, "histogram": [0, 0], "events": [] }));
}

/// A stretch of an edit's timeline gets the messages that play in it, at
/// their time in the edit, each marked with the site its VOD is on, since the
/// list mixes VODs' (edits.rs `edit_chat_windows`: "each piece's VOD's chat
/// where it plays, at its time in the edit"; `ChatLine::platform`).
#[test]
fn an_edits_windows_give_the_messages_at_their_time_in_the_edit_and_their_site() {
    let app = TestApp::new();
    let edit = an_edit_of_two_vods(&app);
    let windows =
        ask(&app, "edit_chat_windows", json!({ "id": edit, "ranges": [[0.0, 10.0], [10.0, 30.0]], "cap": 5 }));
    let first = windows[0]["messages"].as_array().unwrap();
    assert_eq!(first.iter().map(|m| m["time"].as_f64().unwrap()).collect::<Vec<_>>(), [5.0]);
    assert_eq!(first[0]["platform"], "twitch");
    let second = windows[1]["messages"].as_array().unwrap();
    assert_eq!(second.iter().map(|m| m["time"].as_f64().unwrap()).collect::<Vec<_>>(), [20.0]);
    assert_eq!(second[0]["platform"], "kick");
    assert_eq!((windows[0]["total"].clone(), windows[1]["total"].clone()), (json!(1), json!(1)));
}

/// A VOD's own windows carry no site: only an edit's list mixes VODs.
#[test]
fn a_vods_windows_carry_no_site() {
    let app = TestApp::new();
    let vod = app.add_site_vod("twitch", "9300003", "A", 100.0);
    has_chat(&app, &vod, &[said(5.0, "ada", "hi")]);
    let windows = ask(&app, "chat_windows", json!({ "vodId": vod.id, "ranges": [[0.0, 30.0]], "cap": 5 }));
    assert!(windows[0]["messages"][0].get("platform").is_none());
}

/// An edit's stretch gets its first `cap` messages and how many it has, as a
/// VOD's does (chats.rs `edit_chat_windows`: "its first `cap` messages and how
/// many it has").
#[test]
fn an_edits_stretch_gets_its_first_messages_and_how_many_it_has() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    has_chat(&app, &vod, &[said(12.0, "ada", "one"), said(14.0, "bo", "two"), said(16.0, "cy", "three")]);
    let edit = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let windows = ask(&app, "edit_chat_windows", json!({ "id": edit, "ranges": [[0.0, 10.0]], "cap": 2 }));
    let said: Vec<String> = lines(&windows, 0).into_iter().map(|l| l.2).collect();
    assert_eq!(said, ["one", "two"]);
    assert_eq!(windows[0]["total"], 3);
}

/// A moment an edit plays twice shows its messages once in a stretch that
/// holds both plays, where it was first played (edits.rs `edit_chat_windows`:
/// "A moment played twice in the window: its messages once, where first
/// played").
#[test]
fn a_moment_an_edit_plays_twice_shows_its_messages_once() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    has_chat(&app, &vod, &[said(15.0, "ada", "once")]);
    let edit = app.edit_of(&[(vod.id, 10.0, 20.0), (vod.id, 10.0, 20.0)]);
    let windows = ask(&app, "edit_chat_windows", json!({ "id": edit, "ranges": [[0.0, 20.0]], "cap": 5 }));
    assert_eq!(times(&lines(&windows, 0)), [5.0]);
}

// The emotes a VOD's chat shows.

/// The emotes in a VOD's chat come as (name, image), each once, by name; those
/// of another VOD's chat aren't among them (chats.rs `chat_emotes`: "The
/// emotes in VOD `vod_id`'s chat, as (name, image URL)"; db.rs `chat_emotes`:
/// "each once").
#[test]
fn a_vods_emotes_are_listed_once_each_with_their_image() {
    let app = TestApp::new();
    let (vod, other) = (a_vod(&app, 1), a_vod(&app, 2));
    let (kappa, pog, lul) = (
        "https://static-cdn.jtvnw.net/emoticons/v2/chats-kappa/default/dark/1.0",
        "https://static-cdn.jtvnw.net/emoticons/v2/chats-pog/default/dark/1.0",
        "https://static-cdn.jtvnw.net/emoticons/v2/chats-lul/default/dark/1.0",
    );
    let chat = [
        showing(5.0, "ada", &[("PogChamp", pog), ("Kappa", kappa)]),
        said(6.0, "bo", "no emote here"),
        showing(7.0, "cy", &[("Kappa", kappa)]),
    ];
    has_chat(&app, &vod, &chat);
    has_chat(&app, &other, &[showing(5.0, "di", &[("LUL", lul)])]);
    let emotes = ask(&app, "chat_emotes", json!({ "vodId": vod.id }));
    assert_eq!(emotes, json!([["Kappa", kappa], ["PogChamp", pog]]));
}

/// A chat with no emotes in it lists none.
#[test]
fn a_chat_with_no_emotes_lists_none() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    has_chat(&app, &vod, &[said(5.0, "ada", "plain words")]);
    assert_eq!(ask(&app, "chat_emotes", json!({ "vodId": vod.id })), json!([]));
}

/// The word emote providers' links.
const HOSTS: [&str; 3] = ["https://7tv.io/", "https://api.betterttv.net/", "https://api.frankerfacez.com/"];

/// The providers' links on the kit's network are the same for every test, so
/// the tests that serve them take turns.
static PROVIDERS_ANSWER: Mutex<()> = Mutex::new(());

/// A test's providers (7TV, BetterTTV, FrankerFaceZ, and Twitch's lookup of a
/// VOD's channel): they know no one until the test says so, and answer to this
/// test only for the ids it gives.
struct Providers {
    net: Net,
    asked: Arc<AtomicBool>,
    _turn: MutexGuard<'static, ()>,
}

impl Providers {
    fn new() -> Self {
        let turn = PROVIDERS_ANSWER.lock().unwrap_or_else(|e| e.into_inner());
        let net = Net::new();
        net.sites();
        // Asked before the answers below, which would end each ask.
        let asked = Arc::new(AtomicBool::new(false));
        for host in HOSTS {
            let asked = asked.clone();
            net.replies(host, move |_| {
                asked.store(true, Ordering::SeqCst);
                None
            });
            net.answers(host, 404);
        }
        Providers { net, asked, _turn: turn }
    }

    /// Twitch says VOD `video`'s channel is `channel`.
    fn knows_channel(&self, video: &str, channel: &str) {
        let (video, channel) = (video.to_string(), channel.to_string());
        self.net.replies("https://gql.twitch.tv/gql", move |request| {
            let body = request.json()?;
            let ours = body.pointer("/variables/id").and_then(Value::as_str) == Some(video.as_str());
            let owner = body["query"].as_str()?.contains("owner");
            (ours && owner).then(|| Reply::json(&json!({ "data": { "video": { "owner": { "id": channel } } } })))
        });
    }

    /// Whether any provider was asked anything.
    fn was_asked(&self) -> bool {
        self.asked.load(Ordering::SeqCst)
    }
}

/// 7TV's answer for an emote called `name`, its image `id`.
fn seven(name: &str, id: &str) -> Value {
    json!({ "name": name, "flags": 0, "data": { "host": { "url": format!("//cdn.7tv.app/emote/{id}") } } })
}

/// A word emote as the command gives it: its name and image.
fn named(emotes: &Value) -> Vec<(String, String)> {
    emotes
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["name"].as_str().unwrap().into(), e["url"].as_str().unwrap().into()))
        .collect()
}

/// With no provider turned on, no one is asked anything and none is shown
/// (chats.rs `chat_word_emotes`: "None when that's empty"; CHANGELOG.md,
/// 0.2.1: "Each is off until you turn it on, since it means contacting that
/// service").
#[test]
fn no_provider_turned_on_gives_no_word_emotes_and_asks_no_one() {
    let providers = Providers::new();
    let app = TestApp::new();
    let vod = app.add_site_vod("twitch", "9400001", "A", 100.0);
    providers.knows_channel("9400001", "7400001");
    let emotes = ask(&app, "chat_word_emotes", json!({ "vodId": vod.id, "providers": [] }));
    assert_eq!(emotes, json!([]));
    assert!(!providers.was_asked());
}

/// A Twitch VOD's chat gets its channel's emotes, then the global ones, from
/// a provider turned on (word_emotes.rs `for_vod`: "The channel's own emotes
/// come first, … then the global ones"; CHANGELOG.md, 0.2.1: "Chat can show
/// 7TV, BetterTTV and FrankerFaceZ emotes for Kick and Twitch VODs (Settings → Appearance): the
/// channel's own and the global ones").
#[test]
fn a_vods_word_emotes_are_its_channels_then_the_global_ones() {
    let providers = Providers::new();
    let app = TestApp::new();
    let vod = app.add_site_vod("twitch", "9400002", "A", 100.0);
    providers.knows_channel("9400002", "7400002");
    let account = json!({ "emote_set": { "emotes": [seven("Hype", "chan-hype")] } });
    providers.net.serves("https://7tv.io/v3/users/twitch/7400002", account.to_string());
    providers
        .net
        .serves("https://7tv.io/v3/emote-sets/global", json!({ "emotes": [seven("Wave", "glob-wave")] }).to_string());
    let emotes = ask(&app, "chat_word_emotes", json!({ "vodId": vod.id, "providers": ["7tv"] }));
    let wanted = [
        ("Hype".to_string(), "https://cdn.7tv.app/emote/chan-hype/1x.webp".to_string()),
        ("Wave".to_string(), "https://cdn.7tv.app/emote/glob-wave/1x.webp".to_string()),
    ];
    assert_eq!(named(&emotes), wanted);
}

/// Only the providers sent are used, which the view sends rather than the
/// command reading settings (chats.rs `chat_word_emotes`: "from `providers`
/// (ids, in priority order): the ones turned on in settings, which the view
/// sends rather than this reading them").
#[test]
fn only_the_providers_sent_are_used() {
    let providers = Providers::new();
    let app = TestApp::new();
    let vod = app.add_site_vod("twitch", "9400003", "A", 100.0);
    providers.knows_channel("9400003", "7400003");
    providers.net.serves(
        "https://7tv.io/v3/users/twitch/7400003",
        json!({ "emote_set": { "emotes": [seven("SevenOnly", "s")] } }).to_string(),
    );
    let bttv = json!({ "channelEmotes": [{ "code": "BttvOnly", "id": "b1" }], "sharedEmotes": [] });
    providers.net.serves("https://api.betterttv.net/3/cached/users/twitch/7400003", bttv.to_string());
    let names = |sent: Value| {
        named(&ask(&app, "chat_word_emotes", json!({ "vodId": vod.id, "providers": sent })))
            .into_iter()
            .map(|e| e.0)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(json!(["bttv"])), ["BttvOnly"]);
    assert_eq!(names(json!(["7tv"])), ["SevenOnly"]);
}

/// With several providers turned on, a VOD gets the emotes of each of them
/// (CHANGELOG.md, 0.2.1: "Chat can show 7TV, BetterTTV and FrankerFaceZ
/// emotes for Kick and Twitch VODs (Settings → Appearance)").
#[test]
fn several_providers_turned_on_all_give_their_emotes() {
    let providers = Providers::new();
    let app = TestApp::new();
    let vod = app.add_site_vod("twitch", "9400005", "A", 100.0);
    providers.knows_channel("9400005", "7400005");
    let account = json!({ "emote_set": { "emotes": [seven("SevenOne", "s1")] } });
    providers.net.serves("https://7tv.io/v3/users/twitch/7400005", account.to_string());
    let bttv = json!({ "channelEmotes": [{ "code": "BttvOne", "id": "b1" }], "sharedEmotes": [] });
    providers.net.serves("https://api.betterttv.net/3/cached/users/twitch/7400005", bttv.to_string());
    let emotes = ask(&app, "chat_word_emotes", json!({ "vodId": vod.id, "providers": ["7tv", "bttv"] }));
    let mut names: Vec<String> = named(&emotes).into_iter().map(|e| e.0).collect();
    names.sort();
    assert_eq!(names, ["BttvOne", "SevenOne"]);
}

/// Where two providers have an emote of one name, the one sent first wins
/// (chats.rs `chat_word_emotes`: "`providers` (ids, in priority order)").
#[test]
fn the_providers_order_says_whose_emote_wins_a_shared_name() {
    let providers = Providers::new();
    let app = TestApp::new();
    let vod = app.add_site_vod("twitch", "9400004", "A", 100.0);
    providers.knows_channel("9400004", "7400004");
    providers.net.serves(
        "https://7tv.io/v3/users/twitch/7400004",
        json!({ "emote_set": { "emotes": [seven("Same", "from-7tv")] } }).to_string(),
    );
    let bttv = json!({ "channelEmotes": [{ "code": "Same", "id": "frombttv" }], "sharedEmotes": [] });
    providers.net.serves("https://api.betterttv.net/3/cached/users/twitch/7400004", bttv.to_string());
    let url_of =
        |sent: Value| named(&ask(&app, "chat_word_emotes", json!({ "vodId": vod.id, "providers": sent })))[0].1.clone();
    assert_eq!(url_of(json!(["7tv", "bttv"])), "https://cdn.7tv.app/emote/from-7tv/1x.webp");
    assert_eq!(url_of(json!(["bttv", "7tv"])), "https://cdn.betterttv.net/emote/frombttv/1x");
}

/// A VOD the providers don't cover, a file or a YouTube video, has no word
/// emotes (word_emotes.rs `for_vod`: "None for a platform the providers don't
/// cover (YouTube, files)").
#[test]
fn a_vod_the_providers_do_not_cover_has_no_word_emotes() {
    let providers = Providers::new();
    let app = TestApp::new();
    let file = a_vod(&app, 1);
    let youtube = app.add_site_vod("youtube", "chatsYt00001", "A talk", 60.0);
    for vod in [&file, &youtube] {
        let emotes = ask(&app, "chat_word_emotes", json!({ "vodId": vod.id, "providers": ["7tv", "bttv", "ffz"] }));
        assert_eq!(emotes, json!([]));
    }
    assert!(!providers.was_asked());
}

// Try again.

/// Try again on a failed chat has it fetched again: it's waiting, the error
/// it failed with is gone, and the page hears the VOD change (chats.rs
/// `retry_chat`: "Fetches VOD `id`'s chat again, after a failure"; CHANGELOG.md,
/// 0.6.0 Fixes: "A chat that stops partway is kept … with Try again to fetch
/// the rest from where it stopped").
#[test]
fn try_again_has_a_failed_chat_wait_to_be_fetched_again() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    app.db().set_chat_status(vod.id, ChatStatus::Failed, Some("Twitch wouldn't give the chat: service error")).unwrap();
    ask(&app, "retry_chat", json!({ "id": vod.id }));
    let now = app.db().vod(vod.id).unwrap().unwrap();
    assert_eq!((now.chat, now.chat_error), (ChatStatus::Pending, None));
    let heard = app.heard("vod");
    let last = heard.last().unwrap();
    assert_eq!(
        (last["id"].clone(), last["chat"].clone(), last["chat_error"].clone()),
        (json!(vod.id), json!("pending"), Value::Null)
    );
}

/// A chat that failed is fetched again on Try again, and shows when it's in:
/// the fetch that failed because Twitch said the VOD wasn't there is
/// followed by one that gets the chat, and nothing of the failure stays.
#[test]
fn a_failed_chat_is_fetched_again_on_try_again_and_shows() {
    let app = TestApp::new();
    app.fetches_chats();
    let vod = app.add_site_vod("twitch", "9500001", "A", 100.0);
    let lost = Arc::new(AtomicBool::new(true));
    let still_lost = lost.clone();
    app.net.replies("https://gql.twitch.tv/gql", move |request| {
        let body = request.json()?;
        let ours = body.pointer("/variables/id").and_then(Value::as_str) == Some("9500001");
        let comments = body["query"].as_str()?.contains("comments(");
        (ours && comments && still_lost.load(Ordering::SeqCst))
            .then(|| Reply::json(&json!({ "data": { "video": null } })))
    });
    app.net.twitch_comments("9500001", &TwitchChat::of(vec![sites::twitch_comment(10.0, "ada", "hello")]));
    app.state().chat.push(vod.id);
    eventually("the first fetch fails", || app.db().vod(vod.id).unwrap().unwrap().chat == ChatStatus::Failed);
    let failed = app.db().vod(vod.id).unwrap().unwrap();
    assert!(failed.chat_error.unwrap().contains("doesn't exist"));
    lost.store(false, Ordering::SeqCst);
    ask(&app, "retry_chat", json!({ "id": vod.id }));
    eventually("the chat is fetched", || app.db().vod(vod.id).unwrap().unwrap().chat == ChatStatus::Done);
    assert_eq!(app.db().vod(vod.id).unwrap().unwrap().chat_error, None);
    let windows = ask(&app, "chat_windows", json!({ "vodId": vod.id, "ranges": [[0.0, 100.0]], "cap": 5 }));
    assert_eq!(lines(&windows, 0), [(10.0, "ada".to_string(), "hello".to_string())]);
}

// An emote's image.

/// A picture's first bytes, which is all the app looks at to know it's one.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n a picture";

/// An emote's image is given by its link on an emote provider's CDN, and kept:
/// asked again with the provider gone, it comes from the copy (chats.rs `emote`:
/// "An emote's image, by its URL on the emote provider's CDN"; emotes.rs `get`:
/// "from `dir` if it was fetched before").
#[test]
fn an_emotes_image_is_given_by_its_link_and_kept() {
    let app = TestApp::new();
    let url = format!("https://cdn.7tv.app/emote/chats-image-{}/1x.webp", std::process::id());
    let args = json!({ "url": url });
    {
        let provider = Net::new();
        provider.serves(&url, PNG);
        assert_eq!(app.invoke_bytes("emote", args.clone()).unwrap(), PNG);
    }
    // The provider answers no more (the app's own network says 404 for it).
    assert_eq!(app.invoke_bytes("emote", args).unwrap(), PNG);
}

/// A link that isn't on an emote provider's CDN is refused and not asked for
/// (emotes.rs module doc: "the UI asks for emotes by URL, and this mustn't
/// become a way to fetch anything else").
#[test]
fn a_link_off_the_emote_providers_is_refused_and_not_asked_for() {
    let app = TestApp::new();
    let url = "https://chats-elsewhere.example/emote.png";
    let asked = Arc::new(AtomicBool::new(false));
    let was = asked.clone();
    app.net.replies(url, move |_| {
        was.store(true, Ordering::SeqCst);
        Some(Reply::body(PNG))
    });
    assert_eq!(app.invoke_bytes("emote", json!({ "url": url })).unwrap_err(), "Not an emote.");
    assert!(!asked.load(Ordering::SeqCst));
}

/// What a provider's link answers that isn't an image is refused (emotes.rs
/// `get`).
#[test]
fn what_is_not_an_image_is_refused() {
    let app = TestApp::new();
    let url = format!("https://cdn.7tv.app/emote/chats-text-{}/1x.webp", std::process::id());
    app.net.serves(&url, "<html>not a picture</html>");
    assert_eq!(app.invoke_bytes("emote", json!({ "url": url })).unwrap_err(), "That emote isn't an image.");
}

// The messages a timeline layer's words match.

/// A chat for `chat_matches`: ids and times as the view gets them.
fn matches(app: &TestApp, vod: &Vod, terms: Value) -> Value {
    ask(app, "chat_matches", json!({ "vodId": vod.id, "terms": terms }))
}

/// The id of the message `author` wrote in VOD `vod`.
fn id_of(app: &TestApp, vod: &Vod, author: &str) -> i64 {
    app.db().chat(vod.id).unwrap().into_iter().find(|m| m.author == author).unwrap().id
}

/// A list of terms gets the ids and times of the messages that contain any of
/// them, in time order, the ids those `chat_windows` gives (chats.rs
/// `chat_matches`: "For each list of terms, the ids and times of VOD `vod_id`'s
/// chat messages that contain any of them"; CHANGELOG.md, 0.2.0: "Timeline
/// layers mark the moments you care about: laughter, hype and "clip it" to
/// start, or any words and emotes").
#[test]
fn a_list_of_terms_gets_the_ids_and_times_of_the_messages_with_any_of_them() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    let chat = [
        said(30.0, "cy", "LOL that"),
        said(10.0, "ada", "hype hype"),
        said(20.0, "bo", "nothing"),
        said(40.0, "di", "clip it"),
    ];
    has_chat(&app, &vod, &chat);
    let found = matches(&app, &vod, json!([["lol", "hype", "clip it"]]));
    let wanted =
        json!([[[id_of(&app, &vod, "ada"), 10.0], [id_of(&app, &vod, "cy"), 30.0], [id_of(&app, &vod, "di"), 40.0]]]);
    assert_eq!(found, wanted);
}

/// Each list of terms is answered on its own, in the order asked, so each
/// layer has its own moments.
#[test]
fn each_list_of_terms_is_answered_on_its_own_in_order() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    has_chat(&app, &vod, &[said(10.0, "ada", "lol"), said(20.0, "bo", "hype")]);
    let found = matches(&app, &vod, json!([["hype"], ["lol"], ["nothing"]]));
    assert_eq!(found, json!([[[id_of(&app, &vod, "bo"), 20.0]], [[id_of(&app, &vod, "ada"), 10.0]], []]));
}

/// A message with several of a list's terms is given once.
#[test]
fn a_message_with_several_of_the_terms_is_given_once() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    has_chat(&app, &vod, &[said(10.0, "ada", "lol hype")]);
    let found = matches(&app, &vod, json!([["lol", "hype"]]));
    assert_eq!(found, json!([[[id_of(&app, &vod, "ada"), 10.0]]]));
}

/// Terms match as search matches words: whole words, ignoring case and
/// accents (db.rs `chat_matches`: "each a whole word or phrase … matched as
/// search matches words: ignoring case and accents").
#[test]
fn terms_match_whole_words_ignoring_case_and_accents() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    has_chat(&app, &vod, &[said(10.0, "ada", "LOL"), said(20.0, "bo", "lollipop"), said(30.0, "cy", "un café")]);
    let found = matches(&app, &vod, json!([["lol"], ["cafe"]]));
    assert_eq!(found, json!([[[id_of(&app, &vod, "ada"), 10.0]], [[id_of(&app, &vod, "cy"), 30.0]]]));
}

/// A phrase matches only its words together, in order.
#[test]
fn a_phrase_matches_only_its_words_together_and_in_order() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    has_chat(
        &app,
        &vod,
        &[
            said(10.0, "ada", "that was a good game"),
            said(20.0, "bo", "game was good"),
            said(30.0, "cy", "good, not a game"),
        ],
    );
    let found = matches(&app, &vod, json!([["good game"]]));
    assert_eq!(found, json!([[[id_of(&app, &vod, "ada"), 10.0]]]));
}

/// An emote is matched by its name (db.rs `chat_matches`: "an emote by its
/// name"; CHANGELOG.md, 0.2.0: "any words and emotes").
#[test]
fn an_emote_is_matched_by_its_name() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    let image = "https://static-cdn.jtvnw.net/emoticons/v2/chats-kek/default/dark/1.0";
    has_chat(&app, &vod, &[showing(10.0, "ada", &[("KEKW", image)]), said(20.0, "bo", "nothing")]);
    let found = matches(&app, &vod, json!([["KEKW"]]));
    assert_eq!(found, json!([[[id_of(&app, &vod, "ada"), 10.0]]]));
}

/// Only the VOD asked about has matches, though another has the word too
/// (db.rs `chat_matches`: "VOD `vod_id`'s chat messages").
#[test]
fn only_the_vod_asked_about_has_matches() {
    let app = TestApp::new();
    let (first, second) = (a_vod(&app, 1), a_vod(&app, 2));
    has_chat(&app, &first, &[said(10.0, "ada", "lol")]);
    has_chat(&app, &second, &[said(20.0, "bo", "lol"), said(30.0, "cy", "lol")]);
    let found = matches(&app, &first, json!([["lol"]]));
    assert_eq!(found, json!([[[id_of(&app, &first, "ada"), 10.0]]]));
}

/// Terms with no letters or digits, and no terms at all, match nothing
/// (db.rs `chat_matches`: "Terms are alphanumeric only").
#[test]
fn terms_with_no_letters_or_digits_match_nothing() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    has_chat(&app, &vod, &[said(10.0, "ada", "lol!!"), said(20.0, "bo", "ok")]);
    assert_eq!(matches(&app, &vod, json!([[], [""], ["!!", "  "]])), json!([[], [], []]));
}

/// A term can't change the search it is part of: quotes and operators in it
/// are only punctuation (db.rs `chat_matches`: "Terms are alphanumeric only,
/// so quoting them can't inject FTS syntax").
#[test]
fn a_term_cannot_change_the_search_it_is_part_of() {
    let app = TestApp::new();
    let vod = a_vod(&app, 1);
    has_chat(&app, &vod, &[said(10.0, "ada", "ada or bo"), said(20.0, "bo", "just bo"), said(30.0, "cy", "just ada")]);
    let found = matches(&app, &vod, json!([["ada\" OR \"bo"]]));
    assert_eq!(found, json!([[[id_of(&app, &vod, "ada"), 10.0]]]));
}
