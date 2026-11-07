//! Windows (OLE): the data of a drag of files out of a window, made or not.
//!
//! [`Drag::new`] on the UI thread, with OLE initialized there, gives the
//! drag's `IDataObject` ([`Drag::data_object`]) and `IDropSource`
//! ([`Drag::drop_source`]). Set its picture if you like
//! (IDragSourceHelper, on the data object), call `DoDragDrop` with them and
//! `DROPEFFECT_COPY`, then [`Drag::returned`] with what it returned. Begin
//! only while the button is down ([`button_held`]).
//!
//! Each item is offered two ways in one drag, and the app it's let go on
//! picks:
//!
//! - as a virtual file (FileGroupDescriptorW + FileContents, with
//!   IDataObjectAsyncCapability): Explorer and other shell-based receivers
//!   take this, even when the file itself is there too. They ask for the
//!   contents after the drop, on a thread of their call into the data
//!   object, where the answer waits until the item is made; the drag itself
//!   is over at once, and Explorer shows its own copy dialog meanwhile. An
//!   item that won't be made is answered "canceled", which Explorer drops
//!   silently (any other error gets its error box);
//! - as the file itself (CF_HDROP), answered only when every item is made,
//!   never held (holding it stalls the receiver's drop, and the drag's loop
//!   with it, with nothing on screen): an app that takes real files only gets
//!   them if they're made by the drop; if not, nothing lands, and the outcome
//!   says it took finished files only ([`Outcome::made`] false).
//!
//! Your own window's drop target asks for CF_HDROP on the UI thread as the
//! drag comes over it, and is answered at once with the files made so far:
//! refused, it would ignore the drag (no drag-over, no drop).
//! Every other format (the drag's picture from IDragSourceHelper, Explorer's
//! drop description and Performed DropEffect) goes to an inner store
//! (SHCreateDataObject), which answers for it.

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::Com::*;
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::{Ending, Event, Failure, Item, Outcome, State, settled, trace};

/// How often an answer waiting for an item looks at it.
const TICK: Duration = Duration::from_millis(100);

/// What's known of the drag as it goes; shared by the UI thread and the
/// receivers' calls, which come on other threads.
struct Inner {
    items: Vec<Box<dyn Item>>,
    ui_thread: u32,
    /// The app's own window (its root's handle).
    own: isize,
    /// The button came up (anywhere).
    dropped: AtomicBool,
    /// It came up over one of the app's own windows, or the app's own window
    /// asked for the files after it did.
    own_drop: AtomicBool,
    /// The process whose window it came up over.
    receiver_pid: AtomicU32,
    /// The receiver copies after the drop (StartOperation); its
    /// EndOperation ends the drag.
    extracting: AtomicBool,
    /// FileContents asked for after the drop.
    contents_asked: AtomicBool,
    /// Each item's contents handed over (a stream given).
    handed: Vec<AtomicBool>,
    /// CF_HDROP answered to another app after the drop.
    files_given: AtomicBool,
    /// CF_HDROP asked for by another app after the drop while an item was
    /// still being made, and refused.
    files_refused: AtomicBool,
    /// Over: told how it ended.
    over: AtomicBool,
    failure: Mutex<Option<String>>,
    on: Arc<dyn Fn(Event) + Send + Sync>,
}

impl Inner {
    fn on_ui(&self) -> bool {
        // SAFETY: a plain query.
        unsafe { GetCurrentThreadId() == self.ui_thread }
    }

    fn states(&self) -> Vec<State> {
        self.items.iter().map(|i| i.state()).collect()
    }

    /// Tells how the drag ended, once, from a thread of its own.
    fn end(&self, outcome: Outcome) {
        if self.over.swap(true, Ordering::SeqCst) {
            return;
        }
        trace(&format!("over: {outcome:?}"));
        let on = self.on.clone();
        std::thread::spawn(move || on(Event::Ended(outcome)));
    }

    /// Waits until item `i` is made (its path) or won't be (why), or the
    /// drag is over (None).
    fn wait_for(&self, i: usize) -> Option<std::result::Result<PathBuf, String>> {
        let item = self.items.get(i)?;
        loop {
            match item.state() {
                State::Ready(path) => return Some(Ok(path)),
                State::Failed(why) => return Some(Err(why)),
                State::Making(_) => {}
            }
            if self.over.load(Ordering::SeqCst) {
                return None;
            }
            std::thread::sleep(TICK);
        }
    }

    /// FILEGROUPDESCRIPTORW: one descriptor for each item, by name, with no
    /// size (it isn't known before the file is made: Explorer asks the
    /// stream).
    fn descriptors(&self) -> Result<STGMEDIUM> {
        let names: Vec<String> = self.items.iter().map(|i| i.name()).collect();
        let n = names.len();
        // SAFETY: a count, then `n` descriptors, written within the block's size.
        unsafe {
            let h = GlobalAlloc(GHND, 4 + n * std::mem::size_of::<FILEDESCRIPTORW>())?;
            let p = GlobalLock(h) as *mut u8;
            *(p as *mut u32) = n as u32;
            let fds = p.add(4) as *mut FILEDESCRIPTORW;
            for (i, name) in names.iter().enumerate() {
                let mut file = [0u16; 260];
                for (to, c) in file.iter_mut().take(259).zip(name.encode_utf16()) {
                    *to = c;
                }
                let fd = FILEDESCRIPTORW { dwFlags: FD_PROGRESSUI.0 as u32, cFileName: file, ..Default::default() };
                std::ptr::write_unaligned(fds.add(i), fd);
            }
            let _ = GlobalUnlock(h);
            Ok(global(h))
        }
    }

    /// GetData of FileContents for item `lindex`: after the drop, its file
    /// once it's made (waited for here, on the receiver's thread).
    fn contents(&self, lindex: i32) -> Result<STGMEDIUM> {
        // Explorer asks once on hover too, for its preview: not made for that.
        if !self.dropped.load(Ordering::SeqCst) {
            return Err(E_FAIL.into());
        }
        self.contents_asked.store(true, Ordering::SeqCst);
        let i = usize::try_from(lindex).map_err(|_| Error::from(DV_E_LINDEX))?;
        trace(&format!("contents {i} asked for"));
        let own = self.own_drop.load(Ordering::SeqCst);
        if let Some(item) = self.items.get(i).filter(|item| !own && matches!(item.state(), State::Making(_))) {
            trace(&format!("contents {i}: waited for, still being made"));
            item.waited_for();
        }
        match self.wait_for(i) {
            Some(Ok(path)) => {
                let wide = HSTRING::from(path.as_os_str());
                // SAFETY: a stream over the file, owned by the medium handed over.
                let stream =
                    unsafe { SHCreateStreamOnFileEx(&wide, (STGM_READ | STGM_SHARE_DENY_WRITE).0, 0, false, None)? };
                self.handed[i].store(true, Ordering::SeqCst);
                trace(&format!("contents {i} handed over"));
                Ok(STGMEDIUM {
                    tymed: TYMED_ISTREAM.0 as u32,
                    u: STGMEDIUM_0 { pstm: std::mem::ManuallyDrop::new(Some(stream)) },
                    pUnkForRelease: Default::default(),
                })
            }
            Some(Err(why)) => {
                trace(&format!("contents {i}: not made ({why}), answered canceled"));
                *self.failure.lock().unwrap() = Some(why);
                Err(HRESULT::from_win32(ERROR_CANCELLED.0).into())
            }
            None => Err(HRESULT::from_win32(ERROR_CANCELLED.0).into()),
        }
    }

    /// GetData of CF_HDROP: on the UI thread (the app's own window) the
    /// files made so far, at once; from other apps, all of them if all are
    /// made, else refused.
    fn files(&self) -> Result<STGMEDIUM> {
        let states = self.states();
        let dropped = self.dropped.load(Ordering::SeqCst);
        if self.on_ui() {
            if dropped {
                self.own_drop.store(true, Ordering::SeqCst);
            }
            let ready: Vec<_> =
                states.into_iter().filter_map(|s| if let State::Ready(p) = s { Some(p) } else { None }).collect();
            return hdrop(&ready);
        }
        match settled(states) {
            Some(Ok(paths)) => {
                if dropped {
                    self.files_given.store(true, Ordering::SeqCst);
                    trace(&format!("files asked for after the drop: given, {}", paths.len()));
                }
                hdrop(&paths)
            }
            Some(Err(why)) => {
                if dropped {
                    trace(&format!("files asked for after the drop: refused, not made ({why})"));
                    *self.failure.lock().unwrap() = Some(why);
                }
                Err(DV_E_FORMATETC.into())
            }
            None => {
                if dropped {
                    trace("files asked for after the drop: refused, still being made");
                    self.files_refused.store(true, Ordering::SeqCst);
                }
                Err(DV_E_FORMATETC.into())
            }
        }
    }
}

/// A medium holding global memory `h`, the receiver's to free.
fn global(h: HGLOBAL) -> STGMEDIUM {
    STGMEDIUM { tymed: TYMED_HGLOBAL.0 as u32, u: STGMEDIUM_0 { hGlobal: h }, pUnkForRelease: Default::default() }
}

/// CF_HDROP of `paths`: DROPFILES, then the paths, wide, each ended by a
/// nul, and one more.
fn hdrop(paths: &[PathBuf]) -> Result<STGMEDIUM> {
    let mut wide: Vec<u16> = vec![];
    for p in paths {
        wide.extend(p.as_os_str().encode_wide());
        wide.push(0);
    }
    wide.push(0);
    let head = std::mem::size_of::<DROPFILES>();
    // SAFETY: the block is allocated for the header and the names, then
    // filled within that size.
    unsafe {
        let h = GlobalAlloc(GHND, head + wide.len() * 2)?;
        let p = GlobalLock(h) as *mut u8;
        *(p as *mut DROPFILES) =
            DROPFILES { pFiles: head as u32, pt: POINT::default(), fNC: false.into(), fWide: true.into() };
        std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, p.add(head), wide.len() * 2);
        let _ = GlobalUnlock(h);
        Ok(global(h))
    }
}

#[implement(IDataObject, IDataObjectAsyncCapability)]
struct Data {
    drag: Arc<Inner>,
    descriptor: u16,
    contents: u16,
    /// Holds what others set on the drag and answers for it.
    store: IDataObject,
}

impl Data {
    fn formats(&self) -> Vec<FORMATETC> {
        let format = |cf: u16, lindex: i32, tymed: TYMED| FORMATETC {
            cfFormat: cf,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex,
            tymed: tymed.0 as u32,
        };
        let mut all = vec![format(self.descriptor, -1, TYMED_HGLOBAL)];
        all.extend((0..self.drag.items.len()).map(|i| format(self.contents, i as i32, TYMED_ISTREAM)));
        all.push(format(CF_HDROP.0, -1, TYMED_HGLOBAL));
        all
    }
}

impl IDataObject_Impl for Data_Impl {
    fn GetData(&self, f: *const FORMATETC) -> Result<STGMEDIUM> {
        // A receiver may pass null; reading it would bring the app down.
        // SAFETY: otherwise COM hands a valid FORMATETC for the call.
        let Some(f) = (unsafe { f.as_ref() }) else {
            return Err(E_INVALIDARG.into());
        };
        let hglobal = f.tymed & TYMED_HGLOBAL.0 as u32 != 0;
        if f.cfFormat == self.descriptor && hglobal {
            return self.drag.descriptors();
        }
        if f.cfFormat == self.contents && f.tymed & TYMED_ISTREAM.0 as u32 != 0 {
            return self.drag.contents(f.lindex);
        }
        if f.cfFormat == CF_HDROP.0 && hglobal {
            return self.drag.files();
        }
        // SAFETY: passed on as it came.
        unsafe { self.store.GetData(f) }
    }

    fn GetDataHere(&self, _: *const FORMATETC, _: *mut STGMEDIUM) -> Result<()> {
        Err(DV_E_FORMATETC.into())
    }

    fn QueryGetData(&self, f: *const FORMATETC) -> HRESULT {
        // SAFETY: as in GetData.
        let Some(f) = (unsafe { f.as_ref() }) else {
            return E_INVALIDARG;
        };
        // CF_HDROP is said to be there even while it would be refused: an app
        // that takes real files only then takes the drop, and its refusal is
        // seen (the outcome says so).
        let ours = (f.cfFormat == self.descriptor || f.cfFormat == CF_HDROP.0) && f.tymed & TYMED_HGLOBAL.0 as u32 != 0
            || f.cfFormat == self.contents && f.tymed & TYMED_ISTREAM.0 as u32 != 0;
        if ours { S_OK } else { unsafe { self.store.QueryGetData(f) } }
    }

    fn GetCanonicalFormatEtc(&self, _: *const FORMATETC, out: *mut FORMATETC) -> HRESULT {
        // SAFETY: a FORMATETC to write, unless the caller passed null.
        let Some(out) = (unsafe { out.as_mut() }) else {
            return E_INVALIDARG;
        };
        out.ptd = std::ptr::null_mut();
        DATA_S_SAMEFORMATETC
    }

    fn SetData(&self, f: *const FORMATETC, m: *const STGMEDIUM, release: BOOL) -> Result<()> {
        // SAFETY: passed on as it came.
        unsafe { self.store.SetData(f, m, release.as_bool()) }
    }

    fn EnumFormatEtc(&self, direction: u32) -> Result<IEnumFORMATETC> {
        if direction == DATADIR_GET.0 as u32 {
            // SAFETY: the formats are copied into the enumerator.
            unsafe { SHCreateStdEnumFmtEtc(&self.formats()) }
        } else {
            Err(E_NOTIMPL.into())
        }
    }

    fn DAdvise(&self, _: *const FORMATETC, _: u32, _: Ref<IAdviseSink>) -> Result<u32> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn DUnadvise(&self, _: u32) -> Result<()> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
}

impl IDataObjectAsyncCapability_Impl for Data_Impl {
    fn SetAsyncMode(&self, _: BOOL) -> Result<()> {
        Ok(())
    }

    fn GetAsyncMode(&self) -> Result<BOOL> {
        Ok(true.into())
    }

    fn StartOperation(&self, _: Ref<IBindCtx>) -> Result<()> {
        trace("the receiver copies on its own thread (StartOperation)");
        self.drag.extracting.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn InOperation(&self) -> Result<BOOL> {
        Ok(self.drag.extracting.load(Ordering::SeqCst).into())
    }

    /// The receiver's end of its copies: S_OK when written (also when it was
    /// answered "canceled": `failure` says), ERROR_CANCELLED when its own
    /// Cancel was pressed (heard only here, once the item it waited for is
    /// made: Explorer's dialog says "Canceling..." until then).
    fn EndOperation(&self, hr: HRESULT, _: Ref<IBindCtx>, _: u32) -> Result<()> {
        let drag = &self.drag;
        trace(&format!("the receiver's copies are over (EndOperation {:#x})", hr.0));
        drag.extracting.store(false, Ordering::SeqCst);
        let failure = drag.failure.lock().unwrap().take();
        if hr.is_err() && failure.is_none() {
            for (item, handed) in drag.items.iter().zip(&drag.handed) {
                if !handed.load(Ordering::SeqCst) {
                    item.cancelled();
                }
            }
        }
        drag.end(copied(hr.is_ok(), failure));
        Ok(())
    }
}

#[implement(IDropSource)]
struct Source {
    drag: Arc<Inner>,
}

impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            return DRAGDROP_S_CANCEL;
        }
        if keys.0 & MK_LBUTTON.0 == 0 {
            let drag = &self.drag;
            let (window, pid) = window_under_pointer();
            // SAFETY: plain queries.
            let own = window.0 as isize == drag.own || pid == unsafe { GetCurrentProcessId() };
            drag.receiver_pid.store(pid, Ordering::SeqCst);
            drag.own_drop.fetch_or(own, Ordering::SeqCst);
            drag.dropped.store(true, Ordering::SeqCst);
            trace(&format!("let go over {} (process {pid})", if own { "the app's own window" } else { "another app" }));
            if !own {
                (drag.on)(Event::LetGo);
            }
            return DRAGDROP_S_DROP;
        }
        S_OK
    }

    fn GiveFeedback(&self, _: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// What was seen of a drag by the time DoDragDrop returned, as its outcome
/// stands then (the receiver's copies, if it makes them, end it later).
#[derive(Default)]
struct Returned {
    called_off: bool,
    /// The button came up over a window that took the drop.
    taken_drop: bool,
    /// ...over another app's window (one that may have refused it).
    over_other: bool,
    own: bool,
    /// An item wasn't made as it returned.
    unmade: bool,
    contents_asked: bool,
    files_given: bool,
    files_refused: bool,
    failure: Option<String>,
}

/// The outcome of a drag as DoDragDrop returned (`Returned`), naming the
/// receiver (`receiver`) only when it took finished files only. Let go on
/// another app that refused the drop (DRAGDROP_S_DROP, no effect; Escape
/// is DRAGDROP_S_CANCEL) while an item was still being made is that too:
/// it may take the file once it's made (CapCut's timeline refuses one
/// whose address it can't read yet).
fn returned(r: Returned, receiver: impl FnOnce() -> Option<String>) -> Outcome {
    let refused = !r.taken_drop && r.over_other && r.unmade && r.failure.is_none();
    let ending = match () {
        _ if r.called_off => Ending::CalledOff,
        _ if r.own => Ending::Back,
        _ if !r.taken_drop && !refused => Ending::Nowhere,
        _ => Ending::Elsewhere,
    };
    let none = Outcome { ending, delivered: false, copied: false, failure: None, made: true, receiver: None };
    let failure = r.failure.map(Failure::Failed);
    match ending {
        Ending::CalledOff | Ending::Back => none,
        Ending::Nowhere => Outcome { failure, ..none },
        Ending::Elsewhere => {
            let refused_unmade = refused || (r.files_refused && !r.files_given && !r.contents_asked);
            let delivered = failure.is_none() && (r.files_given || r.contents_asked);
            Outcome {
                delivered,
                // Read as virtual files, not given the files themselves.
                copied: delivered && !r.files_given,
                failure,
                made: !refused_unmade,
                receiver: if refused_unmade { receiver() } else { None },
                ..none
            }
        }
    }
}

/// The outcome of a drag whose receiver copied the files (EndOperation):
/// delivered if its copies were written (`written`) and none failed.
fn copied(written: bool, failure: Option<String>) -> Outcome {
    Outcome {
        ending: Ending::Elsewhere,
        delivered: written && failure.is_none(),
        copied: true,
        failure: failure.map(Failure::Failed),
        made: true,
        receiver: None,
    }
}

/// The top-level window under the pointer, and its process.
fn window_under_pointer() -> (HWND, u32) {
    // SAFETY: plain queries.
    unsafe {
        let mut at = POINT::default();
        if GetCursorPos(&mut at).is_err() {
            return (HWND::default(), 0);
        }
        let window = GetAncestor(WindowFromPoint(at), GA_ROOT);
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        (window, pid)
    }
}

/// The name process `pid`'s program gives itself (its version information's
/// FileDescription: "VLC media player", "Windows PowerShell"), else its
/// file's (`stem_name`).
fn app_name(pid: u32) -> Option<String> {
    // SAFETY: plain queries; the process handle is closed before returning.
    let exe = unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut name = [0u16; 1024];
        let mut len = name.len() as u32;
        let got = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(name.as_mut_ptr()), &mut len);
        let _ = CloseHandle(process);
        got.ok()?;
        PathBuf::from(String::from_utf16_lossy(&name[..len as usize]))
    };
    described(&exe).or_else(|| stem_name(&exe))
}

/// Program `exe`'s file name without its extension, capitalized, as a name
/// for it: "Kdenlive" for kdenlive.exe, which gives no description.
fn stem_name(exe: &Path) -> Option<String> {
    let stem = exe.file_stem()?.to_string_lossy();
    let mut chars = stem.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

/// The FileDescription in program `exe`'s version information, if it has one.
fn described(exe: &Path) -> Option<String> {
    let path = HSTRING::from(exe.as_os_str());
    // SAFETY: the block is the size asked for; VerQueryValueW points into it,
    // read only while it lives, within the lengths it gives.
    unsafe {
        let size = GetFileVersionInfoSizeW(&path, None);
        if size == 0 {
            return None;
        }
        let mut block = vec![0u8; size as usize];
        GetFileVersionInfoW(&path, None, size, block.as_mut_ptr().cast()).ok()?;
        let query = |key: &str| -> Option<(*const u8, usize)> {
            let key = HSTRING::from(key);
            let (mut at, mut len) = (std::ptr::null_mut(), 0u32);
            VerQueryValueW(block.as_ptr().cast(), &key, &mut at, &mut len)
                .as_bool()
                .then_some((at as *const u8, len as usize))
        };
        // Its languages and code pages, then US English's in Unicode, as most have.
        let mut tried: Vec<(u16, u16)> = match query("\\VarFileInfo\\Translation") {
            Some((at, len)) => std::slice::from_raw_parts(at as *const u16, len / 2)
                .chunks(2)
                .filter(|p| p.len() == 2)
                .map(|p| (p[0], p[1]))
                .collect(),
            None => vec![],
        };
        tried.push((0x0409, 0x04b0));
        for (language, page) in tried {
            let Some((at, len)) = query(&format!("\\StringFileInfo\\{language:04x}{page:04x}\\FileDescription")) else {
                continue;
            };
            // (A length in characters, its nul counted.)
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(at as *const u16, len));
            let text = text.trim_end_matches('\0').trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
        None
    }
}

/// The drag's state, and its data object over it.
fn data_object(
    items: Vec<Box<dyn Item>>,
    own: isize,
    on: Arc<dyn Fn(Event) + Send + Sync>,
) -> Result<(Arc<Inner>, IDataObject)> {
    let handed = items.iter().map(|_| AtomicBool::new(false)).collect();
    let drag = Arc::new(Inner {
        items,
        // SAFETY: a plain query.
        ui_thread: unsafe { GetCurrentThreadId() },
        own,
        dropped: false.into(),
        own_drop: false.into(),
        receiver_pid: 0.into(),
        extracting: false.into(),
        contents_asked: false.into(),
        handed,
        files_given: false.into(),
        files_refused: false.into(),
        over: false.into(),
        failure: Mutex::new(None),
        on,
    });
    // SAFETY: registering the shell's formats (the same ids each time) and
    // making an empty store.
    let data = unsafe {
        Data {
            drag: drag.clone(),
            descriptor: RegisterClipboardFormatW(CFSTR_FILEDESCRIPTORW) as u16,
            contents: RegisterClipboardFormatW(CFSTR_FILECONTENTS) as u16,
            store: SHCreateDataObject(None, None, None)?,
        }
    };
    Ok((drag, data.into()))
}

/// Whether the left mouse button is down now. A drag begun with no button
/// held never ends as one (it's let go at once where the pointer is): the
/// release may have come in the moment between the press and the drag's
/// start. See [`crate::ReleasedEarly`].
pub fn button_held() -> bool {
    // SAFETY: a plain query.
    unsafe { GetAsyncKeyState(VK_LBUTTON.0.into()) < 0 }
}

/// One drag's files, and what it carries for `DoDragDrop`.
pub struct Drag {
    inner: Arc<Inner>,
    data: IDataObject,
}

impl Drag {
    /// A drag of `items` out of `window` (any window of yours: its root is
    /// taken as your own), made on the UI thread with OLE initialized
    /// (`OleInitialize`); `on` hears how it goes. [`Event::LetGo`] comes
    /// from inside `DoDragDrop`, [`Event::Ended`] once it's over: for
    /// Explorer, once its copies are written, after `DoDragDrop` returned.
    pub fn new(window: HWND, items: Vec<Box<dyn Item>>, on: impl Fn(Event) + Send + Sync + 'static) -> Result<Drag> {
        // SAFETY: a plain query.
        let own = unsafe { GetAncestor(window, GA_ROOT) }.0 as isize;
        let (inner, data) = data_object(items, own, Arc::new(on))?;
        Ok(Drag { inner, data })
    }

    /// The data object for `DoDragDrop` (and IDragSourceHelper's picture).
    pub fn data_object(&self) -> &IDataObject {
        &self.data
    }

    /// The drop source for `DoDragDrop`: it ends the drag at the button's
    /// release, and Escape calls it off.
    pub fn drop_source(&self) -> IDropSource {
        Source { drag: self.inner.clone() }.into()
    }

    /// `DoDragDrop` returned `hr`, with `effect`: the drag is over on
    /// screen. Unless the receiver copies on a thread of its own (its
    /// EndOperation ends it then), `on` hears how it ended.
    pub fn returned(&self, hr: HRESULT, effect: DROPEFFECT) {
        let drag = &self.inner;
        trace(&format!("DoDragDrop returned {:#x}, effect {}", hr.0, effect.0));
        if drag.extracting.load(Ordering::SeqCst) && !drag.own_drop.load(Ordering::SeqCst) {
            // The receiver copies on its own thread now; its EndOperation ends it.
            return;
        }
        let unmade = drag.states().iter().any(|s| !matches!(s, State::Ready(_)));
        let seen = Returned {
            called_off: hr == DRAGDROP_S_CANCEL,
            taken_drop: hr == DRAGDROP_S_DROP && effect != DROPEFFECT_NONE,
            over_other: hr == DRAGDROP_S_DROP && !drag.own_drop.load(Ordering::SeqCst),
            own: drag.own_drop.load(Ordering::SeqCst),
            unmade,
            contents_asked: drag.contents_asked.load(Ordering::SeqCst),
            files_given: drag.files_given.load(Ordering::SeqCst),
            files_refused: drag.files_refused.load(Ordering::SeqCst),
            failure: drag.failure.lock().unwrap().take(),
        };
        let pid = drag.receiver_pid.load(Ordering::SeqCst);
        drag.end(returned(seen, || app_name(pid)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test's item: its state as `state` says, now; how often it was
    /// waited for, `waited`.
    #[derive(Default)]
    struct Test {
        name: &'static str,
        state: Arc<Mutex<Option<std::result::Result<PathBuf, String>>>>,
        waited: Arc<AtomicU32>,
    }

    impl Item for Test {
        fn name(&self) -> String {
            self.name.into()
        }
        fn state(&self) -> State {
            match &*self.state.lock().unwrap() {
                None => State::Making(None),
                Some(Ok(path)) => State::Ready(path.clone()),
                Some(Err(why)) => State::Failed(why.clone()),
            }
        }
        fn waited_for(&self) {
            self.waited.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn format(cf: u16, lindex: i32, tymed: TYMED) -> FORMATETC {
        FORMATETC {
            cfFormat: cf,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex,
            tymed: tymed.0 as u32,
        }
    }

    /// What a receiver on another app's thread is answered, before and after
    /// the drop, as the item's making goes: the virtual file's descriptor
    /// always; its contents never before the drop, and after it once made
    /// (waited for, which the item hears); the file itself only once made,
    /// refused meanwhile and the refusal remembered.
    #[test]
    fn a_receiver_is_answered_as_the_item_is_made() {
        // SAFETY: COM for the test's thread; the objects are used within it
        // and from the thread it starts.
        unsafe {
            let _ = OleInitialize(None);
        }
        let file = std::env::temp_dir().join(format!("dragndrop-source-test-{}.mp4", std::process::id()));
        std::fs::write(&file, b"made").unwrap();
        let state = Arc::new(Mutex::new(None));
        let waited = Arc::new(AtomicU32::new(0));
        let item = Test { name: "clip one.mp4", state: state.clone(), waited: waited.clone() };
        let (drag, data) = data_object(vec![Box::new(item)], 0, Arc::new(|_| {})).unwrap();
        // SAFETY: as above.
        let (descriptor, contents) = unsafe {
            (
                RegisterClipboardFormatW(CFSTR_FILEDESCRIPTORW) as u16,
                RegisterClipboardFormatW(CFSTR_FILECONTENTS) as u16,
            )
        };
        // Another app's thread (the test's own is the "UI thread" here).
        let ask = |(cf, lindex, tymed): (u16, i32, TYMED)| {
            let data = AgileReference::new(&data).unwrap();
            std::thread::spawn(move || unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                data.resolve().unwrap().GetData(&format(cf, lindex, tymed)).map(|_| ()).map_err(|e| e.code())
            })
        };
        let wait = |h: std::thread::JoinHandle<std::result::Result<(), HRESULT>>| h.join().unwrap();
        assert_eq!(wait(ask((descriptor, -1, TYMED_HGLOBAL))), Ok(()), "the descriptor");
        assert_eq!(wait(ask((contents, 0, TYMED_ISTREAM))), Err(E_FAIL), "contents before the drop");
        assert_eq!(wait(ask((CF_HDROP.0, -1, TYMED_HGLOBAL))), Err(DV_E_FORMATETC), "the file, not made");
        assert!(!drag.files_refused.load(Ordering::SeqCst), "a refusal before the drop isn't the receiver's");
        drag.dropped.store(true, Ordering::SeqCst);
        assert_eq!(wait(ask((CF_HDROP.0, -1, TYMED_HGLOBAL))), Err(DV_E_FORMATETC), "after the drop");
        assert!(drag.files_refused.load(Ordering::SeqCst));
        // The contents wait until it's made.
        let waiting = ask((contents, 0, TYMED_ISTREAM));
        std::thread::sleep(TICK * 3);
        assert!(!waiting.is_finished(), "answered before it was made");
        assert_eq!(waited.load(Ordering::SeqCst), 1, "waited for once, after the drop (the page says it's being made)");
        *state.lock().unwrap() = Some(Ok(file.clone()));
        assert_eq!(wait(waiting), Ok(()), "its contents once made");
        assert!(drag.handed[0].load(Ordering::SeqCst));
        assert_eq!(wait(ask((CF_HDROP.0, -1, TYMED_HGLOBAL))), Ok(()), "the file once made");
        assert_eq!(wait(ask((contents, 0, TYMED_ISTREAM))), Ok(()), "its contents, made");
        assert_eq!(waited.load(Ordering::SeqCst), 1, "not waited for once made");
        assert!(drag.files_given.load(Ordering::SeqCst));
        // One that won't be made: its contents answered "canceled", why kept.
        *state.lock().unwrap() = Some(Err("blocked".into()));
        let canceled = HRESULT::from_win32(ERROR_CANCELLED.0);
        assert_eq!(wait(ask((contents, 0, TYMED_ISTREAM))), Err(canceled));
        assert_eq!(drag.failure.lock().unwrap().as_deref(), Some("blocked"));
        let _ = std::fs::remove_file(&file);
    }

    /// The app's own window asks on the UI thread and is answered at once,
    /// with what's made so far (none), else it would ignore the drag.
    #[test]
    fn the_apps_own_window_is_answered_at_once() {
        // SAFETY: COM for the test's thread, which plays the UI thread.
        unsafe {
            let _ = OleInitialize(None);
        }
        let item = Test { name: "a.mp4", ..Default::default() };
        let (drag, data) = data_object(vec![Box::new(item)], 0, Arc::new(|_| {})).unwrap();
        drag.dropped.store(true, Ordering::SeqCst);
        // SAFETY: a valid FORMATETC, on the object's own thread.
        let got = unsafe { data.GetData(&format(CF_HDROP.0, -1, TYMED_HGLOBAL)) };
        assert!(got.is_ok());
        assert!(drag.own_drop.load(Ordering::SeqCst), "asked after the drop: dropped back on the window");
        assert!(!drag.files_refused.load(Ordering::SeqCst));
    }

    /// A receiver that passes no FORMATETC is told it's wrong, and the app
    /// stays up.
    #[test]
    fn a_receiver_passing_null_is_refused() {
        // SAFETY: COM for the test's thread, which plays the UI thread.
        unsafe {
            let _ = OleInitialize(None);
        }
        let item = Test { name: "a.mp4", ..Default::default() };
        let (_, data) = data_object(vec![Box::new(item)], 0, Arc::new(|_| {})).unwrap();
        // SAFETY: null on purpose; the object must not read it.
        unsafe {
            assert_eq!(data.GetData(std::ptr::null()).map(|_| ()).map_err(|e| e.code()), Err(E_INVALIDARG));
            assert_eq!(data.QueryGetData(std::ptr::null()), E_INVALIDARG);
            let f = format(CF_HDROP.0, -1, TYMED_HGLOBAL);
            assert_eq!(data.GetCanonicalFormatEtc(&f, std::ptr::null_mut()), E_INVALIDARG);
        }
    }

    /// A program with no description is named by its file, capitalized.
    #[test]
    fn a_program_is_named_by_its_file_when_it_says_nothing() {
        assert_eq!(stem_name(Path::new(r"C:\Programs\Kdenlive\bin\kdenlive.exe")).as_deref(), Some("Kdenlive"));
        assert_eq!(stem_name(Path::new(r"C:\x\LosslessCut.exe")).as_deref(), Some("LosslessCut"));
        assert_eq!(stem_name(Path::new("")), None);
    }

    /// How the drag ends as DoDragDrop returns, by what was seen of it.
    #[test]
    fn a_drag_ends_as_its_receiver_took_it() {
        let named = || Some("Kdenlive".to_string());
        let r = |f: fn(&mut Returned)| {
            let mut r = Returned { taken_drop: true, ..Default::default() };
            f(&mut r);
            returned(r, named)
        };
        // Escape, or let go on nothing: nothing, nobody named.
        let off = r(|r| r.called_off = true);
        assert_eq!((off.ending, off.delivered, off.receiver), (Ending::CalledOff, false, None));
        let nowhere = r(|r| r.taken_drop = false);
        assert_eq!((nowhere.ending, nowhere.delivered), (Ending::Nowhere, false));
        assert_eq!(r(|r| r.own = true).ending, Ending::Back);
        // Given the files themselves after the drop: delivered, and they're its.
        let given = r(|r| r.files_given = true);
        assert!(given.delivered && !given.copied && given.made);
        // Refused while one was still being made, nothing else asked: it takes finished files only.
        let refused = r(|r| r.files_refused = true);
        assert!(!refused.delivered && !refused.made);
        assert_eq!(refused.receiver.as_deref(), Some("Kdenlive"));
        // Let go on another app that refused the drop while one was being
        // made: the same, named. Made by then, or let go on nothing: silent.
        let refusing = |r: &mut Returned| (r.taken_drop, r.over_other, r.unmade) = (false, true, true);
        let refused_drop = r(refusing);
        assert_eq!((refused_drop.ending, refused_drop.delivered), (Ending::Elsewhere, false));
        assert!(!refused_drop.made);
        assert_eq!(refused_drop.receiver.as_deref(), Some("Kdenlive"));
        assert_eq!(r(|r| (r.taken_drop, r.over_other) = (false, true)).ending, Ending::Nowhere, "made");
        assert_eq!(r(|r| (r.taken_drop, r.unmade) = (false, true)).ending, Ending::Nowhere, "not on another app");
        let failed_drop =
            r(|r| (r.taken_drop, r.over_other, r.unmade, r.failure) = (false, true, true, Some("blocked".into())));
        assert_eq!((failed_drop.ending, failed_drop.made), (Ending::Nowhere, true));
        let escaped = r(|r| (r.called_off, r.taken_drop, r.unmade) = (true, false, true));
        assert_eq!((escaped.ending, escaped.made), (Ending::CalledOff, true));
        // Refused, then given (asked again once made), or read as a virtual file: not that.
        assert!(r(|r| (r.files_refused, r.files_given) = (true, true)).made);
        let read = r(|r| (r.files_refused, r.contents_asked) = (true, true));
        assert!(read.delivered && read.copied && read.made);
        // One couldn't be made: why, and nothing delivered.
        let failed = r(|r| (r.contents_asked, r.failure) = (true, Some("blocked".into())));
        assert_eq!((failed.delivered, failed.failure), (false, Some(Failure::Failed("blocked".into()))));
        // Copied (Explorer): delivered once written, unless one failed.
        assert!(copied(true, None).delivered && copied(true, None).copied);
        assert!(!copied(false, None).delivered, "its own Cancel");
        assert_eq!(copied(true, Some("blocked".into())).failure, Some(Failure::Failed("blocked".into())));
    }
}
