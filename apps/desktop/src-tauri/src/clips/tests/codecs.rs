//! A format (codec) the ffmpeg in use can't make (handbook plans/clips-pilot.md,
//! "Decided", 3: the clip fails with the app's own sentence, which format and
//! Settings › Encoding, and that format's option in Settings is greyed out):
//! through `add_clip`, `render_edit` by way of `remove_stretch`, `delete_vod`
//! keeping an edit's stretch (`realize`), and `missing_clip_codecs`, which
//! the page asks for the options to grey out.

use super::*;

use serde_json::{Value, json};
use vodscribe_core::settings::ClipCodec;

use crate::testkit::TestApp;

/// What a clip in MP4 H.265 says when the ffmpeg in use can't make it.
const NO_H265: &str = "VODscribe's ffmpeg can't make MP4 H.265. Pick another format in Settings › Encoding.";

/// Settings › Encoding set to `codec`, as the page saves it.
fn clips_are(app: &TestApp, codec: ClipCodec) {
    app.invoke("set_settings", json!({ "settings": { "clip_codec": codec } })).unwrap();
}

/// The codecs the page greys out (`missing_clip_codecs`), of the app's own
/// ffmpeg (the testkit's is in its bin).
fn greyed(app: &TestApp) -> Value {
    let answer = app.invoke("missing_clip_codecs", json!({})).unwrap();
    assert_eq!(answer["own"], true, "VODscribe's own ffmpeg taken for the system's");
    answer["missing"].clone()
}

/// How many times ffmpeg was asked for its list of encoders.
fn lists_asked(app: &TestApp) -> usize {
    app.ffmpeg.runs().iter().filter(|args| args.iter().any(|a| a == "-encoders")).count()
}

/// A clip in a codec the ffmpeg in use lacks fails with the app's sentence,
/// not ffmpeg's, and makes no file; the choice stays as it was. A clip of
/// sound alone needs no picture's encoder, and is made.
#[test]
fn a_clip_in_a_codec_ffmpeg_lacks_fails_saying_which_and_where_to_pick_another() {
    let app = TestApp::new();
    app.ffmpeg.lacks("libx265");
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    clips_are(&app, ClipCodec::H265);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    let clip = app.made(clip.id);
    assert_eq!((clip.status, clip.error.as_deref(), clip.file_path), (ClipStatus::Error, Some(NO_H265), None));
    assert_eq!(app.invoke("get_settings", json!({})).unwrap()["clip_codec"], "h265", "the choice kept");
    let talk = app.add_file("Talk.m4a", &vodscribe_core::testkit::Media::sound(60.0));
    let sound = app.make_clip(talk.id, "0:10", "0:20");
    assert_eq!((sound.status, sound.error), (ClipStatus::Ready, None), "sound alone is made");
}

/// An edit of `range` of VOD `vod_id`, and a clip of its 0:05–0:09 on its
/// clip bar, made (`add_edit_clip`).
fn edit_bar_clip(app: &TestApp, vod_id: i64, range: (f64, f64)) -> Clip {
    let edit = {
        let db = app.db();
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod_id, range, "a", None).unwrap();
        edit
    };
    let made = app.invoke("add_edit_clip", json!({ "editId": edit, "from": 5.0, "to": 9.0 })).unwrap();
    app.made(made["id"].as_i64().unwrap())
}

/// The files of footage fetched from sites.
fn footage(app: &TestApp) -> Vec<std::ffi::OsString> {
    std::fs::read_dir(app.paths().source_footage())
        .map(|d| d.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default()
}

/// An edit rendered in a codec the ffmpeg in use lacks fails the same way.
#[test]
fn an_edit_in_a_codec_ffmpeg_lacks_fails_saying_which_and_where_to_pick_another() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = edit_bar_clip(&app, vod.id, (0.0, 60.0));
    // Another ffmpeg in its place, without H.265: the one asked as the clip
    // was made had it.
    app.ffmpeg.lacks("libx265");
    app.ffmpeg.updated();
    clips_are(&app, ClipCodec::H265);
    // A second out of its middle: two pieces, rendered as an edit is.
    app.invoke("remove_stretch", json!({ "id": clip.id, "from": 1.0, "to": 2.0 })).unwrap();
    let after = app.made(clip.id);
    assert_eq!((after.status, after.error.as_deref()), (ClipStatus::Error, Some(NO_H265)));
}

/// Settings greys out the codecs the ffmpeg in use lacks. Its list of
/// encoders is asked for once, whatever asks (Settings, a clip), and again
/// once that ffmpeg is replaced, as an update replaces it.
#[test]
fn the_ffmpeg_s_encoders_are_asked_once_and_again_when_it_changes() {
    let app = TestApp::new();
    app.ffmpeg.lacks("libx265");
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    assert_eq!(greyed(&app), json!(["h265"]));
    clips_are(&app, ClipCodec::H265);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    assert_eq!(app.made(clip.id).error.as_deref(), Some(NO_H265));
    assert_eq!(greyed(&app), json!(["h265"]));
    assert_eq!(lists_asked(&app), 1, "asked once");

    // An ffmpeg with H.265 in its place.
    app.ffmpeg.works();
    app.ffmpeg.updated();
    assert_eq!(greyed(&app), json!([]));
    let clip = app.add_clip(vod.id, "0:30", "0:40").unwrap();
    let clip = app.made(clip.id);
    assert_eq!((clip.status, clip.error), (ClipStatus::Ready, None));
    assert_eq!(lists_asked(&app), 2, "asked again once it changed");
}

/// An edit of a site's VOD rendered again in a codec the ffmpeg in use
/// lacks fails before its footage is fetched again (review M1).
#[test]
fn an_edit_in_a_codec_ffmpeg_lacks_fetches_no_footage_for_it() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "Test video", 600.0);
    let clip = edit_bar_clip(&app, vod.id, (60.0, 120.0));
    std::fs::remove_dir_all(app.paths().source_footage()).unwrap();
    app.ffmpeg.lacks("libx265");
    app.ffmpeg.updated();
    clips_are(&app, ClipCodec::H265);
    app.invoke("remove_stretch", json!({ "id": clip.id, "from": 1.0, "to": 2.0 })).unwrap();
    let after = app.made(clip.id);
    assert_eq!((after.status, after.error.as_deref()), (ClipStatus::Error, Some(NO_H265)));
    assert_eq!(footage(&app), Vec::<std::ffi::OsString>::new(), "footage fetched for it");
}

/// While an edit's render waits on ffmpeg's list of encoders, the library
/// isn't held: the app goes on (review M2).
#[test]
fn an_edit_s_render_holds_no_library_while_ffmpeg_lists_its_encoders() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = edit_bar_clip(&app, vod.id, (0.0, 60.0));
    let listing = vodscribe_core::testkit::Gate::new();
    app.ffmpeg.holds_list(&listing);
    app.ffmpeg.updated();
    app.invoke("remove_stretch", json!({ "id": clip.id, "from": 1.0, "to": 2.0 })).unwrap();
    listing.reached();
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = app.handle().clone();
    std::thread::spawn(move || {
        drop(handle.state::<AppState>().db.lock().unwrap());
        let _ = tx.send(());
    });
    let free = rx.recv_timeout(std::time::Duration::from_secs(2));
    listing.open();
    assert!(free.is_ok(), "the library was held while ffmpeg listed its encoders");
    assert_eq!(app.made(clip.id).status, ClipStatus::Ready);
}

/// An ffmpeg without AAC makes no MP4: both MP4 formats are greyed out,
/// and a clip in one says so in the app's words, not ffmpeg's; MOV, whose
/// sound is PCM, is made (review L4).
#[test]
fn an_ffmpeg_without_aac_makes_no_mp4() {
    let app = TestApp::new();
    app.ffmpeg.lacks("aac");
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    assert_eq!(greyed(&app), json!(["h264", "h265"]));
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let says = "VODscribe's ffmpeg can't make MP4 H.264. Pick another format in Settings › Encoding.";
    assert_eq!((clip.status, clip.error.as_deref()), (ClipStatus::Error, Some(says)));
    let talk = app.add_file("Talk.m4a", &vodscribe_core::testkit::Media::sound(60.0));
    let sound = app.make_clip(talk.id, "0:10", "0:20");
    assert_eq!(sound.error.as_deref(), Some(says), "an M4A is AAC too");
    clips_are(&app, ClipCodec::Dnxhr);
    let mov = app.make_clip(vod.id, "0:30", "0:40");
    assert_eq!((mov.status, mov.error), (ClipStatus::Ready, None));
}

/// An edit of a sound file and a stretch of a video file since removed is
/// sound alone (the video's piece a hole, with no picture), so a missing
/// picture encoder doesn't refuse it.
#[test]
fn an_edit_whose_only_video_is_a_hole_is_made_as_sound_without_the_picture_s_encoder() {
    let app = TestApp::new();
    let talk = app.add_file("Talk.m4a", &vodscribe_core::testkit::Media::sound(60.0));
    let day = app.add_video_file("Day 3.mp4", 60.0);
    clips_are(&app, ClipCodec::H265);
    let edit = {
        let db = app.db();
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, talk.id, (0.0, 10.0), "a", None).unwrap();
        db.add_piece(edit, day.id, (0.0, 10.0), "b", None).unwrap();
        edit
    };
    let made = app.invoke("add_edit_clip", json!({ "editId": edit, "from": 0.0, "to": 18.0 })).unwrap();
    let clip = app.made(made["id"].as_i64().unwrap());
    std::fs::remove_file(app.db().vod(day.id).unwrap().unwrap().source_path.unwrap()).unwrap();
    app.ffmpeg.lacks("libx265");
    app.ffmpeg.updated();
    app.invoke("remove_stretch", json!({ "id": clip.id, "from": 1.0, "to": 2.0 })).unwrap();
    let after = app.made(clip.id);
    assert_eq!((after.status, after.error), (ClipStatus::Ready, None));
    assert!(after.file_path.is_some_and(|f| f.ends_with(".m4a")), "sound alone");
}

/// An edit of VOD `vod_id`'s 0:05–0:15: Delete video keeps that stretch as
/// a video of its own first (`realize`).
fn in_an_edit(app: &TestApp, vod_id: i64) -> i64 {
    let db = app.db();
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod_id, (5.0, 15.0), "a", None).unwrap();
    edit
}

/// The value after `flag` in the ffmpeg run that encoded with `encoder`.
fn after(app: &TestApp, encoder: &str, flag: &str) -> Option<String> {
    let runs = app.ffmpeg.runs();
    let run = runs.iter().find(|r| r.iter().any(|a| a == encoder))?;
    run.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

/// Remove and delete the video on a VOD an edit uses, then the file the
/// edit's stretch was kept in (`realize`).
fn delete_kept(app: &TestApp, vod_id: i64) -> String {
    let edit = in_an_edit(app, vod_id);
    let answer = app.invoke("delete_vod", json!({ "id": vod_id, "deleteVideo": true })).unwrap();
    assert_eq!(answer, Value::Null, "the delete refused");
    let kept = app.db().piece_rows(edit).unwrap()[0].vod_id;
    app.db().media_path(kept).unwrap().unwrap()
}

/// The stretch an edit keeps replaces the VOD's footage, which goes next:
/// it's made as before, H.264 at CRF 18, whatever quality clips are made in
/// (re-review of fix-codecs-073, 1).
#[test]
fn a_kept_stretch_is_made_as_before_whatever_the_clips_quality() {
    let app = TestApp::new();
    app.invoke("set_settings", json!({ "settings": { "clip_codec": "h264", "clip_quality": "proxy" } })).unwrap();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let file = delete_kept(&app, vod.id);
    assert!(file.ends_with(".mp4"), "kept as {file}");
    assert_eq!(after(&app, "libx264", "-crf").as_deref(), Some("18"));
}

/// The same with clips made as MOV DNxHR (after Switch to MOV, say): an
/// ffmpeg with H.264 keeps the stretch as before.
#[test]
fn a_kept_stretch_is_made_as_before_with_clips_made_as_mov() {
    let app = TestApp::new();
    app.invoke("set_settings", json!({ "settings": { "clip_codec": "dnxhr", "clip_format": "mov" } })).unwrap();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let file = delete_kept(&app, vod.id);
    assert!(file.ends_with(".mp4"), "kept as {file}");
    assert_eq!(after(&app, "libx264", "-crf").as_deref(), Some("18"));
}

/// With an ffmpeg without H.264, the stretch is kept in the codec Settings ›
/// Encoding names, at High quality, so the delete goes ahead as clips are
/// made (correctness review 0.7.3, 1).
#[test]
fn without_h264_a_kept_stretch_is_made_in_the_chosen_codec_at_high_quality() {
    let app = TestApp::new();
    app.ffmpeg.lacks("libx264");
    app.invoke("set_settings", json!({ "settings": { "clip_codec": "dnxhr", "clip_quality": "proxy" } })).unwrap();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let file = delete_kept(&app, vod.id);
    assert!(file.ends_with(".mov"), "kept as {file}");
    assert_eq!(after(&app, "dnxhd", "-profile:v").as_deref(), Some("dnxhr_hq"));
}

/// Without H.264 and the chosen codec either, Remove and delete the video
/// refuses with the app's sentence, as a clip in it fails, and keeps the
/// VOD.
#[test]
fn without_h264_or_the_chosen_codec_delete_video_refuses_saying_which() {
    let app = TestApp::new();
    app.ffmpeg.lacks("libx264");
    app.ffmpeg.lacks("libx265");
    clips_are(&app, ClipCodec::H265);
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    in_an_edit(&app, vod.id);
    let answer = app.invoke("delete_vod", json!({ "id": vod.id, "deleteVideo": true })).unwrap();
    assert_eq!(answer, json!(NO_H265));
    assert!(app.db().vod(vod.id).unwrap().is_some(), "the VOD kept");
}
