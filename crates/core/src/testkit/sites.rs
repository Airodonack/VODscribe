//! The sites' own APIs as a test's network answers them (`Net`), for what a
//! test needs of a site beyond yt-dlp's details: what each says, in the
//! shape the site gives it, for one video of the test's own, so tests on
//! the same site's host don't meet.

use serde_json::{Value, json};

use super::net::{Net, Reply, Request};

/// The GraphQL request `request` is, if it's about video `id`: its query.
fn about(request: &Request, id: &str) -> Option<String> {
    let body = request.json()?;
    (body.pointer("/variables/id").and_then(Value::as_str) == Some(id))
        .then(|| body["query"].as_str().unwrap_or_default().to_string())
}

impl Net {
    /// Twitch's GraphQL API has VOD `id`'s chat (its number, without
    /// yt-dlp's "v"): `said`, each `(seconds, who, text)`. A page asked for
    /// has the messages from its offset on and says there are no more; the
    /// channel has no cheermotes but the built-in ones.
    pub fn twitch_chat(&self, id: &str, said: &[(f64, &str, &str)]) -> &Self {
        let id = id.to_string();
        let said: Vec<(f64, String, String)> =
            said.iter().map(|(t, a, s)| (*t, a.to_string(), s.to_string())).collect();
        self.replies(crate::chat::TWITCH_GQL, move |request| {
            let query = about(request, &id)?;
            if query.contains("cheerConfig") {
                return Some(Reply::json(&json!({ "data": { "cheerConfig": { "groups": [] }, "video": null } })));
            }
            if !query.contains("comments(") {
                return None;
            }
            let offset = request.json()?.pointer("/variables/offset").and_then(Value::as_f64).unwrap_or(0.0);
            let edges: Vec<Value> = said
                .iter()
                .enumerate()
                .filter(|(_, (at, _, _))| *at >= offset)
                .map(|(i, (at, who, text))| {
                    json!({ "node": {
                        "id": format!("{id}-{i}"),
                        "contentOffsetSeconds": at,
                        "commenter": { "displayName": who },
                        "message": { "userColor": null, "fragments": [{ "text": text, "emote": null }] },
                    } })
                })
                .collect();
            let page = json!({ "data": { "video": {
                "comments": { "edges": edges, "pageInfo": { "hasNextPage": false } },
            } } });
            Some(Reply::json(&page))
        })
    }
}
