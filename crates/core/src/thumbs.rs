//! Local copies of VOD thumbnails. They're fetched once, when a VOD is added,
//! so showing the library doesn't tell the platforms' CDNs what's in it.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use anyhow::Result;

use crate::net;

/// Larger than any real thumbnail; stops a bogus URL from filling the disk.
const MAX_BYTES: u64 = 8 << 20;

/// Downloads the image at `url` into `dir` as `<id>.<ext>` and returns that file
/// name. `None` when there's no usable image there (gone, or not an image); an
/// error only for failures worth retrying (network, server errors).
pub fn fetch(url: &str, dir: &Path, id: i64) -> Result<Option<String>> {
    let never = AtomicBool::new(false);
    let body = match net::get(url, &never) {
        Ok((body, _)) => body,
        Err(e) if net::is_client_error(&e) => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    body.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    let Some(ext) = image_ext(&bytes).filter(|_| bytes.len() as u64 <= MAX_BYTES) else { return Ok(None) };
    fs::create_dir_all(dir)?;
    remove(dir, id);
    let name = format!("{id}.{ext}");
    let part = dir.join(format!("{id}.part"));
    fs::write(&part, &bytes)?;
    fs::rename(&part, dir.join(&name))?;
    Ok(Some(name))
}

/// Deletes VOD `id`'s thumbnail, if any.
pub fn remove(dir: &Path, id: i64) {
    for ext in EXTS {
        let _ = fs::remove_file(dir.join(format!("{id}.{ext}")));
    }
}

const EXTS: [&str; 4] = ["jpg", "png", "webp", "gif"];

fn image_ext(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => Some("jpg"),
        [0x89, b'P', b'N', b'G', ..] => Some("png"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("webp"),
        [b'G', b'I', b'F', b'8', ..] => Some("gif"),
        _ => None,
    }
}

/// The MIME type for a thumbnail file name this module wrote, or None for
/// anything else (so the app's `thumb:` scheme serves nothing else).
pub fn mime(name: &str) -> Option<&'static str> {
    let (id, ext) = name.split_once('.')?;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(match ext {
        "jpg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_only_thumbnail_names() {
        assert_eq!(mime("12.jpg"), Some("image/jpeg"));
        assert_eq!(mime("3.webp"), Some("image/webp"));
        assert_eq!(mime("../library.sqlite"), None);
        assert_eq!(mime("12.part"), None);
        assert_eq!(mime(".jpg"), None);
        assert_eq!(mime("1.jpg/../x"), None);
    }

    #[test]
    fn sniffs_images() {
        assert_eq!(image_ext(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(image_ext(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(image_ext(b"<html>"), None);
    }
}
