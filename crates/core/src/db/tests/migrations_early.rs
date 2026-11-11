//! Upgrades of libraries from before the clip model (V1 to V21), and the
//! upgrade itself: the copy kept before it, a library from a newer version.

use super::*;

/// A copy from before an upgrade goes after 30 days (CHANGELOG.md, 0.4.0:
/// "A copy of your library is kept before each upgrade of its format (for
/// 30 days)"; db.rs `COPIES_KEPT_FOR`: "time to find an upgrade gone wrong,
/// but not for good, as a copy keeps what's been deleted since").
#[test]
fn a_copy_from_before_an_upgrade_goes_after_thirty_days() {
    let dir = crate::TempDir::new("old-copies");
    let path = dir.join("library.sqlite");
    drop(Db::open(&path).unwrap());
    let (old, new, other) = (dir.join("library-v3.sqlite"), dir.join("library-v9.sqlite"), dir.join("notes.sqlite"));
    for p in [&old, &new, &other] {
        std::fs::write(p, b"copy").unwrap();
    }
    let long_ago = SystemTime::now() - std::time::Duration::from_secs(31 * 24 * 3600);
    for p in [&old, &other] {
        std::fs::File::options().write(true).open(p).unwrap().set_modified(long_ago).unwrap();
    }
    drop(Db::open(&path).unwrap());
    assert!(!old.exists(), "a copy 31 days old goes");
    assert!(new.exists(), "a new one stays");
    assert!(other.exists(), "only the library's copies");
}

#[test]
fn an_upgrade_keeps_a_copy_from_before_and_only_the_last_two() {
    let dir = crate::TempDir::new("before-upgrade");
    let path = dir.join("library.sqlite");
    let version =
        |p: &Path| -> i32 { Connection::open(p).unwrap().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap() };
    // Copies left by earlier upgrades, 3 the older.
    for v in [3, 9] {
        std::fs::write(dir.join(format!("library-v{v}.sqlite")), b"old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(&format!("{SCHEMA} PRAGMA user_version = 1;")).unwrap();
        conn.execute(
            "INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES ('kick', 'x', 'u', 't', 'done', 0)",
            [],
        )
        .unwrap();
    }
    drop(Db::open(&path).unwrap());
    let copy = dir.join("library-v1.sqlite");
    assert_eq!(version(&copy), 1, "the copy is as it was before");
    let vods: i64 = Connection::open(&copy).unwrap().query_row("SELECT count(*) FROM vods", [], |r| r.get(0)).unwrap();
    assert_eq!(vods, 1);
    assert!(version(&path) > 1, "and the library upgraded");
    // The two newest kept: the one just made and 9; 3, the oldest, goes.
    let mut kept: Vec<String> = std::fs::read_dir(&*dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("library-v"))
        .collect();
    kept.sort();
    assert_eq!(kept, ["library-v1.sqlite", "library-v9.sqlite"]);
    // Opened again, nothing to upgrade: no copy made.
    std::fs::remove_file(&copy).unwrap();
    drop(Db::open(&path).unwrap());
    assert!(!copy.exists());
}

#[test]
fn migrates_v1_databases() {
    let dir = crate::TempDir::new("migrate");
    let path = dir.join("library.sqlite");
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(&format!("{SCHEMA} PRAGMA user_version = 1;")).unwrap();
        conn.execute(
            "INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES ('kick', 'x', 'u', 't', 'done', 0)",
            [],
        )
        .unwrap();
    }
    let mut db = Db::open(&path).unwrap();
    let vod = db.vods().unwrap().remove(0);
    db.conn.execute("UPDATE clips SET position = 5", []).unwrap();
    let later = db.add_vod(&info("later")).unwrap().0;
    assert_eq!(db.vods().unwrap()[0].id, later.id, "added on top of migrated VODs");
    db.delete_vod(later.id).unwrap();
    assert_eq!(vod.note, None);
    assert!(!vod.outdated, "no transcript yet");
    assert_eq!((vod.status, vod.transcribed), (Status::Done, Transcribed::All), "done from 0 to the end");
    db.insert_segments(vod.id, &[Segment { start: 0.0, end: 1.0, text: "hi".into(), words: Vec::new() }]).unwrap();
    assert!(db.vod(vod.id).unwrap().unwrap().outdated, "transcribed before models were recorded");
    let done = db.stretches(vod.id).unwrap()[0].id;
    db.set_stretch_model(done, Some(crate::model::CPU_MODEL.id)).unwrap();
    assert!(!db.vod(vod.id).unwrap().unwrap().outdated);
    db.set_stretch_model(done, Some("sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8")).unwrap();
    assert!(db.vod(vod.id).unwrap().unwrap().outdated, "older model");
    db.set_note(vod.id, Some("ran on CPU")).unwrap();
    assert_eq!(db.vod(vod.id).unwrap().unwrap().note.as_deref(), Some("ran on CPU"));
}

#[test]
fn chat_from_before_kinds_is_fetched_again() {
    let dir = crate::TempDir::new("migrate-v10");
    let path = dir.join("library.sqlite");
    {
        let conn = Connection::open(&path).unwrap();
        let before =
            [SCHEMA, MIGRATE_V2, MIGRATE_V3, MIGRATE_V4, MIGRATE_V5, MIGRATE_V6, MIGRATE_V7, MIGRATE_V8, MIGRATE_V9];
        conn.execute_batch(&format!("{} PRAGMA user_version = 9;", before.join("\n"))).unwrap();
        conn.execute_batch(
            "INSERT INTO vods (platform, platform_id, url, title, status, added_at, chat) VALUES
                 ('kick', 'a', 'u', 't', 'done', 0, 'done'), ('local', 'b', 'u', 't', 'done', 0, 'none');
             INSERT INTO chat (vod_id, time, author, text, parts) VALUES (1, 1.0, 'v', 'hi', '[]');",
        )
        .unwrap();
    }
    let db = Db::open(&path).unwrap();
    let chat: Vec<_> = db.vods().unwrap().iter().map(|v| v.chat).collect();
    assert!(chat.contains(&ChatStatus::Pending) && chat.contains(&ChatStatus::None));
    assert_eq!(db.chat(1).unwrap()[0].kind, Kind::Message, "kept until fetched again");
}

#[test]
fn pieces_of_clips_become_their_own() {
    let dir = crate::TempDir::new("migrate-v19");
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
        ];
        conn.execute_batch(&format!("{} PRAGMA user_version = 19;", before.join("\n"))).unwrap();
        conn.execute_batch(
            "INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES ('kick', 'a', 'u', 't', 'done', 0);
             INSERT INTO clips (vod_id, start, end, title, position, created_at) VALUES (1, 10.0, 20.0, 'Lunch', 1, 0);
             INSERT INTO edits (title, position, created_at) VALUES ('e', 0, 0);
             INSERT INTO edit_pieces (edit_id, clip_id, position, start, end, struck)
                 VALUES (1, 1, 0, 5.0, 18.0, '[[12.0,13.0]]');",
        )
        .unwrap();
    }
    let db = Db::open(&path).unwrap();
    let edit = db.edits(crate::edits::tests::at_first()).unwrap()[0].id;
    let pieces = db.edit_pieces(edit).unwrap();
    assert_eq!(pieces.len(), 1);
    let p = &pieces[0];
    assert_eq!((p.vod_id, p.start, p.end, p.title.as_str()), (1, 10.0, 18.0, "Lunch"), "within the clip it was of");
    assert_eq!(p.struck, [(12.0, 13.0)]);
    // The clip's gone; the piece stays.
    db.delete_clip(1).unwrap();
    assert_eq!(db.edit_pieces(edit).unwrap().len(), 1);
}

#[test]
fn canceled_vods_say_stopped() {
    let dir = crate::TempDir::new("migrate-v12");
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
        ];
        conn.execute_batch(&format!("{} PRAGMA user_version = 11;", before.join("\n"))).unwrap();
        conn.execute_batch(
            "INSERT INTO vods (platform, platform_id, url, title, status, error, added_at) VALUES
                 ('kick', 'a', 'u', 't', 'error', 'Canceled', 0), ('kick', 'b', 'u', 't', 'error', 'No audio', 0);",
        )
        .unwrap();
    }
    let db = Db::open(&path).unwrap();
    // As v12 left them (since v27 the VOD's own error is unread).
    let mut stmt = db.conn.prepare("SELECT error FROM vods").unwrap();
    let errors: Vec<Option<String>> = stmt.query_map([], |r| r.get(0)).unwrap().map(|e| e.unwrap()).collect();
    assert!(errors.contains(&Some("Stopped".into())) && errors.contains(&Some("No audio".into())));
}

#[test]
fn ids_are_never_reused() {
    let dir = crate::TempDir::new("ids");
    let path = dir.join("library.sqlite");
    let before: Vec<_> = {
        // A library from before v14, whose newest VOD was deleted.
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
        ];
        conn.execute_batch(&format!("{} PRAGMA user_version = 13;", before.join("\n"))).unwrap();
        conn.execute_batch(
            "INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES
                 ('kick', 'a', 'u', 't', 'done', 0), ('kick', 'b', 'u', 't', 'done', 0);",
        )
        .unwrap();
        conn.query_row("SELECT max(id) FROM vods", [], |r| r.get::<_, i64>(0)).into_iter().collect()
    };
    let db = Db::open(&path).unwrap();
    let a = db.add_vod(&info("a2")).unwrap().0;
    assert!(a.id > before[0]);
    db.delete_vod(a.id).unwrap();
    let b = db.add_vod(&info("b2")).unwrap().0;
    assert!(b.id > a.id, "the deleted VOD's id isn't given out again");
    // Even after a VOD added without taking an id (as an older version would).
    db.conn
        .execute("INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES ('kick', 'c', 'u', 't', 'done', 0)", [])
        .unwrap();
    let older = db.conn.last_insert_rowid();
    db.delete_vod(older).unwrap();
    assert!(db.add_vod(&info("d2")).unwrap().0.id > older);
}

/// A chat fetch an older version touched since starts over (db.rs
/// `MIGRATE_V15`: "a fetch an older version left unfinished starts over,
/// and so does one an older version touched since (`chat_slices`)";
/// CHANGELOG.md, 0.3.0: "Kick and Twitch chats download 3 to 5 times
/// faster."). The "left unfinished" half waits for a library at version 14.
#[test]
fn a_chat_an_older_version_touched_since_starts_over() {
    let mut db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    db.add_chat(vod, &[], &[10.0, f64::MAX]).unwrap();
    assert_eq!(db.chat_slices(vod).unwrap(), Some(vec![10.0, f64::MAX]));
    // 0.2.1 cleared the chat and fetched a little, saving its own position.
    db.conn.execute("UPDATE vods SET chat_resume = 5.0 WHERE id = ?1", [vod]).unwrap();
    assert_eq!(db.chat_slices(vod).unwrap(), None, "start over");
    db.clear_chat(vod).unwrap();
    db.add_chat(vod, &[], &[3.0]).unwrap();
    assert_eq!(db.chat_slices(vod).unwrap(), Some(vec![3.0]), "and carry on after that");
}

/// Deleted words don't stay in the library file (db.rs `MIGRATE_V6`:
/// "deleting a transcript removes its words from the search index right
/// away"; `MIGRATE_V13`: "so deleted words still don't stay in the file").
/// It starts from a new library; the start from version 5 waits for a
/// library at a version.
#[test]
fn deleted_words_do_not_stay_in_the_library_file() {
    let dir = crate::TempDir::new("delete");
    let path = dir.join("library.sqlite");
    let mut db = Db::open(&path).unwrap();
    // Another VOD stays, sharing the search indexes with the one removed.
    let kept = db.add_vod(&info("kept")).unwrap().0;
    let vod = db.add_vod(&info("x")).unwrap().0;
    let secret = "zanzibarquokka";
    for (i, id) in [kept.id, vod.id, kept.id].into_iter().enumerate() {
        let text = if id == vod.id { secret.to_string() } else { format!("plain words {i}") };
        db.insert_segments(id, &[Segment { start: i as f64, end: 1.0, text: text.clone(), words: words(&text, 0.0) }])
            .unwrap();
        db.add_chat(id, &[message(i as f64, "v", &format!("chat{text}"))], &[i as f64]).unwrap();
    }
    assert_eq!(db.search(secret, &SearchFilter::default(), 10).unwrap().hits.len(), 1);
    while db.delete_some_of(vod.id, 1000).unwrap() > 0 {}
    // A batch saved after the erase, by a fetch that hadn't stopped yet.
    db.add_chat(vod.id, &[message(9.0, "v", "late quokkazanzibar")], &[9.0]).unwrap();
    db.delete_vod(vod.id).unwrap();
    assert_eq!(db.search("words", &SearchFilter::default(), 10).unwrap().hits.len(), 4, "the other VOD's stay");
    drop(db);
    let bytes =
        [std::fs::read(&path).unwrap(), std::fs::read(path.with_extension("sqlite-wal")).unwrap_or_default()].concat();
    assert!(!bytes.windows(8).any(|w| w == &secret.as_bytes()[6..]), "deleted text still on disk");
    assert!(!bytes.windows(8).any(|w| w == b"zanzibar"), "a late batch's text still on disk");
}

// The specs below start from a library as the first release left it, which
// is `SCHEMA` alone: they need no list of migrations to stop partway.

/// A library as 0.1.0 left it at `path` (format 1), with `rows` put in.
fn first_release(path: &Path, rows: &str) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(&format!("{SCHEMA} PRAGMA user_version = 1; {rows}")).unwrap();
}

/// One VOD 0.1.0 had transcribed, saying "the eagle has landed" at 12 s.
const TRANSCRIBED: &str = r#"
INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES ('kick', 'x', 'u', 'Day 3', 'done', 0);
INSERT INTO segments (vod_id, start, end, text, words) VALUES (1, 12.0, 14.0, 'the eagle has landed',
    '[{"text":"the","start":12.0,"end":12.2},{"text":"eagle","start":12.2,"end":12.8},
      {"text":"has","start":12.8,"end":13.2},{"text":"landed","start":13.2,"end":14.0}]');
"#;

/// The copies from before upgrades beside the library at `path`.
fn copies_beside(path: &Path) -> Vec<String> {
    let mut copies: Vec<String> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("library-v"))
        .collect();
    copies.sort();
    copies
}

/// The format number of the library file at `path`.
fn format_of(path: &Path) -> i32 {
    Connection::open(path).unwrap().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap()
}

/// The copy at `copy` put in a library's place in a folder of its own, as
/// a user undoing an upgrade would, and opened.
fn put_back(copy: &Path) -> (crate::TempDir, Db) {
    let back = crate::TempDir::new("put-back");
    std::fs::copy(copy, back.join("library.sqlite")).unwrap();
    let db = Db::open(&back.join("library.sqlite")).unwrap();
    (back, db)
}

/// An upgrade keeps a copy of the library as it was, and only an upgrade
/// does (CHANGELOG.md, 0.4.0: "A copy of your library is kept before each
/// upgrade of its format (for 30 days)"; db.rs `keep_before_upgrade`:
/// "Copies the library … before it's upgraded, whole and as it stands").
#[test]
fn an_upgrade_keeps_a_copy_of_the_library_from_before() {
    let dir = crate::TempDir::new("copy-before");
    let path = dir.join("library.sqlite");
    first_release(&path, TRANSCRIBED);
    drop(Db::open(&path).unwrap());
    assert_eq!(copies_beside(&path), ["library-v1.sqlite"]);
    let copy = dir.join("library-v1.sqlite");
    let (_back, kept) = put_back(&copy);
    let vod = kept.vods().unwrap().remove(0);
    assert_eq!(kept.lines(vod.id).unwrap()[0].text, "the eagle has landed", "the copy holds what the library held");
    // Opened again, there's nothing to upgrade, so no copy is made.
    std::fs::remove_file(&copy).unwrap();
    drop(Db::open(&path).unwrap());
    assert_eq!(copies_beside(&path), [] as [&str; 0]);
    // Nor when a library is made: there was nothing before it.
    let new = crate::TempDir::new("copy-new");
    drop(Db::open(&new.join("library.sqlite")).unwrap());
    assert_eq!(copies_beside(&new.join("library.sqlite")), [] as [&str; 0]);
}

/// Only the two newest copies from before upgrades are kept (db.rs
/// `KEPT_COPIES`: "How many copies of the library from before upgrades are
/// kept"; `keep_before_upgrade`: "removes all but the newest copies").
#[test]
fn only_the_two_newest_copies_are_kept() {
    let dir = crate::TempDir::new("two-copies");
    let path = dir.join("library.sqlite");
    first_release(&path, TRANSCRIBED);
    // Copies left by earlier upgrades: 9's a day old, 3's two.
    for (version, days) in [(3, 2), (9, 1)] {
        let copy = dir.join(format!("library-v{version}.sqlite"));
        std::fs::write(&copy, b"old").unwrap();
        let made = SystemTime::now() - std::time::Duration::from_secs(days * 24 * 3600);
        std::fs::File::options().write(true).open(&copy).unwrap().set_modified(made).unwrap();
    }
    drop(Db::open(&path).unwrap());
    assert_eq!(copies_beside(&path), ["library-v1.sqlite", "library-v9.sqlite"], "the one just made and 9");
}

/// A copy reads as the version it was, so put back in the library's place
/// it's upgraded again (db.rs `keep_before_upgrade`: "VACUUM INTO doesn't
/// carry the format's number over").
#[test]
fn a_copy_reads_as_the_version_it_was() {
    let dir = crate::TempDir::new("copy-version");
    let path = dir.join("library.sqlite");
    first_release(&path, TRANSCRIBED);
    drop(Db::open(&path).unwrap());
    let copy = dir.join("library-v1.sqlite");
    assert_eq!(format_of(&copy), 1);
    let (_back, db) = put_back(&copy);
    let vod = db.vods().unwrap().remove(0);
    assert_eq!(vod.info.title, "Day 3");
    assert_eq!(db.lines(vod.id).unwrap()[0].text, "the eagle has landed");
}

/// No copy, no upgrade: when the copy can't be made, the open fails and the
/// library stays at its version, as it was (db.rs `Db::open`: "A copy as it
/// was first, so an upgrade that turns out wrong can be undone; no copy, no
/// upgrade").
#[test]
fn no_copy_no_upgrade() {
    let dir = crate::TempDir::new("no-copy");
    let path = dir.join("library.sqlite");
    first_release(&path, TRANSCRIBED);
    // The copy's place is taken by a folder.
    let copy = dir.join("library-v1.sqlite");
    std::fs::create_dir(&copy).unwrap();
    assert!(Db::open(&path).is_err());
    assert_eq!(format_of(&path), 1, "not upgraded");
    // Its place free again, the library upgrades with what it had.
    std::fs::remove_dir(&copy).unwrap();
    let db = Db::open(&path).unwrap();
    let vod = db.vods().unwrap().remove(0);
    assert_eq!(db.lines(vod.id).unwrap()[0].text, "the eagle has landed");
}

/// A library a newer version made opens (db.rs `Db::open`: "A newer
/// VODscribe made this library (the user went back a version) … carry on
/// rather than refuse to start"). Going back before 0.5.0 isn't supported
/// (sections-074.md "Decided", D4); this library is today's.
#[test]
fn a_library_from_a_newer_version_opens() {
    let dir = crate::TempDir::new("newer");
    let path = dir.join("library.sqlite");
    {
        let db = Db::open(&path).unwrap();
        db.add_vod(&info("kept")).unwrap();
    }
    Connection::open(&path).unwrap().execute_batch("PRAGMA user_version = 1000;").unwrap();
    let db = Db::open(&path).unwrap();
    assert_eq!(db.vods().unwrap()[0].info.platform_id, "kept");
}

/// A library from the first release opens with its VODs, and their
/// transcripts are searchable (CHANGELOG.md, 0.1.0: "First release.";
/// tests/e2e/scenarios/upgrade.mjs: "a library and settings as 0.3.0 left
/// them … open in this version with the VOD, its transcript (one stretch,
/// all of it)").
#[test]
fn a_library_from_the_first_release_opens_with_its_vods_and_transcripts_searchable() {
    let dir = crate::TempDir::new("first-release");
    let path = dir.join("library.sqlite");
    first_release(&path, TRANSCRIBED);
    let db = Db::open(&path).unwrap();
    let vods = db.vods().unwrap();
    assert_eq!(vods.len(), 1);
    let vod = &vods[0];
    assert_eq!((vod.info.title.as_str(), vod.status, vod.transcribed), ("Day 3", Status::Done, Transcribed::All));
    assert_eq!(db.stretches(vod.id).unwrap().len(), 1, "one stretch, all of it");
    let lines = db.lines(vod.id).unwrap();
    assert_eq!((lines[0].start, lines[0].text.as_str()), (12.0, "the eagle has landed"));
    let hits = db.search("eagle has landed", &SearchFilter::default(), 10).unwrap().hits;
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].vod_id, hits[0].time), (vod.id, 12.2), "found at its first word");
}

/// A VOD from before notes were kept has none (db.rs `MIGRATE_V2`: "a note
/// shown with a VOD that isn't an error, such as why it ran on the CPU").
#[test]
fn a_vod_from_before_notes_has_none() {
    let dir = crate::TempDir::new("before-notes");
    let path = dir.join("library.sqlite");
    first_release(&path, TRANSCRIBED);
    let db = Db::open(&path).unwrap();
    assert_eq!(db.vods().unwrap()[0].note, None);
}

/// A transcript from before the speech model was recorded reads as
/// outdated (db.rs `MIGRATE_V4`: "the speech model that produced the
/// transcript"; `Vod::outdated`: "Has transcript segments from a model this
/// version no longer uses (or from before models were recorded)").
#[test]
fn a_transcript_from_before_models_were_recorded_reads_as_outdated() {
    let dir = crate::TempDir::new("before-models");
    let path = dir.join("library.sqlite");
    first_release(&path, TRANSCRIBED);
    let db = Db::open(&path).unwrap();
    assert!(db.vods().unwrap()[0].outdated);
}
