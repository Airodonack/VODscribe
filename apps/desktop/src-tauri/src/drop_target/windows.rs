//! The window's drop target on Windows (OLE). A drag is taken one of two
//! ways, as it comes over the window:
//!
//! - as paths (CF_HDROP), told to the target as they are: files dropped from
//!   Explorer, and the app's own drags out, coming back over the window
//!   (`drag_source` answers the window at once, with the files made so far);
//! - else as virtual files (FileGroupDescriptorW + FileContents), when there
//!   are some the target takes: a video inside a zip folder in Explorer.
//!   Once let go, each is read from the app it comes from into what the
//!   target gives for it (`dragndrop_target::windows::fetch`: on a thread of
//!   its own if that app copies asynchronously, as Explorer does, else before
//!   the drop returns). The fetches are kept here until they end.
//!
//! A drag offering both is taken as paths. Anything else is refused.
//!
//! WebView2 would take drops itself (its AllowExternalDrop, which the app
//! turns off) and wry would put its own target over it (Tauri's drag-drop
//! handler, which the app doesn't use on Windows); this is registered on the
//! window and on each window in it, as wry's was.

use std::cell::RefCell;
use std::path::PathBuf;

use dragndrop_target::windows::{fetch, formats, items, paths};
use dragndrop_target::{Fetch, Item};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, GetClassNameW};
use windows::core::*;

use super::{Event, Target};

/// Takes drops on window `hwnd` (its raw handle) and the windows in it for
/// `target`; `trace` hears what's going on (the formats each drag offers,
/// each file received). Called on the window's thread, which OLE is set up
/// on (Tauri's). Gives the class names of the windows it's registered on.
pub(crate) fn register(hwnd: isize, target: Box<dyn Target>, trace: fn(&str)) -> Result<Vec<String>> {
    let window = HWND(hwnd as *mut _);
    let drop_target: IDropTarget =
        DropTarget { window, target, trace, offer: RefCell::new(Offer::Nothing), fetches: RefCell::default() }.into();
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

/// How the drag over the window is taken.
#[derive(Clone, Debug, PartialEq)]
enum Offer {
    Paths(Vec<PathBuf>),
    Virtual(Vec<Item>),
    Nothing,
}

#[implement(IDropTarget)]
struct DropTarget {
    window: HWND,
    target: Box<dyn Target>,
    trace: fn(&str),
    /// The drag over the window now, as taken when it came.
    offer: RefCell<Offer>,
    /// The virtual files let go on the window being read, kept until they
    /// end (dropped, they'd stop).
    fetches: RefCell<Vec<Fetch>>,
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
        if let Some(paths) = paths(data) {
            return Offer::Paths(paths);
        }
        let taken: Vec<Item> = items(data).into_iter().filter(|f| self.target.takes(f.name())).collect();
        if taken.is_empty() { Offer::Nothing } else { Offer::Virtual(taken) }
    }

    /// Receives the virtual files `files` of `data`, let go on the window.
    fn receive(&self, data: &IDataObject, files: Vec<Item>) {
        let requests: Vec<_> = files.into_iter().filter_map(|f| self.target.incoming(&f).map(|to| (f, to))).collect();
        if requests.is_empty() {
            return;
        }
        let reading = fetch(data, requests);
        (self.trace)(match reading.in_background() {
            true => "the source waits (StartOperation): read on a thread of its own",
            false => "read before the drop returned",
        });
        let mut fetches = self.fetches.borrow_mut();
        fetches.retain(|f| !f.is_finished());
        fetches.push(reading);
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
                let names = files.iter().map(|f| f.name().to_string()).collect();
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
            Offer::Paths(_) => self.target.heard(Event::Drop { paths: paths(data).unwrap_or_default(), at }),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_taken_is_answered_copy() {
        assert_eq!(answer(&Offer::Nothing), DROPEFFECT_NONE);
        assert_eq!(answer(&Offer::Paths(vec![])), DROPEFFECT_COPY, "the app's own, nothing made yet");
        assert_eq!(answer(&Offer::Virtual(vec![])), DROPEFFECT_COPY);
    }
}
