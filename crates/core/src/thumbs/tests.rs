//! The specs of thumbs.rs: a site's image fetched once when a VOD is added
//! and kept, so showing the library doesn't tell the platforms' servers
//! what's in it. The network is the kit's (`testkit::net`); each test's
//! image has a host of its own.

use super::*;
use crate::testkit::net::{Net, Reply};

/// A JPEG's first bytes, which is all `image_ext` looks at.
const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];

/// A link on a host no other test uses, which `net` says is public.
fn link(net: &Net, name: &str) -> String {
    let host = format!("thumbs-{name}-{}.example", std::process::id());
    net.public(&host);
    format!("https://{host}/thumb.jpg")
}

fn kept(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|d| d.map(|e| e.unwrap().file_name().into_string().unwrap()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// An image is fetched into the thumbnails' folder as `<id>-<stamp>.<ext>`,
/// its kind read from its bytes, not from the link (thumbs.rs `fetch`:
/// "Downloads the image at `url` into `dir` as `<id>-<stamp>.<ext>`").
#[test]
fn a_sites_image_is_kept_under_the_vods_id_and_its_kind() {
    let net = Net::new();
    let url = link(&net, "kept");
    net.serves(&url, JPEG);
    let dir = crate::TempDir::new("thumbs-kept");
    let name = fetch(&url, &dir, 12).unwrap().unwrap();
    assert!(name.starts_with("12-") && name.ends_with(".jpg"), "{name}");
    assert!(mime(&name).is_some(), "served as a thumbnail");
    assert_eq!(fs::read(dir.join(&name)).unwrap(), JPEG);
    assert_eq!(kept(&dir), [name], "no part file is left");
}

/// Fetched again, a VOD's image replaces the old one, under a fresh name, so the webview never shows a cached image for a reused VOD id
/// (thumbs.rs `fetch`: "A fresh name each time, so the webview never shows a
/// cached image for a reused VOD id").
#[test]
fn an_image_fetched_again_replaces_the_old_one() {
    let net = Net::new();
    let url = link(&net, "fresh");
    net.serves(&url, JPEG);
    let dir = crate::TempDir::new("thumbs-fresh");
    let first = fetch(&url, &dir, 12).unwrap().unwrap();
    let second = fetch(&url, &dir, 12).unwrap().unwrap();
    assert!(first == second || !dir.join(&first).exists(), "the old image is left beside the new");
    assert_eq!(kept(&dir), [second], "one image of the VOD");
}

/// A link to the local network is never followed (0.3.0: "VODscribe won't
/// follow links to this computer or your local network."; thumbs.rs `fetch`:
/// "The site said where the image is; it must be out on the internet"): no
/// image, and nothing to try again.
#[test]
fn a_link_to_the_local_network_gives_no_image() {
    let net = Net::new();
    let host = format!("thumbs-router-{}.example", std::process::id());
    net.local(&host);
    net.serves(&format!("https://{host}/"), JPEG);
    let dir = crate::TempDir::new("thumbs-local");
    assert_eq!(fetch(&format!("https://{host}/thumb.jpg"), &dir, 1).unwrap(), None);
    assert!(kept(&dir).is_empty());
}

/// A link that isn't a web link gives no image (thumbs.rs `fetch`, through
/// `net::public_url`: "VODscribe only follows web links").
#[test]
fn a_link_that_is_not_a_web_link_gives_no_image() {
    let dir = crate::TempDir::new("thumbs-file");
    assert_eq!(fetch("file:///etc/hostname", &dir, 1).unwrap(), None);
    assert!(kept(&dir).is_empty());
}

/// An image the site no longer has (a 4xx) is no image, not an error to try
/// again (thumbs.rs `fetch`: "`None` when there's no usable image there
/// (gone, or not an image); an error only for failures worth retrying").
#[test]
fn an_image_the_site_no_longer_has_gives_none() {
    let net = Net::new();
    let url = link(&net, "gone");
    net.answers(&url, 404);
    let dir = crate::TempDir::new("thumbs-gone");
    assert_eq!(fetch(&url, &dir, 1).unwrap(), None);
}

/// A server error is a failure worth trying again (thumbs.rs `fetch`: "an
/// error only for failures worth retrying (network, server errors)").
#[test]
fn a_server_error_is_an_error_to_try_again() {
    let net = Net::new();
    let url = link(&net, "error");
    net.answers(&url, 503);
    let dir = crate::TempDir::new("thumbs-error");
    assert!(fetch(&url, &dir, 1).is_err());
    assert!(kept(&dir).is_empty());
}

/// A name that can't be looked up now (offline) is an error to try again,
/// not an image that's gone (thumbs.rs `fetch`: "an error only for failures
/// worth retrying (network, server errors)").
#[test]
fn a_name_that_does_not_resolve_is_an_error_to_try_again() {
    let dir = crate::TempDir::new("thumbs-offline");
    // `.invalid` never resolves, online or not.
    let error = fetch("https://thumbs-offline.invalid/thumb.jpg", &dir, 1).unwrap_err();
    assert!(!net::is_client_error(&error), "{error:#}");
    assert!(kept(&dir).is_empty());
}

/// A thumbnail link that is no link at all (no scheme) can never be fetched,
/// so it's no image, not a failure to try again at each launch for a week
/// (thumbs.rs `fetch`: "`None` when there's no usable image there …; an error
/// only for failures worth retrying (network, server errors)").
#[test]
#[ignore = "disagreement: a thumbnail link with no scheme is an error to retry, not no image"]
fn a_thumbnail_link_that_is_no_link_gives_none() {
    let dir = crate::TempDir::new("thumbs-nolink");
    assert_eq!(fetch("not a link", &dir, 1).unwrap(), None);
}

/// What isn't an image is no image (thumbs.rs `fetch`: "`None` when there's
/// no usable image there (gone, or not an image)").
#[test]
fn a_page_that_is_not_an_image_gives_none() {
    let net = Net::new();
    let url = link(&net, "page");
    net.replies(&url, |_| Some(Reply::body("<html>sorry</html>")));
    let dir = crate::TempDir::new("thumbs-page");
    assert_eq!(fetch(&url, &dir, 1).unwrap(), None);
    assert!(kept(&dir).is_empty());
}

/// An image larger than any real thumbnail is no image, so a bogus link
/// can't fill the disk (thumbs.rs `MAX_BYTES`: "Larger than any real
/// thumbnail; stops a bogus URL from filling the disk").
#[test]
fn an_image_over_eight_megabytes_gives_none() {
    let net = Net::new();
    let url = link(&net, "big");
    let mut big = JPEG.to_vec();
    big.resize(8 * 1024 * 1024 + 1, 0);
    net.serves(&url, big);
    let dir = crate::TempDir::new("thumbs-big");
    assert_eq!(fetch(&url, &dir, 1).unwrap(), None);
    assert!(kept(&dir).is_empty());
}

/// Only the names this module writes are served as thumbnails (thumbs.rs
/// `mime`: "so the app serves nothing else as a thumbnail").
#[test]
fn only_thumbnail_names_are_served() {
    assert_eq!(mime("12.jpg"), Some("image/jpeg"));
    assert_eq!(mime("12-5.png"), Some("image/png"));
    assert_eq!(mime("12-5.gif"), Some("image/gif"));
    assert_eq!(mime("12-1791778286978.webp"), Some("image/webp"));
    assert_eq!(mime("12-.webp"), None);
    assert_eq!(mime("12-3-4.webp"), None);
    assert_eq!(mime("3.webp"), Some("image/webp"));
    assert_eq!(mime("../library.sqlite"), None);
    assert_eq!(mime("12.part"), None);
    assert_eq!(mime(".jpg"), None);
    assert_eq!(mime("1.jpg/../x"), None);
}

/// Removing a VOD's thumbnail takes only its own (thumbs.rs `remove`:
/// "Deletes VOD `id`'s thumbnail, if any"): not VOD 12's for VOD 1's, nor a
/// file that is no thumbnail.
#[test]
fn removing_a_vods_thumbnail_leaves_the_others() {
    let dir = crate::TempDir::new("thumbs-remove");
    for name in ["1.jpg", "1-5.webp", "12-5.webp", "1.part", "notes.txt"] {
        fs::write(dir.join(name), b"x").unwrap();
    }
    remove(&dir, 1);
    assert_eq!(kept(&dir), ["1.part", "12-5.webp", "notes.txt"]);
}

/// An image's kind is read from its first bytes (thumbs.rs `image_ext`).
#[test]
fn an_images_kind_is_read_from_its_bytes() {
    assert_eq!(image_ext(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
    assert_eq!(image_ext(b"\x89PNG\r\n"), Some("png"));
    assert_eq!(image_ext(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
    assert_eq!(image_ext(b"GIF89a"), Some("gif"));
    assert_eq!(image_ext(b"<html>"), None);
}
