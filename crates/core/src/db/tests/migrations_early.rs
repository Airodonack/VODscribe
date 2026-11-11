//! Upgrades of libraries from before the clip model (V1 to V21).

use super::*;

#[test]
fn copies_from_before_upgrades_go_after_thirty_days() {
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

#[test]
fn slices_written_over_by_an_older_version_are_dropped() {
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

#[test]
fn deleted_transcripts_leave_the_file() {
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
