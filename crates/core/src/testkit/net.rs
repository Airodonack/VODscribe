//! The network as a test records it: an answer for each link (`Net`),
//! given by `net::get` and `net::get_bytes` (thumbnails, emotes, Kick's
//! playlists, yt-dlp's checksums) in place of a request. Links are matched
//! by their start, so one answer can stand for a host. A link no test has
//! answered goes to the network as before.
//!
//! Requests made through `net::agent` directly (Kick's API, chat replays,
//! the emote services, the annotations host) aren't answered here.

use std::sync::{Arc, Mutex};

static ANSWERS: Mutex<Vec<Recorded>> = Mutex::new(Vec::new());

struct Recorded {
    /// The links it answers: those starting with this.
    prefix: String,
    answer: Answer,
    /// The `Net` it's part of, so dropping that takes back only its own.
    owner: usize,
}

#[derive(Clone)]
enum Answer {
    Body(Arc<Vec<u8>>),
    Status(u16),
}

/// Answers recorded for links, until it's dropped. Prefixes are best made
/// unique to a test (a host of its own), since tests run at once.
pub struct Net {
    owner: usize,
}

impl Net {
    pub fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
        Net { owner: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) }
    }

    fn record(&self, prefix: &str, answer: Answer) -> &Self {
        let mut answers = ANSWERS.lock().unwrap();
        // The longest prefix wins, so a link answered alone beats its host.
        answers.push(Recorded { prefix: prefix.into(), answer, owner: self.owner });
        answers.sort_by_key(|r| std::cmp::Reverse(r.prefix.len()));
        self
    }

    /// Links starting with `prefix` answer `body`.
    pub fn serves(&self, prefix: &str, body: impl Into<Vec<u8>>) -> &Self {
        self.record(prefix, Answer::Body(Arc::new(body.into())))
    }

    /// Links starting with `prefix` answer HTTP `status` (404, 403, 500…)
    /// with no body, as `ureq` reports it.
    pub fn answers(&self, prefix: &str, status: u16) -> &Self {
        self.record(prefix, Answer::Status(status))
    }
}

impl Default for Net {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Net {
    fn drop(&mut self) {
        ANSWERS.lock().unwrap().retain(|r| r.owner != self.owner);
    }
}

/// The recorded answer for `url`, if a test gave one (`net::get`).
pub(crate) fn answer(url: &str) -> Option<Result<Vec<u8>, anyhow::Error>> {
    let answer = ANSWERS.lock().unwrap().iter().find(|r| url.starts_with(&r.prefix))?.answer.clone();
    Some(match answer {
        Answer::Body(body) => Ok(body.to_vec()),
        Answer::Status(code) => Err(ureq::Error::StatusCode(code).into()),
    })
}
