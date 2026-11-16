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
//!   read before `fetch` returns. [`windows::links`] reads the web links of
//!   a link dragged from a browser (UniformResourceLocatorW), or of text
//!   that is links only.
//! - **macOS** ([`macos`]): give it the `NSDraggingInfo` of
//!   `performDragOperation:` (and of `draggingEntered:`, to decide).
//!   [`macos::paths`] reads the file URLs; [`macos::items`] reads the
//!   NSFilePromiseReceivers, named by the name each source suggests (a
//!   promise's real name is known only once it's received), no size.
//!   [`macos::fetch`] calls `receivePromisedFilesAtDestination` and ends each
//!   item from its reader block, on a queue of its own: a folder destination
//!   has the file moved in, a sink is fed from it. Bytes so far are known
//!   only at the end, and a cancel waits for the source's file.
//!   [`macos::links`] reads the web links a drag offers (its URLs, or text
//!   that is links only).
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
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
mod links;

#[cfg(feature = "fake")]
pub mod fake;
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

/// What the crate's tests share: a fresh folder and a sink that keeps what it's given.
#[cfg(test)]
pub(crate) mod testkit {
    use super::*;
    use std::sync::Mutex;

    /// A fresh, empty folder of this test's own (made), under the temp folder.
    pub(crate) fn fresh(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dragndrop-target-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The names in `dir`, sorted.
    pub(crate) fn names(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<_> =
            std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    /// What a test's sink was given: its bytes, and how it finished (None until it has).
    pub(crate) type Given = (Vec<u8>, Option<Result<u64, String>>);

    /// A sink that keeps what it's given.
    #[derive(Clone, Default)]
    pub(crate) struct Kept(pub(crate) Arc<Mutex<Given>>);

    impl Kept {
        pub(crate) fn given(&self) -> Given {
            self.0.lock().unwrap().clone()
        }
    }

    impl Sink for Kept {
        fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.0.lock().unwrap().0.extend_from_slice(bytes);
            Ok(())
        }
        fn finish(self: Box<Self>, result: Result<u64, Error>) {
            let mut given = self.0.lock().unwrap();
            assert!(given.1.is_none(), "an item finishes once");
            given.1 = Some(result.map_err(|e| e.to_string()));
        }
    }

    pub(crate) fn item(name: &str, size: Option<u64>) -> Item {
        Item { name: name.into(), size, index: 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;
    use std::sync::mpsc;

    /// The shared state of one item going to `kept`, and its job.
    fn one(size: Option<u64>) -> (Arc<Shared>, Job, Fetch, Kept) {
        let kept = Kept::default();
        let (shared, mut jobs) = Shared::new(vec![(item("a.mp4", size), Destination::sink(kept.clone()))]);
        let fetch = shared.handle(false);
        (shared, jobs.remove(0), fetch, kept)
    }

    /// `Item::name` and `Item::size`: "Its file name as the source gives it, without any folder: not
    /// made safe for a file system"; "Its length in bytes, if the source says."
    #[test]
    fn an_item_says_its_name_as_given_and_its_size_if_known() {
        let (named, unsized_) = (item("a:b.mp4", Some(7)), item("c.mkv", None));
        assert_eq!((named.name(), named.size()), ("a:b.mp4", Some(7)));
        assert_eq!((unsized_.name(), unsized_.size()), ("c.mkv", None));
    }

    /// `Offer`: "Files that are where they are, as paths. Empty with none."
    #[test]
    fn an_offer_of_nothing_has_no_paths_and_no_items() {
        let offer = Offer::default();
        assert!(offer.paths.is_empty() && offer.items.is_empty());
    }

    /// `Fetch::progress`: "How far each item is, in the order asked", `Progress::got`: "Bytes
    /// fetched so far."
    #[test]
    fn progress_counts_the_bytes_each_item_has_been_given() {
        let kept = (Kept::default(), Kept::default());
        let (shared, mut jobs) = Shared::new(vec![
            (item("a.mp4", Some(4)), Destination::sink(kept.0.clone())),
            (item("b.mp4", None), Destination::sink(kept.1.clone())),
        ]);
        let fetch = shared.handle(false);
        let mut b = jobs.remove(1);
        shared.write(&mut b, b"xyz").unwrap();
        assert_eq!(
            fetch.progress(),
            [Progress { got: 0, size: Some(4), ended: false }, Progress { got: 3, size: None, ended: false }]
        );
    }

    /// `Sink`: "Where an item's bytes go, yours: written in order from whichever thread fetches,
    /// then finished once."
    #[test]
    fn a_sink_hears_the_bytes_in_order_then_how_it_ended_once() {
        let (shared, mut job, fetch, kept) = one(Some(4));
        shared.write(&mut job, b"ab").unwrap();
        shared.write(&mut job, b"cd").unwrap();
        assert_eq!(kept.given(), (b"abcd".to_vec(), None));
        shared.end(job, Ok(4));
        assert_eq!(kept.given(), (b"abcd".to_vec(), Some(Ok(4))));
        assert_eq!(fetch.progress(), [Progress { got: 4, size: Some(4), ended: true }]);
    }

    /// `Fetch::is_finished`: "Every item has ended."
    #[test]
    fn a_fetch_is_finished_only_once_every_item_has_ended() {
        let (shared, mut jobs) = Shared::new(vec![
            (item("a.mp4", None), Destination::sink(Kept::default())),
            (item("b.mp4", None), Destination::sink(Kept::default())),
        ]);
        let fetch = shared.handle(false);
        shared.end(jobs.remove(0), Ok(0));
        assert!(!fetch.is_finished());
        shared.end(jobs.remove(0), Ok(0));
        assert!(fetch.is_finished());
    }

    /// `Fetch::cancel`: "the item being read stops at its next bytes, and every item not yet whole
    /// ends with `Error::Canceled`."
    #[test]
    fn a_cancel_stops_the_next_bytes_and_the_item_ends_canceled() {
        let (shared, mut job, fetch, kept) = one(Some(4));
        shared.write(&mut job, b"ab").unwrap();
        fetch.cancel();
        let stopped = shared.write(&mut job, b"cd");
        assert!(matches!(stopped, Err(Error::Canceled)));
        shared.end(job, stopped.map(|()| 4));
        assert_eq!(kept.given(), (b"ab".to_vec(), Some(Err("canceled".into()))));
        assert_eq!(fetch.progress()[0].got, 2, "what was refused isn't counted");
    }

    /// `Fetch`: "Keep it until `Fetch::is_finished`: dropped earlier, it cancels what's left."
    #[test]
    fn a_fetch_dropped_before_it_finishes_cancels() {
        let (shared, mut job, fetch, _) = one(None);
        drop(fetch);
        assert!(matches!(shared.write(&mut job, b"x"), Err(Error::Canceled)));
    }

    /// `Sink::write`: "An error stops this item, which then finishes with `Error::Write`."
    #[test]
    fn a_sink_that_fails_a_write_stops_its_item_with_a_write_error() {
        struct Refuses(mpsc::Sender<String>);
        impl Sink for Refuses {
            fn write(&mut self, _: &[u8]) -> io::Result<()> {
                Err(io::Error::other("disk full"))
            }
            fn finish(self: Box<Self>, result: Result<u64, Error>) {
                self.0.send(result.unwrap_err().to_string()).unwrap();
            }
        }
        let (tell, heard) = mpsc::channel();
        let (shared, mut jobs) = Shared::new(vec![(item("a.mp4", None), Destination::sink(Refuses(tell)))]);
        let fetch = shared.handle(false);
        let mut job = jobs.remove(0);
        let failed = shared.write(&mut job, b"x");
        assert!(matches!(failed, Err(Error::Write(_))));
        shared.end(job, failed.map(|()| 1));
        assert_eq!(heard.recv().unwrap(), "it couldn't be written: disk full");
        assert_eq!(fetch.progress()[0].got, 0);
    }

    /// `Destination::folder`: "`done` hears the file's path, or why it isn't there."
    #[test]
    fn a_folder_destination_tells_its_callback_the_file_it_wrote() {
        let dir = fresh("lib-folder");
        let (tell, heard) = mpsc::channel();
        let (shared, mut jobs) =
            Shared::new(vec![(item("a.mp4", None), Destination::folder(&dir, move |r| tell.send(r).unwrap()))]);
        let mut job = jobs.remove(0);
        shared.write(&mut job, b"abc").unwrap();
        shared.end(job, Ok(3));
        assert_eq!(heard.recv().unwrap().unwrap(), dir.join("a.mp4"));
        assert_eq!(std::fs::read(dir.join("a.mp4")).unwrap(), b"abc");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `Fetch::in_background`: "It's read on a thread of its own ... False when it was all read
    /// before `fetch` returned."
    #[test]
    fn a_fetch_says_whether_it_is_read_in_the_background() {
        let (shared, _) = Shared::new(vec![]);
        assert!(shared.handle(true).in_background());
        assert!(!shared.handle(false).in_background());
    }

    /// `Error`'s `Display`, which the app shows when an item fails.
    #[test]
    fn why_an_item_failed_reads_as_a_sentence() {
        assert_eq!(Error::Canceled.to_string(), "canceled");
        assert_eq!(Error::Source("refused".into()).to_string(), "the app it came from didn't give it: refused");
        assert_eq!(Error::Write(io::Error::other("full")).to_string(), "it couldn't be written: full");
    }

    /// `Error`: "Write: It couldn't be written: the sink's error, or the folder's." keeps the cause.
    #[test]
    fn a_write_error_keeps_its_cause_and_the_others_have_none() {
        use std::error::Error as _;
        assert!(Error::Write(io::Error::other("full")).source().is_some());
        assert!(Error::Canceled.source().is_none() && Error::Source("x".into()).source().is_none());
    }
}
