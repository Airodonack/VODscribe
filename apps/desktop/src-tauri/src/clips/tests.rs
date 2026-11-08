//! The specs of clips and their commands. Each test is named for what it
//! says the app does and cites where that's said.

use super::*;

fn info() -> vodscribe_core::fetch::VodInfo {
    vodscribe_core::fetch::VodInfo {
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
    }
}

fn library() -> (Db, i64) {
    let db = Db::open(Path::new(":memory:")).unwrap();
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

mod drag;
mod encoding;
mod files;
mod handed_out;
mod making;
mod marks;
mod queue;
