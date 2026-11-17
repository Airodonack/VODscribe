//! Unfinished work carries on after a quit in the order it was asked
//! (CHANGELOG.md, 0.7.4: "Unfinished work resumes in the order asked."), each
//! queue finding its own work as before and only its place from the library
//! (core asked.rs, `Db::in_asked_order`; db.rs `MIGRATE_V47`). Each test asks
//! for work, quits (or is killed) with it unfinished, and starts again on
//! the same folder, the first job held as it begins, to see the line.

use serde_json::json;
use vodscribe_core::asked::Queue;
use vodscribe_core::db::Status;
use vodscribe_core::testkit::library::beside;
use vodscribe_core::testkit::{Gate, back_to, eventually};

use crate::testkit::{Folder, TestApp};

/// Starts the app on `folder`, its first cut held halfway, and gives the
/// clips' queue once that cut has begun: the one being made, then those
/// waiting, in order.
fn restarted_clips(folder: Folder) -> (TestApp, (Option<i64>, Vec<i64>)) {
    let again = Gate::new();
    let app = TestApp::on_with(folder, |fakes| fakes.ffmpeg.holds_at(0.5, &again));
    again.reached();
    let listed = app.state().clips.listed();
    (app, listed)
}

/// An app with a file of a minute, its first clip held halfway as it's
/// made, then `more` clips asked after it, waiting; and their ids, in the
/// order asked.
fn clips_waiting(more: usize) -> (TestApp, i64, Vec<i64>) {
    let app = TestApp::new();
    let vod = app.add_video_file("Day 3.mp4", 60.0);
    let halfway = Gate::new();
    app.ffmpeg.holds_at(0.5, &halfway);
    let first = app.add_clip(vod.id, "0:01", "0:02").unwrap().id;
    halfway.reached();
    let mut ids = vec![first];
    for n in 0..more {
        let from = format!("0:{:02}", 10 + n * 5);
        let to = format!("0:{:02}", 12 + n * 5);
        ids.push(app.add_clip(vod.id, &from, &to).unwrap().id);
    }
    (app, vod.id, ids)
}

/// Clips being made and waiting when the app was killed are made in the
/// order they were asked, the one being made first again (core asked.rs:
/// "the queues carry on after a quit in the order they had").
#[test]
fn clips_left_are_made_in_the_order_asked() {
    let (app, _, ids) = clips_waiting(2);
    let (_app, listed) = restarted_clips(app.kill());
    assert_eq!(listed, (Some(ids[0]), ids[1..].to_vec()));
}

/// Clips moved in the list of jobs keep the order they were moved into
/// across a quit (job_list.rs `reorder_jobs`: "and keeps that order for a
/// start after a quit").
#[test]
fn clips_moved_in_the_list_keep_their_new_order_across_a_quit() {
    let (app, _, ids) = clips_waiting(3);
    let moved = [ids[3], ids[1], ids[2]];
    app.invoke("reorder_jobs", json!({ "ids": moved })).unwrap();
    assert_eq!(app.state().clips.listed().1, moved, "moved");
    let (_app, listed) = restarted_clips(app.quit());
    assert_eq!(listed, (Some(ids[0]), moved.to_vec()));
}

/// An edit asked to render between two clips renders between them after a
/// quit, not after every clip as before (core asked.rs: "one row per job
/// asked for ... on one counter across every queue").
#[test]
fn an_edit_asked_between_two_clips_renders_between_them_after_a_quit() {
    let (app, vod, ids) = clips_waiting(1);
    let edit = app.db().add_edit("Best of").unwrap().id;
    app.db().add_piece(edit, vod, (10.0, 20.0), "The landing", None).unwrap();
    app.invoke("render_edit_now", json!({ "id": edit })).unwrap();
    let last = app.add_clip(vod, "0:40", "0:42").unwrap().id;
    assert_eq!(app.state().clips.listed().1, [ids[1], edit, last]);
    let (_app, listed) = restarted_clips(app.quit());
    assert_eq!(listed, (Some(ids[0]), vec![ids[1], edit, last]));
}

/// A clip dragged out of the window while it waits goes first, its file
/// waited for, and stays first after a quit, ahead of the one that was
/// being made too (clips.rs `Cutter::queue_first`: "Queues clip `id` first,
/// or moves it there if it's waiting: first in `db` too.").
#[test]
fn a_clip_put_first_for_a_drag_stays_first_after_a_quit() {
    let (app, _, ids) = clips_waiting(2);
    assert_eq!(app.drag_out(&[ids[2]]), Ok(json!(true)));
    assert_eq!(app.state().clips.listed().1, [ids[2], ids[1]], "first");
    let (_app, listed) = restarted_clips(app.kill());
    assert_eq!(listed, (Some(ids[2]), vec![ids[0], ids[1]]));
}

/// A library from before the order was kept (format 46, as 0.7.3's and
/// 0.7.4's first starts leave it) carries its clips on in the order they
/// always were, and each gets its place (core asked.rs `in_asked_order`:
/// "those with no place get one, in that order").
#[test]
fn a_library_from_before_the_order_carries_on_as_before_and_gets_places() {
    let (app, _, ids) = clips_waiting(2);
    let folder = app.kill();
    drop(back_to(&folder.paths().db(), 46));
    let (app, listed) = restarted_clips(folder);
    assert_eq!(listed, (Some(ids[0]), ids[1..].to_vec()));
    assert_eq!(app.db().asked(Queue::Render).unwrap(), ids);
}

/// A clip asked by a version that doesn't keep the order (0.7.3, gone back
/// to) has no place: it comes after those that have one, since it was asked
/// later (core asked.rs: "a job an older version asked … costs order at
/// most, never work").
#[test]
fn a_clip_an_older_version_asked_comes_after_those_with_a_place() {
    let (app, _, ids) = clips_waiting(2);
    let folder = app.kill();
    let conn = beside(&folder.paths().db());
    conn.execute("DELETE FROM asked WHERE queue = 'render' AND job = ?1", [ids[0]]).unwrap();
    drop(conn);
    let (_app, listed) = restarted_clips(folder);
    assert_eq!(listed, (Some(ids[1]), vec![ids[2], ids[0]]));
}

/// Kept videos waiting or downloading at a quit carry on in the order they
/// were asked, not the library's as before (CHANGELOG.md, 0.7.4:
/// "Unfinished work resumes in the order asked.").
#[test]
fn kept_videos_carry_on_in_the_order_asked_not_the_librarys() {
    let app = TestApp::new();
    let a = app.add_site_vod("youtube", "noa29qsTc4g", "Day 3", 60.0);
    let b = app.add_site_vod("twitch", "2901646294", "Day 4", 60.0);
    let begun = Gate::new();
    app.yt_dlp.holds(&begun);
    // Asked against the library's order.
    let library: Vec<i64> = {
        let db = app.db();
        for vod in [a.id, b.id] {
            db.set_video_wanted(vod, Some(720)).unwrap();
        }
        let order = db.wanted_videos().unwrap().into_iter().map(|(id, _)| id).collect();
        for vod in [a.id, b.id] {
            db.set_video_wanted(vod, None).unwrap();
        }
        order
    };
    let asked = [library[1], library[0]];
    for id in asked {
        app.invoke("keep_video", json!({ "id": id })).unwrap();
    }
    begun.reached();
    let folder = app.quit();
    let again = Gate::new();
    let app = TestApp::on_with(folder, |fakes| fakes.yt_dlp.holds(&again));
    again.reached();
    assert_eq!(app.invoke("video_downloads", json!({})).unwrap(), json!(asked));
}

/// Removed VODs left to erase at a quit are erased in the order they were
/// removed, not by their ids (CHANGELOG.md, 0.7.4: "Unfinished work
/// resumes in the order asked."). The first removed had erased its transcript, and is erased
/// whole at the next start without a part to hold at; the next is held at
/// its first part, the first gone by then.
#[test]
fn removed_vods_are_erased_in_the_order_removed() {
    let app = TestApp::new();
    let a = app.add_video_file("Day 3.mp4", 60.0);
    let b = app.add_video_file("Day 4.mp4", 60.0);
    for vod in [&a, &b] {
        app.says(vod.id, 12.0, "The Eagle has landed");
    }
    let between = Gate::new();
    app.holds_erasures(&between);
    for vod in [b.id, a.id] {
        app.invoke("delete_vod", json!({ "id": vod })).unwrap();
    }
    between.reached();
    let folder = app.kill();
    let again = Gate::new();
    folder.holds_erasures(&again);
    let app = TestApp::on(folder);
    again.reached();
    assert_eq!(app.state().sweeper.listed(), (Some(a.id), vec![]), "not erased after the one removed before it");
    assert!(app.db().vod(b.id).unwrap().is_none(), "the one removed first left");
}

/// A transcription resumed or retried goes to the back of the line, after
/// what was asked since, as anything asked now, within a session and after
/// a quit (CHANGELOG.md, 0.7.4: "Resume and Retry put a job last."; core
/// stretches.rs `requeue_request`, `next_stretch`).
#[test]
fn a_transcription_resumed_or_retried_goes_to_the_back() {
    let folder = Folder::new();
    folder.holds_worker_start(&Gate::new());
    let app = TestApp::on(folder);
    // From sites: with no audio here, each waits to download (`vods::requeue`).
    let sites =
        [("youtube", "noa29qsTc4g"), ("twitch", "2901646294"), ("kick", "01a1b43f-da70-76d4-88f2-d22fbea8ec73")];
    let vods: Vec<i64> = sites.iter().map(|&(site, id)| app.add_site_vod(site, id, "Day 3", 600.0).id).collect();
    let mut requests = Vec::new();
    for &vod in &vods {
        let asked = app.invoke("transcribe_stretch", json!({ "vodId": vod, "start": 0.0, "end": null })).unwrap();
        requests.push(asked["id"].as_i64().unwrap());
    }
    let next = |app: &TestApp| app.db().next_stretch(Status::Queued, false).unwrap().map(|(_, s)| s.id);
    assert_eq!(next(&app), Some(requests[0]));
    app.invoke("pause_vod", json!({ "id": vods[0] })).unwrap();
    app.invoke("resume_vod", json!({ "id": vods[0] })).unwrap();
    assert_eq!(next(&app), Some(requests[1]), "resumed ahead of what was asked since");
    app.db().set_stretch_status(requests[1], Status::Error, 0.0, Some("The site refused.")).unwrap();
    app.invoke("retry_vod", json!({ "id": vods[1] })).unwrap();
    assert_eq!(next(&app), Some(requests[2]), "retried ahead of what was asked since");
    let folder = app.quit();
    folder.holds_worker_start(&Gate::new());
    let app = TestApp::on(folder);
    let order: Vec<i64> = std::iter::from_fn(|| {
        let id = next(&app)?;
        app.db().set_stretch_status(id, Status::Done, 1.0, None).unwrap();
        Some(id)
    })
    .collect();
    assert_eq!(order, [requests[2], requests[0], requests[1]], "after a quit");
}

/// A whole received file a quit left before it was added is added in the
/// order the files arrived (receive.rs `take_up`: "In the order they
/// arrived (when each was last written), not the folder's.").
#[test]
fn files_received_whole_are_added_in_the_order_they_arrived() {
    let app = TestApp::new();
    let folder = app.quit();
    let incoming = folder.paths().received_incoming();
    std::fs::create_dir_all(&incoming).unwrap();
    let names = ["c.mp4", "a.mp4", "b.mp4"];
    for (n, name) in names.iter().enumerate() {
        let mut media = vodscribe_core::testkit::Media::video(60.0);
        let video = media.video.as_mut().unwrap();
        (video.width, video.height) = (64, 36);
        let path = incoming.join(name);
        media.write(&path);
        let at = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 + n as u64 * 60);
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(at).unwrap();
    }
    let app = TestApp::on(folder);
    eventually("all three added", || app.db().vods().unwrap().len() == 3);
    let mut vods = app.db().vods().unwrap();
    vods.sort_by_key(|v| v.id);
    let titles: Vec<String> = vods.into_iter().map(|v| v.info.title).collect();
    assert_eq!(titles, ["c", "a", "b"]);
}

/// Two drags in the list of jobs keep the order the second left, though
/// their writes to the library come after both, the first's last: what's
/// written is the queue as it is then (clips.rs `Cutter::keep_order`;
/// job_list.rs `reorder_jobs`: the queue is moved at once, the library
/// told after, on a thread that waits for its lock).
#[test]
fn two_drags_keep_the_order_the_second_left_whichever_write_lands_last() {
    let (app, _, ids) = clips_waiting(3);
    let clips = &app.state().clips;
    clips.reorder(&[ids[3], ids[1], ids[2]]);
    clips.reorder(&[ids[2], ids[3], ids[1]]);
    for _ in 0..2 {
        clips.keep_order(&app.db());
    }
    let order = clips.listed().1;
    assert_eq!(order, [ids[2], ids[3], ids[1]]);
    let (_app, listed) = restarted_clips(app.quit());
    assert_eq!(listed, (Some(ids[0]), order));
}

/// The footage an opened edit lacks that can't be fetched isn't asked for
/// again at every start: the failed fetch takes its edit's place away, and
/// it waits for the edit to ask again, opened, as a failed video download
/// does (footage.rs, `Footage`'s `askers`: "whose places
/// (`Db::keep_place`) a failed fetch takes away"; `Footage::run`).
#[test]
fn footage_that_cant_be_fetched_loses_its_place() {
    let app = TestApp::new();
    let vod = app.add_site_vod("twitch", "2901646294", "Day 3", 600.0);
    let edit = app.db().add_edit("Best of").unwrap().id;
    app.db().add_piece(edit, vod.id, (60.0, 90.0), "The landing", None).unwrap();
    app.yt_dlp.refuses(&vod.info.url);
    app.with_screen();
    let _ = app.invoke("player_open_edit", json!({ "id": edit, "at": 0.0, "play": false }));
    eventually("its fetch failed", || {
        app.heard("footage").iter().any(|e| e["vod_id"] == vod.id && e["error"].is_string())
    });
    assert!(!app.db().is_asked(Queue::Footage, edit).unwrap(), "asked for again at the next start");
}
