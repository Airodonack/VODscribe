//! The specs of word_emotes.rs at its seam, `for_vod`: which emotes a VOD's
//! chat writes as words, from the providers turned on, kept for a day
//! The providers answer on the kit's network
//! (`testkit::net`). Twitch's own lookup of a VOD's channel goes outside it,
//! so a Twitch VOD's channel is given as already looked up.

use super::*;
use crate::testkit::net::Net;
use std::sync::MutexGuard;

/// A day, in seconds.
const DAY: u64 = 24 * 60 * 60;

/// The providers' links on the kit's network, for one test at a time: their
/// global lists are at links every test shares.
static PROVIDERS_ANSWER: Mutex<()> = Mutex::new(());

/// A test's providers: what they answer, and the folder the lists are kept in.
struct Providers {
    net: Net,
    dir: crate::TempDir,
    _turn: MutexGuard<'static, ()>,
}

impl Providers {
    /// Providers that know no one: every link of theirs, and Kick's channel
    /// lookups, answer 404 until the test says otherwise, so nothing reaches
    /// the real services.
    fn new(name: &str) -> Self {
        let turn = PROVIDERS_ANSWER.lock().unwrap_or_else(|e| e.into_inner());
        let net = Net::new();
        for host in
            ["https://7tv.io/", "https://api.betterttv.net/", "https://api.frankerfacez.com/", "https://kick.com/"]
        {
            net.answers(host, 404);
        }
        Providers { net, dir: crate::TempDir::new(name), _turn: turn }
    }

    fn serves(&self, url: &str, body: Value) -> &Self {
        self.net.serves(url, body.to_string());
        self
    }

    /// What `info`'s chat shows as emotes, as "name url" with the CDN's
    /// part of the link left out.
    fn shown(&self, info: &VodInfo, enabled: &[&str]) -> Vec<String> {
        for_vod(info, &self.dir, enabled).unwrap().iter().map(shown).collect()
    }

    /// A list kept at `file`, fetched `age_secs` ago.
    fn kept(&self, file: &str, age_secs: u64, emotes: Vec<WordEmote>) {
        let cached = Cached { fetched_at: now() - age_secs, emotes };
        write_atomic(&self.dir.join(file), &serde_json::to_vec(&cached).unwrap()).unwrap();
    }

    /// The emotes kept at `file`.
    fn kept_in(&self, file: &str) -> Vec<String> {
        read_cached(&self.dir.join(file)).unwrap().emotes.iter().map(shown).collect()
    }
}

fn shown(e: &WordEmote) -> String {
    let link =
        ["https://cdn.7tv.app/emote/", "https://cdn.betterttv.net/emote/", "https://cdn.frankerfacez.com/emote/"]
            .iter()
            .find_map(|cdn| e.url.strip_prefix(cdn))
            .unwrap_or(&e.url);
    format!("{}{} {link}", e.name, if e.overlay { " (over)" } else { "" })
}

fn vod(platform: &str, id: &str, url: &str) -> VodInfo {
    serde_json::from_value(json!({ "platform": platform, "platform_id": id, "url": url, "title": "" })).unwrap()
}

/// Twitch VOD `id`, its channel `channel` already looked up and kept.
fn twitch_vod(providers: &Providers, id: &str, channel: &str) -> VodInfo {
    let kept = json!({ "platform": "twitch", "id": channel }).to_string();
    write_atomic(&providers.dir.join(format!("twitch-video-{id}.channel.json")), kept.as_bytes()).unwrap();
    vod("twitch", id, &format!("https://www.twitch.tv/videos/{id}"))
}

/// Kick VOD `id` on channel page `slug`.
fn kick_vod(id: &str, slug: &str) -> VodInfo {
    vod("kick", id, &format!("https://kick.com/{slug}/videos/{id}"))
}

/// 7TV's answer for an emote, `flags` 1 for zero-width.
fn seven(name: &str, id: &str, flags: u64) -> Value {
    json!({ "name": name, "flags": flags, "data": { "host": { "url": format!("//cdn.7tv.app/emote/{id}") } } })
}

/// A 7TV channel's account with these emotes in its active set.
fn seven_account(emotes: &[Value]) -> Value {
    json!({ "emote_set": { "emotes": emotes } })
}

fn emote(name: &str, id: &str) -> WordEmote {
    WordEmote { name: name.into(), url: format!("https://cdn.7tv.app/emote/{id}/1x.webp"), overlay: false }
}

// Which providers are asked.

/// With no provider turned on, no one is asked anything and nothing is kept
/// (word_emotes.rs's module doc: "None is contacted until the user turns it on in
/// Settings"; `for_vod`: "nothing is fetched when none is enabled"; 0.2.1:
/// "Each is off until you turn it on, since it means contacting that
/// service").
#[test]
fn no_provider_turned_on_asks_no_one() {
    let providers = Providers::new("word-emotes-off");
    let dir = providers.dir.join("emotes");
    for enabled in [&[][..], &["unknown"]] {
        assert!(for_vod(&kick_vod("off", "barky"), &dir, enabled).unwrap().is_empty());
        assert!(for_vod(&vod("twitch", "1", "https://www.twitch.tv/videos/1"), &dir, enabled).unwrap().is_empty());
    }
    assert!(!dir.exists(), "not even the folder is made");
}

/// BetterTTV and FrankerFaceZ don't cover Kick: with only them on, a Kick
/// VOD asks no one, not even Kick whose channel it is (word_emotes.rs
/// `Provider::platforms`: "The platforms whose channels it has emotes for";
/// 0.2.1: "Chat can show 7TV, BetterTTV and FrankerFaceZ emotes for Kick and
/// Twitch VODs").
#[test]
fn no_provider_for_the_platform_asks_no_one() {
    let dir = crate::TempDir::new("word-emotes-none");
    let kick = vod("kick", "01a0", "https://kick.com/barky/videos/01a0");
    assert!(for_vod(&kick, &dir, &["bttv", "ffz"]).unwrap().is_empty());
    assert!(!dir.join("kick-video-01a0.channel.json").exists());
}

/// A VOD from a site the providers don't cover, YouTube or a file, has no
/// word emotes, and no one is asked (word_emotes.rs `for_vod`: "None for a
/// platform the providers don't cover (YouTube, files)").
#[test]
fn a_vod_from_a_site_no_provider_covers_has_none() {
    let providers = Providers::new("word-emotes-youtube");
    for info in [vod("youtube", "noa29qsTc4g", "https://www.youtube.com/watch?v=noa29qsTc4g"), vod("local", "7", "")] {
        assert!(for_vod(&info, &providers.dir, PROVIDER_IDS).unwrap().is_empty(), "{}", info.platform);
    }
    assert!(fs::read_dir(&*providers.dir).unwrap().next().is_none(), "nothing kept");
}

/// Settings can turn on every provider there is, by the id each goes by
/// (word_emotes.rs `PROVIDER_IDS`: "The providers settings can turn on, in
/// their starting order").
#[test]
fn settings_know_every_provider() {
    assert_eq!(PROVIDER_IDS, PROVIDERS.iter().map(|p| p.id).collect::<Vec<_>>());
}

// Which emote wins.

/// The channel's own emotes come first, each provider's in the order
/// Settings puts them, then the global ones, and the first of a name wins
/// (word_emotes.rs `for_vod`: "The channel's own emotes come first, each
/// provider's in that order, then the global ones; the first of each name is
/// kept"; 0.2.1: "drag them to choose which wins when two have an emote of
/// the same name").
#[test]
fn fresh_copies_are_used_in_priority_order() {
    let providers = Providers::new("word-emotes");
    let info = twitch_vod(&providers, "1", "36973271");
    providers.kept("twitch-36973271.7tv.json", 0, vec![emote("KEKW", "seven")]);
    providers.kept("twitch-36973271.ffz.json", 0, vec![emote("KEKW", "ffz"), emote("OMEGALUL", "ffz")]);
    providers.kept("global-twitch.7tv.json", 0, vec![emote("OMEGALUL", "global"), emote("EZ", "global")]);
    providers.kept("global-twitch.ffz.json", 0, vec![]);
    assert_eq!(
        providers.shown(&info, &["7tv", "ffz"]),
        ["KEKW seven/1x.webp", "OMEGALUL ffz/1x.webp", "EZ global/1x.webp"]
    );
    assert_eq!(
        providers.shown(&info, &["ffz", "7tv"]),
        ["KEKW ffz/1x.webp", "OMEGALUL ffz/1x.webp", "EZ global/1x.webp"]
    );
}

// Kept for a day.

/// A list fetched less than a day ago is used as it is, though the provider
/// has changed it since (word_emotes.rs's module doc: "Each list is kept for a day";
/// `for_vod`: "using the copies in `dir` while they're fresh").
#[test]
fn a_list_fetched_within_a_day_is_used_without_asking() {
    let providers = Providers::new("word-emotes-fresh");
    let info = twitch_vod(&providers, "2", "1002");
    providers.kept("twitch-1002.7tv.json", DAY - 60 * 60, vec![emote("KEKW", "kept")]);
    providers.kept("global-twitch.7tv.json", 0, vec![]);
    providers.serves("https://7tv.io/v3/users/twitch/1002", seven_account(&[seven("KEKW", "new", 0)]));
    assert_eq!(providers.shown(&info, &["7tv"]), ["KEKW kept/1x.webp"]);
}

/// A list kept a day is fetched again, and the new one kept (word_emotes.rs's module doc:
/// "Each list is kept for a day"; `FRESH_SECS`: "How long a list is used
/// before it's fetched again").
#[test]
fn a_list_kept_a_day_is_fetched_again() {
    let providers = Providers::new("word-emotes-stale");
    let info = twitch_vod(&providers, "3", "1003");
    providers.kept("twitch-1003.7tv.json", DAY, vec![emote("KEKW", "old")]);
    providers.kept("global-twitch.7tv.json", 0, vec![]);
    providers.serves("https://7tv.io/v3/users/twitch/1003", seven_account(&[seven("PogU", "new", 0)]));
    assert_eq!(providers.shown(&info, &["7tv"]), ["PogU new/1x.webp"]);
    assert_eq!(providers.kept_in("twitch-1003.7tv.json"), ["PogU new/1x.webp"]);
}

// A provider that can't be reached.

/// A provider that can't be reached is left out, the others shown
/// (word_emotes.rs `for_vod`: "A provider that can't be reached is left
/// out").
#[test]
fn a_provider_that_cant_be_reached_is_left_out() {
    let providers = Providers::new("word-emotes-down");
    let info = twitch_vod(&providers, "4", "1004");
    providers.net.answers("https://7tv.io/v3/users/twitch/1004", 500);
    providers.serves(
        "https://api.betterttv.net/3/cached/users/twitch/1004",
        json!({ "channelEmotes": [{ "id": "5f1b0186cf6d2144653d2970", "code": "catJAM" }] }),
    );
    assert_eq!(providers.shown(&info, &["7tv", "bttv"]), ["catJAM 5f1b0186cf6d2144653d2970/1x"]);
}

/// A provider that can't be reached gives the list it gave before, however
/// old (word_emotes.rs's module doc: "an older copy is used when a provider can't be
/// reached"; `for_vod`: "with its older copy used if there is one").
#[test]
fn a_provider_that_cant_be_reached_gives_its_older_list() {
    let providers = Providers::new("word-emotes-down-kept");
    let info = twitch_vod(&providers, "5", "1005");
    providers.kept("twitch-1005.7tv.json", 30 * DAY, vec![emote("KEKW", "old")]);
    providers.kept("global-twitch.7tv.json", 0, vec![]);
    providers.net.answers("https://7tv.io/v3/users/twitch/1005", 503);
    assert_eq!(providers.shown(&info, &["7tv"]), ["KEKW old/1x.webp"]);
}

/// A provider that couldn't be reached isn't asked again straight away: a
/// VOD opened next goes without it, though it's back (0.3.0: "a provider
/// that can't be reached isn't waited on again for a few minutes";
/// word_emotes.rs `RETRY_AFTER`: "offline, each VOD opened would otherwise
/// wait on every provider in turn"). That it's asked again once the few
/// minutes are over waits on a clock `for_vod` reads; until then
/// `a_failed_fetch_waits_before_trying_again` says it of the function inside.
#[test]
fn a_provider_that_couldnt_be_reached_isnt_asked_again_at_once() {
    let providers = Providers::new("word-emotes-retry");
    let info = twitch_vod(&providers, "6", "1006");
    providers.kept("global-twitch.7tv.json", 0, vec![]);
    {
        let down = Net::new();
        down.answers("https://7tv.io/v3/users/twitch/1006", 500);
        assert!(providers.shown(&info, &["7tv"]).is_empty());
    }
    providers.serves("https://7tv.io/v3/users/twitch/1006", seven_account(&[seven("KEKW", "back", 0)]));
    assert!(providers.shown(&info, &["7tv"]).is_empty(), "not asked again so soon");
}

/// Once the wait is over it's asked again, and a success is forgotten
/// (word_emotes.rs `RETRY_AFTER`: "How long after failing a list isn't asked
/// for again"). Said of `fetch_unless_failed`, the time put back by hand,
/// until a clock `for_vod` reads lets it move up.
#[test]
fn a_failed_fetch_waits_before_trying_again() {
    let path = PathBuf::from("/nowhere/a-failed-fetch-test.json");
    let tries = std::cell::Cell::new(0);
    let fail = || -> Result<()> {
        tries.set(tries.get() + 1);
        bail!("offline")
    };
    assert!(fetch_unless_failed(&path, fail).is_err());
    assert!(fetch_unless_failed(&path, fail).is_err());
    assert_eq!(tries.get(), 1, "not asked again so soon");
    FAILED.lock().unwrap().insert(path.clone(), Instant::now() - RETRY_AFTER);
    assert!(fetch_unless_failed(&path, || Ok(())).is_ok());
    assert!(!failed_lately(&path));
}

/// A provider that has no account for the channel isn't one that can't be
/// reached: the channel has none there, and the global ones still show
/// (word_emotes.rs `Provider::channel`: "A channel's emotes; none if it has
/// no account there").
#[test]
fn a_channel_with_no_account_on_a_provider_has_none_there() {
    let providers = Providers::new("word-emotes-no-account");
    let info = twitch_vod(&providers, "7", "1007");
    providers.serves("https://7tv.io/v3/emote-sets/global", json!({ "emotes": [seven("EZ", "global", 0)] }));
    assert_eq!(providers.shown(&info, &["7tv"]), ["EZ global/1x.webp"]);
    assert!(providers.kept_in("twitch-1007.7tv.json").is_empty(), "kept as none, for the day");
}

// Whose VOD it is.

/// A Kick VOD's channel is its streamer's user id, which 7TV goes by, looked
/// up from the page name in its link (word_emotes.rs `channel_of`: "Kick's
/// user id (which 7TV goes by, rather than the channel id its chat is under)
/// from its page name").
#[test]
fn a_kick_vods_channel_is_the_user_id_7tv_goes_by() {
    let providers = Providers::new("word-emotes-kick");
    providers.serves("https://kick.com/api/v2/channels/emotes-test-barky", json!({ "id": 999, "user_id": 123 }));
    providers.serves("https://7tv.io/v3/users/kick/123", seven_account(&[seven("barkyHi", "b1", 0)]));
    providers.serves("https://7tv.io/v3/users/kick/999", seven_account(&[seven("wrong", "w1", 0)]));
    assert_eq!(providers.shown(&kick_vod("01a0b0d6-75c0", "emotes-test-barky"), &["7tv"]), ["barkyHi b1/1x.webp"]);
}

/// Whose VOD it is is looked up once and kept: its lists are found again
/// with the site no longer answering (word_emotes.rs `for_vod`'s comment: "Whose VOD it is,
/// looked up once per VOD and kept (a VOD's channel never changes, where a
/// channel's name can); the lists go by the channel's id").
#[test]
fn whose_vod_it_is_is_looked_up_once_and_kept() {
    let providers = Providers::new("word-emotes-once");
    let info = kick_vod("k2", "emotes-test-once");
    providers.serves("https://7tv.io/v3/users/kick/124", seven_account(&[seven("onceHi", "o1", 0)]));
    {
        let kick = Net::new();
        kick.serves("https://kick.com/api/v2/channels/emotes-test-once", json!({ "user_id": 124 }).to_string());
        assert_eq!(providers.shown(&info, &["7tv"]), ["onceHi o1/1x.webp"]);
    }
    // Kick no longer knows the page name (taken by someone else, say).
    assert_eq!(providers.shown(&info, &["7tv"]), ["onceHi o1/1x.webp"]);
}

/// A channel kept that isn't a number isn't put into a provider's link: the
/// VOD's channel is looked up again (word_emotes.rs `Channel::is_valid`:
/// "Both platforms' ids are numbers; nothing else goes into a provider's
/// URL").
#[test]
fn a_kept_channel_that_isnt_a_number_is_looked_up_again() {
    let providers = Providers::new("word-emotes-bad-channel");
    let kept = br#"{"platform":"kick","id":"abc"}"#;
    write_atomic(&providers.dir.join("kick-video-k3.channel.json"), kept).unwrap();
    providers.serves("https://kick.com/api/v2/channels/emotes-test-bad", json!({ "user_id": 125 }));
    providers.serves("https://7tv.io/v3/users/kick/125", seven_account(&[seven("badHi", "x1", 0)]));
    assert_eq!(providers.shown(&kick_vod("k3", "emotes-test-bad"), &["7tv"]), ["badHi x1/1x.webp"]);
}

/// A VOD whose id isn't plain letters, digits and dashes, or a Kick link
/// whose page name isn't, gets no emotes, and nothing is written for it
/// (word_emotes.rs `video_key`: "None for … an id that isn't plain letters,
/// digits and dashes"; `kick_slug`: "The channel's page name in a Kick VOD
/// link").
#[test]
fn a_vod_whose_id_or_page_isnt_plain_gets_no_emotes() {
    let providers = Providers::new("word-emotes-unsafe");
    let twitch = vod("twitch", "../../x", "https://www.twitch.tv/videos/x");
    assert!(for_vod(&twitch, &providers.dir, PROVIDER_IDS).unwrap().is_empty());
    // Kick would answer for such a page name, were it asked.
    for slug in ["..", "a.b"] {
        providers.serves(&format!("https://kick.com/api/v2/channels/{slug}"), json!({ "user_id": 127 }));
    }
    providers.serves("https://7tv.io/v3/users/kick/127", seven_account(&[seven("unsafe", "z1", 0)]));
    assert!(for_vod(&kick_vod("k4", ".."), &providers.dir, PROVIDER_IDS).is_err(), "not a Kick VOD link");
    assert!(for_vod(&kick_vod("k4b", "a.b"), &providers.dir, PROVIDER_IDS).is_err(), "not a Kick VOD link");
    assert!(fs::read_dir(&*providers.dir).unwrap().next().is_none(), "nothing kept");
}

/// Whose VOD it is that can't be kept is only slower next time: its emotes
/// still show (word_emotes.rs `for_vod`'s comment: "Not kept is only slower next time, not a
/// reason to go without").
#[test]
fn whose_vod_it_is_not_kept_still_shows_its_emotes() {
    let providers = Providers::new("word-emotes-unkept");
    // A folder where the lookup's file would go, so it can't be written.
    fs::create_dir_all(providers.dir.join("kick-video-k5.channel.part")).unwrap();
    providers.serves("https://kick.com/api/v2/channels/emotes-test-unkept", json!({ "user_id": 126 }));
    providers.serves("https://7tv.io/v3/users/kick/126", seven_account(&[seven("unkeptHi", "u1", 0)]));
    assert_eq!(providers.shown(&kick_vod("k5", "emotes-test-unkept"), &["7tv"]), ["unkeptHi u1/1x.webp"]);
    assert!(!providers.dir.join("kick-video-k5.channel.json").exists());
}

// Each provider's emotes.

/// 7TV gives the channel's active emote set and its global set, a
/// zero-width emote marked to go over the one before (word_emotes.rs, the comment over `seven_tv_channel`:
/// "a channel's account (by its Twitch or Kick id) has its active emote set
/// … flag 1 is zero-width"; `WordEmote::overlay`: "Drawn over the emote
/// before it (7TV's and FFZ's \"zero-width\" emotes …)"; 0.2.1: "with
/// \"zero-width\" emotes (a hat, a spin) drawn over the emote before them").
#[test]
fn seven_tv_gives_a_channels_active_set_and_its_zero_width_emotes() {
    let providers = Providers::new("word-emotes-7tv");
    let info = twitch_vod(&providers, "8", "1008");
    providers.serves(
        "https://7tv.io/v3/users/twitch/1008",
        seven_account(&[seven("GAMBA", "01G3", 0), seven("PETPET", "01F6", 1), seven("Wide", "01F7", 256)]),
    );
    providers.serves("https://7tv.io/v3/emote-sets/global", json!({ "emotes": [seven("EZ", "01AB", 0)] }));
    assert_eq!(
        providers.shown(&info, &["7tv"]),
        ["GAMBA 01G3/1x.webp", "PETPET (over) 01F6/1x.webp", "Wide 01F7/1x.webp", "EZ 01AB/1x.webp"]
    );
}

/// BetterTTV gives a Twitch channel's own and shared emotes, and the global
/// ones, but no modifiers (word_emotes.rs, the comment over `bttv_channel`: "BetterTTV: Twitch only. A
/// channel's own and shared emotes; modifiers (such as \"w!\", which widens
/// the next emote) aren't emotes to show").
#[test]
fn betterttv_gives_a_channels_own_and_shared_emotes_but_no_modifiers() {
    let providers = Providers::new("word-emotes-bttv");
    let info = twitch_vod(&providers, "9", "1009");
    providers.serves(
        "https://api.betterttv.net/3/cached/users/twitch/1009",
        json!({
            "channelEmotes": [{ "id": "5f1b0186cf6d2144653d2970", "code": "catJAM" }],
            "sharedEmotes": [
                { "id": "65cbe7dbaed093b2eaf87c65", "code": "w!", "modifier": true },
                { "id": "54fa8f1401e468494b85b537", "code": "pepeD" },
            ],
        }),
    );
    providers.serves(
        "https://api.betterttv.net/3/cached/emotes/global",
        json!([{ "id": "54fa8f1401e468494b85b5ab", "code": "SourPls" }]),
    );
    assert_eq!(
        providers.shown(&info, &["bttv"]),
        [
            "catJAM 5f1b0186cf6d2144653d2970/1x",
            "pepeD 54fa8f1401e468494b85b537/1x",
            "SourPls 54fa8f1401e468494b85b5ab/1x"
        ]
    );
}

/// FrankerFaceZ gives a Twitch channel's room sets and only the default
/// global sets, its modifier emotes going over the one before
/// (word_emotes.rs, the comment over `ffz_channel`: "FrankerFaceZ: Twitch only. A room (channel) and the
/// global sets each list emoticons with images by scale; modifier emotes
/// draw over the one before"; `ffz_global`'s comment: "Only the default sets are everyone's;
/// the others belong to some users").
#[test]
fn frankerfacez_gives_a_rooms_sets_and_only_the_default_global_ones() {
    let providers = Providers::new("word-emotes-ffz");
    let info = twitch_vod(&providers, "10", "1010");
    let ffz = |name: &str, id: u32, modifier: bool| json!({ "name": name, "modifier": modifier, "urls": { "1": format!("//cdn.frankerfacez.com/emote/{id}/1") } });
    providers.serves(
        "https://api.frankerfacez.com/v1/room/id/1010",
        json!({ "sets": { "77": { "emoticons": [ffz("monkaW", 1, false), ffz("ffzHat", 2, true)] } } }),
    );
    providers.serves(
        "https://api.frankerfacez.com/v1/set/global",
        json!({ "default_sets": [3], "sets": {
            "3": { "emoticons": [ffz("ZrehplaR", 9, false)] },
            "4": { "emoticons": [ffz("OnlySome", 10, false)] },
        } }),
    );
    assert_eq!(providers.shown(&info, &["ffz"]), ["monkaW 1/1", "ffzHat (over) 2/1", "ZrehplaR 9/1"]);
}

/// Every emote's image is an https link on its provider's CDN, which
/// emotes.rs fetches from, however the provider writes it (word_emotes.rs's module doc:
/// "Images come from the providers' CDNs (`emotes::HOSTS`)").
#[test]
fn every_emotes_image_is_on_its_providers_cdn() {
    let providers = Providers::new("word-emotes-cdn");
    let info = twitch_vod(&providers, "11", "1011");
    providers.serves("https://7tv.io/v3/users/twitch/1011", seven_account(&[seven("GAMBA", "01G3", 0)]));
    providers.serves(
        "https://api.betterttv.net/3/cached/users/twitch/1011",
        json!({ "channelEmotes": [{ "id": "5f1b0186cf6d2144653d2970", "code": "catJAM" }] }),
    );
    providers.serves(
        "https://api.frankerfacez.com/v1/room/id/1011",
        json!({ "sets": { "1": { "emoticons": [
            { "name": "httpsOne", "urls": { "1": "https://cdn.frankerfacez.com/emote/9/1" } },
            { "name": "schemeless", "urls": { "1": "//cdn.frankerfacez.com/emote/10/1" } },
        ] } } }),
    );
    let emotes = for_vod(&info, &providers.dir, PROVIDER_IDS).unwrap();
    let names: Vec<&str> = emotes.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["GAMBA", "catJAM", "httpsOne", "schemeless"]);
    for e in &emotes {
        assert!(crate::emotes::allowed(&e.url), "{e:?}");
    }
}

/// The providers' answers stood in for above, checked against the real
/// ones: a Twitch and a Kick channel with every provider, each image on its
/// CDN. Asks the providers for real: `cargo test -p vodscribe-core --
/// --ignored word_emotes`.
#[test]
#[ignore = "network"]
fn fetches_real_channels() {
    let dir = crate::TempDir::new("word-emotes-live");
    for info in [
        vod("twitch", "2899842479", "https://www.twitch.tv/videos/2899842479"),
        vod("kick", "01a0b0d6-75c0-75a7-8050-a72ab4dc9e0f", "https://kick.com/barky/videos/01a0b0d6"),
    ] {
        let emotes = for_vod(&info, &dir, PROVIDER_IDS).unwrap();
        let overlays = emotes.iter().filter(|e| e.overlay).count();
        eprintln!("{}: {} emotes, {overlays} overlays", info.url, emotes.len());
        assert!(emotes.len() > 20, "{} has only {} emotes", info.url, emotes.len());
        assert!(emotes.iter().all(|e| crate::emotes::allowed(&e.url)));
    }
}
