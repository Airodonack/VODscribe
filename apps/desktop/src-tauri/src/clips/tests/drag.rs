use super::*;

use crate::testkit::{LetGo, TestApp};
use serde_json::json;
use vodscribe_core::testkit::{Gate, eventually};

/// On Linux, macOS and Windows alike a drag out of the window begins before
/// the clips' files are made: the page asks the app, and the app says so
/// (0.7.1: "On Linux, clips drag out at once. The drag doesn't wait at the
/// window's edge for the file, which lands where you drop it once it's made",
/// and the Windows and Mac bullets of that release; `drags_before_made`'s doc
/// in app clips.rs: "Linux, macOS and Windows"). The switch that goes back to
/// waiting (`VODSCRIBE_DRAG_WAITS=1`) is the developer's and is tested with
/// the fake drag.
#[test]
fn clips_drag_out_before_their_files_are_made_on_every_system() {
    let app = TestApp::new();
    assert_eq!(app.invoke("drag_before_made", json!({})), Ok(json!(true)));
}

/// F4. Where the system can't wait for a file (`VODSCRIBE_DRAG_WAITS=1`,
/// the developer's fallback), the page waits for the clips at the window's
/// edge, and a drag of one not made is refused, saying why; one made drags
/// out (app clips.rs:1484, `drags_before_made`: "there VODSCRIBE_DRAG_WAITS=1
/// goes back to waiting for the files at the window's edge before the drag
/// begins"; app clips.rs:1613, `drag_clip`: "That clip's file isn't made
/// yet.", "Couldn't make the clip: {why}").
#[test]
fn where_the_system_cannot_wait_a_clip_not_made_is_refused_saying_why() {
    let app = TestApp::new();
    app.started_with("VODSCRIBE_DRAG_WAITS", "1");
    assert_eq!(app.invoke("drag_before_made", json!({})), Ok(json!(false)));
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let making = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    halfway.reached();
    assert_eq!(app.drag_out(&[making.id]), Err(json!("That clip's file isn't made yet.")));
    // The next file it makes fails; this one goes on.
    app.ffmpeg.fails_at(0.5, "Invalid data found when processing input");
    halfway.open();
    app.made(making.id);
    let failed = app.make_clip(vod.id, "0:30", "0:40");
    assert_eq!(failed.status, ClipStatus::Error);
    let refused = app.drag_out(&[failed.id]).unwrap_err();
    let refused = refused.as_str().unwrap();
    assert!(refused.starts_with("Couldn't make the clip: ") && refused.contains("Invalid data"), "{refused}");
    assert_eq!(app.drags().begun(), 0, "a drag begun");
    assert_eq!(app.drag_out(&[making.id]), Ok(json!(true)), "the one made");
}

/// What a drag out of the window comes to, by what was seen of it (app
/// clips.rs `drag_outcome`; 0.7.1: "Some apps take finished files only. If one
/// takes nothing or refuses the drop before the clip is made, VODscribe says
/// which app it was", and "A drag called off makes nothing. Escape, or a drop
/// on nothing, stops the clip's making ... If the clip can't be made,
/// VODscribe says why only when you let it go on another app."): let go
/// on an app that took nothing while the files were still being made, it
/// takes finished files only (named, where the system says); one that
/// failed is said only once it's let go on another app; called off or
/// dropped on the app's own window, nothing.
#[test]
fn a_drag_ends_as_its_receiver_took_it() {
    use Ending::*;
    let named = || Some("Kdenlive".to_string());
    let blocked = || Some("blocked".to_string());
    let ended = |delivered, failed: Option<&str>, finished_only, receiver: Option<&str>| DragEnded {
        drag: None,
        delivered,
        failed: failed.map(String::from),
        finished_only,
        receiver: receiver.map(String::from),
        resolve: None,
    };
    assert_eq!(drag_outcome(Elsewhere, true, None, true, named()), ended(true, None, false, None), "given");
    assert_eq!(
        drag_outcome(Elsewhere, false, None, false, named()),
        ended(false, None, true, Some("Kdenlive")),
        "let go before it was made, nothing taken"
    );
    assert_eq!(drag_outcome(Elsewhere, false, None, false, None), ended(false, None, true, None), "name unknown");
    assert_eq!(drag_outcome(Elsewhere, false, None, true, named()), DragEnded::default(), "made, not taken");
    assert_eq!(drag_outcome(Elsewhere, false, blocked(), false, named()), ended(false, Some("blocked"), false, None));
    assert_eq!(drag_outcome(Nowhere, false, blocked(), false, None), ended(false, Some("blocked"), false, None));
    assert_eq!(drag_outcome(Nowhere, false, None, false, None), DragEnded::default(), "dropped on nothing");
    assert_eq!(drag_outcome(Elsewhere, false, Some(CANCELED.into()), false, named()), DragEnded::default());
    for ending in [CalledOff, Back] {
        for (given, failure) in [(true, None), (false, None), (false, blocked())] {
            assert_eq!(drag_outcome(ending, given, failure, false, named()), DragEnded::default(), "{ending:?}");
        }
    }
}

/// The page told an early release from a refused drag by the word
/// "released" in its error: a reworded sentence would have shown an
/// ordinary release, too soon, as an error (app clips.rs `began`: "refused
/// because the button came up first, it didn't, which is no error to show;
/// refused otherwise, why").
#[test]
fn a_drag_refused_for_an_early_release_is_none_begun_not_an_error() {
    assert_eq!(began(Ok(())), Ok(true));
    assert_eq!(began(Err(ReleasedEarly.into())), Ok(false));
    assert_eq!(began(Err(anyhow::Error::new(ReleasedEarly).context("Not now."))), Ok(false), "said otherwise");
    assert_eq!(began(Err(anyhow::anyhow!("no view"))), Err("no view".into()));
    assert_eq!(
        began(Err(anyhow::anyhow!("The button was released before the drag could begin."))),
        Err("The button was released before the drag could begin.".into()),
        "only the type tells",
    );
}

/// The picture clip `id`'s drag out of the window shows, the page sending
/// none of its own; the drag then called off.
fn dragged_with(app: &TestApp, id: i64) -> Vec<u8> {
    app.drag_out(&[id]).unwrap();
    let drag = app.drags().last();
    let image = drag.image().to_vec();
    drag.let_go(LetGo::CalledOff);
    app.drag_ended();
    image
}

/// A clip whose picture couldn't be made drags with the app's icon
/// (plans/clips-pilot.md, Decided 9: "A clip whose picture couldn't be made
/// drags with the app's icon (a spec)"; app clips.rs `DRAG_ICON`).
#[test]
fn a_clip_whose_picture_could_not_be_made_drags_with_the_app_s_icon() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.ffmpeg.lacks("png");
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    assert_eq!(clip.status, ClipStatus::Ready);
    assert_eq!(dragged_with(&app, clip.id), DRAG_ICON);
}

/// A clip made again whose new picture couldn't be made drags with the app's
/// icon, not the picture of what it was before (Decided 9, as above).
#[test]
fn a_clip_made_again_whose_picture_could_not_be_made_drags_with_the_app_s_icon() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    assert_ne!(dragged_with(&app, clip.id), DRAG_ICON, "made with its picture");
    app.ffmpeg.lacks("png");
    app.invoke("set_clip_range", json!({ "id": clip.id, "start": 10.0, "end": 15.0 })).unwrap();
    assert_eq!(app.made(clip.id).status, ClipStatus::Ready);
    assert_eq!(dragged_with(&app, clip.id), DRAG_ICON);
}

/// A clip whose picture couldn't be made has it tried again the next time
/// the clip is shown, on its clip bar, and drags with it once it's made
/// (Decided 9: "its picture is tried again when the clip is next shown").
#[test]
fn a_clip_s_picture_that_could_not_be_made_is_tried_again_when_the_clip_is_next_shown() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.ffmpeg.lacks("png");
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    app.ffmpeg.works();
    let thumb = app.paths().clip_thumb(clip.id);
    assert!(!thumb.exists(), "a picture made");
    app.invoke("list_clips", json!({ "vodId": vod.id })).unwrap();
    eventually("the clip's picture made", || thumb.is_file());
    assert_eq!(dragged_with(&app, clip.id), std::fs::read(&thumb).unwrap());
}

/// Clips of VOD `vod_id` from 0:00 on, `n` of them half a second long each,
/// made.
fn many_made(app: &TestApp, vod_id: i64, n: usize) -> Vec<Clip> {
    let ranges: Vec<(f64, f64)> = (0..n).map(|i| (i as f64, i as f64 + 0.5)).collect();
    let made = app.run(crate::clips::add_clips(app.handle().clone(), vod_id, ranges)).unwrap();
    made.iter().map(|c| app.made(c.id)).collect()
}

/// A picture that couldn't be made, tried again as its clip is shown and
/// failing again, isn't tried at each listing after (the page asks for a
/// VOD's clips every tenth of a second while a drag waits on one: clips.svelte.ts
/// `ready`); it's tried again once the app starts again (Decided 9, as
/// above; the clips-fixes review, L2).
#[test]
fn a_picture_that_could_not_be_made_again_is_tried_once_a_run() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    app.ffmpeg.lacks("png");
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let thumb = app.paths().clip_thumb(clip.id);
    app.invoke("list_clips", json!({ "vodId": vod.id })).unwrap();
    eventually("its picture tried again", || app.state().clips.pictures_idle());
    app.ffmpeg.works();
    app.invoke("list_clips", json!({ "vodId": vod.id })).unwrap();
    eventually("the listing's pictures", || app.state().clips.pictures_idle());
    assert!(!thumb.exists(), "tried again at the next listing");
    let app = TestApp::on(app.quit());
    app.invoke("list_clips", json!({ "vodId": vod.id })).unwrap();
    eventually("the clip's picture made after a start", || thumb.is_file());
}

/// How many panics said the app's state was gone: a thread of the app's that
/// outlived it (`TestApp`'s drop lets go of the state once every queue is
/// joined).
static STATE_GONE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A quit while pictures are being made again waits for them: nothing of
/// that work runs on, or writes, once the app has ended (as every queue of
/// the app's ends with it: testkit.rs, `TestApp`'s drop; the clips-fixes
/// review, M2).
#[test]
fn a_quit_while_pictures_are_made_again_leaves_nothing_running() {
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.to_string().contains("state() called before manage()") {
            STATE_GONE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        before(info);
    }));
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 600.0);
    app.ffmpeg.lacks("png");
    many_made(&app, vod.id, 300);
    app.ffmpeg.works();
    app.invoke("list_clips", json!({ "vodId": vod.id })).unwrap();
    assert!(!app.state().clips.pictures_idle(), "no pictures being made at the quit");
    let folder = app.quit();
    let thumbs = |dir: &Path| -> Vec<_> {
        let mut names: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name()).collect();
        names.sort();
        names
    };
    let dir = folder.paths().clip_thumbs();
    let at_quit = thumbs(&dir);
    // Nothing to wait for: what's looked for is that nothing happens.
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(thumbs(&dir), at_quit, "pictures written after the quit");
    assert_eq!(STATE_GONE.load(std::sync::atomic::Ordering::SeqCst), 0, "a thread ran on after the app");
}

/// A stopped batch of pictures made again makes none past the stop: the
/// app's quit (`Cutter::close`, as every queue's) doesn't wait out a whole
/// listing's (the clips-fixes re-review, N2).
#[test]
fn a_batch_of_pictures_stopped_makes_no_more() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 600.0);
    app.ffmpeg.lacks("png");
    let made = many_made(&app, vod.id, 20);
    app.ffmpeg.works();
    let batch: Vec<_> = made.iter().filter_map(made_file).collect();
    make_pictures(app.handle(), batch, &std::sync::atomic::AtomicBool::new(true));
    let pictured = made.iter().filter(|c| app.paths().clip_thumb(c.id).exists()).count();
    assert_eq!(pictured, 0, "pictures made after the stop");
}

/// A clip made only for a drag, let go on an app that takes copies
/// (Finder, Explorer), is forgotten once its copy is written (dragout.ts
/// `forgetHidden`), and its picture goes with it (`after_delete`: "its files
/// and its thumbnail deleted"), even when the forget comes while the picture
/// is being made, ffmpeg having the file open already (the 0.7.4 quality
/// pass, item 5: macOS logged the picture job running after the file was
/// gone).
#[test]
fn a_drag_only_clip_forgotten_while_its_picture_is_made_leaves_no_picture() {
    use crate::testkit::Receiver;
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    // The cut held at its end, so the drag begins while it's being made.
    let cut_end = Gate::new();
    app.ffmpeg.holds_at(1.0, &cut_end);
    let clip = app.run(crate::clips::add_clip(app.handle().clone(), vod.id, 10.0, 20.0, Some(false))).unwrap();
    cut_end.reached();
    let picture = Gate::new();
    app.ffmpeg.holds_stills(&picture);
    app.drag_out(&[clip.id]).unwrap();
    app.drags().last().let_go(LetGo::On(Receiver::taking_copies("Finder")));
    cut_end.open();
    picture.reached();
    assert_eq!(app.drag_ended()["delivered"], json!(true));
    assert!(!app.clip(clip.id).unwrap().shown, "on its clip bar");
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    picture.open();
    eventually("the clips queue idle", || app.state().clips.listed().0.is_none());
    let thumb = app.paths().clip_thumb(clip.id);
    assert!(!thumb.exists(), "a picture left for the deleted clip: {}", thumb.display());
}
