//! Upgrades of libraries from the clip model on (V22 on), and the libraries
//! releases left: old schema and rows in, the same data out, read back
//! through the library's readers.

use super::*;
use crate::testkit::{Released, at_version, back_to, format_of};

/// A folder of the test's own, `name`d, and where its library file goes.
fn library(name: &str) -> (crate::TempDir, PathBuf) {
    let dir = crate::TempDir::new(name);
    let path = dir.join("library.sqlite");
    (dir, path)
}

/// As `library`, laid out as the app lays one out (`Paths`), for the specs
/// of the library's own files.
fn library_in(name: &str) -> (crate::TempDir, crate::Paths) {
    let dir = crate::TempDir::new(name);
    let paths = crate::Paths::new(dir.join("lib"));
    paths.ensure().unwrap();
    (dir, paths)
}

/// The library at `path` opened as the app opens it, so upgraded to this
/// version's format.
fn upgraded(path: &Path) -> Db {
    let db = Db::open(path).unwrap();
    assert_eq!(format_of(&db.conn), MIGRATIONS.len(), "upgraded to this version's format");
    db
}

/// As `upgraded`, the library `paths` lays out.
fn upgraded_in(paths: &crate::Paths) -> Db {
    let db = Db::open_in(paths).unwrap();
    assert_eq!(format_of(&db.conn), MIGRATIONS.len(), "upgraded to this version's format");
    db
}

/// `seconds` in flicks, as the library has kept times since V22.
fn flicks(seconds: f64) -> i64 {
    Flicks::from_seconds(seconds).0
}

/// A library as 0.4.0 left one (format 21, before the clip model), written
/// at `path`: VOD 1 listed last and VOD 2 first, with the edit between;
/// VOD 1's clip bar with two clips (Lunch, its file dragged out, after
/// Dinner) and VOD 2's with one only dragged out; a chapter on VOD 1 and a
/// highlight on VOD 2; the edit, rendered, of a piece of each, one with a
/// stretch struck (in seconds, as then).
fn at_21(path: &Path) {
    at_version(path, 21)
        .execute_batch(
            "INSERT INTO vods (id, platform, platform_id, url, title, duration, status, added_at, position) VALUES
                 (1, 'kick', 'a', 'u', 'First', 600.0, 'done', 0, 2), (2, 'kick', 'b', 'u', 'Second', 300.0, 'done', 0, 0);
             INSERT INTO clips (id, vod_id, start, end, title, position, created_at, status, file_path, shown, handed_out) VALUES
                 (1, 1, 10.0, 20.5, 'Lunch', 2, 0, 'ready', '/v/lunch.mp4', 1, 1),
                 (2, 1, 30.0, 40.0, 'Dinner', 1, 0, 'waiting', NULL, 1, 0),
                 (3, 2, 1.0, 2.0, 'Dragged', 1, 0, 'ready', '/v/d.mp4', 0, 0);
             INSERT INTO marks (id, vod_id, kind, start, text, created_at) VALUES
                 (7, 1, 'chapter', 65.25, 'The raid', 0), (8, 2, 'highlight', 3.0, NULL, 0);
             INSERT INTO edits (id, title, position, created_at, version, status, file_path, rendered_version) VALUES
                 (1, 'Best of', 1, 0, 4, 'ready', '/v/best.mp4', 4);
             INSERT INTO edit_pieces (id, edit_id, vod_id, position, start, end, title, struck) VALUES
                 (5, 1, 1, 1, 100.0, 130.0, 'b', '[]'), (4, 1, 2, 0, 5.0, 18.0, 'a', '[[12.0,13.5]]');",
        )
        .unwrap();
}

/// db.rs `MIGRATE_V22`: "`clips` holds every document: a VOD's own clip over
/// the whole VOD (`whole_of` …), the edits (listed, `position`), and the clip
/// bar's clips, now snippets hanging off their VOD's clip (`bar_of`). `items`
/// holds their pieces" (0.5.0: "VODs and edits open in one player").
#[test]
fn vods_clips_and_edits_become_documents_with_the_vods_own_clip_listing_it() {
    let (_dir, path) = library("migrate-v22");
    at_21(&path);
    let db = upgraded(&path);
    let edit = db.edits(crate::edits::tests::at_first()).unwrap().remove(0);
    assert_eq!((edit.title.as_str(), edit.current), ("Best of", true));
    // Its file isn't there, nor where settle takes files from: read as
    // gone (where it would be built), as before, so it's made again.
    let file = edit.file_path.expect("a file, gone");
    assert_eq!(file, shown(&db.files().clip_file(edit.id, "mp4")));
    assert!(!Path::new(&file).exists());
    // Piece 4's stretch struck is taken out (V46): two pieces in its place.
    let pieces = db.edit_pieces(edit.id).unwrap();
    let split = pieces[1].id;
    assert_eq!(
        pieces.iter().map(|p| (p.id, p.vod_id, p.start, p.end)).collect::<Vec<_>>(),
        [(4, 2, 5.0, 12.0), (split, 2, 13.5, 18.0), (5, 1, 100.0, 130.0)]
    );
    // Each VOD has its own clip, over all of it, listing it.
    for vod in [1, 2] {
        let own = db.vod_clip(vod).unwrap().unwrap();
        let length = db.vod(vod).unwrap().unwrap().info.duration.unwrap();
        assert_eq!(db.edit_pieces(own).unwrap().iter().map(|p| (p.start, p.end)).collect::<Vec<_>>(), [(0.0, length)]);
        assert_ne!(own, edit.id);
        assert!(db.vod(vod).unwrap().unwrap().listed);
    }
    // The clip bars' clips hang off their VOD's clip, with their files; the one only dragged out is hidden still.
    let bar: Vec<_> = db.clips(1).unwrap().into_iter().map(|c| (c.title, c.start, c.end, c.shown)).collect();
    assert_eq!(bar, [("Dinner".into(), 30.0, 40.0, true), ("Lunch".into(), 10.0, 20.5, true)]);
    let own = db.vod_clip(1).unwrap().unwrap();
    assert!(db.clips(1).unwrap().iter().all(|c| c.bar == Some(ClipBar::Vod { id: 1 }) && c.id != own));
    assert!(db.clip_handed_out(1).unwrap() && !db.clip_handed_out(2).unwrap());
    let lunch = db.clip(1).unwrap().unwrap().file_path;
    assert_eq!(lunch, Some(shown(&db.files().clip_file(1, "mp4"))), "its file isn't there either: read as gone");
    assert_eq!(
        db.clips(2).unwrap().iter().map(|c| (c.title.as_str(), c.shown)).collect::<Vec<_>>(),
        [("Dragged", false)]
    );
    // Deleting a VOD takes its clip, the bar and its marks.
    db.delete_vod(1).unwrap();
    assert!(db.vod_clip(1).unwrap().is_none() && db.clips(1).unwrap().is_empty() && db.marks(1).unwrap().is_empty());
    assert_eq!(db.clip(2).unwrap(), None);
    assert_eq!(db.edit_pieces(edit.id).unwrap().len(), 2, "the edit keeps its pieces of the other VOD");
}

/// A VOD's own clip comes "with the VOD's place in the library's list, which
/// `vods.position` held", and the edits are listed by `position` (db.rs
/// `MIGRATE_V22`).
#[test]
fn the_library_keeps_its_order_through_the_clip_model() {
    let (_dir, path) = library("migrate-v22-order");
    at_21(&path);
    let db = upgraded(&path);
    use crate::edits::LibraryItem::{Edit, Vod};
    let edit = db.edits(crate::edits::tests::at_first()).unwrap().remove(0).id;
    assert_eq!(db.library_order().unwrap(), [Vod(2), Edit(edit), Vod(1)]);
    assert_eq!(db.vods().unwrap().iter().map(|v| v.id).collect::<Vec<_>>(), [2, 1], "VODs list in the order too");
}

/// "Clips keep their ids (their pictures are named by them); VODs' clips and
/// edits get ids above them" (db.rs `MIGRATE_V22`): each clip's picture
/// (`Paths::clip_thumb`) is the one it had.
#[test]
fn clips_keep_their_ids_through_the_clip_model_so_their_pictures_are_found() {
    let (dir, path) = library("migrate-v22-ids");
    at_21(&path);
    let pictures = crate::Paths::new(&*dir);
    std::fs::create_dir_all(pictures.clip_thumbs()).unwrap();
    for (id, title) in [(1, "Lunch"), (2, "Dinner"), (3, "Dragged")] {
        std::fs::write(pictures.clip_thumb(id), title).unwrap();
    }
    let db = upgraded(&path);
    let clips: Vec<Clip> = [1, 2].iter().flat_map(|&vod| db.clips(vod).unwrap()).collect();
    assert_eq!(
        clips.iter().map(|c| (c.id, c.title.as_str())).collect::<Vec<_>>(),
        [(2, "Dinner"), (1, "Lunch"), (3, "Dragged")]
    );
    for clip in &clips {
        assert_eq!(std::fs::read_to_string(db.files().clip_thumb(clip.id)).unwrap(), clip.title, "its own picture");
    }
    let edit = db.edits(crate::edits::tests::at_first()).unwrap().remove(0).id;
    let others = [db.vod_clip(1).unwrap().unwrap(), db.vod_clip(2).unwrap().unwrap(), edit];
    assert!(others.iter().all(|&id| id > 3), "a VOD's clip or an edit took a clip's id: {others:?}");
}

/// "Marks belong to a clip (the VOD's own, for those made so far) and keep
/// the VOD moment they're at" (db.rs `MIGRATE_V22`).
#[test]
fn marks_stay_on_their_moment_through_the_clip_model() {
    let (_dir, path) = library("migrate-v22-marks");
    at_21(&path);
    let db = upgraded(&path);
    let marks =
        |vod| -> Vec<_> { db.marks(vod).unwrap().into_iter().map(|m| (m.id, m.kind, m.start, m.text)).collect() };
    assert_eq!(marks(1), [(7, "chapter".into(), 65.25, Some("The raid".into()))]);
    assert_eq!(marks(2), [(8, "highlight".into(), 3.0, None)]);
    // The VOD's own clip's, at that moment of the VOD.
    let own: Vec<_> = db
        .clip_marks(db.vod_clip(1).unwrap().unwrap())
        .unwrap()
        .into_iter()
        .map(|(id, m)| (id, m.vod_id, m.at))
        .collect();
    assert_eq!(own, [(7, 1, Flicks::from_seconds(65.25))]);
}

/// "The struck stretches, JSON seconds till now, are put in flicks by
/// `Db::open` in the same transaction" (db.rs `MIGRATE_V22`;
/// `struck_to_flicks`): they're read in seconds, from a library from before
/// the clip model and from one already in flicks, then taken out (V46,
/// `struck_stretches_become_splits`): the piece is split there, to the flick.
#[test]
fn struck_stretches_read_the_same_in_flicks() {
    let (_dir, path) = library("migrate-v22-struck");
    at_21(&path);
    let db = upgraded(&path);
    let edit = db.edits(crate::edits::tests::at_first()).unwrap().remove(0).id;
    let pieces: Vec<_> = db.edit_pieces(edit).unwrap().into_iter().map(|p| (p.start, p.end, p.title)).collect();
    assert_eq!(pieces, [(5.0, 12.0, "a".into()), (13.5, 18.0, "a".into()), (100.0, 130.0, "b".into())]);
    let row = db.piece_row(4).unwrap().unwrap();
    assert_eq!((row.start, row.end), (Flicks::from_seconds(5.0), Some(Flicks::from_seconds(12.0))));

    // At V22 they're in flicks already: put in flicks again, they'd be far off.
    let (_dir, path) = library("migrate-v23-struck");
    at_version(&path, 22)
        .execute_batch(&format!(
            "INSERT INTO vods (id, platform, platform_id, url, title, duration, status, added_at) VALUES
                 (1, 'kick', 'a', 'u', 'First', 600.0, 'done', 0);
             INSERT INTO clips (id, title, created_at, position, whole_of) VALUES (1, 'First', 0, 1, 1), (2, 'Best of', 0, 0, NULL);
             INSERT INTO items (clip_id, position, kind, vod_id, start, end) VALUES (1, 0, 'piece', 1, 0, NULL);
             INSERT INTO items (id, clip_id, position, kind, vod_id, start, end, struck, title) VALUES
                 (5, 2, 0, 'piece', 1, {}, {}, '[[{},{}]]', 'a');",
            flicks(5.0),
            flicks(18.0),
            flicks(12.0),
            flicks(13.5)
        ))
        .unwrap();
    let db = upgraded(&path);
    let pieces: Vec<_> = db.edit_pieces(2).unwrap().into_iter().map(|p| (p.start, p.end)).collect();
    assert_eq!(pieces, [(5.0, 12.0), (13.5, 18.0)]);
}

/// "What's been rendered of a clip (its status, file, error, the revision
/// it's of, and whether its file was dragged out) is kept apart from the
/// clip" (db.rs `MIGRATE_V23`), and reads the same: a clip's status, file,
/// error and hand-out; an edit up to date if rendered at its revision.
#[test]
fn what_was_rendered_of_a_clip_is_kept_apart_and_reads_the_same() {
    let (_dir, path) = library("migrate-v23");
    at_version(&path, 22)
        .execute_batch(&format!(
            "INSERT INTO vods (id, platform, platform_id, url, title, duration, status, added_at) VALUES
                 (1, 'kick', 'a', 'u', 'First', 600.0, 'done', 0);
             INSERT INTO clips (id, title, created_at, revision, position, bar_of, bar_position, whole_of, shown, status,
                                file_path, error, rendered_revision, handed_out) VALUES
                 (1, 'First', 0, 0, 1, NULL, NULL, 1, 1, 'waiting', NULL, NULL, NULL, 0),
                 (2, 'Lunch', 0, 2, NULL, 1, 1, NULL, 1, 'ready', '/v/2.mp4', NULL, 2, 1),
                 (3, 'Broken', 0, 0, NULL, 1, 2, NULL, 1, 'error', NULL, 'ffmpeg failed', NULL, 0),
                 (4, 'Best of', 0, 4, 0, NULL, NULL, NULL, 1, 'ready', '/v/4.mp4', NULL, 4, 0),
                 (5, 'Older', 0, 5, 2, NULL, NULL, NULL, 1, 'ready', '/v/5.mp4', NULL, 3, 0);
             INSERT INTO items (clip_id, position, kind, vod_id, start, end) VALUES
                 (1, 0, 'piece', 1, 0, NULL), (2, 0, 'piece', 1, {ten}, {twenty}), (3, 0, 'piece', 1, {ten}, {twenty}),
                 (4, 0, 'piece', 1, 0, {ten}), (5, 0, 'piece', 1, 0, {ten});",
            ten = flicks(10.0),
            twenty = flicks(20.0)
        ))
        .unwrap();
    let db = upgraded(&path);
    let lunch = db.clip(2).unwrap().unwrap();
    assert_eq!(
        (lunch.status, lunch.file_path, lunch.error, lunch.revision),
        (ClipStatus::Ready, Some(shown(&db.files().clip_file(2, "mp4"))), None, 2)
    );
    assert!(db.clip_handed_out(2).unwrap(), "its file was dragged out");
    let broken = db.clip(3).unwrap().unwrap();
    assert_eq!(
        (broken.status, broken.file_path, broken.error.as_deref()),
        (ClipStatus::Error, None, Some("ffmpeg failed"))
    );
    assert!(!db.clip_handed_out(3).unwrap());
    let edits: Vec<_> = db
        .edits(crate::edits::tests::at_first())
        .unwrap()
        .into_iter()
        .map(|e| (e.title, e.status, e.revision, e.current))
        .collect();
    assert_eq!(edits, [("Best of".into(), ClipStatus::Ready, 4, true), ("Older".into(), ClipStatus::Ready, 5, false)]);
}

/// "Each VOD (a source) counts the pieces of clips that use it, its own
/// clip's included … A source at zero is unused: no entry lists it and no
/// clip holds any of it" (db.rs `MIGRATE_V24`): counted as it upgrades, then
/// kept as clips change.
#[test]
fn a_vod_used_by_an_edit_counts_its_pieces_after_the_upgrade() {
    let (_dir, path) = library("migrate-v24");
    at_version(&path, 23)
        .execute_batch(&format!(
            "INSERT INTO vods (id, platform, platform_id, url, title, duration, status, added_at) VALUES
                 (1, 'kick', 'a', 'u', 'Used', 600.0, 'done', 0), (2, 'kick', 'b', 'u', 'Alone', 600.0, 'done', 0),
                 (3, 'kick', 'c', 'u', 'Clipped', 600.0, 'done', 0);
             INSERT INTO clips (id, title, created_at, position, bar_of, bar_position, whole_of) VALUES
                 (1, 'Used', 0, 0, NULL, NULL, 1), (2, 'Alone', 0, 1, NULL, NULL, 2), (3, 'Clipped', 0, 2, NULL, NULL, 3),
                 (4, 'Best of', 0, 3, NULL, NULL, NULL), (5, 'Snippet', 0, NULL, 3, 1, NULL);
             INSERT INTO renders (clip_id) VALUES (1), (2), (3), (4), (5);
             INSERT INTO items (clip_id, position, kind, vod_id, start, end) VALUES
                 (1, 0, 'piece', 1, 0, NULL), (2, 0, 'piece', 2, 0, NULL), (3, 0, 'piece', 3, 0, NULL),
                 (4, 0, 'piece', 1, 0, {one}), (4, 1, 'piece', 1, {two}, {three}), (5, 0, 'piece', 3, 0, {one});",
            one = flicks(1.0),
            two = flicks(2.0),
            three = flicks(3.0)
        ))
        .unwrap();
    let db = upgraded(&path);
    assert_eq!([1, 2, 3].map(|vod| db.uses(vod).unwrap()), [3, 1, 2], "its clip, the edit's pieces, the snippet");
    assert_eq!(db.unused_sources().unwrap(), Vec::<i64>::new());
    assert!(db.remove_entry(2).unwrap(), "nothing else holds it");
    assert!(!db.remove_entry(1).unwrap(), "the edit holds it");
    assert_eq!(db.unused_sources().unwrap(), [2]);
    assert!(db.delete_edit(4).unwrap());
    assert_eq!(db.uses(1).unwrap(), 0);
    assert!(db.remove_entry(3).unwrap(), "its snippet went with its clip");
    assert_eq!(db.unused_sources().unwrap(), [1, 2, 3]);
}

/// "A source can be gone (`gone`): its files, transcript and chat erased
/// while clips still have pieces of it" (db.rs `MIGRATE_V25`; 0.5.0:
/// "Removing a VOD that an edit uses leaves the edit's pieces of it where
/// they are"): none is, from before.
#[test]
fn no_source_from_before_is_gone() {
    let (_dir, path) = library("migrate-v25");
    at_version(&path, 24)
        .execute_batch(&format!(
            "INSERT INTO vods (id, platform, platform_id, url, title, duration, status, added_at) VALUES
                 (1, 'kick', 'a', 'u', 'First', 600.0, 'done', 0), (2, 'kick', 'b', 'u', 'Second', 600.0, 'done', 0);
             INSERT INTO clips (id, title, created_at, position, whole_of) VALUES
                 (1, 'First', 0, 0, 1), (2, 'Second', 0, 1, 2), (3, 'Best of', 0, 2, NULL);
             INSERT INTO renders (clip_id) VALUES (1), (2), (3);
             INSERT INTO items (clip_id, position, kind, vod_id, start, end) VALUES
                 (1, 0, 'piece', 1, 0, NULL), (2, 0, 'piece', 2, 0, NULL), (3, 0, 'piece', 1, 0, {one}),
                 (3, 1, 'piece', 2, 0, {one});",
            one = flicks(1.0)
        ))
        .unwrap();
    let db = upgraded(&path);
    assert_eq!([1, 2].map(|vod| db.is_gone(vod).unwrap()), [false, false]);
    assert!(db.vods().unwrap().iter().all(|v| v.listed));
    assert_eq!(db.edit_pieces(3).unwrap().iter().map(|p| p.vod_id).collect::<Vec<_>>(), [1, 2]);
}

/// V26 keeps "what a clip's rendered file was made of … so a change that
/// leaves those as they were (a highlight, a cut) doesn't leave it out of
/// date" (db.rs `MIGRATE_V26`; 0.5.0: "A rendered edit has its chapters").
/// Decided for 0.7.4: one rendered before has none kept, and reads as up to
/// date till it's changed.
#[test]
fn an_edit_rendered_before_made_of_was_kept_reads_as_up_to_date_till_changed() {
    let (_dir, path) = library("migrate-v26");
    at_version(&path, 25)
        .execute_batch(&format!(
            "INSERT INTO vods (id, platform, platform_id, url, title, duration, status, added_at) VALUES
                 (1, 'kick', 'a', 'u', 'First', 600.0, 'done', 0);
             INSERT INTO clips (id, title, created_at, revision, position, whole_of) VALUES
                 (1, 'First', 0, 0, 0, 1), (2, 'Best of', 0, 3, 1, NULL), (3, 'Older', 0, 4, 2, NULL);
             INSERT INTO renders (clip_id, status, file_path, revision) VALUES
                 (1, 'waiting', NULL, NULL), (2, 'ready', '/v/2.mp4', 3), (3, 'ready', '/v/3.mp4', 2);
             INSERT INTO items (clip_id, position, kind, vod_id, start, end) VALUES
                 (1, 0, 'piece', 1, 0, NULL), (2, 0, 'piece', 1, 0, {ten}), (3, 0, 'piece', 1, 0, {ten});",
            ten = flicks(10.0)
        ))
        .unwrap();
    let db = upgraded(&path);
    let now = crate::edits::tests::at_first();
    let current = |id| db.edit(id, now).unwrap().unwrap().current;
    assert!(current(2), "rendered at the revision it's at");
    assert!(!current(3), "changed since it was rendered, as before");
    db.rename_edit(2, "Best of all").unwrap();
    assert!(!current(2), "changed");
}

/// "Each VOD's transcription so far becomes stretches: a finished one, done
/// from 0 to the end; one under way, waiting, paused or failed, a request for
/// all of it in that state (queued ones were added when adding meant
/// transcribing); a stopped one, done as far as its lines go, or nothing"
/// (db.rs `MIGRATE_V27`; 0.6.0: "Adding a VOD doesn't transcribe it."; e2e
/// upgrade-0.5.0.mjs header).
#[test]
fn transcriptions_so_far_become_stretches() {
    let (_dir, path) = library("migrate-v27");
    at_version(&path, 26)
        .execute_batch(&format!(
            "INSERT INTO vods (id, platform, platform_id, url, title, duration, status, progress, error, model, added_at) VALUES
                 (1, 'kick', 'done', 'u', 't', 600, 'done', 1, NULL, '{model}', 0),
                 (2, 'kick', 'paused', 'u', 't', 600, 'paused', 0.2, NULL, '{model}', 0),
                 (3, 'kick', 'queued', 'u', 't', 600, 'queued', 0, NULL, NULL, 0),
                 (4, 'kick', 'stopped', 'u', 't', 600, 'error', 0, 'Stopped', NULL, 0),
                 (5, 'kick', 'failed', 'u', 't', 600, 'error', 0, 'HTTP 404', NULL, 0),
                 (6, 'kick', 'running', 'u', 't', 600, 'transcribing', 0.5, NULL, '{model}', 0),
                 (7, 'kick', 'stopped-lines', 'u', 't', 600, 'error', 0, 'Stopped', '{model}', 0),
                 (8, 'kick', 'waiting', 'u', 't', 600, 'ready', 0, NULL, NULL, 0),
                 (9, 'kick', 'downloading', 'u', 't', 600, 'downloading', 0.3, NULL, NULL, 0);
             INSERT INTO segments (vod_id, start, end, text, words) VALUES
                 (1, 0, 5, 'a', '[]'), (2, 100, 120, 'b', '[]'), (6, 280, 300, 'c', '[]'), (7, 40, 50, 'd', '[]');",
            model = crate::model::CPU_MODEL.id
        ))
        .unwrap();
    let db = upgraded(&path);
    let vod = |id| db.vod(id).unwrap().unwrap();
    let stretches = |id| -> Vec<_> {
        db.stretches(id).unwrap().into_iter().map(|s| (s.start, s.end, s.status, s.reached)).collect()
    };
    // Finished.
    assert_eq!(stretches(1), [(0.0, None, Status::Done, None)]);
    assert_eq!((vod(1).status, vod(1).transcribed, vod(1).outdated), (Status::Done, Transcribed::All, false));
    assert_eq!(db.stretches(1).unwrap()[0].model.as_deref(), Some(crate::model::CPU_MODEL.id));
    // Under way: downloading, or transcribing as far as its lines go.
    assert_eq!(stretches(9), [(0.0, None, Status::Downloading, None)]);
    assert_eq!(stretches(6), [(0.0, None, Status::Transcribing, Some(300.0))]);
    // Waiting.
    assert_eq!(stretches(8), [(0.0, None, Status::Ready, None)]);
    assert_eq!(stretches(3), [(0.0, None, Status::Queued, None)], "added when adding meant transcribing");
    assert_eq!((vod(3).status, vod(3).transcribed), (Status::Queued, Transcribed::None));
    // Paused.
    assert_eq!(stretches(2), [(0.0, None, Status::Paused, Some(120.0))]);
    assert_eq!((vod(2).status, vod(2).progress, vod(2).transcribed), (Status::Paused, 0.2, Transcribed::Some));
    // Failed: the request, with its error.
    assert_eq!(stretches(5), [(0.0, None, Status::Error, None)]);
    assert_eq!(db.stretches(5).unwrap()[0].error.as_deref(), Some("HTTP 404"));
    assert_eq!((vod(5).status, vod(5).error.as_deref()), (Status::Error, Some("HTTP 404")));
    // Stopped: done as far as its lines go, or nothing.
    assert_eq!(stretches(7), [(0.0, Some(50.0), Status::Done, None)], "stopped, its lines kept");
    assert_eq!((vod(7).status, vod(7).transcribed), (Status::None, Transcribed::Some));
    assert_eq!(stretches(4), [], "stopped, with nothing to keep");
    assert_eq!((vod(4).status, vod(4).error), (Status::None, None));
}

/// Chat from before is searchable by who wrote it: "Chat's authors indexed,
/// any case, for `from:` and its suggestions" (db.rs `MIGRATE_V28`; 0.6.0:
/// "Search learns from, in, within, after, before.";
/// tests/e2e/fixtures/upgrade-matrix.mjs header: "the author index of format
/// 28 is filled from the old rows").
#[test]
fn chat_from_before_is_searchable_by_who_wrote_it() {
    let (_dir, path) = library("migrate-v28");
    at_version(&path, 27)
        .execute_batch(
            "INSERT INTO vods (id, platform, platform_id, url, title, channel, duration, status, added_at) VALUES
                 (1, 'twitch', '1', 'u', 't', 'Streamer', 600, 'done', 0);
             INSERT INTO clips (id, title, created_at, position, whole_of) VALUES (1, 't', 0, 0, 1);
             INSERT INTO renders (clip_id) VALUES (1);
             INSERT INTO items (clip_id, position, kind, vod_id, start, end) VALUES (1, 0, 'piece', 1, 0, NULL);
             INSERT INTO segments (vod_id, start, end, text, words) VALUES
                 (1, 0, 1, 'Clutch play', '[]'), (1, 5, 6, 'what a play', '[]');
             INSERT INTO chat (vod_id, time, author, text, parts) VALUES
                 (1, 1, 'Bob', 'PLAY', '[]'), (1, 2, 'bobby', 'hi', '[]'), (1, 3, 'alice', 'hi', '[]');",
        )
        .unwrap();
    let db = upgraded(&path);
    let found = |query: &str| -> Vec<(Source, Option<String>, f64)> {
        let hits = db.search(query, &SearchFilter::default(), 10).unwrap().hits;
        hits.into_iter().map(|h| (h.source, h.author, h.time)).collect()
    };
    for typed in ["from:bob", "from:BOB"] {
        assert_eq!(found(typed), [(Source::Chat, Some("Bob".into()), 1.0)], "{typed}");
    }
    assert_eq!(found("from:bob play"), [(Source::Chat, Some("Bob".into()), 1.0)]);
    assert_eq!(found("from:alice"), [(Source::Chat, Some("alice".into()), 3.0)]);
    let names: Vec<String> = db.author_suggestions("BO", None).unwrap().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["Bob", "bobby"]);
    // What was said, as before.
    let said: Vec<f64> = found("play").into_iter().filter(|h| h.0 == Source::Speech).map(|h| h.2).collect();
    assert_eq!(said.len(), 2, "{said:?}");
}

/// V29 adds "annotations imported from a file … A highlight's end, its kind …
/// and why it's one …, on its mark; who says a line (`speaker`), on the line;
/// a VOD's cast and what it does when" (db.rs `MIGRATE_V29`; 0.7.0: "A VOD
/// can come with annotations."): a library from before has none of them.
#[test]
fn a_library_from_before_annotations_has_none_imported() {
    let (_dir, path) = library("migrate-v29");
    at_version(&path, 28)
        .execute_batch(&format!(
            "INSERT INTO vods (id, platform, platform_id, url, title, duration, status, added_at) VALUES
                 (1, 'twitch', '1', 'u', 't', 600, 'done', 0);
             INSERT INTO clips (id, title, created_at, position, whole_of) VALUES (1, 't', 0, 0, 1);
             INSERT INTO renders (clip_id) VALUES (1);
             INSERT INTO items (clip_id, position, kind, vod_id, start, end) VALUES (1, 0, 'piece', 1, 0, NULL);
             INSERT INTO marks (id, clip_id, vod_id, kind, at, text, source, created_at) VALUES
                 (1, 1, 1, 'highlight', {five}, 'Big laugh', 'user', 0);
             INSERT INTO segments (vod_id, start, end, text, words) VALUES (1, 0, 1, 'hello', '[]');
             INSERT INTO stretches (vod_id, start, end, status, progress) VALUES (1, 0, NULL, 'done', 1);",
            five = flicks(5.0)
        ))
        .unwrap();
    let db = upgraded(&path);
    assert!(db.vod_annotations(1).unwrap().is_none(), "no cast or activity");
    let marks = db.marks(1).unwrap();
    assert_eq!((marks[0].text.as_deref(), &marks[0].detail), (Some("Big laugh"), &MarkDetail::default()));
    let lines: Vec<_> = db.lines(1).unwrap().into_iter().map(|l| (l.text, l.speaker)).collect();
    assert_eq!(lines, [("hello".into(), None)]);
}

/// A highlight from before casts names nobody; V30 adds "who is in an
/// imported highlight (`people`: its cast's names, as a JSON array), on its
/// mark" (db.rs `MIGRATE_V30`).
#[test]
fn a_highlight_from_before_casts_names_nobody() {
    let (_dir, path) = library("migrate-v30");
    let vod = {
        let db = Db::open(&path).unwrap();
        let (vod, _) = db.add_vod(&crate::edits::tests::info("abc")).unwrap();
        db.add_mark(vod.id, "highlight", 5.0, Some("Big laugh")).unwrap();
        vod.id
    };
    // Back to version 29: the column 30 adds, gone.
    back_to(&path, 29);
    let db = upgraded(&path);
    let marks = db.marks(vod).unwrap();
    assert_eq!((marks[0].text.as_deref(), marks[0].detail.cast.len()), (Some("Big laugh"), 0));
    // And a cast, once there is one, is kept and read back.
    let clip = db.vod_clip(vod).unwrap().unwrap();
    let detail = MarkDetail { cast: vec!["Marlowe".into(), "Ada".into()], ..MarkDetail::default() };
    db.insert_mark_with(clip, vod, "highlight", Flicks::from_seconds(9.0), None, &detail).unwrap();
    assert_eq!(db.marks(vod).unwrap()[1].detail.cast, ["Marlowe", "Ada"]);
}

/// A chapter from before icons has none; V31 adds "what an imported chapter
/// is about, as one emoji (`icon`), on its mark" (db.rs `MIGRATE_V31`).
#[test]
fn a_chapter_from_before_icons_has_none() {
    let (_dir, path) = library("migrate-v31");
    let vod = {
        let db = Db::open(&path).unwrap();
        let (vod, _) = db.add_vod(&crate::edits::tests::info("abc")).unwrap();
        db.add_mark(vod.id, "chapter", 5.0, Some("The game")).unwrap();
        vod.id
    };
    // Back to version 30: the column 31 adds, gone.
    back_to(&path, 30);
    let db = upgraded(&path);
    let marks = db.marks(vod).unwrap();
    assert_eq!((marks[0].text.as_deref(), marks[0].detail.icon.as_deref()), (Some("The game"), None));
    // And an icon, once there is one, is kept and read back.
    let clip = db.vod_clip(vod).unwrap().unwrap();
    let detail = MarkDetail { icon: Some("🎮".into()), ..MarkDetail::default() };
    db.insert_mark_with(clip, vod, "chapter", Flicks::from_seconds(9.0), None, &detail).unwrap();
    assert_eq!(db.marks(vod).unwrap()[1].detail.icon.as_deref(), Some("🎮"));
}

/// A VOD from before look-ups is unknown to the file host: whether
/// annotations are published for it is "unknown until asked" (db.rs
/// `MIGRATE_V32`).
#[test]
fn a_vod_from_before_look_ups_is_unknown_to_the_file_host() {
    let (_dir, path) = library("migrate-v32");
    let vod = {
        let db = Db::open(&path).unwrap();
        db.add_vod(&crate::edits::tests::info("abc")).unwrap().0.id
    };
    // Back to version 31: the columns 32 adds, gone.
    back_to(&path, 31);
    let db = upgraded(&path);
    assert_eq!(db.vod(vod).unwrap().unwrap().annotations, Availability::Unknown);
    let listed = db.vods().unwrap();
    assert_eq!(listed[0].annotations, Availability::Unknown);
}

/// A VOD from before its start was kept has none till its chat says: "A chat
/// always replaces the others ... Null when nothing says" (db.rs
/// `MIGRATE_V33`).
#[test]
fn a_vod_from_before_its_start_was_kept_has_none_till_its_chat_says() {
    let (_dir, path) = library("migrate-v33");
    let vod = {
        let db = Db::open(&path).unwrap();
        db.add_vod(&crate::edits::tests::info("abc")).unwrap().0.id
    };
    // Back to version 32: the columns 33 and 34 add, gone.
    back_to(&path, 32);
    let db = upgraded(&path);
    assert_eq!(db.vod(vod).unwrap().unwrap().info.started_at, None);
    db.set_chat_start(vod, 1790631005.0).unwrap();
    assert_eq!(db.vods().unwrap()[0].info.started_at.as_deref(), Some("2026-09-28T21:30:05Z"));
}

/// An import from before points of view has none: "Null for an import from
/// before: none known" (db.rs `MIGRATE_V34`).
#[test]
fn an_import_from_before_points_of_view_has_none() {
    let (_dir, path) = library("migrate-v34");
    let vod = {
        let db = Db::open(&path).unwrap();
        let vod = db.add_vod(&crate::edits::tests::info("abc")).unwrap().0.id;
        db.conn
            .execute(
                "INSERT INTO vod_annotations (vod_id, cast_members, activity, imported_at) VALUES (?1, '[]', '[]', 0)",
                [vod],
            )
            .unwrap();
        vod
    };
    // Back to version 33: the column 34 adds, gone.
    back_to(&path, 33);
    let db = upgraded(&path);
    let kept = db.vod_annotations(vod).unwrap().unwrap();
    assert!(kept.povs.is_empty() && kept.cast.is_empty());
}

/// "Where the VOD it was kept from is still in the library (keeping a later
/// stretch failed, so the VOD stayed), those come back from its rows": who
/// speaks each line, and what each chat message is, how much and when it
/// happened (db.rs `MIGRATE_V36`; 0.7.0 Fixes: "Kept stretches keep subs,
/// gifts and tips.").
#[test]
fn stretches_kept_while_their_vod_stays_get_back_their_speakers_and_chat_kinds() {
    let (_dir, path) = library("migrate-v36");
    let (vod, kept) = {
        let mut db = Db::open(&path).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        let word = |text: &str, start: f64, end: f64| Word { text: text.into(), start, end };
        let segments = [
            Segment { start: 9.0, end: 11.0, text: " a".into(), words: vec![word(" a", 10.5, 11.0)] },
            Segment { start: 11.0, end: 12.0, text: " hi".into(), words: vec![word(" hi", 11.0, 12.0)] },
        ];
        db.insert_segments(vod, &segments).unwrap();
        db.conn.execute("UPDATE segments SET speaker = 'S' || id WHERE vod_id = ?1", [vod]).unwrap();
        let chat = [
            event(12.0, "hello", Kind::Message, None, None),
            event(13.0, "resubbed", Kind::Sub, Some(2.0), Some(12.5)),
            event(14.0, "gift", Kind::Gift, Some(5.0), None),
        ];
        crate::db::insert_chat(&db.conn, vod, &chat).unwrap();
        // Kept with the copy that left columns out; its VOD stays, as when
        // keeping the next stretch failed and nothing was removed.
        let kept = db.add_realized(vod, 10.0, 15.0).unwrap();
        db.conn
            .execute_batch(&format!(
                "UPDATE segments SET speaker = NULL WHERE vod_id = {kept};
                 UPDATE chat SET kind = 'message', amount = NULL, event_time = NULL WHERE vod_id = {kept};"
            ))
            .unwrap();
        (vod, kept)
    };
    back_to(&path, 35);
    let db = upgraded(&path);

    let lines = db.lines(kept).unwrap();
    let speakers: Vec<Option<String>> = db.lines(vod).unwrap().into_iter().map(|l| l.speaker).collect();
    assert_eq!(lines.into_iter().map(|l| l.speaker).collect::<Vec<_>>(), speakers);
    let events = |kind| {
        db.chat_of_kind(kept, kind)
            .unwrap()
            .into_iter()
            .map(|(_, m)| (m.time, m.amount, m.event_time))
            .collect::<Vec<_>>()
    };
    assert_eq!(events(Kind::Sub), [(3.0, Some(2.0), Some(2.5))]);
    assert_eq!(events(Kind::Gift), [(4.0, Some(5.0), None)]);
    assert_eq!(events(Kind::Message).len(), 1);
}

/// A chat message from "fan" at `time`, of `kind`.
fn event(time: f64, text: &str, kind: Kind, amount: Option<f64>, event_time: Option<f64>) -> ChatMessage {
    ChatMessage { kind, amount, event_time, color: None, author: "fan".into(), ..message(time, "fan", text) }
}

/// Of the speakers and chat kinds a kept stretch was copied without, "where
/// the VOD is gone, so are they" (db.rs `MIGRATE_V36`): it keeps the lines
/// and messages it has, as it has them.
#[test]
fn a_kept_stretch_whose_vod_is_gone_keeps_what_it_has() {
    let (_dir, path) = library("migrate-v36-gone");
    let (orphan, lines, chat) = {
        let mut db = Db::open(&path).unwrap();
        let vod = db.add_vod(&info("y")).unwrap().0.id;
        db.insert_segments(vod, &[Segment { start: 0.0, end: 2.0, text: " tip".into(), words: Vec::new() }]).unwrap();
        db.conn.execute("UPDATE segments SET speaker = 'Marlowe' WHERE vod_id = ?1", [vod]).unwrap();
        let chat = [event(1.0, "tip", Kind::Tip, Some(100.0), None), event(2.0, "hi", Kind::Message, None, None)];
        crate::db::insert_chat(&db.conn, vod, &chat).unwrap();
        let orphan = db.add_realized(vod, 0.0, 5.0).unwrap();
        db.conn
            .execute_batch(&format!(
                "UPDATE segments SET speaker = NULL WHERE vod_id = {orphan};
                 UPDATE chat SET kind = 'message', amount = NULL WHERE vod_id = {orphan};"
            ))
            .unwrap();
        db.delete_vod(vod).unwrap();
        let lines: Vec<_> =
            db.lines(orphan).unwrap().into_iter().map(|l| (l.start, l.end, l.text, l.speaker)).collect();
        let chat: Vec<_> =
            db.chat(orphan).unwrap().into_iter().map(|m| (m.time, m.author, m.kind, m.amount, m.parts)).collect();
        (orphan, lines, chat)
    };
    assert_eq!((lines.len(), chat.len()), (1, 2));
    back_to(&path, 35);
    let db = upgraded(&path);
    let after: Vec<_> = db.lines(orphan).unwrap().into_iter().map(|l| (l.start, l.end, l.text, l.speaker)).collect();
    assert_eq!(after, lines);
    let after: Vec<_> =
        db.chat(orphan).unwrap().into_iter().map(|m| (m.time, m.author, m.kind, m.amount, m.parts)).collect();
    assert_eq!(after, chat);
}

/// A VOD from before channel logins has none: "Null for a VOD added before:
/// unknown" (db.rs `MIGRATE_V37`; 0.7.2 Fixes: "Every Twitch channel shows
/// its picture.").
#[test]
fn a_vod_from_before_channel_logins_has_none() {
    let (_dir, path) = library("migrate-v37");
    let before = {
        let db = Db::open(&path).unwrap();
        let info =
            VodInfo {
                platform: "twitch".into(), channel: Some("마를로".into()), ..crate::edits::tests::info("old")
            };
        db.add_vod(&info).unwrap().0.id
    };
    // Back to version 36: the column 37 adds, gone.
    back_to(&path, 36);
    let db = upgraded(&path);
    assert_eq!(db.vod(before).unwrap().unwrap().info.channel_login, None);
    let info = VodInfo {
        platform: "twitch".into(),
        channel: Some("마를로".into()),
        channel_login: Some("marlowe".into()),
        channel_url: None,
        ..crate::edits::tests::info("new")
    };
    let (added, _) = db.add_vod(&info).unwrap();
    assert_eq!(added.info.channel_login.as_deref(), Some("marlowe"));
    let listed = db.vods().unwrap();
    let shown = listed.iter().find(|v| v.id == added.id).unwrap();
    assert_eq!((shown.info.channel.as_deref(), shown.info.channel_login.as_deref()), (Some("마를로"), Some("marlowe")));
    // The page reads it beside the name.
    let json = serde_json::to_value(shown).unwrap();
    assert_eq!(json["channel_login"], "marlowe");
}

/// V38 keeps "the contexts an import names … Null for an import from before:
/// none known" (db.rs, the doc of `MIGRATE_V38`, which sits on
/// `MIGRATE_V40`'s constant).
#[test]
fn an_import_from_before_contexts_names_none() {
    let (_dir, path) = library("migrate-v38");
    let vod = {
        let db = Db::open(&path).unwrap();
        let mut file = crate::annotations::tests::file("youtube", "abcdefghijk");
        file.contexts = vec!["gaming".into()];
        let (vod, _) = db.import_annotations(&file, None).unwrap();
        assert_eq!(db.vod_annotations(vod.id).unwrap().unwrap().contexts, ["gaming"]);
        vod.id
    };
    // Back to version 37: the column 38 adds, gone.
    back_to(&path, 37);
    let db = upgraded(&path);
    let kept = db.vod_annotations(vod).unwrap().unwrap();
    assert_eq!((kept.contexts.len(), kept.cast.len()), (0, 1), "no contexts; the cast it had");
}

/// The first open: "The old columns still get the full path written beside
/// the extension, for 0.7.1 … and `library::settle` moves what they record
/// that isn't where it's built" (db.rs `MIGRATE_BUILT_PATHS`; decided for
/// 0.7.2: the old columns stay filled for 0.7.1): the upgrade moves no file,
/// and settle then finds it where it was recorded. (A second open moving
/// nothing is crates/core/tests/race_settle_mid_session.rs
/// `a_second_connection_moves_no_file_a_job_has`.)
#[test]
fn files_recorded_by_path_keep_their_paths_till_settle_moves_them() {
    let (_dir, paths) = library_in("migrate-v39");
    let vod = {
        let db = Db::open_in(&paths).unwrap();
        db.add_vod(&info("kept")).unwrap().0.id
    };
    // As 0.7.1 recorded a kept video: its path, in the library's folder for them.
    let video = paths.media().join("A talk [000001].mp4");
    std::fs::create_dir_all(paths.media()).unwrap();
    std::fs::write(&video, b"video").unwrap();
    back_to(&paths.db(), 38)
        .execute("UPDATE vods SET media_path = ?2 WHERE id = ?1", params![vod, video.to_string_lossy()])
        .unwrap();
    let db = upgraded_in(&paths);
    let built = paths.source_video(vod, "mp4");
    assert_eq!(std::fs::read(&video).unwrap(), b"video", "where it was recorded");
    assert!(!built.exists(), "not moved by the upgrade");
    crate::library::settle(&db).unwrap();
    assert_eq!(db.media_path(vod).unwrap(), Some(shown(&built)));
    assert_eq!(std::fs::read(&built).unwrap(), b"video");
}

/// V40 keeps the channel's page; "Null for a VOD added before, filled from
/// its info where that is still kept (`pipeline::fill_channels`)" (db.rs
/// `MIGRATE_V40`; 0.7.2: "Names open a streamer card.", "The channel over the
/// player links to its page on every site"): only the VOD's own info, only
/// what's missing.
#[test]
fn a_vod_from_before_channel_pages_has_its_page_filled_from_its_info_where_kept() {
    let (_dir, paths) = library_in("migrate-v40");
    let [kept, other, none] = {
        let db = Db::open_in(&paths).unwrap();
        ["kept", "other", "none"].map(|id| db.add_vod(&info(id)).unwrap().0.id)
    };
    // Back to version 39: the column 40 adds, gone. Two VODs' info still kept, one of another video.
    back_to(&paths.db(), 39);
    let info_of = |id: &str| {
        serde_json::json!({
            "extractor_key": "Youtube", "id": id, "webpage_url": format!("https://www.youtube.com/watch?v={id}"),
            "title": "t", "channel": "Quill", "uploader": "Quill", "uploader_id": "@quill",
            "uploader_url": "https://www.youtube.com/@quill"
        })
    };
    std::fs::create_dir_all(paths.work()).unwrap();
    std::fs::write(paths.source_info(kept), info_of("kept").to_string()).unwrap();
    std::fs::write(paths.source_info(other), info_of("not-this-one").to_string()).unwrap();
    let db = upgraded_in(&paths);
    assert!(db.vods().unwrap().iter().all(|v| v.info.channel_url.is_none()), "none known from before");

    crate::pipeline::fill_channels(&db, &paths, &db.vods().unwrap());
    let channel = |id| {
        let info = db.vod(id).unwrap().unwrap().info;
        (info.channel_login, info.channel_url)
    };
    assert_eq!(channel(kept), (Some("@quill".into()), Some("https://www.youtube.com/@quill".into())));
    assert_eq!(channel(other), (None, None), "another video's info");
    assert_eq!(channel(none), (None, None), "nothing kept, nothing known");

    // One added since keeps its own page; what's there stays.
    let page = "https://www.youtube.com/@own";
    let (added, _) = db.add_vod(&VodInfo { channel_url: Some(page.into()), ..info("added") }).unwrap();
    std::fs::write(paths.source_info(added.id), info_of("added").to_string()).unwrap();
    crate::pipeline::fill_channels(&db, &paths, &db.vods().unwrap());
    assert_eq!(channel(added.id), (Some("@quill".into()), Some(page.into())), "only the login was missing");
    let json = serde_json::to_value(db.vod(added.id).unwrap().unwrap()).unwrap();
    assert_eq!(json["channel_url"], page);
}

/// V41 keeps the codec and quality a clip's (or edit's) file was made with;
/// "Null for a file made before: known only by its extension" (db.rs
/// `MIGRATE_V41`; 0.7.2: "Clips come in more formats."): such a file reads as
/// its format was made then (settings.rs `clip_encoding`, "the format's
/// codec, H.264 at CRF 18 or DNxHR LB"), so it's made again only when the
/// codec or quality chosen now isn't that.
#[test]
fn a_clip_made_before_encodings_were_recorded_reads_as_its_format_was_made_then() {
    use crate::settings::{ClipCodec, ClipFormat, ClipQuality, Settings};
    let (_dir, path) = library("migrate-v41");
    let (clip, edit) = {
        let db = Db::open(&path).unwrap();
        let vod = db.add_vod(&info("a")).unwrap().0.id;
        let clip = db.add_clip(vod, 1.0, 2.0, "c", true).unwrap().id;
        let file = shown(&db.files().clip_file(clip, "mp4"));
        db.set_clip_file(clip, 1.0, 2.0, ClipStatus::Ready, Some(&file), None).unwrap();
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (0.0, 10.0), "a", None).unwrap();
        (clip, edit)
    };
    // Back to version 40, the edit rendered as a MOV as that version recorded it.
    let edit_file = shown(&crate::Paths::new(path.parent().unwrap()).clip_file(edit, "mov"));
    back_to(&path, 40)
        .execute(
            "UPDATE renders SET status = 'ready', file_ext = 'mov', file_path = ?2,
                 revision = (SELECT revision FROM clips WHERE id = ?1) WHERE clip_id = ?1",
            params![edit, edit_file],
        )
        .unwrap();
    let db = upgraded(&path);
    let then = |clip_format| Settings { clip_format, ..Settings::default() }.clip_encoding();
    let made = db.clip(clip).unwrap().unwrap();
    assert_eq!(made.encoding, None, "not known");
    let file = made.file_path.unwrap();
    assert!(crate::clips::made_as(Path::new(&file), made.encoding, then(ClipFormat::Mp4)));
    let h265 = ClipEncoding { codec: ClipCodec::H265, quality: ClipQuality::Standard };
    assert!(!crate::clips::made_as(Path::new(&file), made.encoding, h265), "made again in another codec");
    assert!(db.edit(edit, then(ClipFormat::Mov)).unwrap().unwrap().current);
    let prores = ClipEncoding { codec: ClipCodec::Prores, quality: ClipQuality::Standard };
    assert!(!db.edit(edit, prores).unwrap().unwrap().current, "made again in another codec");
}

/// An upgraded library never gives a deleted clip's id again: "a deleted
/// clip's id is never given to a new one, as a VOD's isn't (v14), so what
/// still holds it (an undo step, the clipboard, the page, the clip's picture
/// and files, all named by it) can't reach the new clip" (db.rs
/// `MIGRATE_V42`; 0.7.3 Fixes: "Undo no longer changes another clip."). Edits
/// and pasted clips are clips too. A library from before upgrades with all it
/// had, as it had it.
#[test]
fn an_upgraded_library_never_gives_a_deleted_clips_id_again() {
    use crate::clipboard::Paste;
    use crate::edits::Put;
    let (_dir, path) = library("migrate-v42");
    // Every row of the tables that hold clips and what's theirs.
    let rows = |conn: &Connection| -> Vec<Vec<rusqlite::types::Value>> {
        let mut all = Vec::new();
        for table in ["clips", "items", "renders", "marks"] {
            let mut stmt = conn.prepare(&format!("SELECT * FROM {table} ORDER BY rowid")).unwrap();
            let n = stmt.column_count();
            let found = stmt.query_map([], |r| (0..n).map(|i| r.get(i)).collect()).unwrap();
            all.extend(found.map(|row| row.unwrap()));
        }
        all
    };
    let (vod, before) = {
        let db = Db::open(&path).unwrap();
        let vod = db.add_vod(&info("a")).unwrap().0.id;
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (0.0, 10.0), "a", None).unwrap();
        db.add_clip(vod, 1.0, 2.0, "a", true).unwrap();
        db.add_mark(vod, "cut", 1.5, None).unwrap();
        let newest = db.add_clip(vod, 3.0, 4.0, "b", true).unwrap().id;
        db.delete_clip(newest).unwrap();
        (vod, rows(&db.conn))
    };
    // Back to version 41, before clips kept their ids.
    back_to(&path, 41);
    let db = upgraded(&path);
    assert_eq!(rows(&db.conn), before, "upgraded, the same clips, pieces, files and marks");

    let a = db.add_clip(vod, 5.0, 6.0, "c", true).unwrap().id;
    db.delete_clip(a).unwrap();
    let b = db.add_clip(vod, 5.0, 6.0, "c", true).unwrap().id;
    assert!(b > a, "the deleted clip's id {a} isn't given out again");
    let e = db.add_edit("f").unwrap().id;
    assert!(db.delete_edit(e).unwrap());
    let f = db.add_edit("f").unwrap().id;
    assert!(f > e, "nor a deleted edit's");
    let copied = db.copy_of(&Put::Clip { id: b }).unwrap();
    assert!(db.delete_edit(f).unwrap());
    let bar = db.vod_clip(vod).unwrap().unwrap();
    let pasted = db.paste(Paste::Bar { bar, after: None }, &[copied]).unwrap().clips[0].0;
    assert!(pasted > f, "nor to a clip pasted");
    // Even after a clip added without taking an id (as an older version would).
    db.conn.execute("INSERT INTO clips (title, created_at) VALUES ('older', 0)", []).unwrap();
    let older = db.conn.last_insert_rowid();
    db.delete_clip(older).unwrap();
    assert!(db.add_clip(vod, 7.0, 8.0, "d", true).unwrap().id > older);
}

/// V43 records which make of a clip its file is; "Null for one made before,
/// at the path its id alone builds, which it keeps; so nothing is moved"
/// (db.rs `MIGRATE_V43`; 0.7.3 Fixes: "A file another app took stays.").
#[test]
fn a_clip_made_before_v43_keeps_its_file_where_it_is() {
    let (_dir, path) = library("migrate-v43");
    let clip = {
        let db = Db::open(&path).unwrap();
        let vod = db.add_vod(&info("a")).unwrap().0.id;
        let clip = db.add_clip(vod, 1.0, 2.0, "c", true).unwrap().id;
        let file = db.files().clip_file(clip, "mp4");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"made before").unwrap();
        db.set_clip_file(clip, 1.0, 2.0, ClipStatus::Ready, Some(&shown(&file)), None).unwrap();
        clip
    };
    // Back to version 42: which make it is, gone.
    back_to(&path, 42);
    let db = upgraded(&path);
    let file = db.files().clip_file(clip, "mp4");
    let made = db.clip(clip).unwrap().unwrap();
    assert_eq!((made.status, made.file_path), (ClipStatus::Ready, Some(shown(&file))));
    assert_eq!(std::fs::read(&file).unwrap(), b"made before");
    let files: Vec<_> = std::fs::read_dir(db.files().clips()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(files, [file.file_name().unwrap()], "nothing moved or made");
}

/// V44: the stretches' `note` (v27), which nothing ever read or wrote, goes
/// (`MIGRATE_V44`), and a library from before upgrades with every stretch as
/// it was: done, asked for, paused partway, failed, with its audio (old
/// schema and rows in, the same data out).
#[test]
fn the_stretches_unread_note_goes_and_every_stretch_stays_as_it_was() {
    let (_dir, path) = library("stretch-note");
    let columns = |conn: &Connection| -> Vec<String> {
        let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('stretches')").unwrap();
        stmt.query_map([], |r| r.get(0)).unwrap().map(|c| c.unwrap()).collect()
    };
    let (vod, before, shown) = {
        let db = Db::open(&path).unwrap();
        assert!(!columns(&db.conn).contains(&"note".to_string()), "a new library's: {:?}", columns(&db.conn));
        let vod = db.add_vod(&info("a")).unwrap().0.id;
        db.insert_done(vod, 0.0, Some(100.0), Some("large")).unwrap();
        let paused = db.request(vod, 100.0, Some(200.0)).unwrap().id;
        db.set_stretch_status(paused, Status::Paused, 0.4, None).unwrap();
        db.set_reached(paused, Some(140.0)).unwrap();
        db.set_stretch_model(paused, Some("large")).unwrap();
        let failed = db.request(vod, 200.0, Some(300.0)).unwrap().id;
        db.set_stretch_status(failed, Status::Error, 0.0, Some("worker gone")).unwrap();
        let queued = db.request(vod, 300.0, None).unwrap().id;
        let audio = db.files.source_section(vod, queued, "m4a");
        db.set_stretch_audio(queued, Some(audio.to_str().unwrap())).unwrap();
        let shown = serde_json::to_value(db.vod(vod).unwrap().unwrap()).unwrap();
        (vod, db.stretches(vod).unwrap(), shown)
    };
    assert_eq!(before.len(), 4);
    // Back to version 43, the column there, holding something on one row.
    back_to(&path, 43).execute("UPDATE stretches SET note = 'old' WHERE start = 100", []).unwrap();
    let db = upgraded(&path);
    assert_eq!(db.stretches(vod).unwrap(), before, "upgraded, the same stretches");
    assert_eq!(serde_json::to_value(db.vod(vod).unwrap().unwrap()).unwrap(), shown, "and the VOD shown the same");
    assert!(!columns(&db.conn).contains(&"note".to_string()), "upgraded: {:?}", columns(&db.conn));
}

/// A library from before video heights were kept (`MIGRATE_V45`) has no video
/// download for a start to carry on, "Null for a VOD from before: nothing was
/// asked that a start could know of", and its VODs read as they were; one
/// asked for after the upgrade is kept with its height. Decided for 0.7.4: a
/// whole video's download waiting at a quit carries on at the next start;
/// this is its migration (old schema and rows in, the same data out).
#[test]
fn a_library_from_before_video_heights_has_no_video_download_to_carry_on() {
    let (_dir, path) = library("video-height");
    let (vod, shown) = {
        let db = Db::open(&path).unwrap();
        let vod = db.add_vod(&info("a")).unwrap().0.id;
        db.set_video_wanted(vod, Some(720)).unwrap();
        (vod, serde_json::to_value(db.vod(vod).unwrap().unwrap()).unwrap())
    };
    back_to(&path, 44);
    let db = upgraded(&path);
    assert_eq!(db.wanted_videos().unwrap(), [], "a download asked for before is known of");
    assert_eq!(serde_json::to_value(db.vod(vod).unwrap().unwrap()).unwrap(), shown, "the VOD shown otherwise");
    db.set_video_wanted(vod, Some(480)).unwrap();
    assert_eq!(db.wanted_videos().unwrap(), [(vod, 480)]);
}

/// Struck stretches become stretches taken out (V46). Decided for 0.7.4:
/// struck lines are like deleted video, so the upgrade turns each into a
/// stretch taken out, its piece split there. Each spec builds a library
/// with this version, takes it back to 0.7.3's format (43), strikes
/// stretches of its pieces as 0.4.x did, and opens it.
mod struck_stretches_become_splits {
    use super::*;
    use crate::edits::EditPiece;

    /// What 0.7.3 played of `start`..`end` less the `struck` stretches (its
    /// `edits::spans`, kept here as the rule the upgrade must play the same
    /// as): what's between them, slivers left out.
    fn played_before(start: f64, end: f64, struck: &[(f64, f64)]) -> Vec<(f64, f64)> {
        let mut cuts: Vec<(f64, f64)> = struck.iter().copied().filter(|(a, b)| b > a).collect();
        cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out = Vec::new();
        let mut at = start;
        for (a, b) in cuts {
            if a > at {
                out.push((at, a.min(end)));
            }
            at = at.max(b);
            if at >= end {
                break;
            }
        }
        if at < end {
            out.push((at, end));
        }
        out.retain(|(a, b)| b - a >= 0.04);
        out
    }

    /// A library made by `build` (given VOD `edits::tests::info("x")`, 600 s long), taken
    /// back to format 43 (0.7.3's) with the `strikes` (pieces' struck stretches, in
    /// seconds, from what `build` gave), then opened by this version.
    fn struck<T>(
        name: &str,
        build: impl FnOnce(&Db, i64) -> T,
        strikes: impl FnOnce(&T) -> Vec<(i64, Vec<(f64, f64)>)>,
    ) -> (crate::TempDir, Db, i64, T) {
        let (dir, path) = library(name);
        let (vod, built) = {
            let db = Db::open(&path).unwrap();
            let vod = db.add_vod(&crate::edits::tests::info("x")).unwrap().0.id;
            (vod, build(&db, vod))
        };
        let conn = back_to(&path, 43);
        for (piece, stretches) in strikes(&built) {
            let flicked: Vec<(i64, i64)> = stretches.iter().map(|&(a, b)| (flicks(a), flicks(b))).collect();
            let json = serde_json::to_string(&flicked).unwrap();
            conn.execute("UPDATE items SET struck = ?2 WHERE id = ?1", params![piece, json]).unwrap();
        }
        drop(conn);
        (dir, upgraded(&path), vod, built)
    }

    /// Each piece of clip `id`: its id, stretch and title.
    fn pieces(db: &Db, id: i64) -> Vec<(i64, f64, f64, String)> {
        db.edit_pieces(id).unwrap().into_iter().map(|p| (p.id, p.start, p.end, p.title)).collect()
    }

    /// What clip `id` plays, in order: each VOD's stretches.
    fn plays(db: &Db, id: i64) -> Vec<(i64, f64, f64)> {
        let pieces: Vec<EditPiece> = db.edit_pieces(id).unwrap();
        crate::edits::played(&pieces).map(|s| (s.piece.vod_id, s.from, s.to)).collect()
    }

    /// `played_before`'s stretches, of VOD `vod`.
    fn of(vod: i64, stretches: Vec<(f64, f64)>) -> Vec<(i64, f64, f64)> {
        stretches.into_iter().map(|(a, b)| (vod, a, b)).collect()
    }

    /// Puts `start`..`end` of `vod` last in clip `clip`, titled `title`: its id.
    fn add(db: &Db, clip: i64, vod: i64, (start, end): (f64, f64), title: &str) -> i64 {
        db.add_piece(clip, vod, (start, end), title, None).unwrap();
        db.item_rows(clip).unwrap().into_iter().map(|i| i.id).max().unwrap()
    }

    /// "Delete then takes the selected lines' stretch out, as any stretch
    /// taken out: the piece splits in two there, each part its own band"
    /// (CHANGELOG.md, 0.5.0): a piece with a stretch struck is two pieces in
    /// its place, with its title, that play what it played.
    #[test]
    fn a_piece_with_a_struck_stretch_becomes_two_pieces_that_play_the_same() {
        let (_dir, db, vod, (edit, a, b)) = struck(
            "struck-one",
            |db, vod| {
                let edit = db.add_edit("Best of").unwrap().id;
                (edit, add(db, edit, vod, (10.0, 20.0), "a"), add(db, edit, vod, (30.0, 35.0), "b"))
            },
            |&(_, a, _)| vec![(a, vec![(12.0, 14.0)])],
        );
        let now = pieces(&db, edit);
        assert_eq!(now.len(), 3, "{now:?}");
        assert_eq!(now, [(a, 10.0, 12.0, "a".into()), (now[1].0, 14.0, 20.0, "a".into()), (b, 30.0, 35.0, "b".into())]);
        let mut before = played_before(10.0, 20.0, &[(12.0, 14.0)]);
        before.push((30.0, 35.0));
        assert_eq!(plays(&db, edit), of(vod, before));
        // The column stays, emptied, for 0.5.0 to 0.7.3, which read it (`MIGRATE_V46`).
        let struck: i64 =
            db.conn.query_row("SELECT count(*) FROM items WHERE struck != '[]'", [], |r| r.get(0)).unwrap();
        assert_eq!(struck, 0);
    }

    /// As a split does (`Db::split_piece`), both parts of a realized piece
    /// (sources.rs `realize_piece`) keep where it came from.
    #[test]
    fn a_realized_pieces_parts_keep_where_it_came_from() {
        let (_dir, db, _vod, (edit, a)) = struck(
            "struck-origin",
            |db, vod| {
                let edit = db.add_edit("Best of").unwrap().id;
                let a = add(db, edit, vod, (10.0, 20.0), "a");
                db.conn.execute("UPDATE items SET origin = ?2 WHERE id = ?1", params![a, flicks(100.0)]).unwrap();
                (edit, a)
            },
            |&(_, a)| vec![(a, vec![(12.0, 14.0)])],
        );
        let rows = db.piece_rows(edit).unwrap();
        assert_eq!(rows.iter().map(|p| p.origin).collect::<Vec<_>>(), [Some(Flicks::from_seconds(100.0)); 2]);
        assert_eq!(rows[0].id, a);
    }

    /// Several stretches struck, in any order and overlapping: a piece for
    /// each stretch left between them, a sliver between two left out, as it
    /// played (db.rs `struck_to_splits`: "the piece split at its ends, what's
    /// between gone").
    #[test]
    fn several_struck_stretches_split_a_piece_at_each_one() {
        let strikes = [(25.0, 27.0), (5.0, 8.0), (26.0, 30.0), (10.0, 12.0), (12.02, 14.0)];
        let (_dir, db, vod, (edit, a)) = struck(
            "struck-several",
            |db, vod| {
                let edit = db.add_edit("Best of").unwrap().id;
                (edit, add(db, edit, vod, (0.0, 40.0), "a"))
            },
            |&(_, a)| vec![(a, strikes.to_vec())],
        );
        let now = pieces(&db, edit);
        let stretches: Vec<_> = now.iter().map(|(_, s, e, t)| (*s, *e, t.as_str())).collect();
        assert_eq!(stretches, [(0.0, 5.0, "a"), (8.0, 10.0, "a"), (14.0, 25.0, "a"), (30.0, 40.0, "a")]);
        assert_eq!(plays(&db, edit), of(vod, played_before(0.0, 40.0, &strikes)));
        assert_eq!(now[0].0, a, "the first part is the piece");
    }

    /// A stretch struck at a piece's start or end trims it, as one taken out
    /// there does ("the pieces it reaches are trimmed, or split at its ends",
    /// commit 29146934), and a sliver left at an edge goes with it.
    #[test]
    fn a_stretch_struck_at_a_pieces_start_or_end_trims_it() {
        let (a_struck, b_struck) = ([(10.0, 12.0), (19.0, 20.0)], [(32.0, 39.98)]);
        let (_dir, db, vod, (edit, a, b)) = struck(
            "struck-edges",
            |db, vod| {
                let edit = db.add_edit("Best of").unwrap().id;
                (edit, add(db, edit, vod, (10.0, 20.0), "a"), add(db, edit, vod, (30.0, 40.0), "b"))
            },
            |&(_, a, b)| vec![(a, a_struck.to_vec()), (b, b_struck.to_vec())],
        );
        assert_eq!(pieces(&db, edit), [(a, 12.0, 19.0, "a".into()), (b, 30.0, 32.0, "b".into())]);
        let mut before = played_before(10.0, 20.0, &a_struck);
        before.extend(played_before(30.0, 40.0, &b_struck));
        assert_eq!(plays(&db, edit), of(vod, before));
    }

    /// A piece struck whole, or but for a sliver, played nothing: it goes,
    /// and a group it leaves empty goes too, as `remove_edit_stretch` takes
    /// away groups left empty.
    #[test]
    fn a_wholly_struck_piece_goes_with_a_group_it_leaves_empty() {
        let (_dir, db, vod, (edit, gone, kept)) = struck(
            "struck-whole",
            |db, vod| {
                let edit = db.add_edit("Best of").unwrap().id;
                let whole = add(db, edit, vod, (10.0, 20.0), "whole");
                let sliver = add(db, edit, vod, (30.0, 40.0), "sliver");
                let snippet = db.add_clip(vod, 50.0, 55.0, "g", true).unwrap().id;
                db.add_group(edit, snippet, None).unwrap();
                let grouped = db.item_rows(edit).unwrap().into_iter().map(|i| i.id).max().unwrap();
                (edit, [whole, sliver, grouped], add(db, edit, vod, (60.0, 65.0), "kept"))
            },
            |&(_, [whole, sliver, grouped], _)| {
                vec![(whole, vec![(9.0, 21.0)]), (sliver, vec![(30.02, 40.0)]), (grouped, vec![(50.0, 55.0)])]
            },
        );
        assert!(gone.iter().all(|id| db.piece_row(*id).unwrap().is_none()));
        assert_eq!(pieces(&db, edit), [(kept, 60.0, 65.0, "kept".into())]);
        assert_eq!(db.item_rows(edit).unwrap().len(), 1, "the group went too");
        assert_eq!(plays(&db, edit), [(vod, 60.0, 65.0)]);
    }

    /// A piece inside a group (a clip dropped in) is split where it is: its
    /// parts in the group, in order, before what came after it there.
    #[test]
    fn a_grouped_pieces_parts_stay_in_its_group_in_order() {
        let (_dir, db, vod, (edit, _)) = struck(
            "struck-group",
            |db, vod| {
                let edit = db.add_edit("Best of").unwrap().id;
                add(db, edit, vod, (0.0, 5.0), "x");
                let snippet = db.add_clip(vod, 10.0, 20.0, "g", true).unwrap().id;
                db.add_piece(snippet, vod, (40.0, 45.0), "g2", None).unwrap();
                db.add_group(edit, snippet, None).unwrap();
                add(db, edit, vod, (70.0, 75.0), "y");
                (edit, db.edit_pieces(edit).unwrap()[1].id)
            },
            |&(_, grouped)| vec![(grouped, vec![(12.0, 14.0)])],
        );
        let now: Vec<_> =
            db.edit_pieces(edit).unwrap().into_iter().map(|p| (p.start, p.end, p.group.map(|g| g.title))).collect();
        let g = || Some("g".to_string());
        assert_eq!(
            now,
            [(0.0, 5.0, None), (10.0, 12.0, g()), (14.0, 20.0, g()), (40.0, 45.0, g()), (70.0, 75.0, None)]
        );
        assert_eq!(db.item_rows(edit).unwrap().iter().filter(|i| i.vod_id.is_none()).count(), 1, "one group");
        let before = vec![(0.0, 5.0), (10.0, 12.0), (14.0, 20.0), (40.0, 45.0), (70.0, 75.0)];
        assert_eq!(plays(&db, edit), of(vod, before));
    }

    /// Where clip `id` shows its marks: each kind, title and start.
    fn shown(db: &Db, id: i64) -> Vec<(String, Option<String>, f64)> {
        crate::edits::place_marks(&db.edit_pieces(id).unwrap(), &db.clip_marks(id).unwrap())
            .into_iter()
            .map(|m| (m.kind, m.text, m.start))
            .collect()
    }

    /// Decided for 0.7.4: a chapter whose moment was struck moves to the
    /// start of the part after the stretch, where 0.7.3 showed it, so no made
    /// file goes out of date (db.rs `struck_to_splits`: "A chapter 0.7.3
    /// showed where its piece played again after its struck moment moves
    /// there"). It shows where 0.7.3 showed it (its piece playing again after
    /// the stretch) in each case: an earlier piece of the same VOD starting
    /// after the moment, and a later piece playing the moment itself. The
    /// file 0.7.3 made of each, chapters and all, stays made. A highlight
    /// there stays hidden, as 0.7.3 hid it.
    #[test]
    fn a_chapter_whose_moment_was_struck_shows_where_0_7_3_showed_it() {
        let (_dir, db, _vod, (simple, earlier, later)) = struck(
            "struck-chapter",
            |db, vod| {
                let edit = |title: &str, pieces: &[(f64, f64)]| {
                    let edit = db.add_edit(title).unwrap().id;
                    let ids: Vec<i64> = pieces.iter().map(|&s| add(db, edit, vod, s, "p")).collect();
                    db.add_clip_mark(edit, vod, "chapter", 13.0, Some("Raid")).unwrap();
                    db.add_clip_mark(edit, vod, "highlight", 13.5, Some("Clutch")).unwrap();
                    (edit, ids)
                };
                let simple = edit("Simple", &[(10.0, 20.0), (30.0, 35.0)]);
                let earlier = edit("Earlier", &[(50.0, 60.0), (10.0, 20.0)]);
                let later = edit("Later", &[(10.0, 20.0), (12.5, 16.0)]);
                (simple, earlier, later)
            },
            |(simple, earlier, later)| {
                let strike = vec![(12.0, 14.0)];
                vec![(simple.1[0], strike.clone()), (earlier.1[1], strike.clone()), (later.1[0], strike)]
            },
        );
        let raid = |at: f64| vec![("chapter".to_string(), Some("Raid".to_string()), at)];
        // As 0.7.3 placed them: in the struck piece, where it played again (14 s).
        assert_eq!(shown(&db, simple.0), raid(2.0));
        assert_eq!(shown(&db, earlier.0), raid(12.0), "not at the earlier piece's start");
        let mut both = raid(2.0);
        both.push(("highlight".to_string(), Some("Clutch".to_string()), 9.0));
        assert_eq!(shown(&db, later.0), both, "not where the later piece plays it; the highlight there, as then");
        let moved: Vec<f64> = db.clip_marks(later.0).unwrap().iter().map(|(_, m)| m.at.seconds()).collect();
        assert_eq!(moved, [13.5, 14.0], "the chapter on the part's start, the highlight where it was");
    }

    /// As `a_chapter_whose_moment_was_struck_shows_where_0_7_3_showed_it`, for
    /// a file 0.7.3 made: what it recorded the file made of (`Db::made_of`:
    /// title, what plays, chapters) is still what the edit is, so it isn't
    /// made again.
    #[test]
    fn a_file_made_with_a_struck_chapter_stays_made() {
        let (_dir, path) = library("struck-chapter-made");
        let (vod, earlier, later) = {
            let db = Db::open(&path).unwrap();
            let vod = db.add_vod(&crate::edits::tests::info("x")).unwrap().0.id;
            let edit = |title: &str, pieces: &[(f64, f64)]| {
                let edit = db.add_edit(title).unwrap().id;
                for &s in pieces {
                    add(&db, edit, vod, s, "p");
                }
                db.add_clip_mark(edit, vod, "chapter", 13.0, Some("Raid")).unwrap();
                edit
            };
            (vod, edit("Earlier", &[(50.0, 60.0), (10.0, 20.0)]), edit("Later", &[(10.0, 20.0), (12.5, 16.0)]))
        };
        let conn = back_to(&path, 43);
        let struck = serde_json::to_string(&[(flicks(12.0), flicks(14.0))]).unwrap();
        for (clip, start) in [(earlier, 10.0), (later, 10.0)] {
            conn.execute(
                "UPDATE items SET struck = ?3 WHERE clip_id = ?1 AND start = ?2",
                params![clip, flicks(start), struck],
            )
            .unwrap();
        }
        // What 0.7.3 recorded of each file: its title, what played, its chapters.
        let made = |clip: i64, made_of: String| {
            conn.execute(
                "UPDATE renders SET status = 'ready', file_ext = 'mp4', made_of = ?2 WHERE clip_id = ?1",
                params![clip, made_of],
            )
            .unwrap();
        };
        made(
            earlier,
            format!(
                r#"["Earlier",[[{vod},50000000,60000000],[{vod},10000000,12000000],[{vod},14000000,20000000]],[[12000000,"Raid"]]]"#
            ),
        );
        made(
            later,
            format!(
                r#"["Later",[[{vod},10000000,12000000],[{vod},14000000,20000000],[{vod},12500000,16000000]],[[2000000,"Raid"]]]"#
            ),
        );
        drop(conn);
        let db = upgraded(&path);
        let current = |id| db.edit(id, crate::edits::tests::at_first()).unwrap().unwrap().current;
        assert!(current(earlier), "{}", db.made_of_now(earlier).unwrap());
        assert!(current(later), "{}", db.made_of_now(later).unwrap());
    }

    /// A chapter struck to its piece's very end has no part after it there:
    /// it stays on its moment, and shows where the next piece of its VOD
    /// begins ("A chapter cut away by Remove starts at the next piece",
    /// CHANGELOG.md, 0.7.4), as 0.7.4 shows it with the stretch still struck.
    #[test]
    fn a_chapter_struck_to_its_pieces_end_starts_at_the_next_piece() {
        let (_dir, db, _vod, (edit, a)) = struck(
            "struck-chapter-end",
            |db, vod| {
                let edit = db.add_edit("e").unwrap().id;
                let a = add(db, edit, vod, (10.0, 20.0), "a");
                add(db, edit, vod, (30.0, 35.0), "b");
                db.add_clip_mark(edit, vod, "chapter", 19.0, Some("End")).unwrap();
                (edit, a)
            },
            |&(_, a)| vec![(a, vec![(18.0, 20.0)])],
        );
        assert_eq!(shown(&db, edit), [("chapter".to_string(), Some("End".to_string()), 8.0)]);
        assert_eq!(db.clip_marks(edit).unwrap()[0].1.at.seconds(), 19.0, "on its moment");
        assert_eq!(db.piece_row(a).unwrap().unwrap().end, Some(Flicks::from_seconds(18.0)));
    }

    /// Decided for 0.7.4: a clip on a clip bar struck all the way through is
    /// not deleted; its pieces are left unsplit, so it stays on the bar and
    /// plays its whole stretch again (db.rs `struck_to_splits`: "left whole
    /// instead, its struck stretches emptied: it stays on its bar and plays
    /// its stretch again"). A piece struck whole inside an edit with others
    /// still goes
    /// (`a_wholly_struck_piece_goes_with_a_group_it_leaves_empty`).
    #[test]
    fn a_clip_struck_all_the_way_through_stays_on_its_bar_whole() {
        let (_dir, db, vod, (clip, piece)) = struck(
            "struck-bar-whole",
            |db, vod| {
                let clip = db.add_clip(vod, 30.0, 40.0, "Dinner", true).unwrap().id;
                (clip, db.piece_rows(clip).unwrap()[0].id)
            },
            |&(_, piece)| vec![(piece, vec![(29.0, 41.0)])],
        );
        let on_bar: Vec<i64> = db.clips(vod).unwrap().iter().map(|c| c.id).collect();
        assert_eq!(on_bar, [clip], "on its bar");
        assert_eq!(pieces(&db, clip), [(piece, 30.0, 40.0, String::new())], "its piece as it was");
        assert_eq!(plays(&db, clip), [(vod, 30.0, 40.0)], "its whole stretch");
        let struck: i64 =
            db.conn.query_row("SELECT count(*) FROM items WHERE struck != '[]'", [], |r| r.get(0)).unwrap();
        assert_eq!(struck, 0);
    }

    /// A clip (on a VOD's clip bar) with lines struck becomes pieces as an
    /// edit does, and what was made stays made: its file is of what it still
    /// plays (`Db::made_of`, "A change that leaves this as it was … leaves its
    /// file as it is"). One left a single piece is cut straight from its VOD
    /// from now on (`cut_whole`), and its file, of that stretch, counts as
    /// made so.
    #[test]
    fn a_clip_with_struck_lines_becomes_pieces_and_stays_made() {
        let (_dir, path) = library("struck-clip");
        let (vod, middle, edge, edit) = {
            let db = Db::open(&path).unwrap();
            let vod = db.add_vod(&crate::edits::tests::info("x")).unwrap().0.id;
            let middle = db.add_clip(vod, 10.0, 20.0, "Lunch", true).unwrap().id;
            let edge = db.add_clip(vod, 30.0, 40.0, "Dinner", true).unwrap().id;
            let edit = db.add_edit("Best of").unwrap().id;
            db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
            db.add_clip_mark(edit, vod, "chapter", 11.0, Some("Start")).unwrap();
            (vod, middle, edge, edit)
        };
        let conn = back_to(&path, 43);
        // As 0.7.3 left them: struck, each made of what it played then.
        let strike = |clip: i64, a: f64, b: f64| {
            let json = serde_json::to_string(&[(flicks(a), flicks(b))]).unwrap();
            conn.execute("UPDATE items SET struck = ?2 WHERE clip_id = ?1", params![clip, json]).unwrap();
        };
        strike(middle, 12.0, 13.0);
        strike(edge, 30.0, 32.0);
        strike(edit, 12.0, 13.0);
        let made = |clip: i64, made_of: String| {
            conn.execute(
                "UPDATE renders SET status = 'ready', file_ext = 'mp4', made_of = ?2 WHERE clip_id = ?1",
                params![clip, made_of],
            )
            .unwrap();
        };
        made(middle, format!(r#"["Lunch",[[{vod},10000000,12000000],[{vod},13000000,20000000]],[]]"#));
        made(edge, format!(r#"["Dinner",[[{vod},32000000,40000000]],[]]"#));
        made(edit, format!(r#"["Best of",[[{vod},10000000,12000000],[{vod},13000000,20000000]],[[1000000,"Start"]]]"#));
        drop(conn);
        let db = upgraded(&path);
        let now = crate::edits::tests::at_first();
        let current = |id| db.edit(id, now).unwrap().unwrap().current;
        assert_eq!(plays(&db, middle), [(vod, 10.0, 12.0), (vod, 13.0, 20.0)]);
        assert_eq!(plays(&db, edge), [(vod, 32.0, 40.0)]);
        assert_eq!(plays(&db, edit), [(vod, 10.0, 12.0), (vod, 13.0, 20.0)]);
        assert!(current(middle), "made of what it plays");
        assert!(current(edge), "left one piece: made of what it plays, as a clip cut straight is");
        assert!(current(edit), "the edit too, its chapter where it was");
        let clip = |id| db.clip(id).unwrap().unwrap();
        assert_eq!((clip(middle).status, clip(middle).plain), (ClipStatus::Ready, false));
        assert_eq!((clip(edge).status, clip(edge).plain), (ClipStatus::Ready, true), "one piece of its VOD");
        assert_eq!((clip(edge).start, clip(edge).end), (32.0, 40.0));
    }
}

/// Each library a release left (tests/e2e/fixtures, `Released`) opens in this
/// version with every row it kept: "the library is at this version's format,
/// a copy of the old one kept beside it …; the library lists every VOD, with
/// its transcription …; a word from a transcript and a chat author (`from:`,
/// in any case) are found by search …; clips, marks and the edits with their
/// pieces (and group) are there" (tests/e2e/fixtures/upgrade-matrix.mjs
/// header; e2e keeps only what a window shows, the edit open in the page).
/// One test per release.
mod each_released_library_opens_with_every_row_it_kept {
    use super::*;

    /// Release `version`'s library opened in this version, from format
    /// `from`, a copy of it as it was kept beside it.
    fn opened(version: &str, from: usize) -> (Released, Db) {
        let released = Released::new(version);
        let db = upgraded(&released.library);
        let copy = released.dir().join(format!("library-v{from}.sqlite"));
        assert_eq!(format_of(&Connection::open(&copy).unwrap()), from, "{version}: the copy kept from before");
        (released, db)
    }

    /// Each VOD's platform, title and status, and whether it's listed.
    fn vods(db: &Db) -> Vec<(String, String, Status, bool)> {
        let mut vods: Vec<_> =
            db.vods().unwrap().into_iter().map(|v| (v.info.platform, v.info.title, v.status, v.listed)).collect();
        vods.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        vods
    }

    /// The VOD of `platform`.
    fn on(db: &Db, platform: &str) -> Vod {
        db.vods().unwrap().into_iter().find(|v| v.info.platform == platform).unwrap()
    }

    fn stretches(db: &Db, vod: i64) -> Vec<(f64, Option<f64>, Status, Option<f64>)> {
        db.stretches(vod).unwrap().into_iter().map(|s| (s.start, s.end, s.status, s.reached)).collect()
    }

    /// Where its last line ends.
    fn last_line(db: &Db, vod: i64) -> f64 {
        db.lines(vod).unwrap().iter().map(|l| l.end).fold(0.0, f64::max)
    }

    /// The landed file on this computer, as the fixtures name its folder.
    const LANDED: &str = "{FIXTURES}/eagle-has-landed.mp3";

    /// The library 0.4.0, 0.5.1 and 0.5.2's development left, every kind
    /// of row each kept (make-matrix-library.mjs), with the Kick VOD's
    /// transcription as `paused` (its last line's end where `reached` is
    /// None) and the landing edit's `pieces`, a group's included.
    fn the_matrix(version: &str, from: usize, paused: &[(f64, Option<f64>, Status, Option<f64>)], pieces: usize) {
        let (_released, db) = opened(version, from);
        let done = |platform: &str, title: &str| (platform.into(), title.into(), Status::Done, true);
        assert_eq!(
            vods(&db),
            [
                ("kick".into(), "Walking the wetland trail".into(), Status::Paused, true),
                done("local", "eagle-has-landed"),
                done("twitch", "Speedrunning the old tutorial"),
                done("youtube", "Building a lighthouse on the cliff"),
            ]
        );
        let [local, youtube, twitch, kick] = ["local", "youtube", "twitch", "kick"].map(|p| on(&db, p));
        assert_eq!(local.source_path.as_deref(), Some(LANDED), "where it is, as the fixture names it");

        // Every line and message, the transcriptions whole, the paused one as this release's shape becomes.
        let counts = |vod: &Vod| (db.lines(vod.id).unwrap().len(), db.chat(vod.id).unwrap().len());
        assert_eq!([&local, &youtube, &twitch, &kick].map(counts), [(1, 0), (6, 6), (4, 4), (5, 0)]);
        for vod in [&local, &youtube, &twitch] {
            assert_eq!(stretches(&db, vod.id), [(0.0, None, Status::Done, None)], "{}", vod.info.platform);
        }
        let last = last_line(&db, kick.id);
        let paused: Vec<_> =
            paused.iter().map(|&(a, b, status, reached)| (a, b, status, reached.or(Some(last)))).collect();
        assert_eq!(stretches(&db, kick.id), paused);
        assert_eq!(kick.transcribed, Transcribed::Some);
        let events = |vod: &Vod, kind| -> Vec<_> {
            db.chat_of_kind(vod.id, kind).unwrap().into_iter().map(|(_, m)| (m.author, m.amount)).collect()
        };
        assert_eq!(events(&youtube, Kind::Gift), [("LoudLarry".into(), Some(5.0))]);
        assert_eq!(events(&twitch, Kind::Sub), [("Subbed_Sam".into(), Some(1.0))]);
        assert_eq!(events(&twitch, Kind::Tip), [("BigTipper".into(), Some(5.0))]);

        // Search: a word of the transcript, and a chat author in any case (format 28's index).
        let search = |query: &str| db.search(query, &SearchFilter::default(), 50).unwrap().hits;
        let found: Vec<_> = search("lighthouse").into_iter().map(|h| (h.vod_id, h.source)).collect();
        assert!(found.contains(&(youtube.id, Source::Speech)), "{found:?}");
        for (typed, wanted) in [("from:nightowl42", 4), ("from:NightOwl42 night", 1)] {
            let hits = search(typed);
            let theirs = hits.iter().filter(|h| h.source == Source::Chat && h.author.as_deref() == Some("NightOwl42"));
            assert_eq!((hits.len(), theirs.count()), (wanted, wanted), "{typed}");
        }
        let stone: Vec<_> = search("in:chat stone").into_iter().map(|h| h.author).collect();
        assert_eq!(stone, [Some("LoudLarry".into())]);

        // Clips, with their ids and files; marks.
        let clips = |vod: &Vod| -> Vec<_> {
            db.clips(vod.id).unwrap().into_iter().map(|c| (c.id, c.title, c.status, c.file_path)).collect()
        };
        let clip =
            |id: i64, title: &str| (id, title.into(), ClipStatus::Ready, Some(shown(&db.files().clip_file(id, "mp3"))));
        assert_eq!(clips(&local), [clip(1, "Houston")]);
        assert_eq!(clips(&youtube), [clip(2, "Welcome back"), clip(3, "The lamp")]);
        assert_eq!(clips(&twitch), [clip(4, "Pelican skip")]);
        assert_eq!(clips(&kick), []);
        let marks = |vod: &Vod| -> Vec<_> {
            db.marks(vod.id).unwrap().into_iter().map(|m| (m.kind, m.start, m.text)).collect()
        };
        assert_eq!(
            marks(&youtube),
            [
                ("chapter".into(), 0.0, Some("Intro".into())),
                ("cut".into(), 24.0, None),
                ("highlight".into(), 40.0, Some("Tower done".into())),
            ]
        );
        assert_eq!(marks(&twitch), [("chapter".into(), 7.0, Some("The skip".into()))]);

        // Edits with their pieces, a stretch struck taken out (V46), and the landing's group.
        let edits = db.edits(crate::edits::tests::at_first()).unwrap();
        let edit = |title: &str| edits.iter().find(|e| e.title == title).unwrap().id;
        assert_eq!(edits.len(), 2);
        let landing = db.edit_pieces(edit("Landing highlights")).unwrap();
        let stream = db.edit_pieces(edit("Stream highlights")).unwrap();
        assert_eq!(landing.len(), pieces);
        assert_eq!(landing.iter().filter(|p| p.group.is_some()).count(), pieces - 4);
        let piece = |p: &crate::edits::EditPiece| (p.vod_id, p.title.clone(), p.start, p.end);
        assert_eq!(
            landing[..4].iter().map(piece).collect::<Vec<_>>(),
            [
                (local.id, "Houston".into(), 0.9, 2.0),
                (local.id, "Base".into(), 2.2, 2.6),
                (local.id, "Base".into(), 3.0, 4.0),
                (local.id, "Landed".into(), 4.2, 5.2),
            ]
        );
        assert_eq!(
            stream.iter().map(piece).collect::<Vec<_>>(),
            [
                (youtube.id, "Calm wind".into(), 16.0, 24.0),
                (twitch.id, "Pelican".into(), 7.0, 10.0),
                (twitch.id, "Pelican".into(), 11.5, 15.0),
                (kick.id, "Boardwalk".into(), 20.0, 45.0),
            ]
        );
    }

    #[test]
    fn v0_3_0() {
        let (_released, db) = opened("0.3.0", 16);
        assert_eq!(vods(&db), [("local".into(), "eagle-has-landed".into(), Status::Done, true)]);
        let vod = on(&db, "local");
        assert_eq!(vod.source_path.as_deref(), Some(LANDED), "where it is, as the fixture names it");
        assert_eq!(vod.transcribed, Transcribed::All);
        assert_eq!(stretches(&db, vod.id), [(0.0, None, Status::Done, None)], "transcribed whole, before stretches");
        let said: Vec<_> = db.lines(vod.id).unwrap().into_iter().map(|l| l.text).collect();
        assert_eq!(said, ["Swift in uh Tranguelity Base here, the angle has landed."]);
        assert_eq!(db.search("landed", &SearchFilter::default(), 10).unwrap().hits.len(), 1);
        assert!(db.clips(vod.id).unwrap().is_empty() && db.edits(crate::edits::tests::at_first()).unwrap().is_empty());
    }

    #[test]
    fn v0_4_0() {
        the_matrix("0.4.0", 21, &[(0.0, None, Status::Paused, None)], 4);
    }

    #[test]
    fn v0_5_0() {
        let (_released, db) = opened("0.5.0", 26);
        assert_eq!(
            vods(&db),
            [
                ("local".into(), "eagle-has-landed".into(), Status::Done, true),
                ("local".into(), "landed-10-minutes".into(), Status::Paused, true),
            ]
        );
        let by = |title: &str| db.vods().unwrap().into_iter().find(|v| v.info.title == title).unwrap();
        let (done, paused) = (by("eagle-has-landed"), by("landed-10-minutes"));
        assert_eq!(stretches(&db, done.id), [(0.0, None, Status::Done, None)]);
        assert_eq!(db.lines(paused.id).unwrap().len(), 12, "its lines kept");
        let last = last_line(&db, paused.id);
        assert_eq!(stretches(&db, paused.id), [(0.0, None, Status::Paused, Some(last))], "paused at its last line");
        assert_eq!(
            (paused.transcribed, paused.source_path.as_deref()),
            (Transcribed::Some, Some("{SPEECH}/landed-10-minutes.mp3"))
        );
    }

    #[test]
    fn v0_5_1() {
        the_matrix("0.5.1", 26, &[(0.0, None, Status::Paused, None)], 6);
    }

    #[test]
    fn v0_5_2_dev() {
        the_matrix(
            "0.5.2-dev",
            27,
            &[(0.0, Some(60.0), Status::Done, Some(60.0)), (60.0, Some(120.0), Status::Paused, Some(90.0))],
            6,
        );
    }
}
