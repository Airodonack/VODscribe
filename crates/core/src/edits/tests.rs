//! The specs of `edits.rs`: the helpers they share, and the files in `tests/`
//! that hold them. Other modules' tests take a VOD's details (`info`) and how
//! clips are made at first (`at_first`) from here.

use super::*;
use crate::fetch::VodInfo;
use std::path::Path;

mod document;
mod render;

/// How clips are made at first (H.264 Standard), as the tests' edits are.
pub(crate) fn at_first() -> ClipEncoding {
    ClipEncoding::default()
}

/// The details of YouTube video `id`, ten minutes long, titled "VOD `id`".
pub(crate) fn info(id: &str) -> VodInfo {
    VodInfo {
        platform: "youtube".into(),
        platform_id: id.into(),
        url: format!("https://youtu.be/{id}"),
        title: format!("VOD {id}"),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(600.0),
        thumbnail: None,
        published: None,
        started_at: None,
    }
}

/// A library of the test's own, in memory, with every migration.
fn library() -> Db {
    Db::open(Path::new(":memory:")).unwrap()
}

/// A library with VOD "x" (ten minutes) and an empty edit "e" in it: the
/// library, the VOD's id and the edit's.
fn with_edit() -> (Db, i64, i64) {
    let db = library();
    let vod = db.add_vod(&info("x")).unwrap().0.id;
    let edit = db.add_edit("e").unwrap().id;
    (db, vod, edit)
}

/// What clip `id`'s pieces are, in its order: each one's stretch of its VOD.
fn stretches(db: &Db, id: i64) -> Vec<(f64, f64)> {
    db.edit_pieces(id).unwrap().iter().map(|p| (p.start, p.end)).collect()
}

/// Whether `got` is `want`, each time to the microsecond (a time kept in
/// flicks comes back as near as that).
fn same_times(got: &[(f64, f64)], want: &[(f64, f64)]) -> bool {
    let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
    got.len() == want.len() && got.iter().zip(want).all(|(g, w)| near(g.0, w.0) && near(g.1, w.1))
}

/// Piece `id` of an edit: `start`..`end` of VOD `vod_id`, in no group, for
/// the specs of the functions that take pieces as the app reads them.
fn piece(id: i64, vod_id: i64, start: f64, end: f64) -> EditPiece {
    EditPiece {
        id,
        edit_id: 1,
        vod_id,
        start,
        end,
        title: String::new(),
        vod_title: String::new(),
        hole: false,
        group: None,
    }
}
