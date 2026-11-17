//! The specs of thumbnails.rs: the local copy of each VOD's picture, fetched
//! when it's added (or at the start, for those without one) and handed to
//! the page. The sites' image servers are the kit's network, each test's
//! image a link of its own; a file's picture is a frame of its storyboard,
//! made on the fake ffmpeg.

use super::*;

use serde_json::{Value, json};
use vodscribe_core::db::Vod;
use vodscribe_core::testkit::{Gate, Reply, eventually, library, media, yt_dlp};

use vodscribe_player::storyboard::Storyboard;

use crate::testkit::TestApp;

/// A JPEG's first bytes, which is all the fetching looks at.
const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 0xFF, 0xD9];

/// An image link on a site's image server (`i.ytimg.com`), one for each
/// test's VOD.
fn image(id: &str) -> String {
    format!("https://i.ytimg.com/vi/{id}-{}/hq.jpg", std::process::id())
}

/// A YouTube VOD `id` added by its link, as the page adds it, the site
/// saying its picture is at `picture` (none: it says none), and the thread
/// that asks for the picture waited for.
fn added(app: &TestApp, id: &str, picture: Option<&str>) -> Vod {
    let url = format!("https://www.youtube.com/watch?v={id}");
    let mut details = yt_dlp::details("youtube", id, &url, "Their stream", 60.0);
    if let Some(picture) = picture {
        details["thumbnail"] = picture.into();
    }
    app.yt_dlp.serves(details);
    app.invoke("add_vod", json!({ "url": url })).unwrap();
    app.threads_done();
    app.db().find_vod("youtube", id).unwrap().unwrap()
}

/// What the library says VOD `id`'s picture is.
fn thumb(app: &TestApp, id: i64) -> Option<String> {
    app.db().vod(id).unwrap().unwrap().thumb
}

/// A site's picture is kept when its VOD is added, and the page is told: the
/// VOD's `thumb` names the file, which `thumbnail` gives (thumbnails.rs,
/// module doc: "Local copies of VOD thumbnails: fetched when a VOD is added
/// … and handed to the UI over IPC"; 0.1.0 and on: showing the library
/// doesn't tell the platforms' servers what's in it).
#[test]
fn a_sites_picture_is_kept_when_its_vod_is_added() {
    let app = TestApp::new();
    let link = image("kept");
    app.net.serves(&link, JPEG);
    let vod = added(&app, "kept", Some(&link));
    let name = vod.thumb.clone().expect("a picture was asked for");
    assert!(name.starts_with(&format!("{}-", vod.id)) && name.ends_with(".jpg"), "{name}");
    assert_eq!(app.invoke_bytes("thumbnail", json!({ "name": name })).unwrap(), JPEG);
    let heard = app.heard("vod");
    assert_eq!(heard.last().unwrap()["thumb"], json!(name), "the page was told");
}

/// A VOD whose site has no picture for it, or whose link has none any more,
/// gets an empty one, so it isn't tried again (thumbnails.rs
/// `cache_thumbnail`: "A VOD with no usable image gets an empty `thumb` so
/// it isn't tried again").
#[test]
fn a_vod_with_no_usable_picture_gets_an_empty_one_so_it_is_not_tried_again() {
    let app = TestApp::new();
    let gone = image("gone");
    app.net.answers(&gone, 404);
    let none = added(&app, "none", None);
    let missing = added(&app, "missing", Some(&gone));
    assert_eq!(none.thumb.as_deref(), Some(""));
    assert_eq!(missing.thumb.as_deref(), Some(""), "a picture the site no longer has");
    // At the start, an empty one is left as it is.
    app.net.serves(&gone, JPEG);
    cache_missing_thumbnails(app.handle().clone());
    assert_eq!(thumb(&app, missing.id).as_deref(), Some(""));
}

/// A picture that can't be fetched (offline, a server error) is tried again
/// at the next launch (thumbnails.rs `cache_thumbnail`: "network failures
/// are retried next launch, for a week after it was added").
#[test]
fn a_picture_that_fails_to_come_is_fetched_at_the_next_launch() {
    let app = TestApp::new();
    let link = image("retry");
    let up = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let site = up.clone();
    app.net.replies(&link, move |_| {
        Some(if site.load(std::sync::atomic::Ordering::SeqCst) { Reply::body(JPEG) } else { Reply::status(503) })
    });
    let vod = added(&app, "retry", Some(&link));
    assert_eq!(vod.thumb, None, "not given up on");
    up.store(true, std::sync::atomic::Ordering::SeqCst);
    cache_missing_thumbnails(app.handle().clone());
    let name = thumb(&app, vod.id).expect("fetched at the launch");
    assert!(app.paths().source_thumb(&name).is_file());
}

/// A picture still failing a week after the VOD was added is given up on:
/// an empty one, so it's not tried at each launch for good (thumbnails.rs
/// `RETRY_FOR_SECONDS`: "How long a site's image that can't be fetched
/// (offline, a server error) is tried again, at each launch, before it's
/// given up on").
#[test]
fn a_picture_still_failing_a_week_after_its_vod_was_added_is_given_up_on() {
    let app = TestApp::new();
    let link = image("week");
    app.net.answers(&link, 503);
    let vod = added(&app, "week", Some(&link));
    assert_eq!(vod.thumb, None);
    let beside = library::beside(&app.paths().db());
    let (just_under, over) = (vod.added_at - 6 * 24 * 3600, vod.added_at - 8 * 24 * 3600);
    beside.execute("UPDATE vods SET added_at = ?1 WHERE id = ?2", (just_under, vod.id)).unwrap();
    cache_missing_thumbnails(app.handle().clone());
    assert_eq!(thumb(&app, vod.id), None, "six days in: tried again later");
    beside.execute("UPDATE vods SET added_at = ?1 WHERE id = ?2", (over, vod.id)).unwrap();
    cache_missing_thumbnails(app.handle().clone());
    assert_eq!(thumb(&app, vod.id).as_deref(), Some(""), "eight days in: given up on");
}

/// A picture still arriving for a VOD that was removed doesn't land on the
/// next VOD added (0.3.0 Fixes: "Chat or a thumbnail still arriving for a
/// removed VOD could land on the next one added.").
#[test]
fn a_picture_arriving_for_a_removed_vod_does_not_land_on_the_next_one() {
    let app = TestApp::new();
    let link = image("late");
    app.net.serves(&link, JPEG);
    let slow = Gate::new();
    app.net.holds(&link, &slow);
    // Added, its picture on its way (held at the site).
    let url = "https://www.youtube.com/watch?v=late";
    let mut details = yt_dlp::details("youtube", "late", url, "Their stream", 60.0);
    details["thumbnail"] = link.as_str().into();
    app.yt_dlp.serves(details);
    app.invoke("add_vod", json!({ "url": url })).unwrap();
    let first = app.db().find_vod("youtube", "late").unwrap().unwrap();
    slow.reached();
    // Removed and erased, then another added, which may take its id.
    app.invoke("delete_vod", json!({ "id": first.id })).unwrap();
    app.erasures_done();
    let next = app.add_video_file("Next.mp4", 60.0);
    app.db().set_thumb(next.id, "").unwrap();
    slow.open();
    app.threads_done();
    assert_eq!(thumb(&app, next.id).as_deref(), Some(""), "VOD {} (the removed one was {})", next.id, first.id);
}

/// The page can read only thumbnails, by the name the VOD has (thumbnails.rs
/// `thumbnail`: "A thumbnail's bytes, by the file name in `Vod::thumb`, and
/// nothing else.").
#[test]
fn only_a_thumbnails_bytes_are_given_by_its_name() {
    let app = TestApp::new();
    let thumbs = app.paths().thumbs();
    std::fs::create_dir_all(&thumbs).unwrap();
    std::fs::write(thumbs.join("5-12.png"), b"\x89PNG picture").unwrap();
    std::fs::write(app.paths().thumbs().parent().unwrap().join("library.txt"), b"not a picture").unwrap();
    assert_eq!(app.invoke_bytes("thumbnail", json!({ "name": "5-12.png" })).unwrap(), b"\x89PNG picture");
    for name in ["../library.txt", "5-12.png/../../library.txt", "library.txt", "notes.txt", ""] {
        assert_eq!(app.invoke("thumbnail", json!({ "name": name })), Err(json!("Not a thumbnail.")), "{name:?}");
    }
    assert!(app.invoke("thumbnail", json!({ "name": "5-13.png" })).is_err(), "a name with no file");
}

/// A file on this computer gets its picture from its storyboard, a frame an
/// eighth of the way in (thumbnails.rs `cache_thumbnail`: "for a local file a
/// frame of its storyboard, which is asked for here"; 0.6.0: "A file's
/// library picture comes later. It's a frame an eighth of the way in, not a
/// tenth, from the small frames VODscribe keeps of each video on this
/// computer").
#[test]
fn a_file_added_gets_a_frame_of_its_storyboard_as_its_picture() {
    let app = TestApp::new();
    let user = vodscribe_core::testkit::TempDir::new("thumbnails-files");
    let dir = user.join("added");
    media::Media::video(60.0).write(&dir.join("Day 1.mp4"));
    let added =
        app.run(crate::vods::add_files(app.handle().clone(), vec![dir.join("Day 1.mp4").display().to_string()]));
    added.unwrap();
    let vod = app.db().vods().unwrap().remove(0);
    eventually("the picture is taken", || thumb(&app, vod.id).is_some_and(|t| !t.is_empty()));
    let name = thumb(&app, vod.id).unwrap();
    let picture = app.invoke_bytes("thumbnail", json!({ "name": name })).unwrap();
    let at = media::storyboard_time(&picture).expect("a storyboard's frame");
    // A 60 s file, a frame every 2 s: the one at or before 7.5 s.
    assert!((4.0..=7.5).contains(&at), "{at} s, not about an eighth of 60 s");
    assert!(app.heard("vod").last().is_some_and(|v: &Value| v["thumb"] == json!(name)), "the page was told");
}

/// A file that's sound alone has no picture to take, and says so: an empty
/// one, so it isn't tried again (storyboards.rs `make`: "No picture to make
/// one of (a sound file): no picture.").
#[test]
fn a_sound_file_has_no_picture() {
    let app = TestApp::new();
    let user = vodscribe_core::testkit::TempDir::new("thumbnails-sound");
    let dir = user.join("added");
    media::Media::sound(60.0).write(&dir.join("Podcast.m4a"));
    let added =
        app.run(crate::vods::add_files(app.handle().clone(), vec![dir.join("Podcast.m4a").display().to_string()]));
    added.unwrap();
    let vod = app.db().vods().unwrap().remove(0);
    eventually("it's said it has none", || thumb(&app, vod.id).is_some());
    assert_eq!(thumb(&app, vod.id).as_deref(), Some(""));
}

/// At the start, the VODs without a picture get one, and one whose picture
/// file went is fetched again (thumbnails.rs `cache_missing_thumbnails`:
/// "Fills in local thumbnails for VODs that don't have one yet, such as
/// those added before thumbnails were stored locally").
#[test]
fn at_the_start_a_vod_without_its_picture_gets_it_again() {
    let app = TestApp::new();
    let (first, second) = (image("first"), image("second"));
    app.net.serves(&first, JPEG);
    app.net.serves(&second, JPEG);
    let vod = added(&app, "first", Some(&first));
    let lost = added(&app, "second", Some(&second));
    let (kept, was) = (thumb(&app, vod.id).unwrap(), thumb(&app, lost.id).unwrap());
    // One's file went (a cleaning of the folder); one was never made.
    std::fs::remove_file(app.paths().source_thumb(&was)).unwrap();
    let never = app.add_site_vod("youtube", "never", "Their stream", 60.0);
    assert_eq!(thumb(&app, never.id), None);
    cache_missing_thumbnails(app.handle().clone());
    app.threads_done();
    assert_eq!(thumb(&app, vod.id), Some(kept), "one with its file is left alone");
    let again = thumb(&app, lost.id).unwrap();
    assert!(app.paths().source_thumb(&again).is_file(), "fetched again");
    assert_eq!(thumb(&app, never.id).as_deref(), Some(""), "no picture said: none");
}

/// A file's picture that's gone is taken again from its storyboard, made
/// already, while one that's there is left as it is (storyboards.rs `make`:
/// "Made before: its picture is taken again only if it's missing").
#[test]
fn a_files_picture_that_went_is_taken_again_from_its_storyboard_and_one_there_is_kept() {
    let app = TestApp::new();
    let user = vodscribe_core::testkit::TempDir::new("thumbnails-again");
    media::Media::video(60.0).write(&user.join("Day 1.mp4"));
    app.run(crate::vods::add_files(app.handle().clone(), vec![user.join("Day 1.mp4").display().to_string()])).unwrap();
    let vod = app.db().vods().unwrap().remove(0);
    eventually("the picture is taken", || thumb(&app, vod.id).is_some_and(|t| !t.is_empty()));
    let first = thumb(&app, vod.id).unwrap();
    let board = app.paths().source_storyboard(vod.id, std::path::Path::new(vod.source_path.as_deref().unwrap()));
    eventually("its storyboard is made", || Storyboard::open(&board).is_ok_and(|b| b.complete()));
    // Its picture is there: asked for again, it's left.
    cache_missing_thumbnails(app.handle().clone());
    app.state().storyboards.swept();
    assert_eq!(thumb(&app, vod.id).as_deref(), Some(first.as_str()));
    // Gone: taken again.
    std::fs::remove_file(app.paths().source_thumb(&first)).unwrap();
    cache_missing_thumbnails(app.handle().clone());
    eventually("the picture is taken again", || {
        thumb(&app, vod.id).is_some_and(|t| app.paths().source_thumb(&t).is_file())
    });
}
