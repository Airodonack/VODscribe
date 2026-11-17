use super::*;

use std::sync::atomic::AtomicUsize;

/// A test's file: made once `ready` says, or failed.
struct Test {
    path: PathBuf,
    ready: Arc<Mutex<Option<Result<(), String>>>>,
}

impl Item for Test {
    fn name(&self) -> String {
        self.path.file_name().unwrap().to_string_lossy().into_owned()
    }
    fn state(&self) -> State {
        match &*self.ready.lock().unwrap() {
            None => State::Making(None),
            Some(Ok(())) => State::Ready(self.path.clone()),
            Some(Err(why)) => State::Failed(why.clone()),
        }
    }
}

/// A test's file that counts how often it's looked at.
struct Polled {
    of: Test,
    polls: Arc<AtomicUsize>,
}

impl Item for Polled {
    fn name(&self) -> String {
        self.of.name()
    }
    fn state(&self) -> State {
        self.polls.fetch_add(1, Ordering::SeqCst);
        self.of.state()
    }
}

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dragndrop-source-ark-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("made")).unwrap();
    std::fs::create_dir_all(dir.join("folder")).unwrap();
    dir
}

/// Waits, ten seconds at most, until `done` says so.
fn eventually(what: &str, done: impl Fn() -> bool) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !done() {
        assert!(std::time::Instant::now() < until, "gave up waiting for {what}");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn ready_item(path: PathBuf) -> Box<dyn Item> {
    Box::new(Test { path, ready: Arc::new(Mutex::new(Some(Ok(())))) })
}

/// Each file goes into the folder as it's made, in order, under its own
/// name, numbered if one's there; the copies end once the last is in (module
/// doc: "each file is copied there as it's made, one by one"; `copy_all`:
/// "Copies each of `items` into `folder` once it's made ... another name if
/// one's there").
#[test]
fn the_files_are_copied_into_the_folder_as_they_are_made() {
    let dir = dir("copied");
    let (a, b) = (dir.join("made/a.mp4"), dir.join("made/b.mp4"));
    std::fs::write(&a, "a").unwrap();
    std::fs::write(&b, "b").unwrap();
    std::fs::write(dir.join("folder/a.mp4"), "theirs").unwrap();
    let made_b = Arc::new(Mutex::new(None));
    let items: Vec<Box<dyn Item>> = vec![ready_item(a), Box::new(Test { path: b, ready: made_b.clone() })];
    let folder = dir.join("folder");
    let copying = std::thread::spawn(move || copy_all(&items, &folder));
    eventually("the first copied", || dir.join("folder/a (2).mp4").is_file());
    assert!(!copying.is_finished(), "over before the second was made");
    assert_eq!(std::fs::read_to_string(dir.join("folder/a (2).mp4")).unwrap(), "a", "the first, numbered");
    assert!(!dir.join("folder/b.mp4").exists(), "the second is there before it was made");
    *made_b.lock().unwrap() = Some(Ok(()));
    assert_eq!(copying.join().unwrap(), Ok(()));
    assert_eq!(std::fs::read_to_string(dir.join("folder/b.mp4")).unwrap(), "b");
    assert_eq!(std::fs::read_to_string(dir.join("folder/a.mp4")).unwrap(), "theirs", "theirs kept");
    let _ = std::fs::remove_dir_all(&dir);
}

/// One that won't be made stops the copies, and says why (`copy_all`: "stops
/// at one that can't be"; `Offer::new`'s answer to the drag).
#[test]
fn a_file_that_fails_stops_the_copies() {
    let dir = dir("failed");
    let failed = Arc::new(Mutex::new(Some(Err("blocked".to_string()))));
    let items: Vec<Box<dyn Item>> = vec![Box::new(Test { path: dir.join("made/a.mp4"), ready: failed })];
    assert_eq!(copy_all(&items, &dir.join("folder")), Err("blocked".into()));
    assert_eq!(std::fs::read_dir(dir.join("folder")).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The copies go in the files' order, "one by one": the second made before
/// the first waits for it.
#[test]
fn a_file_made_before_the_one_ahead_of_it_waits_for_it() {
    let dir = dir("order");
    let (a, b) = (dir.join("made/a.mp4"), dir.join("made/b.mp4"));
    std::fs::write(&a, "a").unwrap();
    std::fs::write(&b, "b").unwrap();
    let made_a = Arc::new(Mutex::new(None));
    let polls = Arc::new(AtomicUsize::new(0));
    let items: Vec<Box<dyn Item>> =
        vec![Box::new(Polled { of: Test { path: a, ready: made_a.clone() }, polls: polls.clone() }), ready_item(b)];
    let folder = dir.join("folder");
    let copying = std::thread::spawn(move || copy_all(&items, &folder));
    // The loop has gone round, with the second ready all along.
    eventually("the loop to look at the first again", || polls.load(Ordering::SeqCst) >= 3);
    assert_eq!(std::fs::read_dir(dir.join("folder")).unwrap().count(), 0, "the second went ahead of the first");
    *made_a.lock().unwrap() = Some(Ok(()));
    assert_eq!(copying.join().unwrap(), Ok(()));
    assert_eq!(std::fs::read_to_string(dir.join("folder/a.mp4")).unwrap(), "a");
    assert_eq!(std::fs::read_to_string(dir.join("folder/b.mp4")).unwrap(), "b");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A name that's taken goes by " (2)", " (3)" and so on, before its
/// extension, and the files already there are never written over (`copy_all`:
/// "another name if one's there"; `unique_path`).
#[test]
fn a_name_taken_twice_is_numbered_on_to_the_next_free() {
    let dir = dir("numbered");
    let from = dir.join("made/clip.mp4");
    std::fs::write(&from, "new").unwrap();
    for taken in ["clip.mp4", "clip (2).mp4"] {
        std::fs::write(dir.join("folder").join(taken), "theirs").unwrap();
    }
    assert_eq!(copy_all(&[ready_item(from)], &dir.join("folder")), Ok(()));
    assert_eq!(std::fs::read_to_string(dir.join("folder/clip (3).mp4")).unwrap(), "new");
    for taken in ["clip.mp4", "clip (2).mp4"] {
        assert_eq!(std::fs::read_to_string(dir.join("folder").join(taken)).unwrap(), "theirs", "{taken}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file that can't be put in the folder (here one made and gone) stops the
/// copies, and says why; what went before stays, and no
/// half-written copy is left (`copy_one`: written beside, renamed once whole).
#[test]
fn a_file_that_cannot_be_copied_stops_the_copies() {
    let dir = dir("unwritable");
    let a = dir.join("made/a.mp4");
    std::fs::write(&a, "a").unwrap();
    let items = vec![ready_item(a), ready_item(dir.join("made/gone.mp4"))];
    let folder = dir.join("folder");
    let failed = copy_all(&items, &folder).unwrap_err();
    assert!(!failed.is_empty(), "no reason given");
    assert_eq!(std::fs::read_to_string(folder.join("a.mp4")).unwrap(), "a", "the one before it");
    let left: Vec<_> = std::fs::read_dir(&folder).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(left, ["a.mp4"], "a half-written copy left");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A drag of no files is copied at once.
#[test]
fn no_files_are_no_copies() {
    let dir = dir("none");
    assert_eq!(copy_all(&[], &dir.join("folder")), Ok(()));
    let _ = std::fs::remove_dir_all(&dir);
}
