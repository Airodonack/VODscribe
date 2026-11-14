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

/// Clip `id` once made, and whether ffmpeg wrote a file (a cut or a render,
/// told by its progress) since its `from`th run: made again after a change
/// then, not left as it was.
fn made_again_since(app: &crate::testkit::TestApp, id: i64, from: usize) -> (Clip, bool) {
    let clip = app.made(id);
    let written = app.ffmpeg.runs().iter().skip(from).any(|run| run.iter().any(|a| a == "-progress"));
    (clip, written)
}

mod codecs;
mod drag;
mod encoding;
mod files;
mod handed_out;
mod making;
mod marks;
mod queue;
