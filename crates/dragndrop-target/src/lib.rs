//! What a drag dropped on your window offers: the plain paths, and the
//! virtual items that have no path yet (a file inside a zip folder in
//! Explorer, an Outlook attachment, a Finder or Photos file promise), with
//! each item's bytes fetched into a folder or a writer of yours.
//!
//! Data only. You register your own drop target and handle its events (wry,
//! winit, your toolkit already do); this crate never registers one, owns an
//! event loop or starts a drag. From inside your drop handler you hand it the
//! native drag object you were given. It has no async runtime: what is
//! asynchronous is a handshake done inside that call, then a [`Fetch`] you
//! keep until it ends, with its progress and a cancel.
//!
//! # On each system
//!
//! - **Windows** ([`windows`]): give it the `IDataObject` your `IDropTarget`
//!   got. [`windows::paths`] reads CF_HDROP, [`windows::items`] reads
//!   FileGroupDescriptorW (names, sizes when given). [`windows::fetch`],
//!   called inside `Drop`, reads each item's FileContents (a stream or a
//!   memory block). If the source copies asynchronously
//!   (IDataObjectAsyncCapability, as Explorer does), the operation is started
//!   there and the reading goes on a thread of its own, so `Drop` returns at
//!   once and the source shows its own copy progress; otherwise everything is
//!   read before `fetch` returns.
//! - **macOS** ([`macos`]): give it the `NSDraggingInfo` of
//!   `performDragOperation:` (and of `draggingEntered:`, to decide).
//!   [`macos::paths`] reads the file URLs; [`macos::items`] reads the
//!   NSFilePromiseReceivers, named by the name each source suggests (a
//!   promise's real name is known only once it's received), no size.
//!   [`macos::fetch`] calls `receivePromisedFilesAtDestination` and ends each
//!   item from its reader block, on a queue of its own: a folder destination
//!   has the file moved in, a sink is fed from it. Bytes so far are known
//!   only at the end, and a cancel waits for the source's file.
//! - **Linux** ([`linux`], GTK 3): give it the `GdkDragContext` of your
//!   widget's drag signals. [`linux::paths`] reads text/uri-list (from your
//!   `drag-data-received`), [`linux::items`] an XdndDirectSave offer (in
//!   `drag-motion`). At the drop, [`linux::receive`] asks the source and
//!   hears back on GTK's thread: the paths (text/uri-list, else the files of
//!   `application/vnd.portal.filetransfer`, which the document portal gives
//!   as files on disk), or the files an XDS source saved into a folder you
//!   name (archive managers write each file under its own name), as items.
//!   It finishes the drag (`gtk_drag_finish`) once the source is done.
//!   [`linux::fetch`] then moves each item into a folder, or copies it into
//!   a sink, on a thread of its own. Saving has no progress or cancel (the
//!   source does it); the move or copy has both.
//!
//! On every system you get the same things back: [`Item`]s (a name, a size if
//! known), a [`Destination`] per item you take (a folder, or a [`Sink`] of
//! yours), and a [`Fetch`] whose items each end once, through the folder's
//! callback or the sink's [`Sink::finish`]. A drag may offer both paths and
//! items, usually the same files twice; the paths are better (nothing to
//! copy), unless they stand in for files of another type, as Photos' JPEG of
//! a video's frame beside its promise of the movie ([`macos`] says which
//! with `promises_preferred`).

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod choice;
#[cfg_attr(not(windows), allow(dead_code))]
mod descriptor;
mod folder;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;

pub use folder::safe_file_name;

/// A file a drag offers with no path yet: its bytes come from the source
/// when fetched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    name: String,
    size: Option<u64>,
    /// Its place in the drag, as the platform counts (FileContents' index on
    /// Windows, the promise's on macOS, the saved file's on Linux).
    #[cfg_attr(not(any(windows, target_os = "macos", target_os = "linux")), allow(dead_code))]
    index: usize,
}

impl Item {
    /// Its file name as the source gives it, without any folder: not made
    /// safe for a file system (see [`safe_file_name`]).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its length in bytes, if the source says.
    pub fn size(&self) -> Option<u64> {
        self.size
    }
}

/// Everything a drag offers, as read from it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Offer {
    /// Files that are where they are, as paths. Empty with none.
    pub paths: Vec<PathBuf>,
    /// Virtual files, to be fetched.
    pub items: Vec<Item>,
}

/// Where a fetched item's bytes go.
pub struct Destination(pub(crate) To);

pub(crate) enum To {
    Folder { dir: PathBuf, done: Box<dyn FnOnce(Result<PathBuf, Error>) + Send> },
    Sink(Box<dyn Sink>),
}

impl Destination {
    /// Into folder `dir` (made if needed), under the item's name made safe
    /// ([`safe_file_name`]) and numbered if taken ("name (2).ext"). It's
    /// written as "<name>.part" and renamed once whole; nothing is left when
    /// it isn't. `done` hears the file's path, or why it isn't there.
    pub fn folder(dir: impl Into<PathBuf>, done: impl FnOnce(Result<PathBuf, Error>) + Send + 'static) -> Self {
        Destination(To::Folder { dir: dir.into(), done: Box::new(done) })
    }

    /// Through `sink`, which hears the bytes in order, then how it ended.
    pub fn sink(sink: impl Sink + 'static) -> Self {
        Destination(To::Sink(Box::new(sink)))
    }
}

/// Where an item's bytes go, yours: written in order from whichever thread
/// fetches (perhaps the one that called `fetch`), then finished once.
pub trait Sink: Send {
    /// The next of its bytes. An error stops this item, which then finishes
    /// with [`Error::Write`].
    fn write(&mut self, bytes: &[u8]) -> io::Result<()>;
    /// It ended: whole (its length), or not and why. Called exactly once.
    fn finish(self: Box<Self>, result: Result<u64, Error>);
}

/// Why an item wasn't fetched.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The fetch was canceled ([`Fetch::cancel`], or the [`Fetch`] dropped).
    Canceled,
    /// The source didn't give it: refused, let go of, or in a form not read.
    Source(String),
    /// It couldn't be written: the sink's error, or the folder's.
    Write(io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Canceled => write!(f, "canceled"),
            Error::Source(why) => write!(f, "the app it came from didn't give it: {why}"),
            Error::Write(e) => write!(f, "it couldn't be written: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Write(e) => Some(e),
            _ => None,
        }
    }
}

/// How far one item of a [`Fetch`] is.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Progress {
    /// Bytes fetched so far.
    pub got: u64,
    /// Its length, if the source said.
    pub size: Option<u64>,
    /// It ended (whole or not).
    pub ended: bool,
}

/// The items of one drop being fetched. Keep it until [`Fetch::is_finished`]:
/// dropped earlier, it cancels what's left.
pub struct Fetch {
    shared: Arc<Shared>,
    background: bool,
}

impl Fetch {
    /// How far each item is, in the order asked.
    pub fn progress(&self) -> Vec<Progress> {
        self.shared
            .items
            .iter()
            .map(|i| Progress {
                got: i.got.load(Ordering::Relaxed),
                size: i.size,
                ended: i.ended.load(Ordering::SeqCst),
            })
            .collect()
    }

    /// Stops it: the item being read stops at its next bytes, and every item
    /// not yet whole ends with [`Error::Canceled`]. The source is told the
    /// drop failed.
    pub fn cancel(&self) {
        self.shared.canceled.store(true, Ordering::SeqCst);
    }

    /// Every item has ended.
    pub fn is_finished(&self) -> bool {
        self.shared.items.iter().all(|i| i.ended.load(Ordering::SeqCst))
    }

    /// It's read on a thread of its own: the drop handler has returned to the
    /// source, which waits for it (Explorer's copy dialog). False when it was
    /// all read before `fetch` returned.
    pub fn in_background(&self) -> bool {
        self.background
    }
}

impl Drop for Fetch {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// What a fetch and its reading share.
pub(crate) struct Shared {
    pub(crate) canceled: AtomicBool,
    pub(crate) items: Vec<Slot>,
}

/// One item's progress.
pub(crate) struct Slot {
    pub(crate) got: AtomicU64,
    pub(crate) size: Option<u64>,
    pub(crate) ended: AtomicBool,
}

/// One item to read: where it is in the drag, where it goes, its progress.
#[cfg_attr(not(any(windows, target_os = "macos", target_os = "linux")), allow(dead_code))]
pub(crate) struct Job {
    pub(crate) item: Item,
    pub(crate) sink: Box<dyn Sink>,
    pub(crate) slot: usize,
}

#[cfg_attr(not(any(windows, target_os = "macos", target_os = "linux")), allow(dead_code))]
impl Shared {
    /// The shared state of `requests`, and each as a job: a folder
    /// destination becomes the sink that writes its file.
    pub(crate) fn new(requests: Vec<(Item, Destination)>) -> (Arc<Shared>, Vec<Job>) {
        let items = requests
            .iter()
            .map(|(item, _)| Slot { got: AtomicU64::new(0), size: item.size, ended: AtomicBool::new(false) })
            .collect();
        let jobs = requests
            .into_iter()
            .enumerate()
            .map(|(slot, (item, to))| {
                let sink = match to.0 {
                    To::Sink(sink) => sink,
                    To::Folder { dir, done } => Box::new(folder::FolderSink::new(dir, &item.name, done)),
                };
                Job { item, sink, slot }
            })
            .collect();
        (Arc::new(Shared { canceled: AtomicBool::new(false), items }), jobs)
    }

    pub(crate) fn canceled(&self) -> bool {
        self.canceled.load(Ordering::SeqCst)
    }

    /// Ends `job` with `result`, its one finish.
    pub(crate) fn end(&self, job: Job, result: Result<u64, Error>) {
        job.sink.finish(result);
        self.items[job.slot].ended.store(true, Ordering::SeqCst);
    }

    /// Writes `bytes` of `job`, unless the fetch is canceled.
    pub(crate) fn write(&self, job: &mut Job, bytes: &[u8]) -> Result<(), Error> {
        if self.canceled() {
            return Err(Error::Canceled);
        }
        job.sink.write(bytes).map_err(Error::Write)?;
        self.items[job.slot].got.fetch_add(bytes.len() as u64, Ordering::Relaxed);
        Ok(())
    }

    /// The handle on this fetch.
    pub(crate) fn handle(self: &Arc<Self>, background: bool) -> Fetch {
        Fetch { shared: self.clone(), background }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// What a test's sink was given: its bytes, and how it finished.
    type Given = (Vec<u8>, Option<Result<u64, String>>);

    /// A sink that keeps what it's given, for the tests.
    struct Kept(Arc<Mutex<Given>>);

    impl Sink for Kept {
        fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.0.lock().unwrap().0.extend_from_slice(bytes);
            Ok(())
        }
        fn finish(self: Box<Self>, result: Result<u64, Error>) {
            self.0.lock().unwrap().1 = Some(result.map_err(|e| e.to_string()));
        }
    }

    fn item(name: &str, size: Option<u64>) -> Item {
        Item { name: name.into(), size, index: 0 }
    }

    #[test]
    fn progress_is_counted_and_a_cancel_stops_the_next_bytes() {
        let kept = Arc::new(Mutex::new((vec![], None)));
        let (shared, mut jobs) = Shared::new(vec![(item("a.mp4", Some(4)), Destination::sink(Kept(kept.clone())))]);
        let fetch = shared.handle(false);
        let mut job = jobs.remove(0);
        shared.write(&mut job, b"ab").unwrap();
        assert_eq!(fetch.progress(), [Progress { got: 2, size: Some(4), ended: false }]);
        assert!(!fetch.is_finished());
        fetch.cancel();
        let stopped = shared.write(&mut job, b"cd");
        assert!(matches!(stopped, Err(Error::Canceled)));
        shared.end(job, stopped.map(|()| 4));
        assert!(fetch.is_finished());
        assert_eq!(*kept.lock().unwrap(), (b"ab".to_vec(), Some(Err("canceled".into()))));
    }

    #[test]
    fn a_fetch_dropped_cancels() {
        let (shared, _jobs) = Shared::new(vec![]);
        drop(shared.handle(true));
        assert!(shared.canceled());
    }
}
