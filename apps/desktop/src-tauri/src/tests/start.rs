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
/// go at the start: part files, whose drop is gone, the folders an archive
/// manager saved into first, and a file received whole that can't be added
/// (not a video VODscribe can read), as its drop's add would have. A VOD's
/// own received file stays, in its place or still where it arrived, as a
/// crash before the move leaves it (lib.rs:243; receive.rs `tidy`,
/// `take_up`; core db.rs `add_received`: "till it's moved there it plays
/// where it arrived"). The start's settle moves that one to its place
/// first (`library::settle`). One received whole that can be added is
/// (`a_file_received_whole_and_not_yet_added_is_added_at_the_next_start`).
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
    assert!(!whole.exists(), "a file received whole that can't be added was kept");
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

/// A video download a quit stopped carries on at the next start, from the
/// part yt-dlp left, and so does one still waiting its turn (owner,
/// 2026-11-11, plans/sections-074.md 2C spec 9: "Yes all should restart";
/// core db.rs `MIGRATE_V45`: "A quit stops the library's writes first, so
/// it stays, and the next start carries the download on at that height";
/// videos.rs `download`: "resuming any part already there").
#[test]
fn a_video_download_a_quit_stopped_carries_on_at_the_next_start() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "Day 3", 60.0);
    let next = app.add_site_vod("twitch", "2901646294", "Day 4", 60.0);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    app.invoke("keep_video", json!({ "id": vod.id, "height": 480 })).unwrap();
    begun.reached();
    app.invoke("keep_video", json!({ "id": next.id })).unwrap();
    let folder = app.quit();
    let parts = vodscribe_core::fetch::part_files(&folder.paths().source_video_stem(vod.id));
    assert!(!parts.is_empty(), "the quit left no part of the download");
    let asked = library_of(&folder).wanted_videos().unwrap();
    assert!(asked.contains(&(vod.id, 480)), "the height asked wasn't kept: {asked:?}");
    let app = TestApp::on(folder);
    for id in [vod.id, next.id] {
        eventually("its video kept", || app.db().vod(id).unwrap().unwrap().media);
    }
    let kept = app.db().media_path(vod.id).unwrap().expect("its video");
    assert!(Path::new(&kept).is_file(), "its video isn't where the library says");
    assert!(parts.iter().all(|(part, _)| !part.exists()), "a part left after the download");
    assert_eq!(app.db().wanted_videos().unwrap(), [], "kept, and still asked for");
}

/// Whether the video queue took VOD `id` within a second: waiting, under
/// way, or over (the page heard it).
fn video_queue_took(app: &TestApp, id: i64) -> bool {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while std::time::Instant::now() < until {
        if app.state().videos.wanted(id) || app.heard("video-progress").iter().any(|e| e["id"] == id) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

/// A video download that failed isn't started again at the next start: it
/// ends as it says, and only "Keep video" asks again (core db.rs
/// `MIGRATE_V45`: "cleared when it's done, stopped or failed"; review of
/// queues-resume, 2026-11-11, F2). yt-dlp leaves its part, as it does.
#[test]
fn a_video_download_that_failed_is_not_started_again_at_the_next_start() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "Day 3", 60.0);
    app.yt_dlp.refuses(&vod.info.url);
    app.invoke("keep_video", json!({ "id": vod.id })).unwrap();
    eventually("its failure told", || {
        app.heard("video-progress").iter().any(|e| e["id"] == vod.id && e["error"].is_string())
    });
    let app = TestApp::on(app.quit());
    assert!(!video_queue_took(&app, vod.id), "a download that failed was started again, unasked");
}

/// A video asked to download whose VOD's whole transcription will download
/// it into the same file, from a site without its own player, is left to
/// that transcription at the next start (videos.rs `left_unfinished`: "Not
/// one that transcribing all of it will download into the same file"; review
/// of queues-resume, 2026-11-11, F1). The worker's start sets its request
/// back to queued (`Db::requeue_interrupted`); held, it comes after the
/// video queue's start, as on a busy start.
#[test]
fn a_video_a_whole_transcription_downloads_is_left_to_it_at_the_next_start() {
    let app = TestApp::new();
    let vod = app.add_site_vod("rumble", "r", "Day 3", 60.0);
    let folder = app.quit();
    {
        let db = library_of(&folder);
        db.set_video_wanted(vod.id, Some(720)).unwrap();
        let whole = db.request(vod.id, 0.0, None).unwrap().id;
        db.set_stretch_status(whole, vodscribe_core::db::Status::Downloading, 0.1, None).unwrap();
    }
    let paths = folder.paths();
    std::fs::create_dir_all(paths.media()).unwrap();
    std::fs::write(paths.media().join(format!("{}.mp4.part", vod.id)), b"half").unwrap();
    let worker = Gate::new();
    folder.holds_worker_start(&worker);
    let app = TestApp::on(folder);
    assert!(!video_queue_took(&app, vod.id), "the video queue downloads it too, beside its transcription");
}

/// A video asked to download beside a whole transcription that failed (its
/// request stays, in error) is carried on at the next start: that
/// transcription downloads nothing (videos.rs `transcribing_all`: "One
/// failed or paused downloads nothing"; re-review of queues-resume,
/// 2026-11-11, R1).
#[test]
fn a_video_asked_beside_a_failed_whole_transcription_is_carried_on_at_the_next_start() {
    let app = TestApp::new();
    let vod = app.add_site_vod("rumble", "r", "Day 3", 60.0);
    let folder = app.quit();
    {
        let db = library_of(&folder);
        let whole = db.request(vod.id, 0.0, None).unwrap().id;
        let refused = Some("The site refused the download.");
        db.set_stretch_status(whole, vodscribe_core::db::Status::Error, 0.0, refused).unwrap();
    }
    let app = TestApp::on(folder);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    app.invoke("keep_video", json!({ "id": vod.id, "height": 480 })).unwrap();
    begun.reached();
    let folder = app.quit();
    assert_eq!(library_of(&folder).wanted_videos().unwrap(), [(vod.id, 480)], "the quit kept the ask");
    let app = TestApp::on(folder);
    assert!(video_queue_took(&app, vod.id), "the asked video download wasn't carried on");
}

/// A video download stopped by removing its VOD isn't asked any more, though
/// the app quits before the stopped download ends: the VOD, added again
/// (kept for an edit's piece, its row listed again), doesn't download its
/// video unasked at the next start (videos.rs `stop`: "No longer asked,
/// waiting or under way"; re-review of queues-resume, 2026-11-11, R2).
#[test]
fn a_video_download_stopped_by_removing_its_vod_is_not_asked_after_a_quit() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "Day 3", 60.0);
    let edit = app.db().add_edit("Best of").unwrap().id;
    app.db().add_piece(edit, vod.id, (10.0, 20.0), "The landing", None).unwrap();
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    app.invoke("keep_video", json!({ "id": vod.id })).unwrap();
    begun.reached();
    app.invoke("delete_vod", json!({ "id": vod.id })).unwrap();
    let app = TestApp::on(app.quit());
    assert!(!video_queue_took(&app, vod.id), "removed, and carried on");
    // Added again, as adding its link again does (`add_to_library`).
    let info = app.db().vod(vod.id).unwrap().unwrap().info;
    let (again, _) = app.db().add_vod(&info).unwrap();
    assert_eq!(again.id, vod.id, "listed again under its own row");
    let app = TestApp::on(app.quit());
    assert!(!video_queue_took(&app, vod.id), "a download stopped by removing its VOD was started again, unasked");
}

/// A removed VOD's video download isn't carried on at the next start: the
/// start erases it (core db.rs `wanted_videos`: "only those in its list,
/// and not gone"; vods.rs `Sweeper`; review of queues-resume, 2026-11-11,
/// F3).
#[test]
fn a_removed_vods_video_download_is_not_carried_on_at_the_next_start() {
    let app = TestApp::new();
    let vod = app.add_site_vod("youtube", "noa29qsTc4g", "Day 3", 60.0);
    let folder = app.quit();
    {
        let db = library_of(&folder);
        db.set_video_wanted(vod.id, Some(720)).unwrap();
        assert!(db.remove_entry(vod.id).unwrap(), "still used");
    }
    let paths = folder.paths();
    std::fs::create_dir_all(paths.media()).unwrap();
    std::fs::write(paths.media().join(format!("{}.mp4.part", vod.id)), b"half").unwrap();
    let app = TestApp::on(folder);
    assert!(!video_queue_took(&app, vod.id), "the download of a removed VOD was carried on");
    eventually("the removed VOD erased", || app.db().vod(vod.id).unwrap().is_none());
}

/// An edit a quit stopped rendering is rendered at the next start (owner,
/// 2026-11-11, plans/sections-074.md: resuming edit renders a quit
/// interrupted; core edits.rs `unfinished_renders`: "Edits left rendering
/// ... rendered again at the start").
#[test]
fn an_edit_a_quit_stopped_rendering_is_rendered_at_the_next_start() {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let other = app.add_video_file("Day 4.mp4", 60.0);
    let edit = app.db().add_edit("Best of").unwrap().id;
    app.db().add_piece(edit, vod.id, (10.0, 20.0), "The landing", None).unwrap();
    app.db().add_piece(edit, other.id, (5.0, 8.0), "The crowd", None).unwrap();
    let ended = Gate::new();
    app.holds_cut_ends(&ended);
    app.invoke("render_edit_now", json!({ "id": edit })).unwrap();
    ended.reached();
    let app = TestApp::on(app.quit());
    let encoding = crate::edits::made_now(app.handle());
    eventually("the edit rendered", || {
        app.db().edit(edit, encoding).unwrap().is_some_and(|e| e.status == ClipStatus::Ready)
    });
}

/// The footage an edit's piece needs, a quit stopped downloading, is
/// downloaded at the next start. A section can't be carried on (ffmpeg
/// writes it): it's downloaded again, its files a quit left gone first
/// (owner, 2026-11-11, plans/sections-074.md 2C spec 9; lib.rs, the
/// comment at `footage::tidy_work`).
#[test]
fn footage_a_quit_stopped_downloading_is_downloaded_at_the_next_start() {
    let app = TestApp::new();
    let vod = app.add_site_vod("twitch", "2901646294", "Day 3", 600.0);
    let edit = app.db().add_edit("Best of").unwrap().id;
    app.db().add_piece(edit, vod.id, (60.0, 90.0), "The landing", None).unwrap();
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    app.with_screen();
    let _ = app.invoke("player_open_edit", json!({ "id": edit, "at": 0.0, "play": false }));
    begun.reached();
    let folder = app.quit();
    let work = folder.paths().work();
    let left: Vec<PathBuf> = std::fs::read_dir(&work)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().ends_with(".part"))
        .collect();
    assert!(!left.is_empty(), "the quit left no part of the section");
    let app = TestApp::on(folder);
    eventually("the piece's footage here", || {
        let db = app.db();
        crate::footage::find(app.handle(), &db, vod.id, 60.0, 90.0).is_some()
    });
    assert!(left.iter().all(|p| !p.exists()), "a part the quit left is still there");
}

/// A file received whole from another app's drop, which a quit stopped
/// before it was added, is added at the next start, as its drop would have
/// (owner, 2026-11-11, plans/sections-074.md 2C spec 9; receive.rs: each is
/// "renamed once whole and added as a file (`vods::add_received`), which
/// moves it to `received/<its VOD's id>`"). One still being received can't
/// be: what was dropping it is gone, so its part goes
/// (`at_start_files_a_quit_left_being_received_go`).
#[test]
fn a_file_received_whole_and_not_yet_added_is_added_at_the_next_start() {
    let app = TestApp::new();
    let folder = app.quit();
    let arrived = folder.paths().received_incoming().join("Day 6.mp4");
    std::fs::create_dir_all(arrived.parent().unwrap()).unwrap();
    let mut media = vodscribe_core::testkit::Media::video(60.0);
    let video = media.video.as_mut().unwrap();
    (video.width, video.height) = (64, 36);
    media.write(&arrived);
    let app = TestApp::on(folder);
    let vods = app.db().vods().unwrap();
    let added = vods.iter().find(|v| v.info.title == "Day 6").expect("not added");
    let file = added.source_path.clone().expect("its file");
    assert_eq!(Path::new(&file), app.paths().source_received(added.id, "mp4"));
    assert!(Path::new(&file).is_file(), "its file isn't where the library says");
    assert!(!arrived.exists(), "left where it arrived");
    // Added as a drop adds it, its picture from its storyboard.
    eventually("its picture", || app.db().vod(added.id).unwrap().unwrap().thumb.is_some());
}

/// A storyboard a quit stopped partway is made on at the next start, from
/// what it had (owner, 2026-11-11, plans/sections-074.md 2C spec 9;
/// storyboards.rs `sweep`: "asks for those missing, of files changed since,
/// or unfinished"; crates/player storyboard.rs `make`: "Makes (or carries
/// on making)").
#[test]
fn a_storyboard_a_quit_stopped_partway_is_made_at_the_next_start() {
    use std::sync::atomic::AtomicBool;
    use vodscribe_player::storyboard::{self, Storyboard};

    let app = TestApp::new();
    let mut source = vodscribe_core::testkit::Media::video(60.0);
    let video = source.video.as_mut().unwrap();
    (video.width, video.height) = (64, 36);
    let vod = app.add_file("Day 3.mp4", &source);
    let file = PathBuf::from(vod.source_path.clone().unwrap());
    let board = app.paths().source_storyboard(vod.id, &file);
    std::fs::create_dir_all(board.parent().unwrap()).unwrap();
    // Its first batch made, then stopped, as a quit stops it.
    let stopped = storyboard::make(app.ffmpeg.path(), &file, &board, &AtomicBool::new(false), |_| false);
    assert!(stopped.is_err(), "made whole before the quit");
    let had = Storyboard::open(&board).unwrap().made();
    assert!(had > 0, "nothing of it made before the quit");
    let _app = TestApp::on(app.quit());
    eventually("its storyboard made", || Storyboard::open(&board).is_ok_and(|b| b.complete()));
    assert!(Storyboard::open(&board).unwrap().made() > had);
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
