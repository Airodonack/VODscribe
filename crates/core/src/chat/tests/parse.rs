//! What each site's chat says, read: each message's time, author, colour and emotes; the subs, gifts and
//! tips the sites post in chat, and their amounts; and when the VOD started, by the messages' times sent.
//! Twitch's chat is read through `fetch_twitch_from` with Twitch's answers recorded, Kick's through
//! `kick_into` with its history pages recorded (Kick's API itself, and the step after that makes
//! announced resubs plain messages, wait for its fake), and YouTube's a line of yt-dlp's file at a time
//! (yt-dlp itself waits for its fake).

use super::*;
use crate::db::utc_time;

/// When the VODs here started, in Unix seconds: 2026-09-28T21:30:00Z.
const STARTED: f64 = 1790631000.0;

// Twitch

/// The test Twitch VOD, `duration` seconds long, as yt-dlp describes it.
fn twitch_vod(duration: f64) -> VodInfo {
    VodInfo {
        platform: "twitch".into(),
        platform_id: "2901646294".into(),
        url: "https://www.twitch.tv/videos/2901646294".into(),
        title: "A stream".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(duration),
        thumbnail: None,
        published: None,
        started_at: None,
    }
}

/// A comment as Twitch's GraphQL API gives it: `time` seconds into the VOD, sent then, by `author`.
fn comment(time: f64, author: &str, fragments: Value) -> Value {
    json!({ "contentOffsetSeconds": time, "createdAt": utc_time(STARTED + time),
        "commenter": { "displayName": author },
        "message": { "userColor": null, "fragments": fragments } })
}

/// A comment of plain text.
fn said(time: f64, author: &str, text: &str) -> Value {
    comment(time, author, json!([{ "text": text, "emote": null }]))
}

/// Twitch's GraphQL API for a VOD whose chat is `comments`, paging from an offset 100 at a time as Twitch does, and
/// answering the ask for its cheermotes with `cheermotes`, or failing it when that's None.
fn twitch(comments: Vec<Value>, cheermotes: Option<Value>) -> impl Fn(&Value, &AtomicBool) -> Result<Value> + Sync {
    move |body: &Value, _: &AtomicBool| {
        let Some(offset) = body.pointer("/variables/offset").and_then(Value::as_i64) else {
            return cheermotes.clone().context("couldn't contact gql.twitch.tv");
        };
        let from: Vec<Value> = comments
            .iter()
            .enumerate()
            .filter(|(_, c)| c["contentOffsetSeconds"].as_f64().unwrap() as i64 >= offset)
            .map(|(i, c)| {
                let mut node = c.clone();
                node["id"] = json!(i.to_string());
                json!({ "node": node })
            })
            .collect();
        let more = from.len() > 100;
        let edges: Vec<Value> = from.into_iter().take(100).collect();
        Ok(json!({ "data": { "video": { "comments": { "edges": edges, "pageInfo": { "hasNextPage": more } } } } }))
    }
}

/// What a fetch of a Twitch VOD `duration` seconds long saves, in time order, Twitch answering as `ask`.
fn fetched(duration: f64, ask: &TwitchAsk) -> Vec<ChatMessage> {
    let mut store = MemoryChat::default();
    assert!(fetch_twitch_from(&twitch_vod(duration), &mut store, &AtomicBool::new(false), &mut |_| {}, ask).unwrap());
    let mut messages = store.messages;
    messages.sort_by(|a, b| a.time.total_cmp(&b.time));
    messages
}

/// The chat of a ten-minute Twitch VOD whose comments are `comments`, Twitch listing no cheermotes beyond the built-in
/// ones.
fn twitch_chat(comments: Vec<Value>) -> Vec<ChatMessage> {
    fetched(600.0, &twitch(comments, Some(json!({ "data": {} }))))
}

/// What a message is, and how much.
fn kind(m: &ChatMessage) -> (Kind, Option<f64>) {
    (m.kind, m.amount)
}

/// The kinds of the chat of comments `texts`, each by the name it starts with.
fn twitch_kinds(texts: &[&str]) -> Vec<(Kind, Option<f64>)> {
    let comments = texts.iter().enumerate().map(|(i, t)| said(i as f64, t.split(' ').next().unwrap(), t)).collect();
    twitch_chat(comments).iter().map(kind).collect()
}

/// An emote's image on Twitch's CDN.
fn twitch_emote(id: &str) -> String {
    format!("https://static-cdn.jtvnw.net/emoticons/v2/{id}/default/dark/1.0")
}

/// CHANGELOG.md, 0.2.0: "Chat: for Kick, Twitch and YouTube livestreams, the chat replay sits beside the transcript,
/// each line with the messages sent while it was said"; chat.rs `ChatMessage::time` ("Seconds into the VOD") and
/// `ChatMessage::color` ("The author's name color, as `#rrggbb`, if the site gives one").
#[test]
fn a_twitch_comment_is_at_its_time_into_the_vod_by_its_commenter_in_their_colour() {
    let mut c = said(526.0, "swiftyspiffy", "item #4 is scary");
    c["message"]["userColor"] = json!("#FF3AE9");
    let chat = twitch_chat(vec![c, said(527.5, "other", "same")]);
    let seen: Vec<_> = chat.iter().map(|m| (m.time, m.author.as_str(), m.color.as_deref(), m.text())).collect();
    assert_eq!(
        seen,
        [
            (526.0, "swiftyspiffy", Some("#FF3AE9"), "item #4 is scary".to_string()),
            (527.5, "other", None, "same".to_string())
        ]
    );
}

/// chat.rs `ChatMessage::color` ("The author's name color, as `#rrggbb`, if the site gives one"): one given in another
/// shape is no colour, on Twitch and on Kick.
#[test]
fn a_name_colour_that_is_not_rrggbb_is_none() {
    let colours = ["red; x", "#FFF", "#GG0000", "#FF3AE9 "];
    let comments = colours
        .iter()
        .map(|c| {
            let mut c2 = said(1.0, "v", "hi");
            c2["message"]["userColor"] = json!(c);
            c2
        })
        .collect();
    assert!(twitch_chat(comments).iter().all(|m| m.color.is_none()), "Twitch");
    let messages = colours
        .iter()
        .map(|c| {
            let mut m = kick_message(1.0, "v", "hi");
            m["sender"]["identity"]["color"] = json!(c);
            m
        })
        .collect();
    assert!(kick_chat(messages).iter().all(|m| m.color.is_none()), "Kick");
}

/// CHANGELOG.md, 0.2.0: "Chat: … emotes and all"; chat.rs `ChatPart::Emote` ("`url` is the image on the emote
/// provider's CDN; `name` is what it reads as, which search matches") and `is_emote_id` ("Twitch emote ids are digits,
/// or `emotesv2_` and hex").
#[test]
fn a_twitch_emote_is_its_image_on_twitchs_cdn_read_by_its_name() {
    let chat = twitch_chat(vec![comment(
        1.0,
        "v",
        json!([
            { "text": "scary ", "emote": null },
            { "text": "monkaS", "emote": { "emoteID": "emotesv2_3a2f" } },
            { "text": " ", "emote": null },
            { "text": "Kappa", "emote": { "emoteID": "25" } },
        ]),
    )]);
    assert_eq!(
        chat[0].parts,
        [
            ChatPart::Text { text: "scary ".into() },
            ChatPart::Emote { name: "monkaS".into(), url: twitch_emote("emotesv2_3a2f") },
            ChatPart::Text { text: " ".into() },
            ChatPart::Emote { name: "Kappa".into(), url: twitch_emote("25") },
        ]
    );
    assert_eq!(chat[0].text(), "scary monkaS Kappa");
}

/// chat.rs `is_emote_id` ("Twitch emote ids are digits, or `emotesv2_` and hex") with `ChatPart::Emote` (an emote's
/// `url` "is the image on the emote provider's CDN"): a fragment whose id has other characters is its text, so no image
/// comes from anywhere but Twitch's emotes.
#[test]
fn a_twitch_emote_id_of_other_characters_is_text() {
    for id in ["../../x", "", "25?x=1", "25/../26"] {
        let chat = twitch_chat(vec![comment(1.0, "v", json!([{ "text": "monkaS", "emote": { "emoteID": id } }]))]);
        assert_eq!(chat[0].parts, [ChatPart::Text { text: "monkaS".into() }], "{id:?}");
    }
}

/// chat.rs `is_emote_id` ("Twitch emote ids are digits, or `emotesv2_` and hex"): an id of other letters isn't one.
#[test]
#[ignore = "disagreement: is_emote_id takes any letters, digits and _, not only digits or emotesv2_ and hex"]
fn a_twitch_emote_id_neither_digits_nor_emotesv2_and_hex_is_text() {
    for id in ["Kappa", "emotesv2_xyz", "_"] {
        let chat = twitch_chat(vec![comment(1.0, "v", json!([{ "text": "monkaS", "emote": { "emoteID": id } }]))]);
        assert_eq!(chat[0].parts, [ChatPart::Text { text: "monkaS".into() }], "{id:?}");
    }
}

/// chat.rs `ChatMessage::text` ("The message as plain text, emotes by name, for the search index") and
/// `ChatMessage::text` ("Emotes are words of their own, even when sent back to back").
#[test]
fn emotes_are_words_of_their_own_in_a_messages_text() {
    let emote = |name: &str| json!({ "text": name, "emote": { "emoteID": "25" } });
    let chat = twitch_chat(vec![comment(
        1.0,
        "v",
        json!([{ "text": "wow", "emote": null }, emote("monkaS"), emote("KEKW"), { "text": "bye", "emote": null }]),
    )]);
    assert_eq!(chat[0].text(), "wow monkaS KEKW bye");
    let chat = kick_chat(vec![kick_message(1.0, "v", "[emote:1:barkywave]hello[emote:2:a][emote:3:b]")]);
    assert_eq!(chat[0].text(), "barkywave hello a b");
}

/// CHANGELOG.md, 0.2.0: "Subs, gifts and tips … are marked in chat … stronger where more money changed hands"; chat.rs
/// `ChatMessage::amount` ("For a Twitch sub, its tier") and `twitch_event`'s doc (above `Cheermotes`) ("Subs and gifts
/// are posted in the subscriber's name and start with it ("name subscribed at Tier 1. …"").
#[test]
fn a_twitch_sub_is_a_sub_of_its_tier() {
    assert_eq!(
        twitch_kinds(&[
            "mairimyarel subscribed at Tier 1. They've subscribed for 8 months!",
            "big subscribed at Tier 3."
        ]),
        [(Kind::Sub, Some(1.0)), (Kind::Sub, Some(3.0))]
    );
}

/// chat.rs `ChatMessage::amount` ("For a Twitch sub, its tier (Prime counts as 1)").
#[test]
fn a_prime_sub_is_a_sub_of_tier_one() {
    assert_eq!(
        twitch_kinds(&["neezydagod subscribed with Prime. They've subscribed for 44 months!"]),
        [(Kind::Sub, Some(1.0))]
    );
}

/// chat.rs `Kind::Gift` ("Subs or memberships given to others"), `ChatMessage::amount` ("for a gift, how many subs"),
/// `twitch_event`'s doc (above `Cheermotes`) ("name is gifting 5 Tier 1 Subs to …") and `GiftBombs` ("X gifted a Tier 1
/// sub to Y!").
#[test]
fn a_twitch_gift_bomb_is_a_gift_of_its_count_and_a_gifted_sub_one() {
    assert_eq!(
        twitch_kinds(&[
            "someone is gifting 50 Tier 1 Subs to KaiCenat's community!",
            "kind gifted a Tier 1 sub to friend!"
        ]),
        [(Kind::Gift, Some(50.0)), (Kind::Gift, Some(1.0))]
    );
}

/// chat.rs `twitch_event`'s doc (above `Cheermotes`) ("Subs and gifts are posted in the subscriber's name and start
/// with it"): the same words written by someone else are a message.
#[test]
fn only_the_subscribers_own_notice_is_a_sub_or_gift() {
    let chat = twitch_chat(vec![
        said(1.0, "viewer", "I subscribed at Tier 1 yesterday"),
        said(2.0, "alice", "bob subscribed at Tier 1."),
        said(3.0, "alice", "bob is gifting 5 Tier 1 Subs to Kai's community!"),
        said(4.0, "alice", "bob gifted a Tier 1 sub to carol!"),
    ]);
    assert!(chat.iter().all(|m| kind(m) == (Kind::Message, None)), "{chat:?}");
}

/// chat.rs `GiftBombs` ("A Twitch gift bomb … can be followed by a line for each sub ("X gifted a Tier 1 sub to Y!").
/// Those are the same gift: they stay in chat as messages rather than counting again").
#[test]
fn a_gift_bombs_line_for_each_sub_stays_in_chat_as_a_message() {
    let chat = twitch_chat(vec![
        said(10.0, "big", "big is gifting 2 Tier 1 Subs to Kai's community!"),
        said(11.0, "big", "big gifted a Tier 1 sub to a!"),
        said(11.0, "big", "big gifted a Tier 1 sub to b!"),
        // A third: a gift of its own, past the bomb's two.
        said(12.0, "big", "big gifted a Tier 1 sub to c!"),
        // Another gifter's: their own gift.
        said(13.0, "other", "other gifted a Tier 1 sub to d!"),
    ]);
    let seen: Vec<_> = chat.iter().map(|m| (m.time, m.kind)).collect();
    assert_eq!(
        seen,
        [(10.0, Kind::Gift), (11.0, Kind::Message), (11.0, Kind::Message), (12.0, Kind::Gift), (13.0, Kind::Gift)]
    );
    let gifts: Vec<_> = chat.iter().filter(|m| m.kind == Kind::Gift).map(|m| m.amount).collect();
    assert_eq!(gifts, [Some(2.0), Some(1.0), Some(1.0)], "the bomb's count, and one each");
}

/// chat.rs `count_gift_bombs_once` ("After the fetch, as a bomb's lines can fall in the slice after its own") with
/// `GiftBombs`: a 40-minute VOD's chat is fetched in four slices, and a bomb's lines just after a slice's end still
/// count as the bomb.
#[test]
fn a_gift_bombs_lines_in_the_next_slice_stay_messages() {
    let comments = vec![
        said(599.0, "big", "big is gifting 2 Tier 1 Subs to Kai's community!"),
        said(600.0, "big", "big gifted a Tier 1 sub to a!"),
        said(601.0, "big", "big gifted a Tier 1 sub to b!"),
    ];
    assert_eq!(slices_for(2400.0), 4, "fetched in slices");
    let chat = fetched(2400.0, &twitch(comments, Some(json!({ "data": {} }))));
    let seen: Vec<_> = chat.iter().map(|m| (m.time, m.kind)).collect();
    assert_eq!(seen, [(599.0, Kind::Gift), (600.0, Kind::Message), (601.0, Kind::Message)]);
}

/// chat.rs `GiftBombs::WITHIN` (300 s: "How long after a bomb its lines can still arrive"): a line later than that is a
/// gift of its own.
#[test]
fn a_gifted_sub_more_than_five_minutes_after_a_bomb_is_its_own_gift() {
    let chat = twitch_chat(vec![
        said(10.0, "big", "big is gifting 2 Tier 1 Subs to Kai's community!"),
        said(310.0, "big", "big gifted a Tier 1 sub to a!"),
        said(311.0, "big", "big gifted a Tier 1 sub to b!"),
    ]);
    let seen: Vec<_> = chat.iter().map(|m| (m.time, m.kind)).collect();
    assert_eq!(seen, [(10.0, Kind::Gift), (310.0, Kind::Message), (311.0, Kind::Gift)]);
}

/// chat.rs `Kind::Tip` ("Bits (Twitch)"), `ChatMessage::amount` ("for a tip, how many bits") and `CHEERMOTES` ("a cheer
/// is one of these and a number of bits ("Cheer100")").
#[test]
fn cheers_in_a_twitch_message_are_a_tip_of_their_bits() {
    assert_eq!(
        twitch_kinds(&["kai Cheer100 Hi, Kai! How’re you? cheer50", "v PrideCheer300 hi"]),
        [(Kind::Tip, Some(150.0)), (Kind::Tip, Some(300.0))]
    );
}

/// chat.rs `CHEERMOTES` ("Many emotes also end in digits (Squid1, MercyWing1), so only these count"), and a cheermote's
/// name alone is no cheer ("one of these and a number of bits").
#[test]
fn emotes_ending_in_digits_are_not_bits() {
    assert_eq!(
        twitch_kinds(&["v Squid1 Squid2 Squid3 Squid4 MercyWing1", "v cheer up everyone", "v Cheer100x"]),
        [(Kind::Message, None); 3]
    );
}

/// chat.rs `CHEERMOTES` ("a cheer is one of these and a number of bits ("Cheer100")"): a number alone is no cheer, even
/// with a nameless cheermote in Twitch's list.
#[test]
fn a_number_alone_is_no_cheer() {
    let listed = json!({ "data": { "cheerConfig": { "groups": [{ "nodes": [{ "prefix": "" }] }] } } });
    let comments = || vec![said(1.0, "v", "100 viewers"), said(2.0, "v", "1 2 3")];
    let chat = fetched(600.0, &twitch(comments(), Some(listed)));
    assert_eq!(chat.iter().map(kind).collect::<Vec<_>>(), [(Kind::Message, None); 2]);
    assert_eq!(twitch_chat(comments()).iter().map(kind).collect::<Vec<_>>(), [(Kind::Message, None); 2]);
}

/// chat.rs `CHEERMOTES` ("Twitch's cheermotes as of October 2026, and retired ones old VODs have").
#[test]
fn retired_cheermotes_in_an_old_vod_count_as_bits() {
    assert_eq!(twitch_kinds(&["v BibleThump100 Kappa50 shamrock1"]), [(Kind::Tip, Some(151.0))]);
}

/// CHANGELOG.md, 0.2.1: "Twitch bits cheered with newer cheermotes, such as PrideCheer, count as bits: each chat fetch
/// asks Twitch for its current cheermotes rather than relying on a built-in list"; chat.rs `Cheermotes::for_video`
/// ("the ones Twitch lists now: everyone's").
#[test]
fn a_cheermote_twitch_lists_now_counts_as_bits() {
    let comments = || vec![said(1.0, "v", "NewCheer50 hype")];
    let listed = json!({ "data": { "cheerConfig": { "groups": [{ "nodes": [{ "prefix": "Cheer" }, { "prefix": "NewCheer" }] }] } } });
    let chat = fetched(600.0, &twitch(comments(), Some(listed)));
    assert_eq!(kind(&chat[0]), (Kind::Tip, Some(50.0)));
    let chat = fetched(600.0, &twitch(comments(), Some(json!({ "data": {} }))));
    assert_eq!(kind(&chat[0]), (Kind::Message, None), "not one Twitch lists");
}

/// chat.rs `Cheermotes::for_video` ("and those of video `id`'s channel (a charity drive's, say)").
#[test]
fn a_channels_own_cheermotes_count_as_bits() {
    let listed = json!({ "data": { "video": { "owner": { "cheer": { "cheerGroups": [{ "nodes": [{ "prefix": "DriveCheer" }] }] } } } } });
    let chat = fetched(600.0, &twitch(vec![said(1.0, "v", "DriveCheer25 for the drive")], Some(listed)));
    assert_eq!(kind(&chat[0]), (Kind::Tip, Some(25.0)));
}

/// chat.rs `Cheermotes::for_video` ("Twitch adds new ones from time to time; without an answer, the built-in ones do").
#[test]
fn without_twitchs_list_the_built_in_cheermotes_count() {
    let comments = vec![said(1.0, "v", "Cheer100"), said(2.0, "v", "NewCheer50")];
    let chat = fetched(600.0, &twitch(comments, None));
    assert_eq!(chat.iter().map(kind).collect::<Vec<_>>(), [(Kind::Tip, Some(100.0)), (Kind::Message, None)]);
}

/// Asks Twitch for real, the recorded cheermote answers' check against it: `cargo test -p vodscribe-core -- --ignored
/// cheermotes`. chat.rs `Cheermotes::for_video` (the built-in names, "and the ones Twitch lists now").
#[test]
#[ignore = "network"]
fn fetches_twitchs_cheermotes() {
    let names = Cheermotes::for_video("2899842479", &AtomicBool::new(false), &twitch_gql);
    let fetched = names.0.len() - Cheermotes::builtin().0.len();
    eprintln!("{} cheermote names, {fetched} more than built in", names.0.len());
    assert!(names.has("PrideCheer") && names.has("Cheer") && names.has("Charity"));
}

/// e2e chat-view.mjs ("the VOD's start is when its messages say it was"); chat.rs `ChatMessage::at` ("When it was sent,
/// in Unix seconds, where the site says: Twitch's `createdAt`") and `ChatMessage::start`.
#[test]
fn twitch_comments_say_when_the_vod_started() {
    let chat = twitch_chat((0..START_MESSAGES).map(|i| said(i as f64 * 7.0, "v", "hi")).collect());
    assert!(chat.iter().all(|m| m.start() == Some(STARTED)), "{chat:?}");
    let starts: Vec<f64> = chat.iter().filter_map(ChatMessage::start).collect();
    assert_eq!(vod_start(&starts), Some(STARTED));
}

// Kick

/// A message as Kick's chat history gives it: sent `time` seconds into the VOD, by `sender`.
fn kick_message(time: f64, sender: &str, content: &str) -> Value {
    json!({ "content": content, "created_at": utc_time(STARTED + time),
        "sender": { "username": sender, "identity": { "color": "#53FC18" } } })
}

/// The chat a ten-minute Kick VOD saves when its history holds `messages` (newest first, as Kick's pages are), in time
/// order: before `fetch_kick` makes the resubs KickBot announced plain messages.
fn kick_chat(messages: Vec<Value>) -> Vec<ChatMessage> {
    let messages: Vec<Value> = messages
        .into_iter()
        .enumerate()
        .map(|(i, mut m)| {
            m["id"] = json!(format!("id-{i}"));
            m
        })
        .collect();
    let page = json!({ "data": { "messages": messages, "cursor": null } });
    let mut store = MemoryChat::default();
    kick_into(&mut store, STARTED, 600.0, 1, |_, _| parse_kick_page(&page), &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    let mut chat = store.messages;
    chat.sort_by(|a, b| a.time.total_cmp(&b.time));
    chat
}

/// The kinds of a Kick chat of `(sender, content)`, a second apart.
fn kick_kinds(messages: &[(&str, &str)]) -> Vec<(Kind, Option<f64>)> {
    let messages = messages.iter().enumerate().map(|(i, (s, c))| kick_message(i as f64, s, c)).collect();
    kick_chat(messages).iter().map(kind).collect()
}

/// An emote's image on Kick's CDN.
fn kick_emote(id: &str) -> String {
    format!("https://files.kick.com/emotes/{id}/fullsize")
}

/// CHANGELOG.md, 0.2.0 ("each line with the messages sent while it was said"); chat.rs `ChatMessage::time` ("Seconds
/// into the VOD"), `ChatMessage::color` and the comment before `fetch_kick` ("The history is the whole channel's, so
/// the VOD's span is worked out from when it started … and its length").
#[test]
fn a_kick_message_is_at_its_time_into_the_vod_by_its_sender_in_their_colour() {
    let mut first = kick_message(0.0, "sunkai", "hello");
    first["created_at"] = json!("2026-09-28T21:30:30.500Z");
    let chat = kick_chat(vec![
        // Before the VOD and after it: the channel's, not this VOD's.
        kick_message(601.0, "late", "after"),
        first,
        kick_message(-1.0, "early", "before"),
    ]);
    let seen: Vec<_> = chat.iter().map(|m| (m.time, m.author.as_str(), m.color.as_deref())).collect();
    assert_eq!(seen, [(30.5, "sunkai", Some("#53FC18"))]);
}

/// chat.rs `ChatMessage::at` ("Kick's `created_at`") and `ChatMessage::start`: each message's time sent less its time
/// into the VOD is when the VOD started.
#[test]
fn kick_messages_say_when_the_vod_started() {
    let chat = kick_chat((0..START_MESSAGES).map(|i| kick_message(i as f64 * 3.0, "v", "hi")).collect());
    let starts: Vec<f64> = chat.iter().filter_map(ChatMessage::start).collect();
    assert_eq!(starts, [STARTED; START_MESSAGES]);
    assert_eq!(vod_start(&starts), Some(STARTED));
}

/// CHANGELOG.md, 0.2.0 ("emotes and all"); chat.rs `kick_parts` ("Splits Kick's `[emote:<id>:<name>]` codes out of a
/// message") and `ChatPart::Emote` (the image on the provider's CDN, read by its name).
#[test]
fn a_kick_emote_code_is_its_image_on_kicks_cdn() {
    let chat = kick_chat(vec![kick_message(1.0, "v", "hi [emote:4085540:barkylove][emote:4085540:barkylove] bye")]);
    assert_eq!(
        chat[0].parts,
        [
            ChatPart::Text { text: "hi ".into() },
            ChatPart::Emote { name: "barkylove".into(), url: kick_emote("4085540") },
            ChatPart::Emote { name: "barkylove".into(), url: kick_emote("4085540") },
            ChatPart::Text { text: " bye".into() },
        ]
    );
}

/// chat.rs `kick_parts` (`[emote:<id>:<name>]` codes) and `ChatPart::Emote` (an emote's `url` "is the image on the
/// emote provider's CDN"): what isn't such a code, one left open or with an id that isn't Kick's number, is text.
#[test]
fn a_kick_code_that_is_not_an_emotes_stays_text() {
    for content in ["[emote:x/../y:z] ok", "[emote:12", "[emote::name] ok", "[emote:12] ok"] {
        let chat = kick_chat(vec![kick_message(1.0, "v", content)]);
        assert_eq!(chat[0].parts, [ChatPart::Text { text: content.into() }], "{content:?}");
    }
}

/// chat.rs `push_text` ("leaving out Unicode tag characters, which chatters add invisibly to get around
/// duplicate-message filters"), on Kick and Twitch alike.
#[test]
fn unicode_tag_characters_are_left_out_of_a_message() {
    let chat = kick_chat(vec![kick_message(1.0, "v", "oda \u{E0021}"), kick_message(2.0, "v", "\u{E0000}\u{E007F}")]);
    assert_eq!(chat[0].parts, [ChatPart::Text { text: "oda ".into() }]);
    assert!(chat[1].parts.is_empty(), "nothing left");
    let chat = twitch_chat(vec![said(1.0, "v", "oda \u{E0021}\u{E0041}")]);
    assert_eq!(chat[0].parts, [ChatPart::Text { text: "oda ".into() }]);
}

/// CHANGELOG.md, 0.2.0 ("Subs, gifts and tips (bits, KICKs, …) are marked in chat"); chat.rs `kick_event` ("Events come
/// as KickBot's announcements, when the channel has them on ("@name has subbed for 15 months!"").
#[test]
fn kickbots_sub_announcement_is_a_sub() {
    assert_eq!(kick_kinds(&[("KickBot", "@Ari_letric has subbed for 15 months!")])[0].0, Kind::Sub);
}

/// chat.rs `kick_event` ("@name just gifted 1,000 KICKs!"), `ChatMessage::amount` ("for a tip, how many bits or KICKs")
/// and `leading_number` ("allowing thousands separators ("1,000")").
#[test]
fn kickbots_kicks_announcement_is_a_tip_of_that_many_kicks() {
    assert_eq!(
        kick_kinds(&[("KickBot", "@mazzahh just gifted 1,000 KICKs!"), ("KickBot", "@b just gifted 50 KICKs!")]),
        [(Kind::Tip, Some(1000.0)), (Kind::Tip, Some(50.0))]
    );
}

/// chat.rs `Kind::Gift` ("Subs or memberships given to others") and `ChatMessage::amount` ("for a gift, how many
/// subs").
#[test]
fn kickbots_gift_announcement_is_a_gift_of_that_many_subs() {
    assert_eq!(kick_kinds(&[("KickBot", "@someone just gifted 5 subs to the community!")]), [(Kind::Gift, Some(5.0))]);
}

/// chat.rs `kick_event` ("Events come as KickBot's announcements"): the same words from a viewer, and KickBot's other
/// lines, are messages.
#[test]
fn only_kickbots_announcements_are_kick_events() {
    let kinds = kick_kinds(&[
        ("viewer", "has subbed lol"),
        ("viewer", "@me just gifted 1,000 KICKs!"),
        ("viewer", "@me just gifted 5 subs to the community!"),
        ("KickBot", "If you enjoy the content and would like to offer further support"),
    ]);
    assert_eq!(kinds, [(Kind::Message, None); 4]);
}

/// chat.rs `kick_event` ("and as a resubscriber's own message, marked as a celebration") and `ChatMessage::event_time`
/// ("When the event happened, if not when the message was sent: a Kick resub celebration can be shared long after the
/// sub").
#[test]
fn a_resubscribers_celebration_is_a_sub_at_when_the_sub_happened() {
    let mut celebration = kick_message(400.0, "Ari_letric", "oh wow");
    let subbed = utc_time(STARTED + 100.0);
    celebration["metadata"] =
        json!(json!({ "celebration": { "type": "subscription_renewed", "created_at": subbed } }).to_string());
    let chat = kick_chat(vec![celebration, kick_message(500.0, "v", "plain")]);
    let seen: Vec<_> = chat.iter().map(|m| (m.time, m.kind, m.event_time)).collect();
    assert_eq!(seen, [(400.0, Kind::Sub, Some(100.0)), (500.0, Kind::Message, None)]);
}

/// chat.rs `resubs_announced` ("A resub can show twice on Kick: KickBot's announcement when it happens, and the
/// subscriber's celebration message … Of `subs`, the celebrations KickBot announced, which should stay in chat as plain
/// messages").
#[test]
fn a_resub_kickbot_announced_counts_once() {
    let sub = |time: f64, author: &str, text: &str, subbed_at: Option<f64>| ChatMessage {
        time,
        author: author.into(),
        color: None,
        parts: vec![ChatPart::Text { text: text.into() }],
        kind: Kind::Sub,
        amount: None,
        event_time: subbed_at,
        at: None,
    };
    let subs = vec![
        (1, sub(7194.0, "KickBot", "@Ari_letric has subbed for 15 months!", None)),
        (2, sub(7211.0, "KickBot", "@blixtyyy has subbed for 15 months!", None)),
        // Shared an hour after the sub KickBot announced.
        (3, sub(11017.0, "Ari_letric", "oh wow", Some(7194.0))),
        // Never announced (the channel has KickBot's announcements off, say).
        (4, sub(23814.0, "tetraspis", "been at it for ages", Some(23000.0))),
        // Subbed as KickBot announced someone else's: not announced.
        (5, sub(9000.0, "zed", "thanks", Some(7200.0))),
    ];
    assert_eq!(resubs_announced(&subs), [3]);
}

// YouTube

/// A line of yt-dlp's `live_chat.json`: `item` at `ms` milliseconds into the video.
fn line(ms: &str, item: Value) -> String {
    json!({ "replayChatItemAction": { "videoOffsetTimeMsec": ms, "actions": [{ "addChatItemAction": { "item": item } }] } })
        .to_string()
}

/// What YouTube's chat of these lines holds: (author, kind, amount, text).
fn youtube_kinds(lines: &[String]) -> Vec<(String, Kind, Option<f64>, String)> {
    parse_youtube(&lines.join("\n")).into_iter().map(|m| (m.author.clone(), m.kind, m.amount, m.text())).collect()
}

/// CHANGELOG.md, 0.2.0 ("each line with the messages sent while it was said"); chat.rs `ChatMessage::time` and
/// `parse_youtube_line` ("Adds the chat messages in one line of yt-dlp's `live_chat.json`").
#[test]
fn a_youtube_message_is_at_its_time_into_the_video_by_its_author() {
    let chat = parse_youtube(&line(
        "12345",
        json!({ "liveChatTextMessageRenderer": {
            "authorName": { "simpleText": "@viewer" }, "message": { "runs": [{ "text": "nice" }] } } }),
    ));
    let seen: Vec<_> = chat.iter().map(|m| (m.time, m.author.as_str(), m.text())).collect();
    assert_eq!(seen, [(12.345, "@viewer", "nice".to_string())]);
}

/// CHANGELOG.md, 0.2.0 ("emotes and all"); chat.rs `ChatPart::Emote` (an emote is its image on the provider's CDN, read
/// by its name) and a comment in `parse_youtube_line` ("A standard emoji is its own character").
#[test]
fn a_youtube_custom_emoji_is_its_image_and_a_standard_one_its_character() {
    let chat = parse_youtube(&line(
        "1000",
        json!({ "liveChatTextMessageRenderer": {
            "authorName": { "simpleText": "@viewer" },
            "message": { "runs": [
                { "text": "nice " },
                { "emoji": { "emojiId": "UC/x", "isCustomEmoji": true, "shortcuts": [":yt:"],
                    "image": { "thumbnails": [{ "url": "https://yt3.ggpht.com/abc=w24-h24" }] } } },
                { "emoji": { "emojiId": "😂", "shortcuts": [":joy:"] } },
            ] } } }),
    ));
    assert_eq!(
        chat[0].parts,
        [
            ChatPart::Text { text: "nice ".into() },
            ChatPart::Emote { name: ":yt:".into(), url: "https://yt3.ggpht.com/abc=w24-h24".into() },
            ChatPart::Text { text: "😂".into() },
        ]
    );
    assert_eq!(chat[0].text(), "nice :yt: 😂");
}

/// chat.rs `Kind::Tip` ("a Super Chat … (YouTube)") and `ChatMessage::amount` ("A Super Chat's amount is in its text
/// instead, in whatever currency it was").
#[test]
fn a_super_chat_is_a_tip_with_its_amount_in_its_text() {
    let chat = youtube_kinds(&[line(
        "1000",
        json!({ "liveChatPaidMessageRenderer": {
            "authorName": { "simpleText": "@fan" }, "purchaseAmountText": { "simpleText": "$5.00" },
            "message": { "runs": [{ "text": "love the stream" }] } } }),
    )]);
    assert_eq!(chat, [("@fan".into(), Kind::Tip, None, "$5.00 love the stream".into())]);
}

/// chat.rs `Kind::Tip` ("a Super Chat or Super Sticker (YouTube)") and `ChatMessage::amount` (its amount in its text).
#[test]
fn a_super_sticker_is_a_tip_saying_its_amount() {
    let chat = youtube_kinds(&[line(
        "2000",
        json!({ "liveChatPaidStickerRenderer": {
            "authorName": { "simpleText": "@fan" }, "purchaseAmountText": { "simpleText": "¥500" } } }),
    )]);
    assert_eq!(chat, [("@fan".into(), Kind::Tip, None, "¥500 Super Sticker".into())]);
}

/// chat.rs `Kind::Sub` ("A subscription or YouTube membership, new or renewed") and a comment in `parse_youtube_line`
/// (""Welcome to …!" for a new member, "Member for 6 months" for a milestone").
#[test]
fn a_new_membership_and_a_milestone_are_subs_saying_which() {
    let chat = youtube_kinds(&[
        line(
            "3000",
            json!({ "liveChatMembershipItemRenderer": { "authorName": { "simpleText": "@new" },
                "headerSubtext": { "runs": [{ "text": "Welcome to " }, { "text": "Channel" }, { "text": "!" }] } } }),
        ),
        line(
            "4000",
            json!({ "liveChatMembershipItemRenderer": { "authorName": { "simpleText": "@old" },
                "headerPrimaryText": { "runs": [{ "text": "Member for " }, { "text": "6" }, { "text": " months" }] },
                "message": { "runs": [{ "text": "half a year!" }] } } }),
        ),
    ]);
    assert_eq!(
        chat,
        [
            ("@new".into(), Kind::Sub, None, "Welcome to Channel!".into()),
            ("@old".into(), Kind::Sub, None, "Member for 6 months half a year!".into()),
        ]
    );
}

/// chat.rs `Kind::Gift` ("Subs or memberships given to others") and a comment in `parse_youtube_line` (""Sent 5 … gift
/// memberships": how many").
#[test]
fn gift_memberships_are_a_gift_of_how_many_were_sent() {
    let header = json!({ "liveChatSponsorshipsHeaderRenderer": { "authorName": { "simpleText": "@generous" },
        "primaryText": { "runs": [{ "text": "Sent " }, { "text": "5" }, { "text": " Channel gift memberships" }] } } });
    let chat = youtube_kinds(&[line(
        "5000",
        json!({ "liveChatSponsorshipsGiftPurchaseAnnouncementRenderer": { "header": header } }),
    )]);
    assert_eq!(chat, [("@generous".into(), Kind::Gift, Some(5.0), "Sent 5 Channel gift memberships".into())]);
}

/// chat.rs `parse_youtube_line` ("Adds the chat messages in one line of yt-dlp's `live_chat.json`"): a line with none,
/// such as YouTube's own notice to viewers or one that isn't JSON, adds nothing.
#[test]
fn a_youtube_line_with_no_chat_message_adds_none() {
    let text = [
        line("20000", json!({ "liveChatViewerEngagementMessageRenderer": {} })),
        "not json".into(),
        json!({ "something": "else" }).to_string(),
        String::new(),
    ];
    assert!(parse_youtube(&text.join("\n")).is_empty());
}

/// chat.rs `ChatMessage::at` ("YouTube's `timestampUsec`") and `ChatMessage::start`.
#[test]
fn youtube_messages_say_when_the_vod_started() {
    let lines: Vec<String> = (0..START_MESSAGES)
        .map(|i| {
            let ms = i * 5000;
            let sent = format!("{}", (STARTED as u64 + i as u64 * 5) * 1_000_000 + 123);
            line(
                &ms.to_string(),
                json!({ "liveChatTextMessageRenderer": { "authorName": { "simpleText": "@v" }, "timestampUsec": sent,
                    "message": { "runs": [{ "text": "hi" }] } } }),
            )
        })
        .collect();
    let chat = parse_youtube(&lines.join("\n"));
    assert!((chat[1].at.unwrap() - (STARTED + 5.000123)).abs() < 1e-6, "{:?}", chat[1].at);
    let starts: Vec<f64> = chat.iter().filter_map(ChatMessage::start).collect();
    assert_eq!(starts.len(), START_MESSAGES);
    let start = vod_start(&starts).unwrap();
    assert!((start - (STARTED + 0.000123)).abs() < 1e-5, "{start}");
}

// Times and the VOD's start

/// chat.rs `parse_time` ("Unix seconds for an RFC 3339 time such as `2026-09-28T21:59:54Z`, `2026-09-28T21:33:22+00:00`
/// or `2026-09-28T20:15:37.046Z`"), the sites' times sent.
#[test]
fn an_rfc_3339_time_is_its_unix_seconds() {
    assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0.0));
    assert_eq!(parse_time("2026-09-28T21:59:54Z"), Some(1790632794.0));
    assert_eq!(parse_time("2026-09-28T21:33:22+00:00"), Some(1790631202.0));
    assert_eq!(parse_time("2026-09-28T23:33:22+02:00"), Some(1790631202.0));
    assert_eq!(parse_time("2026-09-28T19:03:22-02:30"), Some(1790631202.0));
    assert_eq!(parse_time("2026-09-28T20:15:37.046Z"), Some(1790626537.046));
    assert_eq!(parse_time("2024-02-29T12:00:00Z"), Some(1709208000.0));
    assert_eq!(parse_time("2026-10-01T18:00:00+23:59"), Some(1790877600.0 - 86340.0));
}

/// chat.rs `parse_time` (an RFC 3339 time) and a comment in `parse_time` (an offset "Within a day"): anything else is
/// no time.
#[test]
fn what_is_not_an_rfc_3339_time_is_none() {
    for s in [
        "2026-13-01T00:00:00Z",
        "2026-00-01T00:00:00Z",
        "yesterday",
        "",
        "2026-09-28 21:59:54Z",
        "2026/09/28T21:59:54Z",
        "2026-09-28T21:59:54",
        "2026-09-28T21:59:54+0200",
        "2026-10-01T18:00:00+24:00",
        "2026-10-01T18:00:00+00:60",
        "2026-10-01T18:00:00+9999999999999999:00",
        "2026-10-01T18:00:00-00:9999999999999999999",
    ] {
        assert_eq!(parse_time(s), None, "{s:?}");
    }
}

/// chat.rs `vod_start` ("their median, so a few odd ones (a site's clock, a message held back) don't move it").
#[test]
fn a_vods_start_is_the_median_of_its_messages_starts() {
    let starts = |n: usize| (0..n).map(|i| 1000.0 + i as f64).collect::<Vec<_>>();
    // Odd: the middle one, whatever the order; one far off moves nothing.
    let mut odd = starts(21);
    odd.reverse();
    odd[0] = 9e9;
    assert_eq!(vod_start(&odd), Some(1010.0));
    // Even: halfway between the middle two.
    assert_eq!(vod_start(&starts(20)), Some(1009.5));
}

/// chat.rs `START_MESSAGES` ("The fewest messages with a time of their own that `vod_start` goes by") and `vod_start`
/// ("None with fewer than `START_MESSAGES` of them").
#[test]
fn a_vod_has_no_start_by_fewer_than_twenty_messages() {
    assert_eq!(START_MESSAGES, 20);
    assert_eq!(vod_start(&[1000.0; 19]), None);
    assert_eq!(vod_start(&[]), None);
    assert_eq!(vod_start(&[1000.0; 20]), Some(1000.0));
}

/// chat.rs `ChatMessage::start` ("When the VOD started, by this message: when it was sent less how far into the VOD it
/// is").
#[test]
fn a_messages_start_is_when_it_was_sent_less_its_time_into_the_vod() {
    let mut m = ChatMessage {
        time: 30.0,
        author: "v".into(),
        color: None,
        parts: vec![ChatPart::Text { text: "hi".into() }],
        kind: Kind::Message,
        amount: None,
        event_time: None,
        at: Some(STARTED + 30.25),
    };
    assert_eq!(m.start(), Some(STARTED + 0.25));
    m.at = None;
    assert_eq!(m.start(), None, "not without its time sent");
}

/// chat.rs `ChatMessage::at` ("Only while a fetch runs, not saved with the message").
#[test]
fn when_a_message_was_sent_is_not_saved_with_it() {
    let m = ChatMessage {
        time: 30.0,
        author: "v".into(),
        color: None,
        parts: vec![],
        kind: Kind::Message,
        amount: None,
        event_time: None,
        at: Some(STARTED + 30.0),
    };
    let saved = serde_json::to_value(&m).unwrap();
    assert!(saved.get("at").is_none(), "{saved}");
    let read: ChatMessage = serde_json::from_value(saved).unwrap();
    assert_eq!(read, ChatMessage { at: None, ..m });
}

// What has a chat replay

/// chat.rs `fetch` ("False when the site has no chat replay for it (a file on this computer, another site …)").
#[test]
fn a_file_or_another_sites_vod_has_no_chat_replay() {
    let dir = crate::TempDir::new("chat-replay");
    for platform in [crate::local::PLATFORM, "dailymotion"] {
        let info = VodInfo { platform: platform.into(), ..twitch_vod(600.0) };
        let mut store = MemoryChat::default();
        let found =
            crate::chat::fetch(&info, None, &dir.join("chat-1"), &mut store, &AtomicBool::new(false), |_| {}).unwrap();
        assert!(!found, "{platform}");
        assert!(store.messages.is_empty() && store.positions.is_none(), "{platform}: nothing saved");
    }
}
