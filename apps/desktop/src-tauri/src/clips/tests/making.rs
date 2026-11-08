use super::*;

use crate::testkit::TestApp;

/// Several clips made at once went in one at a time: one that failed
/// part-way left those before it in the library, waiting to be made, with
/// no step to undo them.
#[test]
fn making_several_clips_is_all_or_none() {
    let (db, vod) = library();
    let history = history::History::default();
    assert!(make_clips_in(&db, &history, vod, vec![(1.0, 2.0), (-1.0, 3.0)], true).is_err());
    assert!(db.clips(vod).unwrap().is_empty(), "none made");
    assert_eq!(history.steps(Doc::Vod(vod)), 0, "no step recorded");
    let made = make_clips_in(&db, &history, vod, vec![(1.0, 2.0), (3.0, 90.0)], true).unwrap();
    assert_eq!(made.iter().map(|c| (c.start, c.end)).collect::<Vec<_>>(), [(1.0, 2.0), (3.0, 60.0)]);
    assert_eq!(history.steps(Doc::Vod(vod)), 1, "one step for both");

    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (0.0, 30.0), "a", None).unwrap();
    let doc = Doc::Edit(edit);
    let before = history.steps(doc);
    assert!(make_edit_clips_in(&db, &history, edit, vec![(1.0, 2.0), (40.0, 50.0)], true).is_err());
    assert!(db.bar_clips(edit).unwrap().is_empty(), "none made on the edit's bar");
    assert_eq!(history.steps(doc), before, "no step recorded");
    let (made, queued) = make_edit_clips_in(&db, &history, edit, vec![(1.0, 2.0), (3.0, 4.0)], true).unwrap();
    assert_eq!(made.iter().map(|c| c.id).collect::<Vec<_>>(), queued);
    assert_eq!(history.steps(doc), before + 1);
}

/// A clip is its stretch of the VOD, made from the VOD's own file when it's
/// one on this computer (crates/core/src/clips.rs: clips "start and end on
/// the exact frame asked for"; this file's doc: "the VOD's file where it
/// plays from one here").
#[test]
fn a_clip_of_a_file_is_its_stretch_of_that_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let media = app.clip_media(&clip);
    let (file, start) = media.cut_from.unwrap();
    assert_eq!(Some(file), vod.source_path);
    assert!((start - 10.0).abs() < 0.001, "starts at {start}");
    assert!((media.duration - 10.0).abs() < 0.001, "lasts {}", media.duration);
}

/// A clip is titled with the first words said in it, or failing that where
/// it starts (crates/core/src/clips.rs, `title`).
#[test]
fn a_clip_is_titled_with_what_is_said_in_it() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 120.0);
    app.says(vod.id, 12.0, "The Eagle has landed");
    let said = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    let silent = app.add_clip(vod.id, "1:05", "1:10").unwrap();
    assert_eq!((said.title.as_str(), silent.title.as_str()), ("The Eagle has landed", "Clip at 1:05"));
    // Their files made, so nothing of this test runs on after it.
    app.made(said.id);
    app.made(silent.id);
}
