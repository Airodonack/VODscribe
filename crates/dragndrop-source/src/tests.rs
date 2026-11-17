use super::*;

/// A drag's answer waits while any of its files is still being made,
/// and is none once one can't be: all or nothing, as one drop (`settled`:
/// "The paths of `states` if all are made; else, if one won't be, why; else
/// (some still being made) None").
#[test]
fn a_drag_is_answered_once_all_its_files_are_made() {
    let ready = |p: &str| State::Ready(PathBuf::from(p));
    assert!(settled(vec![ready("/a"), State::Making(None)]).is_none());
    assert_eq!(settled(vec![ready("/a"), ready("/b")]), Some(Ok(vec![PathBuf::from("/a"), PathBuf::from("/b")])));
    assert_eq!(settled(vec![State::Making(Some(0.5)), State::Failed("no".into())]), Some(Err("no".into())));
    assert_eq!(settled(vec![]), Some(Ok(vec![])));
}

/// An early release is told by its type (`ReleasedEarly`: "Tell this by its
/// type, not its words").
#[test]
fn an_early_release_says_why() {
    let e: Box<dyn std::error::Error> = Box::new(ReleasedEarly);
    assert!(e.is::<ReleasedEarly>());
    assert_eq!(e.to_string(), "The button was released before the drag could begin.");
}

/// An item that is made, with nothing said but what the trait asks.
struct Plain;

impl Item for Plain {
    fn name(&self) -> String {
        "clip.mp4".into()
    }
    fn state(&self) -> State {
        State::Making(None)
    }
}

/// A folder of the test's own, empty.
#[cfg(unix)]
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dragndrop-source-lib-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Item::file_type: "public.data" unless you say, and an item that says
/// nothing of being waited for or called off is none the worse (`cancelled`
/// and `waited_for` do nothing by default).
#[test]
fn an_item_that_names_no_type_is_public_data() {
    assert_eq!(Plain.file_type(), "public.data");
    Plain.waited_for();
    Plain.waited_for();
    Plain.cancelled();
    assert_eq!(Plain.state(), State::Making(None), "still being made");
}

/// Item::write: "Puts the made file `from` at `to`, where a macOS receiver
/// asked for it, as a new file ... A copy, by default".
#[cfg(unix)]
#[test]
fn an_item_is_put_where_a_receiver_asked_as_a_copy() {
    let dir = folder("write");
    let (from, to) = (dir.join("made.mp4"), dir.join("clip.mp4"));
    std::fs::write(&from, "the clip").unwrap();
    Plain.write(&from, &to).unwrap();
    assert_eq!(std::fs::read_to_string(&to).unwrap(), "the clip");
    assert_eq!(std::fs::read_to_string(&from).unwrap(), "the clip", "the made file is still its own");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Item::write: "never onto or through one there, which is refused
/// (AlreadyExists)": a file saved at that name meanwhile is kept, and so is
/// what a link there points at.
#[cfg(unix)]
#[test]
fn an_item_is_never_put_onto_or_through_what_is_there() {
    let dir = folder("write-refused");
    let from = dir.join("made.mp4");
    std::fs::write(&from, "the clip").unwrap();
    let (taken, linked, elsewhere) = (dir.join("taken.mp4"), dir.join("linked.mp4"), dir.join("elsewhere"));
    std::fs::write(&taken, "theirs").unwrap();
    std::fs::write(&elsewhere, "theirs").unwrap();
    std::os::unix::fs::symlink(&elsewhere, &linked).unwrap();
    for to in [&taken, &linked] {
        assert_eq!(Plain.write(&from, to).unwrap_err().kind(), std::io::ErrorKind::AlreadyExists, "{to:?}");
    }
    assert_eq!(std::fs::read_to_string(&taken).unwrap(), "theirs");
    assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "theirs");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Item::write: a file that can't be copied leaves nothing at the receiver's
/// place (placeholder.rs `copy_new`: "taken away again if that fails
/// partway"; a folder as the made file fails as it's read).
#[cfg(unix)]
#[test]
fn an_item_that_could_not_be_copied_leaves_nothing_there() {
    let dir = folder("write-failed");
    let to = dir.join("clip.mp4");
    // (Off macOS: there a folder is cloned, as APFS does.)
    #[cfg(not(target_os = "macos"))]
    {
        assert!(Plain.write(&dir, &to).is_err(), "a folder copied");
        assert!(!to.exists(), "a half-written file left");
    }
    assert_eq!(Plain.write(&dir.join("missing.mp4"), &to).unwrap_err().kind(), std::io::ErrorKind::NotFound);
    assert!(!to.exists(), "a file made of nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

/// trace_with: "Has `to` hear a line about each step of every drag ... The
/// first one set stays."
#[test]
fn a_trace_hears_every_line_and_the_first_one_set_stays() {
    use std::sync::Mutex;
    static FIRST: Mutex<Vec<String>> = Mutex::new(Vec::new());
    static SECOND: Mutex<Vec<String>> = Mutex::new(Vec::new());
    trace_with(|line| FIRST.lock().unwrap().push(line.to_string()));
    trace_with(|line| SECOND.lock().unwrap().push(line.to_string()));
    trace("a step of the drag");
    assert!(FIRST.lock().unwrap().iter().any(|l| l == "a step of the drag"));
    assert!(SECOND.lock().unwrap().is_empty(), "a second trace heard it");
}

/// A drag's answer is none once one file can't be made, even with others
/// still being made: "all or nothing" (`settled`'s doc: "else, if one won't
/// be, why"), and the first failure is the one said.
#[test]
fn the_first_file_that_cannot_be_made_is_the_one_said() {
    let failed = |why: &str| State::Failed(why.into());
    assert_eq!(settled(vec![failed("a"), failed("b"), State::Making(None)]), Some(Err("a".into())));
    assert_eq!(
        settled(vec![State::Making(None), failed("b")]),
        Some(Err("b".into())),
        "though one is still being made"
    );
}
