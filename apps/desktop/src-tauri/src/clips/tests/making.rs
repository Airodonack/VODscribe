//! Making clips: one or several, of a VOD or an edit, kept from a drag,
//! renamed and resized, and what undo makes of each. Every test goes
//! through the commands the page calls (`add_clip`, `add_clips`,
//! `add_piece_clip`, `add_piece_clips`,
//! `add_edit_clip`, `add_edit_clips`, `list_clips`, `edit_clips`,
//! `keep_clip`, `rename_clip`, `set_clip_range`, `undo`, `redo`) and reads
//! what they leave: the library, the clips' files, the news the page hears.

use super::*;

use serde_json::{Value, json};
use vodscribe_core::testkit::{Gate, Media, eventually};

use crate::testkit::{TestApp, at};

/// A clip is its stretch of the VOD, made from the VOD's own file when it's
/// one on this computer (the module doc of crates/core/src/clips.rs: clips
/// "start and end on the exact frame asked for"; app `clips.rs`'s module
/// doc: "the VOD's file where it plays from one here").
#[test]
fn a_clip_of_a_file_is_its_stretch_of_that_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let media = app.clip_media(&clip);
    let file = vod.source_path.as_deref().unwrap();
    assert!(cut_from(&media, file, 10.0) && near(media.duration, 10.0), "made as {media:?}");
}

/// A clip is titled with the first words said in it, or failing that where
/// it starts (crates/core/src/clips.rs `title`: "the first words said in it,
/// or failing that where it starts"; app `clips.rs` `add_clip`: "titled with
/// what's said in it").
#[test]
fn a_clip_is_titled_with_what_is_said_in_it() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 120.0);
    app.says(vod.id, 12.0, "The Eagle has landed");
    let said = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    let silent = app.add_clip(vod.id, "1:05", "1:10").unwrap();
    assert_eq!((said.title.as_str(), silent.title.as_str()), ("The Eagle has landed", "Clip at 1:05"));
    // Their files made, so nothing of this test runs on after it.
    made_all(&app, &[said, silent]);
}

/// A clip asked for past the VOD's end ends where the VOD does, and its
/// file is that long (app `clips.rs` `make_clips`: "its end kept within the
/// VOD"; crates/core/src/clips.rs `check_span`: "an end past the video's is
/// cut there by whoever keeps the span").
#[test]
fn a_clip_s_end_past_the_vod_s_is_cut_there() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:50", "1:30");
    assert_eq!((clip.start, clip.end), (50.0, 60.0));
    let media = app.clip_media(&clip);
    assert!(near(media.duration, 10.0), "lasts {}", media.duration);
}

/// A clip that would start where the VOD has ended is refused, saying so
/// (crates/core/src/clips.rs `PAST_THE_END`: "That's past the end of the
/// video.", and `check_span`: "Every span a command is given is checked
/// here").
#[test]
fn a_clip_starting_past_the_vod_s_end_says_so() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    assert_eq!(app.add_clip(vod.id, "1:00", "1:20").unwrap_err(), clips::PAST_THE_END);
    assert!(app.clips(vod.id).is_empty(), "a clip made");
}

/// A clip whose end comes before its start is refused, saying so
/// (crates/core/src/clips.rs `ENDS_BEFORE_START`: "A clip has to end after it
/// starts.").
#[test]
fn a_clip_ending_before_it_starts_says_so() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    assert_eq!(app.add_clip(vod.id, "0:20", "0:10").unwrap_err(), clips::ENDS_BEFORE_START);
    assert_eq!(app.add_clip(vod.id, "0:10", "0:10").unwrap_err(), clips::ENDS_BEFORE_START);
    assert!(app.clips(vod.id).is_empty(), "a clip made");
}

/// A clip of a VOD taken out of the library is refused, saying so (app
/// `clips.rs` `make_clips_in`: "That VOD is gone.").
#[test]
fn a_clip_of_a_vod_that_is_gone_says_so() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    assert_eq!(app.invoke("delete_vod", json!({ "id": vod.id })), Ok(Value::Null));
    assert_eq!(app.add_clip(vod.id, "0:10", "0:20").unwrap_err(), "That VOD is gone.");
}

/// Several stretches clipped at once go on the clip bar in the order they
/// were picked (app `clips.rs` `add_clips`: "in its clip bar, in that
/// order"; CHANGELOG.md, 0.6.0: "Clip and delete several at once." — "C
/// (or Clip) makes a clip of each in the order you picked them";
/// tests/e2e/scenarios/multi-select.mjs: "C makes a clip of each, in that
/// order, as one change").
#[test]
fn clips_made_at_once_go_on_the_bar_in_the_order_given() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let made = add_clips(&app, vod.id, &[("0:30", "0:40"), ("0:10", "0:20")]).unwrap();
    let starts = |clips: &[Clip]| clips.iter().map(|c| c.start).collect::<Vec<_>>();
    assert_eq!(starts(&made), [30.0, 10.0]);
    assert_eq!(starts(&app.clips(vod.id)), [30.0, 10.0]);
    made_all(&app, &made);
}

/// Several clips made at once are one change: one undo takes them all
/// back (app `clips.rs` `add_clips`: "as one change undo takes back
/// whole"; CHANGELOG.md, 0.6.0: "Clip and delete several at once." — "in
/// one go that a single undo brings back").
#[test]
fn clips_made_at_once_are_one_step_that_undo_takes_back_whole() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let made = add_clips(&app, vod.id, &[("0:10", "0:20"), ("0:30", "0:40")]).unwrap();
    made_all(&app, &made);
    assert!(undo(&app, Doc::Vod(vod.id)).is_some());
    assert!(app.clips(vod.id).is_empty(), "left after one undo: {:?}", app.clips(vod.id));
    assert_eq!(undo(&app, Doc::Vod(vod.id)), None, "a step besides the clips' one");
}

/// Several clips asked for at once, one of which can't be made, make none:
/// the clip bar is as it was, with nothing to undo (app `clips.rs`
/// `make_clips`: "as one change (all of them, or none if one can't be
/// made)").
#[test]
fn clips_made_at_once_are_all_made_or_none_is() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    assert_eq!(add_clips(&app, vod.id, &[("0:10", "0:20"), ("1:10", "1:20")]).unwrap_err(), clips::PAST_THE_END);
    assert!(app.clips(vod.id).is_empty(), "made: {:?}", app.clips(vod.id));
    assert_eq!(undo(&app, Doc::Vod(vod.id)), None, "a step recorded");
}

/// Undo names what it takes back: one clip added is "Clip added", several
/// at once "Clips added" (app `clips.rs` `make_clips_in`, the labels;
/// tests/e2e/scenarios/marks-undo.mjs: "each told in a toast").
#[test]
fn one_clip_added_is_undone_as_clip_added_and_several_as_clips_added() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let one = app.make_clip(vod.id, "0:05", "0:08");
    let several = add_clips(&app, vod.id, &[("0:10", "0:20"), ("0:30", "0:40")]).unwrap();
    made_all(&app, &several);
    assert_eq!(undo(&app, Doc::Vod(vod.id)).as_deref(), Some("Clips added"));
    assert_eq!(undo(&app, Doc::Vod(vod.id)).as_deref(), Some("Clip added"));
    assert!(app.clip(one.id).is_none());
}

/// A clip made only to be dragged out of the window isn't on the clip bar
/// (app `clips.rs` `add_clip`: "in the clip bar unless `shown` is false
/// (one only dragged out of the window)"; CHANGELOG.md, 0.7.1: "Dropped
/// clips show in the jobs list." — "A clip made only for a drag stays off
/// the clip bar").
#[test]
fn a_clip_only_dragged_out_is_off_the_clip_bar() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let pill = app.make_clip(vod.id, "0:05", "0:08");
    let dragged = drag_clip_of(&app, vod.id, "0:10", "0:20");
    assert!(!dragged.shown);
    let bar: Vec<i64> = list_clips(&app, vod.id).iter().filter(|c| c.shown).map(|c| c.id).collect();
    assert_eq!(bar, [pill.id], "the clip bar");
}

/// A clip made only to be dragged out leaves nothing to undo: the clip bar
/// is as it was (crates/core/src/db.rs `add_clip`: "One only dragged out
/// isn't in the bar: no change to the VOD's clip").
#[test]
fn a_clip_only_dragged_out_adds_no_step_to_undo() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let dragged = drag_clip_of(&app, vod.id, "0:10", "0:20");
    assert_eq!(undo(&app, Doc::Vod(vod.id)), None);
    assert!(app.clip(dragged.id).is_some(), "the dragged clip is gone");
}

/// A clip dragged out and dropped on the clip bar is kept there, as a
/// change undo takes back (app `clips.rs` `keep_clip`: "Puts clip `id`
/// (one dragged out of the window) in the clip bar"; CHANGELOG.md, 0.7.1:
/// "A drag called off makes nothing." — "dropped on the clip bar, it's kept
/// there"; tests/e2e/scenarios/drag-out.mjs: "on the clip bar, it's a clip
/// there, and stays").
#[test]
fn a_clip_kept_from_a_drag_joins_the_clip_bar_as_a_step() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let dragged = drag_clip_of(&app, vod.id, "0:10", "0:20");
    let on_bar = || list_clips(&app, vod.id).iter().any(|c| c.id == dragged.id && c.shown);
    let kept = app.run(keep_clip(app.handle().clone(), dragged.id)).unwrap();
    assert!(kept.shown && on_bar(), "not on the clip bar");
    assert_eq!(undo(&app, Doc::Vod(vod.id)).as_deref(), Some("Clip added"));
    assert!(!on_bar(), "still on the clip bar once undone");
}

/// A clip kept from a drag while it's being made: the newest news of it the
/// page hears says both, on the clip bar and made, whichever came last
/// (the comment in app `clips.rs` `keep_clip`: "News of it too, in order
/// with the cutter's (the page keeps the newest)").
#[test]
fn a_kept_clip_s_news_comes_in_order_with_its_making_s() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let dragged = app.run(add_clip(app.handle().clone(), vod.id, at("0:10"), at("0:20"), Some(false))).unwrap();
    halfway.reached();
    app.run(keep_clip(app.handle().clone(), dragged.id)).unwrap();
    halfway.open();
    app.made(dragged.id);
    let newest = || app.heard("clip").into_iter().rfind(|c| c["id"] == dragged.id);
    eventually("the clip's news that it's made", || newest().is_some_and(|c| c["status"] == "ready"));
    let newest = newest().unwrap();
    assert_eq!(newest["shown"], json!(true), "the newest news: {newest}");
}

/// A clip's new name is trimmed and kept to 200 characters (app `clips.rs`
/// `rename_clip`; crates/core/src/clips.rs `written_title`: "trimmed, at
/// most 200 characters"; CHANGELOG.md, 0.4.1: "Enter or F2 renames a clip").
#[test]
fn a_renamed_clip_has_its_name_trimmed_and_capped() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    assert_eq!(rename(&app, clip.id, "  The landing  ").unwrap().title, "The landing");
    let long = format!("  {}  ", "a".repeat(250));
    assert_eq!(rename(&app, clip.id, &long).unwrap().title, "a".repeat(200));
    assert_eq!(app.clip(clip.id).unwrap().title, "a".repeat(200), "as the library has it");
}

/// A clip can't be renamed to nothing, or to spaces: its name stays (app
/// `clips.rs` `rename_clip`: "A clip needs a name.").
#[test]
fn a_clip_cannot_be_renamed_to_nothing() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    assert_eq!(rename(&app, clip.id, "   ").unwrap_err(), "A clip needs a name.");
    assert_eq!(app.clip(clip.id).unwrap().title, clip.title);
}

/// A rename is one change, which undo names and takes back (app `clips.rs`
/// `rename_clip`: "Clip renamed").
#[test]
fn a_rename_is_one_step_undo_takes_back() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    rename(&app, clip.id, "The landing").unwrap();
    assert_eq!(undo(&app, Doc::Vod(vod.id)).as_deref(), Some("Clip renamed"));
    assert_eq!(app.clip(clip.id).unwrap().title, clip.title);
}

/// A clip moved to a new range is made again for it (app `clips.rs`
/// `set_clip_range`: "its file is made again"; tests/e2e/scenarios/clip.mjs:
/// "resized, it's made again, a new file in its place").
#[test]
fn a_resized_clip_is_made_again_for_its_new_range() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    resize(&app, clip.id, "0:30", "0:35").unwrap();
    let clip = app.made(clip.id);
    assert_eq!((clip.start, clip.end), (30.0, 35.0));
    let media = app.clip_media(&clip);
    let file = vod.source_path.as_deref().unwrap();
    assert!(cut_from(&media, file, 30.0) && near(media.duration, 5.0), "made as {media:?}");
}

/// A clip made again for a new range is a new file in the library, named
/// by its id and which make it is, and the one before goes (app `clips.rs`
/// `set_clip_range`: "made again, as a new file"; decided: every make writes
/// a new file of its own; CHANGELOG.md, 0.7.2: "Clips now live in the
/// library.").
#[test]
fn a_resized_clip_is_made_again_as_a_new_file_in_the_library() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let first = app.paths().clip_make(clip.id, 1, "mp4");
    assert_eq!(clip.file_path.as_deref().map(Path::new), Some(first.as_path()));
    resize(&app, clip.id, "0:30", "0:35").unwrap();
    let clip = app.made(clip.id);
    let second = app.paths().clip_make(clip.id, 2, "mp4");
    assert_eq!(clip.file_path.as_deref().map(Path::new), Some(second.as_path()));
    assert!(near(Media::read(&second).unwrap().duration, 5.0), "the file there isn't the new range's");
    assert!(!first.exists(), "the file before stayed");
}

/// A resize undone, then redone, makes the clip again each time, on a VOD's
/// clip bar and on an edit's (CHANGELOG.md, 0.7.1 Fixes: "Undoing a resize
/// makes the clip again." — "On an edit's clip bar, Undo or Redo of a
/// clip's new length left its video as it was"; tests/e2e/scenarios/clip.mjs:
/// "undoing that and the resize").
#[test]
fn a_resize_undone_or_redone_makes_the_clip_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit = edit_of(&app, vod.id, &[("0:00", "0:30")]);
    let of_vod = app.make_clip(vod.id, "0:10", "0:20");
    let of_edit = add_edit_clip_of(&app, edit, "0:10", "0:20").unwrap();
    app.made(of_edit.id);
    for (clip, doc) in [(of_vod, Doc::Vod(vod.id)), (of_edit, Doc::Edit(edit))] {
        let lasts = |id| app.clip_media(&app.made(id)).duration;
        resize(&app, clip.id, "0:12", "0:15").unwrap();
        assert!(near(lasts(clip.id), 3.0), "{doc:?}: resized, lasts {}", lasts(clip.id));
        assert_eq!(undo(&app, doc).as_deref(), Some("Clip resized"));
        assert!(near(lasts(clip.id), 10.0), "{doc:?}: undone, lasts {}", lasts(clip.id));
        assert_eq!(redo(&app, doc).as_deref(), Some("Clip resized"));
        assert!(near(lasts(clip.id), 3.0), "{doc:?}: redone, lasts {}", lasts(clip.id));
    }
}

/// A clip resized while its file is being made is made for its new range,
/// not left as the old one (the comment in app `clips.rs` `cut`: "Stopped:
/// moved (it's queued again)"; crates/core/src/db.rs `set_clip_file`:
/// "unless its range has moved since `start`..`end` seconds was cut").
#[test]
fn a_clip_resized_while_it_is_made_is_made_for_its_new_range() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    halfway.reached();
    resize(&app, clip.id, "0:30", "0:35").unwrap();
    halfway.open();
    let clip = app.made(clip.id);
    assert_eq!((clip.start, clip.end), (30.0, 35.0));
    let media = app.clip_media(&clip);
    let file = vod.source_path.as_deref().unwrap();
    assert!(cut_from(&media, file, 30.0) && near(media.duration, 5.0), "made as {media:?}");
}

/// A piece added to a clip in the editor while it's being cut stops the cut,
/// and the clip is made again of both pieces (edits.rs `remake_snippet`:
/// "its file is made again, as a clip moved in its clip bar is").
#[test]
fn a_piece_added_while_its_clip_is_cut_is_in_its_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    halfway.reached();
    let changed = app.ffmpeg.runs().len();
    app.invoke("add_range_to_edit", json!({ "editId": clip.id, "vodId": vod.id, "start": 30.0, "end": 40.0 })).unwrap();
    app.ffmpeg.works();
    halfway.open();
    let (made, again) = made_again_since(&app, clip.id, changed);
    assert_eq!(made.status, ClipStatus::Ready, "{:?}", made.error);
    assert!(again, "not made again after the change");
}

/// The same as the cut ends, its ffmpeg done and the library not yet taken:
/// the clip is recorded as made of what was cut, not of what it holds by
/// then, so the change makes it again (`cut`, as `edits::render_edit` does).
/// The window is held open by holding the library as ffmpeg ends, and the
/// change is `add_range_to_edit`'s own steps under that lock.
#[test]
fn a_piece_added_as_its_clip_s_cut_ends_is_in_its_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let held = Gate::new();
    app.ffmpeg.holds_at(0.5, &held);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    held.reached();
    let out = app.paths().clip_make(clip.id, 1, "mp4");
    let changed;
    {
        let db = app.db();
        app.ffmpeg.works();
        held.open();
        eventually("the cut's file written", || out.is_file());
        changed = app.ffmpeg.runs().len();
        history::change(app.handle(), &db, Doc::Edit(clip.id), "Piece added", |db| {
            db.add_piece(clip.id, vod.id, (30.0, 40.0), "b", None)
        })
        .unwrap();
    }
    let (made, again) = made_again_since(&app, clip.id, changed);
    assert_eq!(made.status, ClipStatus::Ready, "{:?}", made.error);
    assert!(again, "made and current with its first piece alone: {:?}", made.file_path);
}

/// The same with another clip ahead of its remake in the queue: till that
/// remake, the clip waits, and isn't handed out with the file of its first
/// piece alone; a drag meanwhile waits for the remake and gets it
/// (`status_once_made`).
#[test]
fn a_clip_changed_as_its_cut_ends_is_not_handed_out_before_its_remake() {
    use crate::testkit::{LetGo, Receiver};
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let first = Gate::new();
    app.ffmpeg.holds_at(0.5, &first);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    first.reached();
    // Another clip waits its turn, as when several are made at once.
    app.add_clip(vod.id, "0:45", "0:50").unwrap();
    let out = app.paths().clip_make(clip.id, 1, "mp4");
    let second = Gate::new();
    let cut;
    {
        let db = app.db();
        // The next run (the other clip's) holds; this one goes on.
        app.ffmpeg.holds_at(0.5, &second);
        first.open();
        eventually("the cut's file written", || out.is_file());
        cut = Media::read(&out);
        history::change(app.handle(), &db, Doc::Edit(clip.id), "Piece added", |db| {
            db.add_piece(clip.id, vod.id, (30.0, 40.0), "b", None)
        })
        .unwrap();
    }
    // The other clip is being cut; the remake waits behind it.
    second.reached();
    let now = app.clip(clip.id).unwrap();
    app.drag_out(&[clip.id]).unwrap();
    let drag = app.drags().last();
    drag.let_go(LetGo::On(Receiver::taking_files("Kdenlive")));
    second.open();
    app.drag_ended();
    assert_ne!(now.status, ClipStatus::Ready, "says made while its remake waits, with {:?}", now.file_path);
    let given = drag.given().first().and_then(|g| Media::read(g));
    assert!(given.is_some() && given != cut, "a drag in that window was given its first piece alone: {given:?}");
}

/// A clip of an edit is titled with what's said at its start, not later in
/// it (app `clips.rs` `add_edit_clip`: "titled with what's said at its
/// start").
#[test]
fn a_clip_of_an_edit_is_titled_with_what_is_said_at_its_start() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.says(vod.id, 12.0, "The Eagle has landed");
    app.says(vod.id, 42.0, "Houston");
    // The edit plays 0:10-0:20 of the VOD, then 0:40-0:50.
    let edit = edit_of(&app, vod.id, &[("0:10", "0:20"), ("0:40", "0:50")]);
    let clip = add_edit_clip_of(&app, edit, "0:00", "0:20").unwrap();
    assert_eq!(clip.title, "The Eagle has landed");
    app.made(clip.id);
}

/// The same stretch of an edit clipped twice gives the one clip (app
/// `clips.rs` `make_edit_clips`: "A range with a clip already (C pressed
/// twice) gives that one"; CHANGELOG.md, 0.7.0 Fixes: "C twice in an edit
/// makes one clip.").
#[test]
fn clipping_an_edit_s_range_twice_gives_the_one_clip() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit = edit_of(&app, vod.id, &[("0:00", "0:30")]);
    let first = add_edit_clip_of(&app, edit, "0:10", "0:20").unwrap();
    app.made(first.id);
    let again = add_edit_clip_of(&app, edit, "0:10", "0:20").unwrap();
    assert_eq!(again.id, first.id);
    assert_eq!(edit_clips_of(&app, edit).len(), 1);
    // Given as it is, made: not made again.
    assert_eq!(again.status, ClipStatus::Ready, "made again");
    assert!(!app.state().clips.is_waiting(first.id), "queued to be made again");
}

/// A stretch of an edit whose clip failed, clipped again, is made again
/// (app `clips.rs` `make_edit_clips`: "made again only if it failed";
/// CHANGELOG.md, 0.7.0 Fixes: "C twice in an edit makes one clip." —
/// "pressed again on a clip that failed, C tries it again").
#[test]
fn a_failed_clip_of_an_edit_clipped_again_is_made_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit = edit_of(&app, vod.id, &[("0:00", "0:30")]);
    app.ffmpeg.fails_at(0.5, "Error while decoding stream #0:0");
    let first = add_edit_clip_of(&app, edit, "0:10", "0:20").unwrap();
    assert_eq!(app.made(first.id).status, ClipStatus::Error);
    app.ffmpeg.works();
    let again = add_edit_clip_of(&app, edit, "0:10", "0:20").unwrap();
    assert_eq!(again.id, first.id);
    assert!(near(app.clip_media(&app.made(first.id)).duration, 10.0));
}

/// The same stretch of a VOD clipped twice gives the one clip, made: the
/// command keeps the rule, as it does for an edit, not only the page
/// (decided: one clip per stretch, and the command keeps the rule for VODs
/// as for edits; CHANGELOG.md, 0.7.0 Fixes: "Pressing C twice on the same
/// stretch of an edit makes one clip, as on a VOD").
#[test]
fn clipping_a_vod_s_stretch_twice_gives_the_one_clip() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let first = app.make_clip(vod.id, "0:10", "0:20");
    let again = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    assert_eq!(again.id, first.id);
    assert_eq!(list_clips(&app, vod.id).len(), 1);
    // Given as it is, made: not made again.
    assert_eq!(again.status, ClipStatus::Ready, "made again");
    assert!(!app.state().clips.is_waiting(first.id), "queued to be made again");
    // One asked for past the VOD's end is the stretch it was cut to.
    let to_the_end = app.make_clip(vod.id, "0:50", "1:00");
    assert_eq!(app.add_clip(vod.id, "0:50", "1:30").unwrap().id, to_the_end.id);
}

/// C on the stretch a clip of the clip bar is listed at gives that clip,
/// though it was changed in the editor (a piece added after its first): the
/// page finds it there and asks no download note, so the command gives the
/// same one, as 0.7.2's page did (one clip per stretch, as above;
/// clips.svelte.ts `find`: "The clip of exactly `start`..`end`").
#[test]
fn c_on_the_stretch_of_a_clip_changed_in_the_editor_gives_that_clip() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let first = app.make_clip(vod.id, "0:10", "0:20");
    let range = json!({ "editId": first.id, "vodId": vod.id, "start": 30.0, "end": 32.0 });
    app.invoke("add_range_to_edit", range).unwrap();
    let changed = app.made(first.id);
    let listed = list_clips(&app, vod.id);
    assert_eq!((listed.len(), listed[0].start, listed[0].end), (1, 10.0, 20.0), "listed at its first piece");
    let again = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    assert_eq!(again.id, first.id, "a second clip of that stretch");
    assert_eq!(list_clips(&app, vod.id).len(), 1);
    assert_eq!(app.clip_media(&app.made(first.id)), app.clip_media(&changed), "made again");
}

/// A piece of an edit carried out of the window is a clip that plays just
/// that piece: a clip of the VOD's clip bar listed at its stretch but changed
/// in the editor (a piece added) isn't it, though one that plays just the
/// stretch is given, made already (dragout.ts `carry`; CHANGELOG.md, 0.7.3
/// Fixes: "A dragged piece reuses its clip.").
#[test]
fn a_piece_carried_out_is_a_clip_that_plays_just_it() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let changed = app.make_clip(vod.id, "0:10", "0:20");
    let range = json!({ "editId": changed.id, "vodId": vod.id, "start": 30.0, "end": 32.0 });
    app.invoke("add_range_to_edit", range).unwrap();
    app.made(changed.id);
    let carried = app.run(add_piece_clip(app.handle().clone(), vod.id, 10.0, 20.0, Some(false))).unwrap();
    assert_ne!(carried.id, changed.id, "the changed clip handed out for the piece");
    let carried = app.made(carried.id);
    assert!(near(app.clip_media(&carried).duration, 10.0));
    let whole = app.make_clip(vod.id, "0:40", "0:50");
    let again = app.run(add_piece_clip(app.handle().clone(), vod.id, 40.0, 50.0, Some(false))).unwrap();
    assert_eq!((again.id, again.status), (whole.id, ClipStatus::Ready), "made again for a piece it plays");
}

/// Stretches of a VOD dragged and dropped on its clip bar (several picked)
/// are its footage too: a clip listed at one but changed in the editor
/// isn't given for it, and the page can tell it from one that plays just
/// its stretch, which is given (`Clip::plain`; dragout.ts `carrySet`).
#[test]
fn stretches_dragged_to_the_clip_bar_are_clips_that_play_just_them() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let changed = app.make_clip(vod.id, "0:10", "0:20");
    let range = json!({ "editId": changed.id, "vodId": vod.id, "start": 30.0, "end": 32.0 });
    app.invoke("add_range_to_edit", range).unwrap();
    let whole = app.make_clip(vod.id, "0:40", "0:50");
    let plain: Vec<_> = list_clips(&app, vod.id).iter().map(|c| (c.id, c.plain)).collect();
    assert_eq!(plain, [(changed.id, false), (whole.id, true)], "told apart as listed");
    let ranges = spans(&[("0:10", "0:20"), ("0:40", "0:50")]);
    let made = app.run(crate::clips::add_piece_clips(app.handle().clone(), vod.id, ranges)).unwrap();
    assert_ne!(made[0].id, changed.id, "the changed clip given for the stretch");
    assert!(made[0].plain && made[0].shown);
    assert_eq!(made[1].id, whole.id);
    assert!(near(app.clip_media(&app.made(made[0].id)).duration, 10.0));
}

/// Several stretches clipped at once, one with a clip already, give that
/// one and make the others, in their order (one clip per stretch, as above;
/// CHANGELOG.md, 0.6.0: "Clip and delete several at once.").
#[test]
fn clips_made_at_once_give_the_one_made_already() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let first = app.make_clip(vod.id, "0:10", "0:20");
    let made = add_clips(&app, vod.id, &[("0:30", "0:40"), ("0:10", "0:20")]).unwrap();
    assert_eq!(made[1].id, first.id);
    assert_eq!(list_clips(&app, vod.id).len(), 2);
    made_all(&app, &made);
}

/// A stretch of a VOD whose clip failed, clipped again, is made again
/// (one clip per stretch, as above; CHANGELOG.md, 0.7.0 Fixes: "pressing it
/// again on a clip that failed tries it again", for an edit "as on a VOD").
#[test]
fn a_failed_clip_of_a_vod_clipped_again_is_made_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.ffmpeg.fails_at(0.5, "Error while decoding stream #0:0");
    let first = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    assert_eq!(app.made(first.id).status, ClipStatus::Error);
    app.ffmpeg.works();
    let again = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    assert_eq!(again.id, first.id);
    assert_eq!((again.status, again.error), (ClipStatus::Waiting, None));
    assert!(near(app.clip_media(&app.made(first.id)).duration, 10.0));
}

/// A stretch whose clip was made only for a drag out of the window, clipped,
/// is that clip put on the clip bar; a drag of a stretch on the bar is the
/// bar's clip (one clip per stretch, as above; app `clips.rs` `add_clip`:
/// "in the clip bar unless `shown` is false (one only dragged out of the
/// window)").
#[test]
fn a_stretch_dragged_out_and_clipped_is_one_clip() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let dragged = drag_clip_of(&app, vod.id, "0:10", "0:20");
    let kept = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    assert_eq!(kept.id, dragged.id);
    assert!(kept.shown, "still off the clip bar");
    assert_eq!(undo(&app, Doc::Vod(vod.id)).as_deref(), Some("Clip added"));
    let pill = app.make_clip(vod.id, "0:30", "0:40");
    assert_eq!(drag_clip_of(&app, vod.id, "0:30", "0:40").id, pill.id);
    assert!(app.clip(pill.id).unwrap().shown, "taken off the clip bar by a drag");
}

/// A clip of an edit that would start where the edit has ended is refused,
/// saying so (crates/core/src/edits.rs `check_edit_span`: "a span of
/// clip `clip`'s timeline (an edit's: as long as what its pieces play),
/// `clips::check_span`").
#[test]
fn a_clip_past_an_edit_s_end_says_so() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit = edit_of(&app, vod.id, &[("0:00", "0:30")]);
    assert_eq!(add_edit_clip_of(&app, edit, "0:30", "0:50").unwrap_err(), clips::PAST_THE_END);
    assert!(edit_clips_of(&app, edit).is_empty());
}

/// Several stretches of an edit clipped at once are one change undo takes
/// back whole, and one that can't be made makes none (app `clips.rs`
/// `add_edit_clips`: "as one change undo takes back whole"; `make_edit_clips`:
/// "as one change (all of them, or none if one can't be made)").
#[test]
fn clips_of_an_edit_made_at_once_are_one_step_and_all_or_none() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit = edit_of(&app, vod.id, &[("0:00", "0:30")]);
    let refused = add_edit_clips(&app, edit, &[("0:01", "0:02"), ("0:40", "0:50")]);
    assert_eq!(refused.unwrap_err(), clips::PAST_THE_END);
    assert!(edit_clips_of(&app, edit).is_empty(), "made: {:?}", edit_clips_of(&app, edit));
    let made = add_edit_clips(&app, edit, &[("0:01", "0:02"), ("0:03", "0:04")]).unwrap();
    assert_eq!(made.len(), 2);
    made_all(&app, &made);
    assert_eq!(undo(&app, Doc::Edit(edit)).as_deref(), Some("Clips added"));
    assert!(edit_clips_of(&app, edit).is_empty(), "left after one undo: {:?}", edit_clips_of(&app, edit));
    // The step before is the edit's piece: the refused clips recorded none.
    assert_eq!(undo(&app, Doc::Edit(edit)).as_deref(), Some("Piece added"));
}

/// An edit's clip bar lists its clips in their order: each one clipped goes
/// after those before it, wherever its stretch is (app `clips.rs`
/// `edit_clips`: "The clips on edit `id`'s clip bar, in their order").
#[test]
fn an_edit_s_clip_bar_lists_its_clips_in_their_order() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit = edit_of(&app, vod.id, &[("0:00", "0:30")]);
    let later = add_edit_clip_of(&app, edit, "0:20", "0:25").unwrap();
    let earlier = add_edit_clip_of(&app, edit, "0:01", "0:05").unwrap();
    let listed: Vec<i64> = edit_clips_of(&app, edit).iter().map(|c| c.id).collect();
    assert_eq!(listed, [later.id, earlier.id]);
    made_all(&app, &[later, earlier]);
}

/// A VOD's clip bar lists its clips in their order: each one clipped goes
/// after those before it, wherever its stretch is (app `clips.rs`
/// `list_clips`; crates/core/src/db.rs `clips`: "VOD `vod_id`'s clips (the
/// snippets on its clip bar), in their order"; db.rs `add_clip`: "after its
/// others on the clip bar").
#[test]
fn a_vod_s_clip_bar_lists_its_clips_in_their_order() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let later = app.add_clip(vod.id, "0:30", "0:40").unwrap();
    let earlier = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    let listed: Vec<i64> = list_clips(&app, vod.id).iter().map(|c| c.id).collect();
    assert_eq!(listed, [later.id, earlier.id]);
    made_all(&app, &[later, earlier]);
}

/// A clip brings the chapters and highlights in its stretch along, on the
/// same moments of what it plays, and leaves those outside it (CHANGELOG.md,
/// 0.5.0: "A clip, a stretch put in an edit, and a clip added to an edit
/// bring the chapters and highlights in them along, on the same moments.").
#[test]
fn a_clip_brings_the_chapters_and_highlights_in_it_on_the_same_moments() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    for (time, kind, text) in [(15.0, "chapter", "Landing"), (18.0, "highlight", "Wow"), (40.0, "chapter", "Later")] {
        app.invoke("add_mark", json!({ "vodId": vod.id, "time": time, "kind": kind, "text": text })).unwrap();
    }
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let marks = app.run(edit_marks(app.handle().clone(), clip.id)).unwrap();
    let marks: Vec<_> = marks.iter().map(|m| (m.kind.as_str(), m.start, m.text.as_deref())).collect();
    assert_eq!(marks, [("chapter", 5.0, Some("Landing")), ("highlight", 8.0, Some("Wow"))], "on the clip's timeline");
}

/// A clip of a site's VOD is cut from a section of it downloaded into the
/// footage folder (app `clips.rs`'s module doc: "or else a section of it
/// downloaded into the footage folder"; CHANGELOG.md, 0.7.1: "Clips start
/// as you pick them up." — "a site's is downloaded").
#[test]
fn a_clip_of_a_site_s_vod_is_cut_from_a_section_downloaded_for_it() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "Test video", 600.0);
    let clip = app.make_clip(vod.id, "1:00", "1:10");
    let media = app.clip_media(&clip);
    assert!(cut_from(&media, &vod.info.url, 60.0) && near(media.duration, 10.0), "made as {media:?}");
    let sections: Vec<_> = std::fs::read_dir(app.paths().source_footage())
        .unwrap()
        .flatten()
        .filter_map(|e| vodscribe_core::footage::section_of(&e.path()))
        .collect();
    assert!(
        sections.iter().any(|&(v, from, to)| v == vod.id && from <= 60.0 && to >= 70.0),
        "no section of 1:00-1:10 in the footage folder: {sections:?}"
    );
}

/// A second clip of a site's VOD a little longer than the first is cut from
/// the footage already downloaded: the site, refusing now, isn't asked
/// (app `clips.rs`'s module doc: "so a clip made a little longer, or put in
/// an edit, needs no new download").
#[test]
fn a_clip_made_a_little_longer_needs_no_new_download() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "Test video", 600.0);
    app.make_clip(vod.id, "1:00", "1:10");
    app.yt_dlp.refuses(&vod.info.url);
    let longer = app.make_clip(vod.id, "0:55", "1:15");
    let media = app.clip_media(&longer);
    assert!(cut_from(&media, &vod.info.url, 55.0) && near(media.duration, 20.0), "made as {media:?}");
}

/// A clip of a site's VOD whose download the site refuses says why, and has
/// no file (app `clips.rs` `cut`, the error recorded; CHANGELOG.md, 0.7.0
/// Fixes: "Errors show at the window's bottom." — "a clip that couldn't be
/// made said so").
#[test]
fn a_clip_whose_download_is_refused_says_why_and_has_no_file() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "Test video", 600.0);
    app.yt_dlp.refuses(&vod.info.url);
    let clip = app.make_clip(vod.id, "1:00", "1:10");
    assert_eq!(clip.status, ClipStatus::Error);
    let error = clip.error.unwrap_or_default();
    assert!(error.contains("The site refused the download."), "says {error:?}");
    assert_eq!(clip.file_path, None);
    assert!(!app.paths().clip_make(clip.id, 1, "mp4").exists(), "a file left");
}

/// A clip whose cut fails says what ffmpeg said, and has no file (app
/// `clips.rs` `cut`, the error recorded: `set_clip_file(… Error, None,
/// Some(&format!("{e:#}")))`).
#[test]
fn a_clip_whose_cut_fails_says_ffmpeg_s_reason_and_has_no_file() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.ffmpeg.fails_at(0.5, "Error while decoding stream #0:0");
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    assert_eq!(clip.status, ClipStatus::Error);
    let error = clip.error.unwrap_or_default();
    assert!(error.contains("Error while decoding stream #0:0"), "says {error:?}");
    assert_eq!(clip.file_path, None);
    assert!(!app.paths().clip_make(clip.id, 1, "mp4").exists(), "what ffmpeg wrote is left");
}

/// A made clip has a picture of its first frame, for its drag; a clip of
/// sound alone has none (app `clips.rs` `thumb_path`: "Clip `id`'s first
/// frame, shown while it's dragged out").
#[test]
fn a_made_clip_has_a_picture_of_its_first_frame_and_sound_alone_has_none() {
    let app = TestApp::new();
    let video = app.add_video_file("Day 3.mp4", 60.0);
    let sound = app.add_file("Talk.m4a", &Media::sound(60.0));
    let pictured = app.make_clip(video.id, "0:10", "0:20");
    let heard = app.make_clip(sound.id, "0:10", "0:20");
    assert_eq!(heard.status, ClipStatus::Ready);
    assert!(app.paths().clip_thumb(pictured.id).is_file(), "no picture of the video's clip");
    assert!(!app.paths().clip_thumb(heard.id).exists(), "a picture of sound alone");
}

/// A clip of a site's VOD hours long is made, with nothing asked first
/// (CHANGELOG.md, 0.7.1: "Long downloads no longer ask first." — "A clip
/// downloaded from a site is made however long it is.").
#[test]
fn a_long_clip_of_a_site_s_vod_is_made_with_no_question() {
    let app = TestApp::new();
    let vod = app.add_site_vod("twitch", "2901646294", "Test video", 4.0 * 3600.0);
    let clip = app.make_clip(vod.id, "0:00", "3:00:00");
    let media = app.clip_media(&clip);
    assert!(near(media.duration, 3.0 * 3600.0), "lasts {}", media.duration);
}

/// A clip changed in the editor (a second kept, the rest taken out) is made
/// again as its pieces now say (app `clips.rs` `make`: "one changed in
/// the editor … is rendered as an edit is"; CHANGELOG.md, 0.5.0:
/// "Double-click a clip's name … its file is made again to match";
/// tests/e2e/scenarios/snippet-editor.mjs: "cut a second in (B) and the rest
/// taken out (Delete), its file is made again, a second long").
#[test]
fn a_clip_changed_in_the_editor_is_made_as_its_pieces_say() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    app.invoke("remove_stretch", json!({ "id": clip.id, "from": 1.0, "to": 10.0 })).unwrap();
    let lasts = app.clip_media(&app.made(clip.id)).duration;
    assert!(near(lasts, 1.0), "lasts {lasts}");
}

// The commands as the page calls them, times written as a clock shows them.

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.001
}

/// Whether `media` was cut from `source` at `start` seconds.
fn cut_from(media: &Media, source: &str, start: f64) -> bool {
    media.cut_from.as_ref().is_some_and(|(file, from)| file == source && near(*from, start))
}

fn spans(ranges: &[(&str, &str)]) -> Vec<(f64, f64)> {
    ranges.iter().map(|&(from, to)| (at(from), at(to))).collect()
}

/// Waits till each of `clips` is made, so nothing of a test runs on after it.
fn made_all(app: &TestApp, clips: &[Clip]) {
    for clip in clips {
        app.made(clip.id);
    }
}

fn add_clips(app: &TestApp, vod_id: i64, ranges: &[(&str, &str)]) -> CmdResult<Vec<Clip>> {
    app.run(crate::clips::add_clips(app.handle().clone(), vod_id, spans(ranges)))
}

/// A clip of `from`..`to` made only for a drag out of the window, made.
fn drag_clip_of(app: &TestApp, vod_id: i64, from: &str, to: &str) -> Clip {
    let clip = app.run(add_clip(app.handle().clone(), vod_id, at(from), at(to), Some(false))).unwrap();
    app.made(clip.id)
}

fn list_clips(app: &TestApp, vod_id: i64) -> Vec<Clip> {
    app.run(crate::clips::list_clips(app.handle().clone(), vod_id)).unwrap()
}

fn rename(app: &TestApp, id: i64, title: &str) -> CmdResult<Clip> {
    app.run(rename_clip(app.handle().clone(), id, title.into()))
}

fn resize(app: &TestApp, id: i64, from: &str, to: &str) -> CmdResult<Clip> {
    app.run(set_clip_range(app.handle().clone(), id, at(from), at(to)))
}

/// A new edit playing `ranges` of VOD `vod_id`, a piece each, in order.
fn edit_of(app: &TestApp, vod_id: i64, ranges: &[(&str, &str)]) -> i64 {
    let edit = app.invoke("create_edit", json!({})).unwrap()["id"].as_i64().unwrap();
    for (start, end) in spans(ranges) {
        app.invoke("add_range_to_edit", json!({ "editId": edit, "vodId": vod_id, "start": start, "end": end }))
            .unwrap();
    }
    edit
}

fn add_edit_clip_of(app: &TestApp, edit: i64, from: &str, to: &str) -> CmdResult<Clip> {
    app.run(add_edit_clip(app.handle().clone(), edit, at(from), at(to), None))
}

fn add_edit_clips(app: &TestApp, edit: i64, ranges: &[(&str, &str)]) -> CmdResult<Vec<Clip>> {
    app.run(crate::clips::add_edit_clips(app.handle().clone(), edit, spans(ranges)))
}

fn edit_clips_of(app: &TestApp, edit: i64) -> Vec<Clip> {
    app.run(edit_clips(app.handle().clone(), edit)).unwrap()
}

/// Undo in `view`: the label of what it took back, if anything.
fn undo(app: &TestApp, view: Doc) -> Option<String> {
    step(app, "undo", view)
}

fn redo(app: &TestApp, view: Doc) -> Option<String> {
    step(app, "redo", view)
}

fn step(app: &TestApp, command: &str, view: Doc) -> Option<String> {
    let done = app.invoke(command, json!({ "view": view })).unwrap();
    done["label"].as_str().map(str::to_string)
}
