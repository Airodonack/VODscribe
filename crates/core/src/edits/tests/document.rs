//! The edit document through `Db`: an edit's pieces and groups, split,
//! trimmed, moved and taken out; its marks where they play; its clip bar;
//! what's put in it; undo's state of it; and the VODs it uses.

use super::*;
use crate::time::Rate;

/// Clip `id`'s marks as its timeline shows them (`place_marks`): each one's
/// id and where it starts.
fn placed(db: &Db, id: i64) -> Vec<(i64, f64)> {
    place_marks(&db.edit_pieces(id).unwrap(), &db.clip_marks(id).unwrap()).iter().map(|m| (m.id, m.start)).collect()
}

/// Where mark `mark` of clip `id` shows on its timeline; None where it doesn't.
fn shown_at(db: &Db, id: i64, mark: i64) -> Option<f64> {
    placed(db, id).into_iter().find(|(m, _)| *m == mark).map(|(_, start)| start)
}

/// Clip `id`'s pieces in its order: each one's stretch, and the title of the
/// group it's in at the clip's top level.
fn shape(db: &Db, id: i64) -> Vec<(f64, f64, Option<String>)> {
    db.edit_pieces(id).unwrap().iter().map(|p| (p.start, p.end, p.group.as_ref().map(|g| g.title.clone()))).collect()
}

/// Clip `id`'s pieces' titles, in its order.
fn titles(db: &Db, id: i64) -> Vec<String> {
    db.edit_pieces(id).unwrap().iter().map(|p| p.title.clone()).collect()
}

/// An error's whole message.
fn said<T>(r: Result<T>) -> Option<String> {
    r.err().map(|e| format!("{e:#}"))
}

// The edit and its pieces.

/// A new edit has nothing in it and no file (edits.rs `Db::add_edit`: "A
/// new, empty edit"; `Edit::status`: "waiting (never rendered)").
#[test]
fn a_new_edit_is_empty_and_not_rendered() {
    let (db, _, edit) = with_edit();
    let e = db.edit(edit, at_first()).unwrap().unwrap();
    assert_eq!(
        (e.pieces, e.length, e.current, e.status, e.file_path.clone()),
        (0, 0.0, false, ClipStatus::Waiting, None)
    );
    assert!(e.ranges.is_empty() && e.vod_ids.is_empty());
    assert_eq!(db.edits(at_first()).unwrap(), [e]);
}

/// VODs and edits are one list, a new one at its top, put in any order
/// (CHANGELOG.md, 0.4.0: "An edit sits in the library's list beside your
/// VODs"; edits.rs `LibraryItem`: "VODs and edits share one order";
/// `Db::add_edit`: "at the top of the library's list (as new VODs go)").
#[test]
fn vods_and_edits_share_one_order() {
    let db = library();
    let a = db.add_vod(&info("a")).unwrap().0.id;
    let e = db.add_edit("e").unwrap().id;
    let b = db.add_vod(&info("b")).unwrap().0.id;
    use LibraryItem::{Edit, Vod};
    assert_eq!(db.library_order().unwrap(), [Vod(b), Edit(e), Vod(a)], "newest on top");
    db.reorder_library(&[Vod(a), Vod(b), Edit(e)]).unwrap();
    assert_eq!(db.library_order().unwrap(), [Vod(a), Vod(b), Edit(e)]);
    assert_eq!(db.vods().unwrap().iter().map(|v| v.id).collect::<Vec<_>>(), [a, b]);
    assert_eq!(serde_json::to_string(&Edit(e)).unwrap(), format!(r#"{{"kind":"edit","id":{e}}}"#));
}

/// A stretch put in an edit goes where it's put, or last (edits.rs
/// `Db::add_piece`: "at `index` (or last)").
#[test]
fn pieces_go_where_they_are_put_or_last() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    db.add_piece(edit, vod, (30.0, 35.0), "b", Some(0)).unwrap();
    db.add_piece(edit, vod, (40.0, 45.0), "c", Some(1)).unwrap();
    db.add_piece(edit, vod, (50.0, 55.0), "d", Some(99)).unwrap();
    assert_eq!(titles(&db, edit), ["b", "c", "a", "d"]);
}

/// Pieces are reordered by dragging them, or from the keyboard (CHANGELOG.md,
/// 0.4.0: "In an edit, drag pieces to reorder them"; 0.6.0: "Ctrl+← and
/// Ctrl+→ ... move the piece at the playhead earlier or later"; edits.rs
/// `Db::move_piece`).
#[test]
fn a_piece_moved_goes_where_it_is_moved() {
    let (db, vod, edit) = with_edit();
    for (start, title) in [(10.0, "a"), (30.0, "b"), (50.0, "c")] {
        db.add_piece(edit, vod, (start, start + 5.0), title, None).unwrap();
    }
    let a = db.piece_rows(edit).unwrap()[0].id;
    db.move_piece(a, 2).unwrap();
    assert_eq!(titles(&db, edit), ["b", "c", "a"]);
    db.move_piece(a, 0).unwrap();
    assert_eq!(titles(&db, edit), ["a", "b", "c"]);
}

/// A piece's edges make it shorter or longer, never before its VOD's start
/// (CHANGELOG.md, 0.4.0: "drag ... their edges to make them shorter or
/// longer"; edits.rs `Db::set_piece_range`: "within the VOD").
#[test]
fn a_piece_s_edges_make_it_shorter_or_longer() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let piece = db.piece_rows(edit).unwrap()[0].id;
    db.set_piece_range(piece, 0.0, 18.0).unwrap();
    assert_eq!(stretches(&db, edit), [(0.0, 18.0)], "longer at its start, shorter at its end");
    db.set_piece_range(piece, 10.0, 18.0).unwrap();
    assert_eq!(stretches(&db, edit), [(10.0, 18.0)]);
    assert!(db.set_piece_range(piece, -5.0, 18.0).is_err(), "not before the VOD");
    assert_eq!(stretches(&db, edit), [(10.0, 18.0)], "left as it was");
}

/// A piece is cut at its VOD's end, and one wholly past it is refused
/// (edits.rs `Db::vod_span`: "its end cut at the VOD's"; `Db::set_piece_range`:
/// "within the VOD").
#[test]
fn pieces_stay_within_their_vod() {
    let (db, vod, edit) = with_edit();
    assert!(db.add_piece(edit, vod, (700.0, 800.0), "past", None).is_err(), "all past the end (600 s)");
    db.add_piece(edit, vod, (590.0, 650.0), "a", None).unwrap();
    assert_eq!(stretches(&db, edit), [(590.0, 600.0)], "cut at the end");
    let piece = db.piece_rows(edit).unwrap()[0].id;
    assert!(db.set_piece_range(piece, 700.0, 800.0).is_err(), "trimmed past the end");
    assert_eq!(stretches(&db, edit), [(590.0, 600.0)], "left as it was");
}

/// What goes in comes out, to the flick (edits.rs, the module's doc: "Every
/// time in the record is in flicks (`crate::time`); this module takes and
/// gives seconds at its edge").
#[test]
fn a_piece_keeps_its_times_to_the_flick() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.123456, 18.5), "a", None).unwrap();
    let row = &db.piece_rows(edit).unwrap()[0];
    assert_eq!((row.start, row.end), (Flicks::from_seconds(10.123456), Some(Flicks::from_seconds(18.5))));
    assert!(same_times(&stretches(&db, edit), &[(10.123456, 18.5)]));
}

/// Split (B) makes two pieces of one where it's asked, the second right
/// after the first, both titled as it was; a moment not inside it splits
/// nothing (CHANGELOG.md, 0.4.0: "split at the playhead (B)"; edits.rs
/// `Db::split_piece`: "the second right after the first. False if `at`
/// isn't inside it").
#[test]
fn a_split_makes_two_pieces_the_second_right_after_the_first() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 18.0), "a", None).unwrap();
    db.add_piece(edit, vod, (30.0, 35.0), "b", None).unwrap();
    let a = db.piece_rows(edit).unwrap()[0].id;
    assert!(db.split_piece(a, 16.0).unwrap());
    assert_eq!(stretches(&db, edit), [(10.0, 16.0), (16.0, 18.0), (30.0, 35.0)]);
    assert_eq!(titles(&db, edit), ["a", "a", "b"]);
    assert!(!db.split_piece(a, 3.0).unwrap(), "not inside it");
    assert!(!db.split_piece(a, 17.0).unwrap(), "past its end, in what was split off");
    assert_eq!(stretches(&db, edit).len(), 3);
}

/// A split at a piece's edge, or a sliver from it, splits nothing, so no
/// sliver is left (CHANGELOG.md, 0.7.1 Fixes: "Splitting at a join leaves no
/// sliver."; edits.rs `SLIVER`: "Stretches shorter than this are left out").
#[test]
fn a_split_a_sliver_from_a_piece_s_edge_leaves_no_sliver() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let a = db.piece_rows(edit).unwrap()[0].id;
    for at in [10.0, 10.01, 19.99, 20.0] {
        assert!(!db.split_piece(a, at).unwrap(), "at {at}");
    }
    assert_eq!(stretches(&db, edit), [(10.0, 20.0)]);
    assert!(db.split_piece(a, 10.05).unwrap(), "more than a sliver in");
}

/// Each piece is its own: changing one changes no other (edits.rs, the
/// module's doc: "changing one piece (trimming it, later growing it)
/// changes no other"; CHANGELOG.md, 0.4.0: "What's put in an edit is its own
/// copy").
#[test]
fn pieces_are_their_own_so_changing_one_leaves_the_others() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 18.0), "a", None).unwrap();
    let a = db.piece_rows(edit).unwrap()[0].id;
    assert!(db.split_piece(a, 16.0).unwrap());
    let second = db.piece_rows(edit).unwrap()[1].id;
    db.set_piece_range(second, 16.0, 40.0).unwrap();
    assert_eq!(stretches(&db, edit), [(10.0, 16.0), (16.0, 40.0)]);
}

/// A piece shorter than a sliver is kept but plays nothing (edits.rs
/// `SLIVER`: "Stretches shorter than this are left out (a sliver)").
#[test]
fn a_piece_shorter_than_a_sliver_plays_nothing() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (5.0, 5.02), "a", None).unwrap();
    db.add_piece(edit, vod, (10.0, 12.0), "b", None).unwrap();
    let e = db.edit(edit, at_first()).unwrap().unwrap();
    assert_eq!(e.pieces, 2);
    assert_eq!(e.ranges, [EditRange { vod_id: vod, start: 10.0, end: 12.0 }]);
    assert_eq!(e.length, 2.0);
}

/// An edit says what it plays of each VOD, in order, how long it plays and
/// which VODs it uses (edits.rs `Edit`: "How many pieces it has, and how long
/// it plays", "The VODs its pieces are of", "What it plays of each VOD, in
/// order").
#[test]
fn an_edit_says_what_it_plays_of_each_vod() {
    let (db, x, edit) = with_edit();
    let y = db.add_vod(&info("y")).unwrap().0.id;
    db.add_piece(edit, y, (30.0, 35.0), "b", None).unwrap();
    db.add_piece(edit, x, (10.0, 18.0), "a", None).unwrap();
    db.add_piece(edit, y, (40.0, 41.0), "c", None).unwrap();
    let e = db.edit(edit, at_first()).unwrap().unwrap();
    assert_eq!((e.pieces, e.length), (3, 14.0));
    let mut both = [x, y];
    both.sort();
    assert_eq!(e.vod_ids, both);
    let ranges: Vec<_> = e.ranges.iter().map(|r| (r.vod_id, r.start, r.end)).collect();
    assert_eq!(ranges, [(y, 30.0, 35.0), (x, 10.0, 18.0), (y, 40.0, 41.0)]);
}

/// A piece removed leaves the edit, the others staying in their order
/// (edits.rs `Db::remove_piece`).
#[test]
fn a_removed_piece_leaves_the_edit() {
    let (db, vod, edit) = with_edit();
    for (start, title) in [(10.0, "a"), (30.0, "b"), (50.0, "c")] {
        db.add_piece(edit, vod, (start, start + 5.0), title, None).unwrap();
    }
    db.remove_piece(db.piece_rows(edit).unwrap()[1].id).unwrap();
    assert_eq!(titles(&db, edit), ["a", "c"]);
}

/// An edit renamed is listed by its new name (e2e edits.mjs: "the edit
/// opened, renamed"; edits.rs `Db::rename_edit`).
#[test]
fn a_renamed_edit_has_its_new_title() {
    let (db, _, edit) = with_edit();
    db.rename_edit(edit, "Race night").unwrap();
    assert_eq!(db.edit(edit, at_first()).unwrap().unwrap().title, "Race night");
    assert_eq!(db.edits(at_first()).unwrap()[0].title, "Race night");
}

/// Deleting an edit takes its pieces, marks and clip bar's clips with it;
/// a VOD's own clip, and a clip of a clip bar, aren't edits to delete
/// (CHANGELOG.md, 0.7.1 Fixes: "Deleting an edit deletes its clips' files. The
/// clips on its clip bar go with their files"; edits.rs `Db::delete_edit`:
/// "with its items, marks and snippets", "false if it isn't").
#[test]
fn deleting_an_edit_takes_its_clip_bar_s_clips_with_it() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    db.add_clip_mark(edit, vod, "chapter", 12.0, None).unwrap();
    let on_bar = db.add_edit_clip(edit, 0.0, 5.0, "c", true).unwrap().id;
    let own = db.vod_clip(vod).unwrap().unwrap();
    let snippet = db.add_clip(vod, 1.0, 2.0, "s", true).unwrap().id;
    assert!(!db.delete_edit(own).unwrap(), "a VOD's own clip");
    assert!(!db.delete_edit(snippet).unwrap(), "a clip of a clip bar");
    assert!(db.delete_edit(edit).unwrap());
    assert_eq!(db.edit(edit, at_first()).unwrap(), None);
    assert!(db.clip(on_bar).unwrap().is_none(), "its clip bar's clip goes with it");
    assert!(db.clip_marks(edit).unwrap().is_empty());
    assert!(db.edits(at_first()).unwrap().is_empty());
    assert_eq!((db.vod_clip(vod).unwrap(), db.clip(snippet).unwrap().map(|c| c.id)), (Some(own), Some(snippet)));
}

// A VOD's own clip.

/// A VOD is a clip of one piece over all of it, listed, not an edit (edits.rs,
/// the module's doc: "a VOD's own clip (`whole_of`: one piece over the whole
/// VOD, the document the VOD view works on)"; `Db::vod_clip`).
#[test]
fn a_vod_has_its_own_clip_over_all_of_it() {
    let db = library();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let clip = db.vod_clip(vod).unwrap().unwrap();
    let pieces = db.piece_rows(clip).unwrap();
    assert_eq!((pieces.len(), pieces[0].vod_id, pieces[0].start, pieces[0].end), (1, vod, Flicks::ZERO, None));
    assert_eq!(db.edit_pieces(clip).unwrap()[0].end, 600.0, "to the VOD's end");
    assert!(db.edits(at_first()).unwrap().is_empty(), "not an edit");
    assert!(!db.delete_edit(clip).unwrap(), "nor deleted as one");
    assert_eq!(db.vod_clip(vod).unwrap(), Some(clip));
    assert_eq!(db.library_order().unwrap(), [LibraryItem::Vod(vod)]);
    // Its frame rate, once read.
    assert_eq!(db.rate(vod).unwrap(), None);
    db.set_rate(vod, Some(Rate::new(30000, 1001))).unwrap();
    assert_eq!(db.rate(vod).unwrap(), Some(Rate::new(30000, 1001)));
    db.delete_vod(vod).unwrap();
    assert_eq!(db.vod_clip(vod).unwrap(), None, "goes with the VOD");
}

/// The clip of a VOD's stretch is the one its clip bar lists there (its
/// first piece's stretch), whatever the editor changed in it: a piece
/// added. The page finds it so (clips.svelte.ts `find`), and the app
/// gives it (decided with the clips: a stretch's clip is the one its clip
/// bar lists). A stretch dragged gives only a clip that still plays just
/// that stretch (CHANGELOG.md, 0.7.3 Fixes: "A dragged stretch is the VOD's
/// footage."; edits.rs `Db::vod_clip_playing`).
#[test]
fn a_vod_s_clip_of_a_stretch_is_the_one_listed_there_added_to_or_not() {
    let db = library();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let added = db.add_clip(vod, 30.0, 40.0, "b", true).unwrap().id;
    db.add_piece(added, vod, (50.0, 52.0), "c", None).unwrap();
    let of = |start, end| db.vod_clip_of(vod, start, end).unwrap().map(|c| c.id);
    assert_eq!(of(30.0, 40.0), Some(added), "a piece added");
    assert_eq!(of(30.0, 39.0), None, "a stretch of its own");
    assert_eq!(of(50.0, 52.0), None, "a piece after the first");
    // What plays just the stretch: not that one, but one as it came.
    let playing = |start, end| db.vod_clip_playing(vod, start, end).unwrap().map(|c| c.id);
    assert_eq!(playing(30.0, 40.0), None, "a changed clip");
    let whole = db.add_clip(vod, 30.0, 40.0, "d", true).unwrap().id;
    assert_eq!(playing(30.0, 40.0), Some(whole));
    // The page tells them apart as listed (clips.svelte.ts `find`).
    let plain: Vec<_> = db.clips(vod).unwrap().iter().map(|c| (c.id, c.plain)).collect();
    assert_eq!(plain, [(added, false), (whole, true)]);
    // A piece added and taken away is just the stretch again; a group isn't.
    db.remove_piece(db.piece_rows(added).unwrap()[1].id).unwrap();
    assert_eq!(playing(30.0, 40.0), Some(added), "added to and taken away from");
    let grouped = db.add_clip(vod, 60.0, 70.0, "e", true).unwrap().id;
    let into = db.add_clip(vod, 80.0, 90.0, "f", true).unwrap().id;
    db.add_group(into, grouped, Some(0)).unwrap();
    db.remove_piece(db.piece_rows(into).unwrap().last().unwrap().id).unwrap();
    assert_eq!(db.clip(into).unwrap().map(|c| (c.start, c.plain)), Some((60.0, false)), "in a group");
}

// Revisions.

/// Every change to a clip moves it to its next revision, one per change; a
/// clip of a VOD's clip bar moves its VOD's clip too, once it's on the bar;
/// what's made of a clip isn't a change to it (edits.rs `Db::change`: "one
/// transaction that moves the clip to its next revision", "a change to it
/// moves that clip's revision too").
#[test]
fn every_change_to_a_clip_is_a_revision() {
    let db = library();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let whole = db.vod_clip(vod).unwrap().unwrap();
    let edit = db.add_edit("e").unwrap().id;
    let rev = |id| db.revision(id).unwrap().unwrap();
    assert_eq!((rev(whole), rev(edit)), (0, 0));

    // Edits: each writer moves the edit on by one.
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let piece = db.piece_rows(edit).unwrap()[0].id;
    db.set_piece_range(piece, 10.0, 30.0).unwrap();
    db.add_clip_mark(edit, vod, "chapter", 12.0, None).unwrap();
    assert!(db.split_piece(piece, 20.0).unwrap());
    db.move_piece(piece, 1).unwrap();
    db.rename_edit(edit, "f").unwrap();
    db.remove_piece(piece).unwrap();
    let state = db.edit_state(edit).unwrap().unwrap();
    db.restore_edit(edit, &state).unwrap();
    assert_eq!(rev(edit), 8);
    assert_eq!(db.edit(edit, at_first()).unwrap().unwrap().revision, 8);

    // A VOD's own clip: its marks, and its clip bar.
    let mark = db.add_mark(vod, "cut", 5.0, None).unwrap().id;
    assert!(db.move_mark(mark, 6.0).unwrap());
    assert!(db.set_mark_text(mark, Some("t")).unwrap());
    db.delete_mark(mark).unwrap();
    db.restore_marks(vod, &[]).unwrap();
    assert_eq!(rev(whole), 5);
    let snippet = db.add_clip(vod, 1.0, 2.0, "s", true).unwrap().id;
    assert_eq!((rev(whole), rev(snippet)), (6, 0));
    // A snippet in the bar is part of its clip's document as well as its own.
    db.set_clip_range(snippet, 1.0, 3.0).unwrap();
    db.rename_clip(snippet, "t").unwrap();
    assert_eq!((rev(whole), rev(snippet)), (8, 2));
    db.delete_clip(snippet).unwrap();
    assert_eq!(rev(whole), 9);
    // One only dragged out isn't in the bar until it's kept.
    let out = db.add_clip(vod, 1.0, 2.0, "o", false).unwrap().id;
    db.rename_clip(out, "p").unwrap();
    assert_eq!(rev(whole), 9);
    db.show_clip(out).unwrap();
    assert_eq!(rev(whole), 10);

    // What's made of a clip isn't a change to it.
    db.set_edit_render(
        edit,
        ClipStatus::Ready,
        Some(&db.files().clip_file(edit, "mp4").to_string_lossy()),
        None,
        Some(8),
    )
    .unwrap();
    db.hand_out_edit(edit).unwrap();
    assert_eq!(rev(edit), 8);
    assert!(db.edit(edit, at_first()).unwrap().unwrap().current);
}

/// A change asked against a revision the clip has moved past is refused,
/// writing nothing (edits.rs `Db::change`: "Against revision `base`, if
/// given: refused (`Stale`) if the clip is at another by then"; "An error if
/// the clip's gone").
#[test]
fn a_change_against_an_old_revision_is_refused() {
    let db = library();
    let edit = db.add_edit("e").unwrap().id;
    let rename = |title: &'static str| {
        move |db: &Db| {
            db.conn.execute("UPDATE clips SET title = ?2 WHERE id = ?1", params![edit, title])?;
            Ok(())
        }
    };
    db.change(edit, Some(0), rename("a")).unwrap();
    let e = db.change(edit, Some(0), rename("b")).unwrap_err();
    assert_eq!(e.downcast_ref::<Stale>(), Some(&Stale { clip: edit, base: 0, now: 1 }));
    assert_eq!(e.to_string(), "That clip has changed since (revision 1, not 0).");
    assert_eq!(db.edit(edit, at_first()).unwrap().unwrap().title, "a", "nothing written");
    db.change(edit, Some(1), rename("c")).unwrap();
    assert!(db.change(edit + 100, None, rename("d")).is_err(), "a clip that isn't there");
}

// Groups: a clip put in an edit.

/// A clip added to an edit arrives whole, as one group named for it, where
/// it's put (CHANGELOG.md, 0.5.0: "A clip added to an edit arrives whole, as
/// one band on the timeline"; edits.rs `Db::add_group`: "titled as `from`
/// is").
#[test]
fn a_clip_dropped_in_arrives_whole_as_a_group_named_for_it() {
    let (db, vod, edit) = with_edit();
    let snippet = db.add_clip(vod, 1.0, 2.0, "Hello", true).unwrap().id;
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let group = db.add_group(edit, snippet, Some(0)).unwrap();
    assert_eq!(shape(&db, edit), [(1.0, 2.0, Some("Hello".into())), (10.0, 20.0, None)]);
    assert_eq!(db.edit_pieces(edit).unwrap()[0].group.as_ref().map(|g| g.id), Some(group));
    assert_eq!(db.uses(vod).unwrap(), 4, "its clip, the snippet, the piece, the group's piece");
}

/// A clip dropped in is the edit's own copy: changing the clip changes
/// nothing in the edit (edits.rs `Db::add_group`: "It's the group's own
/// from then on: changing `from` changes nothing here").
#[test]
fn a_clip_dropped_in_is_the_edit_s_own_copy() {
    let (db, vod, edit) = with_edit();
    let snippet = db.add_clip(vod, 1.0, 2.0, "Hello", true).unwrap().id;
    db.add_group(edit, snippet, None).unwrap();
    db.set_clip_range(snippet, 1.0, 5.0).unwrap();
    db.rename_clip(snippet, "Bye").unwrap();
    assert_eq!(shape(&db, edit), [(1.0, 2.0, Some("Hello".into()))]);
}

/// An edit added to another arrives as a group the same way, the groups in
/// it within it, all in order; a clip can't go inside itself (CHANGELOG.md,
/// 0.5.0: "An edit added to another arrives the same way"; edits.rs
/// `Db::add_group`: "A clip can't go inside itself.").
#[test]
fn an_edit_added_to_another_arrives_as_a_group_too() {
    let (db, vod, edit) = with_edit();
    let snippet = db.add_clip(vod, 1.0, 2.0, "Hello", true).unwrap().id;
    db.add_group(edit, snippet, None).unwrap();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let outer = db.add_edit("outer").unwrap().id;
    db.add_group(outer, edit, None).unwrap();
    assert_eq!(shape(&db, outer), [(1.0, 2.0, Some("e".into())), (10.0, 20.0, Some("e".into()))]);
    let groups = db.item_rows(outer).unwrap().iter().filter(|i| i.vod_id.is_none()).count();
    assert_eq!(groups, 2, "the edit's group and the clip's within it");
    assert_eq!(said(db.add_group(outer, outer, None)).as_deref(), Some("A clip can't go inside itself."));
}

/// A group opened shows its pieces one by one, to split and move among
/// themselves, staying in it (CHANGELOG.md, 0.5.0: "its pieces show one by
/// one, to move among themselves, trim, split or remove"; edits.rs
/// `Db::split_piece`: "Beside it, in its group if it's in one").
#[test]
fn a_group_s_pieces_split_and_move_among_themselves_inside_it() {
    let (db, vod, edit) = with_edit();
    let snippet = db.add_clip(vod, 1.0, 2.0, "Hello", true).unwrap().id;
    db.add_group(edit, snippet, None).unwrap();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let inner = db.edit_pieces(edit).unwrap()[0].id;
    assert!(db.split_piece(inner, 1.5).unwrap());
    let hello = Some("Hello".to_string());
    assert_eq!(shape(&db, edit), [(1.0, 1.5, hello.clone()), (1.5, 2.0, hello.clone()), (10.0, 20.0, None)]);
    let second = db.edit_pieces(edit).unwrap()[1].id;
    db.move_piece(second, 0).unwrap();
    assert_eq!(shape(&db, edit), [(1.5, 2.0, hello.clone()), (1.0, 1.5, hello), (10.0, 20.0, None)]);
}

/// A group moves and goes as one (CHANGELOG.md, 0.5.0: "as one band on the
/// timeline that moves and goes as one").
#[test]
fn a_group_moves_and_goes_as_one() {
    let (db, vod, edit) = with_edit();
    let snippet = db.add_clip(vod, 1.0, 3.0, "Hello", true).unwrap().id;
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let group = db.add_group(edit, snippet, Some(0)).unwrap();
    assert!(db.split_piece(db.edit_pieces(edit).unwrap()[0].id, 2.0).unwrap());
    db.move_piece(group, 1).unwrap();
    let hello = Some("Hello".to_string());
    assert_eq!(shape(&db, edit), [(10.0, 20.0, None), (1.0, 2.0, hello.clone()), (2.0, 3.0, hello)]);
    db.remove_piece(group).unwrap();
    assert_eq!(shape(&db, edit), [(10.0, 20.0, None)]);
}

/// A group whose pieces are all taken out goes too (edits.rs
/// `Db::remove_edit_stretch`: "a group left empty goes too").
#[test]
fn removing_all_of_a_groups_pieces_takes_the_group_too() {
    let (db, vod, edit) = with_edit();
    // A group holding a group: an edit of a clip, dropped in.
    let inner = db.add_edit("inner").unwrap().id;
    db.add_piece(inner, vod, (10.0, 14.0), "a", None).unwrap();
    let clip = db.add_edit("clip").unwrap().id;
    db.add_group(clip, inner, None).unwrap();
    db.add_group(edit, clip, None).unwrap();
    db.add_piece(edit, vod, (30.0, 35.0), "b", None).unwrap();
    // The group's split within, then all of it (0-4 s) taken out.
    db.remove_edit_stretch(edit, 1.0, 2.0).unwrap();
    assert_eq!(db.edit_pieces(edit).unwrap().len(), 3);
    db.remove_edit_stretch(edit, 0.0, 3.0).unwrap();
    assert_eq!(stretches(&db, edit), [(30.0, 35.0)]);
    let groups = db.item_rows(edit).unwrap().iter().filter(|i| i.vod_id.is_none()).count();
    assert_eq!(groups, 0, "the emptied groups go, the one in the other too");
}

// Remove: a stretch of the timeline taken out.

/// Pieces 10-19 s (0-9 s of the timeline), 100-110 s (9-19) and 200-210 s
/// (19-29) of VOD "x": the library, the VOD and the edit.
fn three_pieces() -> (Db, i64, i64) {
    let (db, vod, edit) = with_edit();
    for (start, end, title) in [(10.0, 19.0, "a"), (100.0, 110.0, "b"), (200.0, 210.0, "c")] {
        db.add_piece(edit, vod, (start, end), title, None).unwrap();
    }
    (db, vod, edit)
}

/// A stretch taken out across a join takes the end of one piece and the
/// start of the next (edits.rs `cutting_out`: "for each piece it reaches,
/// what becomes of it"; `Db::remove_edit_stretch`).
#[test]
fn removing_a_stretch_across_a_join_trims_both_pieces() {
    let (db, _, edit) = three_pieces();
    db.remove_edit_stretch(edit, 4.0, 11.0).unwrap();
    assert_eq!(stretches(&db, edit), [(10.0, 14.0), (102.0, 110.0), (200.0, 210.0)]);
}

/// A stretch taken out of a piece's middle splits it in two there, each part
/// a piece of its own (CHANGELOG.md, 0.5.0: "Delete then takes the selected
/// lines' stretch out, as any stretch taken out: the piece splits in two
/// there, each part its own band"; edits.rs `cutting_out`).
#[test]
fn removing_a_stretch_inside_a_piece_splits_it_in_two() {
    let (db, _, edit) = three_pieces();
    db.remove_edit_stretch(edit, 21.0, 24.0).unwrap();
    assert_eq!(stretches(&db, edit), [(10.0, 19.0), (100.0, 110.0), (200.0, 202.0), (205.0, 210.0)]);
    assert_eq!(titles(&db, edit), ["a", "b", "c", "c"]);
    let ids: Vec<i64> = db.edit_pieces(edit).unwrap().iter().map(|p| p.id).collect();
    assert_ne!(ids[2], ids[3], "two pieces");
    // Each its own: the second part moved first.
    db.move_piece(ids[3], 0).unwrap();
    assert_eq!(stretches(&db, edit)[0], (205.0, 210.0));
}

/// A stretch taken out over all of a piece, give or take a sliver at its
/// edges, takes that piece out and leaves its neighbours (edits.rs
/// `cutting_out`: "Within a sliver of a piece's edge counts as at it").
#[test]
fn removing_all_of_a_piece_give_or_take_a_sliver_takes_it_out() {
    let (db, _, edit) = three_pieces();
    db.remove_edit_stretch(edit, 8.98, 19.01).unwrap();
    assert_eq!(stretches(&db, edit), [(10.0, 19.0), (200.0, 210.0)]);
}

/// Delete between two cuts takes that stretch out, and those cuts become a
/// real split; a cut elsewhere stays, as sooner as what went before it
/// (CHANGELOG.md, 0.5.0: "Delete with nothing selected takes out the stretch
/// between cuts at the playhead: the piece splits there, and those cuts
/// become a real split"; edits.rs `Db::remove_edit_stretch`: "Its cuts at
/// either end, and any in it, go as well").
#[test]
fn removing_a_stretch_between_split_marks_makes_them_a_real_split() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let mark = |kind: &str, at: f64| db.add_clip_mark(edit, vod, kind, at, None).unwrap();
    let (left, right, after) = (mark("cut", 13.0), mark("cut", 16.0), mark("cut", 18.0));
    let inside = mark("cut", 14.5);
    assert_eq!(placed(&db, edit).iter().map(|m| m.1).collect::<Vec<_>>(), [3.0, 4.5, 6.0, 8.0]);

    // 3-6 s out (from mark to mark, its ends a hair off the frames).
    db.remove_edit_stretch(edit, 3.01, 5.99).unwrap();
    let ids: Vec<i64> = db.clip_marks(edit).unwrap().iter().map(|(id, _)| *id).collect();
    assert!(
        [left, right, inside].iter().all(|id| !ids.contains(id)),
        "the marks at its ends and in it are a real split now"
    );
    assert_eq!(ids, [after], "a cut past it stays");
    let now = placed(&db, edit);
    assert!(now[0].0 == after && (now[0].1 - 5.02).abs() < 1e-6, "18 s, 2.98 s sooner: {now:?}");
    // Two pieces now: 10-13.01 s and 15.99-20 s.
    assert!(same_times(&stretches(&db, edit), &[(10.0, 13.01), (15.99, 20.0)]));
}

/// A highlight in a stretch taken out stays the edit's, hidden while its
/// moment doesn't play (edits.rs `Db::remove_edit_stretch`: only "Its cuts
/// at either end, and any in it, go"; `place_marks`: "A highlight shows
/// where its moment plays").
#[test]
fn a_highlight_in_a_removed_stretch_stays_hidden_while_its_moment_is_out() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let star = db.add_clip_mark(edit, vod, "highlight", 14.0, None).unwrap();
    db.remove_edit_stretch(edit, 3.0, 6.0).unwrap();
    assert!(db.clip_marks(edit).unwrap().iter().any(|(id, _)| *id == star), "kept");
    assert_eq!(shown_at(&db, edit, star), None, "hidden");
    db.set_piece_range(db.piece_rows(edit).unwrap()[0].id, 10.0, 15.0).unwrap();
    assert_eq!(shown_at(&db, edit, star), Some(4.0), "shown again once its moment plays");
}

/// Several stretches taken out at once go each where it was when picked,
/// and the pieces named with them, as one change: all, or none if one
/// can't go (edits.rs `Db::remove_from_edit`: "from the last back, so each
/// one's times are still where they were when it was picked"; CHANGELOG.md,
/// 0.6.0: "Delete takes the picked clips out of the clip bar, and in an edit
/// the picked pieces too, in one go").
#[test]
fn several_things_taken_out_at_once_go_where_they_were_picked() {
    let (db, vod, edit) = with_edit();
    // 0-10 s (0-10 of the timeline), 20-30 s (10-20), 40-50 s (20-30).
    for start in [0.0, 20.0, 40.0] {
        db.add_piece(edit, vod, (start, start + 10.0), "a", None).unwrap();
    }
    let last = db.piece_rows(edit).unwrap()[2].id;
    db.remove_from_edit(edit, &[(2.0, 3.0), (12.0, 13.0)], &[last]).unwrap();
    assert_eq!(stretches(&db, edit), [(0.0, 2.0), (3.0, 10.0), (20.0, 22.0), (23.0, 30.0)]);
    // One that can't go: none of them do.
    let had = stretches(&db, edit);
    assert!(db.remove_from_edit(edit, &[(-1.0, 0.5), (4.0, 5.0)], &[]).is_err());
    assert_eq!(stretches(&db, edit), had, "nothing half done");
}

/// Several things taken out of an edit at once (remove_from_edit) took
/// any piece named, of any edit: one of another edit's went from it
/// with no undo there, recorded in this edit's history instead (edits.rs
/// `Db::remove_from_edit`: "A piece of another edit, or one that's gone, is
/// left alone").
#[test]
fn taking_pieces_out_of_an_edit_leaves_other_edits_alone() {
    let db = library();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let (mine, other) = (db.add_edit("mine").unwrap().id, db.add_edit("other").unwrap().id);
    for edit in [mine, other] {
        db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
        db.add_piece(edit, vod, (30.0, 40.0), "b", None).unwrap();
    }
    let ids = |edit| db.edit_pieces(edit).unwrap().iter().map(|p| p.id).collect::<Vec<_>>();
    let (a, theirs) = (ids(mine)[0], ids(other)[0]);
    db.remove_from_edit(mine, &[(18.0, 20.0)], &[a, theirs, 9999]).unwrap();
    assert_eq!(ids(other).len(), 2, "another edit's piece stays");
    assert_eq!(stretches(&db, mine), [(30.0, 38.0)]);
}

/// Nothing of an edit with nothing in it plays anywhere: taking a stretch
/// out of it, or making a clip of one, says so (edits.rs
/// `Db::check_edit_span`: "An edit with nothing in it has no end to be
/// past"; e2e store-errors.mjs: "C (a clip) that nothing of the edit plays
/// there").
#[test]
fn nothing_of_an_empty_edit_plays_anywhere() {
    let (db, _, edit) = with_edit();
    let nothing = Some("Nothing of the edit plays there.".to_string());
    assert_eq!(said(db.remove_edit_stretch(edit, 0.0, 1.0)), nothing);
    assert_eq!(said(db.add_edit_clip(edit, 0.0, 1.0, "c", true)), nothing);
    assert!(db.bar_clips(edit).unwrap().is_empty());
}

// Marks in an edit.

/// An edit of 10-20 s of VOD "x" with 12-14 s taken out (playing 0-2 s and
/// 2-8 s), then 30-35 s (8-13 s): the library, the VOD and the edit.
fn with_a_stretch_out() -> (Db, i64, i64) {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    db.add_piece(edit, vod, (30.0, 35.0), "b", None).unwrap();
    db.remove_edit_stretch(edit, 2.0, 4.0).unwrap();
    (db, vod, edit)
}

/// A highlight shows where its moment plays, and nowhere while it doesn't
/// (CHANGELOG.md, 0.5.0: "Edits have chapters and highlights of their own
/// ... Each stays on its moment of the video"; edits.rs `place_marks`: "A
/// highlight shows where its moment plays").
#[test]
fn an_edits_marks_show_where_their_moments_play() {
    let (db, vod, edit) = with_a_stretch_out();
    let mark = |kind: &str, at: f64| db.add_clip_mark(edit, vod, kind, at, None).unwrap();
    let (shown, hidden, later) = (mark("highlight", 15.0), mark("highlight", 13.0), mark("chapter", 31.0));
    let outside = mark("highlight", 50.0);
    assert_eq!(shown_at(&db, edit, shown), Some(3.0));
    assert_eq!(shown_at(&db, edit, hidden), None, "a highlight whose moment is taken out hides");
    assert_eq!(shown_at(&db, edit, later), Some(9.0));
    assert_eq!(shown_at(&db, edit, outside), None, "a moment the edit doesn't play");
}

/// A mark stays on its moment, so it moves with its piece (CHANGELOG.md,
/// 0.5.0: "Each stays on its moment of the video, so it moves with its
/// piece").
#[test]
fn a_mark_moves_with_its_piece() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    db.add_piece(edit, vod, (30.0, 35.0), "b", None).unwrap();
    let chapter = db.add_clip_mark(edit, vod, "chapter", 31.0, None).unwrap();
    let star = db.add_clip_mark(edit, vod, "highlight", 33.0, None).unwrap();
    assert_eq!((shown_at(&db, edit, chapter), shown_at(&db, edit, star)), (Some(11.0), Some(13.0)));
    db.move_piece(db.piece_rows(edit).unwrap()[1].id, 0).unwrap();
    assert_eq!((shown_at(&db, edit, chapter), shown_at(&db, edit, star)), (Some(1.0), Some(3.0)));
}

/// A chapter whose start is trimmed off its piece starts where that piece
/// now begins (CHANGELOG.md, 0.5.0: "a chapter whose start is cut away starts
/// where its piece plays again"; edits.rs `place_marks`: "the first, in the
/// edit's order, that plays after its moment").
#[test]
fn a_chapter_whose_start_is_trimmed_off_starts_where_its_piece_begins() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (0.0, 5.0), "before", None).unwrap();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let chapter = db.add_clip_mark(edit, vod, "chapter", 12.0, None).unwrap();
    assert_eq!(shown_at(&db, edit, chapter), Some(7.0));
    db.set_piece_range(db.piece_rows(edit).unwrap()[1].id, 14.0, 20.0).unwrap();
    assert_eq!(shown_at(&db, edit, chapter), Some(5.0), "where its piece begins now");
}

/// A chapter whose moment no piece plays starts where the next piece of its
/// VOD begins: taken out with Remove (in a piece's middle, or to its end),
/// between two pieces, or before them all; a highlight or cut there shows
/// nowhere (CHANGELOG.md, 0.7.4 Fixes: "Chapters in removed stretches still
/// show. A chapter whose start was taken out of an edit with Remove now
/// starts where the next piece begins."; edits.rs `place_marks`).
#[test]
fn a_chapter_in_no_piece_starts_where_the_next_piece_begins() {
    let (db, vod, edit) = with_edit();
    // 10-20 s with 18-20 taken out (plays 0-8), then 30-35 s (8-13).
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    db.add_piece(edit, vod, (30.0, 35.0), "b", None).unwrap();
    db.remove_edit_stretch(edit, 8.0, 10.0).unwrap();
    let mark = |kind: &str, at: f64| db.add_clip_mark(edit, vod, kind, at, None).unwrap();
    let cut_away = mark("chapter", 19.0);
    let between = mark("chapter", 25.0);
    let before = mark("chapter", 5.0);
    let highlight = mark("highlight", 25.0);
    let cut = mark("cut", 25.0);
    assert_eq!(shown_at(&db, edit, cut_away), Some(8.0), "taken out to its piece's end: where the next piece begins");
    assert_eq!(shown_at(&db, edit, between), Some(8.0), "between two pieces: where the next one begins");
    assert_eq!(shown_at(&db, edit, before), Some(0.0), "before every piece: where the first begins");
    assert_eq!(shown_at(&db, edit, highlight), None, "a highlight shows only where its moment plays");
    assert_eq!(shown_at(&db, edit, cut), None, "a cut shows only where its moment plays");
    // Taken out of a piece's middle: where its second part begins.
    let (db, vod, edit) = with_a_stretch_out();
    let middle = db.add_clip_mark(edit, vod, "chapter", 13.0, None).unwrap();
    assert_eq!(shown_at(&db, edit, middle), Some(2.0), "where what's left plays");
}

/// A moment played more than once shows its mark once, where it's first
/// played (edits.rs `place_marks`: "A moment played more than once shows
/// where it's first played").
#[test]
fn a_moment_played_twice_shows_its_mark_where_it_is_first_played() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    db.add_piece(edit, vod, (12.0, 18.0), "again", None).unwrap();
    db.add_clip_mark(edit, vod, "chapter", 15.0, None).unwrap();
    db.add_clip_mark(edit, vod, "highlight", 15.0, None).unwrap();
    assert_eq!(placed(&db, edit).iter().map(|m| m.1).collect::<Vec<_>>(), [5.0, 5.0]);
}

/// A chapter whose moment a later piece plays shows there, not where an
/// earlier piece that only begins after it does (edits.rs `place_marks`: "a
/// later piece that plays the moment itself wins over an earlier one that
/// only begins after it").
#[test]
fn a_later_piece_playing_a_chapter_s_moment_wins_over_one_beginning_after_it() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (30.0, 35.0), "after", None).unwrap();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let chapter = db.add_clip_mark(edit, vod, "chapter", 15.0, None).unwrap();
    assert_eq!(shown_at(&db, edit, chapter), Some(10.0));
}

/// The VOD's moment at a time of an edit is its piece's; at the edit's very
/// end, a millisecond before, where a mark still shows (edits.rs
/// `source_at`: "at its very end, a millisecond before (the end itself plays
/// nothing, and a mark there wouldn't show)").
#[test]
fn the_moment_at_an_edit_s_very_end_is_just_before_it() {
    let (db, vod, edit) = with_a_stretch_out();
    let pieces = db.edit_pieces(edit).unwrap();
    assert_eq!(source_at(&pieces, 3.0), Some((vod, 15.0)));
    assert_eq!(source_at(&pieces, 9.0), Some((vod, 31.0)));
    assert_eq!(source_at(&pieces, 13.0), Some((vod, 34.999)), "the very end: just before it");
    assert_eq!(source_at(&pieces, 14.0), None);
    let end = db.add_clip_mark(edit, vod, "highlight", 34.999, None).unwrap();
    assert!((shown_at(&db, edit, end).unwrap() - 12.999).abs() < 1e-9, "a mark there shows, at the end");
}

/// A clip, a stretch put in an edit and a clip added to one bring the
/// chapters and highlights in them, on the same moments, and no cut
/// (CHANGELOG.md, 0.5.0: "A clip, a stretch put in an edit, and a clip added
/// to an edit bring the chapters and highlights in them along, on the same
/// moments.").
#[test]
fn clips_stretches_and_groups_bring_their_marks() {
    let db = library();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    db.add_mark(vod, "chapter", 12.0, Some("in")).unwrap();
    db.add_mark(vod, "highlight", 15.0, None).unwrap();
    db.add_mark(vod, "cut", 13.0, None).unwrap();
    db.add_mark(vod, "chapter", 25.0, Some("out")).unwrap();
    let kinds = |clip| -> Vec<(String, f64)> {
        db.clip_marks(clip).unwrap().into_iter().map(|(_, m)| (m.kind, m.at.seconds())).collect()
    };
    let want = vec![("chapter".to_string(), 12.0), ("highlight".to_string(), 15.0)];
    // A clip of 10-20 s: its chapter and highlight, not the cut or what's outside.
    let clip = db.add_clip(vod, 10.0, 20.0, "c", true).unwrap().id;
    assert_eq!(kinds(clip), want);
    // A stretch put in an edit.
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (10.0, 20.0), "s", None).unwrap();
    assert_eq!(kinds(edit), want);
    // A clip put in another as a group: its marks that show in it.
    let other = db.add_edit("f").unwrap().id;
    db.add_group(other, clip, None).unwrap();
    assert_eq!(kinds(other), want);
    assert_eq!(placed(&db, other).iter().map(|m| m.1).collect::<Vec<_>>(), [2.0, 5.0]);
}

/// An imported chapter or highlight keeps what it brings (its end, kind,
/// note, who's in it, its emoji) wherever it goes into an edit, and a
/// highlight runs as long as it's in the edit (CHANGELOG.md, 0.7.0:
/// "Highlights drag into edits"; edits.rs `MarkAt::detail`: "What an
/// imported mark brings"; `placed_end`: "a highlight runs as long as it's in
/// the clip").
#[test]
fn imported_marks_keep_their_detail_in_an_edit() {
    use crate::db::MarkDetail;
    let db = library();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let own = db.vod_clip(vod).unwrap().unwrap();
    let imported = |end: Option<f64>, category: Option<&str>, icon: Option<&str>| MarkDetail {
        end,
        category: category.map(Into::into),
        note: category.map(|_| "Why it stands out".into()),
        cast: if category.is_some() { vec!["Marlowe".into(), "Ada".into()] } else { Vec::new() },
        icon: icon.map(Into::into),
        source: crate::annotations::SOURCE.into(),
        imported_as: None,
    };
    let funny = imported(Some(14.0), Some("funny"), None);
    let game = imported(None, None, Some("🎮"));
    db.insert_mark_with(own, vod, "highlight", Flicks::from_seconds(10.0), Some("Ha"), &funny).unwrap();
    db.insert_mark_with(own, vod, "chapter", Flicks::from_seconds(5.0), Some("Play"), &game).unwrap();
    // Each mark of clip `id` as its timeline shows it: kind, start, and detail.
    let shown = |id: i64| -> Vec<(String, f64, MarkDetail)> {
        place_marks(&db.edit_pieces(id).unwrap(), &db.clip_marks(id).unwrap())
            .into_iter()
            .map(|m| (m.kind, m.start, m.detail))
            .collect()
    };
    let at = |end: Option<f64>, d: &MarkDetail| MarkDetail { end, ..d.clone() };
    // 4-20 s of the VOD: the chapter at 1 s, the highlight 6-10 s.
    let whole =
        vec![("chapter".to_string(), 1.0, game.clone()), ("highlight".to_string(), 6.0, at(Some(10.0), &funny))];

    // A stretch put on a timeline.
    let edit = db.add_edit("e").unwrap().id;
    db.put(edit, &[Put::Range { vod_id: vod, start: 4.0, end: 20.0, title: None }], Onto::Timeline(None)).unwrap();
    assert_eq!(shown(edit), whole, "put on a timeline");
    // Undo of a change brings them back as they were.
    let state = db.edit_state(edit).unwrap().unwrap();
    db.restore_edit(edit, &state).unwrap();
    assert_eq!(shown(edit), whole, "after undo");
    // A whole VOD put in: each on its own moment.
    let all = db.add_edit("v").unwrap().id;
    db.put(all, &[Put::Vod { id: vod }], Onto::Timeline(None)).unwrap();
    let on_vod = vec![("chapter".to_string(), 5.0, game.clone()), ("highlight".to_string(), 10.0, funny.clone())];
    assert_eq!(shown(all), on_vod, "a whole VOD put");
    // The highlight dragged onto an edit, or Make new edit (or A): it, one band.
    let made = db.add_edit("h").unwrap().id;
    let put = Put::Highlight { vod_id: vod, start: 10.0, end: 14.0, title: "Ha".into() };
    db.put(made, &[put], Onto::Timeline(None)).unwrap();
    assert_eq!(shown(made), [("highlight".to_string(), 0.0, at(Some(4.0), &funny))], "a highlight put");
    // A clip of the VOD's clip bar put in an edit, as a group.
    let clip = db.add_clip(vod, 4.0, 20.0, "c", true).unwrap().id;
    assert_eq!(shown(clip), whole, "a clip of the VOD");
    let grouped = db.add_edit("g").unwrap().id;
    db.put(grouped, &[Put::Clip { id: clip }], Onto::Timeline(None)).unwrap();
    assert_eq!(shown(grouped), whole, "a clip put as a group");
    // A stretch of the edit made a clip on its bar, and a range put on the bar.
    let bar = db.add_edit_clip(edit, 0.0, 16.0, "b", true).unwrap().id;
    assert_eq!(shown(bar), whole, "a stretch of the edit made a clip");
    let on_bar = db.put(edit, &[Put::Range { vod_id: vod, start: 4.0, end: 20.0, title: None }], Onto::Bar).unwrap();
    assert_eq!(shown(on_bar[0]), whole, "put on the clip bar");

    // A highlight whose end is cut off ends where its stretch does, as
    // it does where a stretch of it is taken out: at its piece's end.
    let short = db.add_edit("s").unwrap().id;
    db.add_piece(short, vod, (8.0, 12.0), "a", None).unwrap();
    assert_eq!(shown(short), [("highlight".to_string(), 2.0, at(Some(4.0), &funny))], "ends at the piece's end");
    let first = db.piece_rows(short).unwrap()[0].id;
    db.set_piece_range(first, 8.0, 20.0).unwrap();
    db.remove_edit_stretch(short, 3.0, 4.0).unwrap();
    assert_eq!(shown(short), [("highlight".to_string(), 2.0, at(Some(3.0), &funny))], "a stretch of it taken out");
}

// The edit's clip bar.

/// C on a stretch of an edit makes a clip on its clip bar of what plays
/// there, piece by piece, with the edit's chapters and highlights in it, as
/// a change to the edit (CHANGELOG.md, 0.5.0: "An edit has a clip bar of its
/// own: C makes a clip of the lines selected, or of the stretch at the
/// playhead ... with the edit's chapters and highlights in it"; edits.rs
/// `Db::add_edit_clip`).
#[test]
fn a_stretch_of_an_edit_makes_a_clip_of_what_plays_there() {
    let (db, vod, edit) = with_a_stretch_out();
    let other = db.add_vod(&info("y")).unwrap().0.id;
    // The second piece of x (30-35 s) swapped for 30-35 s of y.
    db.remove_piece(db.piece_rows(edit).unwrap()[2].id).unwrap();
    db.add_piece(edit, other, (30.0, 35.0), "b", None).unwrap();
    db.add_clip_mark(edit, other, "chapter", 31.0, Some("inside")).unwrap();
    db.add_clip_mark(edit, other, "chapter", 34.0, Some("outside")).unwrap();
    let pieces = db.edit_pieces(edit).unwrap();
    assert_eq!(
        stretch_pieces(&pieces, 1.0, 10.0),
        [
            StretchPiece { vod_id: vod, start: 11.0, end: 12.0, title: "a".into() },
            StretchPiece { vod_id: vod, start: 14.0, end: 20.0, title: "a".into() },
            StretchPiece { vod_id: other, start: 30.0, end: 32.0, title: "b".into() },
        ]
    );
    let before = db.revision(edit).unwrap().unwrap();
    let clip = db.add_edit_clip(edit, 1.0, 10.0, "c", true).unwrap();
    assert!(db.revision(edit).unwrap().unwrap() > before, "a change to the edit: its clip bar");
    assert_eq!(
        (clip.bar, clip.vod_id, clip.start, clip.end),
        (Some(crate::db::ClipBar::Edit { id: edit }), vod, 11.0, 12.0)
    );
    assert_eq!(db.bar_clips(edit).unwrap().iter().map(|c| c.id).collect::<Vec<_>>(), [clip.id]);
    assert_eq!(db.edit_pieces(clip.id).unwrap().len(), 3);
    assert_eq!(db.clip(clip.id).unwrap().unwrap().length, 9.0, "7 s of x, 2 of y");
    let marks: Vec<_> = db.clip_marks(clip.id).unwrap().into_iter().map(|(_, m)| m.text).collect();
    assert_eq!(marks, [Some("inside".to_string())], "the edit's marks in the stretch come with it");
    assert!(db.add_edit_clip(edit, 20.0, 30.0, "d", true).is_err(), "past the end: nothing plays");
    // Made again as a state holds it (undo bringing it back).
    let state = db.edit_state(clip.id).unwrap().unwrap();
    let again = db.add_bar_clip(edit, &state).unwrap();
    assert_eq!(db.edit_state(again).unwrap().unwrap(), state);
}

/// C twice on the same stretch of an edit makes one clip, and the second
/// press changes nothing; put on the bar again, that stretch is left as it
/// is (CHANGELOG.md, 0.7.1 Fixes: "C twice in an edit makes one clip. Each
/// press on the same stretch made another clip, where on a VOD it makes
/// one"; edits.rs `Db::add_edit_clip`: "A clip there already of exactly
/// that stretch is given instead (C pressed twice)").
#[test]
fn c_twice_on_an_edit_s_stretch_makes_one_clip() {
    let (db, _, edit) = with_a_stretch_out();
    let clip = db.add_edit_clip(edit, 1.0, 10.0, "c", true).unwrap();
    let revision = db.revision(edit).unwrap().unwrap();
    assert_eq!(db.add_edit_clip(edit, 1.0, 10.0, "c again", true).unwrap().id, clip.id);
    assert_eq!(db.bar_clips(edit).unwrap().len(), 1);
    assert_eq!(db.revision(edit).unwrap().unwrap(), revision, "nothing changed");
    assert!(db.put(edit, &[Put::Span { clip: edit, from: 1.0, to: 10.0 }], Onto::Bar).unwrap().is_empty());
    assert_eq!(db.bar_clips(edit).unwrap().len(), 1);
}

/// A clip of an edit's stretch only dragged out is off the bar; clipped
/// after, it's that one, put on the bar (edits.rs `Db::add_edit_clip`:
/// "(only dragged out, unless `shown`)", "put on the bar if `shown` and it
/// was only dragged out").
#[test]
fn a_clip_only_dragged_out_and_then_clipped_is_put_on_the_bar() {
    let (db, _, edit) = with_a_stretch_out();
    let out = db.add_edit_clip(edit, 0.0, 1.0, "out", false).unwrap();
    assert!(!out.shown);
    let kept = db.add_edit_clip(edit, 0.0, 1.0, "out", true).unwrap();
    assert_eq!((kept.id, kept.shown), (out.id, true));
}

/// A clip of a VOD's or an edit's clip bar is opened as a clip, not listed
/// as an edit (edits.rs, the module's doc: "A *snippet* has none, and hangs
/// off a listed clip's clip bar"; `Edit::snippet_of`).
#[test]
fn a_clip_on_any_clip_bar_is_a_snippet_not_an_edit() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (0.0, 10.0), "a", None).unwrap();
    let of_vod = db.add_clip(vod, 1.0, 2.0, "v", true).unwrap().id;
    let of_edit = db.add_edit_clip(edit, 1.0, 2.0, "e", true).unwrap().id;
    let snippet_of = |id| db.edit(id, at_first()).unwrap().unwrap().snippet_of;
    assert_eq!(snippet_of(of_vod), Some(crate::db::ClipBar::Vod { id: vod }));
    // Opened in the editor, the page lists what isn't a snippet as an edit.
    assert_eq!(snippet_of(of_edit), Some(crate::db::ClipBar::Edit { id: edit }), "an edit's bar");
    assert_eq!(snippet_of(edit), None, "a listed edit");
    assert_eq!(db.edits(at_first()).unwrap().iter().map(|e| e.id).collect::<Vec<_>>(), [edit]);
}

/// A clip whose timeline starts with a group is read by its first piece
/// (`db::CLIP_JOIN`), with no paste either: an edit's row dropped at the
/// start of a VOD's clip open in the editor, and an edit that starts with
/// a clip dropped in, put on another edit's clip bar. Their bars still list
/// (CHANGELOG.md, 0.7.1 Fixes: "Clips can start with another clip.").
#[test]
fn a_clip_whose_timeline_starts_with_a_group_still_lists() {
    let db = library();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let pill = db.add_clip(vod, 1.0, 2.0, "Hello", true).unwrap().id;
    let a = db.add_edit("A").unwrap().id;
    db.add_group(a, pill, None).unwrap();
    // Edit A's row dropped at the start of another clip of the VOD's bar, open in the editor.
    let clip = db.add_clip(vod, 7.0, 8.0, "Other", true).unwrap().id;
    db.put(clip, &[Put::Clip { id: a }], Onto::Timeline(Some(0))).unwrap();
    let listed = db.clips(vod).map(|c| c.iter().map(|c| (c.start, c.end)).collect::<Vec<_>>());
    assert_eq!(listed.map_err(|e| e.to_string()), Ok(vec![(1.0, 2.0), (1.0, 2.0)]), "by its first piece");
    // Edit A put on edit B's clip bar.
    let b = db.add_edit("B").unwrap().id;
    let made = db.put(b, &[Put::Clip { id: a }], Onto::Bar).map(|m| m.len());
    assert_eq!(made.map_err(|e| e.to_string()), Ok(1));
    assert_eq!(db.bar_clips(b).map(|c| c.len()).map_err(|e| e.to_string()), Ok(1));
    let waiting = db.unfinished_clips().map(|c| c.len()).map_err(|e| e.to_string());
    assert_eq!(waiting, Ok(3), "the two clips of the VOD's bar and the one put wait to be made");
}

/// A clip of a clip bar that is one piece, not a group, is cut straight from
/// its VOD; split, or holding a group, it's rendered as an edit is (edits.rs
/// `cut_whole`: "one piece, not a group ... rather than rendered as an edit
/// is"; CHANGELOG.md, 0.5.0: "Double-click a clip's name in the clip bar to
/// open it in the editor: trim it, split it ... and its file is made again to
/// match").
#[test]
fn a_clip_bar_s_clip_of_one_piece_is_cut_straight_from_its_vod() {
    let db = library();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let clip = db.add_clip(vod, 10.0, 20.0, "c", true).unwrap().id;
    let straight = |id| cut_whole(&db.edit_pieces(id).unwrap());
    assert!(straight(clip));
    db.set_clip_range(clip, 12.0, 20.0).unwrap();
    assert!(straight(clip), "trimmed");
    assert!(db.split_piece(db.piece_rows(clip).unwrap()[0].id, 15.0).unwrap());
    assert!(!straight(clip), "split in two");
    let grouped = db.add_clip(vod, 30.0, 40.0, "g", true).unwrap().id;
    db.add_group(grouped, db.add_clip(vod, 1.0, 2.0, "in", true).unwrap().id, Some(0)).unwrap();
    db.remove_piece(db.piece_rows(grouped).unwrap()[0].id).unwrap();
    assert_eq!(db.edit_pieces(grouped).unwrap().len(), 1);
    assert!(!straight(grouped), "one piece, in a group");
}

// Putting things in an edit.

/// Several things picked go into an edit in their order, as one change: on
/// its timeline where they're dropped, or on its clip bar each a clip of its
/// own, one already there left as it is (CHANGELOG.md, 0.6.0: "Drag what you
/// picked into an edit ... it all goes in, in the order you picked it, as one
/// step a single undo takes back"; edits.rs `Db::put`).
#[test]
fn several_things_put_in_an_edit_go_in_their_order_as_one_change() {
    let db = library();
    let x = db.add_vod(&info("x")).unwrap().0.id;
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, x, (0.0, 10.0), "first", None).unwrap();
    db.add_piece(edit, x, (20.0, 30.0), "last", None).unwrap();
    let range = |start, end, title: &str| Put::Range { vod_id: x, start, end, title: Some(title.into()) };
    let shape = |id| {
        db.edit_pieces(id).unwrap().iter().map(|p| (p.vod_id, p.start, p.end, p.title.clone())).collect::<Vec<_>>()
    };

    // Onto the timeline between its two pieces: in the order given.
    let before = db.revision(edit).unwrap().unwrap();
    let made = db.put(edit, &[range(40.0, 41.0, "b"), range(50.0, 52.0, "a")], Onto::Timeline(Some(1))).unwrap();
    assert!(made.is_empty(), "no clips on the timeline");
    assert!(db.revision(edit).unwrap().unwrap() > before, "a change to the edit");
    assert_eq!(
        shape(edit),
        [
            (x, 0.0, 10.0, "first".into()),
            (x, 40.0, 41.0, "b".into()),
            (x, 50.0, 52.0, "a".into()),
            (x, 20.0, 30.0, "last".into())
        ]
    );

    // A stretch of the edit's own timeline (what plays from 9 to 11 s: the
    // ends of two pieces), at the end.
    db.put(edit, &[Put::Span { clip: edit, from: 9.0, to: 11.0 }], Onto::Timeline(None)).unwrap();
    assert_eq!(&shape(edit)[4..], [(x, 9.0, 10.0, "first".into()), (x, 40.0, 41.0, "b".into())]);

    // Onto the clip bar: a clip of each, in order; one of its own stays as it is.
    let own = db.add_edit_clip(edit, 0.0, 5.0, "own", true).unwrap().id;
    let before = db.revision(edit).unwrap().unwrap();
    let made = db
        .put(
            edit,
            &[range(1.0, 2.0, "one"), Put::Clip { id: own }, Put::Span { clip: edit, from: 0.0, to: 2.0 }],
            Onto::Bar,
        )
        .unwrap();
    assert!(db.revision(edit).unwrap().unwrap() > before, "a change to the edit");
    assert_eq!(made.len(), 2);
    let bar: Vec<(i64, f64, f64)> = db.bar_clips(edit).unwrap().iter().map(|c| (c.id, c.start, c.end)).collect();
    assert_eq!(bar, [(own, 0.0, 5.0), (made[0], 1.0, 2.0), (made[1], 0.0, 2.0)]);
    assert_eq!(db.clip(made[0]).unwrap().unwrap().title, "one");

    // A clip of another edit's clip bar: on the timeline a group, on the bar a copy.
    let other = db.add_edit("o").unwrap().id;
    db.put(other, &[Put::Clip { id: own }], Onto::Timeline(None)).unwrap();
    assert_eq!(db.edit_pieces(other).unwrap()[0].group.as_ref().map(|g| g.title.as_str()), Some("own"));
    let copy = db.put(other, &[Put::Clip { id: own }], Onto::Bar).unwrap();
    assert_eq!(db.edit_state(copy[0]).unwrap().unwrap().items, db.edit_state(own).unwrap().unwrap().items);
}

/// A VOD picked in the library goes into an edit whole, as one piece named
/// for it (CHANGELOG.md, 0.6.0: "a VOD picked in the library goes in whole,
/// as one piece"; edits.rs `Put::Vod`: "All of VOD `id`, as one piece, named
/// for it").
#[test]
fn a_vod_put_in_an_edit_goes_in_whole_as_one_piece() {
    let (db, x, edit) = with_edit();
    db.put(edit, &[Put::Vod { id: x }], Onto::Timeline(None)).unwrap();
    let pieces = db.edit_pieces(edit).unwrap();
    assert_eq!(
        pieces.iter().map(|p| (p.vod_id, p.start, p.end, p.title.as_str())).collect::<Vec<_>>(),
        [(x, 0.0, 600.0, "VOD x")]
    );
}

/// A highlight put in an edit goes in as one band named for it, holding its
/// stretch and the chapter in it; on the clip bar it's a clip of it, named
/// for it (CHANGELOG.md, 0.7.0: "Highlights drag into edits. Drag a highlight
/// onto an edit in the library, or onto Make new edit, and it goes in as one
/// piece named for it"; edits.rs `Put::Highlight`).
#[test]
fn a_highlight_put_in_an_edit_is_one_piece_named_for_it() {
    let (db, x, lit) = with_edit();
    db.add_mark(x, "chapter", 45.0, Some("inside")).unwrap();
    let highlight = Put::Highlight { vod_id: x, start: 44.0, end: 48.0, title: "Close call".into() };
    let range = Put::Range { vod_id: x, start: 0.0, end: 1.0, title: Some("before".into()) };
    db.put(lit, &[range, highlight.clone()], Onto::Timeline(None)).unwrap();
    db.put(lit, std::slice::from_ref(&highlight), Onto::Timeline(Some(0))).unwrap();
    let got: Vec<_> = db
        .edit_pieces(lit)
        .unwrap()
        .iter()
        .map(|p| (p.start, p.end, p.title.clone(), p.group.as_ref().map(|g| g.title.clone())))
        .collect();
    assert_eq!(
        got,
        [
            (44.0, 48.0, "Close call".into(), Some("Close call".into())),
            (0.0, 1.0, "before".into(), None),
            (44.0, 48.0, "Close call".into(), Some("Close call".into())),
        ]
    );
    let chapters = db.clip_marks(lit).unwrap().into_iter().filter(|(_, m)| m.kind == "chapter").count();
    assert_eq!(chapters, 2, "each group brings the chapter in its stretch");
    let made = db.put(lit, &[highlight], Onto::Bar).unwrap();
    assert_eq!(db.clip(made[0]).unwrap().unwrap().title, "Close call");
}

/// When one of several things put in an edit can't go, none of them go
/// (edits.rs `Db::put`: "as one change (all of them, or none if one can't
/// go)").
#[test]
fn when_one_thing_put_can_t_go_none_does() {
    let (db, x, edit) = with_edit();
    db.add_piece(edit, x, (0.0, 10.0), "first", None).unwrap();
    let range = Put::Range { vod_id: x, start: 1.0, end: 2.0, title: Some("c".into()) };
    for onto in [Onto::Timeline(None), Onto::Bar] {
        let r = db.put(edit, &[range.clone(), Put::Vod { id: 999 }], onto);
        assert_eq!(said(r).as_deref(), Some("That VOD is gone."), "{onto:?}");
    }
    assert_eq!(stretches(&db, edit), [(0.0, 10.0)], "nothing half done");
    assert!(db.bar_clips(edit).unwrap().is_empty());
}

/// A stretch put in an edit with no name of its own is named for what's said
/// at its start, or for where it starts when nothing is (edits.rs
/// `Db::put_parts`: "its name: a stretch's own or for what's said at its
/// start"; clips.rs `title`).
#[test]
fn a_stretch_put_with_no_name_is_named_for_what_is_said_at_its_start() {
    let (mut db, x, edit) = with_edit();
    crate::testkit::library::says(&mut db, x, crate::testkit::library::said(40.0, "The Eagle has landed"));
    let unnamed = |start, end| Put::Range { vod_id: x, start, end, title: None };
    db.put(edit, &[unnamed(40.0, 45.0), unnamed(100.0, 105.0)], Onto::Timeline(None)).unwrap();
    assert_eq!(titles(&db, edit), ["The Eagle has landed", "Clip at 1:40"]);
}

// Undo: what an edit holds, put back.

/// Undo puts an edit back as it was: its title, its pieces and their order
/// (edits.rs `Db::restore_edit`: "Puts clip `id` back as `state` had it: its
/// title, and its items in order"; `Db::edit_state`: "None if it's gone").
#[test]
fn an_edit_is_put_back_as_it_was() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    db.add_piece(edit, vod, (30.0, 34.0), "b", None).unwrap();
    let a = db.edit_pieces(edit).unwrap()[0].id;
    let before = db.edit_state(edit).unwrap().unwrap();

    db.remove_piece(a).unwrap();
    db.rename_edit(edit, "renamed").unwrap();
    db.add_piece(edit, vod, (1.0, 2.0), "c", Some(0)).unwrap();
    assert_ne!(db.edit_state(edit).unwrap().unwrap(), before);

    db.restore_edit(edit, &before).unwrap();
    assert_eq!(db.edit_state(edit).unwrap().unwrap(), before, "title, pieces and order");
    assert_eq!(db.edit_state(9999).unwrap(), None);
}

/// Undo puts an edit's groups back with what's in them (edits.rs
/// `EditState::items`: "Its items in the clip's order, each group before
/// what's in it").
#[test]
fn undo_puts_groups_back_as_they_were() {
    let (db, vod, edit) = with_edit();
    let snippet = db.add_clip(vod, 1.0, 2.0, "Hello", true).unwrap().id;
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    db.add_group(edit, snippet, Some(0)).unwrap();
    assert!(db.split_piece(db.edit_pieces(edit).unwrap()[0].id, 1.5).unwrap());
    let state = db.edit_state(edit).unwrap().unwrap();
    let group = db.edit_pieces(edit).unwrap()[0].group.clone().unwrap().id;
    db.move_piece(group, 1).unwrap();
    db.remove_piece(group).unwrap();
    db.restore_edit(edit, &state).unwrap();
    assert_eq!(db.edit_state(edit).unwrap().unwrap(), state, "groups and all put back");
    let hello = Some("Hello".to_string());
    assert_eq!(shape(&db, edit), [(1.0, 1.5, hello.clone()), (1.5, 2.0, hello), (10.0, 20.0, None)]);
}

/// Undo puts an edit's marks back as they were (edits.rs `EditState::marks`:
/// "Its own marks (chapters, highlights), each on a VOD's moment").
#[test]
fn undo_puts_an_edit_s_marks_back_as_they_were() {
    let (db, vod, edit) = with_a_stretch_out();
    let shown = db.add_clip_mark(edit, vod, "highlight", 15.0, None).unwrap();
    let later = db.add_clip_mark(edit, vod, "chapter", 31.0, Some("Later")).unwrap();
    let state = db.edit_state(edit).unwrap().unwrap();
    assert!(db.move_mark_to(shown, vod, 16.0).unwrap());
    db.delete_mark(later).unwrap();
    db.restore_edit(edit, &state).unwrap();
    let back: Vec<_> =
        db.clip_marks(edit).unwrap().into_iter().map(|(_, m)| (m.kind, m.at.seconds(), m.text)).collect();
    assert_eq!(back, [("highlight".into(), 15.0, None), ("chapter".into(), 31.0, Some("Later".into()))]);
}

/// Undo keeps where a piece realized as footage of its own came from
/// (edits.rs `ItemState::origin`; `PieceRow::origin`: "A realized piece: the
/// VOD's time its file starts at").
#[test]
fn undo_keeps_where_a_realized_piece_came_from() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let piece = db.piece_rows(edit).unwrap()[0].id;
    let new = db.add_realized(vod, 10.0, 20.0).unwrap();
    assert!(db.realize_piece(piece, new, db.revision(edit).unwrap().unwrap()).unwrap());
    let state = db.edit_state(edit).unwrap().unwrap();
    db.rename_edit(edit, "f").unwrap();
    db.restore_edit(edit, &state).unwrap();
    assert_eq!(db.piece_rows(edit).unwrap()[0].origin, Some(Flicks::from_seconds(10.0)));
}

/// Undo puts an edit back less the pieces of a VOD deleted since (edits.rs
/// `Db::restore_edit`: "less pieces of a VOD since deleted").
#[test]
fn undo_leaves_out_pieces_of_a_vod_deleted_since() {
    let (db, x, edit) = with_edit();
    let y = db.add_vod(&info("y")).unwrap().0.id;
    db.add_piece(edit, x, (10.0, 20.0), "a", None).unwrap();
    db.add_piece(edit, y, (30.0, 35.0), "b", None).unwrap();
    db.add_clip_mark(edit, y, "chapter", 31.0, None).unwrap();
    let state = db.edit_state(edit).unwrap().unwrap();
    db.rename_edit(edit, "f").unwrap();
    db.delete_vod(y).unwrap();
    db.restore_edit(edit, &state).unwrap();
    let pieces = db.edit_pieces(edit).unwrap();
    assert_eq!(pieces.iter().map(|p| (p.vod_id, p.start, p.end)).collect::<Vec<_>>(), [(x, 10.0, 20.0)]);
    assert_eq!(db.edit(edit, at_first()).unwrap().unwrap().title, "e");
}

// The VODs edits use.

/// A VOD is a source while any piece of any clip uses it, its own clip's
/// included; out of the list, it's a source only until nothing does; added
/// back, it's the same VOD listed again (edits.rs `Db::uses`: "How many
/// pieces of clips use VOD `vod_id`"; `Db::remove_entry`: "The VOD stays a
/// source while other clips use it"; `Db::unused_sources`; CHANGELOG.md,
/// 0.5.0: "Removing a VOD that an edit uses leaves the edit's pieces of it
/// where they are").
#[test]
fn sources_count_the_pieces_that_use_them() {
    let db = library();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    db.request(vod, 0.0, None).unwrap();
    let other = db.add_vod(&info("y")).unwrap().0.id;
    assert_eq!(db.uses(vod).unwrap(), 1, "its own clip");
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let piece = db.piece_rows(edit).unwrap()[0].id;
    assert!(db.split_piece(piece, 15.0).unwrap());
    db.add_clip(vod, 1.0, 2.0, "s", true).unwrap();
    assert_eq!(db.uses(vod).unwrap(), 4);
    assert_eq!(db.piece_ranges().unwrap(), [(vod, 10.0, 15.0), (vod, 15.0, 20.0)], "edits' footage, not snippets'");
    db.remove_piece(piece).unwrap();
    assert_eq!(db.uses(vod).unwrap(), 3);
    let state = db.edit_state(edit).unwrap().unwrap();
    db.restore_edit(edit, &state).unwrap();
    assert_eq!(db.uses(vod).unwrap(), 3, "put back as it was");

    // Out of the list: its own clip and snippet go; the edit still uses it.
    assert!(!db.remove_entry(vod).unwrap());
    assert_eq!(db.uses(vod).unwrap(), 1);
    assert_eq!(stretches(&db, edit), [(15.0, 20.0)], "the edit's piece stays where it is");
    assert!(db.unused_sources().unwrap().is_empty());
    db.delete_edit(edit).unwrap();
    assert_eq!(db.unused_sources().unwrap(), [vod]);
    assert!(db.remove_entry(other).unwrap(), "used by nothing else");
    assert_eq!(db.unused_sources().unwrap(), [vod, other]);
    // Out of the list, it's a source only: not listed, its work held.
    assert_eq!(db.listed_vods().unwrap().len(), 0);
    assert_eq!(db.vods().unwrap().len(), 2);
    assert!(!db.vod(vod).unwrap().unwrap().listed);
    assert!(db.next_stretch(crate::db::Status::Queued, false).unwrap().is_none());
    // Added again, it's listed again, the same VOD.
    let (again, new) = db.add_vod(&info("x")).unwrap();
    assert_eq!((again.id, new, again.listed), (vod, false, true));
    assert_eq!(db.uses(vod).unwrap(), 1);
    assert_eq!(db.next_stretch(crate::db::Status::Queued, false).unwrap().map(|(v, _)| v.id), Some(vod));
}

/// As a VOD removed from the library is erased at once, its row kept
/// for the session as a gone source (desktop vods.rs, `erase_unused`):
/// an undo in an edit that puts back a piece of it still finds the row
/// (a hole), and adding it back adds it anew, not the empty row (edits.rs
/// `Db::unused_sources`: "their rows kept as gone sources till the app next
/// starts (so what's undone meanwhile still finds them, holes)").
#[test]
fn a_vod_erased_after_its_removal_is_added_anew_and_its_pieces_put_back_are_holes() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let had = db.edit_state(edit).unwrap().unwrap();
    db.remove_piece(db.piece_rows(edit).unwrap()[0].id).unwrap();
    assert!(db.remove_entry(vod).unwrap(), "used by nothing now");
    db.set_gone(vod).unwrap();
    assert_eq!(db.erase_left(vod).unwrap(), 0);
    db.tidy_indexes().unwrap();
    // Undone in the edit: the piece is back, of a gone source.
    db.restore_edit(edit, &had).unwrap();
    assert_eq!(db.uses(vod).unwrap(), 1);
    assert!(db.is_gone(vod).unwrap());
    // Taken out again; then the VOD's link added back: a new VOD, listed,
    // not gone; the old row has gone with what was left of it, and the
    // piece the edit had of it with it.
    db.remove_piece(db.piece_rows(edit).unwrap()[0].id).unwrap();
    let (again, new) = db.add_vod(&info("x")).unwrap();
    assert!(new && again.id != vod && again.listed);
    assert!(!db.is_gone(again.id).unwrap());
    assert!(db.vod(vod).unwrap().is_none());
    // One still used is listed again when added back, not gone: its
    // pieces play again, and it's new to the library (`clear_gone`).
    let used = db.add_vod(&info("y")).unwrap().0.id;
    db.add_piece(edit, used, (1.0, 2.0), "b", None).unwrap();
    assert!(!db.remove_entry(used).unwrap());
    db.set_gone(used).unwrap();
    let (back, new) = db.add_vod(&info("y")).unwrap();
    assert_eq!((back.id, new, back.listed), (used, true, true));
    assert!(!db.is_gone(used).unwrap());
}

// Spans.

/// Each command that takes a span checked it its own way: some let one
/// start before 0, one capped a VOD's clips at 3 hours, and they said so
/// in different words. Every one now goes by `clips::check_span`.
#[test]
fn every_span_is_checked_the_same_way() {
    use crate::clips::{ENDS_BEFORE_START, PAST_THE_END, check_span};
    let refused = Some(ENDS_BEFORE_START.to_string());
    let past = Some(PAST_THE_END.to_string());

    assert_eq!(check_span(1.0, 2.0, Some(10.0)), Ok(()));
    assert_eq!(check_span(0.0, 4.0 * 3600.0, None), Ok(()), "any length");
    assert_eq!(check_span(2.0, 2.0, None), Err(ENDS_BEFORE_START));
    assert_eq!(check_span(-1.0, 2.0, None), Err(ENDS_BEFORE_START));
    assert_eq!(check_span(f64::NAN, 2.0, None), Err(ENDS_BEFORE_START));
    assert_eq!(check_span(1.0, f64::INFINITY, None), Err(ENDS_BEFORE_START));
    assert_eq!(check_span(10.0, 12.0, Some(10.0)), Err(PAST_THE_END));

    let db = library();
    let long = db.add_vod(&VodInfo { duration: Some(5.0 * 3600.0), ..info("long") }).unwrap().0.id;
    let vod = db.add_vod(&info("x")).unwrap().0.id;

    // A VOD's clip (`add_clip`, `set_clip_range`): longer than 3 hours too.
    let clip = db.add_clip(long, 0.0, 4.0 * 3600.0, "long", true).unwrap();
    assert_eq!(clip.end, 4.0 * 3600.0);
    assert_eq!(said(db.add_clip(vod, -1.0, 2.0, "c", true)), refused, "a clip from before 0");
    assert_eq!(said(db.add_clip(vod, 600.0, 610.0, "c", true)), past);
    assert_eq!(db.add_clip(vod, 590.0, 700.0, "c", true).unwrap().end, 600.0, "cut at the VOD's end");
    db.set_clip_range(clip.id, 60.0, 4.5 * 3600.0).unwrap();
    assert_eq!(said(db.set_clip_range(clip.id, -1.0, 2.0)), refused, "moved to before 0");
    assert_eq!(said(db.set_clip_range(clip.id, 3.0, 2.0)), refused);
    assert_eq!(said(db.set_clip_range(clip.id, 6.0 * 3600.0, 7.0 * 3600.0)), past);

    // An edit's pieces (`add_piece`, `put`, `set_piece_range`).
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, long, (0.0, 4.0 * 3600.0), "long", None).unwrap();
    assert_eq!(said(db.add_piece(edit, vod, (-1.0, 2.0), "a", None)), refused, "a piece from before 0");
    assert_eq!(said(db.add_piece(edit, vod, (f64::NAN, 2.0), "a", None)), refused);
    let range = |start, end| Put::Range { vod_id: vod, start, end, title: None };
    let highlight = |start, end| Put::Highlight { vod_id: vod, start, end, title: "h".into() };
    for onto in [Onto::Timeline(None), Onto::Bar] {
        assert_eq!(said(db.put(edit, &[range(-1.0, 2.0)], onto)), refused, "{onto:?}");
        assert_eq!(said(db.put(edit, &[highlight(-1.0, 2.0)], onto)), refused, "{onto:?}");
        assert_eq!(said(db.put(edit, &[range(700.0, 710.0)], onto)), past, "{onto:?}");
        let span = Put::Span { clip: edit, from: -1.0, to: 2.0 };
        assert_eq!(said(db.put(edit, &[span], onto)), refused, "{onto:?}");
    }
    let piece = db.piece_rows(edit).unwrap()[0].id;
    assert_eq!(said(db.set_piece_range(piece, -1.0, 2.0)), refused, "trimmed to before 0");
    assert_eq!(said(db.set_piece_range(piece, 5.0, 4.0)), refused);
    db.set_piece_range(piece, 0.0, 4.5 * 3600.0).unwrap();

    // Spans of the edit's timeline (4.5 hours long now): a clip of one
    // (`add_edit_clip`, its title `span_title`), taking one out
    // (`remove_edit_stretch`, `remove_from_edit`).
    let bar = db.add_edit_clip(edit, 0.0, 4.0 * 3600.0, "long", true).unwrap();
    assert_eq!(db.clip(bar.id).unwrap().unwrap().length, 4.0 * 3600.0, "longer than 3 hours");
    assert_eq!(said(db.span_title(edit, -1.0, 2.0)), refused);
    assert_eq!(said(db.add_edit_clip(edit, -1.0, 2.0, "c", true)), refused, "an edit's clip from before 0");
    assert_eq!(said(db.add_edit_clip(edit, 5.0 * 3600.0, 5.5 * 3600.0, "c", true)), past);
    assert_eq!(said(db.remove_edit_stretch(edit, -1.0, 2.0)), refused, "taken out from before 0");
    assert_eq!(said(db.remove_edit_stretch(edit, 2.0, f64::NAN)), refused);
    assert_eq!(said(db.remove_from_edit(edit, &[(-1.0, 2.0)], &[])), refused);
    assert_eq!(said(db.remove_from_edit(edit, &[(5.0 * 3600.0, 5.5 * 3600.0)], &[])), past);
    assert_eq!(db.edit_pieces(edit).unwrap()[0].end, 4.5 * 3600.0, "nothing taken out");
}

// What an edit says, and its chat.

/// A line split between two pieces shows under each only the words said
/// there, and the edit's captions likewise (CHANGELOG.md, 0.5.0: "A line of
/// the transcript split between two pieces of an edit shows, under each,
/// only the words said there, and the edit's captions likewise."; edits.rs
/// `clip_segments`, `clip_cues`).
#[test]
fn lines_and_captions_follow_the_pieces_with_the_words_that_play() {
    let word = |text: &str, start: f64| Word { text: text.into(), start, end: start + 1.0 };
    let line = |start: f64, end: f64, text: &str, words: Vec<Word>| Segment { start, end, text: text.into(), words };
    let lines = |vod| -> Result<Vec<Segment>> {
        Ok(match vod {
            1 => vec![
                line(0.0, 4.0, "a b c d", vec![word("a", 0.0), word("b", 1.0), word("c", 2.0), word("d", 3.0)]),
                line(10.0, 11.0, "ten", vec![word("ten", 10.0)]),
            ],
            _ => vec![line(100.0, 103.0, "elsewhere", vec![])],
        })
    };
    // VOD 1 from 1 to 2.6 s (b, and most of c), VOD 2 from 101 s (a
    // line without word times), VOD 1 from 3 s (d, the line split
    // between them), then at 10 (all of "ten").
    let pieces = [piece(1, 1, 1.0, 2.6), piece(2, 2, 101.0, 102.0), piece(3, 1, 3.0, 4.0), piece(4, 1, 10.0, 11.0)];
    let got: Vec<(f64, f64, String)> =
        clip_segments(&pieces, lines).unwrap().into_iter().map(|s| (s.start, s.end, s.text)).collect();
    let want = [(0.0, 1.6, "b c"), (1.6, 2.6, "elsewhere"), (2.6, 3.6, "d"), (3.6, 4.6, "ten")];
    assert_eq!(got.len(), want.len(), "{got:?}");
    for ((start, end, text), (s, e, t)) in got.iter().zip(want) {
        assert!((start - s).abs() < 1e-9 && (end - e).abs() < 1e-9 && text == t, "{got:?}");
    }
    // Captions are made from the words that play: none of "a".
    let cues = clip_cues(&pieces, lines).unwrap();
    assert_eq!(cues.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["b c", "elsewhere", "d", "ten"]);
    // Each within the stretch it plays in ("c" runs past its piece's end at 2.6 s), none over the next.
    let within = [(0.0, 1.6), (1.6, 2.6), (2.6, 3.6), (3.6, 4.6)];
    for (c, (from, to)) in cues.iter().zip(within) {
        assert!(c.start >= from - 1e-9 && c.end <= to + 1e-9 && c.end > c.start, "{cues:?}");
    }
}

/// A moment an edit plays twice shows its chat once beside its transcript,
/// where it's first played (edits.rs `Db::edit_chat_windows`: "A moment
/// played twice in the window: its messages once, where first played").
#[test]
fn a_moment_played_twice_in_an_edit_shows_its_chat_once() {
    use crate::chat::{ChatMessage, ChatPart, Kind};
    let (mut db, vod, edit) = with_edit();
    let message = ChatMessage {
        time: 15.0,
        author: "ada".into(),
        color: None,
        parts: vec![ChatPart::Text { text: "hi".into() }],
        kind: Kind::Message,
        amount: None,
        event_time: None,
        at: None,
    };
    db.add_chat(vod, &[message], &[]).unwrap();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    db.add_piece(edit, vod, (12.0, 18.0), "again", None).unwrap();
    let windows = db.edit_chat_windows(edit, &[(0.0, 16.0)], 10).unwrap();
    assert_eq!(windows[0].messages.iter().map(|m| m.time).collect::<Vec<_>>(), [5.0]);
}

// Answers to the mutation run: what the specs above left unpinned.

/// A clip dropped into an edit brings the chapters and highlights that show
/// in it, and no cut and nothing trimmed off it (edits.rs `Db::add_group`:
/// "Its chapters and highlights that show in it come too, on the same
/// moments (cuts are where its own stretches were, not its)").
#[test]
fn a_clip_dropped_in_brings_only_the_marks_that_show_in_it() {
    let (db, vod, edit) = with_edit();
    let clip = db.add_clip(vod, 10.0, 20.0, "c", true).unwrap().id;
    db.add_clip_mark(clip, vod, "chapter", 12.0, Some("in")).unwrap();
    db.add_clip_mark(clip, vod, "cut", 13.0, None).unwrap();
    db.add_clip_mark(clip, vod, "highlight", 18.0, Some("trimmed off")).unwrap();
    db.set_clip_range(clip, 10.0, 15.0).unwrap();
    db.add_group(edit, clip, None).unwrap();
    let marks: Vec<_> = db.clip_marks(edit).unwrap().into_iter().map(|(_, m)| (m.kind, m.text)).collect();
    assert_eq!(marks, [("chapter".to_string(), Some("in".to_string()))]);
}

/// A clip of an edit's stretch brings the marks whose moments play in it:
/// one at the stretch's very end plays after it (edits.rs `Db::marks_inside`:
/// "whose moments play in `from`..`to` of its timeline").
#[test]
fn a_clip_of_an_edit_s_stretch_leaves_a_mark_at_its_end_behind() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    db.add_clip_mark(edit, vod, "chapter", 10.0, Some("start")).unwrap();
    db.add_clip_mark(edit, vod, "chapter", 15.0, Some("end")).unwrap();
    let clip = db.add_edit_clip(edit, 0.0, 5.0, "c", true).unwrap().id;
    let texts: Vec<_> = db.clip_marks(clip).unwrap().into_iter().map(|(_, m)| m.text).collect();
    assert_eq!(texts, [Some("start".to_string())]);
}

/// Several clips, highlights or stretches dropped at one place in an edit go
/// there in the order picked, a stretch of several pieces with them
/// (CHANGELOG.md, 0.6.0: "it all goes in, in the order you picked it";
/// edits.rs `Db::put`: "one after another in their order").
#[test]
fn several_things_dropped_at_one_place_keep_their_order() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (0.0, 1.0), "first", None).unwrap();
    let (a, b) =
        (db.add_clip(vod, 10.0, 11.0, "A", true).unwrap().id, db.add_clip(vod, 20.0, 21.0, "B", true).unwrap().id);
    let lit = |start: f64, title: &str| Put::Highlight { vod_id: vod, start, end: start + 1.0, title: title.into() };
    db.put(edit, &[Put::Clip { id: a }, Put::Clip { id: b }, lit(30.0, "C"), lit(40.0, "D")], Onto::Timeline(Some(0)))
        .unwrap();
    // Each by its band's name: a clip's group, a highlight's group, a piece.
    let names: Vec<String> = db
        .edit_pieces(edit)
        .unwrap()
        .iter()
        .map(|p| p.group.as_ref().map_or(p.title.clone(), |g| g.title.clone()))
        .collect();
    assert_eq!(names, ["A", "B", "C", "D", "first"]);
    // What plays over two of its pieces (A's and B's), put after the first.
    let other = db.add_edit("o").unwrap().id;
    db.add_piece(other, vod, (100.0, 101.0), "x", None).unwrap();
    db.add_piece(other, vod, (200.0, 201.0), "y", None).unwrap();
    db.put(other, &[Put::Span { clip: edit, from: 0.5, to: 1.5 }], Onto::Timeline(Some(1))).unwrap();
    assert_eq!(stretches(&db, other), [(100.0, 101.0), (10.5, 11.0), (20.0, 20.5), (200.0, 201.0)]);
}

/// Moving a mark that's gone says so (edits.rs `Db::move_mark_to`: "false if
/// it's gone").
#[test]
fn moving_a_mark_that_is_gone_says_so() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let mark = db.add_clip_mark(edit, vod, "chapter", 12.0, None).unwrap();
    db.delete_mark(mark).unwrap();
    assert!(!db.move_mark_to(mark, vod, 13.0).unwrap());
}

/// A stretch past an edit's end is refused, the edit as long as what its
/// pieces play, wherever in their VODs they are (edits.rs
/// `Db::check_edit_span`: "as long as what its pieces play").
#[test]
fn a_stretch_past_an_edit_s_end_is_refused() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (100.0, 110.0), "a", None).unwrap();
    let past = Some(crate::clips::PAST_THE_END.to_string());
    assert_eq!(said(db.remove_edit_stretch(edit, 15.0, 20.0)), past);
    assert_eq!(said(db.add_edit_clip(edit, 15.0, 20.0, "c", true)), past);
    assert_eq!(stretches(&db, edit), [(100.0, 110.0)]);
}

/// A mark at a piece's very end shows only where a piece plays that moment:
/// the end itself plays nothing (edits.rs `place_marks`: "A highlight shows
/// where its moment plays"; `source_at`: "the end itself plays nothing").
#[test]
fn a_mark_at_a_piece_s_very_end_shows_only_where_a_piece_plays_it() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    db.add_piece(edit, vod, (50.0, 60.0), "b", None).unwrap();
    let star = db.add_clip_mark(edit, vod, "highlight", 20.0, None).unwrap();
    assert_eq!(shown_at(&db, edit, star), None);
    db.add_piece(edit, vod, (15.0, 25.0), "c", None).unwrap();
    assert_eq!(shown_at(&db, edit, star), Some(25.0), "where the third piece plays it");
}

/// Of a line, only a sliver of which plays, nothing shows (edits.rs
/// `on_timeline`: "each thing cut to the stretch it shows in, slivers left
/// out").
#[test]
fn a_line_only_a_sliver_of_which_plays_is_left_out() {
    let line = Segment { start: 1.98, end: 3.0, text: "late".into(), words: vec![] };
    let lines = |_| -> Result<Vec<Segment>> { Ok(vec![line.clone()]) };
    assert!(clip_segments(&[piece(1, 1, 1.0, 2.0)], lines).unwrap().is_empty());
    assert_eq!(clip_segments(&[piece(1, 1, 1.0, 2.1)], lines).unwrap().len(), 1, "more than a sliver");
}

/// A caption that runs to a join is held there a moment, as a VOD's are,
/// but never over the next, nor past the edit's end (edits.rs `clip_cues`:
/// "One that runs to a join is held there a moment ... but never over the
/// next, nor past the clip's end").
#[test]
fn a_caption_at_a_join_is_held_a_moment_but_not_over_the_next() {
    let word = |text: &str, start: f64| Word { text: text.into(), start, end: start + 1.0 };
    let lines = |vod| -> Result<Vec<Segment>> {
        Ok(match vod {
            1 => {
                vec![Segment { start: 0.0, end: 2.0, text: "a b".into(), words: vec![word("a", 0.0), word("b", 1.0)] }]
            }
            _ => vec![Segment { start: 11.0, end: 12.0, text: "c".into(), words: vec![word("c", 11.0)] }],
        })
    };
    // VOD 1's 0-2 s (0-2 of the timeline), then VOD 2's 10-12 s (2-4), "c" at 3.
    let cues = clip_cues(&[piece(1, 1, 0.0, 2.0), piece(2, 2, 10.0, 12.0)], lines).unwrap();
    let got: Vec<(f64, f64)> = cues.iter().map(|c| (c.start, c.end)).collect();
    assert!(same_times(&got, &[(0.0, 2.0 + crate::transcript::CUE_HOLD), (3.0, 4.0)]), "{got:?}");
    // The next one coming first: held only till it starts.
    let cues = clip_cues(&[piece(1, 1, 0.0, 2.0), piece(2, 2, 10.8, 12.0)], lines).unwrap();
    assert!((cues[0].end - 2.2).abs() < 1e-9, "{cues:?}");
}
