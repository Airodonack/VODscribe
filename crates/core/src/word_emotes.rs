//! Emotes that chat writes as plain words: a channel's emotes on 7TV,
//! BetterTTV and FrankerFaceZ (browser extensions most viewers of big channels
//! use), and each provider's global ones. The platforms' own emotes come with
//! the chat (`chat::ChatPart::Emote`); these are matched by word when chat is
//! shown, so chat fetched before a channel added an emote shows it too.
//!
//! Each provider is an entry in `PROVIDERS`: how it finds a channel's emotes
//! and its global ones. None is contacted until the user turns it on in
//! Settings (`settings::EmoteProviders`), which also orders them. Each list is
//! kept for a day, and an older copy is used when a provider can't be reached.
//! Images come from the providers' CDNs (`emotes::HOSTS`).

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::fetch::VodInfo;
use crate::net;

/// An emote chat writes as a word.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WordEmote {
    /// The word, matched exactly (emote names are case-sensitive).
    pub name: String,
    /// Its image, on the provider's CDN.
    pub url: String,
    /// Drawn over the emote before it (7TV's and FFZ's "zero-width" emotes,
    /// such as a hat or a spin), not beside it.
    #[serde(default)]
    pub overlay: bool,
}

/// A channel, by its id on its platform, which the providers look it up by.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Channel {
    /// "twitch" or "kick".
    pub platform: String,
    pub id: String,
}

struct Provider {
    /// What settings call it (`PROVIDER_IDS`).
    id: &'static str,
    /// A channel's emotes; none if it has no account there.
    channel: fn(&Channel) -> Result<Vec<WordEmote>>,
    /// The emotes everyone on `platform` has.
    global: fn(&str) -> Result<Vec<WordEmote>>,
}

const PROVIDERS: &[Provider] = &[
    Provider { id: "7tv", channel: seven_tv_channel, global: seven_tv_global },
    Provider { id: "bttv", channel: bttv_channel, global: bttv_global },
    Provider { id: "ffz", channel: ffz_channel, global: ffz_global },
];

/// The providers settings can turn on, in their starting order.
pub const PROVIDER_IDS: &[&str] = &["7tv", "bttv", "ffz"];

/// How long a list is used before it's fetched again.
const FRESH_SECS: u64 = 24 * 60 * 60;
/// A busy channel's 7TV account, with its emote set, is a few MB.
const MAX_BYTES: u64 = 16 << 20;

/// The emotes VOD `info`'s chat writes as words, from the providers `enabled`
/// (ids, in priority order), using the copies in `dir` while they're fresh.
/// The channel's own emotes come first, each provider's in that order, then
/// the global ones; the first of each name is kept. None for a platform the
/// providers don't cover (YouTube, files), and nothing is fetched when none
/// is enabled. A provider that can't be reached is left out, with its older
/// copy used if there is one.
pub fn for_vod(info: &VodInfo, dir: &Path, enabled: &[&str]) -> Result<Vec<WordEmote>> {
    let Some(key) = cache_key(info) else { return Ok(Vec::new()) };
    // One at a time: lookups made together (a provider toggled quickly, two
    // views) would fetch the same lists twice and write the same files at once.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let providers: Vec<&Provider> = enabled.iter().filter_map(|id| PROVIDERS.iter().find(|p| p.id == *id)).collect();
    if providers.is_empty() {
        return Ok(Vec::new());
    }
    fs::create_dir_all(dir)?;
    // Looked up once, only if a channel list needs fetching, and kept: ids don't change.
    let channel_path = dir.join(format!("{key}.channel.json"));
    let mut channel: Option<Channel> = fs::read(&channel_path).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let mut own = Vec::new();
    let mut global = Vec::new();
    for p in providers {
        let path = dir.join(format!("{key}.{}.json", p.id));
        own.extend(match read_fresh(&path) {
            Some(list) => list,
            None => {
                let fetched = match &channel {
                    Some(c) => (p.channel)(c),
                    None => channel_of(info).and_then(|c| {
                        // Not kept is only slower next time, not a reason to go without.
                        if let Err(e) =
                            serde_json::to_vec(&c).map_err(Into::into).and_then(|b| write_atomic(&channel_path, &b))
                        {
                            eprintln!("{}: {e:#}", channel_path.display());
                        }
                        (p.channel)(channel.insert(c))
                    }),
                };
                settle(&path, fetched)
            }
        });
        let path = dir.join(format!("global-{}.{}.json", info.platform, p.id));
        global.extend(match read_fresh(&path) {
            Some(list) => list,
            None => settle(&path, (p.global)(&info.platform)),
        });
    }
    let mut seen = std::collections::HashSet::new();
    Ok(own.into_iter().chain(global).filter(|e| seen.insert(e.name.clone())).collect())
}

#[derive(Serialize, Deserialize)]
struct Cached {
    /// Unix seconds.
    fetched_at: u64,
    emotes: Vec<WordEmote>,
}

fn read_cached(path: &Path) -> Option<Cached> {
    fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

/// The list at `path` if it's fresh.
fn read_fresh(path: &Path) -> Option<Vec<WordEmote>> {
    read_cached(path).filter(|c| now().saturating_sub(c.fetched_at) < FRESH_SECS).map(|c| c.emotes)
}

/// A list just fetched, kept at `path`; or, if fetching failed, the older
/// copy there, or none.
fn settle(path: &Path, fetched: Result<Vec<WordEmote>>) -> Vec<WordEmote> {
    match fetched {
        Ok(emotes) => {
            let cached = Cached { fetched_at: now(), emotes };
            if let Err(e) = serde_json::to_vec(&cached).map_err(Into::into).and_then(|b| write_atomic(path, &b)) {
                eprintln!("{}: {e:#}", path.display());
            }
            cached.emotes
        }
        Err(e) => {
            eprintln!("{}: {e:#}; using the copy from before, if any", path.display());
            read_cached(path).map(|c| c.emotes).unwrap_or_default()
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let part = path.with_extension("part");
    fs::write(&part, bytes)?;
    fs::rename(&part, path)?;
    Ok(())
}

/// A file name for VOD `info`'s channel: its platform and name, which are
/// known without asking anyone.
fn cache_key(info: &VodInfo) -> Option<String> {
    let name = match info.platform.as_str() {
        "twitch" => info.channel.clone().unwrap_or_default(),
        "kick" => kick_slug(&info.url)?.to_string(),
        _ => return None,
    };
    let safe: String = name.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    // A Twitch name with no letters to go by (in another script, say): the VOD's own.
    let safe = match safe.is_empty() {
        false => safe,
        true if info.platform == "twitch" && info.platform_id.chars().all(|c| c.is_ascii_alphanumeric()) => {
            format!("video{}", info.platform_id)
        }
        true => return None,
    };
    Some(format!("{}-{safe}", info.platform))
}

/// VOD `info`'s channel id: Twitch's from the video, Kick's user id (which
/// 7TV goes by, rather than the channel id its chat is under) from its page name.
fn channel_of(info: &VodInfo) -> Result<Channel> {
    let id = match info.platform.as_str() {
        "twitch" => {
            let video = info.platform_id.trim_start_matches('v');
            let body =
                json!({ "query": "query($id: ID!) { video(id: $id) { owner { id } } }", "variables": { "id": video } });
            let resp = net::agent()
                .post(crate::chat::TWITCH_GQL)
                .header("Client-ID", crate::chat::TWITCH_CLIENT_ID)
                .header("Content-Type", "application/json")
                .send(body.to_string())
                .context("contact Twitch")?;
            let json = read_json(resp)?;
            json.pointer("/data/video/owner/id")
                .and_then(Value::as_str)
                .context("Twitch didn't say whose VOD this is.")?
                .to_string()
        }
        "kick" => {
            let slug = kick_slug(&info.url).context("not a Kick VOD link")?;
            let json = get_json(&format!("https://kick.com/api/v2/channels/{slug}"))?
                .context("Kick doesn't know this channel.")?;
            json.get("user_id").and_then(Value::as_u64).context("Kick didn't say whose channel this is.")?.to_string()
        }
        other => bail!("no emote providers for {other}"),
    };
    Ok(Channel { platform: info.platform.clone(), id })
}

/// The channel's page name in a Kick VOD link: kick.com/<slug>/videos/<id>.
fn kick_slug(url: &str) -> Option<&str> {
    let path = url.split("://").nth(1)?.split_once('/')?.1;
    let slug = path.split('/').next()?;
    (!slug.is_empty() && slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')).then_some(slug)
}

fn read_json(mut resp: ureq::http::Response<ureq::Body>) -> Result<Value> {
    let text = resp.body_mut().with_config().limit(MAX_BYTES).read_to_string()?;
    Ok(serde_json::from_str(&text)?)
}

/// The JSON at `url`; None if it isn't there (a channel with no account).
fn get_json(url: &str) -> Result<Option<Value>> {
    match net::get_bytes(url, MAX_BYTES, &AtomicBool::new(false)) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).with_context(|| format!("unreadable {url}"))?)),
        Err(e) if net::is_client_error(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// An image URL as https, whether given with a scheme or as `//host/path`.
fn https(url: &str) -> Option<String> {
    if url.starts_with("https://") {
        Some(url.to_string())
    } else {
        url.strip_prefix("//").map(|rest| format!("https://{rest}"))
    }
}

// 7TV: a channel's account (by its Twitch or Kick id) has its active emote set.
// An emote's image is its host's URL plus a file name; flag 1 is zero-width.

fn seven_tv_channel(channel: &Channel) -> Result<Vec<WordEmote>> {
    let url = format!("https://7tv.io/v3/users/{}/{}", channel.platform, channel.id);
    Ok(get_json(&url)?.map(|j| seven_tv_emotes(j.pointer("/emote_set/emotes"))).unwrap_or_default())
}

fn seven_tv_global(_platform: &str) -> Result<Vec<WordEmote>> {
    Ok(get_json("https://7tv.io/v3/emote-sets/global")?.map(|j| seven_tv_emotes(j.get("emotes"))).unwrap_or_default())
}

fn seven_tv_emotes(list: Option<&Value>) -> Vec<WordEmote> {
    let Some(list) = list.and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .filter_map(|e| {
            let name = e.get("name")?.as_str()?.to_string();
            let host = https(e.pointer("/data/host/url")?.as_str()?)?;
            let overlay = e.get("flags").and_then(Value::as_u64).is_some_and(|f| f & 1 != 0);
            Some(WordEmote { name, url: format!("{host}/1x.webp"), overlay })
        })
        .collect()
}

// BetterTTV: Twitch only. A channel's own and shared emotes; modifiers (such
// as "w!", which widens the next emote) aren't emotes to show.

fn bttv_channel(channel: &Channel) -> Result<Vec<WordEmote>> {
    if channel.platform != "twitch" {
        return Ok(Vec::new());
    }
    let Some(j) = get_json(&format!("https://api.betterttv.net/3/cached/users/twitch/{}", channel.id))? else {
        return Ok(Vec::new());
    };
    Ok([j.get("channelEmotes"), j.get("sharedEmotes")].into_iter().flat_map(bttv_emotes).collect())
}

fn bttv_global(platform: &str) -> Result<Vec<WordEmote>> {
    if platform != "twitch" {
        return Ok(Vec::new());
    }
    Ok(bttv_emotes(get_json("https://api.betterttv.net/3/cached/emotes/global")?.as_ref()))
}

fn bttv_emotes(list: Option<&Value>) -> Vec<WordEmote> {
    let Some(list) = list.and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .filter(|e| !e.get("modifier").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|e| {
            let id = e.get("id")?.as_str()?;
            id.chars().all(|c| c.is_ascii_alphanumeric()).then_some(())?;
            Some(WordEmote {
                name: e.get("code")?.as_str()?.to_string(),
                url: format!("https://cdn.betterttv.net/emote/{id}/1x"),
                overlay: false,
            })
        })
        .collect()
}

// FrankerFaceZ: Twitch only. A room (channel) and the global sets each list
// emoticons with images by scale; modifier emotes draw over the one before.

fn ffz_channel(channel: &Channel) -> Result<Vec<WordEmote>> {
    if channel.platform != "twitch" {
        return Ok(Vec::new());
    }
    let Some(j) = get_json(&format!("https://api.frankerfacez.com/v1/room/id/{}", channel.id))? else {
        return Ok(Vec::new());
    };
    Ok(ffz_sets(&j, None))
}

fn ffz_global(platform: &str) -> Result<Vec<WordEmote>> {
    if platform != "twitch" {
        return Ok(Vec::new());
    }
    let Some(j) = get_json("https://api.frankerfacez.com/v1/set/global")? else { return Ok(Vec::new()) };
    // Only the default sets are everyone's; the others belong to some users.
    let defaults: Vec<String> = j
        .get("default_sets")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_u64().map(|n| n.to_string())).collect())
        .unwrap_or_default();
    Ok(ffz_sets(&j, Some(&defaults)))
}

/// The emoticons of `j`'s sets (only `only`'s, if given).
fn ffz_sets(j: &Value, only: Option<&[String]>) -> Vec<WordEmote> {
    let Some(sets) = j.get("sets").and_then(Value::as_object) else { return Vec::new() };
    sets.iter()
        .filter(|(id, _)| only.is_none_or(|o| o.contains(id)))
        .filter_map(|(_, set)| set.get("emoticons")?.as_array())
        .flatten()
        .filter_map(|e| {
            Some(WordEmote {
                name: e.get("name")?.as_str()?.to_string(),
                url: https(e.pointer("/urls/1")?.as_str()?)?,
                overlay: e.get("modifier").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect()
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_each_providers_emotes() {
        let seven = json!({ "emote_set": { "emotes": [
            { "name": "GAMBA", "flags": 0, "data": { "host": { "url": "//cdn.7tv.app/emote/01G3" } } },
            { "name": "PETPET", "flags": 1, "data": { "host": { "url": "//cdn.7tv.app/emote/01F6" } } },
            { "name": "broken" }
        ] } });
        assert_eq!(
            seven_tv_emotes(seven.pointer("/emote_set/emotes")),
            [
                WordEmote {
                    name: "GAMBA".into(),
                    url: "https://cdn.7tv.app/emote/01G3/1x.webp".into(),
                    overlay: false
                },
                WordEmote {
                    name: "PETPET".into(),
                    url: "https://cdn.7tv.app/emote/01F6/1x.webp".into(),
                    overlay: true
                },
            ]
        );
        let bttv = json!([
            { "id": "5f1b0186cf6d2144653d2970", "code": "catJAM" },
            { "id": "65cbe7dbaed093b2eaf87c65", "code": "p!", "modifier": true },
            { "id": "../../x", "code": "sneaky" }
        ]);
        assert_eq!(
            bttv_emotes(Some(&bttv)),
            [WordEmote {
                name: "catJAM".into(),
                url: "https://cdn.betterttv.net/emote/5f1b0186cf6d2144653d2970/1x".into(),
                overlay: false
            }]
        );
        let ffz = json!({ "default_sets": [3], "sets": {
            "3": { "emoticons": [{ "name": "ZrehplaR", "urls": { "1": "https://cdn.frankerfacez.com/emote/9/1" } }] },
            "4": { "emoticons": [{ "name": "OnlySome", "urls": { "1": "//cdn.frankerfacez.com/emote/10/1" } }] }
        } });
        let names = |v: Vec<WordEmote>| v.into_iter().map(|e| e.name).collect::<Vec<_>>();
        assert_eq!(names(ffz_sets(&ffz, Some(&["3".to_string()]))), ["ZrehplaR"]);
        assert_eq!(ffz_sets(&ffz, None).len(), 2);
        assert!(ffz_sets(&ffz, None).iter().all(|e| e.url.starts_with("https://") && crate::emotes::allowed(&e.url)));
    }

    #[test]
    fn channels_are_named_for_their_cache_file() {
        let vod = |platform: &str, url: &str, channel: Option<&str>| VodInfo {
            platform: platform.into(),
            platform_id: "1".into(),
            url: url.into(),
            title: String::new(),
            channel: channel.map(Into::into),
            duration: None,
            thumbnail: None,
            published: None,
        };
        assert_eq!(
            cache_key(&vod("twitch", "https://www.twitch.tv/videos/1", Some("GEEGA"))).as_deref(),
            Some("twitch-geega")
        );
        assert_eq!(
            cache_key(&vod("kick", "https://kick.com/barky/videos/01a0", Some("Barky's stream"))).as_deref(),
            Some("kick-barky")
        );
        assert_eq!(cache_key(&vod("twitch", "u", Some("../.."))).as_deref(), Some("twitch-video1"));
        assert_eq!(cache_key(&vod("kick", "https://kick.com/../videos/x", None)), None);
        assert_eq!(cache_key(&vod("youtube", "https://youtu.be/x", Some("Someone"))), None);
        assert_eq!(kick_slug("https://kick.com/../videos/x"), None);
    }

    #[test]
    fn fresh_copies_are_used_in_priority_order_and_off_means_none() {
        let dir = crate::TempDir::new("word-emotes");
        let emote = |name: &str, id: &str| WordEmote {
            name: name.into(),
            url: format!("https://cdn.7tv.app/emote/{id}/1x.webp"),
            overlay: false,
        };
        let keep = |file: &str, emotes: Vec<WordEmote>| {
            write_atomic(&dir.join(file), &serde_json::to_vec(&Cached { fetched_at: now(), emotes }).unwrap()).unwrap()
        };
        keep("twitch-geega.7tv.json", vec![emote("KEKW", "seven")]);
        keep("twitch-geega.ffz.json", vec![emote("KEKW", "ffz"), emote("OMEGALUL", "ffz")]);
        keep("global-twitch.7tv.json", vec![emote("OMEGALUL", "global"), emote("EZ", "global")]);
        keep("global-twitch.ffz.json", vec![]);
        let info = VodInfo {
            platform: "twitch".into(),
            platform_id: "1".into(),
            url: String::new(),
            title: String::new(),
            channel: Some("GEEGA".into()),
            duration: None,
            thumbnail: None,
            published: None,
        };
        let shown = |enabled: &[&str]| {
            for_vod(&info, &dir, enabled)
                .unwrap()
                .into_iter()
                .map(|e| format!("{} {}", e.name, &e.url["https://cdn.7tv.app/emote/".len()..]))
                .collect::<Vec<_>>()
        };
        assert_eq!(shown(&["7tv", "ffz"]), ["KEKW seven/1x.webp", "OMEGALUL ffz/1x.webp", "EZ global/1x.webp"]);
        assert_eq!(shown(&["ffz", "7tv"]), ["KEKW ffz/1x.webp", "OMEGALUL ffz/1x.webp", "EZ global/1x.webp"]);
        assert!(shown(&[]).is_empty());
        assert!(shown(&["unknown"]).is_empty());
    }

    /// Asks the providers for real: `cargo test -p vodscribe-core -- --ignored word_emotes`.
    #[test]
    #[ignore = "network"]
    fn fetches_real_channels() {
        let vod = |platform: &str, id: &str, url: &str, channel: &str| VodInfo {
            platform: platform.into(),
            platform_id: id.into(),
            url: url.into(),
            title: String::new(),
            channel: Some(channel.into()),
            duration: None,
            thumbnail: None,
            published: None,
        };
        let dir = crate::TempDir::new("word-emotes-live");
        for info in [
            vod("twitch", "2899842479", "https://www.twitch.tv/videos/2899842479", "GEEGA"),
            vod("kick", "01a0b0d6-75c0-75a7-8050-a72ab4dc9e0f", "https://kick.com/barky/videos/01a0b0d6", "Barky"),
        ] {
            let emotes = for_vod(&info, &dir, PROVIDER_IDS).unwrap();
            let overlays = emotes.iter().filter(|e| e.overlay).count();
            eprintln!("{}: {} emotes, {overlays} overlays", info.url, emotes.len());
            assert!(emotes.len() > 20, "{} has only {} emotes", info.url, emotes.len());
            assert!(emotes.iter().all(|e| crate::emotes::allowed(&e.url)));
        }
    }
}
