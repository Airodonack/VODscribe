//! Rendering an edit: `render_edit_now` and `stop_render`, the render
//! itself (`render_edit`, a job of the clips' queue), and the rendered video
//! shown in its folder (`show_edit`), dragged out (`drag_edit`) and pictured
//! (`edit_thumb`). Each test is named for what it says the app does and
//! cites where that's said.
//!
//! The fake ffmpeg doesn't read a render's filter graph or its chapters, so
//! what plays in the video, how long it is, a hole's black and silence and
//! its chapters are checked against the real ffmpeg in core `render.rs`'s
//! tests; here, what the app makes of a render: its status, its files, what
//! it's made as and when it's made again.

use super::*;

use serde_json::{Value, json};
use vodscribe_core::settings::{ClipCodec, ClipQuality};
use vodscribe_core::testkit::{Gate, Media, eventually};

use crate::testkit::{LetGo, Receiver, TestApp};

/// Edit `id` as the library's list has it (`list_edits`).
fn edit(app: &TestApp, id: i64) -> Value {
    let edits = app.invoke("list_edits", json!({})).unwrap();
    edits.as_array().unwrap().iter().find(|e| e["id"] == id).cloned().unwrap_or_else(|| panic!("no edit {id}"))
}

/// Render (E) pressed on edit `id`, as the page asks (api.ts `renderEdit`).
fn render(app: &TestApp, id: i64) {
    app.invoke("render_edit_now", json!({ "id": id })).unwrap();
}

/// Stop pressed on edit `id`'s render (api.ts `stopRender`).
fn stop(app: &TestApp, id: i64) {
    app.invoke("stop_render", json!({ "id": id })).unwrap();
}

/// Edit `id` once the clips' queue is done with it, neither rendering nor
/// waiting to: rendered, failed or stopped.
fn rendered(app: &TestApp, id: i64) -> Value {
    eventually(&format!("edit {id} rendered"), || {
        let (running, waiting) = app.state().clips.listed();
        running != Some(id) && !waiting.contains(&id) && edit(app, id)["status"] != "cutting"
    });
    edit(app, id)
}

/// Edit `id`'s video, rendered (`rendered`).
fn video_of(app: &TestApp, id: i64) -> PathBuf {
    let done = rendered(app, id);
    assert_eq!(done["status"], "ready", "edit {id} isn't rendered: {}", done["error"]);
    PathBuf::from(done["file_path"].as_str().expect("a rendered edit's video"))
}

/// Edit `id` rendered as Render (E) asks: its video, once made.
fn rendered_now(app: &TestApp, id: i64) -> PathBuf {
    render(app, id);
    video_of(app, id)
}

/// The jobs the list at the library's bottom shows (`list_jobs`): each
/// one's id and whether it's the one being made.
fn jobs(app: &TestApp) -> Vec<(i64, bool)> {
    let jobs = app.invoke("list_jobs", json!({})).unwrap();
    jobs.as_array().unwrap().iter().map(|j| (j["id"].as_i64().unwrap(), j["running"] == true)).collect()
}

/// The files edit `id` made in the clips' folder (its renders, finished or
/// not), by name.
fn renders_of(app: &TestApp, id: i64) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(app.paths().clips())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&format!("{id}-")) || n.starts_with(&format!(".{id}-")))
        .collect();
    names.sort();
    names
}

/// Clips and edits are made as `codec` at `quality` from now, as Settings ›
/// Encoding saves it (`set_settings`).
fn made_as(app: &TestApp, codec: ClipCodec, quality: ClipQuality) {
    let settings = json!({ "clip_codec": codec, "clip_quality": quality });
    app.invoke("set_settings", json!({ "settings": settings })).unwrap();
}

/// 0:50–0:55 of VOD `vod` added to edit `edit` as a piece, as a drop of a
/// stretch on its timeline does (`add_range_to_edit`).
fn add_piece(app: &TestApp, edit: i64, vod: i64) {
    let range = json!({ "editId": edit, "vodId": vod, "start": 50.0, "end": 55.0, "title": "More", "index": null });
    app.invoke("add_range_to_edit", range).unwrap();
}

/// A clip of VOD `vod` made, so the clips' queue is past what was asked
/// before it (one encode at a time, in order).
fn queue_passed(app: &TestApp, vod: i64) {
    app.make_clip(vod, "0:55", "0:58");
}

/// Edit `edit`'s render held halfway (ffmpeg at the gate given back),
/// ffmpeg working as before for what comes after.
fn held_halfway(app: &TestApp, edit: i64) -> Gate {
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    render(app, edit);
    halfway.reached();
    app.ffmpeg.works();
    halfway
}

/// Pressed, an edit is rendered to one video, in the clips' folder, of what
/// its pieces play (CHANGELOG.md, 0.4.0: "Edits: put clips and stretches of
/// your VODs together into one video …, and render it to one video (E)";
/// edits.rs `render_edit`: "Renders edit `id` into its file").
#[test]
fn an_edit_renders_to_one_video_of_its_pieces() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0), (vod.id, 30.0, 35.0)]);
    let video = rendered_now(&app, edit_id);
    assert!(video.starts_with(app.paths().clips()), "rendered to {video:?}");
    assert_eq!(edit(&app, edit_id)["current"], true, "just rendered, it says it's changed since");
    assert_eq!(video.extension().and_then(|e| e.to_str()), Some("mp4"));
    let media = Media::read(&video).expect("a video");
    let source = vod.source_path.unwrap();
    assert_eq!(media.cut_from.map(|(from, _)| from), Some(source), "not of its pieces' VOD");
}

/// An edit asked to render while another renders waits its turn, and shows
/// as rendering from the moment it's asked (edits.rs `render_edit_now`:
/// "Renders edit `id` (after any being rendered); shown as rendering from
/// now, so it isn't asked for twice").
#[test]
fn an_edit_asked_to_render_while_another_renders_waits_its_turn_shown_as_rendering() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let first = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let second = app.edit_of(&[(vod.id, 30.0, 40.0)]);
    let halfway = held_halfway(&app, first);
    render(&app, second);
    assert_eq!(edit(&app, second)["status"], "cutting", "not shown as rendering");
    eventually("the page told it's rendering", || {
        app.heard("edit").iter().any(|e| e["id"] == second && e["status"] == "cutting")
    });
    assert_eq!(jobs(&app), [(first, true), (second, false)]);
    halfway.open();
    video_of(&app, first);
    video_of(&app, second);
}

/// Asked again while it waits, an edit is rendered once: one job in the list,
/// one video made (edits.rs `render_edit_now`: "Asked again while it waits,
/// once is enough"; core `Paths::clip_make`: "every make writes a file of its
/// own").
#[test]
fn an_edit_asked_again_while_it_waits_is_rendered_once() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let first = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let second = app.edit_of(&[(vod.id, 30.0, 40.0)]);
    let halfway = held_halfway(&app, first);
    render(&app, second);
    render(&app, second);
    assert_eq!(jobs(&app), [(first, true), (second, false)], "asked for twice");
    halfway.open();
    assert_eq!(video_of(&app, second), app.paths().clip_make(second, 1, "mp4"), "rendered more than once");
}

/// An edit changed while it renders and asked again is rendered again once
/// that render is over, as it is now (edits.rs `render_edit_now`: "One
/// running renders the edit as it was, so it's queued again").
#[test]
fn an_edit_changed_while_it_renders_and_asked_again_is_rendered_again_as_changed() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let halfway = held_halfway(&app, edit_id);
    add_piece(&app, edit_id, vod.id);
    render(&app, edit_id);
    halfway.open();
    let video = video_of(&app, edit_id);
    assert_eq!(video, app.paths().clip_make(edit_id, 2, "mp4"), "not rendered again");
    let done = edit(&app, edit_id);
    assert_eq!((done["pieces"].as_u64(), done["current"].clone()), (Some(2), json!(true)));
}

/// A change made while an edit renders leaves it changed since it was
/// rendered: the video is of the edit as it was when its render began (core
/// edits.rs `made_of_now`: "taken as its make begins, and recorded with the
/// file, so a change made meanwhile leaves it not current, to be made
/// again").
#[test]
fn an_edit_changed_while_it_renders_shows_as_changed_since_once_rendered() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let halfway = held_halfway(&app, edit_id);
    add_piece(&app, edit_id, vod.id);
    halfway.open();
    video_of(&app, edit_id);
    assert_eq!(edit(&app, edit_id)["current"], false, "the change made meanwhile is taken as in the video");
}

/// A render that fails says why, and the video rendered before it stays as
/// it was, the edit's still (edits.rs `render_edit`, its error recorded;
/// core render.rs `render`: "`out` is replaced only once the new one is
/// whole").
#[test]
fn a_render_that_fails_says_why_and_leaves_the_video_before_it() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let before = rendered_now(&app, edit_id);
    let was = std::fs::read(&before).unwrap();
    add_piece(&app, edit_id, vod.id);
    app.ffmpeg.fails_at(0.5, "the source broke off");
    render(&app, edit_id);
    let failed = rendered(&app, edit_id);
    assert_eq!(failed["status"], "error");
    let says = failed["error"].as_str().unwrap_or_default();
    assert!(says.contains("the source broke off"), "it says {says:?}");
    assert_eq!(std::fs::read(&before).ok(), Some(was), "the video before was replaced");
    assert_eq!(failed["file_path"].as_str(), before.to_str(), "the edit's video isn't the one before");
    let name = before.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(renders_of(&app, edit_id), [name], "a part left");
}

/// Stop on an edit still waiting its turn leaves it as it was before it was
/// asked for: not rendered, and not rendered later either (edits.rs
/// `stop_render`: "Not begun: as it was before it was asked for"; CHANGELOG.md,
/// 0.7.1: "A canceled clip or edit says so.": "a canceled edit is left not
/// rendered").
#[test]
fn stop_on_an_edit_waiting_its_turn_leaves_it_not_rendered() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let first = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let second = app.edit_of(&[(vod.id, 30.0, 40.0)]);
    let halfway = held_halfway(&app, first);
    render(&app, second);
    stop(&app, second);
    let stopped = edit(&app, second);
    assert_eq!((stopped["status"].clone(), stopped["file_path"].clone()), (json!("waiting"), Value::Null));
    assert_eq!(jobs(&app), [(first, true)], "still in the list of jobs");
    halfway.open();
    queue_passed(&app, vod.id);
    assert_eq!(edit(&app, second)["status"], "waiting", "rendered all the same");
    assert!(renders_of(&app, second).is_empty(), "{:?}", renders_of(&app, second));
}

/// Stop on the edit being rendered stops it, and leaves no video of it,
/// not even a part (edits.rs `render_edit`: "Stopped, or deleted: as it was
/// before, if it's still there"; app clips.rs `Cutter::cancel_job`: "its
/// ffmpeg ended and its part file deleted").
#[test]
fn stop_on_the_edit_being_rendered_stops_it_leaving_no_video() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let halfway = held_halfway(&app, edit_id);
    stop(&app, edit_id);
    halfway.open();
    let stopped = rendered(&app, edit_id);
    assert_eq!((stopped["status"].clone(), stopped["file_path"].clone()), (json!("waiting"), Value::Null));
    assert!(renders_of(&app, edit_id).is_empty(), "{:?}", renders_of(&app, edit_id));
}

/// Stop on an edit rendered before leaves it as it was: its video the one
/// rendered before, changed since; whether its new render had begun or was
/// waiting (edits.rs `stop_render`: "Not begun: as it was before it was
/// asked for"; `render_edit`: "Stopped, or deleted: as it was before, if it's
/// still there").
#[test]
#[ignore = "disagreement: Stop leaves an edit rendered before as not rendered (waiting), not as it was"]
fn stop_on_an_edit_rendered_before_leaves_its_video_as_it_was() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let other = app.edit_of(&[(vod.id, 30.0, 40.0)]);
    let before = rendered_now(&app, edit_id);
    add_piece(&app, edit_id, vod.id);
    let as_it_was = |why: &str| {
        let now = edit(&app, edit_id);
        assert_eq!(now["status"], "ready", "{why}: it says it isn't rendered");
        assert_eq!(now["file_path"].as_str(), before.to_str(), "{why}: not the video before");
        assert_eq!(now["current"], false, "{why}");
        assert!(before.is_file(), "{why}: the video before is gone");
    };
    // Its new render begun, then stopped.
    let halfway = held_halfway(&app, edit_id);
    stop(&app, edit_id);
    halfway.open();
    rendered(&app, edit_id);
    as_it_was("stopped as it rendered");
    // Waiting its turn behind another, then stopped.
    let halfway = held_halfway(&app, other);
    render(&app, edit_id);
    stop(&app, edit_id);
    as_it_was("stopped as it waited");
    halfway.open();
}

/// An edit whose render is over as Stop, or Cancel in the list of jobs, is
/// pressed, before the library is told, stays rendered: nothing was left to
/// stop (app clips.rs `settle_canceled`: "What a clip or edit canceled from
/// the list of jobs shows, unless it was made meanwhile"; edits.rs
/// `render_edit`, which takes only a render stopped as "Stopped").
#[test]
fn an_edit_rendered_just_as_its_render_is_stopped_stays_rendered() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    for how in ["stop", "cancel"] {
        let ended = Gate::new();
        app.holds_cut_ends(&ended);
        render(&app, edit_id);
        ended.reached();
        match how {
            "stop" => stop(&app, edit_id),
            _ => app.invoke("cancel_job", json!({ "kind": "edit", "id": edit_id })).map(drop).unwrap(),
        }
        ended.open();
        let video = video_of(&app, edit_id);
        assert!(video.is_file(), "{how}: {video:?}");
        assert_eq!(edit(&app, edit_id)["current"], true, "{how}");
        add_piece(&app, edit_id, vod.id);
    }
}

/// An edit deleted as its render ends leaves no video: the render finds it
/// gone and deletes what it made (edits.rs `delete_edit`: "Its render stops
/// (one that ends now finds it gone, and deletes its file)").
#[test]
fn an_edit_deleted_as_its_render_ends_leaves_no_video() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let ended = Gate::new();
    app.holds_cut_ends(&ended);
    render(&app, edit_id);
    ended.reached();
    app.invoke("delete_edit", json!({ "id": edit_id })).unwrap();
    ended.open();
    eventually("the render over", || app.state().clips.listed().0.is_none());
    assert!(renders_of(&app, edit_id).is_empty(), "{:?}", renders_of(&app, edit_id));
}

/// Renamed and rendered again, an edit's video is a new file, and the one
/// before goes (CHANGELOG.md, 0.4.0: "Files you made and no longer use don't
/// pile up in Videos/VODscribe: … an edit's when it's renamed and rendered
/// again"; tests/e2e/scenarios/edits.mjs: "Renamed and rendered again: a new
/// file, the one before gone").
#[test]
fn an_edit_renamed_and_rendered_again_is_a_new_video_and_the_one_before_goes() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let before = rendered_now(&app, edit_id);
    app.invoke("rename_edit", json!({ "id": edit_id, "title": "The landing" })).unwrap();
    let again = rendered_now(&app, edit_id);
    assert_ne!(again, before, "rendered over the one before");
    assert!(again.is_file() && !before.exists(), "the one before stayed");
}

/// … unless it was dragged into another app, which keeps it: the file it
/// was given stays as it was, through the render and the edit's delete
/// (CHANGELOG.md, 0.4.0, the same bullet: "unless you dragged that file into
/// another app").
#[test]
fn an_edit_renamed_and_rendered_again_keeps_the_video_dragged_into_another_app() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    rendered_now(&app, edit_id);
    let given = dragged_onto(&app, edit_id, Receiver::taking_files("Kdenlive"));
    let [given] = &given[..] else { panic!("given {given:?}") };
    let was = std::fs::read(given).unwrap();
    app.invoke("rename_edit", json!({ "id": edit_id, "title": "The landing" })).unwrap();
    rendered_now(&app, edit_id);
    assert_eq!(std::fs::read(given).ok(), Some(was.clone()), "the file another app took went with the render");
    app.invoke("delete_edit", json!({ "id": edit_id })).unwrap();
    assert_eq!(std::fs::read(given).ok(), Some(was), "the file another app took went with its edit");
}

/// A rendered edit shows as changed since it was rendered once its video
/// would be different: renamed, or a chapter added, renamed or moved; and as
/// rendered again once put back as it was; what plays changed by as little as
/// a millisecond counts (CHANGELOG.md, 0.5.0: "A rendered
/// edit has its chapters …. It shows as changed since it was rendered only
/// when its video would be different"; core edits.rs `made_of`: "its name,
/// what plays (each VOD's stretches, in order, to the microsecond) and its
/// chapters"). A highlight or a cut: core edits.rs
/// `a_rendered_file_is_out_of_date_only_when_what_it_holds_changes`.
#[test]
fn a_rendered_edit_shows_as_changed_since_once_renamed_or_its_chapters_change() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    rendered_now(&app, edit_id);
    let current = || edit(&app, edit_id)["current"] == true;
    let renamed = |title: &str| app.invoke("rename_edit", json!({ "id": edit_id, "title": title })).unwrap();
    renamed("The landing");
    assert!(!current(), "renamed");
    renamed("Best of");
    assert!(current(), "its name back as it was");
    let mark = json!({ "editId": edit_id, "time": 2.0, "kind": "chapter", "text": "Start" });
    let chapter = app.invoke("add_edit_mark", mark).unwrap()["id"].as_i64().unwrap();
    assert!(!current(), "a chapter added");
    rendered_now(&app, edit_id);
    assert!(current());
    app.invoke("set_mark_text", json!({ "id": chapter, "text": "Landing" })).unwrap();
    assert!(!current(), "a chapter renamed");
    app.invoke("set_mark_text", json!({ "id": chapter, "text": "Start" })).unwrap();
    assert!(current(), "its title back as it was");
    app.invoke("move_mark", json!({ "id": chapter, "time": 4.0 })).unwrap();
    assert!(!current(), "a chapter moved");
    app.invoke("move_mark", json!({ "id": chapter, "time": 2.0 })).unwrap();
    assert!(current(), "the chapter back where it was");
    let piece = app.db().piece_rows(edit_id).unwrap()[0].id;
    app.invoke("trim_piece", json!({ "id": piece, "start": 10.0, "end": 19.999 })).unwrap();
    assert!(!current(), "a millisecond less of it plays");
}

/// An edit is rendered as Settings › Encoding says: in the format of the
/// codec chosen, recorded as made with that codec and quality (CHANGELOG.md,
/// 0.7.2: "Clips come in more formats. Settings › Encoding adds H.265 and
/// ProRes beside H.264 and DNxHR, and a Proxy, Standard or High quality, for
/// clips and edits alike"; core render.rs: "one file encoded as clips are
/// (`ClipEncoding`: MP4, or MOV for editors)").
#[test]
fn an_edit_is_rendered_as_settings_encoding_says() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    for (codec, quality, ext) in [
        (ClipCodec::H264, ClipQuality::Proxy, "mp4"),
        (ClipCodec::H265, ClipQuality::High, "mp4"),
        (ClipCodec::Dnxhr, ClipQuality::Standard, "mov"),
        (ClipCodec::Prores, ClipQuality::High, "mov"),
    ] {
        made_as(&app, codec, quality);
        let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
        let video = rendered_now(&app, edit_id);
        assert_eq!(video.extension().and_then(|e| e.to_str()), Some(ext), "as {codec:?}");
        let recorded: ClipEncoding = serde_json::from_value(edit(&app, edit_id)["encoding"].clone()).unwrap();
        assert_eq!(recorded, ClipEncoding { codec, quality });
    }
    // Rendered as H.264, then clips made otherwise: changed since.
    made_as(&app, ClipCodec::H264, ClipQuality::Standard);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    rendered_now(&app, edit_id);
    made_as(&app, ClipCodec::H265, ClipQuality::Standard);
    assert_eq!(edit(&app, edit_id)["current"], false, "rendered otherwise than clips are made now");
}

/// An edit of sound alone is rendered as sound alone: an M4A, or a WAV when
/// clips are made as MOV (core render.rs: "An edit with no pictures at all
/// makes sound alone (an M4A, or a WAV)"; tests/e2e/scenarios/edits.mjs,
/// its render of an edit of a sound file checked to be an .m4a).
#[test]
fn an_edit_of_sound_alone_renders_as_an_m4a_or_a_wav_by_the_format() {
    let app = TestApp::new();
    let talk = app.add_file("Talk.m4a", &Media::sound(60.0));
    for (codec, ext) in [(ClipCodec::H264, "m4a"), (ClipCodec::Dnxhr, "wav")] {
        made_as(&app, codec, ClipQuality::Standard);
        let edit_id = app.edit_of(&[(talk.id, 10.0, 20.0)]);
        let video = rendered_now(&app, edit_id);
        assert_eq!(video.extension().and_then(|e| e.to_str()), Some(ext), "as {codec:?}: {video:?}");
    }
}

/// A piece whose video file was deleted outside the app is a hole, and its
/// edit still renders (CHANGELOG.md, 0.5.0: "a piece whose video file you
/// deleted outside VODscribe shows striped in the timeline and plays and
/// renders as black and silence, rather than stopping the edit from
/// rendering"; tests/e2e/scenarios/holes.mjs: "it still renders"). Its black
/// and silence: core render.rs `the_graph_trims_fits_and_joins_each_piece`
/// and `renders_pieces_of_files_into_one`.
#[test]
fn an_edit_with_a_piece_whose_file_was_deleted_still_renders() {
    let app = TestApp::new();
    let kept = app.add_video_file("Day 3.mp4", 60.0);
    let gone = app.add_video_file("Day 4.mp4", 60.0);
    let edit_id = app.edit_of(&[(kept.id, 10.0, 20.0), (gone.id, 5.0, 8.0)]);
    std::fs::remove_file(gone.source_path.unwrap()).unwrap();
    let view = app.invoke("get_edit", json!({ "id": edit_id })).unwrap();
    let holes: Vec<bool> = view["pieces"].as_array().unwrap().iter().map(|p| p["hole"] == true).collect();
    assert_eq!(holes, [false, true]);
    assert!(rendered_now(&app, edit_id).is_file());
}

/// An edit of a site's VOD fetches the footage its pieces need, then renders
/// from it (edits.rs `render_edit`: "fetching any footage it needs first").
#[test]
fn an_edit_of_a_site_s_vod_fetches_the_footage_it_needs_then_renders() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "Test video", 600.0);
    let edit_id = app.edit_of(&[(vod.id, 60.0, 90.0)]);
    let video = rendered_now(&app, edit_id);
    let footage = std::fs::read_dir(app.paths().source_footage()).map(|d| d.flatten().count()).unwrap_or(0);
    assert!(footage > 0, "no footage fetched");
    let from = Media::read(&video).unwrap().cut_from.map(|(from, _)| from);
    assert_eq!(from.as_deref(), Some(vod.info.url.as_str()), "not of its VOD");
}

/// A piece of a VOD whose footage is gone for good still renders from what's
/// kept of it here, not as a hole (edits.rs `pieces`: "each a hole if its
/// footage is gone for good: its VOD is gone …, and what's kept here doesn't
/// hold all of the piece (what it holds still plays and renders)").
#[test]
fn a_piece_of_a_vod_gone_for_good_renders_from_what_is_kept_here() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "Test video", 600.0);
    let edit_id = app.edit_of(&[(vod.id, 60.0, 90.0)]);
    rendered_now(&app, edit_id);
    app.db().set_gone(vod.id).unwrap();
    let view = app.invoke("get_edit", json!({ "id": edit_id })).unwrap();
    assert_eq!(view["pieces"][0]["hole"], false, "a hole, its footage here");
    let video = rendered_now(&app, edit_id);
    let from = Media::read(&video).unwrap().cut_from.map(|(from, _)| from);
    assert_eq!(from.as_deref(), Some(vod.info.url.as_str()), "not rendered from what's kept");
}

/// Show in folder shows a rendered edit's video in the file manager under
/// its title (edits.rs `show_edit`: "Shows edit `id`'s video in the system
/// file manager, under its title").
#[test]
fn show_in_folder_shows_a_rendered_edit_s_video_under_its_title() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0), (vod.id, 30.0, 35.0)]);
    let video = rendered_now(&app, edit_id);
    app.invoke("show_edit", json!({ "id": edit_id })).unwrap();
    let shown = app.desktop().shown().pop().expect("shown in the file manager");
    assert_eq!(shown.file_name().and_then(|n| n.to_str()), Some("Best of.mp4"));
    assert_eq!(std::fs::read(&shown).ok(), std::fs::read(&video).ok(), "not its video");
}

/// Show in folder on an edit not rendered, or whose video was deleted outside
/// the app, says it isn't rendered (edits.rs `handed_out`: "This edit isn't
/// rendered yet."; tests/e2e/scenarios/store-errors.mjs: "An edit rendered
/// and its file then deleted outside the app: its row's Show in folder …
/// says it isn't rendered").
#[test]
fn show_in_folder_on_an_edit_not_rendered_says_so() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let show = || app.invoke("show_edit", json!({ "id": edit_id }));
    assert_eq!(show(), Err(json!("This edit isn't rendered yet.")), "never rendered");
    std::fs::remove_file(rendered_now(&app, edit_id)).unwrap();
    assert_eq!(show(), Err(json!("This edit isn't rendered yet.")), "its video deleted");
    assert!(app.desktop().shown().is_empty());
}

/// A video shown in its folder is the user's to take: it stays there when
/// the edit is deleted (edits.rs `show_edit`: "from there it's the user's to
/// take").
#[test]
fn an_edit_s_video_shown_in_its_folder_stays_there_when_the_edit_is_deleted() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    rendered_now(&app, edit_id);
    app.invoke("show_edit", json!({ "id": edit_id })).unwrap();
    let shown = app.desktop().shown().pop().unwrap();
    let was = std::fs::read(&shown).unwrap();
    app.invoke("delete_edit", json!({ "id": edit_id })).unwrap();
    assert_eq!(std::fs::read(&shown).ok(), Some(was), "the video shown went with its edit");
}

/// Edit `id`'s video dragged out of the window and let go on `on`, as the
/// page drags it (api.ts `dragEdit`), the drag waited out: the files that
/// app was given.
fn dragged_onto(app: &TestApp, id: i64, on: Receiver) -> Vec<PathBuf> {
    assert_eq!(app.invoke("drag_edit", json!({ "id": id, "drag": 1 })), Ok(json!(true)), "the drag didn't begin");
    let drag = app.drags().last();
    drag.let_go(LetGo::On(on));
    app.drag_ended();
    drag.given()
}

/// Dragged out of the window onto another app, a rendered edit gives it its
/// video under its title (edits.rs `drag_edit`: "Starts dragging edit `id`'s
/// video out of the window, as for a clip (`clips::drag_clip`)"; app
/// clips.rs `drag_name`: "an edit's … its title").
#[test]
fn dragging_a_rendered_edit_out_gives_its_video_under_its_title() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0), (vod.id, 30.0, 35.0)]);
    let video = rendered_now(&app, edit_id);
    let given = dragged_onto(&app, edit_id, Receiver::taking_files("Kdenlive"));
    assert_eq!(app.drags().last().names(), ["Best of.mp4"]);
    let [given] = &given[..] else { panic!("given {given:?}") };
    assert_eq!(std::fs::read(given).ok(), std::fs::read(&video).ok(), "not its video");
}

/// An edit of one piece is handed out under its title too, shown in its
/// folder or dragged out, as any edit is (edits.rs `handed_out`: "Edit `id`'s
/// rendered video under the name it's handed out by: its title"; app clips.rs
/// `drag_name`: "an edit's … its title, a clip its VOD's date, title and
/// range").
#[test]
#[ignore = "disagreement: an edit of one piece is handed out under its VOD's date, title and range, not its title"]
fn an_edit_of_one_piece_is_handed_out_under_its_title_too() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    rendered_now(&app, edit_id);
    app.invoke("show_edit", json!({ "id": edit_id })).unwrap();
    let shown = app.desktop().shown().pop().expect("shown in the file manager");
    assert_eq!(shown.file_name().and_then(|n| n.to_str()), Some("Best of.mp4"), "shown in its folder");
    dragged_onto(&app, edit_id, Receiver::taking_files("Kdenlive"));
    assert_eq!(app.drags().last().names(), ["Best of.mp4"], "dragged out");
}

/// A drag of an edit called off gives its video to no app: the name it was
/// handed out by goes with the edit (edits.rs `handed_out`: "Noted as handed
/// out once another app has it (`taken`)").
#[test]
fn an_edit_dragged_out_and_called_off_keeps_nothing_for_another_app() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0), (vod.id, 30.0, 35.0)]);
    rendered_now(&app, edit_id);
    app.invoke("drag_edit", json!({ "id": edit_id, "drag": 1 })).unwrap();
    app.drags().last().let_go(LetGo::CalledOff);
    app.drag_ended();
    app.invoke("delete_edit", json!({ "id": edit_id })).unwrap();
    let names = std::fs::read_dir(app.paths().clip_names(edit_id)).map(|d| d.count()).unwrap_or(0);
    assert_eq!(names, 0, "a name no app took stayed");
}

/// An edit not rendered can't be dragged out, and says so (edits.rs
/// `handed_out`: "This edit isn't rendered yet.").
#[test]
fn dragging_an_edit_not_rendered_says_so() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let dragged = app.invoke("drag_edit", json!({ "id": edit_id, "drag": 1 }));
    assert_eq!(dragged, Err(json!("This edit isn't rendered yet.")));
    assert_eq!(app.drags().begun(), 0);
}

/// A drag of an edit whose mouse button came up before it could begin
/// doesn't begin, and says so with false (edits.rs `drag_edit`: "False if it
/// didn't begin, the button up first").
#[test]
fn a_drag_of_an_edit_whose_button_came_up_first_does_not_begin() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    rendered_now(&app, edit_id);
    app.drags().releases_early();
    assert_eq!(app.invoke("drag_edit", json!({ "id": edit_id, "drag": 1 })), Ok(json!(false)));
    assert_eq!(app.drags().begun(), 0);
}

/// On Linux, an MP4 edit let go on DaVinci Resolve, which can't open it, is
/// refused there, and the page hears so, to offer MOV (edits.rs
/// `drag_edit`: "an MP4 is watched for DaVinci Resolve as a clip's is
/// (resolve.rs)"; CHANGELOG.md, 0.7.2: "Dropping on DaVinci Resolve offers
/// MOV. On Linux, where Resolve can't open MP4, a clip or edit dropped on it
/// is held back and VODscribe offers to switch to MOV").
#[cfg(target_os = "linux")]
#[test]
fn an_mp4_edit_let_go_on_resolve_on_linux_is_refused_there() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    rendered_now(&app, edit_id);
    app.invoke("drag_edit", json!({ "id": edit_id, "drag": 1 })).unwrap();
    let drag = app.drags().last();
    drag.let_go(LetGo::On(Receiver::taking_files("resolve")));
    let ended = app.drag_ended();
    assert_eq!(ended["resolve"], "refused", "{ended}");
    assert!(drag.given().is_empty(), "Resolve was given {:?}", drag.given());
}

/// The picture VOD `vod` shows in the library: `bytes`, its thumbnail.
fn pictured(app: &TestApp, vod: i64, bytes: &[u8]) {
    let name = format!("{vod}.jpg");
    std::fs::create_dir_all(app.paths().thumbs()).unwrap();
    std::fs::write(app.paths().source_thumb(&name), bytes).unwrap();
    app.db().set_thumb(vod, &name).unwrap();
}

/// Edit `id`'s picture, as the page asks for it (api.ts `editThumb`).
fn thumb_of(app: &TestApp, id: i64) -> Vec<u8> {
    app.invoke_bytes("edit_thumb", json!({ "id": id })).unwrap()
}

/// An edit's picture is its first piece's first frame, a JPEG, even where
/// its VOD has a thumbnail (edits.rs `edit_thumb`: "Edit `id`'s picture: its
/// first piece's first frame (a JPEG)").
#[test]
fn an_edit_s_picture_is_its_first_piece_s_first_frame() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    pictured(&app, vod.id, b"the VOD's picture");
    let edit_id = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let thumb = thumb_of(&app, edit_id);
    assert!(thumb.starts_with(&[0xff, 0xd8]), "not a JPEG: {thumb:?}");
}

/// An edit whose first piece's footage isn't here yet is pictured by that
/// piece's VOD's thumbnail (edits.rs `edit_thumb`: "or failing that (its
/// footage isn't here yet) its VOD's thumbnail").
#[test]
fn an_edit_whose_footage_is_not_here_yet_is_pictured_by_its_vod_s_thumbnail() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "Test video", 600.0);
    pictured(&app, vod.id, b"the VOD's picture");
    let edit_id = app.edit_of(&[(vod.id, 60.0, 90.0)]);
    assert_eq!(thumb_of(&app, edit_id), b"the VOD's picture");
}

/// An edit with neither a frame nor a thumbnail to show gives an empty
/// picture: one with nothing in it, and one whose footage isn't here and
/// whose VOD has no thumbnail (edits.rs `edit_thumb`: "empty if neither").
#[test]
fn an_edit_with_no_picture_to_give_gives_an_empty_one() {
    let app = TestApp::new();
    let empty = app.invoke("create_edit", json!({})).unwrap()["id"].as_i64().unwrap();
    assert_eq!(thumb_of(&app, empty), Vec::<u8>::new(), "an edit with nothing in it");
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "Test video", 600.0);
    let edit_id = app.edit_of(&[(vod.id, 60.0, 90.0)]);
    assert_eq!(thumb_of(&app, edit_id), Vec::<u8>::new(), "its footage not here, its VOD unpictured");
}

/// An edit not asked to render isn't rendered at the next start: one whose
/// render was stopped, and one changed since it was rendered (core edits.rs
/// `unfinished_renders`: "Those only waiting (changed since their file) are
/// rendered when asked"). One a quit stopped rendering is: tests/start.rs
/// `an_edit_a_quit_stopped_rendering_is_rendered_at_the_next_start`.
#[test]
fn an_edit_not_asked_to_render_is_not_rendered_at_the_next_start() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let stopped = app.edit_of(&[(vod.id, 10.0, 20.0)]);
    let changed = app.edit_of(&[(vod.id, 30.0, 40.0)]);
    let video = rendered_now(&app, changed);
    add_piece(&app, changed, vod.id);
    let halfway = held_halfway(&app, stopped);
    stop(&app, stopped);
    halfway.open();
    rendered(&app, stopped);
    let app = TestApp::on(app.quit());
    queue_passed(&app, vod.id);
    assert_eq!(edit(&app, stopped)["status"], "waiting", "the stopped one rendered");
    assert!(renders_of(&app, stopped).is_empty(), "{:?}", renders_of(&app, stopped));
    let after = edit(&app, changed);
    assert_eq!(after["file_path"].as_str(), video.to_str(), "the changed one rendered");
    assert_eq!(after["current"], false);
}
