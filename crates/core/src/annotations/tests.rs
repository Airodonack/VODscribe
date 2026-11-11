//! The specs of `annotations.rs`: the helpers they share, and the files in `tests/` that hold the rest.

use super::*;
use crate::chat::{ChatPart, Kind};
use crate::db::ChatStatus;
use crate::stretches::Transcribed;

mod format;
mod import;

pub(crate) fn file(platform: &str, id: &str) -> Annotations {
    let json = serde_json::json!({
        "format": FORMAT,
        "version": 1,
        "source": { "platform": platform, "id": id, "channel": "Marlowe", "title": "A stream", "duration": 60.0,
                    "recorded_at": "2026-10-01T18:00:00Z" },
        "produced": { "at": "2026-10-02T00:00:00Z", "build": "b1" },
        "transcript": { "corrected": 0, "segments": [
            { "id": 0, "start": 1.0, "end": 3.0, "text": "hello there", "speaker": "Marlowe",
              "words": [{ "text": "hello", "start": 1.0, "end": 1.5 }, { "text": "there", "start": 1.6, "end": 3.0 }] },
            { "id": 1, "start": 10.0, "end": 12.0, "text": "general kenobi", "words": [] }
        ]},
        "chapters": [{ "start": 0.0, "title": "Start", "icon": " 🎮" }, { "start": 20.0, "title": "Middle", "icon": "a game" }],
        "highlights": [{ "start": 5.0, "end": 9.0, "title": "Big laugh", "kind": "funny", "why": "everyone loses it",
                          "cast": ["Marlowe", " ", "Ada "] }],
        "cast": [{ "name": "Marlowe", "aliases": ["Mar"], "role": "streamer" }],
        "activity": [{ "start": 0.0, "end": 30.0, "kind": "just chatting" }, { "start": 30.0, "end": 60.0, "kind": "game", "name": "Chess" }]
    });
    from_value(json).unwrap()
}

fn message(time: f64, text: &str) -> ChatMessage {
    ChatMessage {
        time,
        author: "Ada".into(),
        color: None,
        parts: vec![ChatPart::Text { text: text.into() }],
        kind: Kind::Message,
        amount: None,
        event_time: None,
        at: None,
    }
}

/// `file("twitch", "v123")` as JSON, with `key` (a path of fields) set to `value`.
fn file_with(path: &[&str], value: serde_json::Value) -> serde_json::Value {
    let mut json = serde_json::to_value(file("twitch", "v123")).unwrap();
    let (last, parents) = path.split_last().unwrap();
    let mut at = &mut json;
    for key in parents {
        at = match key.parse::<usize>() {
            Ok(i) => &mut at[i],
            Err(_) => &mut at[*key],
        };
    }
    at[*last] = value;
    json
}

/// A list of `n` of `item`.
fn many(n: usize, item: serde_json::Value) -> serde_json::Value {
    serde_json::Value::Array(vec![item; n])
}
