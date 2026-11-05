//! The web view's drops on macOS, with file promises taken too. wry's web
//! view class (WryWebView) is the drop target, and it gives the page the
//! paths of file URLs dropped on it (Tauri's drag-drop events). A drag of
//! file promises only (files their app makes or copies out once dropped)
//! arrives there as no paths, and nothing happens.
//!
//! So the class's four dragging methods are hooked, each calling wry's own
//! for every drag but one: a drag from another app that offers no file URLs
//! but promises files the target takes. That one is taken here: told to the
//! target as it goes, and once let go each promise is received through
//! `dragndrop_target::macos::fetch` into what the target gives for it (the
//! source writes the file in its own time; the fetches are kept here until
//! they end). The view is also registered for the promises' types.
//!
//! Hooked, not subclassed: the class is wry's, made when the window is, and
//! its methods are swapped once for ours, which keep wry's to call. Other
//! web views of the class (none today) go to wry's alone. (The view's own
//! class is one the system derives from wry's to observe it, so the methods
//! are looked for up from there.)

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::OnceLock;

use dragndrop_target::macos::{fetch, formats, items, paths};
use dragndrop_target::{Fetch, Item};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Method, ProtocolObject, Sel};
use objc2::{msg_send, sel};
use objc2_app_kit::{NSDragOperation, NSDraggingInfo, NSFilePromiseReceiver, NSView};
use objc2_foundation::{NSMutableArray, NSRect};

use super::{Event, Target};

type Entered = unsafe extern "C-unwind" fn(&AnyObject, Sel, &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation;
type Exited = unsafe extern "C-unwind" fn(&AnyObject, Sel, Option<&ProtocolObject<dyn NSDraggingInfo>>);
type Performed = unsafe extern "C-unwind" fn(&AnyObject, Sel, &ProtocolObject<dyn NSDraggingInfo>) -> Bool;

/// wry's own methods, swapped out for ours.
struct Wry {
    entered: Entered,
    updated: Entered,
    exited: Exited,
    performed: Performed,
}

static WRY: OnceLock<Wry> = OnceLock::new();

thread_local! {
    /// The view taking drops and its target (AppKit's drags are the main
    /// thread's).
    static TAKING: RefCell<Option<Taking>> = const { RefCell::new(None) };
}

/// Takes drops on web view `view` (wry's WKWebView, given by Tauri) for
/// `target`, file promises too; `trace` hears what's going on (the types
/// each drag offers, each file received). Called on the main thread, once.
pub(crate) fn register(view: *mut c_void, target: Box<dyn Target>, trace: fn(&str)) -> Result<(), String> {
    // SAFETY: Tauri's pointer to the window's live web view, on its thread.
    let view: &NSView = unsafe { view.cast::<NSView>().as_ref() }.ok_or("no web view")?;
    hook(view.class())?;
    // (Added to the types WebKit registers, not in their place.)
    let types = NSMutableArray::from_retained_slice(&view.registeredDraggedTypes().to_vec());
    for promised in NSFilePromiseReceiver::readableDraggedTypes().iter() {
        if !types.containsObject(&promised) {
            types.addObject(&promised);
        }
    }
    view.registerForDraggedTypes(&types);
    let view = Retained::from(view);
    TAKING.with_borrow_mut(|t| *t = Some(Taking { view, target, trace, offer: None, fetches: vec![] }));
    Ok(())
}

/// Swaps the dragging methods of `class` (the view's: wry's class, or the
/// one the system makes from it to observe the view) for ours, once, where
/// they're implemented: in wry's class, never WebKit's, which every web view
/// shares.
fn hook(class: &AnyClass) -> Result<(), String> {
    if WRY.get().is_some() {
        return Ok(());
    }
    let webkit = AnyClass::get(c"WKWebView").ok_or("no WKWebView")?;
    let own = |sel: Sel| {
        let mut at = class;
        loop {
            if std::ptr::eq(at, webkit) {
                return Err(format!("{} has no {sel} of its own", class.name().to_string_lossy()));
            }
            let method = at.instance_method(sel).ok_or(format!("{} has no {sel}", at.name().to_string_lossy()))?;
            match at.superclass().and_then(|s| s.instance_method(sel)) {
                Some(inherited) if std::ptr::eq(inherited, method) => at = at.superclass().unwrap_or(webkit),
                _ => return Ok(method),
            }
        }
    };
    let sels = [sel!(draggingEntered:), sel!(draggingUpdated:), sel!(draggingExited:), sel!(performDragOperation:)];
    let [entered, updated, exited, performed] = sels.map(own);
    let (entered, updated, exited, performed) = (entered?, updated?, exited?, performed?);
    // SAFETY: each method swapped for one of its own signature (as wry
    // declares them), which calls the one it replaced.
    let wry = unsafe {
        Wry {
            entered: swap(entered, dragging_entered as Entered),
            updated: swap(updated, dragging_updated as Entered),
            exited: swap(exited, dragging_exited as Exited),
            performed: swap(performed, perform_drag_operation as Performed),
        }
    };
    let _ = WRY.set(wry);
    Ok(())
}

/// Puts `ours` in `method`'s place, giving the function it replaced (one of
/// wry's, alive as long as the program).
///
/// # Safety
///
/// `F` is a function pointer of the method's own signature.
unsafe fn swap<F: Copy>(method: &Method, ours: F) -> F {
    assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<Imp>());
    // SAFETY: as the caller says; both are function pointers.
    unsafe { std::mem::transmute_copy(&method.set_implementation(std::mem::transmute_copy(&ours))) }
}

fn wry() -> &'static Wry {
    WRY.get().expect("hooked before any drag")
}

/// The view taking drops, and the drag of promises over it now.
struct Taking {
    view: Retained<NSView>,
    target: Box<dyn Target>,
    trace: fn(&str),
    /// The promises of the drag over the view that the target takes, as
    /// taken when it came; None when the drag is wry's.
    offer: Option<Vec<Item>>,
    /// The promises let go on the view being received, kept until they end
    /// (dropped, they'd stop).
    fetches: Vec<Fetch>,
}

impl Taking {
    /// Where the drag is, as wry says it (the view's points, from its top left).
    fn at(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> (f64, f64) {
        let at = info.draggingLocation();
        // SAFETY: a plain getter.
        let frame: NSRect = unsafe { msg_send![&*self.view, frame] };
        (at.x, frame.size.height - at.y)
    }

    /// The promises of `info` the target takes, if it's one to take here:
    /// from another app, with no file URLs.
    fn promises(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> Option<Vec<Item>> {
        (self.trace)(&format!("a drag over the window offers {}", formats(info).join(", ")));
        if info.draggingSource().is_some() || paths(info).is_some_and(|p| !p.is_empty()) {
            return None;
        }
        let taken: Vec<Item> = items(info).into_iter().filter(|f| self.target.takes(f.name())).collect();
        if taken.is_empty() { None } else { Some(taken) }
    }

    /// Receives promises `files` of `info`, let go on the view.
    fn receive(&mut self, info: &ProtocolObject<dyn NSDraggingInfo>, files: Vec<Item>) {
        let requests: Vec<_> = files.into_iter().filter_map(|f| self.target.incoming(&f).map(|to| (f, to))).collect();
        if requests.is_empty() {
            return;
        }
        let receiving = fetch(info, requests);
        self.fetches.retain(|f| !f.is_finished());
        self.fetches.push(receiving);
    }
}

/// Runs `f` on the view taking drops if `this` is it.
fn on<R>(this: &AnyObject, f: impl FnOnce(&mut Taking) -> Option<R>) -> Option<R> {
    TAKING.with_borrow_mut(|t| {
        let t = t.as_mut().filter(|t| std::ptr::eq(this, (&*t.view as &NSView).as_ref()))?;
        f(t)
    })
}

unsafe extern "C-unwind" fn dragging_entered(
    this: &AnyObject,
    sel: Sel,
    info: &ProtocolObject<dyn NSDraggingInfo>,
) -> NSDragOperation {
    let taken = on(this, |t| {
        t.offer = t.promises(info);
        let files = t.offer.as_ref()?;
        let names = files.iter().map(|f| f.name().to_string()).collect();
        t.target.heard(Event::Enter { paths: vec![], names, at: t.at(info) });
        Some(NSDragOperation::Copy)
    });
    // SAFETY: wry's own, as AppKit would have called it.
    taken.unwrap_or_else(|| unsafe { (wry().entered)(this, sel, info) })
}

unsafe extern "C-unwind" fn dragging_updated(
    this: &AnyObject,
    sel: Sel,
    info: &ProtocolObject<dyn NSDraggingInfo>,
) -> NSDragOperation {
    let taken = on(this, |t| {
        t.offer.as_ref()?;
        t.target.heard(Event::Over { at: t.at(info) });
        Some(NSDragOperation::Copy)
    });
    // SAFETY: as in dragging_entered.
    taken.unwrap_or_else(|| unsafe { (wry().updated)(this, sel, info) })
}

unsafe extern "C-unwind" fn dragging_exited(
    this: &AnyObject,
    sel: Sel,
    info: Option<&ProtocolObject<dyn NSDraggingInfo>>,
) {
    let taken = on(this, |t| {
        t.offer.take()?;
        t.target.heard(Event::Leave);
        Some(())
    });
    if taken.is_none() {
        // SAFETY: as in dragging_entered.
        unsafe { (wry().exited)(this, sel, info) }
    }
}

unsafe extern "C-unwind" fn perform_drag_operation(
    this: &AnyObject,
    sel: Sel,
    info: &ProtocolObject<dyn NSDraggingInfo>,
) -> Bool {
    let taken = on(this, |t| {
        let files = t.offer.take()?;
        (t.trace)(&format!("let go with {} promised file(s) to receive", files.len()));
        t.target.heard(Event::Drop { paths: vec![], at: t.at(info) });
        t.receive(info, files);
        Some(Bool::YES)
    });
    // SAFETY: as in dragging_entered.
    taken.unwrap_or_else(|| unsafe { (wry().performed)(this, sel, info) })
}
