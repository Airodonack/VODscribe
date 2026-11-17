//! What a clip's rendered file is made of (`Db::made_of`), whether it's of
//! the clip as it is (`Edit::current`), and what's rendered of its pieces
//! (`render_pieces`).

use super::*;

/// Records that clip `id` has a ready file, an MP4 where the library keeps
/// it, of revision `revision` (None: from before revisions were recorded).
fn rendered(db: &Db, id: i64, revision: Option<i64>) {
    let file = db.files().clip_file(id, "mp4");
    db.set_edit_render(id, ClipStatus::Ready, Some(&file.to_string_lossy()), None, revision).unwrap();
}

/// A rendered edit's file says whether it's of the edit as it is; a piece
/// taken out after makes it out of date, the file kept where it is (`Edit`:
/// "Whether the rendered file is of the edit as it is now").
#[test]
fn a_rendered_file_says_whether_it_is_of_the_edit_as_it_is() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    db.add_piece(edit, vod, (30.0, 35.0), "b", None).unwrap();
    rendered(&db, edit, db.revision(edit).unwrap());
    assert!(db.edit(edit, at_first()).unwrap().unwrap().current);
    db.remove_piece(db.piece_rows(edit).unwrap()[1].id).unwrap();
    let after = db.edit(edit, at_first()).unwrap().unwrap();
    assert!(!after.current);
    assert_eq!(after.file_path, Some(db.files().clip_file(edit, "mp4").to_string_lossy().into_owned()));
}

/// A rendered edit shows as changed only when its video would be different
/// (CHANGELOG.md, 0.5.0: "A rendered edit has its chapters, as chapters of
/// the video file that players and editors show. It shows as changed since
/// it was rendered only when its video would be different: a highlight or a
/// cut added doesn't count"; edits.rs `Db::made_of`).
#[test]
fn a_rendered_file_is_out_of_date_only_when_what_it_holds_changes() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let made = || {
        let e = db.edit(edit, at_first()).unwrap().unwrap();
        db.made_of(edit, &e.title, &db.edit_pieces(edit).unwrap()).unwrap()
    };
    rendered(&db, edit, db.revision(edit).unwrap());
    db.set_made_of(edit, &made()).unwrap();
    let current = || db.edit(edit, at_first()).unwrap().unwrap().current;
    assert!(current());
    db.add_clip_mark(edit, vod, "highlight", 12.0, None).unwrap();
    db.add_clip_mark(edit, vod, "cut", 13.0, None).unwrap();
    assert!(current(), "a highlight and a cut leave the file as it is");
    let chapter = db.add_clip_mark(edit, vod, "chapter", 14.0, Some("Start")).unwrap();
    assert!(!current(), "a chapter is in the file");
    assert_eq!(db.file_chapters(edit, &db.edit_pieces(edit).unwrap()).unwrap(), [(4.0, "Start".to_string())]);
    db.delete_mark(chapter).unwrap();
    assert!(current(), "and back as it was");
    db.remove_edit_stretch(edit, 1.0, 2.0).unwrap();
    assert!(!current(), "what plays changed");
}

/// A rendered edit is of the edit as it is only if it was made as clips
/// are made now (the encoding the caller gives, Settings'); one made
/// before that was recorded, as a file of its kind was made then (edits.rs
/// `Db::edit`: "Settings › Encoding changed since, it's made again, as a
/// clip is").
#[test]
fn an_edit_made_otherwise_than_clips_are_made_now_is_out_of_date() {
    use crate::settings::{ClipCodec, ClipQuality};
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (0.0, 10.0), "a", None).unwrap();
    rendered(&db, edit, None);
    db.set_made_now(edit).unwrap();
    let current = |codec, quality| db.edit(edit, ClipEncoding { codec, quality }).unwrap().unwrap().current;
    use ClipCodec::{H264, H265, Prores};
    use ClipQuality::{High, Standard};
    assert!(current(H264, Standard), "an MP4 made before that was recorded is H.264, as clips are made at first");
    assert!(!current(H265, Standard), "clips are made as H.265 now");
    let h265 = ClipEncoding { codec: H265, quality: Standard };
    assert!(!db.edits(h265).unwrap()[0].current, "listed too");
    db.set_made_as(edit, Some(h265)).unwrap();
    assert!(current(H265, Standard));
    assert_eq!(db.edit(edit, h265).unwrap().unwrap().encoding, Some(h265));
    assert!(!current(H265, High), "a change of quality too");
    assert!(!current(Prores, Standard), "and of format");
}

/// A clip of a clip bar cut straight from its VOD has no chapters in its
/// file, so only what plays makes it out of date; split in two, it's
/// rendered as an edit is, chapters and all (edits.rs `Db::made_of`: "A clip
/// bar's clip cut straight from its VOD (`cut_whole`) has neither name nor
/// chapters in its file: only what plays counts").
#[test]
fn a_clip_cut_from_its_vod_is_out_of_date_only_when_what_plays_changes() {
    let db = library();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let clip = db.add_clip(vod, 10.0, 20.0, "c", true).unwrap().id;
    rendered(&db, clip, None);
    db.set_made_now(clip).unwrap();
    let current = || db.edit(clip, at_first()).unwrap().unwrap().current;
    assert!(current());
    db.add_clip_mark(clip, vod, "chapter", 14.0, Some("Start")).unwrap();
    db.add_clip_mark(clip, vod, "highlight", 15.0, None).unwrap();
    assert!(current(), "a cut file has no chapters");
    // In two pieces it's rendered as an edit, with its chapters.
    let piece = db.piece_rows(clip).unwrap()[0].id;
    assert!(db.split_piece(piece, 15.0).unwrap());
    assert!(!current(), "rendered now");
    db.set_made_now(clip).unwrap();
    assert!(current());
    db.add_clip_mark(clip, vod, "chapter", 17.0, Some("Later")).unwrap();
    assert!(!current(), "a chapter is in a rendered file");
}

/// An edit renders from the footage of its pieces, and waits for any not
/// here yet; a hole's piece renders what's kept of it, and black and
/// silence for the rest (CHANGELOG.md, 0.5.0: "a piece whose video file you
/// deleted outside VODscribe ... plays and renders as black and silence,
/// rather than stopping the edit from rendering"; edits.rs `render_pieces`).
#[test]
fn renders_need_the_footage() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    let none = |_: i64, _: f64, _: f64| vec![];
    let err = render_pieces(&db.edit_pieces(edit).unwrap(), none).unwrap_err();
    assert!(err.to_string().contains("isn't here yet"), "{err}");
    // 12-13 s taken out: two pieces.
    db.remove_edit_stretch(edit, 2.0, 3.0).unwrap();
    // Footage of 5 s to 30 s of the VOD: its time 0 is the VOD's 5 s.
    let file = |path: &str, offset: f64| Source { path: path.into(), offset, section: None };
    let found = |_: i64, a: f64, b: f64| vec![(a, b, file("/f/x.mp4", 5.0))];
    let pieces = render_pieces(&db.edit_pieces(edit).unwrap(), found).unwrap();
    assert_eq!(
        pieces,
        [
            Piece { source: Some("/f/x.mp4".into()), start: 5.0, end: 7.0 },
            Piece { source: Some("/f/x.mp4".into()), start: 8.0, end: 15.0 },
        ]
    );
    // Two sections, and nothing from 17 s: a hole's piece plays what's
    // kept, black and silence for the rest; any other waits for it.
    let partly = |_: i64, a: f64, b: f64| {
        vec![(a, 15.0_f64.min(b), file("/f/s1.mkv", 0.0)), (15.0_f64.max(a), 17.0_f64.min(b), file("/f/s2.mkv", 14.0))]
    };
    assert!(render_pieces(&db.edit_pieces(edit).unwrap(), partly).is_err());
    let mut gone = db.edit_pieces(edit).unwrap();
    gone.iter_mut().for_each(|p| p.hole = true);
    assert_eq!(
        render_pieces(&gone, partly).unwrap(),
        [
            Piece { source: Some("/f/s1.mkv".into()), start: 10.0, end: 12.0 },
            Piece { source: Some("/f/s1.mkv".into()), start: 13.0, end: 15.0 },
            Piece { source: Some("/f/s2.mkv".into()), start: 1.0, end: 3.0 },
            Piece { source: None, start: 0.0, end: 3.0 },
        ]
    );
    assert!(covers(&partly(1, 10.0, 17.0), 10.0, 17.0) && !covers(&partly(1, 10.0, 20.0), 10.0, 20.0));
}

/// A piece trimmed by a frame makes its edit's file out of date: what plays
/// is kept to the microsecond (edits.rs `Db::made_of`: "what plays (each
/// VOD's stretches, in order, to the microsecond)").
#[test]
fn a_piece_trimmed_by_a_frame_makes_the_file_out_of_date() {
    let (db, vod, edit) = with_edit();
    db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
    rendered(&db, edit, None);
    db.set_made_now(edit).unwrap();
    assert!(db.edit(edit, at_first()).unwrap().unwrap().current);
    db.set_piece_range(db.piece_rows(edit).unwrap()[0].id, 10.0, 20.0 - 1.0 / 30.0).unwrap();
    assert!(!db.edit(edit, at_first()).unwrap().unwrap().current);
}
