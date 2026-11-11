//! Upgrades of libraries from the clip model on (V22 on).

use super::*;

/// Spec 29, with 30 and 31 till each has a library of its own: VODs, clips
/// and edits become documents, each VOD with its own clip listing it, "a
/// VOD's own clip over the whole VOD (`whole_of`, with the VOD's place in
/// the library's list, which `vods.position` held)"; "Clips keep their ids
/// (their pictures are named by them)" (db.rs:517, `MIGRATE_V22`; 0.5.0:
/// "VODs and edits open in one player").
#[test]
fn vods_clips_and_edits_become_clips() {
    let dir = crate::TempDir::new("migrate-v22");
    let path = dir.join("library.sqlite");
    {
        let conn = Connection::open(&path).unwrap();
        let before = [
            SCHEMA,
            MIGRATE_V2,
            MIGRATE_V3,
            MIGRATE_V4,
            MIGRATE_V5,
            MIGRATE_V6,
            MIGRATE_V7,
            MIGRATE_V8,
            MIGRATE_V9,
            MIGRATE_V10,
            MIGRATE_V11,
            MIGRATE_V12,
            MIGRATE_V13,
            MIGRATE_V14,
            MIGRATE_V15,
            MIGRATE_V16,
            MIGRATE_V17,
            MIGRATE_V18,
            MIGRATE_V19,
            MIGRATE_V20,
            MIGRATE_V21,
        ];
        conn.execute_batch(&format!("{} PRAGMA user_version = 21;", before.join("\n"))).unwrap();
        conn.execute_batch(
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
    let db = Db::open(&path).unwrap();
    use crate::edits::LibraryItem::{Edit, Vod};
    let edit = db.edits(crate::edits::tests::at_first()).unwrap().remove(0);
    assert_eq!(db.library_order().unwrap(), [Vod(2), Edit(edit.id), Vod(1)], "the order kept");
    assert_eq!((edit.title.as_str(), edit.current), ("Best of", true));
    // Its file isn't there, nor where settle takes files from: read as
    // gone (where it would be built), as before, so it's made again.
    let file = edit.file_path.expect("a file, gone");
    assert_eq!(file, shown(&db.files().clip_file(edit.id, "mp4")));
    assert!(!Path::new(&file).exists());
    let pieces = db.edit_pieces(edit.id).unwrap();
    assert_eq!(
        pieces.iter().map(|p| (p.id, p.vod_id, p.start, p.end)).collect::<Vec<_>>(),
        [(4, 2, 5.0, 18.0), (5, 1, 100.0, 130.0)]
    );
    assert_eq!(pieces[0].struck, [(12.0, 13.5)], "in flicks now, the same in seconds");
    assert_eq!(db.piece_row(4).unwrap().unwrap().struck, [(Flicks::from_seconds(12.0), Flicks::from_seconds(13.5))]);
    // The clip bar's clips keep their ids, order and files; the one only dragged out is hidden still.
    let bar: Vec<_> = db.clips(1).unwrap().into_iter().map(|c| (c.id, c.start, c.end, c.title, c.shown)).collect();
    assert_eq!(bar, [(2, 30.0, 40.0, "Dinner".into(), true), (1, 10.0, 20.5, "Lunch".into(), true)]);
    assert!(db.clip_handed_out(1).unwrap() && !db.clip_handed_out(2).unwrap());
    let lunch = db.clip(1).unwrap().unwrap().file_path;
    assert_eq!(lunch, Some(shown(&db.files().clip_file(1, "mp4"))), "its file isn't there either: read as gone");
    assert_eq!(db.clips(2).unwrap().iter().map(|c| (c.id, c.shown)).collect::<Vec<_>>(), [(3, false)]);
    // Each VOD has its clip, over all of it.
    let own = db.vod_clip(1).unwrap().unwrap();
    assert_eq!(db.edit_pieces(own).unwrap().iter().map(|p| (p.start, p.end)).collect::<Vec<_>>(), [(0.0, 600.0)]);
    assert!(own != edit.id && db.vod_clip(2).unwrap().unwrap() != edit.id);
    // Marks are the VODs' clips' own, where they were.
    let marks: Vec<_> = db.marks(1).unwrap().into_iter().map(|m| (m.id, m.kind, m.start, m.text)).collect();
    assert_eq!(marks, [(7, "chapter".into(), 65.25, Some("The raid".into()))]);
    assert_eq!(db.marks(2).unwrap().iter().map(|m| m.id).collect::<Vec<_>>(), [8]);
    assert_eq!(db.vods().unwrap().iter().map(|v| v.id).collect::<Vec<_>>(), [2, 1], "VODs list in the order too");
    // Each VOD counts what uses it: its clip, its snippets (shown or not) and the edit's piece.
    assert_eq!((db.uses(1).unwrap(), db.uses(2).unwrap()), (4, 3));
    // Nothing of the old tables is left, nor the order on VODs.
    let tables: Vec<String> = {
        let mut stmt = db.conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").unwrap();
        stmt.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    };
    assert!(
        !tables.iter().any(|t| t == "edits" || t == "edit_pieces" || t == "clips_old" || t == "marks_old"),
        "{tables:?}"
    );
    assert!(db.conn.execute("SELECT position FROM vods", []).is_err());
    // Deleting a VOD takes its clip, the bar and its marks.
    db.delete_vod(1).unwrap();
    assert!(db.vod_clip(1).unwrap().is_none() && db.clips(1).unwrap().is_empty() && db.marks(1).unwrap().is_empty());
    assert_eq!(db.clip(2).unwrap(), None);
    assert_eq!(db.edit_pieces(edit.id).unwrap().len(), 1, "the edit keeps its piece of the other VOD");
}

/// Spec 38: "Each VOD's transcription so far becomes stretches: a finished
/// one, done from 0 to the end; one under way, waiting, paused or failed, a
/// request for all of it in that state (queued ones were added when adding
/// meant transcribing); a stopped one, done as far as its lines go, or
/// nothing" (db.rs:640, `MIGRATE_V27`; 0.6.0: "Adding a VOD doesn't
/// transcribe it."; e2e upgrade-0.5.0.mjs header).
#[test]
fn transcriptions_so_far_become_stretches() {
    let dir = crate::TempDir::new("migrate-v27");
    let path = dir.join("library.sqlite");
    {
        let conn = Connection::open(&path).unwrap();
        let before = [
            SCHEMA,
            MIGRATE_V2,
            MIGRATE_V3,
            MIGRATE_V4,
            MIGRATE_V5,
            MIGRATE_V6,
            MIGRATE_V7,
            MIGRATE_V8,
            MIGRATE_V9,
            MIGRATE_V10,
            MIGRATE_V11,
            MIGRATE_V12,
            MIGRATE_V13,
            MIGRATE_V14,
            MIGRATE_V15,
            MIGRATE_V16,
            MIGRATE_V17,
            MIGRATE_V18,
            MIGRATE_V19,
            MIGRATE_V20,
            MIGRATE_V21,
            MIGRATE_V22,
            MIGRATE_V23,
            MIGRATE_V24,
            MIGRATE_V25,
            MIGRATE_V26,
        ];
        conn.execute_batch(&format!("{} PRAGMA user_version = 26;", before.join("\n").replace("FLICKS", FLICKS)))
            .unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO vods (id, platform, platform_id, url, title, duration, status, progress, error, model, added_at) VALUES
                 (1, 'kick', 'done', 'u', 't', 600, 'done', 1, NULL, '{model}', 0),
                 (2, 'kick', 'paused', 'u', 't', 600, 'paused', 0.2, NULL, '{model}', 0),
                 (3, 'kick', 'queued', 'u', 't', 600, 'queued', 0, NULL, NULL, 0),
                 (4, 'kick', 'stopped', 'u', 't', 600, 'error', 0, 'Stopped', NULL, 0),
                 (5, 'kick', 'failed', 'u', 't', 600, 'error', 0, 'HTTP 404', NULL, 0),
                 (6, 'kick', 'running', 'u', 't', 600, 'transcribing', 0.5, NULL, '{model}', 0),
                 (7, 'kick', 'stopped-lines', 'u', 't', 600, 'error', 0, 'Stopped', '{model}', 0);
             INSERT INTO segments (vod_id, start, end, text, words) VALUES
                 (1, 0, 5, 'a', '[]'), (2, 100, 120, 'b', '[]'), (6, 280, 300, 'c', '[]'), (7, 40, 50, 'd', '[]');",
            model = crate::model::CPU_MODEL.id
        ))
        .unwrap();
    }
    let db = Db::open(&path).unwrap();
    let vod = |id| db.vod(id).unwrap().unwrap();
    let stretches = |id| -> Vec<_> {
        db.stretches(id).unwrap().into_iter().map(|s| (s.start, s.end, s.status, s.reached)).collect()
    };
    assert_eq!(stretches(1), [(0.0, None, Status::Done, None)]);
    assert_eq!((vod(1).status, vod(1).transcribed, vod(1).outdated), (Status::Done, Transcribed::All, false));
    assert_eq!(stretches(2), [(0.0, None, Status::Paused, Some(120.0))]);
    assert_eq!((vod(2).status, vod(2).progress, vod(2).transcribed), (Status::Paused, 0.2, Transcribed::Some));
    assert_eq!(stretches(3), [(0.0, None, Status::Queued, None)], "added when adding meant transcribing");
    assert_eq!((vod(3).status, vod(3).transcribed), (Status::Queued, Transcribed::None));
    assert_eq!(stretches(4), [], "stopped, with nothing to keep");
    assert_eq!((vod(4).status, vod(4).error), (Status::None, None));
    assert_eq!((vod(5).status, vod(5).error.as_deref()), (Status::Error, Some("HTTP 404")));
    assert_eq!(stretches(6), [(0.0, None, Status::Transcribing, Some(300.0))]);
    assert_eq!(stretches(7), [(0.0, Some(50.0), Status::Done, None)], "stopped, its lines kept");
    assert_eq!((vod(7).status, vod(7).transcribed), (Status::None, Transcribed::Some));
    assert_eq!(db.stretches(1).unwrap()[0].model.as_deref(), Some(crate::model::CPU_MODEL.id));
}

/// Spec 39: chat from before is searchable by who wrote it: "Chat's authors
/// indexed, any case, for `from:` and its suggestions" (db.rs:676,
/// `MIGRATE_V28`; 0.6.0: "Search learns from, in, within, after, before.";
/// tests/e2e/fixtures/upgrade-matrix.mjs header: "the author index of format
/// 28 is filled from the old rows").
#[test]
fn search_reads_word_counts_and_chat_authors_after_upgrading() {
    let dir = crate::TempDir::new("migrate-v28");
    let path = dir.join("library.sqlite");
    {
        let conn = Connection::open(&path).unwrap();
        let before = [
            SCHEMA,
            MIGRATE_V2,
            MIGRATE_V3,
            MIGRATE_V4,
            MIGRATE_V5,
            MIGRATE_V6,
            MIGRATE_V7,
            MIGRATE_V8,
            MIGRATE_V9,
            MIGRATE_V10,
            MIGRATE_V11,
            MIGRATE_V12,
            MIGRATE_V13,
            MIGRATE_V14,
            MIGRATE_V15,
            MIGRATE_V16,
            MIGRATE_V17,
            MIGRATE_V18,
            MIGRATE_V19,
            MIGRATE_V20,
            MIGRATE_V21,
            MIGRATE_V22,
            MIGRATE_V23,
            MIGRATE_V24,
            MIGRATE_V25,
            MIGRATE_V26,
            MIGRATE_V27,
        ];
        conn.execute_batch(&format!("{} PRAGMA user_version = 27;", before.join("\n").replace("FLICKS", FLICKS)))
            .unwrap();
        conn.execute_batch(
            "INSERT INTO vods (id, platform, platform_id, url, title, channel, duration, status, added_at) VALUES
                 (1, 'twitch', '1', 'u', 't', 'Streamer', 600, 'done', 0);
             INSERT INTO segments (vod_id, start, end, text, words) VALUES
                 (1, 0, 1, 'Clutch play', '[]'), (1, 5, 6, 'what a play', '[]');
             INSERT INTO chat (vod_id, time, author, text, parts) VALUES
                 (1, 1, 'Bob', 'PLAY', '[]'), (1, 2, 'bobby', 'hi', '[]'), (1, 3, 'alice', 'hi', '[]');",
        )
        .unwrap();
    }
    let db = Db::open(&path).unwrap();
    let version: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 44);
    // The counts are the indexes' own, words already there included: nothing to fill.
    let lines = |table: &str, term: &str| -> i64 {
        let sql = format!("SELECT doc FROM {table} WHERE term = ?1");
        db.conn.query_row(&sql, [term], |r| r.get(0)).optional().unwrap().unwrap_or(0)
    };
    assert_eq!((lines("segments_vocab", "play"), lines("segments_vocab", "clutch")), (2, 1));
    assert_eq!((lines("chat_vocab", "hi"), lines("chat_vocab", "play"), lines("chat_vocab", "nope")), (2, 1, 0));
    // `from:` and its suggestions go by the author index.
    let plan = |sql: &str| -> String {
        let mut stmt = db.conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(3)).unwrap();
        rows.map(|r| r.unwrap()).collect::<Vec<_>>().join("; ")
    };
    assert!(plan("SELECT id FROM chat WHERE author COLLATE NOCASE IN ('bob')").contains("chat_by_author"));
    assert!(
        plan("SELECT author FROM chat WHERE author COLLATE NOCASE >= 'b' AND author COLLATE NOCASE < 'c'")
            .contains("chat_by_author")
    );
    let names: Vec<String> = db.author_suggestions("BO", None).unwrap().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["Bob", "bobby"]);
}

/// Spec 41: a highlight from before casts names nobody; V30 adds "who is in
/// an imported highlight (`people`: its cast's names, as a JSON array), on
/// its mark" (db.rs:703, `MIGRATE_V30`).
#[test]
fn marks_from_before_highlights_had_a_cast_read_with_none() {
    let dir = crate::TempDir::new("migrate-v29");
    let path = dir.join("library.sqlite");
    let vod = {
        let db = Db::open(&path).unwrap();
        let (vod, _) = db.add_vod(&crate::edits::tests::info("abc")).unwrap();
        db.add_mark(vod.id, "highlight", 5.0, Some("Big laugh")).unwrap();
        vod.id
    };
    // Back to version 29: the column 30 adds, gone.
    Connection::open(&path)
        .unwrap()
        .execute_batch(concat!(
            "ALTER TABLE marks DROP COLUMN icon; ALTER TABLE marks DROP COLUMN people; ",
            "ALTER TABLE vods DROP COLUMN annotations; ALTER TABLE vods DROP COLUMN annotations_version; ",
            "ALTER TABLE vods DROP COLUMN annotations_imported; ALTER TABLE vods DROP COLUMN started_at; ",
            "ALTER TABLE vods DROP COLUMN video_ext; ALTER TABLE vods DROP COLUMN audio_ext; ALTER TABLE vods DROP COLUMN received_ext; ALTER TABLE stretches DROP COLUMN audio_ext; ALTER TABLE renders DROP COLUMN file_ext; ALTER TABLE vods DROP COLUMN channel_login; ALTER TABLE vods DROP COLUMN channel_url; ALTER TABLE vod_annotations DROP COLUMN contexts; ALTER TABLE vod_annotations DROP COLUMN povs; ALTER TABLE marks DROP COLUMN imported_as; ALTER TABLE renders DROP COLUMN encoding; ALTER TABLE renders DROP COLUMN make; ALTER TABLE stretches ADD COLUMN note TEXT; DROP TRIGGER clips_ids; DROP TABLE clip_ids; PRAGMA user_version = 29;"
        ))
        .unwrap();
    let db = Db::open(&path).unwrap();
    let version: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 44);
    let marks = db.marks(vod).unwrap();
    assert_eq!((marks[0].text.as_deref(), marks[0].detail.cast.len()), (Some("Big laugh"), 0));
    // And a cast, once there is one, is kept and read back.
    let clip = db.vod_clip(vod).unwrap().unwrap();
    let detail = MarkDetail { cast: vec!["Marlowe".into(), "Ada".into()], ..MarkDetail::default() };
    db.insert_mark_with(clip, vod, "highlight", Flicks::from_seconds(9.0), None, &detail).unwrap();
    assert_eq!(db.marks(vod).unwrap()[1].detail.cast, ["Marlowe", "Ada"]);
}

/// Spec 42: a chapter from before icons has none; V31 adds "what an imported
/// chapter is about, as one emoji (`icon`), on its mark" (db.rs:708,
/// `MIGRATE_V31`).
#[test]
fn marks_from_before_chapters_had_an_icon_read_with_none() {
    let dir = crate::TempDir::new("migrate-v30");
    let path = dir.join("library.sqlite");
    let vod = {
        let db = Db::open(&path).unwrap();
        let (vod, _) = db.add_vod(&crate::edits::tests::info("abc")).unwrap();
        db.add_mark(vod.id, "chapter", 5.0, Some("The game")).unwrap();
        vod.id
    };
    // Back to version 30: the column 31 adds, gone.
    Connection::open(&path)
        .unwrap()
        .execute_batch(concat!(
            "ALTER TABLE marks DROP COLUMN icon; ALTER TABLE vods DROP COLUMN annotations; ",
            "ALTER TABLE vods DROP COLUMN annotations_version; ALTER TABLE vods DROP COLUMN annotations_imported; ",
            "ALTER TABLE vods DROP COLUMN started_at; ",
            "ALTER TABLE vods DROP COLUMN video_ext; ALTER TABLE vods DROP COLUMN audio_ext; ALTER TABLE vods DROP COLUMN received_ext; ALTER TABLE stretches DROP COLUMN audio_ext; ALTER TABLE renders DROP COLUMN file_ext; ALTER TABLE vods DROP COLUMN channel_login; ALTER TABLE vods DROP COLUMN channel_url; ALTER TABLE vod_annotations DROP COLUMN contexts; ALTER TABLE vod_annotations DROP COLUMN povs; ALTER TABLE marks DROP COLUMN imported_as; ALTER TABLE renders DROP COLUMN encoding; ALTER TABLE renders DROP COLUMN make; ALTER TABLE stretches ADD COLUMN note TEXT; DROP TRIGGER clips_ids; DROP TABLE clip_ids; PRAGMA user_version = 30;"
        ))
        .unwrap();
    let db = Db::open(&path).unwrap();
    let version: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 44);
    let marks = db.marks(vod).unwrap();
    assert_eq!((marks[0].text.as_deref(), marks[0].detail.icon.as_deref()), (Some("The game"), None));
    // And an icon, once there is one, is kept and read back.
    let clip = db.vod_clip(vod).unwrap().unwrap();
    let detail = MarkDetail { icon: Some("🎮".into()), ..MarkDetail::default() };
    db.insert_mark_with(clip, vod, "chapter", Flicks::from_seconds(9.0), None, &detail).unwrap();
    assert_eq!(db.marks(vod).unwrap()[1].detail.icon.as_deref(), Some("🎮"));
}

/// Spec 43: a VOD from before look-ups is unknown to the file host: whether
/// annotations are published for it is "unknown until asked" (db.rs:712,
/// `MIGRATE_V32`).
#[test]
fn vods_from_before_annotations_were_looked_up_are_unknown() {
    let dir = crate::TempDir::new("migrate-v31");
    let path = dir.join("library.sqlite");
    let vod = {
        let db = Db::open(&path).unwrap();
        db.add_vod(&crate::edits::tests::info("abc")).unwrap().0.id
    };
    // Back to version 31: the columns 32 adds, gone.
    Connection::open(&path)
        .unwrap()
        .execute_batch(concat!(
            "ALTER TABLE vods DROP COLUMN annotations; ALTER TABLE vods DROP COLUMN annotations_version; ",
            "ALTER TABLE vods DROP COLUMN annotations_imported; ALTER TABLE vods DROP COLUMN started_at; ",
            "ALTER TABLE vods DROP COLUMN video_ext; ALTER TABLE vods DROP COLUMN audio_ext; ALTER TABLE vods DROP COLUMN received_ext; ALTER TABLE stretches DROP COLUMN audio_ext; ALTER TABLE renders DROP COLUMN file_ext; ALTER TABLE vods DROP COLUMN channel_login; ALTER TABLE vods DROP COLUMN channel_url; ALTER TABLE vod_annotations DROP COLUMN contexts; ALTER TABLE vod_annotations DROP COLUMN povs; ALTER TABLE marks DROP COLUMN imported_as; ALTER TABLE renders DROP COLUMN encoding; ALTER TABLE renders DROP COLUMN make; ALTER TABLE stretches ADD COLUMN note TEXT; DROP TRIGGER clips_ids; DROP TABLE clip_ids; PRAGMA user_version = 31;"
        ))
        .unwrap();
    let db = Db::open(&path).unwrap();
    let version: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 44);
    assert_eq!(db.vod(vod).unwrap().unwrap().annotations, Availability::Unknown);
    let listed = db.vods().unwrap();
    assert_eq!(listed[0].annotations, Availability::Unknown);
}

/// Spec 44: a VOD from before its start was kept has none till its chat
/// says: "A chat always replaces the others ... Null when nothing says"
/// (db.rs:721, `MIGRATE_V33`).
#[test]
fn vods_from_before_their_start_was_kept_have_none_till_their_chat_says() {
    let dir = crate::TempDir::new("migrate-v32");
    let path = dir.join("library.sqlite");
    let vod = {
        let db = Db::open(&path).unwrap();
        db.add_vod(&crate::edits::tests::info("abc")).unwrap().0.id
    };
    // Back to version 32: the columns 33 and 34 add, gone.
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "ALTER TABLE vods DROP COLUMN video_ext; ALTER TABLE vods DROP COLUMN audio_ext; ALTER TABLE vods DROP COLUMN received_ext; ALTER TABLE stretches DROP COLUMN audio_ext; ALTER TABLE renders DROP COLUMN file_ext; ALTER TABLE vods DROP COLUMN started_at; ALTER TABLE vods DROP COLUMN channel_login; ALTER TABLE vods DROP COLUMN channel_url; ALTER TABLE vod_annotations DROP COLUMN contexts; ALTER TABLE vod_annotations DROP COLUMN povs; ALTER TABLE marks DROP COLUMN imported_as; ALTER TABLE renders DROP COLUMN encoding; ALTER TABLE renders DROP COLUMN make; ALTER TABLE stretches ADD COLUMN note TEXT; DROP TRIGGER clips_ids; DROP TABLE clip_ids; PRAGMA user_version = 32;",
        )
        .unwrap();
    let db = Db::open(&path).unwrap();
    let version: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 44);
    assert_eq!(db.vod(vod).unwrap().unwrap().info.started_at, None);
    db.set_chat_start(vod, 1790631005.0).unwrap();
    assert_eq!(db.vods().unwrap()[0].info.started_at.as_deref(), Some("2026-09-28T21:30:05Z"));
}

/// Spec 45: an import from before points of view has none: "Null for an
/// import from before: none known" (db.rs:733, `MIGRATE_V34`).
#[test]
fn annotations_imported_before_povs_were_kept_have_none() {
    let dir = crate::TempDir::new("migrate-v33");
    let path = dir.join("library.sqlite");
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
    Connection::open(&path)
        .unwrap()
        .execute_batch("ALTER TABLE vods DROP COLUMN video_ext; ALTER TABLE vods DROP COLUMN audio_ext; ALTER TABLE vods DROP COLUMN received_ext; ALTER TABLE stretches DROP COLUMN audio_ext; ALTER TABLE renders DROP COLUMN file_ext; ALTER TABLE vods DROP COLUMN channel_login; ALTER TABLE vods DROP COLUMN channel_url; ALTER TABLE vod_annotations DROP COLUMN contexts; ALTER TABLE vod_annotations DROP COLUMN povs; ALTER TABLE marks DROP COLUMN imported_as; ALTER TABLE renders DROP COLUMN encoding; ALTER TABLE renders DROP COLUMN make; ALTER TABLE stretches ADD COLUMN note TEXT; DROP TRIGGER clips_ids; DROP TABLE clip_ids; PRAGMA user_version = 33;")
        .unwrap();
    let db = Db::open(&path).unwrap();
    let version: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 44);
    let kept = db.vod_annotations(vod).unwrap().unwrap();
    assert!(kept.povs.is_empty() && kept.cast.is_empty());
}

/// Spec 49: a VOD from before channel logins has none: "Null for a VOD
/// added before: unknown" (db.rs:779, `MIGRATE_V37`; 0.7.2 Fixes: "Every
/// Twitch channel shows its picture.").
#[test]
fn a_channels_login_is_kept_and_vods_from_before_have_none() {
    let dir = crate::TempDir::new("migrate-v36");
    let path = dir.join("library.sqlite");
    let before = {
        let db = Db::open(&path).unwrap();
        let info =
            VodInfo {
                platform: "twitch".into(), channel: Some("마를로".into()), ..crate::edits::tests::info("old")
            };
        db.add_vod(&info).unwrap().0.id
    };
    // Back to version 36: the column 37 adds, gone.
    Connection::open(&path)
        .unwrap()
        .execute_batch("ALTER TABLE vods DROP COLUMN video_ext; ALTER TABLE vods DROP COLUMN audio_ext; ALTER TABLE vods DROP COLUMN received_ext; ALTER TABLE stretches DROP COLUMN audio_ext; ALTER TABLE renders DROP COLUMN file_ext; ALTER TABLE vods DROP COLUMN channel_login; ALTER TABLE vods DROP COLUMN channel_url; ALTER TABLE vod_annotations DROP COLUMN contexts; ALTER TABLE renders DROP COLUMN encoding; ALTER TABLE renders DROP COLUMN make; ALTER TABLE stretches ADD COLUMN note TEXT; DROP TRIGGER clips_ids; DROP TABLE clip_ids; PRAGMA user_version = 36;")
        .unwrap();
    let db = Db::open(&path).unwrap();
    let version: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 44);
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

/// Spec 52: a VOD from before channel pages has its page filled from its
/// info where kept: "Null for a VOD added before, filled from its info where
/// that is still kept (`pipeline::fill_channels`)" (db.rs:789,
/// `MIGRATE_V40`; 0.7.2: "Names open a streamer card.", "The channel over
/// the player links to its page on every site").
#[test]
fn a_channels_page_is_kept_and_filled_in_only_where_missing() {
    let root = crate::TempDir::new("channel-page");
    let path = root.join("library.sqlite");
    let before = {
        let db = Db::open(&path).unwrap();
        db.add_vod(&VodInfo { platform: "dailymotion".into(), ..crate::edits::tests::info("old") }).unwrap().0.id
    };
    // Back to version 39: the column 40 adds, gone.
    Connection::open(&path)
        .unwrap()
        .execute_batch("ALTER TABLE vods DROP COLUMN channel_url; ALTER TABLE renders DROP COLUMN encoding; ALTER TABLE renders DROP COLUMN make; ALTER TABLE stretches ADD COLUMN note TEXT; DROP TRIGGER clips_ids; DROP TABLE clip_ids; PRAGMA user_version = 39;")
        .unwrap();
    let db = Db::open(&path).unwrap();
    let version: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 44);
    assert_eq!(db.vod(before).unwrap().unwrap().info.channel_url, None);
    let page = "https://www.dailymotion.com/quill";
    let info =
        VodInfo { platform: "dailymotion".into(), channel_url: Some(page.into()), ..crate::edits::tests::info("new") };
    let (added, _) = db.add_vod(&info).unwrap();
    assert_eq!(added.info.channel_url.as_deref(), Some(page));
    let json = serde_json::to_value(db.vod(added.id).unwrap().unwrap()).unwrap();
    assert_eq!(json["channel_url"], page);
    // Filled in where there's none; what's there stays.
    db.fill_channel(before, Some("x2"), Some("https://www.dailymotion.com/x2")).unwrap();
    db.fill_channel(added.id, Some("other"), Some("https://www.dailymotion.com/other")).unwrap();
    let old = db.vod(before).unwrap().unwrap().info;
    assert_eq!(
        (old.channel_login.as_deref(), old.channel_url.as_deref()),
        (Some("x2"), Some("https://www.dailymotion.com/x2"))
    );
    let new = db.vod(added.id).unwrap().unwrap().info;
    assert_eq!(
        (new.channel_login, new.channel_url.as_deref()),
        (Some("other".into()), Some(page)),
        "only the login was missing"
    );
}

/// Spec 54: an upgraded library never gives a deleted clip's id again: "a
/// deleted clip's id is never given to a new one, as a VOD's isn't (v14), so
/// what still holds it (an undo step, the clipboard, the page, the clip's
/// picture and files, all named by it) can't reach the new clip"
/// (db.rs:808, `MIGRATE_V42`; 0.7.3 Fixes: "Undo no longer changes another
/// clip."). Edits and pasted clips are clips too. A library from before
/// upgrades with all it had, as it had it.
#[test]
fn clip_ids_are_never_reused() {
    use crate::clipboard::Paste;
    use crate::edits::Put;
    let dir = crate::TempDir::new("clip-ids");
    let path = dir.join("library.sqlite");
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
    Connection::open(&path)
        .unwrap()
        .execute_batch("ALTER TABLE renders DROP COLUMN make; ALTER TABLE stretches ADD COLUMN note TEXT; DROP TRIGGER clips_ids; DROP TABLE clip_ids; PRAGMA user_version = 41;")
        .unwrap();
    let db = Db::open(&path).unwrap();
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

/// The stretches' `note` (v27), which nothing ever read or wrote, goes
/// (`MIGRATE_V44`), and a library from before upgrades with every stretch
/// as it was: done, asked for, paused partway, failed, with its audio
/// (testing.md, "Order after the pilot" 1: old schema and rows in, the
/// same data out).
#[test]
fn the_stretches_unread_note_goes_and_every_stretch_stays_as_it_was() {
    let dir = crate::TempDir::new("stretch-note");
    let path = dir.join("library.sqlite");
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
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "ALTER TABLE stretches ADD COLUMN note TEXT; UPDATE stretches SET note = 'old' WHERE start = 100;
             PRAGMA user_version = 43;",
        )
        .unwrap();
    let db = Db::open(&path).unwrap();
    let version: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 44);
    assert_eq!(db.stretches(vod).unwrap(), before, "upgraded, the same stretches");
    assert_eq!(serde_json::to_value(db.vod(vod).unwrap().unwrap()).unwrap(), shown, "and the VOD shown the same");
    assert!(!columns(&db.conn).contains(&"note".to_string()), "upgraded: {:?}", columns(&db.conn));
}

/// Spec 47: "Where the VOD it was kept from is still in the library
/// (keeping a later stretch failed, so the VOD stayed), those come back from
/// its rows": who speaks each line, and what each chat message is, how much
/// and when it happened (db.rs:746, `MIGRATE_V36`; 0.7.0 Fixes: "Kept
/// stretches keep subs, gifts and tips."). Moved here from sources.rs.
#[test]
fn stretches_kept_while_their_vod_stays_get_back_their_speakers_and_chat_kinds() {
    let dir = crate::TempDir::new("migrate-v36");
    let path = dir.join("library.sqlite");
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
        let message = |time: f64, text: &str, kind: Kind, amount: Option<f64>, event_time: Option<f64>| ChatMessage {
            time,
            author: "fan".into(),
            color: None,
            parts: vec![ChatPart::Text { text: text.into() }],
            kind,
            amount,
            event_time,
            at: None,
        };
        let chat = [
            message(12.0, "hello", Kind::Message, None, None),
            message(13.0, "resubbed", Kind::Sub, Some(2.0), Some(12.5)),
            message(14.0, "gift", Kind::Gift, Some(5.0), None),
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
        // One whose VOD is gone: nothing to get back.
        let other = db.add_vod(&info("y")).unwrap().0.id;
        crate::db::insert_chat(&db.conn, other, &[message(1.0, "tip", Kind::Tip, Some(100.0), None)]).unwrap();
        let orphan = db.add_realized(other, 0.0, 5.0).unwrap();
        db.conn.execute("UPDATE chat SET kind = 'message', amount = NULL WHERE vod_id = ?1", [orphan]).unwrap();
        db.delete_vod(other).unwrap();
        (vod, kept)
    };
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "ALTER TABLE vods DROP COLUMN video_ext; ALTER TABLE vods DROP COLUMN audio_ext; ALTER TABLE vods DROP COLUMN received_ext; ALTER TABLE stretches DROP COLUMN audio_ext; ALTER TABLE renders DROP COLUMN file_ext; ALTER TABLE vods DROP COLUMN channel_login; ALTER TABLE vods DROP COLUMN channel_url; ALTER TABLE vod_annotations DROP COLUMN contexts; ALTER TABLE renders DROP COLUMN encoding; ALTER TABLE renders DROP COLUMN make; ALTER TABLE stretches ADD COLUMN note TEXT; DROP TRIGGER clips_ids; DROP TABLE clip_ids; PRAGMA user_version = 35;",
        )
        .unwrap();
    let db = Db::open(&path).unwrap();

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
