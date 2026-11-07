//! KDE's way of taking files not made yet (Ark's, which Dolphin takes), for
//! [`crate::linux`]: the drag names a D-Bus object of yours (two more
//! targets: your name on the session bus, the object's path); let go on
//! Dolphin, it asks the object to put the files in its folder
//! (`extractSelectedFilesTo`), its drop done at once, and each file is
//! copied there as it's made, one by one.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use gtk::{gio, glib};

use crate::linux::TICK;
use crate::{Item, State, trace};

/// The drag's targets for it, by their `info`.
pub(crate) const SERVICE: u32 = 1;
pub(crate) const OBJECT: u32 = 2;
pub(crate) const SERVICE_TARGET: &str = "application/x-kde-ark-dndextract-service";
pub(crate) const OBJECT_TARGET: &str = "application/x-kde-ark-dndextract-path";

const INTERFACE: &str = r#"<node><interface name="org.kde.ark.DndExtract"><method name="extractSelectedFilesTo"><arg type="s" name="path" direction="in"/></method></interface></node>"#;

/// What the copies came to, once they're over: why not, if one failed.
type Then = Box<dyn FnOnce(Option<String>) + Send>;

#[derive(Default)]
struct Copies {
    /// The receiver asked for the files.
    asked: bool,
    /// The copies are over (why not, if they failed), and nobody told yet.
    over: Option<Option<String>>,
    /// What to do once they are (the drag over by then).
    then: Option<Then>,
}

/// One drag's object on the bus, while it's offered.
pub(crate) struct Offer {
    conn: gio::DBusConnection,
    id: Mutex<Option<gio::RegistrationId>>,
    /// Your name on the session bus.
    pub(crate) service: String,
    /// The object's path, this drag's own.
    pub(crate) path: String,
    copies: Arc<Mutex<Copies>>,
}

impl Offer {
    /// Puts the drag's object on the bus: asked, it copies `items` into the
    /// folder named as each is made, one by one, and tells those still being
    /// made that they're waited for. None without a bus.
    pub(crate) fn new(items: Arc<Vec<Box<dyn Item>>>) -> Option<Arc<Offer>> {
        static DRAGS: AtomicU32 = AtomicU32::new(0);
        let conn = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()?;
        let service = conn.unique_name()?.to_string();
        let path = format!("/DndExtract/{}", DRAGS.fetch_add(1, Ordering::Relaxed) + 1);
        let node = gio::DBusNodeInfo::for_xml(INTERFACE).ok()?;
        let interface = node.lookup_interface("org.kde.ark.DndExtract")?;
        let copies = Arc::new(Mutex::new(Copies::default()));
        let asked = copies.clone();
        let id = conn
            .register_object(
                &path,
                &interface,
                move |_, _, _, _, _, params, invocation| {
                    let folder: String = params.child_value(0).get().unwrap_or_default();
                    // Once: a second call would copy them all again.
                    let first = !std::mem::replace(&mut asked.lock().unwrap().asked, true);
                    invocation.return_value(None);
                    if first {
                        trace(&format!("Ark: asked to put them in {folder}"));
                        crate::linux::waited(&items);
                        let (items, copies) = (items.clone(), asked.clone());
                        std::thread::spawn(move || {
                            let result = copy_all(&items, Path::new(&folder));
                            let mut c = copies.lock().unwrap();
                            match c.then.take() {
                                Some(then) => {
                                    drop(c);
                                    then(result.err());
                                }
                                None => c.over = Some(result.err()),
                            }
                        });
                    }
                },
                |_, _, _, _, _| glib::Variant::from(""),
                |_, _, _, _, _, _| false,
            )
            .ok()?;
        Some(Arc::new(Offer { conn, id: Mutex::new(Some(id)), service, path, copies }))
    }

    /// The drag is over: `then` runs once the files went (why not, if they
    /// didn't), if the receiver asked for them, and true; else false, and
    /// the object goes from the bus.
    pub(crate) fn ended(self: &Arc<Self>, then: impl FnOnce(Option<String>) + Send + 'static) -> bool {
        let mut c = self.copies.lock().unwrap();
        if !c.asked {
            drop(c);
            self.withdraw();
            return false;
        }
        let me = self.clone();
        let then = move |failed| {
            me.withdraw();
            then(failed);
        };
        match c.over.take() {
            Some(failed) => {
                drop(c);
                then(failed);
            }
            None => c.then = Some(Box::new(then)),
        }
        true
    }

    /// Takes the object off the bus.
    fn withdraw(&self) {
        if let Some(id) = self.id.lock().unwrap().take() {
            let _ = self.conn.unregister_object(id);
        }
    }
}

/// Copies each of `items` into `folder` once it's made, in their order, each
/// written beside and renamed, named as its file (another name if one's
/// there); stops at one that can't be.
fn copy_all(items: &[Box<dyn Item>], folder: &Path) -> Result<(), String> {
    let mut done = 0;
    loop {
        for item in items.iter().skip(done) {
            match item.state() {
                State::Ready(path) => {
                    copy_one(&path, folder).map_err(|e| format!("Couldn't put it in {}: {e}", folder.display()))?;
                    done += 1;
                }
                State::Failed(why) => return Err(why),
                State::Making(_) => break,
            }
        }
        if done == items.len() {
            return Ok(());
        }
        std::thread::sleep(TICK);
    }
}

fn copy_one(from: &Path, folder: &Path) -> std::io::Result<PathBuf> {
    let name = from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "clip".into());
    let part = folder.join(format!(".{name}.part"));
    std::fs::copy(from, &part)?;
    let to = unique_path(folder, &name);
    std::fs::rename(&part, &to).inspect_err(|_| {
        let _ = std::fs::remove_file(&part);
    })?;
    trace(&format!("Ark: {} put in the folder", to.display()));
    Ok(to)
}

/// `name` in `dir`, or with " (2)", " (3)"… before its extension if taken.
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    if !path.exists() {
        return path;
    }
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    (2..).map(|n| dir.join(format!("{stem} ({n}).{ext}"))).find(|p| !p.exists()).expect("a free name")
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dragndrop-source-ark-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("made")).unwrap();
        std::fs::create_dir_all(dir.join("folder")).unwrap();
        dir
    }

    /// Each file goes into the folder as it's made, in order, under its own
    /// name, numbered if one's there; the copies end once the last is in.
    #[test]
    fn the_files_are_copied_into_the_folder_as_they_are_made() {
        let dir = dir("copied");
        let (a, b) = (dir.join("made/a.mp4"), dir.join("made/b.mp4"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        std::fs::write(dir.join("folder/a.mp4"), "theirs").unwrap();
        let made_b = Arc::new(Mutex::new(None));
        let items: Vec<Box<dyn Item>> = vec![
            Box::new(Test { path: a, ready: Arc::new(Mutex::new(Some(Ok(())))) }),
            Box::new(Test { path: b, ready: made_b.clone() }),
        ];
        let folder = dir.join("folder");
        let copying = std::thread::spawn(move || copy_all(&items, &folder));
        std::thread::sleep(TICK * 3);
        assert!(!copying.is_finished(), "over before the second was made");
        assert_eq!(std::fs::read_to_string(dir.join("folder/a (2).mp4")).unwrap(), "a", "the first, numbered");
        *made_b.lock().unwrap() = Some(Ok(()));
        assert_eq!(copying.join().unwrap(), Ok(()));
        assert_eq!(std::fs::read_to_string(dir.join("folder/b.mp4")).unwrap(), "b");
        assert_eq!(std::fs::read_to_string(dir.join("folder/a.mp4")).unwrap(), "theirs", "theirs kept");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One that won't be made stops the copies, and says why.
    #[test]
    fn a_file_that_fails_stops_the_copies() {
        let dir = dir("failed");
        let failed = Arc::new(Mutex::new(Some(Err("blocked".to_string()))));
        let items: Vec<Box<dyn Item>> = vec![Box::new(Test { path: dir.join("made/a.mp4"), ready: failed })];
        assert_eq!(copy_all(&items, &dir.join("folder")), Err("blocked".into()));
        assert_eq!(std::fs::read_dir(dir.join("folder")).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
