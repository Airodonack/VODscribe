//! Local copies of chat emote images, fetched the first time one is shown and
//! kept, so a busy chat doesn't fetch the same image over and over. Only the
//! emote providers' CDNs are fetched from: the UI asks for emotes by URL, and
//! this mustn't become a way to fetch anything else.

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use anyhow::{Result, bail};
use sha2::{Digest, Sha256};

use crate::net;

/// The hosts emote images come from: the platforms' own (see
/// `chat::ChatPart::Emote`) and the word emote providers' (`word_emotes`). A
/// new provider adds its CDN here.
const HOSTS: &[&str] = &[
    "files.kick.com",
    "static-cdn.jtvnw.net",
    "yt3.ggpht.com",
    "cdn.7tv.app",
    "cdn.betterttv.net",
    "cdn.frankerfacez.com",
];

/// Larger than any real emote, animated ones included.
const MAX_BYTES: u64 = 4 << 20;

/// Whether `url` is an https URL on one of the emote CDNs.
pub fn allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    HOSTS.contains(&host)
}

/// The image at `url` and its MIME type, from `dir` if it was fetched before.
pub fn get(url: &str, dir: &Path) -> Result<(Vec<u8>, &'static str)> {
    if !allowed(url) {
        bail!("Not an emote.");
    }
    let stem = net::hex(&Sha256::digest(url.as_bytes())[..16]);
    for ext in crate::thumbs::IMAGE_EXTS {
        if let Ok(bytes) = fs::read(dir.join(format!("{stem}.{ext}"))) {
            return Ok((bytes, crate::thumbs::ext_mime(ext)));
        }
    }
    let bytes = net::get_bytes(url, MAX_BYTES, &AtomicBool::new(false))?;
    let Some(ext) = crate::thumbs::image_ext(&bytes) else { bail!("That emote isn't an image.") };
    fs::create_dir_all(dir)?;
    let part = dir.join(format!("{stem}.part"));
    fs::write(&part, &bytes)?;
    fs::rename(&part, dir.join(format!("{stem}.{ext}")))?;
    Ok((bytes, crate::thumbs::ext_mime(ext)))
}

#[cfg(test)]
mod tests;
