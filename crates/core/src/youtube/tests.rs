//! The specs of `youtube.rs`, at its seam: `channel` asking oEmbed, its
//! answers recorded (`testkit::Net`), and `is_video_id`; and Refresh
//! details, which asks it first (`pipeline::refresh_details`).

use super::*;
use crate::testkit::{Net, TempDir, YtDlp, yt_dlp};
use serde_json::json;

/// The link oEmbed is asked at for video `id`.
fn oembed(id: &str) -> String {
    format!("https://www.youtube.com/oembed?format=json&url=https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3D{id}")
}

/// oEmbed's answer for video `embed`, its channel's page `page`.
fn answer(page: &str, embed: &str) -> String {
    json!({ "author_name": "Quill", "author_url": page,
        "html": format!("<iframe src=\"https://www.youtube.com/embed/{embed}?feature=oembed\"></iframe>") })
    .to_string()
}

fn channel_of(id: &str) -> Result<(String, String)> {
    channel(id, &AtomicBool::new(false))
}

/// A video's id is eleven of its URL-safe characters, and anything else
/// isn't one (youtube.rs `is_video_id`: "eleven of its URL-safe characters").
#[test]
fn a_youtube_video_id_is_eleven_url_safe_characters() {
    assert!(is_video_id("noa29qsTc4g") && is_video_id("a-b_c123456"));
    assert!(!is_video_id("e2eolder") && !is_video_id("noa29qsTc4g?") && !is_video_id("noa29qsTc4gx"));
}

/// Video `id`'s channel is its handle and its page as oEmbed names them
/// (youtube.rs `channel`: "its handle (`@name`) and its page
/// (`https://www.youtube.com/@name`)"; CHANGELOG.md, 0.7.3: "Refresh details
/// is quicker.").
#[test]
fn a_videos_channel_is_its_handle_and_page_as_oembed_names_them() {
    let net = Net::new();
    let id = "3b2ChanOk01";
    net.serves(&oembed(id), answer("https://www.youtube.com/@quill.tv", id));
    assert_eq!(channel_of(id).unwrap(), ("@quill.tv".into(), "https://www.youtube.com/@quill.tv".into()));
}

/// A video YouTube won't say anything of (gone, private, not embeddable:
/// oEmbed's 404 and 401) is an error (youtube.rs `channel`: "An error when YouTube
/// won't say (the video is gone, private or not embeddable)").
#[test]
fn a_video_youtube_wont_say_of_is_an_error() {
    let net = Net::new();
    for (id, status) in [("3b2Gone0404", 404), ("3b2NoEmb401", 401)] {
        net.answers(&oembed(id), status);
        assert!(channel_of(id).is_err(), "{status}");
    }
}

/// An answer for another video isn't this one's channel (youtube.rs `channel`:
/// "answers for another video").
#[test]
fn an_oembed_answer_for_another_video_is_an_error() {
    let net = Net::new();
    let id = "3b2Another1";
    net.serves(&oembed(id), answer("https://www.youtube.com/@quill", "bbbbbbbbbbb"));
    assert!(channel_of(id).is_err());
}

/// A channel named by anything but a plain handle is an error
/// (youtube.rs `channel`: "or names the channel by anything but a plain handle").
#[test]
fn a_channel_named_by_anything_but_a_plain_handle_is_an_error() {
    let net = Net::new();
    for (n, page) in [
        "https://www.youtube.com/channel/UC0123456789",
        "https://www.youtube.com/@",
        "https://www.youtube.com/@%E5%90%8D",
        "https://www.youtube.com/@quill/videos",
        "https://evil.example/@quill",
        "http://www.youtube.com/@quill",
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("3b2Handle0{n}");
        net.serves(&oembed(&id), answer(page, &id));
        assert!(channel_of(&id).is_err(), "{page}");
    }
    let id = "3b2NoPage01";
    net.serves(&oembed(id), json!({ "html": format!("youtube.com/embed/{id}?") }).to_string());
    assert!(channel_of(id).is_err(), "no channel named");
}

/// An id not shaped as a video's isn't asked about, even where oEmbed would
/// answer for it (youtube.rs `is_video_id`: "Anything else isn't asked about").
#[test]
fn an_id_not_shaped_as_a_videos_is_not_asked_about() {
    let net = Net::new();
    let id = "3b2Shape01";
    net.serves(&oembed(id), answer("https://www.youtube.com/@quill", id));
    assert!(channel_of(id).is_err());
}

/// A library with a YouTube VOD of `id` in it, and a fake yt-dlp that
/// serves nothing till told.
struct Refreshing {
    db: std::sync::Mutex<crate::db::Db>,
    paths: crate::Paths,
    fake: YtDlp,
    yt_dlp: crate::tools::YtDlp,
    vod: crate::db::Vod,
    _dir: TempDir,
}

impl Refreshing {
    fn new(name: &str, id: &str) -> Self {
        let dir = TempDir::new(name);
        let paths = crate::Paths::new(dir.join("data"));
        paths.ensure().unwrap();
        let db = crate::db::Db::open_in(&paths).unwrap();
        let vod = crate::testkit::library::add_site_vod(&db, "youtube", id, "Day 3", 600.0);
        let fake = YtDlp::at(dir.join("bin/yt-dlp"));
        let yt_dlp = crate::tools::YtDlp { path: fake.path().into(), js_runtime: None };
        Refreshing { db: std::sync::Mutex::new(db), paths, fake, yt_dlp, vod, _dir: dir }
    }

    /// Refresh details, and the channel's login and page the VOD then has.
    fn refresh(&self) -> Result<(Option<String>, Option<String>)> {
        let vod = crate::pipeline::refresh_details(
            &self.db,
            &self.yt_dlp,
            &self.paths,
            self.vod.id,
            &AtomicBool::new(false),
        )?;
        Ok((vod.info.channel_login, vod.info.channel_url))
    }
}

/// Refresh details of a YouTube VOD takes its channel from oEmbed, with no
/// yt-dlp look-up (CHANGELOG.md, 0.7.3: "Refresh details is quicker. On
/// Twitch and YouTube it takes under a second instead of several";
/// youtube.rs module doc: "Refresh details, which wants only the channel, asks here
/// first"). The fake yt-dlp knows nothing of the VOD, so its look-up would
/// fail. [join: 2A1 refresh_details]
#[test]
fn refresh_details_of_a_youtube_vod_takes_its_channel_from_oembed() {
    let net = Net::new();
    let id = "3b2Refresh1";
    let library = Refreshing::new("youtube-refresh", id);
    net.serves(&oembed(id), answer("https://www.youtube.com/@quill", id));
    assert_eq!(library.refresh().unwrap(), (Some("@quill".into()), Some("https://www.youtube.com/@quill".into())));
}

/// A channel oEmbed names with no plain handle is asked of yt-dlp instead
/// (pipeline.rs `channel_by_api`: "A channel with no plain handle fails, and yt-dlp is
/// asked"). [join: 2A1 refresh_details]
#[test]
fn a_youtube_channel_with_no_plain_handle_is_refreshed_through_yt_dlp() {
    let net = Net::new();
    let id = "3b2Refresh2";
    let library = Refreshing::new("youtube-refresh-handle", id);
    net.serves(&oembed(id), answer("https://www.youtube.com/channel/UC0123456789", id));
    let mut details = yt_dlp::details("youtube", id, &library.vod.info.url, "Day 3", 600.0);
    details["uploader_id"] = json!("@quill");
    details["uploader_url"] = json!("https://www.youtube.com/@quill");
    library.fake.serves(details);
    assert_eq!(library.refresh().unwrap(), (Some("@quill".into()), Some("https://www.youtube.com/@quill".into())));
}

/// Asks YouTube for real: `cargo test -p vodscribe-core -- --ignored youtube_channel_live`.
#[test]
#[ignore = "network"]
fn youtube_channel_live() {
    let id = std::env::var("VIDEO").unwrap_or_else(|_| "noa29qsTc4g".into());
    eprintln!("{:?}", channel(&id, &AtomicBool::new(false)).unwrap());
}
