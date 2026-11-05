//! Files dropped on the window with no path on this computer to play them
//! from, received (`drop_target`): virtual files on Windows (a video inside a
//! zip folder in Explorer), file promises on macOS, and on Linux the files an
//! archive manager saves for the app (File Roller, Engrampa, Ark), first into
//! a hidden folder of `received`. Each is written into the kept videos'
//! folder, under `received`, as `<name>.part`, renamed once whole and added
//! as a file (`vods::add_received`); meanwhile it's listed with the jobs
//! ("Receiving", how far, and Cancel). Canceled or failed, nothing is left.
//!
//! On Windows the window's drops are all taken here (`Window`, for
//! `drop_target`): those with paths reach the page as Tauri's own drag-drop
//! events did, so the page hears the same. On macOS only drags of promises
//! are; wry gives the page the rest, as before. On Linux the web view keeps
//! the drops it takes (file addresses), and those taken here reach the page
//! the same way: a sandboxed app's files, which the document portal gives as
//! paths, as a drop with them.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The files being received.
pub(crate) struct Receives {
    #[cfg_attr(not(any(windows, target_os = "macos", target_os = "linux")), allow(dead_code))]
    next: AtomicI64,
    list: Mutex<Vec<Arc<Receiving>>>,
}

/// A file being received, as listed with the jobs.
struct Receiving {
    id: i64,
    name: String,
    /// Its length, if the app it comes from said.
    size: Option<u64>,
    /// Bytes written so far.
    got: AtomicU64,
    canceled: AtomicBool,
}

impl Receives {
    pub(crate) fn new() -> Self {
        Receives { next: AtomicI64::new(1), list: Mutex::default() }
    }

    /// Each file being received: its id, name and how far it is (0..1; None
    /// while that isn't known).
    pub(crate) fn listed(&self) -> Vec<(i64, String, Option<f64>)> {
        let list = self.list.lock().unwrap();
        list.iter().map(|r| (r.id, r.name.clone(), along(r.got.load(Ordering::Relaxed), r.size))).collect()
    }

    /// Cancels receiving file `id` (the jobs' Cancel): it's off the list at
    /// once, and stops at its next bytes; what was written goes, and so does
    /// what its app still gives (a promise on macOS comes when its app has
    /// written it). Whether it was listed.
    pub(crate) fn cancel(&self, id: i64) -> bool {
        let mut list = self.list.lock().unwrap();
        let Some(at) = list.iter().position(|r| r.id == id) else { return false };
        list.remove(at).canceled.store(true, Ordering::SeqCst);
        true
    }
}

/// How far `got` bytes of `size` are, if it's known.
fn along(got: u64, size: Option<u64>) -> Option<f64> {
    size.filter(|&s| s > 0).map(|s| (got as f64 / s as f64).min(1.0))
}

/// The name a file received as `name` is written under (as a file it
/// would be added from: one of the media extensions), else None.
#[cfg_attr(not(any(windows, target_os = "macos", target_os = "linux")), allow(dead_code))]
fn file_name(name: &str) -> Option<String> {
    // Split as text: another app's name isn't a path here (on Windows "a:b"
    // would read as a drive).
    let (stem, ext) = name.rsplit_once('.')?;
    let ext = ext.to_ascii_lowercase();
    vodscribe_core::local::media_type(std::path::Path::new(&format!("x.{ext}")))?;
    Some(format!("{}.{ext}", vodscribe_core::clips::safe_name(stem)))
}

/// `name` in `dir`, or the first of "<stem> (2).<ext>", "(3)"... that isn't
/// there, finished or being written.
#[cfg_attr(not(any(windows, target_os = "macos", target_os = "linux")), allow(dead_code))]
fn free_name(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(name);
    let (stem, ext) = (path.file_stem().unwrap_or_default().to_string_lossy(), path.extension());
    let taken = |p: &std::path::Path| p.exists() || part_of(p).exists();
    let mut at = dir.join(name);
    for n in 2.. {
        if !taken(&at) {
            break;
        }
        let mut next = format!("{stem} ({n})");
        if let Some(ext) = ext {
            next = format!("{next}.{}", ext.to_string_lossy());
        }
        at = dir.join(next);
    }
    at
}

/// Where file `path` is written until it's whole.
#[cfg_attr(not(any(windows, target_os = "macos", target_os = "linux")), allow(dead_code))]
fn part_of(path: &std::path::Path) -> std::path::PathBuf {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    part.into()
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(crate) use taking::take_drops;

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
mod taking {
    use std::fs::File;
    use std::io::{BufWriter, Write};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    use serde::Serialize;
    use tauri::{AppHandle, Emitter, Manager};

    use super::{Receiving, file_name, free_name, part_of};
    use dragndrop_target::{Destination, Item};

    use crate::AppState;
    use crate::drop_target::{self, Event};

    /// How often the jobs' list hears how far a file is.
    const TELL_EVERY: Duration = Duration::from_millis(250);

    /// A line about a drop, in the tests' builds only.
    #[cfg(feature = "e2e")]
    fn trace(what: &str) {
        eprintln!("drop: {what}");
    }
    #[cfg(not(feature = "e2e"))]
    fn trace(_: &str) {}

    /// Takes the drops on `window` (built without Tauri's drag-drop handler)
    /// with the app's own drop target, the web view's own turned off.
    #[cfg(windows)]
    pub(crate) fn take_drops(window: &tauri::WebviewWindow) -> tauri::Result<()> {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller4;
        use windows_core::Interface;

        let hwnd = window.hwnd()?.0 as isize;
        let target = Window { app: window.app_handle().clone(), label: window.label().to_string() };
        window.with_webview(move |webview| {
            // SAFETY: the web view's controller, on the window's thread.
            let off = unsafe {
                webview.controller().cast::<ICoreWebView2Controller4>().and_then(|c| c.SetAllowExternalDrop(false))
            };
            if let Err(e) = off {
                eprintln!("The web view's own drops couldn't be turned off: {e}");
            }
            match drop_target::register(hwnd, Box::new(target), trace) {
                Ok(on) => trace(&format!("taking drops on {}", on.join(", "))),
                Err(e) => eprintln!("The window can't take dropped files: {e}"),
            }
        })
    }

    /// Takes the drops of file promises on `window`'s web view (wry's drop
    /// target, hooked), and the promises' types.
    #[cfg(target_os = "macos")]
    pub(crate) fn take_drops(window: &tauri::WebviewWindow) -> tauri::Result<()> {
        let target = Window { app: window.app_handle().clone(), label: window.label().to_string() };
        window.with_webview(move |webview| match drop_target::register(webview.inner(), Box::new(target), trace) {
            Ok(()) => trace("taking promised files on the web view"),
            Err(e) => eprintln!("The window can't take promised files: {e}"),
        })
    }

    /// Takes the drops on `window`'s page that its web view can't (files with
    /// no address yet); the web view keeps the rest.
    #[cfg(target_os = "linux")]
    pub(crate) fn take_drops(window: &tauri::WebviewWindow) -> tauri::Result<()> {
        let target = Window { app: window.app_handle().clone(), label: window.label().to_string() };
        match crate::webview::page(window) {
            Ok(page) => {
                drop_target::register(&page, Box::new(target), trace);
                trace("taking the drops the web view can't");
            }
            Err(e) => eprintln!("The window can't take files saved for it: {e:#}"),
        }
        Ok(())
    }

    /// Where a drag over the window is, as Tauri's drag-drop events say it.
    #[derive(Serialize, Clone)]
    struct Position {
        x: f64,
        y: f64,
    }

    #[derive(Serialize, Clone)]
    struct Payload {
        #[serde(skip_serializing_if = "Option::is_none")]
        paths: Option<Vec<PathBuf>>,
        position: Position,
    }

    /// The window's side of its drops.
    struct Window {
        app: AppHandle,
        label: String,
    }

    impl Window {
        fn tell(&self, event: &str, paths: Option<Vec<PathBuf>>, (x, y): (f64, f64)) {
            let target = tauri::EventTarget::Webview { label: self.label.clone() };
            let _ = self.app.emit_to(target, event, Payload { paths, position: Position { x, y } });
        }
    }

    impl drop_target::Target for Window {
        /// Told to the page as Tauri's own events: a drag of virtual files
        /// comes with their names for paths (the page shows it's welcome),
        /// and its drop with none (they come in as jobs).
        fn heard(&self, event: Event) {
            match event {
                Event::Enter { paths, names, at } => {
                    let paths = if paths.is_empty() { names.into_iter().map(PathBuf::from).collect() } else { paths };
                    self.tell("tauri://drag-enter", Some(paths), at)
                }
                Event::Over { at } => self.tell("tauri://drag-over", None, at),
                Event::Leave => {
                    let target = tauri::EventTarget::Webview { label: self.label.clone() };
                    let _ = self.app.emit_to(target, "tauri://drag-leave", ());
                }
                Event::Drop { paths, at } => self.tell("tauri://drag-drop", Some(paths), at),
            }
        }

        fn takes(&self, name: &str) -> bool {
            file_name(name).is_some()
        }

        #[cfg(target_os = "linux")]
        fn stage(&self) -> Option<PathBuf> {
            // (Not while the library can't be opened: the recovery screen.)
            Some(self.app.try_state::<AppState>()?.paths.media().join("received"))
        }

        fn incoming(&self, item: &Item) -> Option<Destination> {
            // (Not while the library can't be opened: the recovery screen.)
            self.app.try_state::<AppState>()?;
            match Sink::start(&self.app, item.name(), item.size()) {
                Ok(sink) => Some(Destination::sink(sink)),
                Err(e) => {
                    eprintln!("{} couldn't be received: {e:#}", item.name());
                    None
                }
            }
        }
    }

    /// Where a file being received is written.
    struct Sink {
        app: AppHandle,
        job: Arc<Receiving>,
        /// Its name once whole.
        to: PathBuf,
        /// Until it's done.
        file: Option<BufWriter<File>>,
        told: Instant,
    }

    impl Sink {
        fn start(app: &AppHandle, name: &str, size: Option<u64>) -> anyhow::Result<Sink> {
            let state = app.state::<AppState>();
            let shown = file_name(name).ok_or_else(|| anyhow::anyhow!("not a video or audio file"))?;
            let dir = state.paths.media().join("received");
            std::fs::create_dir_all(&dir)?;
            let receives = &state.receives;
            let mut list = receives.list.lock().unwrap();
            let to = free_name(&dir, &shown);
            let file = File::create_new(part_of(&to))?;
            let job = Arc::new(Receiving {
                id: receives.next.fetch_add(1, Ordering::SeqCst),
                name: shown,
                size,
                got: Default::default(),
                canceled: Default::default(),
            });
            list.push(job.clone());
            drop(list);
            crate::job_list::changed(app);
            Ok(Sink {
                app: app.clone(),
                job,
                to,
                file: Some(BufWriter::with_capacity(1 << 20, file)),
                told: Instant::now(),
            })
        }

        /// Its job off the list.
        fn end(&self) {
            let state = self.app.state::<AppState>();
            state.receives.list.lock().unwrap().retain(|r| r.id != self.job.id);
            crate::job_list::changed(&self.app);
        }
    }

    impl dragndrop_target::Sink for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
            if self.job.canceled.load(Ordering::SeqCst) {
                return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "Canceled."));
            }
            let Some(file) = self.file.as_mut() else { return Err(std::io::ErrorKind::BrokenPipe.into()) };
            file.write_all(bytes)?;
            self.job.got.fetch_add(bytes.len() as u64, Ordering::Relaxed);
            if self.told.elapsed() >= TELL_EVERY {
                self.told = Instant::now();
                crate::job_list::changed(&self.app);
            }
            Ok(())
        }

        fn finish(mut self: Box<Self>, result: Result<u64, dragndrop_target::Error>) {
            trace(&format!(
                "{}: {}",
                self.job.name,
                result.as_ref().map(|n| format!("{n} bytes read")).unwrap_or_else(|e| e.to_string())
            ));
            let file = self.file.take();
            let part = part_of(&self.to);
            let whole = result
                .map_err(|e| e.to_string())
                .and_then(|_| match self.job.canceled.load(Ordering::SeqCst) {
                    true => Err("Canceled.".to_string()),
                    false => Ok(()),
                })
                .and_then(|()| {
                    let file = file.ok_or("Not written.")?.into_inner().map_err(|e| e.error().to_string())?;
                    file.sync_all().map_err(|e| e.to_string())?;
                    drop(file);
                    std::fs::rename(&part, &self.to).map_err(|e| e.to_string())
                });
            if let Err(why) = whole {
                // (The file is closed before it goes: Windows keeps an open one.)
                let _ = std::fs::remove_file(&part);
                if !self.job.canceled.load(Ordering::SeqCst) {
                    eprintln!("{} wasn't received: {why}", self.job.name);
                }
                self.end();
                return;
            }
            // Added off the reading thread (it may be the window's).
            std::thread::spawn(move || {
                if let Err(e) = crate::vods::add_received(&self.app, &self.to) {
                    eprintln!("{} was received but couldn't be added: {e:#}", self.job.name);
                    let _ = std::fs::remove_file(&self.to);
                }
                self.end();
            });
        }
    }

    impl Drop for Sink {
        /// Never finished (dropped unread): what was written goes.
        fn drop(&mut self) {
            if self.file.take().is_some() {
                let _ = std::fs::remove_file(part_of(&self.to));
                self.end();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_media_names_are_received_made_safe() {
        assert_eq!(file_name("Clip [1].MP4").as_deref(), Some("Clip (1).mp4"));
        assert_eq!(file_name("a:b.mkv").as_deref(), Some("a b.mkv"));
        assert_eq!(file_name("notes.txt"), None);
        assert_eq!(file_name("noext"), None);
        assert_eq!(file_name(".mp4").as_deref(), Some("Clip.mp4"), "no name but its extension");
    }

    #[test]
    fn a_name_taken_finished_or_being_written_gets_a_number() {
        let dir = std::env::temp_dir().join(format!("vodscribe-receive-names-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_name(&dir, "a.mp4"), dir.join("a.mp4"));
        std::fs::write(dir.join("a.mp4"), b"").unwrap();
        std::fs::write(dir.join("a (2).mp4.part"), b"").unwrap();
        assert_eq!(free_name(&dir, "a.mp4"), dir.join("a (3).mp4"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn how_far_is_known_only_with_a_size() {
        assert_eq!(along(5, Some(10)), Some(0.5));
        assert_eq!(along(12, Some(10)), Some(1.0));
        assert_eq!(along(5, None), None);
        assert_eq!(along(0, Some(0)), None);
    }

    #[test]
    fn a_cancel_takes_only_its_own_off_the_list_at_once() {
        let receives = Receives::new();
        for (id, name) in [(1, "a.mp4"), (2, "b.mp4")] {
            receives.list.lock().unwrap().push(Arc::new(Receiving {
                id,
                name: name.into(),
                size: Some(4),
                got: AtomicU64::new(2),
                canceled: AtomicBool::new(false),
            }));
        }
        let second = receives.list.lock().unwrap()[1].clone();
        assert!(receives.cancel(2));
        assert!(second.canceled.load(Ordering::SeqCst), "its sink sees it's canceled");
        assert!(!receives.list.lock().unwrap()[0].canceled.load(Ordering::SeqCst));
        assert_eq!(receives.listed(), [(1, "a.mp4".to_string(), Some(0.5))], "off the list at once");
        assert!(!receives.cancel(2));
    }
}
