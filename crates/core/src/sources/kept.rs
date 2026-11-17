//! The specs of kept stretches: what removing a VOD with its video makes of
//! a stretch other clips use (`Db::add_realized`, `Db::realize_piece`).

use std::path::Path;

use super::*;
use crate::chat::{ChatMessage, ChatPart, Kind};
use crate::edits::tests::info;
use crate::transcript::Segment;

fn word(text: &str, start: f64, end: f64) -> Word {
    Word { text: text.into(), start, end }
}

fn said(db: &mut Db, vod: i64, start: f64, end: f64, words: &[Word]) {
    let text: String = words.iter().map(|w| w.text.as_str()).collect();
    db.insert_segments(vod, &[Segment { start, end, text, words: words.to_vec() }]).unwrap();
}

fn message(time: f64, text: &str) -> ChatMessage {
    ChatMessage {
        time,
        author: "fan".into(),
        color: None,
        parts: vec![ChatPart::Text { text: text.into() }],
        kind: Kind::Message,
        amount: None,
        event_time: None,
        at: None,
    }
}

/// An edit with one piece of `vod`, `start`..`end` seconds: the edit and its piece.
fn an_edit(db: &Db, vod: i64, start: f64, end: f64) -> (i64, i64) {
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (start, end), "p", None).unwrap();
    (edit, db.piece_rows(edit).unwrap()[0].id)
}

fn seconds(at: f64) -> Flicks {
    Flicks::from_seconds(at)
}

/// What removing a VOD with its video leaves to keep is the pieces of clips
/// other than its own and its clip bar's, edit by edit (sources.rs
/// `pieces_using`: "The pieces of VOD `vod_id` in clips other than its own
/// and its clip bar's: what removing it leaves, to realize").
#[test]
fn what_removing_a_vod_leaves_to_keep_is_the_pieces_other_clips_have_of_it() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let other = db.add_vod(&info("y")).unwrap().0.id;
    db.add_clip(vod, 1.0, 2.0, "snippet", true).unwrap();
    let (first, a) = an_edit(&db, vod, 10.0, 15.0);
    let (_, b) = an_edit(&db, vod, 30.0, 35.0);
    db.add_piece(first, other, (1.0, 2.0), "of another VOD", None).unwrap();
    db.add_piece(first, vod, (50.0, 55.0), "again", None).unwrap();
    let again = db.piece_rows(first).unwrap()[2].id;
    assert_eq!(db.pieces_using(vod).unwrap().iter().map(|p| p.id).collect::<Vec<_>>(), [a, again, b]);
}

/// A stretch kept is a source of its own: the stretch's length, the VOD's
/// title, link and channel, and not in the list (sources.rs `add_realized`:
/// "A new source for `start`..`end` seconds of VOD `vod_id`: the VOD's
/// title, link and channel ... Not listed").
#[test]
fn a_kept_stretch_is_a_source_of_its_own_with_the_vods_title_and_link_not_in_the_list() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let mut x = info("x");
    x.channel = Some("Marlowe".into());
    let vod = db.add_vod(&x).unwrap().0.id;
    let new = db.add_realized(vod, 10.0, 15.0).unwrap();
    let source = db.vod(new).unwrap().unwrap();
    assert_ne!(new, vod);
    assert_eq!(
        (source.info.duration, source.listed, source.info.title.as_str(), source.info.url.as_str()),
        (Some(5.0), false, "VOD x", "https://youtu.be/x")
    );
    assert_eq!(source.info.channel.as_deref(), Some("Marlowe"));
}

/// A stretch kept has the words said in it, in its own time from 0, a line
/// cut at the stretch's edges keeping only the words inside it (sources.rs
/// `add_realized`: "its transcript and chat in that stretch, moved to the
/// new source's time (from 0)").
#[test]
fn a_kept_stretch_has_the_words_said_in_it_in_its_own_time() {
    let mut db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    // A line and words cut at the stretch's start and end, and ones that only touch it.
    said(&mut db, vod, 5.0, 12.0, &[word(" a", 5.0, 6.0), word(" k", 8.0, 10.0), word(" b", 9.5, 12.0)]);
    said(&mut db, vod, 8.0, 10.0, &[word(" e", 8.0, 10.0)]);
    said(&mut db, vod, 12.5, 14.0, &[word(" c", 12.5, 14.0)]);
    said(&mut db, vod, 14.0, 16.0, &[word(" i", 14.5, 15.5), word(" m", 15.0, 16.0)]);
    said(&mut db, vod, 15.0, 16.0, &[word(" f", 15.0, 16.0)]);
    said(&mut db, vod, 20.0, 21.0, &[word(" d", 20.0, 21.0)]);
    let new = db.add_realized(vod, 10.0, 15.0).unwrap();
    let lines = db.lines(new).unwrap();
    assert_eq!(
        lines.iter().map(|l| (l.start, l.end, l.text.as_str())).collect::<Vec<_>>(),
        [(0.0, 2.0, "b"), (2.5, 4.0, "c"), (4.0, 5.0, "i")]
    );
    let words = db.words_between(new, 0.0, 5.0).unwrap();
    assert_eq!(
        words.iter().map(|w| (w.text.as_str(), w.start, w.end)).collect::<Vec<_>>(),
        [(" b", 0.0, 2.0), (" c", 2.5, 4.0), (" i", 4.5, 5.0)]
    );
}

/// A stretch kept has the chat sent in it, in its own time from 0 (sources.rs
/// `add_realized`: "its transcript and chat in that stretch, moved to the
/// new source's time (from 0)"): from its start up to, not including, its end.
#[test]
fn a_kept_stretch_has_the_chat_sent_in_it_in_its_own_time() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let chat: Vec<_> = [(9.5, "before"), (10.0, "first"), (12.5, "middle"), (14.9, "last"), (15.0, "after")]
        .map(|(at, text)| message(at, text))
        .into();
    crate::db::insert_chat(&db.conn, vod, &chat).unwrap();
    let new = db.add_realized(vod, 10.0, 15.0).unwrap();
    let kept = db.chat_of_kind(new, Kind::Message).unwrap();
    let said: Vec<_> = kept.iter().map(|(_, m)| (m.time, m.parts.clone())).collect();
    assert_eq!(
        said,
        [
            (0.0, vec![ChatPart::Text { text: "first".into() }]),
            (2.5, vec![ChatPart::Text { text: "middle".into() }]),
            (4.9, vec![ChatPart::Text { text: "last".into() }]),
        ]
    );
}

/// A stretch kept holds as transcribed what of it the VOD had transcribed,
/// in its own time (sources.rs `add_realized`: "What of the stretch was
/// transcribed, in the new source's time"), and nothing of what was only asked.
#[test]
fn a_kept_stretch_is_transcribed_where_the_vod_was() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    db.insert_done(vod, 0.0, Some(12.0), Some("m")).unwrap();
    db.insert_done(vod, 14.0, Some(30.0), Some("m")).unwrap();
    db.insert_done(vod, 40.0, Some(50.0), Some("m")).unwrap();
    // Touching the stretch's edges only: nothing of it.
    db.insert_done(vod, 2.0, Some(10.0), Some("m")).unwrap();
    db.insert_done(vod, 15.0, Some(20.0), Some("m")).unwrap();
    db.conn.execute("UPDATE stretches SET status = 'waiting' WHERE start = 14.0", []).unwrap();
    let new = db.add_realized(vod, 10.0, 15.0).unwrap();
    let done: Vec<_> = db
        .stretches(new)
        .unwrap()
        .iter()
        .map(|s| (s.start, s.end, s.status == crate::db::Status::Done, s.model.clone()))
        .collect();
    assert_eq!(done, [(0.0, Some(2.0), true, Some("m".to_string()))]);
}

/// A stretch kept keeps who speaks, and subs, gifts and tips with their
/// amounts and when they happened (CHANGELOG.md, 0.7.0 Fixes: "Kept
/// stretches keep subs, gifts and tips.").
#[test]
fn a_kept_stretch_keeps_who_speaks_and_chat_events() {
    let mut db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    said(&mut db, vod, 11.0, 12.0, &[word(" hi", 11.0, 12.0)]);
    db.conn.execute("UPDATE segments SET speaker = 'Marlowe' WHERE vod_id = ?1", [vod]).unwrap();
    let sub = ChatMessage {
        color: Some("#ff0000".into()),
        kind: Kind::Sub,
        amount: Some(2.0),
        event_time: Some(12.5),
        ..message(13.0, "resubbed")
    };
    crate::db::insert_chat(&db.conn, vod, &[sub]).unwrap();
    let (edit, piece) = an_edit(&db, vod, 10.0, 15.0);

    // Kept, then the VOD deleted with its video.
    let new = db.add_realized(vod, 10.0, 15.0).unwrap();
    let base = db.revision(edit).unwrap().unwrap();
    assert!(db.realize_piece(piece, new, base).unwrap());
    db.delete_vod(vod).unwrap();

    let lines = db.lines(new).unwrap();
    assert_eq!(
        lines.iter().map(|l| (l.start, l.text.as_str(), l.speaker.as_deref())).collect::<Vec<_>>(),
        [(1.0, "hi", Some("Marlowe"))]
    );
    let subs = db.chat_of_kind(new, Kind::Sub).unwrap();
    assert_eq!(
        subs.iter().map(|(_, m)| (m.time, m.author.as_str(), m.amount, m.event_time)).collect::<Vec<_>>(),
        [(3.0, "fan", Some(2.0), Some(2.5))]
    );
}

/// A piece moves to the stretch kept for it: it keeps its id, plays the
/// stretch from 0, and has as its origin the time its file starts at in the
/// VOD it was of (sources.rs `realize_piece`: "Moves piece `piece` to source
/// `to`, which holds its stretch from 0: its times in `to`'s, its origin
/// where `to` starts in what it was of"; module doc: "the piece moves to it,
/// keeping its id and its origin").
#[test]
fn a_piece_moves_to_its_kept_stretch_keeping_its_id_with_the_origin_where_it_starts() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    db.add_clip(vod, 1.0, 2.0, "snippet", true).unwrap();
    let (edit, piece) = an_edit(&db, vod, 10.0, 15.0);
    let new = db.add_realized(vod, 10.0, 15.0).unwrap();
    assert!(db.realize_piece(piece, new, db.revision(edit).unwrap().unwrap()).unwrap());
    let row = db.piece_rows(edit).unwrap().remove(0);
    assert_eq!((row.id, row.vod_id, row.start, row.end), (piece, new, Flicks::ZERO, Some(seconds(5.0))));
    assert_eq!(row.origin, Some(seconds(10.0)));
    assert_eq!((db.uses(vod).unwrap(), db.uses(new).unwrap()), (2, 1), "the VOD's own clip and snippet still use it");
    assert!(db.pieces_using(vod).unwrap().is_empty());
}

/// A piece kept twice (its kept stretch itself removed with its video) still
/// has its origin in the VOD it first came from (edits.rs `PieceRow::origin`:
/// "A realized piece: the VOD's time its file starts at").
#[test]
fn a_piece_kept_twice_has_its_origin_where_it_first_came_from() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let (edit, piece) = an_edit(&db, vod, 10.0, 20.0);
    let first = db.add_realized(vod, 10.0, 20.0).unwrap();
    assert!(db.realize_piece(piece, first, db.revision(edit).unwrap().unwrap()).unwrap());
    let trimmed = db.piece_rows(edit).unwrap()[0].clone();
    assert_eq!(trimmed.origin, Some(seconds(10.0)));
    // The edit uses 2..5 s of what was kept; keeping that is a stretch again.
    db.change(edit, None, |db| {
        db.conn.execute(
            "UPDATE items SET start = ?2, end = ?3 WHERE id = ?1",
            params![piece, seconds(2.0).0, seconds(5.0).0],
        )?;
        Ok(())
    })
    .unwrap();
    let second = db.add_realized(first, 2.0, 5.0).unwrap();
    assert!(db.realize_piece(piece, second, db.revision(edit).unwrap().unwrap()).unwrap());
    let row = db.piece_rows(edit).unwrap().remove(0);
    assert_eq!((row.vod_id, row.start, row.end), (second, Flicks::ZERO, Some(seconds(3.0))));
    assert_eq!(row.origin, Some(seconds(12.0)));
}

/// A piece of a clip that changed since the stretch was made isn't moved: it
/// is told so, for the stretch to be made again (sources.rs `realize_piece`:
/// "A change to its clip against revision `base`, so one made from the clip
/// as it was before (`Stale` otherwise)").
#[test]
fn a_piece_of_a_clip_changed_since_its_stretch_was_made_is_not_moved() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let (edit, piece) = an_edit(&db, vod, 10.0, 15.0);
    let new = db.add_realized(vod, 10.0, 15.0).unwrap();
    let base = db.revision(edit).unwrap().unwrap();
    db.change(edit, None, |_| Ok(())).unwrap();
    let err = db.realize_piece(piece, new, base).unwrap_err();
    assert!(err.downcast_ref::<crate::edits::Stale>().is_some(), "{err:#}");
    let row = db.piece_rows(edit).unwrap().remove(0);
    assert_eq!((row.vod_id, row.start, row.end), (vod, seconds(10.0), Some(seconds(15.0))));
}

/// A piece isn't moved to a stretch that isn't its own: not one of another
/// length, and not a piece that's gone (sources.rs `realize_piece`: "false if
/// the piece is gone or no longer the stretch `to` holds").
#[test]
fn a_piece_is_not_moved_to_a_stretch_that_is_no_longer_its_own() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let (edit, piece) = an_edit(&db, vod, 10.0, 15.0);
    let longer = db.add_realized(vod, 10.0, 16.0).unwrap();
    let base = db.revision(edit).unwrap().unwrap();
    assert!(!db.realize_piece(piece, longer, base).unwrap());
    let row = db.piece_rows(edit).unwrap().remove(0);
    assert_eq!((row.vod_id, row.start, row.end), (vod, seconds(10.0), Some(seconds(15.0))));
    let new = db.add_realized(vod, 10.0, 15.0).unwrap();
    db.remove_piece(piece).unwrap();
    assert!(!db.realize_piece(piece, new, db.revision(edit).unwrap().unwrap()).unwrap());
}

/// A stretch kept that no piece moved to is a source nothing uses, left to
/// erase (sources.rs module doc: "A source stays while any piece uses it").
#[test]
fn a_kept_stretch_no_piece_moved_to_is_a_source_nothing_uses() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let (edit, piece) = an_edit(&db, vod, 10.0, 15.0);
    let unused = db.add_realized(vod, 10.0, 15.0).unwrap();
    assert_eq!(db.unused_sources().unwrap(), [unused]);
    assert!(db.realize_piece(piece, unused, db.revision(edit).unwrap().unwrap()).unwrap());
    assert!(db.unused_sources().unwrap().is_empty());
}

/// Every stretch other clips use is kept, though two pieces are of the same
/// stretch (sources.rs module doc: "removing a VOD and deleting its video
/// first realizes each stretch other clips use"), as two edits using the same
/// moments are.
#[test]
fn two_pieces_of_the_same_stretch_are_each_kept() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let (a, one) = an_edit(&db, vod, 10.0, 15.0);
    let (b, two) = an_edit(&db, vod, 10.0, 15.0);
    let first = db.add_realized(vod, 10.0, 15.0).unwrap();
    assert!(db.realize_piece(one, first, db.revision(a).unwrap().unwrap()).unwrap());
    let second = db.add_realized(vod, 10.0, 15.0).unwrap();
    assert_ne!(first, second);
    assert!(db.realize_piece(two, second, db.revision(b).unwrap().unwrap()).unwrap());
    assert!(db.pieces_using(vod).unwrap().is_empty());
}
