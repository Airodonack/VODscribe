//! The app's start and quit: what each start tidies before anything runs on
//! the library, the queues taking up what was left from last time, and what
//! a quit leaves to the next start (lib.rs `open_library`, `tidy_at_start`,
//! `quit`). The start is `TestApp::on`, as `open_library` runs it; the quit
//! `TestApp::quit`. Each test is named for what it says the app does and
//! cites where that's said.

use std::path::{Path, PathBuf};

use serde_json::json;
use vodscribe_core::db::{ClipStatus, Db};
use vodscribe_core::testkit::{Gate, eventually};

use crate::testkit::{Folder, TestApp};

/// Writes each of `files`, its folder made if need be.
fn write(files: &[&Path]) {
    for file in files {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, b"left").unwrap();
    }
}

/// The library of an app that ended on `folder`, opened as the next start
/// opens it, before that start.
fn library_of(folder: &Folder) -> Db {
    Db::open_in(&folder.paths()).unwrap()
}

// What a start tidies.

/// Edits' preview segments, "from before edits played in the app's own
/// player (up to 2 GB): nothing reads them now", go at the start (lib.rs:233,
/// `tidy_at_start`).
#[test]
fn at_start_edits_preview_segments_from_before_go() {
    let folder = Folder::new();
    let paths = folder.paths();
    let segment = paths.temp_preview().join("3").join("seg-00001.ts");
    write(&[&segment]);
    let _app = TestApp::on(folder);
    assert!(!paths.temp_preview().exists(), "the preview segments were kept");
}

/// What a site's player was fed (`Paths::temp_embed`, kept while its VOD
/// plays in the app's player) goes at the start (lib.rs:236, `embed::clear`).
#[test]
fn at_start_what_a_sites_player_was_fed_goes() {
    let folder = Folder::new();
    let paths = folder.paths();
    let fed = paths.temp_embed().join("1").join("1-3.mp4");
    write(&[&fed]);
    let _app = TestApp::on(folder);
    assert!(!paths.temp_embed().exists(), "what a site's player was fed was kept");
}

/// "Footage sections a quit (or a crash) left downloading or downloaded in
/// the work folder" go at the start (lib.rs:237, `footage::tidy_work`).
#[test]
fn at_start_footage_a_quit_left_downloading_goes() {
    let app = TestApp::new();
    let vod = app.add_site_vod("twitch", "2901646294", "Day 3", 600.0);
    let folder = app.quit();
    let stem = vodscribe_core::footage::section_stem(vod.id, 60.0, 120.0);
    let work = folder.paths().work();
    let downloading = work.join(format!("footage-{stem}.f251.webm.part"));
    let downloaded = work.join(format!("footage-{stem}.mkv"));
    write(&[&downloading, &downloaded]);
    let _app = TestApp::on(folder);
    assert!(!downloading.exists(), "a section being downloaded was kept");
    assert!(!downloaded.exists(), "a section downloaded into the work folder was kept");
}

/// "Files a quit (or a crash) left being received from another app's drop"
/// go at the start: part files, the folders an archive manager saved into
/// first, and a file received whole that no VOD has. A VOD's own received
/// file stays, in its place or still where it arrived, as a crash before
/// the move leaves it (lib.rs:243; receive.rs:128, `receive::tidy`; core
/// db.rs `add_received`: "till it's moved there it plays where it
/// arrived"). The start's settle moves that one to its place first
/// (`library::settle`).
#[test]
fn at_start_files_a_quit_left_being_received_go() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let paths = app.paths();
    let own = paths.source_received(vod.id, "mp4");
    std::fs::create_dir_all(own.parent().unwrap()).unwrap();
    std::fs::copy(vod.source_path.as_deref().unwrap(), &own).unwrap();
    app.db().set_received(vod.id, &own.to_string_lossy()).unwrap();
    let incoming = paths.received_incoming();
    let arrived = incoming.join("Day 7.mp4");
    std::fs::create_dir_all(&incoming).unwrap();
    std::fs::copy(&own, &arrived).unwrap();
    let path = arrived.to_string_lossy().into_owned();
    let info = vodscribe_core::fetch::VodInfo {
        platform: vodscribe_core::local::PLATFORM.into(),
        platform_id: path.clone(),
        url: path,
        title: "Day 7".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(60.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let arriving = app.db().add_received(&info).unwrap();
    let folder = app.quit();
    let part = incoming.join("Day 4.mp4.part");
    let saved_first = incoming.join(".dragndrop-1a2b").join("Day 5.mp4");
    let whole = incoming.join("Day 6.mp4");
    write(&[&part, &saved_first, &whole]);
    let app = TestApp::on(folder);
    assert!(!part.exists(), "a file being received was kept");
    assert!(!saved_first.parent().unwrap().exists(), "the folder an archive manager saved into was kept");
    assert!(!whole.exists(), "a file received whole that no VOD has was kept");
    assert!(own.is_file(), "VOD {}'s own received file went", vod.id);
    let kept = app.db().vod(arriving.id).unwrap().unwrap().source_path.expect("its file");
    assert!(Path::new(&kept).is_file(), "the file VOD {} arrived with went", arriving.id);
}

/// "Kept videos' leftovers from VODs no longer in the library" go at the
/// start; those of VODs still there stay, "as a paused download carries on
/// from its parts" (lib.rs:247; core pipeline.rs:253, `tidy_media`).
#[test]
fn at_start_kept_videos_of_vods_no_longer_in_the_library_go() {
    let app = TestApp::new();
    let vod = app.add_site_vod("twitch", "2901646294", "Day 3", 600.0);
    let folder = app.quit();
    let media = folder.paths().media();
    // Ids are never used again: one past the last is a VOD removed.
    let gone = vod.id + 1;
    let leftovers = [media.join(format!("{gone}.mp4")), media.join(format!("{gone}.f399.mp4.part"))];
    let kept = media.join(format!("{}.f399.mp4.part", vod.id));
    write(&[&leftovers[0], &leftovers[1], &kept]);
    let _app = TestApp::on(folder);
    for file in &leftovers {
        assert!(!file.exists(), "{} was kept", file.display());
    }
    assert!(kept.is_file(), "a paused download's part of VOD {} went", vod.id);
}

/// VODs added before the library kept their channel's login and page get
/// them at the start, "from their info where it's still kept": one added
/// before either was kept gets both, and one added after the login was
/// kept but before the page was gets its page (lib.rs:251; core db.rs:789,
/// version 40: "Null for a VOD added before, filled from its info where
/// that is still kept (`pipeline::fill_channels`)").
#[test]
fn at_start_vods_added_before_channels_were_kept_get_them_from_their_info() {
    let app = TestApp::new();
    let neither = app.add_site_vod("youtube", "noa29qsTc4g", "Day 3", 600.0);
    let no_page = app.add_site_vod("twitch", "2901646294", "Day 4", 600.0);
    app.db().fill_channel(no_page.id, Some("quill"), None).unwrap();
    let folder = app.quit();
    let paths = folder.paths();
    let infos = [
        (
            neither.id,
            json!({ "id": "noa29qsTc4g", "extractor_key": "Youtube", "webpage_url": neither.info.url,
            "uploader_id": "@quill", "uploader_url": "https://www.youtube.com/@quill" }),
        ),
        (
            no_page.id,
            json!({ "id": "2901646294", "extractor_key": "TwitchVod", "webpage_url": no_page.info.url,
            "uploader_id": "quill", "uploader_url": "https://www.twitch.tv/quill" }),
        ),
    ];
    for (id, info) in infos {
        write(&[&paths.source_info(id)]);
        std::fs::write(paths.source_info(id), info.to_string()).unwrap();
    }
    let app = TestApp::on(folder);
    let channel = |id: i64| {
        let info = app.db().vod(id).unwrap().unwrap().info;
        (info.channel_login, info.channel_url)
    };
    assert_eq!(channel(neither.id), (Some("@quill".into()), Some("https://www.youtube.com/@quill".into())));
    assert_eq!(channel(no_page.id), (Some("quill".into()), Some("https://www.twitch.tv/quill".into())));
}

/// The loudness over time of VODs no longer in the library goes at the
/// start; a VOD's still there stays (lib.rs:254, `peaks::tidy`; core
/// peaks.rs:121: "Removes the files in `dir` of VODs not in `library`").
#[test]
fn at_start_loudness_of_vods_no_longer_in_the_library_goes() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let folder = app.quit();
    let peaks = folder.paths().peaks();
    let (kept, gone) = (peaks.join(format!("{}.peaks", vod.id)), peaks.join(format!("{}.peaks", vod.id + 1)));
    write(&[&kept, &gone]);
    let _app = TestApp::on(folder);
    assert!(!gone.exists(), "a removed VOD's loudness was kept");
    assert!(kept.is_file(), "VOD {}'s loudness went", vod.id);
}

// The queues at a start.

/// The queues begin with what was left unfinished (lib.rs:218: "then the
/// queues, which begin with what was left unfinished"): a clip being made
/// when the app was killed is made (clips.rs `Cutter::run`), and a VOD
/// removed but not yet erased when it ended is erased (vods.rs `Sweeper`:
/// "at start whatever was left (stopped, or the app quit first)"; 0.7.1:
/// "Removed VODs are erased at once ... Quit ... before it's done, and the
/// rest is erased when VODscribe next starts").
#[test]
fn the_queues_begin_with_what_was_left_unfinished() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let removed = app.add_video_file("Day 4.mp4", 60.0);
    app.says(removed.id, 12.0, "The Eagle has landed");
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    halfway.reached();
    let folder = app.kill();
    // Taken out of the list as `delete_vod` does it; the app ended before
    // its erasure began.
    assert!(library_of(&folder).remove_entry(removed.id).unwrap(), "something still uses VOD {}", removed.id);
    let app = TestApp::on(folder);
    assert_eq!(app.made(clip.id).status, ClipStatus::Ready);
    eventually("the removed VOD erased", || app.db().vod(removed.id).unwrap().is_none());
}

// Quitting.

/// The jobs a quit stops record no failure, and carry on at the next start:
/// "The library's writes stop first, so the jobs whose processes stop don't
/// record that as a failure: they carry on at the next start as after any
/// quit" (lib.rs:644, `quit`). A clip's cut is stopped halfway: its ffmpeg
/// ends with the app.
#[test]
fn jobs_stopped_by_a_quit_record_no_failure_and_carry_on_at_the_next_start() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
    halfway.reached();
    let folder = app.quit();
    let left = library_of(&folder).clip(clip.id).unwrap().expect("the clip kept");
    assert!(
        matches!(left.status, ClipStatus::Waiting | ClipStatus::Cutting) && left.error.is_none(),
        "the quit recorded {:?} ({:?})",
        left.status,
        left.error
    );
    let app = TestApp::on(folder);
    let made = app.made(clip.id);
    assert_eq!((made.status, made.error), (ClipStatus::Ready, None));
}

/// A clip cut and not pasted goes as the app quits, its file with it: the
/// library it quit with has neither (lib.rs:674, `quit_library`: "a cut not
/// pasted goes with the copy"; 0.7.3: "Cut clips wait for a paste. Ctrl+X
/// ... keeps the clip and its file until you paste it, copy or cut again,
/// or quit").
#[test]
fn a_cut_not_pasted_goes_as_the_app_quits() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    // Ctrl+X, as the page does it (copyPaste.ts `cutOut`).
    app.invoke("copy_items", json!({ "items": [{ "kind": "clip", "id": clip.id }] })).unwrap();
    app.invoke("copied_text", json!({ "text": "copied" })).unwrap();
    app.invoke("cut_clips", json!({ "ids": [clip.id] })).unwrap();
    let file = PathBuf::from(app.clip(clip.id).expect("held by the cut").file_path.expect("its file"));
    let folder = app.quit();
    assert!(library_of(&folder).clip(clip.id).unwrap().is_none(), "the cut clip is still in the library");
    assert!(!file.exists(), "the cut clip's file was kept");
}

/// A file the app couldn't delete as it ran is tried again as it quits
/// (lib.rs:670, `files::delete_leftovers`; 0.4.1 Fixes: "On Windows, a clip
/// deleted while an edit was rendering from it kept its file"). A folder it
/// can't write to holds the file here, as Windows holds one open; it's let
/// go before the quit. The next start's try is the kit's own spec
/// (testkit/tests.rs, `a_file_left_undeleted_goes_at_the_next_start`).
#[cfg(unix)]
#[test]
fn files_left_to_delete_are_tried_again_as_the_app_quits() {
    use std::os::unix::fs::PermissionsExt;
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let clip = app.make_clip(vod.id, "0:10", "0:20");
    let file = PathBuf::from(clip.file_path.clone().unwrap());
    let clips = app.paths().clips();
    let mode = |m| std::fs::set_permissions(&clips, std::fs::Permissions::from_mode(m)).unwrap();
    mode(0o555);
    app.invoke("delete_clip", json!({ "id": clip.id })).unwrap();
    assert!(file.is_file(), "deleted as it ran after all");
    mode(0o755);
    let _folder = app.quit();
    assert!(!file.exists(), "left after the quit");
}
