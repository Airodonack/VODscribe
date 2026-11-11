//! The library file itself: a broken one set aside.

use super::*;

#[test]
fn a_broken_library_is_set_aside_not_deleted() {
    let dir = crate::TempDir::new("set-aside");
    let paths = crate::Paths::new(dir.join("data"));
    paths.ensure().unwrap();
    let path = paths.db();
    std::fs::write(&path, b"not a database").unwrap();
    assert!(Db::open(&path).is_err());
    std::fs::write(dir.join("data/library.sqlite-wal"), b"wal").unwrap();
    std::fs::create_dir_all(paths.media()).unwrap();
    std::fs::write(paths.source_video(1, "mp4"), b"video").unwrap();
    let aside = set_aside(&paths).unwrap();
    let name = aside.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("library-broken-"), "{name}");
    // Laid out as a library, so moving its contents back restores it.
    let old = crate::Paths::new(&aside);
    assert_eq!(std::fs::read(old.db()).unwrap(), b"not a database");
    assert!(aside.join("library.sqlite-wal").exists(), "its WAL goes with it");
    assert_eq!(std::fs::read(old.source_video(1, "mp4")).unwrap(), b"video", "its kept video goes with it");
    assert!(!path.exists() && !paths.media().exists());
    // A new library opens in its place; a second one set aside the same day gets -2.
    drop(Db::open(&path).unwrap());
    let again = set_aside(&paths).unwrap();
    assert!(again.to_string_lossy().ends_with(&format!("{name}-2")), "{}", again.display());
    assert_eq!(date_of(0), "1970-01-01");
    assert_eq!(date_of(1790632794), "2026-09-28");
}

/// A move that fails and then can't all go back says what's left in the
/// set-aside folder, instead of asking to try again.
#[test]
fn a_library_partly_set_aside_says_what_is_left() {
    let dir = crate::TempDir::new("set-aside-stuck");
    let paths = crate::Paths::new(dir.join("data"));
    paths.ensure().unwrap();
    std::fs::write(paths.db(), b"not a database").unwrap();
    std::fs::create_dir_all(paths.media()).unwrap();
    std::fs::write(paths.source_video(1, "mp4"), b"video").unwrap();
    // The thumbnails move, the downloads under way (the last) don't, and
    // the thumbnails can't go back.
    let (work, thumbs) = (paths.work(), paths.thumbs());
    let rename = |from: &Path, to: &Path| {
        if from == work || to == thumbs {
            return Err(std::io::Error::other("refused"));
        }
        std::fs::rename(from, to)
    };
    let failed = format!("{:#}", set_aside_with(&paths, &rename, &in_use).unwrap_err());
    assert!(failed.contains("couldn't be moved back") && failed.contains("thumbs"), "{failed}");
    assert!(!failed.contains("start a new library again"), "{failed}");
    let aside = dir.join("data").join(format!("library-broken-{}", date_of(now())));
    assert!(aside.join("thumbs").is_dir(), "the thumbnails are in the set-aside folder, as it says");
    assert_eq!(std::fs::read(paths.db()).unwrap(), b"not a database", "the rest is back");
    assert!(paths.source_video(1, "mp4").exists());
}

/// Split (Windows: Roaming and Local), what's stuck is in Local's
/// set-aside folder: the message names it by its whole path, not the
/// Roaming one, and doesn't send it "beside the library" (the database).
#[test]
fn a_library_partly_set_aside_in_two_folders_names_each_path() {
    let dir = crate::TempDir::new("set-aside-split-stuck");
    let paths = crate::Paths::split(dir.join("roaming"), dir.join("local"));
    paths.ensure().unwrap();
    std::fs::write(paths.db(), b"not a database").unwrap();
    let (work, thumbs) = (paths.work(), paths.thumbs());
    let rename = |from: &Path, to: &Path| {
        if from == work || to == thumbs {
            return Err(std::io::Error::other("refused"));
        }
        std::fs::rename(from, to)
    };
    let failed = format!("{:#}", set_aside_with(&paths, &rename, &in_use).unwrap_err());
    let name = format!("library-broken-{}", date_of(now()));
    let stuck = dir.join("local").join(&name).join("thumbs");
    assert!(stuck.is_dir());
    assert!(failed.contains(&crate::tools::shown(&stuck)), "{failed}");
    let roaming = crate::tools::shown(dir.join("roaming").join(&name));
    assert!(!failed.contains(&format!("{roaming} ")), "names the Roaming folder: {failed}");
    assert!(failed.ends_with("Move them back where they were before trying again."), "{failed}");
}

/// A file another app has open stops the move, and something can't go
/// back: the message names the file as the cause, without the in-use
/// message's own advice to start again.
#[test]
fn a_library_partly_set_aside_by_an_open_file_says_only_which() {
    let dir = crate::TempDir::new("set-aside-open-stuck");
    let paths = crate::Paths::new(dir.join("data"));
    paths.ensure().unwrap();
    std::fs::write(paths.db(), b"not a database").unwrap();
    let (work, thumbs) = (paths.work(), paths.thumbs());
    let open = work.join("1.mp4");
    let rename = |from: &Path, to: &Path| {
        if from == work || to == thumbs {
            return Err(std::io::Error::other("refused"));
        }
        std::fs::rename(from, to)
    };
    let in_use = |from: &Path| (from == work).then(|| open.clone());
    let failed = format!("{:#}", set_aside_with(&paths, &rename, &in_use).unwrap_err());
    let shown = crate::tools::shown(&open);
    assert!(
        failed.contains(&format!(
            "after this failed: {shown} is open in another app. Move them back where they were before trying again."
        )),
        "{failed}"
    );
    assert!(!failed.contains("..") && !failed.contains("start a new library again"), "{failed}");
    // With everything back, the in-use message is the whole answer.
    std::fs::rename(dir.join("data").join(format!("library-broken-{}", date_of(now()))).join("thumbs"), &thumbs)
        .unwrap();
    let rename = |from: &Path, to: &Path| {
        if from == work {
            return Err(std::io::Error::other("refused"));
        }
        std::fs::rename(from, to)
    };
    let failed = format!("{:#}", set_aside_with(&paths, &rename, &in_use).unwrap_err());
    assert_eq!(failed, format!("{shown} is open in another app. Close that app, then start a new library again."));
}

/// A library whose folders can't all be moved is left as it was.
#[cfg(unix)]
#[test]
fn a_library_that_cannot_be_set_aside_stays_whole() {
    use std::os::unix::fs::PermissionsExt;
    let dir = crate::TempDir::new("set-aside-fails");
    let paths = crate::Paths::split(dir.join("roaming"), dir.join("local"));
    paths.ensure().unwrap();
    std::fs::write(paths.db(), b"not a database").unwrap();
    std::fs::create_dir_all(paths.media()).unwrap();
    std::fs::write(paths.source_video(1, "mp4"), b"video").unwrap();
    // Moving a folder elsewhere writes its "..": refused without write permission.
    let mode = |p: &Path, m| std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
    mode(&paths.media(), 0o500);
    let failed = set_aside(&paths);
    mode(&paths.media(), 0o700);
    assert!(failed.is_err(), "{failed:?}");
    assert_eq!(std::fs::read(paths.db()).unwrap(), b"not a database", "its file is back");
    assert!(paths.source_video(1, "mp4").exists() && paths.thumbs().is_dir());
    let left = |d: &Path| {
        std::fs::read_dir(d).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("library-broken-"))
    };
    assert!(!left(&dir.join("roaming")) && !left(&dir.join("local")), "a folder left");
}

/// On Windows a clip another app has open stops the move: nothing moves,
/// and the error names the file, for the recovery screen to show.
#[cfg(windows)]
#[test]
fn a_library_with_a_clip_open_elsewhere_stays_whole_and_says_which() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = crate::TempDir::new("set-aside-open");
    let paths = crate::Paths::split(dir.join("roaming"), dir.join("local"));
    paths.ensure().unwrap();
    std::fs::write(paths.db(), b"not a database").unwrap();
    std::fs::create_dir_all(paths.media()).unwrap();
    std::fs::write(paths.source_video(1, "mp4"), b"video").unwrap();
    let clip = paths.clip_file(1, "mp4");
    std::fs::create_dir_all(paths.clips()).unwrap();
    std::fs::write(&clip, b"clip").unwrap();
    // Shared for reading only, as a player or an editor opens it.
    let open = std::fs::OpenOptions::new().read(true).share_mode(1).open(&clip).unwrap();
    let failed = set_aside(&paths).unwrap_err().to_string();
    drop(open);
    assert!(failed.contains("1.mp4") && failed.contains("open in another app"), "{failed}");
    assert_eq!(std::fs::read(paths.db()).unwrap(), b"not a database", "its file is back");
    assert!(paths.source_video(1, "mp4").exists() && clip.exists());
}
