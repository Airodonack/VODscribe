//! Windows: what an OLE drag's `IDataObject` offers, and its virtual files
//! read out of it.
//!
//! Call these from your `IDropTarget`, on its thread: [`paths`] and [`items`]
//! (or [`offer`]) in `DragEnter` to decide whether to take the drag, again in
//! `Drop`, then [`fetch`] in `Drop` with the items you take. A drag of
//! neither may be of web links ([`links`]).
//!
//! The paths are CF_HDROP. The items are FileGroupDescriptorW, each item's
//! bytes FileContents at its index, as an IStream or an HGLOBAL. A source
//! that copies asynchronously (IDataObjectAsyncCapability with async mode on,
//! as Explorer's zip folders) gets `StartOperation` in `fetch` and
//! `EndOperation` once every item has ended, with S_OK if all came whole,
//! else E_ABORT.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{E_ABORT, HGLOBAL, S_OK};
use windows::Win32::System::Com::{
    COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize, DATADIR_GET, DVASPECT_CONTENT, FORMATETC,
    IDataObject, IStream, STGMEDIUM, TYMED, TYMED_HGLOBAL, TYMED_ISTREAM,
};
use windows::Win32::System::DataExchange::{GetClipboardFormatNameW, RegisterClipboardFormatW};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::{CF_HDROP, CF_UNICODETEXT, DROPEFFECT_COPY, ReleaseStgMedium};
use windows::Win32::UI::Shell::{
    CFSTR_FILECONTENTS, CFSTR_FILEDESCRIPTORW, DragQueryFileW, HDROP, IDataObjectAsyncCapability,
};
use windows::core::{AgileReference, Interface, PCWSTR, w};

use crate::{Destination, Error, Fetch, Item, Job, Offer, Shared, descriptor, links};

/// How much of an item is read at a time.
const CHUNK: usize = 1 << 20;

/// The paths `data` offers (CF_HDROP), or None if it offers none. Some and
/// empty is a drag that has the format but no files in it yet (a source that
/// makes its files late may answer again at the drop).
pub fn paths(data: &IDataObject) -> Option<Vec<PathBuf>> {
    // SAFETY: the medium is read while held, then released.
    unsafe {
        let mut medium = hglobal(data.GetData(&format(CF_HDROP.0, -1, TYMED_HGLOBAL)).ok()?)?;
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

/// The virtual files `data` offers (FileGroupDescriptorW), folders left out:
/// the files inside a dragged folder come with their names alone.
pub fn items(data: &IDataObject) -> Vec<Item> {
    let descriptor = registered(CFSTR_FILEDESCRIPTORW);
    // SAFETY: the block is read within its size while locked, then released.
    unsafe {
        let Some(mut medium) = data.GetData(&format(descriptor, -1, TYMED_HGLOBAL)).ok().and_then(hglobal) else {
            return vec![];
        };
        let block = medium.u.hGlobal;
        let at = GlobalLock(block) as *const u8;
        let items = match at.is_null() {
            true => vec![],
            false => {
                let items = descriptor::items(std::slice::from_raw_parts(at, GlobalSize(block)));
                let _ = GlobalUnlock(block);
                items
            }
        };
        ReleaseStgMedium(&mut medium);
        items
    }
}

/// The web links (http and https) `data` offers: a browser's link
/// (UniformResourceLocatorW, one), else its text (CF_UNICODETEXT) when
/// that's links only, one a line.
pub fn links(data: &IDataObject) -> Vec<String> {
    let link = text_of(data, registered(w!("UniformResourceLocatorW"))).and_then(|l| links::web_link(&l));
    if let Some(link) = link {
        return vec![link];
    }
    text_of(data, CF_UNICODETEXT.0).map(|t| links::links_in(&t)).unwrap_or_default()
}

/// The UTF-16 text `data` offers as clipboard format `cf`, up to its first
/// NUL, if it does, in a memory block, within `links::MAX_TEXT`.
fn text_of(data: &IDataObject, cf: u16) -> Option<String> {
    // SAFETY: the block is read within its size while locked, then released.
    unsafe {
        let mut medium = hglobal(data.GetData(&format(cf, -1, TYMED_HGLOBAL)).ok()?)?;
        let block = medium.u.hGlobal;
        let at = GlobalLock(block) as *const u16;
        let text = (!at.is_null()).then(|| {
            let text = links::text_of_utf16(std::slice::from_raw_parts(at, GlobalSize(block) / 2));
            let _ = GlobalUnlock(block);
            text
        });
        ReleaseStgMedium(&mut medium);
        text.flatten()
    }
}

/// Both [`paths`] (empty with none) and [`items`].
pub fn offer(data: &IDataObject) -> Offer {
    Offer { paths: paths(data).unwrap_or_default(), items: items(data) }
}

/// Fetches `requests`, items of `data` and where each goes; call it inside
/// your `Drop`, with the `data` it was given. If the source can wait, this
/// returns at once and the items are read on a thread of its own; else they
/// are all read (and ended) before it returns. Keep the [`Fetch`] until it
/// finishes: dropped, it cancels. Every item ends exactly once.
pub fn fetch(data: &IDataObject, requests: Vec<(Item, Destination)>) -> Fetch {
    let (shared, jobs) = Shared::new(requests);
    let contents = registered(CFSTR_FILECONTENTS);
    if jobs.is_empty() {
        return shared.handle(false);
    }
    // SAFETY: plain calls on the drag's data, valid through the drop.
    let copier = data
        .cast::<IDataObjectAsyncCapability>()
        .ok()
        .filter(|a| unsafe { a.GetAsyncMode().is_ok_and(|on| on.as_bool()) && a.StartOperation(None).is_ok() });
    let Some(copier) = copier else {
        read_all(data, contents, &shared, jobs);
        return shared.handle(false);
    };
    // Read here, then the operation begun above ended: when the data can't
    // go to another thread, or there's no thread to go to.
    let here = |jobs: Vec<Job>| {
        let ok = read_all(data, contents, &shared, jobs);
        // SAFETY: the operation begun above, ended.
        let _ = unsafe { copier.EndOperation(if ok { S_OK } else { E_ABORT }, None, DROPEFFECT_COPY.0) };
        shared.handle(false)
    };
    // The data as any thread may use it (it's another app's, through COM).
    let Ok(agile) = AgileReference::new(data) else { return here(jobs) };
    // (Taken back if the thread can't start.)
    let queued = Arc::new(Mutex::new(Some(jobs)));
    let (on_thread, for_thread) = (shared.clone(), queued.clone());
    let spawned = std::thread::Builder::new().name("dragndrop-fetch".into()).spawn(move || {
        let Some(jobs) = for_thread.lock().unwrap().take() else { return };
        // SAFETY: COM for this thread, undone as it ends.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        match agile.resolve() {
            Ok(data) => {
                let ok = read_all(&data, contents, &on_thread, jobs);
                if let Ok(copier) = data.cast::<IDataObjectAsyncCapability>() {
                    // SAFETY: the operation begun on the drop's thread, ended.
                    let _ = unsafe { copier.EndOperation(if ok { S_OK } else { E_ABORT }, None, DROPEFFECT_COPY.0) };
                }
            }
            Err(e) => {
                for job in jobs {
                    on_thread.end(job, Err(Error::Source(format!("it let go of the drag ({e})"))));
                }
            }
        }
        // SAFETY: as begun above.
        unsafe { CoUninitialize() };
    });
    match spawned {
        Ok(_) => shared.handle(true),
        Err(_) => here(queued.lock().unwrap().take().unwrap_or_default()),
    }
}

/// The formats `data` offers, by name ("CF_HDROP", "FileGroupDescriptorW",
/// "#49171" for one without a name) with their media (TYMED bits): for a
/// trace of what a source sends.
pub fn formats(data: &IDataObject) -> Vec<String> {
    let mut names = vec![];
    // SAFETY: each FORMATETC is read as the enumerator fills it; a target
    // device it carries is freed.
    unsafe {
        let Ok(each) = data.EnumFormatEtc(DATADIR_GET.0 as u32) else { return names };
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

/// `medium` if it's the memory block asked for; else released, None (a
/// source that answers with another medium isn't read as one).
fn hglobal(mut medium: STGMEDIUM) -> Option<STGMEDIUM> {
    if medium.tymed == TYMED_HGLOBAL.0 as u32 {
        return Some(medium);
    }
    // SAFETY: the source's medium, released once, unread.
    unsafe { ReleaseStgMedium(&mut medium) };
    None
}

/// The clipboard format registered as `name`.
fn registered(name: PCWSTR) -> u16 {
    // SAFETY: a plain lookup (registered once per session, then the same).
    unsafe { RegisterClipboardFormatW(name) as u16 }
}

fn format(cf: u16, lindex: i32, tymed: TYMED) -> FORMATETC {
    FORMATETC { cfFormat: cf, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex, tymed: tymed.0 as u32 }
}

/// Reads each of `jobs` from `data` (FileContents is clipboard format
/// `contents`), one after another, ending each; true if all came whole.
fn read_all(data: &IDataObject, contents: u16, shared: &Shared, jobs: Vec<Job>) -> bool {
    let mut all = true;
    for mut job in jobs {
        let read = match shared.canceled() {
            true => Err(Error::Canceled),
            false => read_one(data, contents, shared, &mut job),
        };
        all &= read.is_ok();
        shared.end(job, read);
    }
    all
}

/// Reads `job` from `data`: how many bytes.
fn read_one(data: &IDataObject, contents: u16, shared: &Shared, job: &mut Job) -> Result<u64, Error> {
    let index = i32::try_from(job.item.index).map_err(|_| Error::Source("no such file".into()))?;
    let asked = format(contents, index, TYMED(TYMED_ISTREAM.0 | TYMED_HGLOBAL.0));
    // SAFETY: the medium is read while held, then released.
    unsafe {
        let mut medium = data.GetData(&asked).map_err(|e| Error::Source(e.message()))?;
        let read = if medium.tymed == TYMED_ISTREAM.0 as u32 {
            match &*medium.u.pstm {
                Some(stream) => read_stream(stream, shared, job),
                None => Err(Error::Source("it gave no stream".into())),
            }
        } else if medium.tymed == TYMED_HGLOBAL.0 as u32 {
            read_block(medium.u.hGlobal, shared, job)
        } else {
            Err(Error::Source(format!("it gave it as medium {}, not read", medium.tymed)))
        };
        ReleaseStgMedium(&mut medium);
        read
    }
}

fn read_stream(stream: &IStream, shared: &Shared, job: &mut Job) -> Result<u64, Error> {
    let mut buf = vec![0u8; CHUNK];
    let mut total = 0;
    loop {
        let mut got = 0u32;
        // SAFETY: at most the buffer's length is written into it.
        let hr = unsafe { stream.Read(buf.as_mut_ptr().cast(), buf.len() as u32, Some(&mut got)) };
        if hr.is_err() {
            return Err(Error::Source(format!("its stream couldn't be read: {}", hr.message())));
        }
        if got == 0 {
            return Ok(total);
        }
        shared.write(job, &buf[..got as usize])?;
        total += u64::from(got);
    }
}

fn read_block(block: HGLOBAL, shared: &Shared, job: &mut Job) -> Result<u64, Error> {
    // SAFETY: the block is read within its size while locked.
    unsafe {
        let size = GlobalSize(block);
        let at = GlobalLock(block) as *const u8;
        if at.is_null() {
            return Err(Error::Source("its memory couldn't be read".into()));
        }
        let written = std::slice::from_raw_parts(at, size).chunks(CHUNK).try_for_each(|part| shared.write(job, part));
        let _ = GlobalUnlock(block);
        written.map(|()| size as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Shell::{FD_ATTRIBUTES, FD_FILESIZE, FILEDESCRIPTORW};

    /// The hand-read layout is Windows' own.
    #[test]
    fn the_descriptor_record_is_windows_own() {
        assert_eq!(std::mem::size_of::<FILEDESCRIPTORW>(), descriptor::RECORD);
        let mut name = [0u16; 260];
        name.iter_mut().zip("d\\e.mp4".encode_utf16()).for_each(|(to, u)| *to = u);
        let fd = FILEDESCRIPTORW {
            dwFlags: (FD_FILESIZE.0 | FD_ATTRIBUTES.0) as u32,
            dwFileAttributes: 0x80,
            nFileSizeHigh: 1,
            nFileSizeLow: 2,
            cFileName: name,
            ..Default::default()
        };
        let mut block = 1u32.to_le_bytes().to_vec();
        // SAFETY: the struct read as its bytes, within its size.
        block.extend_from_slice(unsafe {
            std::slice::from_raw_parts(&fd as *const FILEDESCRIPTORW as *const u8, descriptor::RECORD)
        });
        assert_eq!(descriptor::items(&block), [Item { name: "e.mp4".into(), size: Some(1 << 32 | 2), index: 0 }]);
    }
}
