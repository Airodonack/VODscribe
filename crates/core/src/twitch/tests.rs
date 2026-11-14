//! The specs of `twitch.rs`: its parsers of Twitch's answers (a parser, so
//! the function is the spec), and what's refused before Twitch is asked.
//! Twitch's GraphQL API isn't faked yet, so `clip` and `vod_channel`
//! against its answers wait.

use super::*;

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
