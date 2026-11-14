//! The specs of `db.rs`: the helpers they share, and the files in `tests/` that hold the rest.

use super::*;
use crate::transcript::{Segment, Segmenter};

mod chat_store;
mod library;
mod migrations_early;
mod migrations_late;
mod search;
mod search_scripts;

/// A job a part at a time (an erasure) lets a thread waiting for the
/// library in after the part it's on, not after the whole job.
#[test]
fn a_job_in_parts_lets_a_waiting_thread_in() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    const MESSAGES: usize = 20_000;
    let dir = crate::TempDir::new("give-way");
    let db = Db::open(&dir.join("library.db")).unwrap();
    let vod = db.add_vod(&crate::edits::tests::info("x")).unwrap().0.id;
    let db = Arc::new(Mutex::new(db));
    for round in 0..3 {
        {
            let db = db.lock().unwrap();
            db.conn.execute_batch("BEGIN").unwrap();
            for i in 0..MESSAGES {
                db.conn
                    .execute(
                        "INSERT INTO chat (vod_id, time, author, text, parts) VALUES (?1, ?2, 'viewer', ?3, '[]')",
                        params![vod, i as f64, format!("message {i}")],
                    )
                    .unwrap();
            }
            db.conn.execute_batch("COMMIT").unwrap();
        }
        let begun = Arc::new(AtomicBool::new(false));
        let job = {
            let (db, begun) = (db.clone(), begun.clone());
            std::thread::spawn(move || {
                loop {
                    let held = db.lock().unwrap();
                    begun.store(true, Ordering::SeqCst);
                    if held.delete_some_of(vod, 1000).unwrap() == 0 {
                        break;
                    }
                    drop(held);
                    give_way();
                }
            })
        };
        while !begun.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
        let left = db.lock().unwrap().erase_left(vod).unwrap();
        job.join().unwrap();
        assert!(left > MESSAGES / 2, "round {round}: let in with {left} of {MESSAGES} left");
    }
}

#[test]
fn marks_are_put_back_as_they_were() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&crate::edits::tests::info("x")).unwrap().0.id;
    let cut = db.add_mark(vod, "cut", 5.0, None).unwrap();
    db.add_mark(vod, "chapter", 1.0, Some("Intro")).unwrap();
    let before = db.mark_states(vod).unwrap();
    assert_eq!(db.mark_doc(cut.id).unwrap(), Some(MarkDoc::Vod(vod)));

    db.move_mark(cut.id, 7.0).unwrap();
    db.add_mark(vod, "highlight", 3.0, Some("note")).unwrap();
    db.restore_marks(vod, &before).unwrap();
    assert_eq!(db.mark_states(vod).unwrap(), before);
}

fn message(time: f64, author: &str, text: &str) -> ChatMessage {
    ChatMessage {
        time,
        author: author.into(),
        color: Some("#ff0000".into()),
        parts: vec![ChatPart::Text { text: text.into() }],
        kind: Kind::Message,
        amount: None,
        event_time: None,
        at: None,
    }
}

fn info(id: &str) -> VodInfo {
    VodInfo {
        platform: "youtube".into(),
        platform_id: id.into(),
        url: format!("https://youtu.be/{id}"),
        title: "t".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(60.0),
        thumbnail: None,
        published: None,
        started_at: None,
    }
}

/// Files received from drops arrive under the name their app gave, which
/// is free again once the first is renamed for its VOD: a second file of
/// that name is a VOD of its own, not the first one found again; nor is
/// it a 0.7.1 library's received file, whose row names where it arrived.
#[test]
fn a_second_file_received_under_the_same_name_is_a_new_vod() {
    let root = crate::TempDir::new("received-twice");
    let paths = crate::Paths::new(root.join("lib"));
    paths.ensure().unwrap();
    std::fs::create_dir_all(paths.received()).unwrap();
    let db = Db::open_in(&paths).unwrap();
    let arrived = shown(&paths.received().join("video.mp4"));
    let mut file = info("x");
    file.platform = "local".into();
    file.platform_id = arrived.clone();
    file.url = arrived.clone();
    // As 0.7.1 added one, moved since by `library::settle`.
    let (old, _) = db.add_vod(&file).unwrap();
    db.set_received(old.id, &shown(&paths.source_received(old.id, "mp4"))).unwrap();

    let first = db.add_received(&file).unwrap();
    assert_ne!(first.id, old.id, "found as the 0.7.1 library's VOD");
    assert_eq!(first.source_path.as_deref(), Some(arrived.as_str()), "played where it arrived till it's moved");
    let to = shown(&paths.source_received(first.id, "mp4"));
    assert_eq!(
        (first.info.platform_id.clone(), first.info.url.clone()),
        (format!("received:{}", first.id), to.clone())
    );
    std::fs::write(&to, b"first").unwrap();
    db.set_received(first.id, &to).unwrap();
    assert_eq!(db.vod(first.id).unwrap().unwrap().source_path, Some(to));
    // Another drop, another file, the same name.
    let second = db.add_received(&file).unwrap();
    assert!(second.id != first.id && second.id != old.id, "found as VOD {} (the first file's)", second.id);
}

fn words(text: &str, start: f64) -> Vec<Word> {
    text.split(' ')
        .enumerate()
        .map(|(i, t)| Word { text: t.into(), start: start + i as f64, end: start + i as f64 + 0.5 })
        .collect()
}

/// Adding files while the worker writes on its own connection: a
/// transaction that reads, then writes after another connection's
/// commit, waits its turn instead of failing "database is locked".
#[test]
fn a_transaction_that_reads_then_writes_waits_for_other_writers() {
    let dir = crate::TempDir::new("read-then-write");
    let path = dir.join("library.sqlite");
    let (adder, worker) = (Db::open(&path).unwrap(), Db::open(&path).unwrap());
    let (vod, _) = worker.add_vod(&info("a")).unwrap();
    let stretch = worker.request(vod.id, 0.0, None).unwrap();
    // The worker's connection says when it finds the library taken, and
    // tries again, as its busy timeout would.
    static WAITED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    fn waits(tries: i32) -> bool {
        WAITED.store(true, std::sync::atomic::Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(1));
        tries < 5000
    }
    worker.conn.busy_handler(Some(waits)).unwrap();
    let mut writing = None;
    adder
        .atomically(|db| {
            db.vod(vod.id)?;
            // The worker finishes a VOD between the read and the write.
            let finishing = writing.insert(std::thread::spawn(move || worker.finish_stretch(stretch.id)));
            crate::testkit::eventually("the worker waits its turn, or has written", || {
                WAITED.load(std::sync::atomic::Ordering::SeqCst) || finishing.is_finished()
            });
            db.add_vod(&info("b"))
        })
        .unwrap();
    writing.unwrap().join().unwrap().unwrap();
    assert_eq!(adder.vod(vod.id).unwrap().unwrap().status, Status::Done);
}

#[test]
fn a_vods_start_by_its_site_is_kept_till_its_chat_says_better() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let mut info = crate::edits::tests::info("abc");
    info.started_at = Some("2026-09-28T21:29:40Z".into());
    let (vod, _) = db.add_vod(&info).unwrap();
    assert_eq!(vod.info.started_at.as_deref(), Some("2026-09-28T21:29:40Z"));
    // A file's time doesn't replace the site's.
    db.set_start_if_unknown(vod.id, "2026-09-28T21:00:00Z").unwrap();
    assert_eq!(db.vod(vod.id).unwrap().unwrap().info.started_at.as_deref(), Some("2026-09-28T21:29:40Z"));
    // The chat's does, to the nearest second; and a later chat's replaces that.
    db.set_chat_start(vod.id, 1790631004.5).unwrap();
    assert_eq!(db.vod(vod.id).unwrap().unwrap().info.started_at.as_deref(), Some("2026-09-28T21:30:05Z"));
    db.set_chat_start(vod.id, 1790631001.2).unwrap();
    assert_eq!(db.vod(vod.id).unwrap().unwrap().info.started_at.as_deref(), Some("2026-09-28T21:30:01Z"));
    // And on a VOD with nothing yet, a file's time stands, read in any zone and kept in UTC.
    let (other, _) = db.add_vod(&crate::edits::tests::info("xyz")).unwrap();
    db.set_start_if_unknown(other.id, "2026-09-28T23:30:05.6+02:00").unwrap();
    assert_eq!(db.vod(other.id).unwrap().unwrap().info.started_at.as_deref(), Some("2026-09-28T21:30:06Z"));
}

#[test]
fn utc_times_read_back() {
    assert_eq!(utc_time(0.0), "1970-01-01T00:00:00Z");
    assert_eq!(utc_time(1790631005.0), "2026-09-28T21:30:05Z");
    assert_eq!(utc_time(1709251199.6), "2024-03-01T00:00:00Z", "rounded, over a leap day's end");
    for t in [0.0, 951782400.0, 1790631005.0] {
        assert_eq!(crate::chat::parse_time(&utc_time(t)), Some(t));
    }
}

/// As a VOD removed from the library is pasted again while its erasure is
/// under way (desktop vods.rs `add_to_library` finishes the erasure first
/// where it can): the gone row goes with all that was left of it, and the
/// VOD is added anew, under an id never given before though the old one
/// was the newest. One removed but not yet marked gone is listed again,
/// the same row as it was.
#[test]
fn a_vod_added_back_while_being_erased_is_added_anew_under_a_new_id() {
    let mut db = Db::open(Path::new(":memory:")).unwrap();
    let older = db.add_vod(&info("older")).unwrap().0.id;
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let chat: Vec<_> = (0..5).map(|i| message(i as f64, "a", "hi")).collect();
    db.add_chat(vod, &chat, &[]).unwrap();
    db.insert_segments(vod, &[Segment { start: 0.0, end: 1.0, text: "hi".into(), words: Vec::new() }]).unwrap();
    assert!(db.remove_entry(vod).unwrap(), "used by nothing now");
    db.set_gone(vod).unwrap();
    assert_eq!(db.delete_some_of(vod, 2).unwrap(), 2, "a part of its chat erased");
    assert_eq!(db.erase_left(vod).unwrap(), 4, "three messages and a segment left");
    let (again, created) = db.add_vod(&info("x")).unwrap();
    assert!(created && again.listed && !db.is_gone(again.id).unwrap());
    assert!(again.id > vod, "the gone VOD's id {vod} given again: {}", again.id);
    assert!(db.vod(vod).unwrap().is_none(), "the gone row is still there");
    assert_eq!(db.erase_left(vod).unwrap(), 0, "what was left of it went with its row");
    assert_eq!(db.erase_left(again.id).unwrap(), 0);
    assert!(db.vod(older).unwrap().is_some_and(|v| v.listed), "another VOD went too");
    // Removed, its erasure not begun.
    assert!(db.remove_entry(again.id).unwrap());
    let (back, created) = db.add_vod(&info("x")).unwrap();
    assert_eq!((back.id, created, back.listed), (again.id, false, true));
}

#[test]
fn audio_alone_by_file_type() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let vod = |name: &str| db.add_vod(&info(name)).unwrap().0.id;
    let (song, film, kept, nothing) = (vod("a"), vod("b"), vod("c"), vod("d"));
    db.set_source_path(song, "/music/song.MP3").unwrap();
    db.set_source_path(film, "/videos/film.mkv").unwrap();
    let video = |ext| db.files().source_video(kept, ext).to_string_lossy().into_owned();
    db.set_media_path(kept, Some(&video("m4a"))).unwrap();
    let audio_only = |id| db.vod(id).unwrap().unwrap().audio_only;
    assert!(audio_only(song) && audio_only(kept));
    assert!(!audio_only(film) && !audio_only(nothing));
    db.set_media_path(kept, Some(&video("mp4"))).unwrap();
    assert!(!audio_only(kept));
    assert!(db.set_media_path(kept, Some("/data/media/3.mp4")).is_err(), "only where it goes");
}

#[test]
fn marks_come_in_time_order_and_go_with_their_vod() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let b = db.add_mark(vod, "cut", 20.0, None).unwrap();
    let a = db.add_mark(vod, "cut", 10.0, None).unwrap();
    let starts = |db: &Db| db.marks(vod).unwrap().into_iter().map(|m| m.start).collect::<Vec<_>>();
    assert_eq!(starts(&db), [10.0, 20.0]);
    assert!(db.move_mark(a.id, 30.0).unwrap());
    assert_eq!(starts(&db), [20.0, 30.0]);
    db.delete_mark(b.id).unwrap();
    assert_eq!(starts(&db), [30.0]);
    assert!(!db.move_mark(b.id, 1.0).unwrap(), "gone");
    let chapter = db.add_mark(vod, "chapter", 5.0, Some("Intro")).unwrap();
    assert!(db.set_mark_text(chapter.id, Some("Lunch")).unwrap());
    let marks = db.marks(vod).unwrap();
    assert_eq!((marks[0].kind.as_str(), marks[0].text.as_deref()), ("chapter", Some("Lunch")));
    assert!(db.set_mark_text(chapter.id, None).unwrap());
    assert_eq!(db.marks(vod).unwrap()[0].text, None);
    assert!(!db.set_mark_text(b.id, None).unwrap(), "gone");
    db.delete_vod(vod).unwrap();
    assert!(db.marks(vod).unwrap().is_empty());
}

/// What a clip's file was made with is kept with it (`set_made_as`),
/// for the page to make it again when that changes; unknown till then.
#[test]
fn a_clip_keeps_what_its_file_was_made_with() {
    use crate::settings::{ClipCodec, ClipQuality};
    let db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let clip = db.add_clip(vod, 1.0, 2.0, "c", true).unwrap();
    assert_eq!(clip.encoding, None);
    let h265 = ClipEncoding { codec: ClipCodec::H265, quality: ClipQuality::High };
    db.set_made_as(clip.id, Some(h265)).unwrap();
    assert_eq!(db.clip(clip.id).unwrap().unwrap().encoding, Some(h265));
    assert_eq!(db.made_as(clip.id).unwrap(), Some(h265));
    db.set_made_as(clip.id, None).unwrap();
    assert_eq!(db.clip(clip.id).unwrap().unwrap().encoding, None);
    // As the page reads it.
    let json = serde_json::to_value(Some(h265)).unwrap();
    assert_eq!(json, serde_json::json!({ "codec": "h265", "quality": "high" }));
}

#[test]
fn clips_keep_their_order_and_follow_their_range() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let a = db.add_clip(vod, 10.0, 20.0, "first", true).unwrap();
    let b = db.add_clip(vod, 5.0, 8.0, "second", false).unwrap();
    assert_eq!(a.status, ClipStatus::Waiting);
    assert!(!b.shown, "dragged out only");
    db.show_clip(b.id).unwrap();
    assert!(db.clip(b.id).unwrap().unwrap().shown);
    let titles = |db: &Db| db.clips(vod).unwrap().into_iter().map(|c| c.title).collect::<Vec<_>>();
    assert_eq!(titles(&db), ["first", "second"], "in the order they were made");
    assert_eq!(db.unfinished_clips().unwrap().len(), 2);

    // A cut that finishes after its range moved isn't recorded.
    db.set_clip_range(a.id, 11.0, 20.0).unwrap();
    let file = db.files().clip_file(a.id, "mp4").to_string_lossy().into_owned();
    assert!(!db.set_clip_file(a.id, 10.0, 20.0, ClipStatus::Ready, Some(&file), None).unwrap());
    assert!(db.set_clip_file(a.id, 11.0, 20.0, ClipStatus::Ready, Some(&file), None).unwrap());
    let a = db.clip(a.id).unwrap().unwrap();
    assert_eq!((a.status, a.file_path.as_deref()), (ClipStatus::Ready, Some(&*file)));
    // Moved again: made again, at the same file.
    db.set_clip_range(a.id, 12.0, 20.0).unwrap();
    let a = db.clip(a.id).unwrap().unwrap();
    assert_eq!((a.status, a.file_path.as_deref()), (ClipStatus::Waiting, Some(&*file)));
    db.rename_clip(b.id, "renamed").unwrap();
    db.delete_clip(a.id).unwrap();
    assert_eq!(titles(&db), ["renamed"]);
    // One made only to drag out is forgotten later; its file was handed out.
    let c = db.add_clip(vod, 1.0, 2.0, "dragged", false).unwrap();
    assert!(!db.clip_handed_out(c.id).unwrap());
    db.hand_out_clip(c.id).unwrap();
    assert!(db.clip_handed_out(c.id).unwrap());
    assert_eq!(db.forget_hidden_clips().unwrap(), [(c.id, None, true)]);
    assert_eq!(titles(&db), ["renamed"]);
    // They go with their VOD.
    db.delete_vod(vod).unwrap();
    assert!(db.clip(b.id).unwrap().is_none());
}

#[test]
fn lines_all_or_between() {
    let mut db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let line = |start: f64, text: &str| Segment { start, end: start + 1.0, text: text.into(), words: Vec::new() };
    db.insert_segments(vod, &[line(0.0, "one"), line(2.5, "two")]).unwrap();
    db.insert_segments(vod, &[line(4.0, "three")]).unwrap();
    let texts = db.lines(vod).unwrap().into_iter().map(|l| l.text).collect::<Vec<_>>();
    assert_eq!(texts, ["one", "two", "three"]);
    let between = |from, to| db.lines_between(vod, from, to).unwrap().into_iter().map(|l| l.text).collect::<Vec<_>>();
    assert_eq!(between(2.5, Some(4.0)), ["two"], "from its start, to before its end");
    assert_eq!(between(1.0, None), ["two", "three"], "on to the end");
}

#[test]
fn status_names_round_trip() {
    use Status::*;
    for status in [Queued, Downloading, Ready, Transcribing, Paused, Done, Error, Status::None] {
        assert_eq!(Status::parse(status.as_str()), status);
    }
}

#[test]
fn keeps_the_library_order() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let a = db.add_vod(&info("a")).unwrap().0.id;
    let b = db.add_vod(&info("b")).unwrap().0.id;
    let ids = |db: &Db| db.vods().unwrap().iter().map(|v| v.id).collect::<Vec<_>>();
    assert_eq!(ids(&db), [b, a], "newest first");
    db.reorder_library(&[crate::edits::LibraryItem::Vod(a), crate::edits::LibraryItem::Vod(b)]).unwrap();
    assert_eq!(ids(&db), [a, b]);
    let c = db.add_vod(&info("c")).unwrap().0.id;
    assert_eq!(ids(&db), [c, a, b], "added on top");
}

#[test]
fn interrupted_jobs_return_to_their_queues() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let [down, trans, ready, done] = ["d", "t", "r", "x"].map(|id| db.add_vod(&info(id)).unwrap().0.id);
    let ask = |id, status, progress| {
        let stretch = db.request(id, 0.0, None).unwrap().id;
        db.set_stretch_status(stretch, status, progress, None).unwrap();
    };
    ask(down, Status::Downloading, 0.4);
    ask(trans, Status::Transcribing, 0.6);
    ask(ready, Status::Ready, 0.0);
    db.finish_stretch(db.request(done, 0.0, None).unwrap().id).unwrap();
    let status = |id| db.vod(id).unwrap().unwrap().status;

    // A quit mid-stage: each VOD goes back to wait for the stage it was in.
    db.requeue_interrupted().unwrap();
    assert_eq!(
        [status(down), status(trans), status(ready), status(done)],
        [Status::Queued, Status::Ready, Status::Ready, Status::Done]
    );

    // "Free up space" deleted the audio: VODs waiting to transcribe must download again.
    db.set_audio_path(ready, Some(&db.files().source_audio(ready, "m4a").to_string_lossy())).unwrap();
    db.forget_downloads().unwrap();
    assert_eq!(db.audio_path(ready).unwrap(), None);
    assert_eq!([status(trans), status(ready), status(done)], [Status::Queued, Status::Queued, Status::Done]);
}

#[test]
fn local_files_queue_separately() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let [link, file_a, file_b] = ["l", "a", "b"].map(|id| db.add_vod(&info(id)).unwrap().0.id);
    db.set_source_path(file_a, "/videos/a.mp4").unwrap();
    db.set_source_path(file_b, "/videos/b.mp4").unwrap();
    let [_, a, b] = [link, file_a, file_b].map(|id| db.request(id, 0.0, None).unwrap().id);
    let next = |local| db.next_stretch(Status::Queued, local).unwrap().map(|(v, _)| v.id);
    // In the order asked, as for links, but never a link.
    assert_eq!(next(false), Some(link));
    assert_eq!(next(true), Some(file_a));
    db.finish_stretch(a).unwrap();
    assert_eq!(next(true), Some(file_b));
    db.finish_stretch(b).unwrap();
    assert_eq!(next(true), None);
}

#[test]
fn reads_dont_wait_for_writes() {
    let dir = crate::TempDir::new("reader");
    let path = dir.join("library.sqlite");
    let db = Db::open(&path).unwrap();
    db.add_vod(&info("a")).unwrap();
    let reader = Db::open_reader(&crate::Paths::new(&*dir)).unwrap();
    // A write under way on the other connection, not yet committed.
    db.conn.execute_batch("BEGIN IMMEDIATE; DELETE FROM vods;").unwrap();
    let started = std::time::Instant::now();
    assert_eq!(reader.vods().unwrap().len(), 1, "what was there before the write");
    assert!(started.elapsed() < std::time::Duration::from_millis(500), "didn't wait for the writer");
    db.conn.execute_batch("COMMIT;").unwrap();
    assert!(reader.vods().unwrap().is_empty(), "and the write once it's done");
    assert!(reader.add_vod(&info("b")).is_err(), "it only reads");
}
