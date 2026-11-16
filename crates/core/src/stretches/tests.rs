//! The specs of stretches: what of a VOD is transcribed, and what is asked
//! for, through the library's public functions (`Db::request`,
//! `Db::stretches`, `Db::end_request`, ...), on the real database in memory.

use super::*;
use crate::fetch::VodInfo;

fn info(id: &str, duration: Option<f64>) -> VodInfo {
    VodInfo {
        platform: "youtube".into(),
        platform_id: id.into(),
        url: format!("https://youtu.be/{id}"),
        title: id.into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration,
        thumbnail: None,
        published: None,
        started_at: None,
    }
}

fn stretch(id: i64, start: f64, end: Option<f64>, status: Status, reached: Option<f64>) -> Stretch {
    Stretch {
        id,
        vod_id: 1,
        start,
        end,
        status,
        progress: 0.0,
        reached,
        error: None,
        model: Some(crate::model::CPU_MODEL.id.into()),
        audio: None,
        audio_here: false,
    }
}

/// Done ranges as (start, end, model), in time order.
fn done(db: &Db, vod: i64) -> Vec<(f64, Option<f64>, Option<String>)> {
    db.stretches(vod)
        .unwrap()
        .into_iter()
        .filter(|s| s.status == Status::Done)
        .map(|s| (s.start, s.end, s.model))
        .collect()
}

/// Transcribe pressed twice (T, or the bar's button) on a VOD nothing of
/// which is transcribed yet: the whole VOD was queued twice, and
/// transcribed twice over.
#[test]
fn the_same_stretch_asked_for_again_while_it_waits_is_one_request() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("a", Some(600.0))).unwrap().0.id;
    let first = db.request(vod, 0.0, None).unwrap();
    let again = db.request(vod, 0.0, None).unwrap();
    assert_eq!(again.id, first.id);
    assert_eq!(db.requests(vod).unwrap().len(), 1);
    // Another stretch is a request of its own; one stopped (paused, or
    // failed) is asked for anew, as a retry would.
    db.request(vod, 10.0, Some(20.0)).unwrap();
    assert_eq!(db.requests(vod).unwrap().len(), 2);
    db.conn.execute("UPDATE stretches SET status = 'error' WHERE id = ?1", [first.id]).unwrap();
    assert_ne!(db.request(vod, 0.0, None).unwrap().id, first.id);
}

/// stretches.rs `Db::merge_done`: "Makes VOD `vod_id`'s done stretches apart,
/// in time order: where two overlap, the later one's lines replaced the
/// other's, so it keeps the overlap ... Neighbours made by the same model
/// become one."
#[test]
fn done_stretches_merge_apart_and_the_latest_keeps_the_overlap() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("a", Some(600.0))).unwrap().0.id;
    let m = |s: &str| Some(s.to_string());
    db.insert_done(vod, 100.0, Some(200.0), Some("a")).unwrap();
    db.insert_done(vod, 300.0, Some(400.0), Some("a")).unwrap();
    db.merge_done(vod, None).unwrap();
    assert_eq!(done(&db, vod), [(100.0, Some(200.0), m("a")), (300.0, Some(400.0), m("a"))]);
    // One between, by the same model, joins them.
    db.insert_done(vod, 200.0, Some(300.0), Some("a")).unwrap();
    db.merge_done(vod, None).unwrap();
    assert_eq!(done(&db, vod), [(100.0, Some(400.0), m("a"))]);
    // A request older than another, finished last: its range wins, by
    // another model, splitting the one it lands in.
    let old = db.request(vod, 150.0, Some(250.0)).unwrap().id;
    db.conn.execute("UPDATE stretches SET model = 'b' WHERE id = ?1", [old]).unwrap();
    db.insert_done(vod, 140.0, Some(160.0), Some("a")).unwrap();
    db.finish_stretch(old).unwrap();
    assert_eq!(
        done(&db, vod),
        [(100.0, Some(150.0), m("a")), (150.0, Some(250.0), m("b")), (250.0, Some(400.0), m("a"))]
    );
    // To the end, over everything after it.
    db.insert_done(vod, 200.0, None, Some("a")).unwrap();
    db.merge_done(vod, None).unwrap();
    assert_eq!(done(&db, vod), [(100.0, Some(150.0), m("a")), (150.0, Some(200.0), m("b")), (200.0, None, m("a"))]);
    assert_eq!(db.requests(vod).unwrap(), []);
}

/// stretches.rs `Db::outdated_ranges`: "The ranges of VOD `vod_id`
/// transcribed by a model this version no longer uses (or before models were
/// recorded)".
#[test]
fn the_ranges_made_by_an_older_model_are_found() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("a", Some(600.0))).unwrap().0.id;
    let old = "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8";
    db.insert_done(vod, 0.0, Some(100.0), Some(old)).unwrap();
    db.insert_done(vod, 100.0, Some(200.0), Some(crate::model::CPU_MODEL.id)).unwrap();
    db.insert_done(vod, 300.0, None, None).unwrap();
    db.request(vod, 400.0, Some(450.0)).unwrap();
    assert_eq!(db.outdated_ranges(vod).unwrap(), [(0.0, Some(100.0)), (300.0, None)]);
}

/// stretches.rs `covered`, `transcribed`, `gaps`: "The ranges of a VOD that
/// are transcribed, in order and apart: its done stretches, and what its
/// requests have reached."
#[test]
fn coverage_counts_done_stretches_and_how_far_requests_reached() {
    use Status::{Done, Paused, Queued, Transcribing};
    let s = |id, a, b, st, r| stretch(id, a, b, st, r);
    assert_eq!(transcribed(&[], Some(600.0)), Transcribed::None);
    assert_eq!(transcribed(&[s(1, 0.0, None, Queued, None)], Some(600.0)), Transcribed::None);
    assert_eq!(transcribed(&[s(1, 0.0, None, Done, None)], None), Transcribed::All);
    assert_eq!(transcribed(&[s(1, 0.0, None, Paused, Some(100.0))], Some(600.0)), Transcribed::Some);
    // Pieces that meet cover it all, up to a little short of the end.
    let parts = [s(1, 0.0, Some(300.0), Done, None), s(2, 300.0, None, Transcribing, Some(599.8))];
    assert_eq!(covered(&parts), [(0.0, 599.8)]);
    assert_eq!(transcribed(&parts, Some(600.0)), Transcribed::All);
    assert_eq!(transcribed(&parts, None), Transcribed::Some, "an unknown length needs a stretch to the end");
    let apart = [s(1, 0.0, Some(100.0), Done, None), s(2, 200.0, Some(300.0), Done, None)];
    assert_eq!(transcribed(&apart, Some(600.0)), Transcribed::Some);
    assert_eq!(gaps(&apart, Some(600.0)), [(100.0, Some(200.0)), (300.0, None)]);
    assert_eq!(gaps(&[], None), [(0.0, None)]);
    assert_eq!(gaps(&[s(1, 0.0, None, Done, None)], Some(600.0)), []);
}

/// stretches.rs `summarize`: "What a VOD's row shows of its stretches"; 0.6.0:
/// "One partly transcribed says how much".
#[test]
fn a_vods_status_is_the_stretch_its_row_shows() {
    use Status::{Done, Downloading, Error, Paused, Queued, Ready};
    let shown = |stretches: &[Stretch]| {
        let s = summarize(stretches, Some(600.0), true);
        (s.status, s.stretch, s.transcribed, s.waiting)
    };
    assert_eq!(shown(&[]), (Status::None, None, Transcribed::None, 0));
    assert_eq!(shown(&[stretch(1, 0.0, None, Done, None)]), (Done, None, Transcribed::All, 0));
    let some = stretch(1, 0.0, Some(60.0), Done, None);
    assert_eq!(shown(std::slice::from_ref(&some)), (Status::None, None, Transcribed::Some, 0));
    // Running first, then the first waiting, then paused, then the latest failed.
    let mut failed = stretch(2, 60.0, Some(70.0), Error, None);
    failed.error = Some("HTTP 404".into());
    let mut later = failed.clone();
    later.id = 3;
    later.error = Some("gone".into());
    let paused = stretch(4, 70.0, Some(80.0), Paused, Some(75.0));
    let waiting = stretch(5, 80.0, Some(90.0), Ready, None);
    let queued = stretch(6, 90.0, Some(95.0), Queued, None);
    let running = stretch(7, 95.0, Some(99.0), Downloading, None);
    let all = [some.clone(), failed.clone(), later.clone(), paused.clone(), waiting.clone(), queued.clone()];
    assert_eq!(shown(&[all.as_slice(), &[running]].concat()), (Downloading, Some(7), Transcribed::Some, 5));
    assert_eq!(shown(&all), (Ready, Some(5), Transcribed::Some, 4));
    assert_eq!(shown(&all[..4]), (Paused, Some(4), Transcribed::Some, 2));
    let s = summarize(&all[..3], Some(600.0), true);
    assert_eq!((s.status, s.error.as_deref(), s.waiting), (Error, Some("gone"), 1));
    // Outdated: lines made, or begun, with a model no longer used.
    let mut old = stretch(8, 0.0, Some(10.0), Done, None);
    old.model = Some("sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8".into());
    assert!(summarize(std::slice::from_ref(&old), Some(600.0), true).outdated);
    assert!(!summarize(std::slice::from_ref(&old), Some(600.0), false).outdated, "no lines");
    old.status = Queued;
    assert!(!summarize(std::slice::from_ref(&old), Some(600.0), true).outdated, "not begun");
    old.reached = Some(5.0);
    assert!(summarize(std::slice::from_ref(&old), Some(600.0), true).outdated);
    // Done with an older one, and being transcribed again: not outdated meanwhile.
    old.status = Done;
    let again = stretch(9, 0.0, None, Queued, None);
    assert!(!summarize(&[old, again], Some(600.0), true).outdated);
}

/// stretches.rs `Db::end_request`: "Ends request `id`, keeping what it
/// transcribed: done from its start to where it reached, or gone if it
/// reached nothing."
#[test]
fn a_request_stopped_before_any_line_leaves_the_vod_untranscribed() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("a", Some(600.0))).unwrap().0.id;
    for status in [Status::Queued, Status::Downloading, Status::Transcribing, Status::Paused, Status::Error] {
        let asked = db.request(vod, 0.0, None).unwrap().id;
        db.set_stretch_status(asked, status, 0.3, (status == Status::Error).then_some("HTTP 404")).unwrap();
        db.end_request(asked).unwrap();
        let v = db.vod(vod).unwrap().unwrap();
        assert_eq!(
            (v.status, v.error, v.stretch, v.transcribed, v.waiting),
            (Status::None, None, None, Transcribed::None, 0),
            "stopped while {status:?}: like a VOD never asked"
        );
    }
}

/// stretches.rs `Db::next_stretch`: "The first request asked for with
/// `status`, of a VOD in the library's list ... Taking a VOD out of the list
/// ends its requests; one left on it anyway isn't taken up." (A file on this
/// computer's own queue, `local_only`, is `local_files_queue_separately` in
/// db/tests.rs.)
#[test]
fn the_queue_runs_in_the_order_asked_and_waits_for_listing() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let [a, b] = ["a", "b"].map(|id| db.add_vod(&info(id, Some(60.0))).unwrap().0.id);
    let first = db.request(b, 0.0, None).unwrap();
    let second = db.request(a, 10.0, Some(20.0)).unwrap();
    let next = |db: &Db| db.next_stretch(Status::Queued, false).unwrap().map(|(v, s)| (v.id, s.id));
    assert_eq!(next(&db), Some((b, first.id)), "asked first, whatever the library's order");
    db.set_stretch_status(first.id, Status::Downloading, 0.0, None).unwrap();
    assert_eq!(next(&db), Some((a, second.id)));
    assert_eq!(db.count_stretches(Status::Downloading).unwrap(), 1);
    assert_eq!(db.vod(b).unwrap().unwrap().status, Status::Downloading);
    db.remove_entry(a).unwrap();
    assert_eq!(next(&db), None, "out of the list, its work isn't taken up");
}

/// What the waveform asks of a request paused or failed (media.ts
/// `waveformGaps`: levels for "all of a request whose audio is here"):
/// whether its audio came, its own section or the VOD's whole audio.
#[test]
fn a_request_says_whether_its_audio_is_here() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let [a, b] = ["a", "b"].map(|id| db.add_vod(&info(id, Some(600.0))).unwrap().0.id);
    let before = db.request(a, 0.0, Some(100.0)).unwrap().id;
    db.set_stretch_status(before, Status::Paused, 0.3, None).unwrap();
    let section = db.request(a, 100.0, Some(200.0)).unwrap().id;
    let path = db.files.source_section(a, section, "m4a");
    db.set_stretch_audio(section, Some(path.to_str().unwrap())).unwrap();
    db.set_stretch_status(section, Status::Paused, 0.3, None).unwrap();
    let whole = db.request(b, 100.0, None).unwrap().id;
    db.set_audio_path(b, Some(db.files.source_audio(b, "m4a").to_str().unwrap())).unwrap();
    db.set_stretch_status(whole, Status::Error, 0.0, Some("worker gone")).unwrap();
    let here = |vod: i64| db.stretches(vod).unwrap().into_iter().map(|s| (s.id, s.audio_here)).collect::<Vec<_>>();
    assert_eq!(here(a), [(before, false), (section, true)], "paused before its audio came, and after");
    assert_eq!(here(b), [(whole, true)], "failed with the VOD's whole audio here");
    let shown = serde_json::to_value(db.stretch(section).unwrap().unwrap()).unwrap();
    assert_eq!(shown["audio"], true, "the page reads it as `audio`");
}

/// A library with one VOD `length` seconds long, for the specs below.
fn vod_of(length: f64) -> (Db, i64) {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("a", Some(length))).unwrap().0.id;
    (db, vod)
}

fn line(text: &str, start: f64, end: f64) -> Segment {
    Segment { start, end, text: text.into(), words: Vec::new() }
}

fn texts(db: &Db, vod: i64) -> Vec<String> {
    db.segments(vod).unwrap().into_iter().map(|s| s.text).collect()
}

/// stretches.rs `EDGE`: "a VOD transcribed this close to each end as
/// transcribed to it: a VOD's length from its site and the end of its audio
/// differ by a little." Half a second itself is no spec's (the line is drawn
/// at 0.3 and 0.7).
#[test]
fn a_vod_transcribed_to_within_half_a_second_of_each_end_is_transcribed_to_it() {
    use Status::Done;
    let of = |a, b| transcribed(&[stretch(1, a, Some(b), Done, None)], Some(600.0));
    assert_eq!(of(0.3, 599.8), Transcribed::All);
    assert_eq!(of(0.7, 599.8), Transcribed::Some, "0.7 s short at the start");
    assert_eq!(of(0.3, 599.2), Transcribed::Some, "0.8 s short at the end");
    let gaps_of = |a, b| gaps(&[stretch(1, a, Some(b), Done, None)], Some(600.0));
    assert_eq!(gaps_of(0.3, 599.8), []);
    assert_eq!(gaps_of(0.7, 599.8), [(0.0, Some(0.7))]);
    assert_eq!(gaps_of(0.3, 599.2), [(599.2, None)]);
}

/// stretches.rs `EDGE`: "Ranges this close count as touching".
#[test]
fn ranges_within_half_a_second_of_each_other_are_covered_as_one() {
    use Status::Done;
    let two = |gap: f64| [stretch(1, 0.0, Some(100.0), Done, None), stretch(2, 100.0 + gap, Some(200.0), Done, None)];
    assert_eq!(covered(&two(0.3)), [(0.0, 200.0)]);
    assert_eq!(covered(&two(0.7)), [(0.0, 100.0), (100.7, 200.0)]);
    assert_eq!(gaps(&two(0.7), Some(200.0)), [(100.0, Some(100.7))]);
}

/// stretches.rs `covered`: "its done stretches, and what its requests have
/// reached": a request that hasn't started covers nothing, nor one that has
/// reached no further than its start; an end of infinity is the VOD's end.
#[test]
fn a_request_covers_what_it_has_reached_and_a_stretch_to_the_end_covers_to_it() {
    use Status::{Done, Paused, Queued, Transcribing};
    assert_eq!(covered(&[stretch(1, 100.0, Some(200.0), Queued, None)]), []);
    assert_eq!(covered(&[stretch(1, 100.0, Some(200.0), Transcribing, Some(100.0))]), []);
    assert_eq!(covered(&[stretch(1, 100.0, Some(200.0), Paused, Some(130.0))]), [(100.0, 130.0)]);
    assert_eq!(covered(&[stretch(1, 100.0, None, Done, None)]), [(100.0, f64::INFINITY)]);
}

/// stretches.rs `Stretch::from`: "Where its transcription carries on from:
/// its start, or as far as it got"; `is_whole`: "The whole VOD, from its start
/// to its end."
#[test]
fn a_request_carries_on_from_where_it_got_and_the_whole_vod_is_from_its_start_to_its_end() {
    use Status::Paused;
    assert_eq!(stretch(1, 100.0, None, Paused, None).from(), 100.0);
    assert_eq!(stretch(1, 100.0, None, Paused, Some(160.0)).from(), 160.0);
    assert_eq!(stretch(1, 100.0, None, Paused, Some(50.0)).from(), 100.0, "never before its start");
    assert!(stretch(1, 0.0, None, Paused, None).is_whole());
    assert!(!stretch(1, 0.0, Some(600.0), Paused, None).is_whole());
    assert!(!stretch(1, 10.0, None, Paused, None).is_whole());
}

/// stretches.rs `Db::request`: "The same stretch asked for again while it
/// waits or runs is that request (Transcribe pressed twice mustn't transcribe
/// it twice)": in whichever stage it is. A stretch ending elsewhere, or to
/// the end, is another.
#[test]
fn a_stretch_asked_for_again_in_any_stage_of_its_work_is_that_request() {
    let (db, vod) = vod_of(600.0);
    for status in [Status::Queued, Status::Downloading, Status::Ready, Status::Transcribing] {
        let asked = db.request(vod, 10.0, Some(20.0)).unwrap();
        db.set_stretch_status(asked.id, status, 0.5, None).unwrap();
        assert_eq!(db.request(vod, 10.0, Some(20.0)).unwrap().id, asked.id, "{status:?}");
        db.delete_stretch(asked.id).unwrap();
    }
    let asked = db.request(vod, 10.0, Some(20.0)).unwrap();
    assert_ne!(db.request(vod, 10.0, Some(30.0)).unwrap().id, asked.id);
    assert_ne!(db.request(vod, 10.0, None).unwrap().id, asked.id);
    assert_eq!(db.requests(vod).unwrap().len(), 3);
}

/// stretches.rs `Db::stretches`: "VOD `vod_id`'s stretches, done and asked
/// for, in time order"; `Db::requests`: "VOD `vod_id`'s requests, in the order
/// they were made" (done stretches aren't requests).
#[test]
fn a_vods_stretches_are_in_time_order_and_its_requests_in_the_order_asked() {
    let (db, vod) = vod_of(600.0);
    let late = db.request(vod, 300.0, Some(400.0)).unwrap().id;
    let early = db.request(vod, 100.0, Some(200.0)).unwrap().id;
    let before = db.insert_done(vod, 0.0, Some(50.0), Some("a")).unwrap();
    let ids = |list: Vec<Stretch>| list.into_iter().map(|s| s.id).collect::<Vec<_>>();
    assert_eq!(ids(db.stretches(vod).unwrap()), [before, early, late]);
    assert_eq!(ids(db.requests(vod).unwrap()), [late, early]);
}

/// stretches.rs `Db::requeue_interrupted`: "Returns requests left mid-stage
/// by a crash or quit to that stage's queue. Transcription resumes from where
/// it reached." (Each VOD's status after it is `interrupted_jobs_return_to_
/// their_queues` in db/tests.rs, and the app's restart is
/// `interrupted_requests_are_waiting_again_after_a_restart`.)
#[test]
fn an_interrupted_request_resumes_from_where_it_reached_and_a_stopped_one_stays_stopped() {
    let (db, vod) = vod_of(600.0);
    let ask = |start: f64, status, reached| {
        let id = db.request(vod, start, None).unwrap().id;
        db.set_stretch_status(id, status, 0.4, None).unwrap();
        db.set_reached(id, reached).unwrap();
        id
    };
    let transcribing = ask(100.0, Status::Transcribing, Some(160.0));
    let downloading = ask(200.0, Status::Downloading, None);
    let paused = ask(300.0, Status::Paused, Some(320.0));
    let failed = ask(400.0, Status::Error, None);
    db.requeue_interrupted().unwrap();
    let now = |id| db.stretch(id).unwrap().unwrap();
    assert_eq!((now(transcribing).status, now(transcribing).from()), (Status::Ready, 160.0));
    assert_eq!(now(downloading).status, Status::Queued);
    assert_eq!((now(paused).status, now(paused).from()), (Status::Paused, 320.0));
    assert_eq!(now(failed).status, Status::Error);
}

/// stretches.rs `Db::forget_downloads`: "Forgets every finished download,
/// after the work dir has been cleared. Requests waiting to transcribe go back
/// to waiting to download." (The VOD's own audio is `interrupted_jobs_return_
/// to_their_queues` in db/tests.rs.)
#[test]
fn forgetting_the_downloads_has_requests_waiting_to_transcribe_download_again() {
    let (db, vod) = vod_of(600.0);
    let ask = |start: f64, status| {
        let id = db.request(vod, start, Some(start + 10.0)).unwrap().id;
        let section = db.files.source_section(vod, id, "m4a");
        db.set_stretch_audio(id, Some(section.to_str().unwrap())).unwrap();
        db.set_stretch_status(id, status, 0.5, None).unwrap();
        id
    };
    let ready = ask(0.0, Status::Ready);
    let paused = ask(100.0, Status::Paused);
    assert!(db.stretch(ready).unwrap().unwrap().audio_here);
    db.forget_downloads().unwrap();
    let ready = db.stretch(ready).unwrap().unwrap();
    assert_eq!((ready.status, ready.progress, ready.audio_here), (Status::Queued, 0.0, false));
    let paused = db.stretch(paused).unwrap().unwrap();
    assert_eq!((paused.status, paused.audio_here), (Status::Paused, false), "paused stays, its audio gone");
}

/// stretches.rs `Db::end_request`: "keeping what it transcribed: done from
/// its start to where it reached, or gone if it reached nothing" (the VOD
/// that reached nothing is `a_request_stopped_before_any_line_leaves_the_vod_
/// untranscribed`); 0.3.0 Fixes: "Stopping a transcription keeps its lines."
#[test]
fn a_request_ended_after_some_lines_is_done_as_far_as_it_reached() {
    let (db, vod) = vod_of(600.0);
    let model = crate::model::CPU_MODEL.id;
    db.insert_done(vod, 0.0, Some(100.0), Some(model)).unwrap();
    let asked = db.request(vod, 100.0, None).unwrap().id;
    db.set_stretch_model(asked, Some(model)).unwrap();
    db.set_stretch_status(asked, Status::Paused, 0.3, None).unwrap();
    db.set_reached(asked, Some(160.0)).unwrap();
    db.end_request(asked).unwrap();
    assert_eq!(done(&db, vod), [(0.0, Some(160.0), Some(model.to_string()))], "one with its neighbour");
    assert_eq!(db.requests(vod).unwrap(), []);
    let v = db.vod(vod).unwrap().unwrap();
    assert_eq!((v.status, v.transcribed), (Status::None, Transcribed::Some));
    // Reached no further than its start: nothing was transcribed.
    let none = db.request(vod, 300.0, Some(400.0)).unwrap().id;
    db.set_reached(none, Some(300.0)).unwrap();
    db.end_request(none).unwrap();
    assert_eq!(db.stretches(vod).unwrap().len(), 1);
}

/// stretches.rs `Db::finish_stretch`: "Request `id` is done: its range is
/// transcribed, over whatever was there."
#[test]
fn a_finished_request_is_a_done_stretch_with_no_error_and_no_section_of_audio() {
    let (db, vod) = vod_of(600.0);
    let asked = db.request(vod, 100.0, Some(200.0)).unwrap().id;
    let section = db.files.source_section(vod, asked, "m4a");
    db.set_stretch_audio(asked, Some(section.to_str().unwrap())).unwrap();
    db.set_stretch_status(asked, Status::Error, 0.5, Some("worker gone")).unwrap();
    db.finish_stretch(asked).unwrap();
    let all = db.stretches(vod).unwrap();
    assert_eq!(all.len(), 1);
    let s = &all[0];
    assert_eq!((s.start, s.end, s.status, s.progress), (100.0, Some(200.0), Status::Done, 1.0));
    assert_eq!((s.error.as_deref(), s.audio_here), (None, false));
    assert_eq!(db.requests(vod).unwrap(), []);
}

/// stretches.rs `Db::snap_stretch`: "Widens request `id` outwards to the
/// edges of any line its start or end cuts through, so a line is transcribed
/// again whole or not at all; before it starts only (later, its audio is
/// already cut to it)." (Widening a request that hasn't started is
/// `a_stretch_snaps_to_lines_and_replaces_them_as_it_goes` in pipeline.rs.)
#[test]
fn a_request_that_has_begun_or_cuts_no_line_is_not_widened() {
    let (mut db, vod) = vod_of(600.0);
    db.insert_segments(vod, &[line("a", 8.0, 12.0), line("b", 20.0, 25.0)]).unwrap();
    let range = |db: &Db, id| {
        let s = db.snap_stretch(id).unwrap();
        (s.start, s.end)
    };
    let clear = db.request(vod, 12.0, Some(20.0)).unwrap().id;
    assert_eq!(range(&db, clear), (12.0, Some(20.0)), "between two lines");
    let to_the_end = db.request(vod, 10.0, None).unwrap().id;
    assert_eq!(range(&db, to_the_end), (8.0, None), "widened at its start, still to the end");
    let begun = db.request(vod, 10.0, Some(22.0)).unwrap().id;
    db.set_reached(begun, Some(15.0)).unwrap();
    assert_eq!(range(&db, begun), (10.0, Some(22.0)), "its audio is already cut to it");
}

/// stretches.rs `Db::replace_segments`: "Writes `segments`, transcribed for
/// request `stretch_id`, in place of the lines that start from `from` to
/// before `to` (None: on to the end), which it has transcribed over, and
/// records that it reached `reached` ... Returns how many lines it replaced."
#[test]
fn a_requests_lines_replace_those_that_start_in_its_range_and_it_records_how_far_it_reached() {
    let (mut db, vod) = vod_of(600.0);
    let old = [line("a", 0.0, 5.0), line("b", 8.0, 12.0), line("c", 20.0, 25.0), line("d", 25.0, 30.0)];
    db.insert_segments(vod, &old).unwrap();
    let asked = db.request(vod, 8.0, Some(25.0)).unwrap().id;
    let replaced = db.replace_segments(vod, asked, (8.0, Some(25.0)), &[line("new", 9.0, 10.0)], 25.0).unwrap();
    assert_eq!(replaced, 2, "b and c start in 8..25");
    assert_eq!(texts(&db, vod), ["a", "new", "d"], "the line at its end stays");
    assert_eq!(db.stretch(asked).unwrap().unwrap().reached, Some(25.0));
    // To the end: everything from there on.
    let replaced = db.replace_segments(vod, asked, (10.0, None), &[line("last", 11.0, 12.0)], 40.0).unwrap();
    assert_eq!(replaced, 1, "d; the line at 9 starts before 10");
    assert_eq!(texts(&db, vod), ["a", "new", "last"]);
    assert_eq!(db.stretch(asked).unwrap().unwrap().reached, Some(40.0));
}

/// stretches.rs `Db::clear_stretches`: "Forgets all of VOD `vod_id`'s
/// stretches, done and asked for."
#[test]
fn clearing_a_vods_stretches_forgets_those_done_and_those_asked_for_and_no_other_vods() {
    let (db, vod) = vod_of(600.0);
    let other = db.add_vod(&info("b", Some(60.0))).unwrap().0.id;
    db.insert_done(vod, 0.0, Some(50.0), None).unwrap();
    db.request(vod, 100.0, None).unwrap();
    let kept = db.request(other, 0.0, None).unwrap().id;
    db.clear_stretches(vod).unwrap();
    assert_eq!(db.stretches(vod).unwrap(), []);
    assert_eq!(db.stretches(other).unwrap().iter().map(|s| s.id).collect::<Vec<_>>(), [kept]);
}

/// stretches.rs `summarize`: a done stretch "a request covers is being
/// transcribed again", so not outdated meanwhile; one that no request covers
/// whole, or that is only begun with no line yet, still is as it was.
#[test]
fn a_stretch_by_an_old_model_is_outdated_until_a_request_covers_all_of_it() {
    use Status::{Done, Queued, Transcribing};
    let old = || {
        let mut s = stretch(1, 10.0, Some(20.0), Done, None);
        s.model = Some("sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8".into());
        s
    };
    let outdated = |others: &[Stretch]| summarize(&[&[old()], others].concat(), Some(600.0), true).outdated;
    assert!(!outdated(&[stretch(2, 10.0, Some(20.0), Queued, None)]), "asked for again, as it is");
    assert!(!outdated(&[stretch(2, 0.0, Some(30.0), Queued, None)]), "inside what's asked for");
    assert!(outdated(&[stretch(2, 15.0, Some(30.0), Queued, None)]), "a request starting inside it");
    assert!(outdated(&[stretch(2, 0.0, Some(15.0), Queued, None)]), "a request ending inside it");
    assert!(outdated(&[stretch(2, 30.0, None, Queued, None)]), "a request after it");
    // A request by an old model that has reached only its start has written no line.
    let mut begun = stretch(3, 50.0, Some(60.0), Transcribing, Some(50.0));
    begun.model = old().model;
    assert!(!summarize(&[begun], Some(600.0), true).outdated);
}

/// stretches.rs `summarize`: a VOD transcribed all through, with nothing asked
/// for, is done (all of it); one that isn't, is not asked (none of it).
#[test]
fn a_vod_transcribed_all_through_is_done_at_all_of_it_and_one_not_asked_has_none_of_it() {
    let done = summarize(&[stretch(1, 0.0, None, Status::Done, None)], Some(600.0), true);
    assert_eq!((done.status, done.progress), (Status::Done, 1.0));
    let some = summarize(&[stretch(1, 0.0, Some(60.0), Status::Done, None)], Some(600.0), true);
    assert_eq!((some.status, some.progress), (Status::None, 0.0));
}
/// stretches.rs `Db::count_stretches`: how many requests wait with a status,
/// across the library's VODs, which the downloads' limit on those waiting for
/// the worker reads (app worker.rs `next_job`, `MAX_READY`).
#[test]
fn the_requests_with_a_status_are_counted_across_vods() {
    let (db, vod) = vod_of(600.0);
    let other = db.add_vod(&info("b", Some(60.0))).unwrap().0.id;
    assert_eq!(db.count_stretches(Status::Ready).unwrap(), 0);
    for (v, start) in [(vod, 0.0), (vod, 10.0), (other, 0.0)] {
        let id = db.request(v, start, Some(start + 5.0)).unwrap().id;
        db.set_stretch_status(id, Status::Ready, 0.0, None).unwrap();
    }
    let queued = db.request(other, 30.0, Some(40.0)).unwrap().id;
    assert_eq!(db.count_stretches(Status::Ready).unwrap(), 3);
    assert_eq!(db.count_stretches(Status::Queued).unwrap(), 1);
    db.finish_stretch(queued).unwrap();
    assert_eq!(db.count_stretches(Status::Queued).unwrap(), 0);
}

/// stretches.rs `Db::snap_stretch`: the request it gives back is the one
/// widened, at its end as at its start.
#[test]
fn the_request_given_back_is_widened_at_its_end_too() {
    let (mut db, vod) = vod_of(600.0);
    db.insert_segments(vod, &[line("a", 8.0, 12.0), line("b", 20.0, 25.0)]).unwrap();
    let asked = db.request(vod, 10.0, Some(22.0)).unwrap().id;
    let snapped = db.snap_stretch(asked).unwrap();
    assert_eq!((snapped.start, snapped.end), (8.0, Some(25.0)));
    let kept = db.stretch(asked).unwrap().unwrap();
    assert_eq!((kept.start, kept.end), (8.0, Some(25.0)));
}
