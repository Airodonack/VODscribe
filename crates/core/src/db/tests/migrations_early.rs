//! Upgrades of libraries from before the clip model (V1 to V21), and the
//! upgrade itself: the copy kept before it, a library from a newer version.

use super::*;
use crate::testkit::format_of;

/// A library at `path` as the VODscribe whose newest migration was
/// `version` left it (`testkit::at_version`), with `rows` put in as that
/// version wrote them. Its connection is closed, so `Db::open` upgrades it.
fn older(path: &Path, version: usize, rows: &str) {
    crate::testkit::at_version(path, version).execute_batch(rows).unwrap();
}

/// One VOD 0.1.0 had transcribed, saying "the eagle has landed" at 12 s.
const TRANSCRIBED: &str = r#"
INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES ('kick', 'x', 'u', 'Day 3', 'done', 0);
INSERT INTO segments (vod_id, start, end, text, words) VALUES (1, 12.0, 14.0, 'the eagle has landed',
    '[{"text":"the","start":12.0,"end":12.2},{"text":"eagle","start":12.2,"end":12.8},
      {"text":"has","start":12.8,"end":13.2},{"text":"landed","start":13.2,"end":14.0}]');
"#;

/// One VOD of `platform`, as any version from the first wrote it.
fn one_vod(platform: &str) -> String {
    format!(
        "INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES ('{platform}', 'a', 'u', 'Day 3', 'done', 0);"
    )
}

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

/// The copy at `copy` put in a library's place in a folder of its own, as
/// a user undoing an upgrade would, and opened.
fn put_back(copy: &Path) -> (crate::TempDir, Db) {
    let back = crate::TempDir::new("put-back");
    std::fs::copy(copy, back.join("library.sqlite")).unwrap();
    let db = Db::open(&back.join("library.sqlite")).unwrap();
    (back, db)
}

// The upgrade itself.

/// An upgrade keeps a copy of the library as it was, and only an upgrade
/// does (CHANGELOG.md, 0.4.0: "A copy of your library is kept before each
/// upgrade of its format (for 30 days)"; db.rs `keep_before_upgrade`:
/// "Copies the library … before it's upgraded, whole and as it stands").
#[test]
fn an_upgrade_keeps_a_copy_of_the_library_from_before() {
    let dir = crate::TempDir::new("copy-before");
    let path = dir.join("library.sqlite");
    older(&path, 1, TRANSCRIBED);
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
    older(&path, 1, TRANSCRIBED);
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

/// A copy reads as the version it was, so put back in the library's place
/// it's upgraded again (db.rs `keep_before_upgrade`: "VACUUM INTO doesn't
/// carry the format's number over").
#[test]
fn a_copy_reads_as_the_version_it_was() {
    let dir = crate::TempDir::new("copy-version");
    let path = dir.join("library.sqlite");
    older(&path, 1, TRANSCRIBED);
    drop(Db::open(&path).unwrap());
    let copy = dir.join("library-v1.sqlite");
    assert_eq!(format_of(&Connection::open(&copy).unwrap()), 1);
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
    older(&path, 1, TRANSCRIBED);
    // The copy's place is taken by a folder.
    let copy = dir.join("library-v1.sqlite");
    std::fs::create_dir(&copy).unwrap();
    assert!(Db::open(&path).is_err());
    assert_eq!(format_of(&Connection::open(&path).unwrap()), 1, "not upgraded");
    // Its place free again, the library upgrades with what it had.
    std::fs::remove_dir(&copy).unwrap();
    let db = Db::open(&path).unwrap();
    let vod = db.vods().unwrap().remove(0);
    assert_eq!(db.lines(vod.id).unwrap()[0].text, "the eagle has landed");
}

/// An upgrade is all or nothing: one that fails partway leaves the library
/// at its version with every row as it was, so it upgrades whole once what
/// stopped it is gone (db.rs `Db::open`: the pending migrations run between
/// `BEGIN;` and `COMMIT;`, the format's number set last). Here V22 refuses a
/// clip of a VOD that's gone, after V18 to V21 have run.
#[test]
fn an_upgrade_is_all_or_nothing() {
    let dir = crate::TempDir::new("all-or-nothing");
    let path = dir.join("library.sqlite");
    // With the foreign keys off, a clip can outlive its VOD.
    older(
        &path,
        17,
        "PRAGMA foreign_keys = OFF;
         INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES ('kick', 'a', 'u', 'Day 3', 'done', 0);
         INSERT INTO clips (vod_id, start, end, title, position, created_at)
             VALUES (1, 10.0, 20.0, 'Lunch', 0, 0), (2, 0.0, 5.0, 'Of a VOD gone', 1, 0);",
    );
    assert!(Db::open(&path).is_err(), "the upgrade refused the clip");
    assert_eq!(format_of(&Connection::open(&path).unwrap()), 17, "not upgraded");
    // That clip gone, the library upgrades as if the first try hadn't run.
    Connection::open(&path).unwrap().execute("DELETE FROM clips WHERE vod_id = 2", []).unwrap();
    let db = Db::open(&path).unwrap();
    let vod = db.vods().unwrap().remove(0);
    assert_eq!(vod.info.title, "Day 3");
    let clips: Vec<_> = db.clips(vod.id).unwrap().into_iter().map(|c| (c.title, c.start, c.end)).collect();
    assert_eq!(clips, [("Lunch".to_string(), 10.0, 20.0)]);
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

// One per migration.

/// A library from the first release opens with its VODs, and their
/// transcripts are searchable (CHANGELOG.md, 0.1.0: "First release.";
/// tests/e2e/scenarios/upgrade.mjs: "a library and settings as 0.3.0 left
/// them … open in this version with the VOD, its transcript (one stretch,
/// all of it)").
#[test]
fn a_library_from_the_first_release_opens_with_its_vods_and_transcripts_searchable() {
    let dir = crate::TempDir::new("first-release");
    let path = dir.join("library.sqlite");
    older(&path, 1, TRANSCRIBED);
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
    older(&path, 1, TRANSCRIBED);
    let db = Db::open(&path).unwrap();
    assert_eq!(db.vods().unwrap()[0].note, None);
}

/// A VOD downloaded before its audio was kept has none, so it's downloaded
/// again when asked to transcribe (db.rs `MIGRATE_V3`: "the finished
/// download, so a resumed or retried VOD skips straight to transcription").
#[test]
fn a_vod_downloaded_before_audio_was_kept_is_downloaded_again_when_asked() {
    let dir = crate::TempDir::new("before-audio");
    let path = dir.join("library.sqlite");
    older(&path, 2, &one_vod("kick"));
    let db = Db::open(&path).unwrap();
    let vod = db.vods().unwrap().remove(0);
    assert_eq!(db.audio_path(vod.id).unwrap(), None, "no audio to skip to");
}

/// A transcript from before the speech model was recorded reads as
/// outdated (db.rs `MIGRATE_V4`: "the speech model that produced the
/// transcript"; `Vod::outdated`: "Has transcript segments from a model this
/// version no longer uses (or from before models were recorded)").
#[test]
fn a_transcript_from_before_models_were_recorded_reads_as_outdated() {
    let dir = crate::TempDir::new("before-models");
    let path = dir.join("library.sqlite");
    older(&path, 1, TRANSCRIBED);
    let db = Db::open(&path).unwrap();
    assert!(db.vods().unwrap()[0].outdated);
}

/// A library from before its order was kept lists its VODs newest first
/// (db.rs `MIGRATE_V5`: "the library's order, which the user can
/// rearrange. Starts newest first"). Two added at once: the later added first.
#[test]
fn a_library_from_before_its_order_was_kept_lists_newest_first() {
    let dir = crate::TempDir::new("before-order");
    let path = dir.join("library.sqlite");
    older(
        &path,
        4,
        "INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES
             ('kick', 'a', 'u', 'Monday', 'done', 10), ('kick', 'b', 'u', 'Friday', 'done', 50),
             ('kick', 'c', 'u', 'Wednesday', 'done', 30), ('kick', 'd', 'u', 'Thursday', 'done', 30);",
    );
    let db = Db::open(&path).unwrap();
    let titles: Vec<_> = db.listed_vods().unwrap().into_iter().map(|v| v.info.title).collect();
    assert_eq!(titles, ["Friday", "Thursday", "Wednesday", "Monday"]);
}

/// Deleted words don't stay in the file of a library upgraded from before
/// the search index deleted them (db.rs `MIGRATE_V6`: "deleting a
/// transcript removes its words from the search index right away";
/// `MIGRATE_V13`: "so deleted words still don't stay in the file").
#[test]
fn deleted_words_do_not_stay_in_the_library_file() {
    let dir = crate::TempDir::new("delete");
    let path = dir.join("library.sqlite");
    let secret = "zanzibarquokka";
    // Another VOD stays, sharing the search indexes with the one removed.
    let lines = [(1, "plain words 0"), (2, secret), (1, "plain words 2")];
    let mut rows = String::from(
        "INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES
             ('kick', 'kept', 'u', 't', 'done', 0), ('kick', 'x', 'u', 't', 'done', 0);",
    );
    for (i, (vod, text)) in lines.iter().enumerate() {
        let words = serde_json::to_string(&words(text, i as f64)).unwrap();
        rows += &format!(
            "INSERT INTO segments (vod_id, start, end, text, words) VALUES ({vod}, {i}, {i}.5, '{text}', '{words}');"
        );
    }
    older(&path, 5, &rows);
    let mut db = Db::open(&path).unwrap();
    for (i, (vod, text)) in lines.iter().enumerate() {
        db.add_chat(*vod, &[message(i as f64, "v", &format!("chat{text}"))], &[i as f64]).unwrap();
    }
    assert_eq!(db.search(secret, &SearchFilter::default(), 10).unwrap().hits.len(), 1);
    while db.delete_some_of(2, 1000).unwrap() > 0 {}
    // A batch saved after the erase, by a fetch that hadn't stopped yet.
    db.add_chat(2, &[message(9.0, "v", "late quokkazanzibar")], &[9.0]).unwrap();
    db.delete_vod(2).unwrap();
    assert_eq!(db.search("words", &SearchFilter::default(), 10).unwrap().hits.len(), 4, "the other VOD's stay");
    drop(db);
    let bytes =
        [std::fs::read(&path).unwrap(), std::fs::read(path.with_extension("sqlite-wal")).unwrap_or_default()].concat();
    assert!(!bytes.windows(8).any(|w| w == &secret.as_bytes()[6..]), "deleted text still on disk");
    assert!(!bytes.windows(8).any(|w| w == b"zanzibar"), "a late batch's text still on disk");
}

/// A VOD from before its picture was kept has none yet, its site's picture
/// still known to fetch it from (db.rs `MIGRATE_V7`: "the file name of the
/// VOD's local thumbnail"; `Vod::thumb`: "`None` until fetched").
#[test]
fn a_vod_from_before_its_picture_was_kept_has_none() {
    let dir = crate::TempDir::new("before-thumbs");
    let path = dir.join("library.sqlite");
    older(
        &path,
        6,
        "INSERT INTO vods (platform, platform_id, url, title, thumbnail, status, added_at)
             VALUES ('kick', 'a', 'u', 'Day 3', 'https://kick.example/a.jpg', 'done', 0);",
    );
    let db = Db::open(&path).unwrap();
    let vod = db.vods().unwrap().remove(0);
    assert_eq!(vod.thumb, None);
    assert_eq!(vod.info.thumbnail.as_deref(), Some("https://kick.example/a.jpg"));
}

/// A VOD from before files on this computer were added is from a site: it
/// has no file here to read (db.rs `MIGRATE_V8`: "for a file on this
/// computer (see `local`), where it is").
#[test]
fn a_vod_from_before_files_on_this_computer_is_from_a_site() {
    let dir = crate::TempDir::new("before-local");
    let path = dir.join("library.sqlite");
    older(&path, 7, &one_vod("kick"));
    let db = Db::open(&path).unwrap();
    let vod = db.vods().unwrap().remove(0);
    assert_eq!((vod.info.platform.as_str(), vod.source_path), ("kick", None));
    assert_eq!(db.playable_path(vod.id).unwrap(), None);
}

/// A library from before chat has each VOD's chat still to fetch, and
/// searches it once fetched (db.rs `MIGRATE_V9`: "chat replays …,
/// searchable like the transcript, and whether each VOD's has been
/// fetched"; CHANGELOG.md, 0.2.0: "Chat:" and "Search covers chat too.").
#[test]
fn a_library_from_before_chat_has_its_chat_to_fetch_and_searches_it_once_fetched() {
    let dir = crate::TempDir::new("before-chat");
    let path = dir.join("library.sqlite");
    older(&path, 8, &one_vod("kick"));
    let mut db = Db::open(&path).unwrap();
    let vod = db.vods().unwrap().remove(0);
    assert_eq!(vod.chat, ChatStatus::Pending);
    assert_eq!(db.chat_pending().unwrap(), [vod.id], "to fetch");
    db.set_chat(vod.id, &[message(3.0, "viewer", "pogchamp")]).unwrap();
    let hits = db.search("pogchamp", &SearchFilter::default(), 10).unwrap().hits;
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].vod_id, hits[0].source, hits[0].time), (vod.id, Source::Chat, 3.0));
}

/// Chat fetched before its kinds were kept is fetched again, and reads as
/// messages till then (db.rs `MIGRATE_V10`: "Chat fetched before has
/// neither, so it's fetched again"; CHANGELOG.md, 0.2.0: "Subs, gifts and
/// tips … are marked in chat").
#[test]
fn chat_fetched_before_kinds_is_fetched_again() {
    let dir = crate::TempDir::new("migrate-v10");
    let path = dir.join("library.sqlite");
    older(
        &path,
        9,
        "INSERT INTO vods (platform, platform_id, url, title, status, added_at, chat) VALUES
             ('kick', 'a', 'u', 't', 'done', 0, 'done'), ('local', 'b', 'u', 't', 'done', 0, 'none');
         INSERT INTO chat (vod_id, time, author, text, parts) VALUES (1, 1.0, 'v', 'hi', '[]');",
    );
    let db = Db::open(&path).unwrap();
    assert_eq!(db.vod(1).unwrap().unwrap().chat, ChatStatus::Pending, "fetched again");
    assert_eq!(db.vod(2).unwrap().unwrap().chat, ChatStatus::None, "no chat stays none");
    assert_eq!(db.chat(1).unwrap()[0].kind, Kind::Message, "kept until fetched again");
}

/// A chat from before fetches could resume has no place to resume from,
/// and its messages' events are when they were sent (db.rs `MIGRATE_V11`:
/// "where an unfinished fetch got to …, and when a message's event
/// happened, if not when it was sent"; CHANGELOG.md, 0.2.0: "carries on
/// where it stopped if interrupted").
#[test]
fn a_chat_from_before_resuming_has_no_place_to_resume_from() {
    let dir = crate::TempDir::new("before-resume");
    let path = dir.join("library.sqlite");
    // A fetch under way: no chat status yet, a sub fetched.
    older(
        &path,
        10,
        "INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES ('kick', 'a', 'u', 't', 'done', 0);
         INSERT INTO chat (vod_id, time, author, text, parts, kind) VALUES (1, 4.0, 'v', 'subbed', '[]', 'sub');",
    );
    let db = Db::open(&path).unwrap();
    assert_eq!(db.vod(1).unwrap().unwrap().chat, ChatStatus::Pending);
    assert_eq!(db.chat_slices(1).unwrap(), None, "no place to resume from");
    let subs = db.chat_of_kind(1, Kind::Sub).unwrap();
    assert_eq!((subs[0].1.time, subs[0].1.event_time), (4.0, None), "the sub's event is when it was sent");
}

/// A VOD canceled before reads as stopped, transcribed as far as it got,
/// where one that failed still reads as failed (db.rs `MIGRATE_V12`: "a VOD
/// the user stopped says \"Stopped\", as the app has written since 0.1.0's
/// \"Canceled\" (the app shows either as stopped, not failed)"; at today's
/// seam, `MIGRATE_V27`: "a stopped one, done as far as its lines go").
#[test]
fn a_vod_canceled_before_reads_as_stopped() {
    let dir = crate::TempDir::new("migrate-v12");
    let path = dir.join("library.sqlite");
    older(
        &path,
        11,
        "INSERT INTO vods (platform, platform_id, url, title, status, error, added_at) VALUES
             ('kick', 'a', 'u', 't', 'error', 'Canceled', 0), ('kick', 'b', 'u', 't', 'error', 'No audio', 0);
         INSERT INTO segments (vod_id, start, end, text, words) VALUES (1, 0.0, 5.0, 'so far', '[]');",
    );
    let db = Db::open(&path).unwrap();
    let stopped: Vec<_> = db.stretches(1).unwrap().into_iter().map(|s| (s.status, s.start, s.end)).collect();
    assert_eq!(stopped, [(Status::Done, 0.0, Some(5.0))], "done as far as its lines go, no request failed");
    let canceled = db.vod(1).unwrap().unwrap();
    assert_eq!((canceled.status, canceled.error), (Status::None, None), "stopped, not failed");
    let failed = db.vod(2).unwrap().unwrap();
    assert_eq!((failed.status, failed.error.as_deref()), (Status::Error, Some("No audio")));
}

/// An upgraded library never gives a deleted VOD's id again (db.rs
/// `MIGRATE_V14`: "a deleted VOD's id is never given to a new one, so work
/// still running for the deleted VOD … can't land on the new one";
/// CHANGELOG.md, 0.3.0 Fixes: "Chat or a thumbnail still arriving for a
/// removed VOD could land on the next one added.").
#[test]
fn an_upgraded_library_never_gives_a_deleted_vods_id_again() {
    let dir = crate::TempDir::new("ids");
    let path = dir.join("library.sqlite");
    // A library from before v14 with two VODs, 2 the newest.
    older(
        &path,
        13,
        "INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES
             ('kick', 'a', 'u', 't', 'done', 0), ('kick', 'b', 'u', 't', 'done', 0);",
    );
    let db = Db::open(&path).unwrap();
    // The newest from before deleted after the upgrade.
    db.delete_vod(2).unwrap();
    let a = db.add_vod(&info("a2")).unwrap().0;
    assert!(a.id > 2, "VOD 2's id isn't given out again");
    db.delete_vod(a.id).unwrap();
    let b = db.add_vod(&info("b2")).unwrap().0;
    assert!(b.id > a.id, "the deleted VOD's id isn't given out again");
    // Even after a VOD added without taking an id (as an older version would).
    db.conn
        .execute("INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES ('kick', 'c', 'u', 't', 'done', 0)", [])
        .unwrap();
    let by_hand = db.conn.last_insert_rowid();
    db.delete_vod(by_hand).unwrap();
    assert!(db.add_vod(&info("d2")).unwrap().0.id > by_hand);
}

/// A chat fetch an older version left unfinished starts over (db.rs
/// `MIGRATE_V15`: "a fetch an older version left unfinished starts over,
/// and so does one an older version touched since (`chat_slices`)";
/// CHANGELOG.md, 0.3.0: "Kick and Twitch chats download 3 to 5 times
/// faster.").
#[test]
fn a_chat_an_older_version_left_unfinished_starts_over() {
    let dir = crate::TempDir::new("before-slices");
    let path = dir.join("library.sqlite");
    // Fetched to 5 s, one position, when it stopped.
    older(
        &path,
        14,
        "INSERT INTO vods (platform, platform_id, url, title, status, added_at, chat_resume)
             VALUES ('kick', 'a', 'u', 't', 'done', 0, 5.0);
         INSERT INTO chat (vod_id, time, author, text, parts) VALUES (1, 1.0, 'v', 'hi', '[]');",
    );
    let db = Db::open(&path).unwrap();
    assert_eq!(db.vod(1).unwrap().unwrap().chat, ChatStatus::Pending, "still to fetch");
    assert_eq!(db.chat_slices(1).unwrap(), None, "from the start");
}

/// A chat fetch an older version touched since starts over (db.rs
/// `MIGRATE_V15`: "a fetch an older version left unfinished starts over,
/// and so does one an older version touched since (`chat_slices`)";
/// CHANGELOG.md, 0.3.0: "Kick and Twitch chats download 3 to 5 times
/// faster.").
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

/// A VOD from before videos were kept has none kept, and nothing to play
/// here (db.rs `MIGRATE_V16`: "the video kept to play here, for sites other
/// than YouTube, Twitch and Kick"; CHANGELOG.md, 0.3.0: "VODs from other
/// sites").
#[test]
fn a_vod_from_before_videos_were_kept_has_none_kept() {
    let dir = crate::TempDir::new("before-media");
    let path = dir.join("library.sqlite");
    older(&path, 15, &one_vod("vimeo"));
    let db = Db::open(&path).unwrap();
    let vod = db.vods().unwrap().remove(0);
    assert!(!vod.media);
    assert_eq!(db.media_path(vod.id).unwrap(), None);
    assert_eq!(db.playable_path(vod.id).unwrap(), None);
}

/// A library from before clips has none (db.rs `MIGRATE_V17`: "clips,
/// ranges of a VOD made into video files of their own"; CHANGELOG.md,
/// 0.4.0: "Clips:").
#[test]
fn a_library_from_before_clips_has_none() {
    let dir = crate::TempDir::new("before-clips");
    let path = dir.join("library.sqlite");
    older(&path, 16, &one_vod("kick"));
    let db = Db::open(&path).unwrap();
    let vod = db.listed_vods().unwrap().remove(0);
    assert_eq!(db.clips(vod.id).unwrap().len(), 0);
}

/// A library from before marks has none (db.rs `MIGRATE_V18`: "marks on a
/// VOD's timeline"; CHANGELOG.md, 0.4.0: "Chapters and highlights on the
/// timeline").
#[test]
fn a_library_from_before_marks_has_none() {
    let dir = crate::TempDir::new("before-marks");
    let path = dir.join("library.sqlite");
    older(&path, 17, &one_vod("kick"));
    let db = Db::open(&path).unwrap();
    let vod = db.vods().unwrap().remove(0);
    assert_eq!(db.marks(vod.id).unwrap().len(), 0);
}

/// A library from before edits has none (db.rs `MIGRATE_V19`: "edits, each
/// one video made of stretches of clips"; CHANGELOG.md, 0.4.0: "Edits:").
#[test]
fn a_library_from_before_edits_has_none() {
    let dir = crate::TempDir::new("before-edits");
    let path = dir.join("library.sqlite");
    older(&path, 18, &one_vod("kick"));
    let db = Db::open(&path).unwrap();
    assert_eq!(db.edits(crate::edits::tests::at_first()).unwrap().len(), 0);
}

/// An edit's pieces of clips become stretches of their own, of the VOD,
/// within the clip they were of and titled as it was (db.rs `MIGRATE_V20`:
/// "an edit's pieces are their own stretches of VODs, with a title, rather
/// than of clips (so one can change without another)"; CHANGELOG.md, 0.4.0:
/// "What's put in an edit is its own copy."). Its stretch struck is taken
/// out (V46: `struck_stretches_become_splits`).
#[test]
fn an_edits_pieces_of_clips_become_their_own_stretches() {
    let dir = crate::TempDir::new("migrate-v19");
    let path = dir.join("library.sqlite");
    older(
        &path,
        19,
        "INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES ('kick', 'a', 'u', 't', 'done', 0);
         INSERT INTO clips (vod_id, start, end, title, position, created_at) VALUES (1, 10.0, 20.0, 'Lunch', 1, 0);
         INSERT INTO edits (title, position, created_at) VALUES ('e', 0, 0);
         INSERT INTO edit_pieces (edit_id, clip_id, position, start, end, struck)
             VALUES (1, 1, 0, 5.0, 18.0, '[[12.0,13.0]]');",
    );
    let db = Db::open(&path).unwrap();
    let edit = db.edits(crate::edits::tests::at_first()).unwrap()[0].id;
    let pieces: Vec<_> =
        db.edit_pieces(edit).unwrap().into_iter().map(|p| (p.vod_id, p.start, p.end, p.title)).collect();
    let lunch = |start, end| (1, start, end, "Lunch".to_string());
    assert_eq!(pieces, [lunch(10.0, 12.0), lunch(13.0, 18.0)], "within the clip it was of");
    // The clip's gone; the pieces stay.
    db.delete_clip(1).unwrap();
    assert_eq!(db.edit_pieces(edit).unwrap().len(), 2);
}

/// A clip whose file was dragged out before V20 counts as not handed out:
/// nothing recorded it then (db.rs `MIGRATE_V20`: "clips record whether
/// their file was dragged out of the app (`handed_out`), as then it's
/// another app's to keep").
#[test]
fn a_clip_dragged_out_before_v20_counts_as_not_handed_out() {
    let dir = crate::TempDir::new("before-clip-handed-out");
    let path = dir.join("library.sqlite");
    older(
        &path,
        19,
        "INSERT INTO vods (platform, platform_id, url, title, status, added_at) VALUES ('kick', 'a', 'u', 't', 'done', 0);
         INSERT INTO clips (vod_id, start, end, title, position, created_at, status, file_path)
             VALUES (1, 10.0, 20.0, 'Lunch', 0, 0, 'ready', '/clips/1.mp4');",
    );
    let db = Db::open(&path).unwrap();
    let clip = db.clips(1).unwrap().remove(0);
    assert_eq!((clip.id, clip.title.as_str()), (1, "Lunch"));
    assert!(!db.clip_handed_out(clip.id).unwrap());
}

/// An edit rendered before V21 counts as not handed out: nothing recorded
/// it then (db.rs `MIGRATE_V21`: "edits record whether their rendered video
/// was dragged out of the app (`handed_out`), as clips do"; CHANGELOG.md,
/// 0.4.0: "Files you made and no longer use don't pile up … unless you
/// dragged that file into another app.").
#[test]
fn an_edit_rendered_before_v21_counts_as_not_handed_out() {
    let dir = crate::TempDir::new("before-edit-handed-out");
    let path = dir.join("library.sqlite");
    older(
        &path,
        20,
        "INSERT INTO edits (title, position, created_at, status, file_path, rendered_version)
             VALUES ('Best of', 0, 0, 'ready', '/edits/1.mp4', 0);",
    );
    let db = Db::open(&path).unwrap();
    let edit = db.edits(crate::edits::tests::at_first()).unwrap().remove(0);
    assert_eq!((edit.title.as_str(), edit.status), ("Best of", ClipStatus::Ready), "rendered");
    assert!(!db.edit_handed_out(edit.id, false).unwrap());
}
