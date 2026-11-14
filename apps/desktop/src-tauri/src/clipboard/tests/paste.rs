use vodscribe_core::edits::EditState;
use vodscribe_core::time::Flicks;

use super::*;

fn library() -> (Db, i64) {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let info = vodscribe_core::fetch::VodInfo {
        platform: "youtube".into(),
        platform_id: "x".into(),
        url: "https://youtu.be/x".into(),
        title: "t".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(60.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let vod = db.add_vod(&info).unwrap().0.id;
    (db, vod)
}

fn piece(parent: Option<usize>, vod: i64, start: f64, end: Option<f64>) -> ItemState {
    ItemState {
        parent,
        vod_id: Some(vod),
        start: Some(Flicks::from_seconds(start)),
        end: end.map(Flicks::from_seconds),
        title: String::new(),
        origin: None,
    }
}

fn copy(kind: CopiedKind, items: Vec<ItemState>) -> Copied {
    Copied { kind, state: EditState { title: "c".into(), items, marks: Vec::new() } }
}

/// What the page is told of a copy: its pieces (its groups aren't), the
/// first one's stretch, and how long it plays; a piece with no end runs
/// to its VOD's end, one of a VOD that's gone plays nothing.
#[test]
fn a_copy_is_said_by_its_pieces() {
    let (db, vod) = library();
    let group = ItemState { parent: None, vod_id: None, start: None, end: None, title: "g".into(), origin: None };
    let items = vec![
        group.clone(),
        piece(Some(0), vod, 10.0, Some(18.0)),
        piece(None, vod, 30.0, None),
        piece(None, 9999, 5.0, None),
    ];
    let said_of = said(&db, &copy(CopiedKind::Clip, items)).unwrap();
    assert_eq!((said_of.kind, said_of.title.as_str(), said_of.pieces, said_of.length), ("clip", "c", 3, 38.0));
    let first = said_of.first.unwrap();
    assert_eq!((first.vod_id, first.vod_title.as_str(), first.start, first.end), (vod, "t", 10.0, 18.0));

    // Its first piece with no end: that stretch runs to the VOD's end.
    let stretch = copy(CopiedKind::Stretch, vec![piece(None, vod, 5.0, None)]);
    let said_of = said(&db, &stretch).unwrap();
    assert_eq!((said_of.kind, said_of.pieces, said_of.length), ("stretch", 1, 55.0));
    let first = said_of.first.unwrap();
    assert_eq!((first.start, first.end), (5.0, 60.0));

    // A group alone: no stretch, nothing played.
    let said_of = said(&db, &copy(CopiedKind::Clip, vec![group])).unwrap();
    assert!(said_of.first.is_none());
    assert_eq!((said_of.pieces, said_of.length), (0, 0.0));
}
