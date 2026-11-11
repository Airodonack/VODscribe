//! What a clip is made as (Settings › Encoding), and what it's made again as
//! once that changes: through `set_settings` as the page saves it,
//! `add_clip`, and `set_clip_range` on a clip's own range, which is how the
//! page asks a clip to be made again (dragout.ts `remade`); for a clip of an
//! edit's clip bar, `render_edit_now` (dragout.ts `remade` again).

use super::*;

use serde_json::json;
use vodscribe_core::settings::{ClipCodec, ClipEncoding, ClipQuality};
use vodscribe_core::testkit::{Gate, Media};

use crate::testkit::TestApp;

const H264: ClipCodec = ClipCodec::H264;
const H265: ClipCodec = ClipCodec::H265;
const DNXHR: ClipCodec = ClipCodec::Dnxhr;
const PRORES: ClipCodec = ClipCodec::Prores;
const PROXY: ClipQuality = ClipQuality::Proxy;
const STANDARD: ClipQuality = ClipQuality::Standard;
const HIGH: ClipQuality = ClipQuality::High;

fn made_with(codec: ClipCodec, quality: ClipQuality) -> ClipEncoding {
    ClipEncoding { codec, quality }
}

/// Settings › Encoding set to `codec` at `quality`, saved as the page saves
/// it (`set_settings`; the format is the one the codec goes in).
fn clips_are(app: &TestApp, codec: ClipCodec, quality: ClipQuality) {
    let settings = json!({ "clip_codec": codec, "clip_quality": quality });
    app.invoke("set_settings", json!({ "settings": settings })).unwrap();
}

/// Clip `clip` made again as clips are made now, as the page asks it on a
/// drag (dragout.ts `remade`: `setClipRange` on its own range), and waited for.
fn made_again(app: &TestApp, clip: &Clip) -> Clip {
    app.invoke("set_clip_range", json!({ "id": clip.id, "start": clip.start, "end": clip.end })).unwrap();
    app.made(clip.id)
}

/// The codec of the picture in clip `clip`'s file, as ffmpeg names it, and
/// its container.
fn picture_in(app: &TestApp, clip: &Clip) -> (String, String) {
    let media = app.clip_media(clip);
    (media.video.expect("a picture").codec, media.container)
}

/// Clip `clip`'s file's extension.
fn extension_of(clip: &Clip) -> &str {
    Path::new(clip.file_path.as_deref().expect("a file")).extension().unwrap().to_str().unwrap()
}

/// Whether clip `clip`, as the page lists it on its clip bar (`list_clips`,
/// or `edit_clips` for an edit's), is made as clips are made now: what a
/// drag asks of it before handing it over (dragout.ts `madeAsWanted`, which
/// core `clips::made_as` says the same as).
fn made_as_wanted(app: &TestApp, clip: &Clip) -> bool {
    let listed = match clip.bar {
        Some(ClipBar::Edit { id }) => app.invoke("edit_clips", json!({ "id": id })),
        _ => app.invoke("list_clips", json!({ "vodId": clip.vod_id })),
    }
    .unwrap();
    let listed = listed.as_array().unwrap().iter().find(|c| c["id"] == clip.id).expect("listed").clone();
    let encoding: Option<ClipEncoding> = serde_json::from_value(listed["encoding"].clone()).unwrap();
    let file = listed["file_path"].as_str().expect("made");
    let now = Settings::load(&app.paths().settings()).clip_encoding();
    vodscribe_core::clips::made_as(Path::new(file), encoding, now)
}

/// A clip of VOD `vod_id`'s 0:10–0:20 made as `from`, then Settings changed
/// to `to`: whether a drag finds it made as wanted then, and the clip made
/// again on that drag.
fn changed_from(app: &TestApp, vod_id: i64, from: ClipEncoding, to: ClipEncoding) -> (bool, Clip) {
    clips_are(app, from.codec, from.quality);
    let clip = app.make_clip(vod_id, "0:10", "0:20");
    assert_eq!(clip.encoding, Some(from));
    clips_are(app, to.codec, to.quality);
    let wanted = made_as_wanted(app, &clip);
    (wanted, made_again(app, &clip))
}

/// An edit of a minute of VOD `vod_id`, and a clip of its 0:05–0:09 on its
/// clip bar, made (`add_edit_clip`).
fn edit_bar_clip(app: &TestApp, vod_id: i64) -> Clip {
    let edit = {
        let db = app.db();
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod_id, (0.0, 60.0), "a", None).unwrap();
        edit
    };
    let made = app.invoke("add_edit_clip", json!({ "editId": edit, "from": 5.0, "to": 9.0 })).unwrap();
    app.made(made["id"].as_i64().unwrap())
}

/// What the page writes to the system's clipboard for a copy, in these tests.
const COPIED: &str = "copied";

/// Ctrl+C on clip `id`, `between` done, then Ctrl+V on VOD `vod_id`'s clip
/// bar, as the page does them (copyPaste.ts): the clip pasted, once made.
fn copied_and_pasted(app: &TestApp, id: i64, vod_id: i64, between: impl FnOnce()) -> Clip {
    app.invoke("copy_items", json!({ "items": [{ "kind": "clip", "id": id }] })).unwrap();
    app.invoke("copied_text", json!({ "text": COPIED })).unwrap();
    between();
    let target = json!({ "kind": "vod", "id": vod_id });
    let pasted = app
        .invoke("paste_items", json!({ "target": target, "bar": true, "at": null, "after": null, "text": COPIED }))
        .unwrap();
    let ids: Vec<i64> = pasted["clips"].as_array().unwrap().iter().map(|c| c["id"].as_i64().unwrap()).collect();
    let [id] = ids[..] else { panic!("pasted {ids:?}") };
    app.made(id)
}

/// C1. Clips are made in the codec Settings › Encoding has, each in its file:
/// H.264 and H.265 in MP4, DNxHR and ProRes in MOV (CHANGELOG.md, 0.7.2:
/// "Clips come in more formats. Settings › Encoding adds H.265 and ProRes
/// beside H.264 and DNxHR"; tests/e2e/scenarios/clip-format.mjs: "Clips made
/// as chosen in Settings").
#[test]
fn a_clip_is_made_as_settings_encoding_says() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    for (codec, (from, to), picture, container) in [
        (H264, ("0:00", "0:05"), "h264", "mp4"),
        (H265, ("0:10", "0:15"), "hevc", "mp4"),
        (DNXHR, ("0:20", "0:25"), "dnxhd", "mov"),
        (PRORES, ("0:30", "0:35"), "prores", "mov"),
    ] {
        clips_are(&app, codec, STANDARD);
        let clip = app.make_clip(vod.id, from, to);
        assert_eq!(picture_in(&app, &clip), (picture.into(), container.into()), "as {codec:?}");
        assert_eq!(extension_of(&clip), container, "as {codec:?}");
    }
}

/// C2. A clip of a file of sound alone is sound alone: an M4A (AAC) in MP4,
/// a WAV in MOV (tests/e2e/scenarios/clip-format.mjs: "as MOV DNxHR, a
/// sound-only file's clip is a .wav and a video's a .mov"; core clips.rs
/// `extension`: "sound alone is M4A (AAC) or WAV (PCM)").
#[test]
fn a_clip_of_sound_alone_is_an_m4a_or_a_wav_by_the_format() {
    let app = TestApp::new();
    let talk = app.add_file("Talk.m4a", &Media::sound(60.0));
    for (codec, (from, to), ext, container, sound) in
        [(H264, ("0:10", "0:15"), "m4a", "mp4", "aac"), (DNXHR, ("0:30", "0:35"), "wav", "wav", "pcm_s16le")]
    {
        clips_are(&app, codec, STANDARD);
        let clip = app.make_clip(talk.id, from, to);
        let media = app.clip_media(&clip);
        assert_eq!(extension_of(&clip), ext, "as {codec:?}");
        assert_eq!((media.video, media.container.as_str(), media.audio.as_deref()), (None, container, Some(sound)));
    }
}

/// C3. A made clip keeps what it was made with, codec and quality (app
/// clips.rs:515, `cut`: "and what with, so a change of codec or quality does
/// (dragout.ts)").
#[test]
fn a_clip_records_the_codec_and_quality_it_was_made_with() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    clips_are(&app, H265, HIGH);
    let hevc = app.make_clip(vod.id, "0:10", "0:20");
    clips_are(&app, PRORES, PROXY);
    let prores = app.make_clip(vod.id, "0:30", "0:40");
    assert_eq!(hevc.encoding, Some(made_with(H265, HIGH)));
    assert_eq!(prores.encoding, Some(made_with(PRORES, PROXY)));
}

/// C4. The 0.7.2 encoding-remake bug: with clips changed to H.265, a pill
/// made as H.264 dragged out was handed over as it was. Made again, it's
/// H.265 (handbook plans/0.7.2.md, "The owner's walkthrough", Fix before the
/// tag (walk-clips): "changing the encoding and remaking a clip from the
/// same slice does nothing"; tests/e2e/scenarios/drag-resolve.mjs step 5:
/// "MP4 H.265, the pill made as H.264: dragged out, it's made again";
/// fixed by 8896230).
#[test]
fn a_clip_made_again_after_a_change_of_codec_is_in_the_new_codec() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    assert_eq!(picture_in(&app, &clip).0, "h264");
    clips_are(&app, H265, STANDARD);
    let again = made_again(&app, &clip);
    assert_eq!(picture_in(&app, &again), ("hevc".into(), "mp4".into()));
    assert_eq!(again.encoding, Some(made_with(H265, STANDARD)));
}

/// C5. A clip made again in another format leaves no file in the format
/// before (app clips.rs:532, `cut`: "Its file in the format before goes";
/// CHANGELOG.md, 0.4.0: "Files you made and no longer use don't pile up in
/// Videos/VODscribe").
#[test]
fn a_clip_made_again_in_another_format_loses_its_file_in_the_one_before() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let mp4 = PathBuf::from(clip.file_path.clone().unwrap());
    clips_are(&app, DNXHR, STANDARD);
    let again = made_again(&app, &clip);
    assert_eq!(extension_of(&again), "mov");
    assert!(Path::new(again.file_path.as_deref().unwrap()).is_file(), "no MOV");
    assert!(!mp4.exists(), "the MP4 is left");
}

/// A clip of VOD `vod_id`'s 0:10–0:20 being made as H.264 MP4 when clips
/// are switched to MOV DNxHR (Standard: the quality the user had), once made.
fn made_while_switched_to_mov(app: &TestApp, vod_id: i64) -> Clip {
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let clip = app.add_clip(vod_id, "0:10", "0:20").unwrap();
    halfway.reached();
    clips_are(app, DNXHR, STANDARD);
    halfway.open();
    app.made(clip.id)
}

/// C6. A clip being made when the format changes is made as its making
/// began: the change doesn't reach a cut under way (dragout.ts:341,
/// `remadeOnceMade`: "one still being made when the drag ended … is made in
/// the format its making began in"; handbook reports master/072
/// review-correctness-072.md finding 5: "The cutter reads the format at cut
/// start").
#[test]
fn a_clip_being_made_when_the_format_changes_is_made_in_the_one_its_making_began_in() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = made_while_switched_to_mov(&app, vod.id);
    assert_eq!(picture_in(&app, &clip), ("h264".into(), "mp4".into()));
    assert_eq!(extension_of(&clip), "mp4");
    assert_eq!(clip.encoding, Some(made_with(H264, STANDARD)), "recorded as what it wasn't made with");
}

/// C7. The 0.7.2 Resolve MOV bug, the app's half: switched to MOV while a
/// clip was being made, it was made as MP4 and stayed so. Asked again once
/// made, it's MOV DNxHR, and the MP4 goes (CHANGELOG.md, 0.7.2: "Dropping on
/// DaVinci Resolve offers MOV."; tests/e2e/scenarios/drag-resolve.mjs:
/// "Switch to MOV makes it again as MOV once it's made as MP4";
/// review-correctness-072.md finding 5; fixed by 8499956).
#[test]
fn switched_to_mov_a_clip_made_as_mp4_meanwhile_is_made_again_as_mov() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = made_while_switched_to_mov(&app, vod.id);
    let mp4 = PathBuf::from(clip.file_path.clone().unwrap());
    assert!(!made_as_wanted(&app, &clip), "an MP4 taken as MOV");
    let again = made_again(&app, &clip);
    assert_eq!(picture_in(&app, &again), ("dnxhd".into(), "mov".into()));
    assert_eq!(again.encoding, Some(made_with(DNXHR, STANDARD)));
    assert!(!mp4.exists(), "the MP4 is left");
}

/// Switch to MOV, as the Resolve dialog saves it (resolveDrop.svelte.ts
/// `answer`): the settings as they are, with clips' codec DNxHR in MOV.
fn switched_to_mov(app: &TestApp) {
    let mut settings = app.invoke("get_settings", json!({})).unwrap();
    settings["clip_codec"] = json!("dnxhr");
    settings["clip_format"] = json!("mov");
    app.invoke("set_settings", json!({ "settings": settings })).unwrap();
}

/// C8. Switched to MOV, clips are made as DNxHR at the quality the user
/// chose, its profile in the file (handbook plans/0.7.2.md, resolve-drop:
/// "Switch to MOV sets MOV DNxHR at the user's quality (the old note forced
/// Proxy)"; core clips.rs:130, `output_args`: DNxHR's profile `dnxhr_lb`,
/// `dnxhr_sq` or `dnxhr_hq` by quality).
#[test]
fn switched_to_mov_clips_are_dnxhr_at_the_quality_chosen() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let qualities = [
        (PROXY, ("0:10", "0:20"), "dnxhr_lb"),
        (STANDARD, ("0:20", "0:30"), "dnxhr_sq"),
        (HIGH, ("0:30", "0:40"), "dnxhr_hq"),
    ];
    for (quality, (from, to), profile) in qualities {
        clips_are(&app, H264, quality);
        let clip = app.make_clip(vod.id, from, to);
        switched_to_mov(&app);
        let again = made_again(&app, &clip);
        let picture = app.clip_media(&again).video.expect("a picture");
        assert_eq!((picture.codec.as_str(), picture.profile.as_deref()), ("dnxhd", Some(profile)), "{quality:?}");
        assert_eq!(again.encoding, Some(made_with(DNXHR, quality)));
    }
}

/// C9. A clip of an edit's clip bar is made again after a change of codec or
/// quality, as a VOD's is (handbook plans/0.7.2.md, "The owner's
/// walkthrough": "an edit's clip bar and a rendered edit are remade after an
/// encoding change"; tests/e2e/scenarios/drag-resolve.mjs step 6: "The clip
/// of its bar is made again on its drag, as the VOD's pill was"; fixed by
/// 6fc7f41).
#[test]
fn a_clip_on_an_edit_s_clip_bar_is_made_again_after_a_change_of_codec_or_quality() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = edit_bar_clip(&app, vod.id);
    assert_eq!(clip.encoding, Some(made_with(H264, STANDARD)));
    clips_are(&app, H265, HIGH);
    assert!(!made_as_wanted(&app, &clip), "taken as made as wanted");
    app.invoke("render_edit_now", json!({ "id": clip.id })).unwrap();
    let again = app.made(clip.id);
    assert_eq!(picture_in(&app, &again).0, "hevc");
    assert_eq!(again.encoding, Some(made_with(H265, HIGH)));
}

/// C10. A clip of an edit's clip bar, changed in the editor and rendered,
/// made as clips are made now, isn't rendered again when it's asked to
/// (handbook reports master/072 walk-clips-report.md, item 1: render_edit
/// "now skips only when `vodscribe_core::clips::made_as` holds"; edits.rs
/// `render_edit`: "A snippet whose file is of it as it is already …, and
/// made as clips are made now …, isn't made again"; core clips.rs:118,
/// `made_as`).
#[test]
fn a_clip_on_an_edit_s_clip_bar_made_as_wanted_is_not_rendered_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = edit_bar_clip(&app, vod.id);
    // A second out of its middle in the editor: two pieces, rendered as an edit is.
    app.invoke("remove_stretch", json!({ "id": clip.id, "from": 1.0, "to": 2.0 })).unwrap();
    let clip = app.made(clip.id);
    assert_eq!(app.db().edit_pieces(clip.id).unwrap().len(), 2, "not two pieces");
    let media = app.clip_media(&clip);
    assert!(made_as_wanted(&app, &clip));
    // Made again, it would fail.
    app.ffmpeg.fails_at(0.5, "the source broke off");
    app.invoke("render_edit_now", json!({ "id": clip.id })).unwrap();
    let after = app.made(clip.id);
    assert_eq!((after.status, after.error.as_deref()), (ClipStatus::Ready, None), "rendered again");
    assert_eq!(app.clip_media(&after), media);
}

/// C11. A pasted copy of a made clip is given its file, and keeps what that
/// was made with, though clips are made otherwise now (handbook reports
/// master/072 walk-clips-report.md, files: "clipboard.rs (a pasted copy
/// keeps the encoding)").
#[test]
fn a_pasted_copy_keeps_what_its_file_was_made_with() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let pasted = copied_and_pasted(&app, clip.id, vod.id, || clips_are(&app, H265, HIGH));
    assert_eq!(pasted.encoding, Some(made_with(H264, STANDARD)));
    assert_eq!(picture_in(&app, &pasted).0, "h264");
}

/// C12. A settings file from before codecs were a choice, with only its
/// format, makes clips as they were made then: MP4 as H.264 Standard, MOV as
/// DNxHR Proxy (core settings.rs:131, `clip_codec`: "unset only in a file
/// from before they were a choice, read as clips were made then";
/// `clip_encoding`: "H.264 at CRF 18 or DNxHR LB (the lowest step, as
/// `ClipQuality::Proxy`)").
#[test]
fn an_old_settings_file_makes_clips_as_they_were_made_then() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    for (format, (from, to), made, picture) in [
        ("mp4", ("0:10", "0:15"), made_with(H264, STANDARD), "h264"),
        ("mov", ("0:30", "0:35"), made_with(DNXHR, PROXY), "dnxhd"),
    ] {
        std::fs::write(app.paths().settings(), json!({ "clip_format": format }).to_string()).unwrap();
        let clip = app.make_clip(vod.id, from, to);
        assert_eq!(clip.encoding, Some(made), "as an old {format}");
        assert_eq!(picture_in(&app, &clip), (picture.into(), format.into()));
    }
}

/// P1. A change of codec within MP4 (H.264 to H.265) makes a made clip again
/// on its next drag (CHANGELOG.md, 0.7.2: "Clips come in more formats.";
/// 8896230, "A clip is made again after a change of codec or quality":
/// "dragout.ts compared the file's extension only").
#[test]
fn a_change_of_codec_within_mp4_makes_a_made_clip_again_on_its_next_drag() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let (wanted, again) = changed_from(&app, vod.id, made_with(H264, STANDARD), made_with(H265, STANDARD));
    assert!(!wanted, "an H.264 clip taken as H.265");
    assert_eq!(picture_in(&app, &again), ("hevc".into(), "mp4".into()));
}

/// P2. A change of codec within MOV (DNxHR to ProRes) makes a made clip again
/// on its next drag (CHANGELOG.md, 0.7.2: "Clips come in more formats.";
/// 8896230; walk-clips-report.md item 1: "H.264→H.265 or ProRes→DNxHR
/// changed nothing").
#[test]
fn a_change_of_codec_within_mov_makes_a_made_clip_again_on_its_next_drag() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let (wanted, again) = changed_from(&app, vod.id, made_with(DNXHR, STANDARD), made_with(PRORES, STANDARD));
    assert!(!wanted, "a DNxHR clip taken as ProRes");
    assert_eq!(picture_in(&app, &again), ("prores".into(), "mov".into()));
}

/// P3. A change of quality alone makes a made clip again on its next drag
/// (CHANGELOG.md, 0.7.2: "a Proxy, Standard or High quality, for clips and
/// edits alike"; 8896230; walk-clips-report.md item 1: "and a change of
/// quality didn't either").
#[test]
fn a_change_of_quality_alone_makes_a_made_clip_again_on_its_next_drag() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let (wanted, again) = changed_from(&app, vod.id, made_with(H264, STANDARD), made_with(H264, HIGH));
    assert!(!wanted, "a Standard clip taken as High");
    assert_eq!(again.encoding, Some(made_with(H264, HIGH)));
}

/// P4. A copy pasted from a clip made before codecs were recorded keeps
/// that unknown, and reads as a file of its format was made then: an MP4
/// as H.264 Standard (8896230: "null for a file made before, read by its
/// extension: mp4 as H.264 Standard, mov as DNxHR Proxy"; "a pasted copy
/// keeps what it was made with (clipboard.rs)").
#[test]
fn a_copy_pasted_from_a_clip_made_before_codecs_were_recorded_reads_as_its_format_was_made_then() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    // As a library from 0.7.1 has it after migration V41: made, nothing recorded.
    app.db().set_made_as(clip.id, None).unwrap();
    let pasted = copied_and_pasted(&app, clip.id, vod.id, || {});
    assert_eq!(pasted.encoding, None, "recorded as made with something");
    assert!(made_as_wanted(&app, &pasted), "an MP4 from then not taken as H.264 Standard");
    clips_are(&app, H264, HIGH);
    assert!(!made_as_wanted(&app, &pasted), "an MP4 from then taken as H.264 High");
}

/// C on a stretch whose clip was made in an encoding other than the one
/// Settings › Encoding has now makes it again as clips are made now; one made
/// as wanted is given as it is (plans/clips-pilot.md, Decided 2: "C on a
/// stretch whose clip was made in an older encoding makes it again"; core
/// clips.rs `made_as`).
#[test]
fn clipping_a_stretch_whose_clip_was_made_in_an_older_encoding_makes_it_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let h264 = PathBuf::from(clip.file_path.clone().unwrap());
    clips_are(&app, H265, STANDARD);
    let asked = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    assert_eq!((asked.id, asked.status), (clip.id, ClipStatus::Waiting), "given as it was");
    let again = app.made(clip.id);
    assert_eq!(picture_in(&app, &again), ("hevc".into(), "mp4".into()));
    assert_eq!(again.encoding, Some(made_with(H265, STANDARD)));
    assert!(!h264.exists(), "the H.264 file is left");
    let given = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    assert_eq!((given.status, given.file_path), (ClipStatus::Ready, again.file_path), "made again as wanted");
}

/// The same on an edit's clip bar: C on a stretch whose clip was made in an
/// older encoding makes it again (Decided 2, as above).
#[test]
fn clipping_an_edit_s_stretch_whose_clip_was_made_in_an_older_encoding_makes_it_again() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = edit_bar_clip(&app, vod.id);
    let edit = match clip.bar {
        Some(ClipBar::Edit { id }) => id,
        bar => panic!("on {bar:?}"),
    };
    clips_are(&app, PRORES, HIGH);
    let asked = app.invoke("add_edit_clip", json!({ "editId": edit, "from": 5.0, "to": 9.0 })).unwrap();
    assert_eq!((asked["id"].as_i64(), asked["status"].as_str()), (Some(clip.id), Some("waiting")));
    let again = app.made(clip.id);
    assert_eq!(picture_in(&app, &again), ("prores".into(), "mov".into()));
    assert_eq!(again.encoding, Some(made_with(PRORES, HIGH)));
}
