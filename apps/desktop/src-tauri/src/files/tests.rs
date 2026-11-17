use super::*;

/// On Windows a file another program has open can't be deleted: it goes
/// once that program lets go (files.rs: "Deleting files the app made ...
/// where Windows won't while something has them open: those go later";
/// `delete_file`: "Deletes `path` now, or once nothing has it open").
#[cfg(windows)]
#[test]
fn a_file_in_use_goes_once_let_go() {
    use std::os::windows::fs::OpenOptionsExt;
    let path = std::env::temp_dir().join(format!("vodscribe-in-use-{}.mp4", std::process::id()));
    std::fs::write(&path, b"video").unwrap();
    // Shared for reading only, as ffmpeg opens what it reads.
    let reader = std::fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
    delete_file(&path);
    assert!(path.exists(), "still open");
    delete_leftovers();
    assert!(path.exists(), "still open");
    drop(reader);
    delete_leftovers();
    assert!(!path.exists(), "gone once let go");
}

/// A folder of the test's own, with the file `name` in it holding `text`.
fn made(dir: &vodscribe_core::testkit::TempDir, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    path
}

/// A file that can be deleted goes at once, and one already gone is no
/// matter; neither is left to delete later (`delete_file`: "Deletes `path`
/// now, or once nothing has it open").
#[test]
fn a_file_that_can_be_deleted_goes_at_once_and_is_not_left_to_delete() {
    let dir = vodscribe_core::testkit::TempDir::new("files-now");
    let file = made(&dir, "clip.mp4", "clip");
    delete_file(&file);
    assert!(!file.exists(), "kept");
    delete_file(&file);
    #[cfg(unix)]
    assert!(!left_to_delete().contains(&file), "listed to delete later");
}

/// A file left to delete is listed in the store as it's added, and the store
/// goes once the list is empty: what's left at quit is what the next start
/// tries again (`Leftovers`: "kept in `store` ... as the list changes";
/// `Leftovers::save`: "removed once it's empty"). The store is written under
/// a name of its own first, so none is left beside it.
#[cfg(unix)]
#[test]
fn the_list_of_files_left_is_kept_as_it_changes_and_goes_when_empty() {
    let dir = vodscribe_core::testkit::TempDir::new("files-store");
    let (clips, store) = (dir.join("clips"), dir.join("leftovers.json"));
    let (a, b) = (made(&dir, "clips/a.mp4", "a"), made(&dir, "clips/b.mp4", "b"));
    let mut left = Leftovers::default();
    left.keep_in(store.clone(), &clips);
    assert!(!store.exists(), "a store of nothing");
    left.add(&a);
    left.add(&b);
    let kept = std::fs::read_to_string(&store).unwrap();
    assert!(kept.contains("a.mp4") && kept.contains("b.mp4"), "{kept}");
    assert!(!store.with_extension("json.part").exists(), "the half written one is left");
    left.writing(&a);
    let kept = std::fs::read_to_string(&store).unwrap();
    assert!(!kept.contains("a.mp4") && kept.contains("b.mp4"), "{kept}");
    left.writing(&b);
    assert!(!store.exists(), "kept with nothing in it");
    assert!(left.listed().is_empty());
}

/// A file written anew at its path isn't left to delete (`writing`: "`path`
/// is being written anew: it isn't left to delete"), and one that wasn't
/// listed is no matter.
#[cfg(unix)]
#[test]
fn a_file_written_anew_is_no_longer_left_to_delete() {
    let dir = vodscribe_core::testkit::TempDir::new("files-anew");
    let (stuck, other) = (made(&dir, "clips/5.mp4", "clip"), made(&dir, "clips/6.mp4", "clip"));
    let mut left = Leftovers::default();
    left.add(&stuck);
    left.add(&other);
    left.writing(&dir.join("clips/7.mp4"));
    assert_eq!(left.listed(), [stuck.clone(), other.clone()], "one never listed changed the list");
    left.writing(&stuck);
    assert_eq!(left.listed(), [other], "still listed");
    left.retry();
    assert!(stuck.is_file(), "deleted, though written anew");
}

/// A file listed is deleted only while it's the file listed: "one changed or
/// replaced since is another file, not this one, and stays"; one gone is
/// dropped from the list (`Leftover`'s doc).
#[cfg(unix)]
#[test]
fn a_file_changed_since_it_was_listed_is_another_file_and_stays() {
    let dir = vodscribe_core::testkit::TempDir::new("files-changed");
    let (stuck, changed, gone) =
        (made(&dir, "clips/5.mp4", "clip"), made(&dir, "clips/6.mp4", "clip"), made(&dir, "clips/7.mp4", "clip"));
    let mut left = Leftovers::default();
    for file in [&stuck, &changed, &gone] {
        left.add(file);
    }
    std::fs::write(&changed, "a clip made since").unwrap();
    std::fs::remove_file(&gone).unwrap();
    left.retry();
    assert!(!stuck.exists(), "the file listed was kept");
    assert_eq!(std::fs::read_to_string(&changed).unwrap(), "a clip made since", "another file deleted");
    assert!(left.listed().is_empty(), "still listed: {:?}", left.listed());
}

/// A file listed again is the file as it is then: one changed in between is
/// the one deleted (`Leftover`: "as it was then"; `Leftovers::add`).
#[cfg(unix)]
#[test]
fn a_file_listed_again_is_taken_as_it_is_then() {
    let dir = vodscribe_core::testkit::TempDir::new("files-again");
    let file = made(&dir, "clips/5.mp4", "clip");
    let mut left = Leftovers::default();
    left.add(&file);
    std::fs::write(&file, "a clip made since").unwrap();
    left.add(&file);
    assert_eq!(left.listed(), std::slice::from_ref(&file), "listed twice");
    left.retry();
    assert!(!file.exists(), "kept");
}

/// A file that still can't be deleted stays listed through each try, and goes
/// once it can be (`Leftovers::retry`: "keeps those that still can't be").
#[cfg(unix)]
#[test]
fn a_file_that_still_cannot_be_deleted_stays_listed_until_it_can() {
    let dir = vodscribe_core::testkit::TempDir::new("files-held");
    let file = made(&dir, "clips/5.mp4", "clip");
    let mut left = Leftovers::default();
    left.add(&file);
    held(&dir.join("clips"), || {
        left.retry();
        left.retry();
        assert!(file.is_file(), "deleted from a folder that can't be written to");
        assert_eq!(left.listed(), std::slice::from_ref(&file), "let go of while still held");
    });
    left.retry();
    assert!(!file.exists(), "kept once it could go");
    assert!(left.listed().is_empty());
}

/// A folder that can't be written to holds its files, as Windows holds one
/// open: `f` runs with `folder` so, and it's let go after.
#[cfg(unix)]
fn held<T>(folder: &Path, f: impl FnOnce() -> T) -> T {
    use std::os::unix::fs::PermissionsExt;
    let mode = |m| std::fs::set_permissions(folder, std::fs::Permissions::from_mode(m)).unwrap();
    mode(0o555);
    let result = f();
    mode(0o755);
    result
}

/// A file that can't be deleted when it's to go is left to delete later, and
/// goes once it can (`delete_file`: "Deletes `path` now, or once nothing has
/// it open"; `delete_leftovers`: "Deletes what couldn't be deleted before, if
/// it can be now").
#[cfg(unix)]
#[test]
fn a_file_that_could_not_be_deleted_goes_once_it_can() {
    let dir = vodscribe_core::testkit::TempDir::new("files-later");
    let file = made(&dir, "clips/5.mp4", "clip");
    held(&dir.join("clips"), || {
        delete_file(&file);
        assert!(file.is_file(), "deleted from a folder that can't be written to");
        assert!(left_to_delete().contains(&file), "not left to delete later");
    });
    delete_leftovers();
    assert!(!file.exists(), "kept once it could go");
    assert!(!left_to_delete().contains(&file), "still listed");
}

/// A file that was left to delete, written anew at its path, isn't deleted
/// when the app tries again (`writing`: "if an earlier file there was left to
/// delete later (and has gone since), the new one isn't").
#[cfg(unix)]
#[test]
fn a_path_about_to_be_written_is_no_longer_left_to_delete() {
    let dir = vodscribe_core::testkit::TempDir::new("files-writing");
    let file = made(&dir, "clips/5.mp4", "clip");
    held(&dir.join("clips"), || {
        delete_file(&file);
        assert!(left_to_delete().contains(&file), "not left to delete later");
        writing(&file);
    });
    assert!(!left_to_delete().contains(&file), "still listed after being written anew");
    delete_leftovers();
    assert!(file.is_file(), "the file written anew was deleted");
}
