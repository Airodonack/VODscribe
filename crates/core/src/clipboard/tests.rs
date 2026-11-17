//! The specs of Ctrl+C and Ctrl+V in the library: `Db::copy_of` and
//! `Db::paste`. A spec each, named as it is said, its source cited.

use super::*;
use crate::testkit::library::add_site_vod;
use std::path::Path;

/// A library in memory with one VOD, ten minutes long.
fn library() -> (Db, i64) {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let vod = add_site_vod(&db, "youtube", "x", "VOD x", 600.0).id;
    (db, vod)
}

/// Another VOD in `db`, ten minutes long.
fn another(db: &Db, id: &str) -> i64 {
    add_site_vod(db, "youtube", id, &format!("VOD {id}"), 600.0).id
}

/// A copy of `start`..`end` of VOD `vod`, as a stretch of its timeline.
fn stretch(db: &Db, vod: i64, start: f64, end: f64) -> Copied {
    db.copy_of(&Put::Range { vod_id: vod, start, end, title: None }).unwrap()
}

/// An edit's pieces as (VOD, start, end, group title).
fn shape(db: &Db, edit: i64) -> Vec<(i64, f64, f64, Option<String>)> {
    db.edit_pieces(edit).unwrap().into_iter().map(|p| (p.vod_id, p.start, p.end, p.group.map(|g| g.title))).collect()
}

/// Clip `id`'s chapters and highlights, by their text, in order.
fn mark_texts(db: &Db, id: i64) -> Vec<Option<String>> {
    db.clip_marks(id).unwrap().into_iter().map(|(_, m)| m.text).collect()
}

// Copies, not references.

/// What's copied is what it was when copied: an edit's stretch changed after
/// the copy pastes as it was (clipboard.rs's module doc: "A copy (`Copied`) is
/// what a clip or a stretch was at the moment it was copied ... so changing or
/// deleting the original afterwards changes nothing pasted"; CHANGELOG.md
/// 0.7.1: "Copy and paste clips.").
#[test]
fn a_copy_of_an_edits_stretch_is_what_it_was_when_copied() {
    let (db, vod) = library();
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    db.add_piece(edit, vod, (30.0, 40.0), "b", None).unwrap();
    let copy = db.copy_of(&Put::Span { clip: edit, from: 5.0, to: 15.0 }).unwrap();
    assert_eq!(copy.kind, CopiedKind::Stretch);
    // The edit changes after the copy: its second piece goes.
    let second = db.edit_pieces(edit).unwrap()[1].id;
    db.remove_piece(second).unwrap();
    let other = db.add_edit("f").unwrap().id;
    let bar = db.paste(Paste::Bar { bar: other, after: None }, &[copy]).unwrap().clips;
    assert_eq!(bar.len(), 1);
    let pasted: Vec<(f64, f64)> = db.edit_pieces(bar[0].0).unwrap().iter().map(|p| (p.start, p.end)).collect();
    assert_eq!(pasted, [(15.0, 20.0), (30.0, 35.0)], "the pieces as they were copied");
}

/// A clip copied, then changed or deleted, pastes as it was copied
/// (clipboard.rs's module doc: "changing or deleting the original afterwards
/// changes nothing pasted").
#[test]
fn a_clip_copied_then_changed_or_deleted_pastes_as_it_was_copied() {
    let (db, vod) = library();
    let pill = db.add_clip(vod, 10.0, 20.0, "Hello", true).unwrap().id;
    let copy = db.copy_of(&Put::Clip { id: pill }).unwrap();
    let edit = db.add_edit("e").unwrap().id;
    let onto = Paste::Bar { bar: edit, after: None };
    db.set_clip_range(pill, 10.0, 30.0).unwrap();
    let changed = db.paste(onto, std::slice::from_ref(&copy)).unwrap().clips[0].0;
    db.delete_clip(pill).unwrap();
    let deleted = db.paste(onto, &[copy]).unwrap().clips[0].0;
    for (id, after) in [(changed, "changed"), (deleted, "deleted")] {
        assert_eq!(shape(&db, id), [(vod, 10.0, 20.0, None)], "pasted after the clip was {after}");
        assert_eq!(db.clip(id).unwrap().unwrap().title, "Hello", "pasted after the clip was {after}");
    }
}

/// A pill pasted on its own clip bar is a second clip, a copy with its own
/// marks: deleting the pill leaves it as it is (clipboard.rs's module doc:
/// "copies, not references"; "Pasted on a clip bar, each copy is a new clip
/// there"; tests/e2e/scenarios/copy-paste.mjs: "That pill clicked and copied,
/// pasted on its own bar, is a second pill of the same stretch").
#[test]
fn a_pill_pasted_on_its_own_bar_is_a_second_clip_and_its_own() {
    let (db, vod) = library();
    db.add_mark(vod, "highlight", 12.0, Some("wow")).unwrap();
    let pill = db.add_clip(vod, 10.0, 20.0, "Hello", true).unwrap().id;
    let copy = db.copy_of(&Put::Clip { id: pill }).unwrap();
    assert_eq!(copy.kind, CopiedKind::Clip);
    let bar = db.vod_clip(vod).unwrap().unwrap();
    let made: Vec<i64> =
        db.paste(Paste::Bar { bar, after: None }, &[copy]).unwrap().clips.iter().map(|c| c.0).collect();
    assert_eq!(made.len(), 1);
    assert_ne!(made[0], pill);
    let clip = db.clip(made[0]).unwrap().unwrap();
    assert_eq!((clip.start, clip.end, clip.title.as_str(), clip.shown), (10.0, 20.0, "Hello", true));
    assert_eq!(db.clips(vod).unwrap().len(), 2);
    // Its marks are its own: the pill's highlight came with it, and
    // deleting the pill leaves the copy as it is.
    assert_eq!(mark_texts(&db, made[0]), [Some("wow".to_string())]);
    db.delete_clip(pill).unwrap();
    assert_eq!(mark_texts(&db, made[0]), [Some("wow".to_string())]);
    assert_eq!(db.clip(made[0]).unwrap().unwrap().end, 20.0);
}

/// A pasted pill's marks are the copy's: the VOD's marks changed after the
/// copy aren't in it; in a timeline it brings its chapters and highlights,
/// not its cuts, and on a clip bar the clip whole, cuts too (`group_marks`:
/// "its chapters and highlights whose moments it plays, as `add_group` takes
/// a clip's"; clipboard.rs's module doc: "copies, not references").
#[test]
fn a_pasted_pills_marks_are_its_own() {
    let (db, vod) = library();
    db.add_mark(vod, "chapter", 12.0, Some("in")).unwrap();
    let pill = db.add_clip(vod, 10.0, 20.0, "Hello", true).unwrap().id;
    db.add_clip_mark(pill, vod, "cut", 15.0, None).unwrap();
    let copy = db.copy_of(&Put::Clip { id: pill }).unwrap();
    // The VOD's marks change after the copy: the pasted one keeps the copy's.
    db.add_mark(vod, "highlight", 14.0, Some("later")).unwrap();
    let edit = db.add_edit("e").unwrap().id;
    db.paste(Paste::Timeline { clip: edit, at: 0.0 }, std::slice::from_ref(&copy)).unwrap();
    let kinds = |clip| -> Vec<(String, Option<String>)> {
        db.clip_marks(clip).unwrap().into_iter().map(|(_, m)| (m.kind, m.text)).collect()
    };
    assert_eq!(kinds(edit), [("chapter".to_string(), Some("in".to_string()))], "not its cut, not the new highlight");
    // On a bar, the clip whole: its cut too.
    let bar = db.paste(Paste::Bar { bar: edit, after: None }, &[copy]).unwrap().clips[0].0;
    assert_eq!(kinds(bar), [("chapter".to_string(), Some("in".to_string())), ("cut".to_string(), None)]);
}

/// A VOD's stretch copied brings the VOD's chapters and highlights inside
/// it, and none outside it (`Db::copy_of`: "a stretch of a VOD ... as the
/// pieces that play there, with the chapters and highlights inside it").
#[test]
fn a_vods_stretch_copied_brings_the_chapters_and_highlights_inside_it() {
    let (db, vod) = library();
    db.add_mark(vod, "chapter", 12.0, Some("chapter in")).unwrap();
    db.add_mark(vod, "highlight", 15.0, Some("highlight in")).unwrap();
    db.add_mark(vod, "highlight", 5.0, Some("before")).unwrap();
    db.add_mark(vod, "chapter", 25.0, Some("after")).unwrap();
    // One at its start is inside it; one at its end begins after it.
    db.add_mark(vod, "highlight", 10.0, Some("at its start")).unwrap();
    db.add_mark(vod, "chapter", 20.0, Some("at its end")).unwrap();
    let copy = stretch(&db, vod, 10.0, 20.0);
    let edit = db.add_edit("e").unwrap().id;
    let pasted = db.paste(Paste::Bar { bar: edit, after: None }, &[copy]).unwrap().clips[0].0;
    let mut texts = mark_texts(&db, pasted);
    texts.sort();
    let want = ["at its start", "chapter in", "highlight in"];
    assert_eq!(texts, want.map(|t| Some(t.to_string())));
}

/// An edit's stretch copied brings the edit's chapters and highlights inside
/// it, and none outside it (`Db::copy_of`: "a stretch ... of an edit's
/// timeline as the pieces that play there, with the chapters and highlights
/// inside it").
#[test]
fn an_edits_stretch_copied_brings_its_chapters_and_highlights_inside_it() {
    let (db, vod) = library();
    db.add_mark(vod, "highlight", 12.0, Some("in")).unwrap();
    db.add_mark(vod, "chapter", 18.0, Some("out")).unwrap();
    let edit = db.add_edit("e").unwrap().id;
    // The VOD's 10..20 s, with its marks: they're the edit's at 2 s and 8 s.
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    assert_eq!(mark_texts(&db, edit), [Some("in".to_string()), Some("out".to_string())]);
    // The edit's: one of its own, and not one the VOD gets after.
    db.add_clip_mark(edit, vod, "chapter", 13.0, Some("the edit's own")).unwrap();
    db.add_mark(vod, "highlight", 14.0, Some("the VOD's since")).unwrap();
    let copy = db.copy_of(&Put::Span { clip: edit, from: 0.0, to: 5.0 }).unwrap();
    let other = db.add_edit("f").unwrap().id;
    let pasted = db.paste(Paste::Bar { bar: other, after: None }, &[copy]).unwrap().clips[0].0;
    let mut texts = mark_texts(&db, pasted);
    texts.sort();
    assert_eq!(texts, [Some("in".to_string()), Some("the edit's own".to_string())]);
}

/// A highlight copied goes in a timeline as a clip's pill does, one band
/// named for it (`Db::copy_of`: "A highlight goes in a timeline as a clip's
/// pill does: a band named for it").
#[test]
fn a_highlight_copied_goes_in_a_timeline_as_a_band_named_for_it() {
    let (db, vod) = library();
    let copy = db.copy_of(&Put::Highlight { vod_id: vod, start: 10.0, end: 20.0, title: "Wow".into() }).unwrap();
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (30.0, 40.0), "a", None).unwrap();
    db.paste(Paste::Timeline { clip: edit, at: 10.0 }, &[copy]).unwrap();
    assert_eq!(shape(&db, edit), [(vod, 30.0, 40.0, None), (vod, 10.0, 20.0, Some("Wow".into()))]);
}

// On a clip bar.

/// Pasted on a clip bar with no clip named, the copies go after the others,
/// in the order they were copied (`Paste::Bar`: "each a clip of its own:
/// after clip `after` of the bar, or else after the others"; `Db::paste`:
/// "Puts `items` (copies, in their order)").
#[test]
fn pasted_on_a_bar_the_copies_go_after_the_others_in_order() {
    let (db, vod) = library();
    let bar = db.vod_clip(vod).unwrap().unwrap();
    let a = db.add_clip(vod, 1.0, 2.0, "a", true).unwrap().id;
    let b = db.add_clip(vod, 3.0, 4.0, "b", true).unwrap().id;
    let copies = [stretch(&db, vod, 7.0, 8.0), stretch(&db, vod, 5.0, 6.0)];
    let made: Vec<i64> =
        db.paste(Paste::Bar { bar, after: None }, &copies).unwrap().clips.iter().map(|c| c.0).collect();
    let order: Vec<i64> = db.clips(vod).unwrap().iter().map(|c| c.id).collect();
    assert_eq!(order, [a, b, made[0], made[1]]);
    let starts: Vec<f64> = made.iter().map(|&id| db.clip(id).unwrap().unwrap().start).collect();
    assert_eq!(starts, [7.0, 5.0], "in the order copied");
}

/// Pasted on a clip bar after a clip, the copies go next to it, in order
/// (`Paste::Bar`: "after clip `after` of the bar";
/// apps/desktop/src-tauri/src/clipboard.rs `paste_items`: "on its clip bar if
/// `bar` (after its clip `after`, else last)").
#[test]
fn pasted_on_a_bar_after_a_clip_it_goes_next_to_it() {
    let (db, vod) = library();
    let bar = db.vod_clip(vod).unwrap().unwrap();
    let a = db.add_clip(vod, 1.0, 2.0, "a", true).unwrap().id;
    let b = db.add_clip(vod, 3.0, 4.0, "b", true).unwrap().id;
    let copies = [stretch(&db, vod, 5.0, 6.0), stretch(&db, vod, 7.0, 8.0)];
    let made: Vec<i64> =
        db.paste(Paste::Bar { bar, after: Some(a) }, &copies).unwrap().clips.iter().map(|c| c.0).collect();
    let order: Vec<i64> = db.clips(vod).unwrap().iter().map(|c| c.id).collect();
    assert_eq!(order, [a, made[0], made[1], b]);
}

/// A stretch pasted on a clip bar where it's already a clip is that clip:
/// pasted twice, one clip, and one only dragged out is shown; a clip copied
/// is still pasted beside itself (clipboard.rs's module doc: "a stretch
/// already there as a clip is that clip, not a second one"; `Pasted::found`;
/// tests/e2e/scenarios/copy-paste.mjs: "pasted again, no second pill: the one
/// there is picked").
#[test]
fn a_stretch_pasted_on_a_bar_twice_is_one_clip() {
    let (db, vod) = library();
    let bar = db.vod_clip(vod).unwrap().unwrap();
    let copy = stretch(&db, vod, 1.0, 2.0);
    let onto = Paste::Bar { bar, after: None };
    let first = db.paste(onto, std::slice::from_ref(&copy)).unwrap();
    assert_eq!((first.clips.len(), first.found.len()), (1, 0));
    let pill = first.clips[0].0;
    let revision = db.revision(bar).unwrap();
    // Again: no second clip, and the answer says which clip it is.
    let again = db.paste(onto, std::slice::from_ref(&copy)).unwrap();
    assert_eq!((again.clips, again.found), (vec![], vec![(pill, 0)]));
    assert_eq!(db.clips(vod).unwrap().len(), 1);
    assert_eq!(db.revision(bar).unwrap(), revision, "nothing changed");
    // A stretch from the same moment but longer is a clip of its own.
    let longer = db.paste(onto, &[stretch(&db, vod, 1.0, 3.0)]).unwrap();
    assert_eq!((longer.clips.len(), longer.found.len()), (1, 0));
    db.delete_clip(longer.clips[0].0).unwrap();
    // One made by C (the same stretch) is that clip too; one only
    // dragged out is shown.
    db.delete_clip(pill).unwrap();
    let dragged = db.add_clip(vod, 1.0, 2.0, "", false).unwrap().id;
    let shown = db.paste(onto, std::slice::from_ref(&copy)).unwrap();
    assert_eq!(shown.found, [(dragged, 0)]);
    assert!(db.clip(dragged).unwrap().unwrap().shown);
    // A clip copied is still pasted beside itself.
    let pill = db.copy_of(&Put::Clip { id: dragged }).unwrap();
    assert_eq!(db.paste(onto, &[pill]).unwrap().clips.len(), 1);
}

/// A VOD's clip bar, and a clip of it open in the editor, take only that
/// VOD's footage: another VOD's copies are refused, the rest pasted; an edit
/// takes any VOD's (`Db::paste`: "On a VOD's clip bar, or in a clip of it open
/// in the editor, only copies of that VOD's own footage go (`Pasted::refused`);
/// an error if none are"; tests/e2e/scenarios/copy-paste.mjs: "on the first
/// VOD's own bar it's refused, nothing made").
#[test]
fn a_vods_bar_takes_only_its_own_footage() {
    let (db, vod) = library();
    let other = another(&db, "y");
    let theirs = stretch(&db, other, 1.0, 2.0);
    let ours = stretch(&db, vod, 1.0, 2.0);
    let bar = db.vod_clip(vod).unwrap().unwrap();
    let refused = db.paste(Paste::Bar { bar, after: None }, std::slice::from_ref(&theirs)).unwrap_err();
    assert_eq!(refused.to_string(), "Clips of another VOD go in an edit.");
    assert!(db.clips(vod).unwrap().is_empty());
    let pasted = db.paste(Paste::Bar { bar, after: None }, &[theirs.clone(), ours]).unwrap();
    assert_eq!((pasted.clips.len(), pasted.refused), (1, 1));
    // A clip of the VOD's bar open in the editor: the same.
    assert_eq!(pasted.clips[0].1, 1, "the second copy's");
    let pill = pasted.clips[0].0;
    assert!(db.paste(Paste::Timeline { clip: pill, at: 0.5 }, std::slice::from_ref(&theirs)).is_err());
    // An edit takes any VOD's.
    let edit = db.add_edit("e").unwrap().id;
    db.paste(Paste::Bar { bar: edit, after: None }, &[theirs]).unwrap();
}

// In a timeline.

/// Pasted in a timeline at the playhead, a copied clip goes in as a group and
/// a copied stretch as pieces, the piece there split at that moment; inside a
/// group, at its nearer edge, the group left whole; a VOD's own timeline
/// takes nothing (clipboard.rs's module doc: "on an edit's timeline, it goes
/// in at a moment of it (`Paste::Timeline`), a copied clip as a group (one
/// band, as a clip dropped there), a copied stretch as pieces"; `insertion`:
/// "As pasted text goes in at the caret: the piece there is split at `at` ...
/// inside a group ... at its nearer edge, the group left whole").
#[test]
fn pasted_in_a_timeline_at_the_playhead_splits_the_piece_there() {
    let (db, vod) = library();
    let pill = db.add_clip(vod, 1.0, 2.0, "Hello", true).unwrap().id;
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (1.0, 2.0), "a", None).unwrap();
    let copy = db.copy_of(&Put::Clip { id: pill }).unwrap();
    let before = db.edit_state(edit).unwrap().unwrap();
    let pasted = db.paste(Paste::Timeline { clip: edit, at: 0.5 }, std::slice::from_ref(&copy)).unwrap();
    assert_eq!(pasted.span, Some((0.5, 1.5)));
    assert_eq!(shape(&db, edit), [(vod, 1.0, 1.5, None), (vod, 1.0, 2.0, Some("Hello".into())), (vod, 1.5, 2.0, None)]);
    // Undone (the state before put back), one piece again.
    db.restore_edit(edit, &before).unwrap();
    assert_eq!(shape(&db, edit), [(vod, 1.0, 2.0, None)]);

    // Inside a group: at its nearer edge, the group left whole.
    db.paste(Paste::Timeline { clip: edit, at: 1.0 }, std::slice::from_ref(&copy)).unwrap();
    let copied = stretch(&db, vod, 5.0, 6.0);
    let pasted = db.paste(Paste::Timeline { clip: edit, at: 1.8 }, std::slice::from_ref(&copied)).unwrap();
    assert_eq!(pasted.span, Some((2.0, 3.0)));
    assert_eq!(shape(&db, edit), [(vod, 1.0, 2.0, None), (vod, 1.0, 2.0, Some("Hello".into())), (vod, 5.0, 6.0, None)]);
    // A VOD's own timeline takes nothing.
    let own = db.vod_clip(vod).unwrap().unwrap();
    assert!(db.paste(Paste::Timeline { clip: own, at: 0.0 }, &[copy]).is_err());
}

/// A paste within a sliver of a piece's start or end splits nothing: it goes
/// in before or after the piece (`insertion`: "nothing split within a sliver
/// of its ends").
#[test]
fn a_paste_within_a_sliver_of_a_pieces_ends_splits_nothing() {
    let (db, vod) = library();
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let before = db.edit_state(edit).unwrap().unwrap();
    let copy = stretch(&db, vod, 1.0, 2.0);
    let pasted = db.paste(Paste::Timeline { clip: edit, at: 9.99 }, std::slice::from_ref(&copy)).unwrap();
    assert_eq!(pasted.span, Some((10.0, 11.0)));
    assert_eq!(shape(&db, edit), [(vod, 10.0, 20.0, None), (vod, 1.0, 2.0, None)], "just before the end");
    db.restore_edit(edit, &before).unwrap();
    let pasted = db.paste(Paste::Timeline { clip: edit, at: 0.01 }, &[copy]).unwrap();
    assert_eq!(pasted.span, Some((0.0, 1.0)));
    assert_eq!(shape(&db, edit), [(vod, 1.0, 2.0, None), (vod, 10.0, 20.0, None)], "just after the start");
}

/// With a stretch selected, a paste goes in after it, at its end (the piece
/// there split as at the playhead), never in its place; pasted again at the
/// end of what was pasted, the copies line up in order (`Paste::Timeline`:
/// "the end of the stretch selected (a paste goes in after it, never in its
/// place)"; CHANGELOG.md 0.7.1: "Copy and paste clips." ("after what's
/// selected, or at the playhead when nothing is")).
#[test]
fn pasted_after_a_selection_splits_the_piece_at_its_end_and_replaces_nothing() {
    let (db, vod) = library();
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let one = stretch(&db, vod, 1.0, 2.0);
    let two = stretch(&db, vod, 5.0, 7.0);
    let before = db.edit_state(edit).unwrap().unwrap();
    // 2..4 selected: the paste at its end, 4 s in.
    let pasted = db.paste(Paste::Timeline { clip: edit, at: 4.0 }, std::slice::from_ref(&one)).unwrap();
    assert_eq!(pasted.span, Some((4.0, 5.0)));
    assert_eq!(shape(&db, edit), [(vod, 10.0, 14.0, None), (vod, 1.0, 2.0, None), (vod, 14.0, 20.0, None)]);
    // Pasted again after what was pasted (now selected): next in line.
    let pasted = db.paste(Paste::Timeline { clip: edit, at: 5.0 }, std::slice::from_ref(&two)).unwrap();
    assert_eq!(pasted.span, Some((5.0, 7.0)));
    assert_eq!(
        shape(&db, edit),
        [(vod, 10.0, 14.0, None), (vod, 1.0, 2.0, None), (vod, 5.0, 7.0, None), (vod, 14.0, 20.0, None)]
    );
    // The first paste's split and copy are one change: the state before it put back, one piece again.
    db.restore_edit(edit, &before).unwrap();
    assert_eq!(shape(&db, edit), [(vod, 10.0, 20.0, None)]);
}

// A VOD gone since the copy.

/// Pieces of a VOD gone since the copy are left out and counted; with
/// nothing left, the paste is refused and nothing changes (`Db::paste`:
/// "Pieces whose VOD is gone since the copy are left out (`Pasted::dropped`);
/// an error if nothing's left"; tests/e2e/scenarios/copy-paste.mjs: "the app
/// says its VOD is gone and the edit stays as it was").
#[test]
fn pieces_of_a_vod_gone_since_the_copy_are_left_out() {
    let (db, vod) = library();
    let other = another(&db, "y");
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (0.0, 10.0), "a", None).unwrap();
    db.add_piece(edit, other, (0.0, 10.0), "b", None).unwrap();
    let both = db.copy_of(&Put::Clip { id: edit }).unwrap();
    let theirs = stretch(&db, other, 1.0, 2.0);
    // The other VOD taken out of the library and used no more: erased.
    let other_edit_piece = db.edit_pieces(edit).unwrap()[1].id;
    db.remove_piece(other_edit_piece).unwrap();
    assert!(db.remove_entry(other).unwrap(), "nothing uses it");
    let target = db.add_edit("f").unwrap().id;
    let pasted = db.paste(Paste::Bar { bar: target, after: None }, &[both]).unwrap();
    assert_eq!(pasted.dropped, 1);
    assert_eq!(shape(&db, pasted.clips[0].0), [(vod, 0.0, 10.0, None)]);
    let before = db.edit_state(target).unwrap();
    let gone = db.paste(Paste::Timeline { clip: target, at: 0.0 }, &[theirs]).unwrap_err();
    assert_eq!(gone.to_string(), "The VOD it was copied from is gone.");
    assert_eq!(db.edit_state(target).unwrap(), before, "nothing changed");
}

/// A copy pasted after a VOD in it went leaves out that VOD's groups, left
/// empty, and its marks (`present`: "less its pieces whose VOD is gone ...
/// and the groups left empty, with its marks on those VODs").
#[test]
fn a_copy_pasted_after_its_vod_went_leaves_out_that_vods_groups_and_marks() {
    let (db, vod) = library();
    let other = another(&db, "y");
    db.add_mark(vod, "highlight", 4.0, Some("ours")).unwrap();
    db.add_mark(other, "highlight", 3.0, Some("theirs")).unwrap();
    let pill = db.add_clip(other, 1.0, 5.0, "Theirs", true).unwrap().id;
    let ours = db.add_clip(vod, 20.0, 30.0, "Ours", true).unwrap().id;
    let edit = db.add_edit("e").unwrap().id;
    // The other VOD's piece first: what's kept after it moves up.
    db.add_piece(edit, other, (40.0, 50.0), "b", None).unwrap();
    db.add_piece(edit, vod, (0.0, 10.0), "a", None).unwrap();
    db.add_group(edit, pill, None).unwrap();
    db.add_group(edit, ours, None).unwrap();
    let copy = db.copy_of(&Put::Clip { id: edit }).unwrap();
    assert!(copy.state.marks.iter().any(|m| m.vod_id == other), "the copy has the other VOD's highlight");
    // The edit deleted, the other VOD taken out of the library: nothing uses it.
    db.delete_edit(edit).unwrap();
    assert!(db.remove_entry(other).unwrap(), "nothing uses it");
    let target = db.add_edit("f").unwrap().id;
    let pasted = db.paste(Paste::Bar { bar: target, after: None }, &[copy]).unwrap();
    assert_eq!(pasted.dropped, 2);
    let id = pasted.clips[0].0;
    assert_eq!(db.edit_state(id).unwrap().unwrap().items.len(), 3, "an empty group pasted");
    assert_eq!(shape(&db, id), [(vod, 0.0, 10.0, None), (vod, 20.0, 30.0, Some("Ours".into()))]);
    assert!(db.clip_marks(id).unwrap().iter().all(|(_, m)| m.vod_id == vod), "the other VOD's marks pasted");
}

/// A VOD taken out of the library that an edit still uses isn't gone for a
/// paste: its pieces go in (`gone_for_paste`: gone is "no row, or taken out
/// of the library with nothing using it").
#[test]
fn a_vod_out_of_the_library_that_an_edit_still_uses_still_pastes() {
    let (db, _) = library();
    let other = another(&db, "y");
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, other, (0.0, 10.0), "b", None).unwrap();
    let copy = stretch(&db, other, 1.0, 2.0);
    assert!(!db.remove_entry(other).unwrap(), "the edit uses it");
    let target = db.add_edit("f").unwrap().id;
    let pasted = db.paste(Paste::Bar { bar: target, after: None }, &[copy]).unwrap();
    assert_eq!((pasted.clips.len(), pasted.dropped), (1, 0));
    assert_eq!(shape(&db, pasted.clips[0].0), [(other, 1.0, 2.0, None)]);
}

/// Pasted in a timeline, the copies go in in the order they were copied,
/// clips and stretches alike (`Db::paste`: "Puts `items` (copies, in their
/// order)").
#[test]
fn pasted_in_a_timeline_the_copies_go_in_in_order() {
    let (db, vod) = library();
    let a = db.add_clip(vod, 1.0, 2.0, "a", true).unwrap().id;
    let b = db.add_clip(vod, 3.0, 4.0, "b", true).unwrap().id;
    let clips = [db.copy_of(&Put::Clip { id: a }).unwrap(), db.copy_of(&Put::Clip { id: b }).unwrap()];
    let edit = db.add_edit("e").unwrap().id;
    db.paste(Paste::Timeline { clip: edit, at: 0.0 }, &clips).unwrap();
    assert_eq!(shape(&db, edit), [(vod, 1.0, 2.0, Some("a".into())), (vod, 3.0, 4.0, Some("b".into()))]);
    let stretches = [stretch(&db, vod, 5.0, 6.0), stretch(&db, vod, 7.0, 8.0)];
    let other = db.add_edit("f").unwrap().id;
    db.paste(Paste::Timeline { clip: other, at: 0.0 }, &stretches).unwrap();
    assert_eq!(shape(&db, other), [(vod, 5.0, 6.0, None), (vod, 7.0, 8.0, None)]);
}

/// Pasted at a moment of a later piece, that piece is split there, at the
/// VOD's moment that plays then (`insertion`: "the piece there is split at
/// `at`").
#[test]
fn pasted_in_a_later_piece_it_splits_that_piece_at_the_moment() {
    let (db, vod) = library();
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    db.add_piece(edit, vod, (30.0, 40.0), "b", None).unwrap();
    let pasted = db.paste(Paste::Timeline { clip: edit, at: 10.3 }, &[stretch(&db, vod, 1.0, 2.0)]).unwrap();
    assert_eq!(pasted.span, Some((10.3, 11.3)));
    let pieces = shape(&db, edit);
    let near = |(v, a, b, g): &(i64, f64, f64, Option<String>), want: (f64, f64)| {
        *v == vod && g.is_none() && (a - want.0).abs() < 1e-6 && (b - want.1).abs() < 1e-6
    };
    assert_eq!(pieces.len(), 4, "{pieces:?}");
    for (piece, want) in pieces.iter().zip([(10.0, 20.0), (30.0, 30.3), (1.0, 2.0), (30.3, 40.0)]) {
        assert!(near(piece, want), "{pieces:?}");
    }
}

/// Inside a group of several pieces, a paste goes at the group's nearer
/// edge, the group left whole (`insertion`: "inside a group (a clip dropped
/// in, one band), at its nearer edge, the group left whole").
#[test]
fn pasted_inside_a_group_it_goes_at_the_groups_nearer_edge() {
    let (db, vod) = library();
    let dropped = db.add_edit("dropped").unwrap().id;
    db.add_piece(dropped, vod, (10.0, 12.0), "a", None).unwrap();
    db.add_piece(dropped, vod, (20.0, 23.0), "b", None).unwrap();
    let edit = db.add_edit("e").unwrap().id;
    // A second's piece, then the group: it plays from 1 s to 6 s.
    db.add_piece(edit, vod, (40.0, 41.0), "c", None).unwrap();
    db.add_group(edit, dropped, None).unwrap();
    let before = db.edit_state(edit).unwrap().unwrap();
    let copy = stretch(&db, vod, 1.0, 2.0);
    let group = Some("dropped".to_string());
    let piece = (vod, 40.0, 41.0, None);
    let (a, b) = ((vod, 10.0, 12.0, group.clone()), (vod, 20.0, 23.0, group));
    // 4.5 s, in its second piece: nearer its end.
    let pasted = db.paste(Paste::Timeline { clip: edit, at: 4.5 }, std::slice::from_ref(&copy)).unwrap();
    assert_eq!(pasted.span, Some((6.0, 7.0)));
    assert_eq!(shape(&db, edit), [piece.clone(), a.clone(), b.clone(), (vod, 1.0, 2.0, None)]);
    db.restore_edit(edit, &before).unwrap();
    // 3 s, in its second piece too: nearer its start.
    let pasted = db.paste(Paste::Timeline { clip: edit, at: 3.0 }, &[copy]).unwrap();
    assert_eq!(pasted.span, Some((1.0, 2.0)));
    assert_eq!(shape(&db, edit), [piece, (vod, 1.0, 2.0, None), a, b]);
}

/// A copied clip goes in a timeline with the chapters and highlights whose
/// moments it plays, and none it holds of moments it doesn't play
/// (`group_marks`: "its chapters and highlights whose moments it plays").
#[test]
fn a_copied_clip_goes_in_a_timeline_with_only_the_marks_it_plays() {
    let (db, vod) = library();
    let other = another(&db, "y");
    let dropped = db.add_edit("dropped").unwrap().id;
    db.add_piece(dropped, vod, (0.0, 10.0), "a", None).unwrap();
    db.add_piece(dropped, other, (50.0, 60.0), "b", None).unwrap();
    db.add_clip_mark(dropped, other, "highlight", 55.0, Some("played")).unwrap();
    // The other VOD's moments it doesn't play: 5 s (the first VOD's 5 s
    // plays) and 60 s, where its piece ends.
    db.add_clip_mark(dropped, other, "highlight", 5.0, Some("not played")).unwrap();
    db.add_clip_mark(dropped, other, "chapter", 60.0, Some("at its end")).unwrap();
    let copy = db.copy_of(&Put::Clip { id: dropped }).unwrap();
    let edit = db.add_edit("e").unwrap().id;
    db.paste(Paste::Timeline { clip: edit, at: 0.0 }, &[copy]).unwrap();
    assert_eq!(mark_texts(&db, edit), [Some("played".to_string())]);
}
