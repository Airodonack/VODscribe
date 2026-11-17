use super::*;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dragndrop-source-placeholder-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn made(dir: &Path) -> PathBuf {
    let from = dir.join("made.mp4");
    std::fs::write(&from, "the clip").unwrap();
    from
}

/// The placeholder goes, then the made file takes its place (module doc: "The
/// empty file a promised file stands in for at the receiver's place while it's
/// made, and the copy that takes its place").
#[test]
fn the_placeholder_gives_way_to_the_made_file() {
    let dir = dir("own");
    let to = dir.join("clip.mp4");
    let placeholder = Placeholder::create(&to).unwrap();
    placeholder.remove().unwrap();
    copy_new(&made(&dir), &to).unwrap();
    assert_eq!(std::fs::read_to_string(&to).unwrap(), "the clip");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A placeholder already gone is no matter (`Placeholder::remove`: "nothing
/// there at all is fine").
#[test]
fn a_placeholder_already_gone_is_no_matter() {
    let dir = dir("gone");
    let to = dir.join("clip.mp4");
    let placeholder = Placeholder::create(&to).unwrap();
    std::fs::remove_file(&to).unwrap();
    placeholder.remove().unwrap();
    copy_new(&made(&dir), &to).unwrap();
    assert_eq!(std::fs::read_to_string(&to).unwrap(), "the clip");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file saved at the name meanwhile is kept, and the removal says so
/// (`Placeholder::remove`: "Anything else there (a file saved at that name
/// since, a link) is left as it is and refused with AlreadyExists"; 0.7.2
/// Fixes: "A file you saved under the clip's name while the clip was being
/// made is no longer deleted or written over").
#[test]
fn a_file_saved_at_its_name_meanwhile_is_kept() {
    let dir = dir("saved");
    let to = dir.join("clip.mp4");
    let placeholder = Placeholder::create(&to).unwrap();
    // The person saves another file of that name there while the clip is made.
    std::fs::remove_file(&to).unwrap();
    std::fs::write(&to, "theirs").unwrap();
    // Ready: refused, theirs kept. Failed or canceled: the same.
    let removed = placeholder.remove();
    assert_eq!(std::fs::read_to_string(&to).ok().as_deref(), Some("theirs"));
    assert_eq!(removed.unwrap_err().kind(), ErrorKind::AlreadyExists);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A link put in the placeholder's place is left, and nothing is written
/// through it (module doc: "only that file is ever removed ... never onto or
/// through whatever is there").
#[test]
fn a_link_put_in_its_place_is_left_and_never_written_through() {
    let dir = dir("link");
    let to = dir.join("clip.mp4");
    let elsewhere = dir.join("elsewhere");
    std::fs::write(&elsewhere, "theirs").unwrap();
    let placeholder = Placeholder::create(&to).unwrap();
    std::fs::remove_file(&to).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &to).unwrap();
    let removed = placeholder.remove();
    assert!(std::fs::symlink_metadata(&to).is_ok_and(|m| m.file_type().is_symlink()));
    assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "theirs");
    assert_eq!(removed.unwrap_err().kind(), ErrorKind::AlreadyExists);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A link put there between the placeholder's removal and the copy is never
/// written through (`copy_new`: "never onto or through one there").
#[test]
fn a_link_put_there_after_the_placeholder_went_is_never_written_through() {
    let dir = dir("late-link");
    let to = dir.join("clip.mp4");
    let elsewhere = dir.join("elsewhere");
    std::fs::write(&elsewhere, "theirs").unwrap();
    let placeholder = Placeholder::create(&to).unwrap();
    placeholder.remove().unwrap();
    // Planted between the placeholder's removal and the copy.
    std::os::unix::fs::symlink(&elsewhere, &to).unwrap();
    let copied = copy_new(&made(&dir), &to);
    assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "theirs");
    assert_eq!(copied.unwrap_err().kind(), ErrorKind::AlreadyExists);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Placeholder::create: "Makes it, as a new file: one already there is
/// refused (AlreadyExists)", whatever it is, and what's there is left.
#[test]
fn a_placeholder_is_never_made_onto_what_is_there() {
    let dir = dir("create-taken");
    let (file, linked, elsewhere) = (dir.join("file.mp4"), dir.join("linked.mp4"), dir.join("elsewhere"));
    std::fs::write(&file, "theirs").unwrap();
    std::fs::write(&elsewhere, "theirs").unwrap();
    std::os::unix::fs::symlink(&elsewhere, &linked).unwrap();
    for to in [&file, &linked] {
        assert_eq!(Placeholder::create(to).err().map(|e| e.kind()), Some(ErrorKind::AlreadyExists), "{to:?}");
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "theirs");
    assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "theirs", "written through the link");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Placeholder::create is the empty file at the receiver's place while the
/// file is made ("with an empty file there meanwhile", lib.rs, macOS).
#[test]
fn a_placeholder_is_an_empty_file_at_its_place() {
    let dir = dir("empty");
    let to = dir.join("clip.mp4");
    let placeholder = Placeholder::create(&to).unwrap();
    assert!(std::fs::metadata(&to).unwrap().is_file());
    assert_eq!(std::fs::metadata(&to).unwrap().len(), 0);
    placeholder.remove().unwrap();
    assert!(!to.exists(), "left when the file won't come");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Placeholder::remove: "Anything else there (a file saved at that name
/// since, a link) is left as it is and refused with AlreadyExists": a folder
/// put there too.
#[test]
fn a_folder_put_in_the_placeholders_place_is_left() {
    let dir = dir("folder");
    let to = dir.join("clip.mp4");
    let placeholder = Placeholder::create(&to).unwrap();
    std::fs::remove_file(&to).unwrap();
    std::fs::create_dir(&to).unwrap();
    assert_eq!(placeholder.remove().unwrap_err().kind(), ErrorKind::AlreadyExists);
    assert!(to.is_dir(), "the folder was taken away");
    let _ = std::fs::remove_dir_all(&dir);
}

/// copy_new: "as a new file, never onto or through one there (AlreadyExists)":
/// a file there is kept, untouched.
#[test]
fn a_copy_is_never_made_onto_a_file_there() {
    let dir = dir("copy-taken");
    let to = dir.join("clip.mp4");
    std::fs::write(&to, "theirs").unwrap();
    assert_eq!(copy_new(&made(&dir), &to).unwrap_err().kind(), ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read_to_string(&to).unwrap(), "theirs");
    let _ = std::fs::remove_dir_all(&dir);
}

/// copy_new: "written out, and taken away again if that fails partway": a
/// source that can't be read (a folder) leaves no file, nor does one that
/// isn't there.
#[test]
fn a_copy_that_fails_leaves_no_file() {
    let dir = dir("copy-failed");
    let to = dir.join("clip.mp4");
    // (Off macOS: there a folder is cloned, as APFS does.)
    #[cfg(not(target_os = "macos"))]
    {
        assert!(copy_new(&dir, &to).is_err());
        assert!(!to.exists(), "left after the copy failed");
    }
    assert_eq!(copy_new(&dir.join("missing.mp4"), &to).unwrap_err().kind(), ErrorKind::NotFound);
    assert!(!to.exists(), "made of nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Placeholder::remove: only "nothing there at all is fine"; a place that
/// can't be looked at (its folder not to be searched) is an error, not a
/// placeholder that went, so a file there is never taken for gone.
#[test]
fn a_place_that_cannot_be_looked_at_is_not_a_placeholder_that_went() {
    use std::os::unix::fs::PermissionsExt;
    let dir = dir("unsearchable");
    let folder = dir.join("folder");
    std::fs::create_dir(&folder).unwrap();
    let placeholder = Placeholder::create(&folder.join("clip.mp4")).unwrap();
    let mode = |m| std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(m)).unwrap();
    mode(0o000);
    let removed = placeholder.remove();
    mode(0o755);
    assert_eq!(removed.unwrap_err().kind(), ErrorKind::PermissionDenied);
    assert!(folder.join("clip.mp4").is_file(), "taken away");
    let _ = std::fs::remove_dir_all(&dir);
}
