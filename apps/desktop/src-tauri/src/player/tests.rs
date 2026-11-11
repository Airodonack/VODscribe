use super::*;
use vodscribe_core::fetch::VodInfo;

/// The tests' account of the player: none open, then one, as JSON the dump reads.
#[test]
fn the_player_seen_says_whether_one_is_open_and_what_its_threads_are_at() {
    let native = Native::default();
    let none = serde_json::to_value(native.seen()).unwrap();
    assert_eq!(none["open"], false);
    assert!(none["player"].is_null());
    let player = Player::open(std::path::Path::new("ffmpeg"), Timeline::default()).unwrap();
    *native.open.lock().unwrap() = Some(Open { player, doc: Doc::Vod(7), embed: None });
    let seen = serde_json::to_value(native.seen()).unwrap();
    assert_eq!(seen["open"], true);
    assert_eq!(seen["doc"], serde_json::json!({ "kind": "vod", "id": 7 }));
    assert_eq!(seen["player"]["thread_alive"], true);
    assert_eq!(seen["player"]["decoding"], false);
    assert_eq!(seen["player"]["state"]["playing"], false);
    // Held by a stuck thread: said so, not waited on.
    let held = native.open.lock().unwrap();
    let begun = std::time::Instant::now();
    let stuck = std::thread::scope(|s| s.spawn(|| serde_json::to_value(native.seen()).unwrap()).join().unwrap());
    assert!(stuck["open"].is_null());
    assert!(begun.elapsed() < std::time::Duration::from_secs(2));
    drop(held);
}

/// A VOD from a site without its own player, its video downloading
/// (videos.rs `stream_then_keep`), opened while the download ends: the
/// open reads the library before the video is recorded, and the
/// recording's `refile` comes before the open is in place.
#[test]
fn a_vod_opened_as_its_video_is_kept_plays_the_video_kept() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let info = VodInfo {
        platform: "vimeo".into(),
        platform_id: "x".into(),
        url: "https://vimeo.com/x".into(),
        title: "t".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(60.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let id = db.add_vod(&info).unwrap().0.id;
    let db = Mutex::new(db);
    let native = Native::default();
    // player_open's `streamed`: what's in of the section (none, here), waiting for the rest.
    assert!(!db.lock().unwrap().vod(id).unwrap().unwrap().media, "no video kept yet");
    let player = Player::open(std::path::Path::new("ffmpeg"), Timeline::covering(60.0, Vec::new())).unwrap();
    player.set_waits_for_footage(true);
    // Meanwhile (the probe), the download ends: recorded, then refiled, with no player open yet.
    let kept = db.lock().unwrap().files().source_video(id, "mp4");
    db.lock().unwrap().set_media_path(id, Some(&kept.to_string_lossy())).unwrap();
    refile_open(&native, id, &kept, 60.0);
    // The open is in place (open_with), and catches up.
    *native.open.lock().unwrap() = Some(Open { player, doc: Doc::Vod(id), embed: None });
    catch_up(&native, &db, id);
    let open = native.open.lock().unwrap();
    let player = &open.as_ref().unwrap().player;
    assert_eq!(player.timeline(), Timeline::file(&kept, 60.0), "the video kept, not the section it replaced");
    assert!(!player.waits_for_footage());
}

/// An edit playing holds back the storyboards of other VODs only: those
/// of its own pieces' VODs are made meanwhile, as the edit's timeline
/// shows them.
#[test]
fn an_edit_playing_holds_back_only_other_vods_work() {
    let db = Db::open(std::path::Path::new(":memory:")).unwrap();
    let vod = |id: &str| {
        let info = VodInfo {
            platform: "vimeo".into(),
            platform_id: id.into(),
            url: format!("https://vimeo.com/{id}"),
            title: "t".into(),
            channel: None,
            channel_login: None,
            channel_url: None,
            duration: Some(60.0),
            thumbnail: None,
            published: None,
            started_at: None,
        };
        db.add_vod(&info).unwrap().0.id
    };
    let (own, other, third) = (vod("a"), vod("b"), vod("c"));
    let edit = db.add_edit("e").unwrap().id;
    db.add_piece(edit, own, (10.0, 20.0), "a", None).unwrap();
    let native = Native::default();
    let player = Player::open(std::path::Path::new("ffmpeg"), Timeline::covering(60.0, Vec::new())).unwrap();
    player.play();
    assert!(player.playing());
    *native.open.lock().unwrap() = Some(Open { player, doc: Doc::Edit(edit), embed: None });
    let db = Mutex::new(db);
    assert!(!native.playing_other(&db, own), "the edit's own piece's VOD isn't held back");
    assert!(native.playing_other(&db, other), "another VOD's is");
    assert!(native.playing_other(&db, third), "and a third's");
    // A piece of the other added while it plays: its work goes on too.
    db.lock().unwrap().add_piece(edit, other, (0.0, 5.0), "b", None).unwrap();
    assert!(!native.playing_other(&db, other), "a piece added since counts");
    // Paused: nothing waits.
    native.open.lock().unwrap().as_ref().unwrap().player.pause();
    assert!(!native.playing_other(&db, third), "nothing waits while it's paused");
}

#[test]
fn frames_scale_down_by_mixing_or_by_area() {
    // 4 by 2: a white column, then black ones, over two rows.
    let row = [255u8, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let rgb = [row, row].concat();
    // Halved: each pixel the mean of the two under it.
    let mut out = Vec::new();
    scale_rgba(&rgb, 4, 2, 2, 1, &mut out);
    assert_eq!(out, [128, 128, 128, 255, 0, 0, 0, 255]);
    // To one pixel, a quarter: the mean of all four columns, not two of them.
    let mut out = Vec::new();
    scale_rgba(&rgb, 4, 2, 1, 1, &mut out);
    assert_eq!(out, [64, 64, 64, 255]);
    // A thin line in a 12x reduction still counts, as bilinear's taps would miss it.
    let mut wide = vec![0u8; 24 * 3];
    wide[13 * 3..14 * 3].copy_from_slice(&[240, 240, 240]);
    let mut out = Vec::new();
    scale_rgba(&wide, 24, 1, 2, 1, &mut out);
    assert_eq!(out, [0, 0, 0, 255, 20, 20, 20, 255]);
}

/// The loudness set is between silent and full, whatever the page sends
/// (`player_set_volume`: "The loudness, 0 to 1."; plans/sections-074.md
/// "Decided", unstated behaviour 7, owner 2026-11-11: "the volume is clamped
/// to 0..1: spec it"). A site's player can send more: the embed's stand-in
/// for its video (embed_shim.js, `volume`) takes any number it's given.
#[test]
fn the_volume_set_is_between_silent_and_full() {
    let app = crate::testkit::TestApp::new();
    let player = Player::open(std::path::Path::new("ffmpeg"), Timeline::default()).unwrap();
    *app.state().player.open.lock().unwrap() = Some(Open { player, doc: Doc::Vod(1), embed: None });
    let set = |volume: f64| {
        app.invoke("player_set_volume", serde_json::json!({ "volume": volume })).unwrap();
        app.state().player.open.lock().unwrap().as_ref().unwrap().player.volume()
    };
    assert_eq!(set(1.5), 1.0);
    assert_eq!(set(-0.5), 0.0);
    assert_eq!(set(0.25), 0.25);
    assert_eq!(set(100.0), 1.0, "a site's 0 to 100");
}
