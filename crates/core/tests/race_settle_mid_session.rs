//! A second connection to the library, opened mid-session (a
//! transcription opens its own, worker.rs), moves no file: `library::settle`
//! runs once, as the app starts, so a job never has its file moved after
//! it took the path. A received file left where it arrived (its rename in
//! vods.rs add_local failed) is one settle would move.

use vodscribe_core::Paths;
use vodscribe_core::db::Db;
use vodscribe_core::fetch::VodInfo;

#[test]
fn a_second_connection_moves_no_file_a_job_has() {
    let root = std::env::temp_dir().join(format!("vodscribe-settle-mid-session-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths::new(&root);
    paths.ensure().unwrap();
    std::fs::create_dir_all(paths.received()).unwrap();
    // The app's own connection, as lib.rs opens it at start.
    let app = Db::open_in(&paths).unwrap();
    // A file received and added, its rename to received/<id>.mp4 failed:
    // its row records where it arrived (add_to_library's source path).
    let arrived = paths.received().join("clip from zip.mp4");
    std::fs::write(&arrived, b"media").unwrap();
    let info = VodInfo {
        platform: "local".into(),
        platform_id: arrived.to_string_lossy().into_owned(),
        url: arrived.to_string_lossy().into_owned(),
        title: "clip from zip".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(1.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let (vod, _) = app.add_vod(&info).unwrap();
    app.set_source_path(vod.id, &arrived.to_string_lossy()).unwrap();
    // 1. The transcription resolves its audio (worker.rs transcribe).
    let audio = app.audio_path(vod.id).unwrap().unwrap();
    assert!(std::path::Path::new(&audio).is_file(), "resolved: {audio}");
    // 2. It opens its second connection (worker.rs): nothing moves.
    let _notes = Db::open_in(&paths).unwrap();
    // 3. It reads the audio it resolved.
    let still = std::path::Path::new(&audio).is_file();
    let now = app.audio_path(vod.id).unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&root);
    assert!(still, "the file the job resolved ({audio}) was moved mid-session by settle, to {now}");
}
