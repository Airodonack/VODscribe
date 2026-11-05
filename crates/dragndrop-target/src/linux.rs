//! Linux (GTK 3): what a drag dropped on a widget offers, its paths and the
//! files its source saves for you.
//!
//! Call these on GTK's thread, from your widget's drag handlers: [`saves`]
//! and [`items`] (and [`offers`]) in `drag-motion` to decide whether to take
//! the drag, [`paths`] in `drag-data-received` if you ask for text/uri-list
//! yourself, and [`receive`] in `drag-drop`, which asks the source for
//! everything and hands you a [`Received`]; then [`fetch`] with the items you
//! take.
//!
//! Four kinds of offer are read:
//!
//! - text/uri-list: file URIs, the paths.
//! - application/vnd.portal.filetransfer (and its older name
//!   application/vnd.portal.files): a key that the document portal's
//!   FileTransfer.RetrieveFiles turns into the files, as sandboxed (Flatpak)
//!   apps send them. The portal gives files that are on disk (their own
//!   paths to an app outside a sandbox, document-portal paths to one inside),
//!   so they are paths too, nothing to copy.
//! - XdndDirectSave0 (XDS): the source writes the file where the target
//!   names, as archive managers on GTK do (File Roller 3, Engrampa). Its
//!   offer suggests a name; [`receive`] names a file in a fresh folder inside
//!   the stage folder you give, and asks the source to save. Archive
//!   managers take only the folder from it and write each file picked in the
//!   archive under its own name (Engrampa suggests "xds.txt" for any). X11
//!   only: XDS works through a property of the source's X window.
//! - Ark's (KDE): application/x-kde-ark-dndextract-service and -path name a
//!   D-Bus object whose `extractSelectedFilesTo` writes the files picked into
//!   a folder; [`receive`] names a fresh one in the stage.
//!
//! A source that saves may do it before it answers (File Roller 3) or after
//! the drag ends (Engrampa, Ark): [`receive`] waits until the folder holds
//! files and they've stopped changing for a second, with no process holding
//! one open, then gives each as an item with its own name and size. An XDS
//! source that answers "F" (it can't write there) is asked for
//! application/octet-stream instead, if it has it.
//!
//! Progress and cancel: the portal's files and the paths have nothing to
//! fetch. A source saves in its own process, so that part has no progress
//! and can't be canceled (the source shows its own progress); what [`fetch`]
//! then does, moving each file into a folder or copying it into a sink, has
//! both.
//!
//! The drag is finished (`gtk_drag_finish`) by [`receive`], as soon as the
//! source has answered (the paths read, the save asked, or the drop
//! refused), not when the items are fetched: some sources save only once the
//! drag is finished, and GTK's give a drop up that isn't finished within
//! five minutes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use gtk::gdk;
use gtk::gio;
use gtk::glib::{self, ToVariant, translate::FromGlibPtrNone};
use gtk::prelude::*;

use crate::folder::{FolderSink, free_path};
use crate::{Destination, Error, Fetch, Item, Job, Offer, Shared, To, safe_file_name};

/// File URIs, the paths.
pub const URI_LIST: &str = "text/uri-list";
/// A document portal file transfer's key.
pub const PORTAL: &str = "application/vnd.portal.filetransfer";
/// The file transfer's first name, which GTK still offers beside it.
pub const PORTAL_FILES: &str = "application/vnd.portal.files";
/// XDS: the source writes the file where the target names.
pub const DIRECT_SAVE: &str = "XdndDirectSave0";
/// Ark's: the D-Bus name of the source that extracts the files.
pub const ARK_SERVICE: &str = "application/x-kde-ark-dndextract-service";
/// Ark's: the D-Bus object that extracts them.
pub const ARK_PATH: &str = "application/x-kde-ark-dndextract-path";

/// What an XDS source falls back to giving when it can't write ("F").
const OCTET_STREAM: &str = "application/octet-stream";
/// The XDS property's type, as the protocol names it.
const TEXT_PLAIN: &str = "text/plain";
/// How much of a file is copied at a time.
const CHUNK: usize = 1 << 20;
/// How long the portal, or Ark, is given to answer.
const DBUS_TIMEOUT_MS: i32 = 30_000;
/// How often a folder being saved into is looked at.
const LOOK_EVERY: std::time::Duration = std::time::Duration::from_millis(250);
/// How long what's saved must stay as it is to be taken as whole.
const QUIET: std::time::Duration = std::time::Duration::from_secs(1);
/// How long a source that said it saves is given to start.
const SAVE_LIMIT: std::time::Duration = std::time::Duration::from_secs(120);

/// The targets drag `context` offers, by name: for a trace of what a source
/// sends.
pub fn formats(context: &gdk::DragContext) -> Vec<String> {
    context.list_targets().iter().map(|t| t.name().to_string()).collect()
}

/// Whether drag `context` offers target `name` (one of the consts here, or
/// any other).
pub fn offers(context: &gdk::DragContext, name: &str) -> bool {
    context.list_targets().iter().any(|t| t.name() == name)
}

/// The paths in `data`, as `drag-data-received` gave it: text/uri-list's
/// file URIs, or the files of a portal transfer key (asked of the portal
/// while you wait, as GTK's own `gtk_selection_data_get_uris` does). None
/// when `data` is neither, or empty (the source gave nothing).
pub fn paths(data: &gtk::SelectionData) -> Option<Vec<PathBuf>> {
    if data.length() < 0 {
        return None;
    }
    let kind = [data.target().name(), data.data_type().name()];
    if kind.iter().any(|k| k == URI_LIST) {
        return Some(data.uris().iter().filter_map(|u| file_path(u)).collect());
    }
    if kind.iter().any(|k| k == PORTAL || k == PORTAL_FILES) {
        let key = String::from_utf8_lossy(&data.data()).trim_end_matches('\0').to_string();
        let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()?;
        let answer = bus.call_sync(
            Some("org.freedesktop.portal.Documents"),
            "/org/freedesktop/portal/documents",
            "org.freedesktop.portal.FileTransfer",
            "RetrieveFiles",
            Some(&retrieve_args(&key)),
            glib::VariantTy::new("(as)").ok(),
            gio::DBusCallFlags::NONE,
            DBUS_TIMEOUT_MS,
            gio::Cancellable::NONE,
        );
        return answer.ok().and_then(|a| a.get::<(Vec<String>,)>()).map(|(files,)| files_of(files));
    }
    None
}

/// Whether drag `context`'s source would save files for you at the drop
/// (XDS on X11, or Ark's): files with no path yet, to be had through
/// [`receive`].
pub fn saves(context: &gdk::DragContext) -> bool {
    direct_save_name(context).is_some() || (offers(context, ARK_SERVICE) && offers(context, ARK_PATH))
}

/// The virtual files drag `context` offers before it's let go: its XDS offer,
/// named as its source suggests (an archive manager suggests the first file
/// picked, and writes them all). Empty without one, or with no X11 window
/// to read it from (Wayland).
pub fn items(context: &gdk::DragContext) -> Vec<Item> {
    direct_save_name(context).map(|name| vec![Item { name, size: None, index: 0 }]).unwrap_or_default()
}

/// What a drag let go on your widget gave: the paths, or the files its
/// source saved for you (each an [`Item`]), or why it gave nothing. The
/// saved files are in a folder of the stage you named, removed with this
/// (or once [`fetch`] is done with them).
pub struct Received {
    offer: Offer,
    failure: Option<Error>,
    /// The folder the source saved into, and each item's file there.
    stage: Option<(PathBuf, Vec<PathBuf>)>,
}

impl Received {
    /// The paths and the items.
    pub fn offer(&self) -> &Offer {
        &self.offer
    }

    /// Why the source gave nothing, if it was asked and didn't.
    pub fn failure(&self) -> Option<&Error> {
        self.failure.as_ref()
    }

    fn paths(paths: Vec<PathBuf>) -> Self {
        Received { offer: Offer { paths, items: vec![] }, failure: None, stage: None }
    }

    fn failed(why: Error) -> Self {
        Received { offer: Offer::default(), failure: Some(why), stage: None }
    }
}

impl Drop for Received {
    fn drop(&mut self) {
        if let Some((dir, _)) = self.stage.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Asks the source of drag `context`, let go on your widget at `time` (your
/// `drag-drop` handler's arguments; return true from it), for what it has:
/// its paths (text/uri-list, else a portal file transfer), else to save its
/// file into a new folder inside `stage` (XDS; the folder is made, and
/// `stage` with it). `done` hears it on this thread once the source has
/// answered, after the drag is finished. Call it only with a drag GTK has
/// brought to a widget of yours.
pub fn receive(
    context: &gdk::DragContext,
    time: u32,
    stage: impl Into<PathBuf>,
    done: impl FnOnce(Received) + 'static,
) {
    let finish = {
        let context = context.clone();
        move |ok: bool| context.drag_finish(ok, false, time)
    };
    if offers(context, URI_LIST) {
        return ask(context, URI_LIST, time, move |data| {
            let paths = data.and_then(paths).unwrap_or_default();
            finish(!paths.is_empty());
            done(Received::paths(paths))
        });
    }
    if let Some(portal) = [PORTAL, PORTAL_FILES].into_iter().find(|t| offers(context, t)) {
        return ask(context, portal, time, move |data| {
            let key = data.map(|d| String::from_utf8_lossy(&d.data()).trim_end_matches('\0').to_string());
            let Some(key) = key.filter(|k| !k.is_empty()) else {
                finish(false);
                return done(Received::failed(Error::Source("it gave no file transfer".into())));
            };
            retrieve(&key, move |files| {
                finish(files.is_ok());
                done(match files {
                    Ok(paths) => Received::paths(paths),
                    Err(why) => Received::failed(Error::Source(why)),
                })
            })
        });
    }
    let stage = stage.into();
    if direct_save_name(context).is_some() {
        return direct_save(context, time, stage, finish, done);
    }
    if offers(context, ARK_SERVICE) && offers(context, ARK_PATH) {
        return ark(context, time, stage, finish, done);
    }
    finish(false);
    done(Received::failed(Error::Source("it offers no files".into())))
}

/// Fetches `requests`, items of `received` and where each goes, on a thread
/// of its own: into a folder, each file is moved (copied if the folder is on
/// another disk); into a sink, copied. Keep the [`Fetch`] until it finishes:
/// dropped, it cancels. Every item ends exactly once; the stage goes once
/// all have.
pub fn fetch(mut received: Received, requests: Vec<(Item, Destination)>) -> Fetch {
    let (dir, files) = received.stage.take().unwrap_or_default();
    // Folder destinations are moved, not written: kept aside, their jobs'
    // sinks hear nothing.
    let mut folders: HashMap<usize, (PathBuf, Done)> = HashMap::new();
    let requests = requests
        .into_iter()
        .enumerate()
        .map(|(n, (item, to))| match to.0 {
            To::Folder { dir, done } => {
                folders.insert(n, (dir, done));
                (item, Destination::sink(Unused))
            }
            To::Sink(sink) => (item, Destination(To::Sink(sink))),
        })
        .collect();
    let (shared, jobs) = Shared::new(requests);
    let on_thread = shared.clone();
    let work = move |jobs: Vec<Job>| {
        let mut folders = folders;
        for job in jobs {
            let file = files
                .get(job.item.index)
                .filter(|f| f.file_name().is_some_and(|n| n.to_string_lossy() == job.item.name));
            let folder = folders.remove(&job.slot);
            fetch_one(&on_thread, job, file, folder);
        }
        if !dir.as_os_str().is_empty() {
            let _ = std::fs::remove_dir_all(&dir);
        }
    };
    // (Taken back, and done here, if the thread can't start.)
    let queued = Arc::new(Mutex::new(Some((jobs, work))));
    let for_thread = queued.clone();
    let spawned = std::thread::Builder::new().name("dragndrop-fetch".into()).spawn(move || {
        if let Some((jobs, work)) = for_thread.lock().unwrap().take() {
            work(jobs)
        }
    });
    if spawned.is_err() {
        if let Some((jobs, work)) = queued.lock().unwrap().take() {
            work(jobs);
            return shared.handle(false);
        }
    }
    shared.handle(true)
}

type Done = Box<dyn FnOnce(Result<PathBuf, Error>) + Send>;

/// Fetches `job` from staged `file`: moved into `folder` if it goes to one,
/// else copied into its sink; ended either way.
fn fetch_one(shared: &Shared, mut job: Job, file: Option<&PathBuf>, folder: Option<(PathBuf, Done)>) {
    let Some(file) = file else {
        if let Some((_, done)) = folder {
            done(Err(Error::Source("not one of this drop's files".into())));
        }
        return shared.end(job, Err(Error::Source("not one of this drop's files".into())));
    };
    if shared.canceled() {
        if let Some((_, done)) = folder {
            done(Err(Error::Canceled));
        }
        return shared.end(job, Err(Error::Canceled));
    }
    if let Some((dir, done)) = folder {
        match move_into(&dir, &job.item.name, file) {
            Ok((to, len)) => {
                shared.items[job.slot].got.store(len, Ordering::Relaxed);
                done(Ok(to));
                return shared.end(job, Ok(len));
            }
            // Another disk: written as any folder's item is.
            Err(_) => job.sink = Box::new(FolderSink::new(dir, &job.item.name, done)),
        }
    }
    let copied = copy(shared, &mut job, file);
    shared.end(job, copied)
}

/// Moves `file` into folder `dir` under `name` made safe and numbered if
/// taken: its path there and length. Fails, leaving it, across disks.
fn move_into(dir: &Path, name: &str, file: &Path) -> io::Result<(PathBuf, u64)> {
    std::fs::create_dir_all(dir)?;
    let len = std::fs::metadata(file)?.len();
    loop {
        let to = free_path(dir, &safe_file_name(name));
        // A link never replaces a file that came since, as a rename would.
        match std::fs::hard_link(file, &to) {
            Ok(()) => {
                let _ = std::fs::remove_file(file);
                return Ok((to, len));
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Copies `file` into `job`'s sink: how many bytes.
fn copy(shared: &Shared, job: &mut Job, file: &Path) -> Result<u64, Error> {
    let mut from = File::open(file).map_err(|e| Error::Source(format!("what it saved can't be read: {e}")))?;
    let mut buf = vec![0u8; CHUNK];
    let mut total = 0;
    loop {
        let got = from.read(&mut buf).map_err(|e| Error::Source(format!("what it saved can't be read: {e}")))?;
        if got == 0 {
            return Ok(total);
        }
        shared.write(job, &buf[..got])?;
        total += got as u64;
    }
}

/// The sink of an item moved, not written.
struct Unused;

impl crate::Sink for Unused {
    fn write(&mut self, _: &[u8]) -> io::Result<()> {
        Ok(())
    }
    fn finish(self: Box<Self>, _: Result<u64, Error>) {}
}

/// Asks drag `context`'s source for `target`, through a widget of the
/// crate's own (one with no drop site, so GTK hands it any target): `then`
/// hears the data, or None if the source gave none (GTK gives up on a
/// source silent for 30 s).
fn ask(context: &gdk::DragContext, target: &str, time: u32, then: impl FnOnce(Option<&gtk::SelectionData>) + 'static) {
    let asker = gtk::Invisible::new();
    let then = RefCell::new(Some(then));
    asker.connect_drag_data_received(move |asker, _, _, _, data, _, _| {
        let Some(then) = then.take() else { return };
        then(Some(data).filter(|d| d.length() >= 0));
        // Gone once GTK is done with it (it's a toplevel GTK keeps).
        let asker = asker.clone();
        glib::idle_add_local_once(move || {
            // SAFETY: the crate's own widget, nothing else holds it.
            unsafe { asker.destroy() };
        });
    });
    asker.drag_get_data(context, &gdk::Atom::intern(target), time);
}

/// Asks the document portal for the files of transfer `key`; `then` hears
/// their paths on this thread.
fn retrieve(key: &str, then: impl FnOnce(Result<Vec<PathBuf>, String>) + 'static) {
    let bus = match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        Ok(bus) => bus,
        Err(e) => return then(Err(format!("no session bus to reach the portal: {e}"))),
    };
    bus.call(
        Some("org.freedesktop.portal.Documents"),
        "/org/freedesktop/portal/documents",
        "org.freedesktop.portal.FileTransfer",
        "RetrieveFiles",
        Some(&retrieve_args(key)),
        glib::VariantTy::new("(as)").ok(),
        gio::DBusCallFlags::NONE,
        DBUS_TIMEOUT_MS,
        gio::Cancellable::NONE,
        move |answer| {
            then(match answer {
                Ok(a) => a.get::<(Vec<String>,)>().map(|(f,)| files_of(f)).ok_or_else(|| "the portal's answer".into()),
                Err(e) => Err(format!("the portal didn't give its files: {e}")),
            })
        },
    );
}

/// RetrieveFiles' arguments: the key, no options.
fn retrieve_args(key: &str) -> glib::Variant {
    // (The options as a dictionary, not a variant holding one.)
    glib::Variant::tuple_from_iter([key.to_variant(), glib::VariantDict::new(None).end()])
}

/// The portal's answer as paths.
fn files_of(files: Vec<String>) -> Vec<PathBuf> {
    files.into_iter().filter(|f| !f.is_empty()).map(PathBuf::from).collect()
}

/// The local path of URI `uri`, if it's a file's.
fn file_path(uri: &str) -> Option<PathBuf> {
    glib::filename_from_uri(uri)
        .ok()
        .filter(|(_, host)| host.as_deref().is_none_or(|h| h == "localhost"))
        .map(|(p, _)| p)
}

/// The window drag `context` comes from, where X11 says (none on Wayland,
/// nor from another app there).
fn source_window(context: &gdk::DragContext) -> Option<gdk::Window> {
    // SAFETY: a plain getter; the window may be null, and is taken as such.
    unsafe {
        let w = gdk::ffi::gdk_drag_context_get_source_window(context.as_ptr());
        (!w.is_null()).then(|| gdk::Window::from_glib_none(w))
    }
}

/// The name drag `context`'s XDS offer suggests, if it makes one.
fn direct_save_name(context: &gdk::DragContext) -> Option<String> {
    if !offers(context, DIRECT_SAVE) {
        return None;
    }
    let window = source_window(context)?;
    let (_, _, bytes) =
        gdk::property_get(&window, &gdk::Atom::intern(DIRECT_SAVE), &gdk::Atom::intern(TEXT_PLAIN), 0, 4096, 0)?;
    let name = String::from_utf8_lossy(&bytes).trim_end_matches('\0').to_string();
    // (Some put a whole path there; the name is its last part.)
    let name = name.rsplit('/').next().unwrap_or_default().to_string();
    (!name.is_empty()).then_some(name)
}

/// Asks drag `context`'s XDS source to save into a new folder of `stage`:
/// `finish` ends the drag once it has answered, `done` hears what it saved.
fn direct_save(
    context: &gdk::DragContext,
    time: u32,
    stage: PathBuf,
    finish: impl FnOnce(bool) + 'static,
    done: impl FnOnce(Received) + 'static,
) {
    let named = || {
        let name = direct_save_name(context).ok_or_else(|| Error::Source("it offers no files".into()))?;
        let window = source_window(context).ok_or_else(|| Error::Source("it can't be asked to save here".into()))?;
        let dir = fresh_folder(&stage).map_err(Error::Write)?;
        let file = dir.join(safe_file_name(&name));
        let uri = glib::filename_to_uri(&file, None).map_err(|e| Error::Write(io::Error::other(e.to_string())))?;
        gdk::property_change(
            &window,
            &gdk::Atom::intern(DIRECT_SAVE),
            &gdk::Atom::intern(TEXT_PLAIN),
            8,
            gdk::PropMode::Replace,
            gdk::ChangeData::UChars(uri.as_bytes()),
        );
        Ok((dir, file))
    };
    let (dir, file) = match named() {
        Ok(named) => named,
        Err(why) => {
            finish(false);
            return done(Received::failed(why));
        }
    };
    let asked = context.clone();
    ask(context, DIRECT_SAVE, time, move |data| match answer(data) {
        // Saved, or to be (some save once the drag ends); or silent, as
        // one still saving when GTK stops waiting is.
        Answer::Saved | Answer::None => {
            finish(true);
            settled(dir, done)
        }
        Answer::Cant if offers(&asked, OCTET_STREAM) => ask(&asked, OCTET_STREAM, time, move |data| {
            let written = data.map(|d| std::fs::write(&file, d.data()));
            finish(matches!(written, Some(Ok(()))));
            match written {
                Some(Ok(())) => settled(dir, done),
                _ => done(failed(&dir, "it couldn't save it, nor give it")),
            }
        }),
        Answer::Cant => {
            finish(false);
            done(failed(&dir, "it couldn't save it there"))
        }
        Answer::Error => {
            finish(false);
            done(failed(&dir, "it couldn't save it"))
        }
    });
}

/// Asks drag `context`'s Ark source to extract its files into a new folder
/// of `stage`: `finish` ends the drag once it has, `done` hears them.
fn ark(
    context: &gdk::DragContext,
    time: u32,
    stage: PathBuf,
    finish: impl FnOnce(bool) + 'static,
    done: impl FnOnce(Received) + 'static,
) {
    let text = |data: Option<&gtk::SelectionData>| {
        data.map(|d| String::from_utf8_lossy(&d.data()).trim_end_matches('\0').to_string()).filter(|t| !t.is_empty())
    };
    let asked = context.clone();
    ask(context, ARK_SERVICE, time, move |data| {
        let service = text(data);
        ask(&asked, ARK_PATH, time, move |data| {
            let (Some(service), Some(object)) = (service, text(data)) else {
                finish(false);
                return done(Received::failed(Error::Source("it didn't say where to ask".into())));
            };
            let dir = match fresh_folder(&stage) {
                Ok(dir) => dir,
                Err(e) => {
                    finish(false);
                    return done(Received::failed(Error::Write(e)));
                }
            };
            let bus = match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
                Ok(bus) => bus,
                Err(e) => {
                    finish(false);
                    return done(failed(&dir, &format!("no session bus to reach it: {e}")));
                }
            };
            let to = dir.to_string_lossy().into_owned();
            bus.call(
                Some(&service),
                &object,
                "org.kde.ark.DndExtract",
                "extractSelectedFilesTo",
                Some(&(to,).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                DBUS_TIMEOUT_MS,
                gio::Cancellable::NONE,
                move |answer| {
                    finish(answer.is_ok());
                    match answer {
                        Ok(_) => settled(dir, done),
                        Err(e) => done(failed(&dir, &format!("it didn't extract them: {e}"))),
                    }
                },
            );
        })
    });
}

/// Nothing received, `dir` gone, for `why`.
fn failed(dir: &Path, why: &str) -> Received {
    let _ = std::fs::remove_dir_all(dir);
    Received::failed(Error::Source(why.into()))
}

/// Waits, on this thread's main loop, for what a source saves into `dir` to
/// be whole: files there that have stayed as they are for [`QUIET`] with no
/// process holding one open. `done` then hears them as items, or that none
/// came within [`SAVE_LIMIT`].
fn settled(dir: PathBuf, done: impl FnOnce(Received) + 'static) {
    let started = std::time::Instant::now();
    let mut seen = look(&dir);
    let mut since = started;
    let mut done = Some(done);
    let mut check = move || {
        let now = look(&dir);
        if now != seen {
            (seen, since) = (now, std::time::Instant::now());
            return glib::ControlFlow::Continue;
        }
        let whole = !seen.is_empty() && (since == started || since.elapsed() >= QUIET) && !open_in(&dir);
        if !whole && !(seen.is_empty() && started.elapsed() >= SAVE_LIMIT) {
            return glib::ControlFlow::Continue;
        }
        let Some(done) = done.take() else { return glib::ControlFlow::Break };
        done(match whole {
            true => staged(dir.clone()),
            false => failed(&dir, "it said it would save them, and nothing came"),
        });
        glib::ControlFlow::Break
    };
    // Saved before it answered: taken at once.
    if check() == glib::ControlFlow::Break {
        return;
    }
    glib::timeout_add_local(LOOK_EVERY, check);
}

/// What's in `dir` now: each file, its length and when it last changed.
fn look(dir: &Path) -> Vec<(PathBuf, u64, Option<std::time::SystemTime>)> {
    staged_files(dir)
        .into_iter()
        .map(|f| {
            let meta = std::fs::metadata(&f).ok();
            let (len, modified) = meta.map(|m| (m.len(), m.modified().ok())).unwrap_or_default();
            (f, len, modified)
        })
        .collect()
}

/// Whether a process of this user's holds a file in `dir` open (one still
/// being written). Processes it can't see (another sandbox's) aren't asked.
fn open_in(dir: &Path) -> bool {
    let Ok(procs) = std::fs::read_dir("/proc") else { return false };
    procs.flatten().filter(|p| p.file_name().to_str().is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))).any(|p| {
        std::fs::read_dir(p.path().join("fd"))
            .map(|fds| fds.flatten().any(|fd| std::fs::read_link(fd.path()).is_ok_and(|to| to.starts_with(dir))))
            .unwrap_or(false)
    })
}

/// The files saved into `dir`, as a drop's items.
fn staged(dir: PathBuf) -> Received {
    let files = staged_files(&dir);
    let items = files
        .iter()
        .enumerate()
        .map(|(index, f)| Item {
            name: f.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            size: std::fs::metadata(f).ok().map(|m| m.len()),
            index,
        })
        .collect();
    Received { offer: Offer { paths: vec![], items }, failure: None, stage: Some((dir, files)) }
}

/// An XDS source's answer.
#[derive(Debug, PartialEq)]
enum Answer {
    /// "S": saved where named.
    Saved,
    /// "F": it can't write there; the target may ask for the bytes.
    Cant,
    /// "E": it failed.
    Error,
    /// No answer, or one the protocol doesn't have.
    None,
}

fn answer(data: Option<&gtk::SelectionData>) -> Answer {
    match data.filter(|d| d.format() == 8 && d.length() == 1).map(|d| d.data()) {
        Some(bytes) => answer_of(&bytes),
        None => Answer::None,
    }
}

fn answer_of(bytes: &[u8]) -> Answer {
    match bytes {
        b"S" => Answer::Saved,
        b"F" => Answer::Cant,
        b"E" => Answer::Error,
        _ => Answer::None,
    }
}

/// A new empty folder inside `stage` (made if needed), named for this
/// process and a count.
fn fresh_folder(stage: &Path) -> io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    std::fs::create_dir_all(stage)?;
    loop {
        let dir = stage.join(format!(".dragndrop-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

/// The regular files under `dir`, at any depth, in name order (links not
/// followed).
fn staged_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = vec![];
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for entry in entries.flatten() {
            match entry.file_type() {
                Ok(t) if t.is_dir() => dirs.push(entry.path()),
                Ok(t) if t.is_file() => found.push(entry.path()),
                _ => {}
            }
        }
    }
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dragndrop-target-linux-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A drop whose source saved `files` (name, bytes) into a fresh folder of `stage`.
    fn saved(stage: &Path, files: &[(&str, &[u8])]) -> Received {
        let dir = fresh_folder(stage).unwrap();
        for (name, bytes) in files {
            let at = dir.join(name);
            std::fs::create_dir_all(at.parent().unwrap()).unwrap();
            std::fs::write(at, bytes).unwrap();
        }
        staged(dir)
    }

    fn wait(fetch: &Fetch) {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !fetch.is_finished() {
            assert!(std::time::Instant::now() < until, "the fetch never ended");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn the_answers_are_the_protocols() {
        assert_eq!(answer_of(b"S"), Answer::Saved);
        assert_eq!(answer_of(b"F"), Answer::Cant);
        assert_eq!(answer_of(b"E"), Answer::Error);
        assert_eq!(answer_of(b"s"), Answer::None);
        assert_eq!(answer_of(b""), Answer::None);
    }

    #[test]
    fn only_local_file_uris_are_paths() {
        assert_eq!(file_path("file:///a%20b/c.mp4"), Some(PathBuf::from("/a b/c.mp4")));
        assert_eq!(file_path("file://localhost/c.mp4"), Some(PathBuf::from("/c.mp4")));
        assert_eq!(file_path("file://elsewhere/c.mp4"), None);
        assert_eq!(file_path("https://example.com/c.mp4"), None);
    }

    #[test]
    fn a_file_held_open_in_the_folder_is_seen() {
        let dir = scratch("open");
        std::fs::create_dir_all(&dir).unwrap();
        let file = File::create(dir.join("a.mp4")).unwrap();
        assert!(open_in(&dir));
        drop(file);
        assert!(!open_in(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn what_a_source_saved_is_found_at_any_depth() {
        let stage = scratch("staged");
        let received = saved(&stage, &[("b.mp4", b"bb"), ("in/a.mp4", b"a")]);
        let names: Vec<_> = received.offer().items.iter().map(|i| (i.name().to_string(), i.size())).collect();
        assert_eq!(names, [("b.mp4".to_string(), Some(2)), ("a.mp4".to_string(), Some(1))]);
        let (dir, _) = received.stage.clone().unwrap();
        drop(received);
        assert!(!dir.exists(), "a drop let go of takes its folder with it");
        std::fs::remove_dir_all(&stage).unwrap();
    }

    #[test]
    fn into_a_folder_a_file_is_moved_and_into_a_sink_copied() {
        let stage = scratch("fetch");
        let into = stage.join("into");
        std::fs::create_dir_all(&into).unwrap();
        std::fs::write(into.join("a.mp4"), b"there already").unwrap();
        let received = saved(&stage, &[("a.mp4", b"aaa"), ("b.mp4", b"bbbb"), ("left.txt", b"")]);
        let (dir, _) = received.stage.clone().unwrap();
        let items = received.offer().items.clone();
        let (tell, heard) = mpsc::channel();
        let kept = Arc::new(Mutex::new((vec![], None)));
        type Given = (Vec<u8>, Option<Result<u64, String>>);
        struct Kept(Arc<Mutex<Given>>);
        impl crate::Sink for Kept {
            fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
                self.0.lock().unwrap().0.extend_from_slice(bytes);
                Ok(())
            }
            fn finish(self: Box<Self>, result: Result<u64, Error>) {
                self.0.lock().unwrap().1 = Some(result.map_err(|e| e.to_string()));
            }
        }
        let fetch = super::fetch(
            received,
            vec![
                (
                    items[0].clone(),
                    Destination::folder(&into, move |r| tell.send(r.map_err(|e| e.to_string())).unwrap()),
                ),
                (items[1].clone(), Destination::sink(Kept(kept.clone()))),
            ],
        );
        assert!(fetch.in_background());
        wait(&fetch);
        assert_eq!(heard.recv().unwrap().unwrap(), into.join("a (2).mp4"));
        assert_eq!(std::fs::read(into.join("a (2).mp4")).unwrap(), b"aaa");
        assert_eq!(*kept.lock().unwrap(), (b"bbbb".to_vec(), Some(Ok(4))));
        assert_eq!(fetch.progress().iter().map(|p| p.got).collect::<Vec<_>>(), [3, 4]);
        // (The stage goes just after the last item ends.)
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while dir.exists() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!dir.exists(), "the stage, with what wasn't taken, is gone");
        std::fs::remove_dir_all(&stage).unwrap();
    }

    #[test]
    fn a_canceled_fetch_ends_its_items_canceled() {
        let stage = scratch("cancel");
        let received = saved(&stage, &[("a.mp4", b"aaa")]);
        let item = received.offer().items[0].clone();
        let (tell, heard) = mpsc::channel();
        let into = stage.join("into");
        // Canceled before its thread gets to the item: dropped at once.
        let fetch = super::fetch(received, vec![(item, Destination::folder(&into, move |r| tell.send(r).unwrap()))]);
        fetch.cancel();
        wait(&fetch);
        match heard.recv().unwrap() {
            // (Or moved, if the thread got there first.)
            Ok(p) => assert_eq!(p, into.join("a.mp4")),
            Err(e) => assert!(matches!(e, Error::Canceled)),
        }
        std::fs::remove_dir_all(&stage).unwrap();
    }
}
