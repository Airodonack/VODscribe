//! The specs of `twitch.rs`: its parsers of Twitch's answers (a parser, so
//! the function is the spec), what's refused before Twitch is asked, and
//! `clip` and `vod_channel` against Twitch's GraphQL API as the kit's
//! network answers it.

use super::*;
use crate::testkit::{Net, Reply};

/// A clip's slug is told from a VOD's number, which yt-dlp may give with a
/// "v" (twitch.rs `is_clip`: "Whether `platform_id` (a Twitch item's id in the
/// library) is a clip's slug rather than a VOD's number (which yt-dlp may
/// give as `v123`)"; CHANGELOG.md, 0.3.0: "Twitch clips play").
#[test]
fn a_clips_slug_is_told_from_a_vods_number_with_or_without_its_v() {
    assert!(!is_clip("2899842479") && !is_clip("v2899842479"));
    assert!(is_clip("CoweringRudeDiamondMikeHogu-somPr5Um-WHGc4ga"));
    assert!(!is_clip(""));
}

/// A clip comes from its broadcast, where in it it starts, while Twitch
/// keeps the broadcast; once it's gone the clip has none (twitch.rs `Clip::source`:
/// "The broadcast's VOD and where in it the clip starts, while Twitch keeps
/// it"; CHANGELOG.md, 0.3.0: "Twitch clips play, and show the chat from the
/// broadcast they were cut from, while Twitch still has it.").
#[test]
fn a_clip_comes_from_its_broadcast_while_twitch_keeps_it() {
    let c = json!({ "durationSeconds": 32, "videoOffsetSeconds": 1805,
        "video": { "id": "2896314708" }, "broadcaster": { "id": "719949561" } });
    assert_eq!(
        parse_clip(&c),
        Clip { source: Some(("2896314708".into(), 1805.0)), duration: 32.0, broadcaster: Some("719949561".into()) }
    );
    let gone = json!({ "durationSeconds": 34, "videoOffsetSeconds": null, "video": null });
    assert_eq!(parse_clip(&gone).source, None);
    let no_offset = json!({ "durationSeconds": 34, "videoOffsetSeconds": null, "video": { "id": "2896314708" } });
    assert_eq!(parse_clip(&no_offset).source, None, "a broadcast with no place in it is none");
}

/// A Twitch VOD's channel is its login, as Twitch's API names it
/// (twitch.rs `vod_channel`: "VOD `id`'s channel's login (yt-dlp's `uploader_id`), as
/// Twitch's GraphQL API names it"; CHANGELOG.md, 0.7.3: "Refresh details is
/// quicker.").
#[test]
fn a_twitch_vods_channel_is_its_login_as_twitch_names_it() {
    let vod = json!({ "data": { "video": { "id": "2901646294", "owner": { "login": "quill" } } } });
    assert_eq!(parse_vod_channel(&vod, "2901646294").unwrap(), "quill");
}

/// An answer for another VOD, for none, or naming no channel is an error
/// (twitch.rs `vod_channel`: "An error when Twitch doesn't know the VOD or names no
/// channel, or answers for another VOD").
#[test]
fn an_answer_for_another_vod_none_or_no_channel_is_an_error() {
    let answer = |video: Value| json!({ "data": { "video": video } });
    let vod = answer(json!({ "id": "2901646294", "owner": { "login": "quill" } }));
    assert!(parse_vod_channel(&vod, "2901646295").is_err(), "another VOD's");
    assert!(parse_vod_channel(&answer(Value::Null), "2901646294").is_err(), "none");
    for named in [json!(null), json!({ "login": "" }), json!({})] {
        let vod = answer(json!({ "id": "2901646294", "owner": named }));
        assert!(parse_vod_channel(&vod, "2901646294").is_err(), "no channel named: {named}");
    }
}

/// Only a VOD's number is asked about: a clip's slug, or nothing, isn't
/// (twitch.rs `vod_channel`: "`id` is the VOD's number, with or without yt-dlp's
/// "v"").
#[test]
fn only_a_vods_number_is_asked_about() {
    for id in ["CoweringRudeDiamond", "", "v", "v12a", "12 3"] {
        assert!(vod_channel(id, &AtomicBool::new(false)).is_err(), "{id:?}");
    }
}

/// A clip Twitch's API knows comes from the broadcast it names, while
/// Twitch keeps it, and one it doesn't know is an error (twitch.rs `clip`:
/// "What Twitch says of clip `slug`"; twitch.rs `ask`: "Twitch doesn't know
/// this clip.").
#[test]
fn a_clip_twitch_knows_comes_from_its_broadcast_and_one_it_does_not_is_an_error() {
    let net = Net::new();
    net.twitch_clip("KitClipFromItsBroadcast", Some(("2896314708", 1805.0)), 32.0);
    net.twitch_clip("KitClipWhoseBroadcastIsGone", None, 34.0);
    net.replies(TWITCH_GQL, |asked| {
        let unknown = asked.json()?["variables"]["slug"] == "KitClipTwitchDoesNotKnow";
        unknown.then(|| Reply::json(&json!({ "data": { "clip": null } })))
    });
    assert_eq!(
        clip("KitClipFromItsBroadcast").unwrap(),
        Clip { source: Some(("2896314708".into(), 1805.0)), duration: 32.0, broadcaster: Some("1000".into()) }
    );
    let gone = clip("KitClipWhoseBroadcastIsGone").unwrap();
    assert_eq!((gone.source, gone.duration), (None, 34.0));
    assert_eq!(format!("{:#}", clip("KitClipTwitchDoesNotKnow").unwrap_err()), "Twitch doesn't know this clip.");
}

/// A VOD's channel is asked of Twitch's API by the VOD's number, with or
/// without yt-dlp's "v", and is the login it names; its answer for another
/// VOD is an error (twitch.rs `vod_channel`: "VOD `id`'s channel's login
/// (yt-dlp's `uploader_id`), as Twitch's GraphQL API names it", "An error
/// when … answers for another VOD").
#[test]
fn a_vods_channel_is_the_login_twitch_names_for_its_number() {
    let net = Net::new();
    net.replies(TWITCH_GQL, |asked| {
        let body = asked.json()?;
        if !body["query"].as_str()?.contains("owner { login }") {
            return None;
        }
        let answered = match body["variables"]["id"].as_str()? {
            "3100000001" => "3100000001",
            "3100000002" => "3100000003",
            _ => return None,
        };
        Some(Reply::json(&json!({ "data": { "video": { "id": answered, "owner": { "login": "quill" } } } })))
    });
    let never = AtomicBool::new(false);
    assert_eq!(vod_channel("3100000001", &never).unwrap(), "quill");
    assert_eq!(vod_channel("v3100000001", &never).unwrap(), "quill", "with yt-dlp's v");
    let other = vod_channel("3100000002", &never).unwrap_err();
    assert_eq!(format!("{other:#}"), "Twitch answered for another VOD.");
}

/// Asks Twitch for real: `cargo test -p vodscribe-core -- --ignored twitch_vod_channel_live`.
#[test]
#[ignore = "network"]
fn twitch_vod_channel_live() {
    let id = std::env::var("VOD").unwrap_or_else(|_| "2901646294".into());
    eprintln!("{}", vod_channel(&id, &AtomicBool::new(false)).unwrap());
}

/// Asks Twitch for real: `cargo test -p vodscribe-core -- --ignored twitch_clip`.
#[test]
#[ignore = "network"]
fn twitch_clip_live() {
    let slug = std::env::var("CLIP").unwrap_or_else(|_| "CoweringRudeDiamondMikeHogu-somPr5Um-WHGc4ga".into());
    eprintln!("{:?}", clip(&slug).unwrap());
    let url = clip_video(&slug).unwrap();
    let resp = crate::net::agent().head(&url).call().unwrap();
    assert_eq!(resp.status().as_u16(), 200);
}

/// A recent clip's chat, from its broadcast: `CLIP=<slug> cargo test -p vodscribe-core -- --ignored twitch_clip_chat`.
#[test]
#[ignore = "network"]
fn twitch_clip_chat_live() {
    let slug = std::env::var("CLIP").expect("CLIP: a recent clip's slug");
    let info = crate::fetch::VodInfo {
        platform: "twitch".into(),
        platform_id: slug,
        url: String::new(),
        title: String::new(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: None,
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let mut store = crate::chat::MemoryChat::default();
    let found =
        crate::chat::fetch(&info, None, std::path::Path::new("/nowhere"), &mut store, &AtomicBool::new(false), |_| {});
    let times: Vec<f64> = store.messages.iter().map(|m| m.time).collect();
    eprintln!(
        "chat: {:?}, {} messages, from {:?} to {:?}",
        found.as_ref().ok(),
        times.len(),
        times.first(),
        times.last()
    );
    assert!(found.unwrap());
    assert!(times.iter().all(|t| (0.0..=60.0).contains(t)), "at the clip's own times");
}
