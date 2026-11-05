//! macOS: what a drag's `NSDraggingInfo` offers, and its file promises
//! received.
//!
//! Call these from your `NSDraggingDestination`, on the main thread: [`paths`]
//! and [`items`] (or [`offer`]) in `draggingEntered:` to decide whether to
//! take the drag, again in `performDragOperation:`, then [`fetch`] there with
//! the items you take. Your view must be registered for
//! `NSFilePromiseReceiver.readableDraggedTypes` (as well as file URLs) to be
//! asked about promises at all.
//!
//! The paths are the pasteboard's file URLs. The items are its
//! `NSFilePromiseReceiver`s, one each. A promise has no name until it's
//! received, so an item's name is the one its source suggests as the drag
//! comes (asked of the source then), else "file" with its type's extension;
//! never a size. [`fetch`] asks each source for its file
//! (`receivePromisedFilesAtDestination`) in a folder of its own made for it,
//! and each item ends from its reader block, on an operation queue of the
//! fetch's: a folder destination has the file moved in (beside the folder,
//! on its volume, so nothing is copied), a sink is fed from it. The source
//! writes the file in its own time and can't be stopped, so a cancel ends an
//! item only once its file comes; bytes so far are known only at the end.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};

use block2::RcBlock;
use objc2::ClassType;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ProtocolObject};
use objc2_app_kit::{NSDraggingInfo, NSFilePromiseReceiver, NSPasteboard, NSPasteboardTypeFileURL, NSWorkspace};
use objc2_foundation::{
    NSArray, NSDictionary, NSError, NSFileManager, NSOperationQueue, NSSearchPathDirectory, NSSearchPathDomainMask,
    NSString, NSURL,
};

use crate::folder::FolderSink;
use crate::{Destination, Error, Fetch, Item, Job, Offer, Shared, Sink, To};

/// How much of a received file is read at a time, for a sink.
const CHUNK: usize = 1 << 20;

/// The pasteboard type a promise's source answers with its file's name, as
/// the drag comes (AppKit's own, as NSFilePromiseProvider gives it).
const SUGGESTED_NAME: &str = "com.apple.pasteboard.promised-suggested-file-name";
/// The type an NSFilePromiseProvider's item carries, one per promise.
const PROMISE_ITEM: &str = "com.apple.NSFilePromiseItemMetaData";

/// The files `info` offers as paths (its file URLs), or None if it offers
/// none. Some and empty is a drag that has file URLs but none to read yet (a
/// source that makes its files late may answer at the drop).
pub fn paths(info: &ProtocolObject<dyn NSDraggingInfo>) -> Option<Vec<PathBuf>> {
    let pasteboard = info.draggingPasteboard();
    // SAFETY: a static AppKit string.
    let url = unsafe { NSPasteboardTypeFileURL };
    pasteboard.availableTypeFromArray(&NSArray::from_slice(&[url]))?;
    let read = read(&pasteboard, NSURL::class());
    Some(
        read.iter()
            .filter_map(|url| url.downcast_ref::<NSURL>().filter(|url| url.isFileURL())?.to_file_path())
            .collect(),
    )
}

/// The file promises `info` offers, in order.
pub fn items(info: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<Item> {
    let pasteboard = info.draggingPasteboard();
    let receivers = receivers(&pasteboard);
    let mut suggested = suggested_names(&pasteboard);
    if suggested.len() != receivers.len() {
        suggested = vec![None; receivers.len()];
    }
    receivers
        .iter()
        .zip(suggested)
        .enumerate()
        .map(|(index, (receiver, name))| Item {
            name: name.filter(|n| !n.is_empty()).unwrap_or_else(|| typed_name(receiver)),
            size: None,
            index,
        })
        .collect()
}

/// Both [`paths`] (empty with none) and [`items`].
pub fn offer(info: &ProtocolObject<dyn NSDraggingInfo>) -> Offer {
    Offer { paths: paths(info).unwrap_or_default(), items: items(info) }
}

/// The pasteboard types `info` offers: for a trace of what a source sends.
pub fn formats(info: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<String> {
    let types = info.draggingPasteboard().types();
    types.map(|types| types.iter().map(|t| t.to_string()).collect()).unwrap_or_default()
}

/// Fetches `requests`, items of `info` and where each goes; call it inside
/// your `performDragOperation:`, with the `info` it was given. It returns at
/// once: each item's source is asked for its file, and the item ends when
/// the file comes (or the source says it won't), on a queue of the fetch's.
/// Keep the [`Fetch`] until it finishes: dropped, it cancels. Every item ends
/// exactly once.
pub fn fetch(info: &ProtocolObject<dyn NSDraggingInfo>, requests: Vec<(Item, Destination)>) -> Fetch {
    let (shared, jobs) = prepare(requests);
    if jobs.is_empty() {
        return shared.handle(false);
    }
    let receivers = receivers(&info.draggingPasteboard());
    let queue = NSOperationQueue::new();
    for (job, folder) in jobs {
        let Some(receiver) = receivers.get(job.item.index) else {
            shared.end(job, Err(Error::Source("no such promise".into())));
            continue;
        };
        let near = folder.as_ref().map(|(dir, _)| dir.clone()).unwrap_or_else(std::env::temp_dir);
        let at = match receiving_folder(&near) {
            Ok(at) => at,
            Err(e) => {
                shared.end(job, Err(e));
                continue;
            }
        };
        let Some(at_url) = NSURL::from_directory_path(&at) else {
            let _ = std::fs::remove_dir_all(&at);
            shared.end(job, Err(Error::Source(format!("{} can't be named to it", at.display()))));
            continue;
        };
        let waiting = Arc::new(Waiting {
            job: Mutex::new(Some(job)),
            into: folder.map(|(_, into)| into),
            at,
            shared: shared.clone(),
        });
        let reader = RcBlock::new(move |url: NonNull<NSURL>, error: *mut NSError| {
            // SAFETY: AppKit gives the file's URL, and an error or null.
            let (url, error) = unsafe { (url.as_ref(), error.as_ref()) };
            waiting.read(url, error);
        });
        // SAFETY: the receiver is the drag's, asked on its thread inside the
        // drop; the reader runs on `queue`, which AppKit holds until it's done.
        unsafe {
            receiver.receivePromisedFilesAtDestination_options_operationQueue_reader(
                &at_url,
                &NSDictionary::<AnyObject, AnyObject>::new(),
                &queue,
                &reader,
            );
        }
    }
    shared.handle(true)
}

/// A folder destination's sink, within reach of the reader.
type Folder = Arc<Mutex<Option<FolderSink>>>;

/// A job, with the folder it goes into, if one.
type Prepared = (Job, Option<(PathBuf, Folder)>);

/// The shared state of `requests`, and each as a job, with its folder if it
/// goes into one: kept within reach, to move the file in.
fn prepare(requests: Vec<(Item, Destination)>) -> (Arc<Shared>, Vec<Prepared>) {
    let mut folders = vec![];
    let requests: Vec<_> = requests
        .into_iter()
        .map(|(item, to)| match to.0 {
            To::Folder { dir, done } => {
                let into = Arc::new(Mutex::new(Some(FolderSink::new(dir.clone(), &item.name, done))));
                folders.push(Some((dir, into.clone())));
                (item, Destination::sink(Moving(into)))
            }
            To::Sink(sink) => {
                folders.push(None);
                (item, Destination(To::Sink(sink)))
            }
        })
        .collect();
    let (shared, jobs) = Shared::new(requests);
    (shared, jobs.into_iter().zip(folders).collect())
}

/// The objects of class `class` the pasteboard has.
fn read(pasteboard: &NSPasteboard, class: &AnyClass) -> Vec<Retained<AnyObject>> {
    // SAFETY: the array holds a class AppKit reads objects of.
    let read = unsafe { pasteboard.readObjectsForClasses_options(&NSArray::from_slice(&[class]), None) };
    read.map(|read| read.to_vec()).unwrap_or_default()
}

fn receivers(pasteboard: &NSPasteboard) -> Vec<Retained<NSFilePromiseReceiver>> {
    let read = read(pasteboard, NSFilePromiseReceiver::class());
    read.into_iter().filter_map(|r| r.downcast::<NSFilePromiseReceiver>().ok()).collect()
}

/// The name each promise's source suggests, in order, read from the
/// pasteboard's items that are promises (empty for a source that gives
/// promises another way, as the older API's one item for many).
fn suggested_names(pasteboard: &NSPasteboard) -> Vec<Option<String>> {
    let (item, name) = (NSString::from_str(PROMISE_ITEM), NSString::from_str(SUGGESTED_NAME));
    let Some(items) = pasteboard.pasteboardItems() else { return vec![] };
    items
        .iter()
        .filter(|i| i.types().containsObject(&item))
        .map(|i| i.stringForType(&name).map(|n| n.to_string()))
        .collect()
}

/// A name for a promise whose source suggests none: "file" with its type's
/// extension.
fn typed_name(receiver: &NSFilePromiseReceiver) -> String {
    let types = receiver.fileTypes();
    #[allow(deprecated)] // (UTType's, its successor, is Swift's and newer.)
    let ext = types.iter().find_map(|t| NSWorkspace::sharedWorkspace().preferredFilenameExtensionForType(&t));
    match ext {
        Some(ext) => format!("file.{ext}"),
        None => "file".into(),
    }
}

/// A new folder for one promise's file, near `dir` (on its volume): the
/// system's place for a file about to replace another.
fn receiving_folder(dir: &Path) -> Result<PathBuf, Error> {
    let near = NSURL::from_directory_path(dir).ok_or_else(|| Error::Write(std::io::ErrorKind::InvalidInput.into()))?;
    let made = NSFileManager::defaultManager().URLForDirectory_inDomain_appropriateForURL_create_error(
        NSSearchPathDirectory::ItemReplacementDirectory,
        NSSearchPathDomainMask::UserDomainMask,
        Some(&near),
        true,
    );
    let made = made.map_err(|e| Error::Write(std::io::Error::other(e.localizedDescription().to_string())))?;
    made.to_file_path().ok_or_else(|| Error::Write(std::io::ErrorKind::InvalidInput.into()))
}

/// A folder destination's sink, also within reach of the reader to move the
/// file in.
struct Moving(Folder);

impl Sink for Moving {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self.0.lock().unwrap().as_mut() {
            Some(sink) => sink.write(bytes),
            None => Err(std::io::ErrorKind::BrokenPipe.into()),
        }
    }

    fn finish(self: Box<Self>, result: Result<u64, Error>) {
        if let Some(sink) = self.0.lock().unwrap().take() {
            Box::new(sink).finish(result);
        }
    }
}

/// One promise waited for: its job until the file comes.
struct Waiting {
    job: Mutex<Option<Job>>,
    /// Its folder destination's sink, to move the file in.
    into: Option<Folder>,
    /// The folder it's received in.
    at: PathBuf,
    shared: Arc<Shared>,
}

impl Waiting {
    /// The reader block: file `url` came, or `error` instead. A promise of
    /// several files (the older API's) is taken as its first.
    fn read(&self, url: &NSURL, error: Option<&NSError>) {
        let file = url.to_file_path();
        let Some(mut job) = self.job.lock().unwrap().take() else {
            if let Some(file) = file {
                let _ = std::fs::remove_file(file);
            }
            return;
        };
        let result = match (error, file) {
            (Some(e), _) => Err(Error::Source(e.localizedDescription().to_string())),
            _ if self.shared.canceled() => Err(Error::Canceled),
            (None, None) => Err(Error::Source("it gave no file".into())),
            (None, Some(file)) => self.take(&mut job, &file),
        };
        self.shared.end(job, result);
        let _ = std::fs::remove_dir_all(&self.at);
    }

    /// File `file` taken as `job`: moved in, else read into its sink.
    fn take(&self, job: &mut Job, file: &Path) -> Result<u64, Error> {
        if let Some(into) = &self.into {
            if let Some(Ok(len)) = into.lock().unwrap().as_mut().map(|sink| sink.adopt(file)) {
                self.shared.items[job.slot].got.store(len, std::sync::atomic::Ordering::Relaxed);
                return Ok(len);
            }
        }
        let mut from = std::fs::File::open(file).map_err(|e| Error::Source(format!("its file can't be read: {e}")))?;
        let mut buf = vec![0u8; CHUNK];
        let mut total = 0;
        loop {
            let got = from.read(&mut buf).map_err(|e| Error::Source(format!("its file can't be read: {e}")))?;
            if got == 0 {
                return Ok(total);
            }
            self.shared.write(job, &buf[..got])?;
            total += got as u64;
        }
    }
}

impl Drop for Waiting {
    /// AppKit let go of the reader without calling it: the item still ends.
    fn drop(&mut self) {
        if let Some(job) = self.job.get_mut().unwrap().take() {
            self.shared.end(job, Err(Error::Source("it never gave the file".into())));
            let _ = std::fs::remove_dir_all(&self.at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dragndrop-target-macos-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn item(name: &str) -> Item {
        Item { name: name.into(), size: None, index: 0 }
    }

    /// The one job of `requests` waiting for its file, received in a
    /// folder near `near` as a fetch's would be, and a file `name` of
    /// `bytes` given there.
    fn given(near: &Path, requests: Vec<(Item, Destination)>, name: &str, bytes: &[u8]) -> (Fetch, Waiting, PathBuf) {
        let (shared, mut jobs) = prepare(requests);
        let (job, folder) = jobs.remove(0);
        let at = receiving_folder(near).unwrap();
        let file = at.join(name);
        std::fs::write(&file, bytes).unwrap();
        let waiting = Waiting { job: Mutex::new(Some(job)), into: folder.map(|(_, f)| f), at, shared: shared.clone() };
        (shared.handle(true), waiting, file)
    }

    #[test]
    fn a_promise_for_a_folder_is_moved_in_under_the_items_name() {
        let dir = scratch("folder");
        let (tell, heard) = mpsc::channel();
        let to = Destination::folder(&dir, move |r| tell.send(r).unwrap());
        let (fetch, waiting, file) = given(&dir, vec![(item("a:b.mp4"), to)], "as named.mp4", b"whole");
        let at = waiting.at.clone();
        waiting.read(&NSURL::from_file_path(&file).unwrap(), None);
        assert_eq!(heard.recv().unwrap().unwrap(), dir.join("a_b.mp4"));
        assert_eq!(std::fs::read(dir.join("a_b.mp4")).unwrap(), b"whole");
        assert!(!at.exists(), "the receiving folder goes");
        assert!(fetch.is_finished());
        assert_eq!(fetch.progress()[0].got, 5);
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, ["a_b.mp4"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A sink that keeps what it's given.
    struct Kept(mpsc::Sender<(Vec<u8>, Result<u64, String>)>, Vec<u8>);

    impl Sink for Kept {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
            self.1.extend_from_slice(bytes);
            Ok(())
        }
        fn finish(self: Box<Self>, result: Result<u64, Error>) {
            self.0.send((self.1, result.map_err(|e| e.to_string()))).unwrap();
        }
    }

    #[test]
    fn a_promise_for_a_sink_is_fed_from_its_file() {
        let (tell, heard) = mpsc::channel();
        let to = Destination::sink(Kept(tell, vec![]));
        let (fetch, waiting, file) = given(&std::env::temp_dir(), vec![(item("a.mp4"), to)], "a.mp4", b"bytes");
        let at = waiting.at.clone();
        waiting.read(&NSURL::from_file_path(&file).unwrap(), None);
        assert_eq!(heard.recv().unwrap(), (b"bytes".to_vec(), Ok(5)));
        assert!(!at.exists());
        assert!(fetch.is_finished());
    }

    #[test]
    fn a_canceled_fetch_ends_its_promise_canceled_when_the_file_comes() {
        let (tell, heard) = mpsc::channel();
        let to = Destination::sink(Kept(tell, vec![]));
        let (fetch, waiting, file) = given(&std::env::temp_dir(), vec![(item("a.mp4"), to)], "a.mp4", b"bytes");
        fetch.cancel();
        assert!(!fetch.is_finished());
        waiting.read(&NSURL::from_file_path(&file).unwrap(), None);
        assert_eq!(heard.recv().unwrap(), (vec![], Err("canceled".into())));
        assert!(fetch.is_finished());
    }

    #[test]
    fn a_promise_never_kept_still_ends_and_leaves_nothing() {
        let dir = scratch("never");
        let (tell, heard) = mpsc::channel();
        let to = Destination::folder(&dir, move |r| tell.send(r).unwrap());
        let (fetch, waiting, _) = given(&dir, vec![(item("a.mp4"), to)], "a.mp4", b"");
        drop(waiting);
        assert!(matches!(heard.recv().unwrap(), Err(Error::Source(_))));
        assert!(fetch.is_finished());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "no part file left");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
