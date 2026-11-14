//! The specs of emotes.rs: the emote images chat shows, fetched from the
//! emote CDNs only and kept. The network is the
//! kit's (`testkit::net`); each test's image has a link of its own.

use super::*;
use crate::testkit::net::Net;

/// A PNG's first bytes, which is all `get` looks at to know it's one.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n a picture";

/// A link on 7TV's CDN no other test uses.
fn link(name: &str) -> String {
    format!("https://cdn.7tv.app/emote/emotes-test-{name}-{}/1x.webp", std::process::id())
}

/// The files `dir` holds, if it's there.
fn kept(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .map(|d| d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

/// Only https links on the platforms' and the providers' emote CDNs are
/// emotes (emotes.rs `allowed`: "Whether `url` is an https URL on one of the
/// emote CDNs"; `HOSTS`: "the platforms' own … and the word emote
/// providers'").
#[test]
fn fetches_only_from_emote_cdns() {
    assert!(allowed("https://files.kick.com/emotes/4085540/fullsize"));
    assert!(allowed("https://static-cdn.jtvnw.net/emoticons/v2/25/default/dark/1.0"));
    assert!(allowed("https://yt3.ggpht.com/abc=w24-h24-c-k-nd"));
    assert!(allowed("https://cdn.7tv.app/emote/01G3WEGZN0000ET2J0MQP5YJ0G/1x.webp"));
    assert!(allowed("https://cdn.betterttv.net/emote/5f1b0186cf6d2144653d2970/1x"));
    assert!(allowed("https://cdn.frankerfacez.com/emote/9/1"));
    assert!(!allowed("http://files.kick.com/emotes/1/fullsize"), "https only");
    assert!(!allowed("https://files.kick.com.evil.example/x"));
    assert!(!allowed("https://evil.example/files.kick.com"));
    assert!(!allowed("https://user@files.kick.com/x"));
    assert!(!allowed("https://files.kick.com:8443/x"));
    assert!(!allowed("file:///etc/passwd"));
}

/// A link off the emote CDNs is refused, never fetched, though it would
/// answer with an image (emotes.rs's module doc: "Only the emote providers' CDNs are
/// fetched from: the UI asks for emotes by URL, and this mustn't become a way
/// to fetch anything else").
#[test]
fn a_link_off_the_emote_cdns_is_never_fetched() {
    let net = Net::new();
    let elsewhere = format!("https://emotes-test-{}.example/kekw.png", std::process::id());
    net.serves(&elsewhere, PNG);
    let dir = crate::TempDir::new("emotes-elsewhere");
    let err = get(&elsewhere, &dir).unwrap_err();
    assert_eq!(err.to_string(), "Not an emote.");
    assert!(kept(&dir).is_empty());
}

/// An emote's image is fetched the first time it's shown and kept: shown
/// again, it's the copy, with the network gone (emotes.rs's module doc: "Local copies
/// of chat emote images, fetched the first time one is shown and kept, so a
/// busy chat doesn't fetch the same image over and over").
#[test]
fn an_emote_is_fetched_the_first_time_and_kept() {
    let url = link("kept");
    let dir = crate::TempDir::new("emotes-kept");
    {
        let net = Net::new();
        net.serves(&url, PNG);
        assert_eq!(get(&url, &dir).unwrap(), (PNG.to_vec(), "image/png"));
    }
    let net = Net::new();
    net.answers(&url, 500);
    assert_eq!(get(&url, &dir).unwrap(), (PNG.to_vec(), "image/png"), "the copy, not the network");
    assert_eq!(kept(&dir).len(), 1);
}

/// An image comes with its type, by what it is rather than its link, kept
/// copies included (emotes.rs `get`: "The image at `url` and its MIME type").
#[test]
fn an_emote_comes_with_its_image_type() {
    let net = Net::new();
    let dir = crate::TempDir::new("emotes-types");
    for (name, bytes, mime) in [
        ("jpeg", &b"\xFF\xD8\xFF\xE0 a picture"[..], "image/jpeg"),
        ("png", PNG, "image/png"),
        ("webp", b"RIFF\x10\0\0\0WEBPVP8 a picture", "image/webp"),
        ("gif", b"GIF89a a picture", "image/gif"),
    ] {
        let url = link(name);
        net.serves(&url, bytes);
        assert_eq!(get(&url, &dir).unwrap(), (bytes.to_vec(), mime), "{name}");
        assert_eq!(get(&url, &dir).unwrap().1, mime, "{name}, kept");
    }
}

/// What a CDN answers that isn't an image is refused and not kept, so it's
/// asked for again next time (emotes.rs `get`: "That emote isn't an image.").
#[test]
fn what_isnt_an_image_is_refused_and_not_kept() {
    let url = link("not-an-image");
    let dir = crate::TempDir::new("emotes-not-image");
    {
        let net = Net::new();
        net.serves(&url, "<html>Not found</html>");
        assert_eq!(get(&url, &dir).unwrap_err().to_string(), "That emote isn't an image.");
        assert!(kept(&dir).is_empty());
    }
    let net = Net::new();
    net.serves(&url, PNG);
    assert_eq!(get(&url, &dir).unwrap().0, PNG);
}

/// An answer larger than any real emote, 4 MB, is refused and not kept
/// (emotes.rs `MAX_BYTES`: "Larger than any real emote, animated ones
/// included").
#[test]
fn an_image_larger_than_any_emote_is_refused() {
    let url = link("huge");
    let dir = crate::TempDir::new("emotes-huge");
    let net = Net::new();
    let mut huge = PNG.to_vec();
    huge.resize(4 << 20, 0);
    net.serves(&url, huge.clone());
    assert_eq!(get(&url, &dir).unwrap().0.len(), 4 << 20, "4 MB is still an emote");
    let url = link("huger");
    huge.push(0);
    net.serves(&url, huge);
    assert!(get(&url, &dir).is_err());
    assert_eq!(kept(&dir).len(), 1, "only the first");
}
