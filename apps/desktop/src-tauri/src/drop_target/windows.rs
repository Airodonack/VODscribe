//! The window's drop target on Windows (OLE). A drag is taken one of two
//! ways, as it comes over the window:
//!
//! - as paths (CF_HDROP), told to the target as they are: files dropped from
//!   Explorer, and the app's own drags out, coming back over the window
//!   (`drag_source` answers the window at once, with the files made so far);
//! - else as virtual files (FileGroupDescriptorW + FileContents), when there
//!   are some the target takes: a video inside a zip folder in Explorer.
//!   Once let go, each is read from the app it comes from into what the
//!   target gives for it. If that app copies asynchronously
//!   (IDataObjectAsyncCapability, as Explorer does), the reading is on a
//!   thread of its own and the drop returns at once (Explorer goes on, and
//!   shows its own copy dialog); else it's read before the drop returns.
//!
//! A drag offering both is taken as paths. Anything else is refused.
//!
//! WebView2 would take drops itself (its AllowExternalDrop, which the app
//! turns off) and wry would put its own target over it (Tauri's drag-drop
//! handler, which the app doesn't use on Windows); this is registered on the
//! window and on each window in it, as wry's was.

use std::cell::RefCell;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;
use windows::Win32::System::Com::*;
use windows::Win32::System::DataExchange::{GetClipboardFormatNameW, RegisterClipboardFormatW};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, GetClassNameW};
use windows::core::*;

use super::{Event, Incoming, Target};

/// How much of a virtual file is read at a time.
const CHUNK: usize = 1 << 20;

/// Takes drops on window `hwnd` (its raw handle) and the windows in it for
/// `target`; `trace` hears what's going on (the formats each drag offers,
/// each file received). Called on the window's thread, which OLE is set up
/// on (Tauri's). Gives the class names of the windows it's registered on.
pub(crate) fn register(hwnd: isize, target: Box<dyn Target>, trace: fn(&str)) -> Result<Vec<String>> {
    let window = HWND(hwnd as *mut _);
    // SAFETY: registered clipboard formats are plain lookups.
    let (descriptor, contents) = unsafe {
        (RegisterClipboardFormatW(CFSTR_FILEDESCRIPTORW) as u16, RegisterClipboardFormatW(CFSTR_FILECONTENTS) as u16)
    };
    let drop_target: IDropTarget =
        DropTarget { window, target, trace, descriptor, contents, offer: RefCell::new(Offer::Nothing) }.into();
    let mut windows = vec![window];
    windows.extend(children(window));
    let mut registered = vec![];
    for w in windows {
        // SAFETY: plain calls on windows of this thread's; OLE holds its own
        // reference to the target while it's registered (until the window goes).
        let taken = unsafe {
            let _ = RevokeDragDrop(w);
            RegisterDragDrop(w, &drop_target)
        };
        match taken {
            Ok(()) => registered.push(class_of(w)),
            // The window itself must take it; windows in it of another
            // process's (WebView2's) can't be, and needn't.
            Err(e) if w == window => return Err(e),
            Err(_) => {}
        }
    }
    Ok(registered)
}

/// Every window inside `parent`, at any depth.
fn children(parent: HWND) -> Vec<HWND> {
    unsafe extern "system" fn each(child: HWND, found: LPARAM) -> BOOL {
        // SAFETY: `found` is the Vec below, alive through EnumChildWindows.
        unsafe { (*(found.0 as *mut Vec<HWND>)).push(child) };
        true.into()
    }
    let mut found: Vec<HWND> = vec![];
    // SAFETY: the callback writes only to `found`, during the call.
    unsafe {
        let _ = EnumChildWindows(Some(parent), Some(each), LPARAM(&mut found as *mut Vec<HWND> as isize));
    }
    found
}

fn class_of(w: HWND) -> String {
    let mut name = [0u16; 256];
    // SAFETY: the buffer's length is passed with it.
    let len = unsafe { GetClassNameW(w, &mut name) };
    String::from_utf16_lossy(&name[..len.max(0) as usize])
}

/// A virtual file a drag offers: its place in the drag (FileContents'
/// lindex), its name and size if given.
#[derive(Clone, Debug, PartialEq)]
struct Described {
    index: i32,
    name: String,
    size: Option<u64>,
}

/// How the drag over the window is taken.
#[derive(Clone, Debug, PartialEq)]
enum Offer {
    Paths(Vec<PathBuf>),
    Virtual(Vec<Described>),
    Nothing,
}

#[implement(IDropTarget)]
struct DropTarget {
    window: HWND,
    target: Box<dyn Target>,
    trace: fn(&str),
    descriptor: u16,
    contents: u16,
    /// The drag over the window now, as taken when it came.
    offer: RefCell<Offer>,
}

impl DropTarget {
    /// Screen point `pt` in the window's own pixels.
    fn at(&self, pt: &POINTL) -> (f64, f64) {
        let mut p = POINT { x: pt.x, y: pt.y };
        // SAFETY: a plain conversion on the window's handle.
        let _ = unsafe { ScreenToClient(self.window, &mut p) };
        (f64::from(p.x), f64::from(p.y))
    }

    /// How `data` is taken: its paths, else the virtual files in it the
    /// target takes, else not.
    fn offer_of(&self, data: &IDataObject) -> Offer {
        if let Some(paths) = paths_in(data) {
            return Offer::Paths(paths);
        }
        let taken: Vec<Described> = described_in(data, self.descriptor)
            .unwrap_or_default()
            .into_iter()
            .filter(|f| self.target.takes(&f.name))
            .collect();
        if taken.is_empty() { Offer::Nothing } else { Offer::Virtual(taken) }
    }

    /// Receives the virtual files `files` of `data`, let go on the window.
    fn receive(&self, data: &IDataObject, files: Vec<Described>) {
        let incoming: Vec<(Described, Box<dyn Incoming>)> =
            files.into_iter().filter_map(|f| self.target.incoming(&f.name, f.size).map(|sink| (f, sink))).collect();
        if incoming.is_empty() {
            return;
        }
        let (contents, trace) = (self.contents, self.trace);
        // SAFETY: plain calls on the drag's data, valid through Drop.
        let copier = data
            .cast::<IDataObjectAsyncCapability>()
            .ok()
            .filter(|a| unsafe { a.GetAsyncMode().is_ok_and(|on| on.as_bool()) && a.StartOperation(None).is_ok() });
        let Some(copier) = copier else {
            trace("the source can't wait: read before the drop returns");
            read_all(data, contents, incoming, trace);
            return;
        };
        trace("the source waits (StartOperation): read on a thread of its own");
        // The data as any thread may use it (it's another app's, through COM).
        let agile = match AgileReference::new(data) {
            Ok(agile) => agile,
            Err(e) => {
                trace(&format!("the drag's data can't go to another thread ({e}): read here"));
                let ok = read_all(data, contents, incoming, trace);
                // SAFETY: the operation begun above, ended.
                let _ = unsafe { copier.EndOperation(if ok { S_OK } else { E_ABORT }, None, DROPEFFECT_COPY.0) };
                return;
            }
        };
        let spawned = std::thread::Builder::new().name("drop-receive".into()).spawn(move || {
            // SAFETY: COM for this thread, undone as it ends.
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            match agile.resolve() {
                Ok(data) => {
                    let ok = read_all(&data, contents, incoming, trace);
                    if let Ok(copier) = data.cast::<IDataObjectAsyncCapability>() {
                        // SAFETY: the operation begun on the window's thread, ended.
                        let ended =
                            unsafe { copier.EndOperation(if ok { S_OK } else { E_ABORT }, None, DROPEFFECT_COPY.0) };
                        trace(&format!("EndOperation: {ended:?}"));
                    }
                }
                Err(e) => {
                    let why = format!("The app it came from let go of it ({e}).");
                    for (_, sink) in incoming {
                        sink.done(Err(why.clone()));
                    }
                }
            }
            // SAFETY: as begun above.
            unsafe { CoUninitialize() };
        });
        if let Err(e) = spawned {
            trace(&format!("no thread to read on: {e}"));
        }
    }
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        data: Ref<'_, IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let data = data.ok()?;
        (self.trace)(&format!("a drag over the window offers {}", formats(data).join(", ")));
        let offer = self.offer_of(data);
        let at = self.at(pt);
        match &offer {
            Offer::Paths(paths) => self.target.heard(Event::Enter { paths: paths.clone(), names: vec![], at }),
            Offer::Virtual(files) => {
                let names = files.iter().map(|f| f.name.clone()).collect();
                self.target.heard(Event::Enter { paths: vec![], names, at })
            }
            Offer::Nothing => {}
        }
        // SAFETY: OLE passes the effect to answer.
        unsafe { *effect = answer(&offer) };
        *self.offer.borrow_mut() = offer;
        Ok(())
    }

    fn DragOver(&self, _: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let offer = self.offer.borrow();
        if *offer != Offer::Nothing {
            self.target.heard(Event::Over { at: self.at(pt) });
        }
        // SAFETY: as in DragEnter.
        unsafe { *effect = answer(&offer) };
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        if self.offer.replace(Offer::Nothing) != Offer::Nothing {
            self.target.heard(Event::Leave);
        }
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<'_, IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let offer = self.offer.replace(Offer::Nothing);
        // SAFETY: as in DragEnter.
        unsafe { *effect = answer(&offer) };
        let data = data.ok()?;
        let at = self.at(pt);
        match offer {
            // Asked again: the app's own drag answers with the files made by now.
            Offer::Paths(_) => self.target.heard(Event::Drop { paths: paths_in(data).unwrap_or_default(), at }),
            Offer::Virtual(files) => {
                (self.trace)(&format!("let go with {} virtual file(s) to receive", files.len()));
                self.target.heard(Event::Drop { paths: vec![], at });
                self.receive(data, files);
            }
            Offer::Nothing => {}
        }
        Ok(())
    }
}

/// The effect a drag that's taken as `offer` is answered.
fn answer(offer: &Offer) -> DROPEFFECT {
    if *offer == Offer::Nothing { DROPEFFECT_NONE } else { DROPEFFECT_COPY }
}

fn format(cf: u16, lindex: i32, tymed: TYMED) -> FORMATETC {
    FORMATETC { cfFormat: cf, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex, tymed: tymed.0 as u32 }
}

/// The paths in `data` (CF_HDROP), if it has them (perhaps none: the app's
/// own drag before its files are made).
fn paths_in(data: &IDataObject) -> Option<Vec<PathBuf>> {
    // SAFETY: the medium is read while held, then released.
    unsafe {
        let mut medium = data.GetData(&format(CF_HDROP.0, -1, TYMED_HGLOBAL)).ok()?;
        let drop = HDROP(medium.u.hGlobal.0);
        let mut paths = vec![];
        for i in 0..DragQueryFileW(drop, u32::MAX, None) {
            let len = DragQueryFileW(drop, i, None) as usize;
            let mut path = vec![0u16; len + 1];
            DragQueryFileW(drop, i, Some(&mut path));
            paths.push(OsString::from_wide(&path[..len]).into());
        }
        ReleaseStgMedium(&mut medium);
        Some(paths)
    }
}

/// The virtual files `data` offers (FileGroupDescriptorW, clipboard format
/// `descriptor`), folders left out.
fn described_in(data: &IDataObject, descriptor: u16) -> Option<Vec<Described>> {
    // SAFETY: the block is read within its size while locked, then released.
    unsafe {
        let mut medium = data.GetData(&format(descriptor, -1, TYMED_HGLOBAL)).ok()?;
        let block = medium.u.hGlobal;
        let size = GlobalSize(block);
        let at = GlobalLock(block) as *const u8;
        let mut files = vec![];
        if !at.is_null() && size >= 4 {
            let count = std::ptr::read_unaligned(at as *const u32) as usize;
            let each = std::mem::size_of::<FILEDESCRIPTORW>();
            for index in 0..count.min((size - 4) / each) {
                let fd = std::ptr::read_unaligned(at.add(4 + index * each) as *const FILEDESCRIPTORW);
                files.push((index, fd));
            }
            let _ = GlobalUnlock(block);
        }
        ReleaseStgMedium(&mut medium);
        Some(files.into_iter().filter_map(|(index, fd)| described(index, &fd)).collect())
    }
}

/// File descriptor `fd`, the `index`th in its drag, unless it's a folder.
fn described(index: usize, fd: &FILEDESCRIPTORW) -> Option<Described> {
    let flags = fd.dwFlags;
    if flags & FD_ATTRIBUTES.0 as u32 != 0 && fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
        return None;
    }
    // (Copied out: the struct is packed, so its fields can't be borrowed.)
    let file = fd.cFileName;
    let len = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    let path = String::from_utf16_lossy(&file[..len]);
    // (A file in a folder of the drag's comes as "folder\name".)
    let name = path.rsplit(['\\', '/']).next().unwrap_or_default().to_string();
    let size =
        (flags & FD_FILESIZE.0 as u32 != 0).then(|| u64::from(fd.nFileSizeHigh) << 32 | u64::from(fd.nFileSizeLow));
    (!name.is_empty()).then_some(Described { index: i32::try_from(index).ok()?, name, size })
}

/// Reads each of `incoming` from `data` (FileContents is clipboard format
/// `contents`), one after another; true if all came whole.
fn read_all(data: &IDataObject, contents: u16, incoming: Vec<(Described, Box<dyn Incoming>)>, trace: fn(&str)) -> bool {
    let mut all = true;
    for (file, mut sink) in incoming {
        let read = read_one(data, contents, file.index, sink.as_mut());
        trace(&format!(
            "{}: {}",
            file.name,
            read.as_ref().map(|n| format!("{n} bytes read")).unwrap_or_else(|e| e.clone())
        ));
        all &= read.is_ok();
        sink.done(read.map(|_| ()));
    }
    all
}

/// Reads virtual file `index` of `data` into `sink`: how many bytes.
fn read_one(
    data: &IDataObject,
    contents: u16,
    index: i32,
    sink: &mut dyn Incoming,
) -> std::result::Result<u64, String> {
    let asked = format(contents, index, TYMED(TYMED_ISTREAM.0 | TYMED_HGLOBAL.0));
    // SAFETY: the medium is read while held, then released.
    unsafe {
        let mut medium = data.GetData(&asked).map_err(|e| format!("Its app didn't give it: {}", e.message()))?;
        let read = if medium.tymed == TYMED_ISTREAM.0 as u32 {
            match &*medium.u.pstm {
                Some(stream) => read_stream(stream, sink),
                None => Err("Its app gave nothing.".to_string()),
            }
        } else if medium.tymed == TYMED_HGLOBAL.0 as u32 {
            read_block(medium.u.hGlobal, sink)
        } else {
            Err("Its app gave it in a form VODscribe can't read.".to_string())
        };
        ReleaseStgMedium(&mut medium);
        read
    }
}

fn read_stream(stream: &IStream, sink: &mut dyn Incoming) -> std::result::Result<u64, String> {
    let mut buf = vec![0u8; CHUNK];
    let mut total = 0;
    loop {
        let mut got = 0u32;
        // SAFETY: at most the buffer's length is written into it.
        let hr = unsafe { stream.Read(buf.as_mut_ptr().cast(), buf.len() as u32, Some(&mut got)) };
        if hr.is_err() {
            return Err(format!("It couldn't be read: {}", hr.message()));
        }
        if got == 0 {
            return Ok(total);
        }
        sink.write(&buf[..got as usize]).map_err(|e| e.to_string())?;
        total += u64::from(got);
    }
}

fn read_block(block: HGLOBAL, sink: &mut dyn Incoming) -> std::result::Result<u64, String> {
    // SAFETY: the block is read within its size while locked.
    unsafe {
        let size = GlobalSize(block);
        let at = GlobalLock(block) as *const u8;
        if at.is_null() {
            return Err("It couldn't be read.".to_string());
        }
        let written = std::slice::from_raw_parts(at, size).chunks(CHUNK).try_for_each(|part| sink.write(part));
        let _ = GlobalUnlock(block);
        written.map(|()| size as u64).map_err(|e| e.to_string())
    }
}

/// The formats `data` offers, by name, for the trace.
fn formats(data: &IDataObject) -> Vec<String> {
    let mut names = vec![];
    // SAFETY: each FORMATETC is read as the enumerator fills it; a target
    // device it carries is freed.
    unsafe {
        let Ok(each) = data.EnumFormatEtc(DATADIR_GET.0 as u32) else { return vec!["(no list)".into()] };
        let mut f = [FORMATETC::default()];
        let mut got = 0u32;
        while each.Next(&mut f, Some(&mut got)) == S_OK && got == 1 {
            names.push(format!("{} ({})", format_name(f[0].cfFormat), f[0].tymed));
            if !f[0].ptd.is_null() {
                CoTaskMemFree(Some(f[0].ptd as *const _));
            }
        }
    }
    names
}

fn format_name(cf: u16) -> String {
    if cf == CF_HDROP.0 {
        return "CF_HDROP".into();
    }
    let mut name = [0u16; 128];
    // SAFETY: the buffer's length is passed with it.
    let len = unsafe { GetClipboardFormatNameW(u32::from(cf), &mut name) };
    if len > 0 { String::from_utf16_lossy(&name[..len as usize]) } else { format!("#{cf}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fd(name: &str, flags: FD_FLAGS, attributes: u32, size: u64) -> FILEDESCRIPTORW {
        let mut file = [0u16; 260];
        for (to, c) in file.iter_mut().zip(name.encode_utf16()) {
            *to = c;
        }
        FILEDESCRIPTORW {
            dwFlags: flags.0 as u32,
            dwFileAttributes: attributes,
            nFileSizeHigh: (size >> 32) as u32,
            nFileSizeLow: size as u32,
            cFileName: file,
            ..Default::default()
        }
    }

    #[test]
    fn a_descriptor_gives_the_name_without_its_folder_and_the_size_if_said() {
        let big = 5 << 32 | 7;
        assert_eq!(
            described(2, &fd("clips\\a b.mp4", FD_FLAGS(FD_FILESIZE.0 | FD_ATTRIBUTES.0), 0x80, big)),
            Some(Described { index: 2, name: "a b.mp4".into(), size: Some(big) })
        );
        assert_eq!(
            described(0, &fd("c.mkv", FD_FLAGS(0), 0, 99)),
            Some(Described { index: 0, name: "c.mkv".into(), size: None }),
            "no size said"
        );
        assert_eq!(described(1, &fd("clips", FD_ATTRIBUTES, FILE_ATTRIBUTE_DIRECTORY.0, 0)), None, "a folder");
        assert_eq!(described(1, &fd("", FD_FLAGS(0), 0, 0)), None, "no name");
    }

    #[test]
    fn a_drag_taken_is_answered_copy() {
        assert_eq!(answer(&Offer::Nothing), DROPEFFECT_NONE);
        assert_eq!(answer(&Offer::Paths(vec![])), DROPEFFECT_COPY, "the app's own, nothing made yet");
        assert_eq!(answer(&Offer::Virtual(vec![])), DROPEFFECT_COPY);
    }
}
