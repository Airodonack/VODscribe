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

/// Downloads the image at `url` into `dir` as `<id>-<stamp>.<ext>` and returns that file
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
    // A fresh name each time, so the webview never shows a cached image for a
    // reused VOD id (SQLite can reuse the id of the last VOD removed).
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let name = format!("{id}-{stamp}.{ext}");
    let part = dir.join(format!("{id}-{stamp}.part"));
    fs::write(&part, &bytes)?;
    fs::rename(&part, dir.join(&name))?;
    Ok(Some(name))
}

/// Deletes VOD `id`'s thumbnail, if any.
pub fn remove(dir: &Path, id: i64) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if mime(&name).is_some() && name.split(['-', '.']).next() == Some(&id.to_string()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

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
/// anything else (so the app serves nothing else as a thumbnail).
pub fn mime(name: &str) -> Option<&'static str> {
    let (stem, ext) = name.split_once('.')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    // "<id>-<stamp>", or "<id>" from before names were stamped.
    if !stem.split('-').all(digits) || stem.split('-').count() > 2 {
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
        assert_eq!(mime("12-1791778286978.webp"), Some("image/webp"));
        assert_eq!(mime("12-.webp"), None);
        assert_eq!(mime("12-3-4.webp"), None);
        assert_eq!(mime("3.webp"), Some("image/webp"));
        assert_eq!(mime("../library.sqlite"), None);
        assert_eq!(mime("12.part"), None);
        assert_eq!(mime(".jpg"), None);
        assert_eq!(mime("1.jpg/../x"), None);
    }

    #[test]
    fn removes_only_that_vods_thumbnails() {
        let dir = std::env::temp_dir().join(format!("vodscribe-thumbs-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for name in ["1.jpg", "1-5.webp", "12-5.webp", "1.part", "notes.txt"] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        remove(&dir, 1);
        let mut left: Vec<String> =
            fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        assert_eq!(left, ["1.part", "12-5.webp", "notes.txt"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sniffs_images() {
        assert_eq!(image_ext(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(image_ext(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(image_ext(b"<html>"), None);
    }
}
