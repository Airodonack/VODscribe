//! Reading a VOD's transcript, and the files it is on this computer.

use crate::testkit::TestApp;
use serde_json::{Value, json};
use vodscribe_core::chat::{ChatMessage, ChatPart, Kind};
use vodscribe_core::fetch::VodInfo;

impl TestApp {
    /// VOD `vod`'s lines as `transcript` answers, as (start, text), for the
    /// whole VOD or from `from` to before `to`.
    fn lines(&self, vod: i64, from: Option<f64>, to: Option<f64>) -> Vec<(f64, String)> {
        let lines = self.invoke("transcript", json!({ "vodId": vod, "from": from, "to": to })).unwrap();
        lines.as_array().unwrap().iter().map(|l| (l["start"].as_f64().unwrap(), text(l))).collect()
    }

    /// A VOD of a site, in `channel`'s name.
    fn add_channels_vod(&self, id: &str, channel: &str) -> vodscribe_core::db::Vod {
        let info = VodInfo {
            platform: "twitch".into(),
            platform_id: id.into(),
            url: format!("https://www.twitch.tv/videos/{id}"),
            title: id.into(),
            channel: Some(channel.into()),
            channel_login: None,
            channel_url: None,
            duration: Some(60.0),
            thumbnail: None,
            published: None,
            started_at: None,
        };
        self.db().add_vod(&info).unwrap().0
    }

    /// `author` writes `messages` messages in VOD `vod`'s chat.
    fn chats(&self, vod: i64, author: &str, messages: usize) {
        let messages: Vec<ChatMessage> = (0..messages)
            .map(|i| ChatMessage {
                time: i as f64,
                author: author.into(),
                color: None,
                parts: vec![ChatPart::Text { text: "hello".into() }],
                kind: Kind::Message,
                amount: None,
                event_time: None,
                at: None,
            })
            .collect();
        self.db().add_chat(vod, &messages, &[]).unwrap();
    }

    /// The names `author_suggestions` gives for `prefix`, with whether each is a channel.
    fn suggested(&self, prefix: &str, vod: Option<i64>) -> Vec<(String, bool)> {
        let names = self.invoke("author_suggestions", json!({ "prefix": prefix, "vodId": vod })).unwrap();
        names.as_array().unwrap().iter().map(|n| (n["name"].as_str().unwrap().into(), n["streamer"] == true)).collect()
    }
}

fn text(line: &Value) -> String {
    line["text"].as_str().unwrap().to_string()
}

/// A VOD with four lines, said at 0, 10, 20 and 30 seconds.
fn four_lines(app: &TestApp) -> i64 {
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    for (at, line) in [(0.0, "one is first"), (10.0, "two is next"), (20.0, "three is after"), (30.0, "four is last")] {
        app.says(vod.id, at, line);
    }
    vod.id
}

fn said(lines: &[(f64, String)]) -> Vec<&str> {
    lines.iter().map(|(_, t)| t.as_str()).collect()
}

/// A VOD's lines are all of them, or with `from`, those starting from it to
/// before `to` (None: on to the end)
/// (vods.rs, the doc comment of `transcript`: "all of them, or with `from`,
/// those starting from it to before `to`").
#[test]
fn a_vods_lines_are_all_of_them_or_those_from_a_moment_to_another() {
    let app = TestApp::new();
    let vod = four_lines(&app);
    assert_eq!(said(&app.lines(vod, None, None)), ["one is first", "two is next", "three is after", "four is last"]);
    assert_eq!(said(&app.lines(vod, Some(10.0), Some(30.0))), ["two is next", "three is after"]);
    assert_eq!(said(&app.lines(vod, Some(20.0), None)), ["three is after", "four is last"]);
    assert_eq!(said(&app.lines(vod, Some(10.0), Some(10.0))), Vec::<&str>::new());
}

/// A file's captions are its lines
/// (vods.rs, the doc comment of `captions`: "VOD `vod_id`'s captions, for
/// the player of a file played here"; their shape is 3B3's).
#[test]
fn a_files_captions_are_its_lines() {
    let app = TestApp::new();
    let vod = four_lines(&app);
    let captions = app.invoke("captions", json!({ "vodId": vod })).unwrap();
    let captions: Vec<(f64, String)> =
        captions.as_array().unwrap().iter().map(|c| (c["start"].as_f64().unwrap(), text(c))).collect();
    assert_eq!(captions, app.lines(vod, None, None));
}

/// `from:` suggests channels first, then the people in chat
/// (CHANGELOG.md, 0.6.0: "The search filter gains Within and From whom.";
/// vods.rs, the doc comment of `author_suggestions`; core search.rs: "the
/// channels of the library's VODs first, then chatters with the most
/// messages"; "(in VOD `vod_id`, if given)").
#[test]
fn from_suggests_channels_first_then_chatters() {
    let app = TestApp::new();
    let vod = app.add_channels_vod("1", "Stormy");
    app.chats(vod.id, "stormfan", 3);
    app.chats(vod.id, "stormlover", 5);
    app.chats(vod.id, "elsewhere", 9);
    assert_eq!(
        app.suggested("sto", None),
        [("Stormy".to_string(), true), ("stormlover".to_string(), false), ("stormfan".to_string(), false)]
    );

    // In one VOD, only its own channel and chat.
    let other = app.add_channels_vod("2", "Stoneface");
    app.chats(other.id, "stoic", 1);
    assert_eq!(app.suggested("sto", Some(other.id)), [("Stoneface".to_string(), true), ("stoic".to_string(), false)]);
}

/// Show in folder shows a VOD's file by its id, never a path
/// (vods.rs, the doc comment of `show_file`: "Takes the VOD's id, not a
/// path, so the UI can't point it anywhere else").
#[test]
fn show_in_folder_shows_a_vods_file_by_its_id_never_a_path() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.invoke("show_file", json!({ "id": vod.id })).unwrap();
    assert_eq!(app.desktop().shown(), [std::path::PathBuf::from(vod.source_path.unwrap())]);

    let elsewhere = app.invoke("show_file", json!({ "id": vod.info.url.clone() }));
    assert!(elsewhere.is_err());
    assert!(app.invoke("show_file", json!({ "id": vod.id + 100 })).is_err());
    assert_eq!(app.desktop().shown().len(), 1);
}

/// Show in folder shows a download as far as it is
/// (vods.rs, the doc comment of `show_file`: "the part of a download so
/// far, as it is"; CHANGELOG.md, 0.7.1 Fixes: "Show in folder works during
/// a download.").
#[test]
fn show_in_folder_shows_a_download_as_far_as_it_is() {
    let app = TestApp::new();
    let vod = app.add_site_vod("twitch", "2901646294", "A stream", 60.0);
    assert!(app.invoke("show_file", json!({ "id": vod.id })).is_err(), "nothing has been downloaded yet");

    let part = app.paths().source_video_stem(vod.id).with_extension("mp4.part");
    std::fs::create_dir_all(part.parent().unwrap()).unwrap();
    std::fs::write(&part, b"the first part").unwrap();
    app.invoke("show_file", json!({ "id": vod.id })).unwrap();
    assert_eq!(app.desktop().shown(), [part]);
}

/// A file the built-in player can't play opens in the system's player, and
/// only a media file does
/// (vods.rs, the doc comment of `open_file`: "Opens a local VOD's file in
/// the system's video player, for formats the built-in player can't play.
/// Takes the VOD's id, not a path"; its comment: "only ever open media files").
#[test]
fn a_file_the_player_cannot_play_opens_in_the_systems_player() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mkv", 60.0);
    app.invoke("open_file", json!({ "id": vod.id })).unwrap();
    assert_eq!(app.desktop().opened(), [std::path::PathBuf::from(vod.source_path.unwrap())]);

    let program = app.add_file("setup.exe", &vodscribe_core::testkit::media::Media::video(60.0));
    assert!(app.invoke("open_file", json!({ "id": program.id })).is_err());
    let site = app.add_site_vod("twitch", "2901646294", "A stream", 60.0);
    assert!(app.invoke("open_file", json!({ "id": site.id })).is_err());
    assert_eq!(app.desktop().opened().len(), 1);
}
