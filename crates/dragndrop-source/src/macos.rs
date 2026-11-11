//! macOS (AppKit): the data of a drag of files out of a window, made or not.
//!
//! [`Drag::new`] on the main thread gives the `NSDraggingItem`s; you begin
//! the session with them (`beginDraggingSessionWithItems:event:source:`),
//! from a press that's still down ([`button_held`]). Your
//! `NSDraggingSource` offers [`Drag::operations`] and calls [`Drag::ended`]
//! from `draggingSession:endedAtPoint:operation:`. Each file is two dragging
//! items:
//!
//! - a file promise (`NSFilePromiseProvider`): Finder, and any receiver that
//!   takes promises, names a place, and the file is written there once it's
//!   made, on an operation queue of its own (off the main thread, so the
//!   wait blocks nothing). Meanwhile an empty file stands there with a
//!   progress published on it: Finder shows it being copied, with a cancel
//!   button. Only that empty file is ever replaced (see `placeholder`).
//! - the file's address (public.file-url on a pasteboard item), for
//!   receivers that take real files only. It's answered at once, when
//!   asked: the path if the file is made, else nothing. Finder asks for it
//!   as the drag hovers over it, and a wait there would freeze the app and
//!   lose the drop.
//!
//! Never the address on the promise itself: receivers then get no address,
//! or refuse the drag while the file isn't made. AppKit counts the items, so
//! one file shows a "2" badge.
//!
//! Files all made as the drag begins go as they are, one file URL each, as
//! any app takes them: apps that read the older list of file names (VLC)
//! refused a drag with a promise in it.
//!
//! You hear [`Event::LetGo`] as it's let go on another app, then
//! [`Event::Ended`] once the session is over (and a moment's grace, as a
//! promise is asked for just after) and every promise asked for is settled.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApp, NSDragOperation, NSDraggingItem, NSEvent, NSFilePromiseProvider, NSFilePromiseProviderDelegate, NSImage,
    NSPasteboard, NSPasteboardItem, NSPasteboardItemDataProvider, NSPasteboardType, NSPasteboardTypeFileURL,
    NSPasteboardWriting, NSRunningApplication, NSWindow,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSError, NSMutableArray, NSNumber, NSOperationQueue, NSPoint, NSProgress,
    NSProgressFileOperationKindCopying, NSProgressKindFile, NSRect, NSString, NSURL,
};

use crate::placeholder::Placeholder;
use crate::promises::{Counts, Promises};
use crate::{Ending, Event, Failure, Item, Outcome, State, trace};

/// How often a promise waiting for its file looks at it.
const TICK: Duration = Duration::from_millis(100);

/// How long after the session ends you're told, if no promise is being
/// written: Finder asks for its promises within a few ms of the end.
const GRACE: Duration = Duration::from_millis(300);

/// AppKit objects the drag owns and never messages again: the promises'
/// delegate (they hold it weakly) and the addresses' data providers.
struct Kept(#[allow(dead_code)] Vec<Retained<AnyObject>>);
// SAFETY: only held, then released (from whichever thread drops the last
// `Hand`); NSObject's release is thread-safe and these objects' ivars are a
// queue and weak references.
unsafe impl Send for Kept {}
unsafe impl Sync for Kept {}

/// One drag's files and what became of them, shared by the AppKit objects
/// (on the main thread and on the promises' queue).
struct Hand {
    items: Vec<Box<dyn Item>>,
    /// Every file was made as the drag began: offered as files, no promise.
    ready: bool,
    on: Box<dyn Fn(Event) + Send + Sync>,
    kept: Mutex<Option<Kept>>,
    /// Promises asked for, being written, and written.
    promises: Promises,
    /// Addresses asked for and given (the file was made).
    addressed: AtomicUsize,
    /// An address asked for while its file wasn't made (and none given):
    /// Finder asks on hover (and then takes the promise); a receiver of
    /// real files only, at the drop.
    unanswered: AtomicBool,
    cancelled: AtomicBool,
    failed: Mutex<Option<String>>,
    /// The session is over: where it ended, the app there, and whether
    /// every file was made by then.
    ended: Mutex<Option<(Ending, Option<String>, bool)>>,
    /// Let go on another app's window, which refused the drop, while a
    /// file wasn't made.
    refused: AtomicBool,
    told: AtomicBool,
}

impl Hand {
    fn made(&self) -> bool {
        self.items.iter().all(|item| matches!(item.state(), State::Ready(_)))
    }

    /// Tells `on` how it went, once: when the session is over and every
    /// promise asked for is settled.
    fn settle(&self) {
        let Some((ending, receiver, made)) = self.ended.lock().unwrap().clone() else { return };
        let Some(Counts { asked, written, .. }) = self.promises.settled() else { return };
        if self.told.swap(true, Ordering::SeqCst) {
            return;
        }
        let unanswered = self.unanswered.load(Ordering::SeqCst);
        let failure = match self.failed.lock().unwrap().clone() {
            Some(why) => Some(Failure::Failed(why)),
            None if self.cancelled.load(Ordering::SeqCst) => Some(Failure::Cancelled),
            None => None,
        };
        // A receiver of promises took those (and asked for the addresses only
        // on its way); else one of addresses took them, if all were made.
        // Files offered as they are: taken if it was let go on an app that
        // accepted the drop.
        let given = if self.ready {
            ending == Ending::Elsewhere
        } else if asked > 0 {
            failure.is_none() && written == asked
        } else {
            self.addressed.load(Ordering::SeqCst) > 0 && !unanswered
        };
        let ended = Outcome {
            ending,
            delivered: given,
            copied: asked > 0,
            failure,
            made: made || !(unanswered || self.refused.load(Ordering::SeqCst)),
            receiver: receiver.filter(|_| ending == Ending::Elsewhere),
        };
        trace(&format!(
            "over: promises asked {asked}, addresses given {}, unanswered {unanswered}: {ended:?}",
            self.addressed.load(Ordering::SeqCst)
        ));
        (self.on)(Event::Ended(ended));
    }

    /// Writes file `index` to `url` once it's made, with an empty file there
    /// meanwhile and a progress on it (Finder's progress and cancel). Why
    /// not, if it wasn't: None if the receiver called it off. A file already
    /// at `url`, or put there during the wait, is left as it is and the
    /// write refused.
    fn write(&self, index: usize, url: &NSURL) -> Result<(), Option<String>> {
        let item = &self.items[index];
        let to = url.path().map(|p| PathBuf::from(p.to_string())).ok_or(Some("no place to write it".to_string()))?;
        // A new file, never one already there. Only this file is the drag's
        // own: one put at its name during the wait is left alone.
        let placeholder = Placeholder::create(&to).map_err(not_written)?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let progress = NSProgress::progressWithTotalUnitCount(100);
        progress.setKind(Some(unsafe { NSProgressKindFile }));
        progress.setFileOperationKind(Some(unsafe { NSProgressFileOperationKindCopying }));
        progress.setFileURL(Some(url));
        progress.setCancellable(true);
        let flag = cancelled.clone();
        let handler = RcBlock::new(move || flag.store(true, Ordering::SeqCst));
        // SAFETY: the block is 'static and Send (an atomic flag), as a
        // handler AppKit may call from any thread must be.
        unsafe { progress.setCancellationHandler(Some(&handler)) };
        progress.publish();
        if matches!(item.state(), State::Making(_)) {
            trace(&format!("{}: waited for, still being made", item.name()));
            item.waited_for();
        }
        let result = loop {
            if cancelled.load(Ordering::SeqCst) {
                trace(&format!("{}: called off where it was dropped", item.name()));
                self.cancelled.store(true, Ordering::SeqCst);
                item.cancelled();
                break Err(None);
            }
            match item.state() {
                State::Making(along) => {
                    if let Some(along) = along.filter(|a| a.is_finite()) {
                        progress.setCompletedUnitCount((along * 100.0).clamp(0.0, 99.0) as i64);
                    }
                    std::thread::sleep(TICK);
                }
                // The placeholder out of the way first: a copy to a new
                // name is a clone on APFS, onto one it's written out.
                State::Ready(from) => {
                    break placeholder.remove().and_then(|()| item.write(&from, &to)).map_err(not_written);
                }
                State::Failed(why) => break Err(Some(why)),
            }
        };
        progress.setCompletedUnitCount(100);
        progress.unpublish();
        if result.is_err() {
            let _ = placeholder.remove();
        }
        result
    }
}

/// Why a promised file couldn't be put where it was dropped.
fn not_written(e: std::io::Error) -> Option<String> {
    Some(match e.kind() {
        std::io::ErrorKind::AlreadyExists => "a file of that name is already there".to_string(),
        _ => format!("couldn't write the file there: {e}"),
    })
}

/// Which file of a drag an object stands for.
struct Of {
    index: usize,
    hand: Arc<Hand>,
}

define_class!(
    // A file's promise, knowing which file.
    #[unsafe(super(NSFilePromiseProvider, NSObject))]
    #[name = "DragndropSourcePromise"]
    #[ivars = Of]
    struct Promise;
);

impl Promise {
    fn new(of: Of, delegate: &Delegate, file_type: &NSString) -> Retained<Self> {
        let this = Self::alloc().set_ivars(of);
        unsafe {
            msg_send![super(this), initWithFileType: file_type, delegate: ProtocolObject::<dyn NSFilePromiseProviderDelegate>::from_ref(delegate)]
        }
    }
}

/// Which file of a drag an address stands for (weakly: the drag owns it).
struct AddressOf {
    index: usize,
    hand: Weak<Hand>,
}

define_class!(
    // A file's address, given when a receiver asks: at once.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "DragndropSourceAddress"]
    #[ivars = AddressOf]
    struct Address;

    unsafe impl NSObjectProtocol for Address {}

    unsafe impl NSPasteboardItemDataProvider for Address {
        #[unsafe(method(pasteboard:item:provideDataForType:))]
        fn provide(&self, _pasteboard: Option<&NSPasteboard>, item: &NSPasteboardItem, kind: &NSPasteboardType) {
            let AddressOf { index, hand } = self.ivars();
            let Some(hand) = hand.upgrade() else { return };
            if kind != unsafe { NSPasteboardTypeFileURL } {
                return;
            }
            let name = hand.items[*index].name();
            match hand.items[*index].state() {
                State::Ready(path) => {
                    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                    if let Some(text) = url.absoluteString() {
                        item.setString_forType(&text, kind);
                        hand.addressed.fetch_add(1, Ordering::SeqCst);
                    }
                    trace(&format!("{name}: its address asked for, given"));
                }
                _ => {
                    trace(&format!("{name}: its address asked for, none given: not made"));
                    hand.unanswered.store(true, Ordering::SeqCst);
                }
            }
        }
    }
);

struct Queue {
    queue: Retained<NSOperationQueue>,
}

define_class!(
    // Writes each promised file where the receiver says, once it's made, on
    // our own queue: AppKit asks for several at once, in any order.
    #[unsafe(super(NSObject))]
    #[name = "DragndropSourcePromises"]
    #[ivars = Queue]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSFilePromiseProviderDelegate for Delegate {
        #[unsafe(method_id(filePromiseProvider:fileNameForType:))]
        fn file_name(&self, provider: &NSFilePromiseProvider, _kind: &NSString) -> Retained<NSString> {
            let Of { index, hand } = of(provider);
            NSString::from_str(&hand.items[*index].name())
        }

        #[unsafe(method(filePromiseProvider:writePromiseToURL:completionHandler:))]
        fn write(&self, provider: &NSFilePromiseProvider, url: &NSURL, done: &DynBlock<dyn Fn(*mut NSError)>) {
            let Of { index, hand } = of(provider);
            // Asked for and being written in one step (`Promises`), so no
            // settle sees it asked for and not being written.
            hand.promises.asked();
            let name = hand.items[*index].name();
            trace(&format!(
                "{name}: its promise asked for, at {}",
                url.path().map(|p| p.to_string()).unwrap_or_default()
            ));
            let written = match hand.write(*index, url) {
                Ok(()) => {
                    trace(&format!("{name}: written"));
                    done.call((std::ptr::null_mut(),));
                    true
                }
                Err(why) => {
                    trace(&format!("{name}: not written: {}", why.as_deref().unwrap_or("called off")));
                    if let Some(why) = why {
                        hand.failed.lock().unwrap().get_or_insert(why);
                    }
                    let error = NSError::new(1, &NSString::from_str("dragndrop-source"));
                    done.call((Retained::as_ptr(&error) as *mut NSError,));
                    false
                }
            };
            hand.promises.done(written);
            hand.settle();
        }

        #[unsafe(method_id(operationQueueForFilePromiseProvider:))]
        fn queue(&self, _provider: &NSFilePromiseProvider) -> Retained<NSOperationQueue> {
            self.ivars().queue.clone()
        }
    }
);

/// The drag's file behind `provider` (always one of ours).
fn of(provider: &NSFilePromiseProvider) -> &Of {
    // SAFETY: the delegate is only ever given Promises (`Drag::new`).
    unsafe { &*(provider as *const NSFilePromiseProvider as *const Promise) }.ivars()
}

/// Whose window is at `at` (screen points): the app's own, or another
/// app's, by its name if it has one, and whether it's an ordinary window
/// (not the desktop's, the menu bar's or the Dock's).
enum Owner {
    Us,
    App { name: Option<String>, window: bool },
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *mut NSArray<NSDictionary<NSString, AnyObject>>;
}

/// kCGWindowListOptionIncludingWindow: the one window named.
const ONE_WINDOW: u32 = 1 << 3;

fn window_owner(at: NSPoint, mtm: MainThreadMarker) -> Option<Owner> {
    let number = NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(at, 0, mtm);
    if number <= 0 {
        return None;
    }
    if NSApp(mtm).windows().iter().any(|w| w.windowNumber() == number) {
        return Some(Owner::Us);
    }
    // SAFETY: a +1 CFArray of CFDictionaries (toll-free NSArray of NSDictionary), or null.
    let info = unsafe { Retained::from_raw(CGWindowListCopyWindowInfo(ONE_WINDOW, number as u32)) }?;
    let window = info.firstObject()?;
    let pid = window.objectForKey(&NSString::from_str("kCGWindowOwnerPID"))?;
    let pid = pid.downcast_ref::<NSNumber>()?.intValue();
    if pid == std::process::id() as i32 {
        return Some(Owner::Us);
    }
    let name = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        .and_then(|app| app.localizedName())
        .or_else(|| window.objectForKey(&NSString::from_str("kCGWindowOwnerName"))?.downcast::<NSString>().ok())
        .map(|name| name.to_string());
    // kCGNormalWindowLevel: apps' windows; the desktop's are below, the
    // menu bar's and the Dock's above.
    let layer = window.objectForKey(&NSString::from_str("kCGWindowLayer"));
    let ordinary = layer.and_then(|l| l.downcast_ref::<NSNumber>().map(|l| l.intValue() == 0)).unwrap_or(false);
    Some(Owner::App { name, window: ordinary })
}

/// Whether the left mouse button is down now. A drag begun with no button
/// held never ends: the release may have come in the moment between the
/// press and the drag's start. See [`crate::ReleasedEarly`].
pub fn button_held() -> bool {
    NSEvent::pressedMouseButtons() & 1 != 0
}

/// One drag's files, as AppKit's drag carries them. Keep it in your
/// `NSDraggingSource` until the session ends.
pub struct Drag {
    hand: Arc<Hand>,
    items: Retained<NSMutableArray<NSDraggingItem>>,
}

impl Drag {
    /// What your `NSDraggingSource` offers
    /// (`draggingSession:sourceOperationMaskForDraggingContext:`): Copy,
    /// and Generic for apps that ask for that (VLC: offered Copy alone, it
    /// refused every drop).
    pub fn operations() -> NSDragOperation {
        NSDragOperation::Copy | NSDragOperation::Generic
    }

    /// A drag of `items`, each shown as `picture`, centred on `at` (in the
    /// window's coordinates, where the press is) and stacked a little apart.
    /// `on` hears how it goes: [`Event::LetGo`] on the main thread,
    /// [`Event::Ended`] from another.
    pub fn new(
        items: Vec<Box<dyn Item>>,
        picture: &NSImage,
        at: NSPoint,
        on: impl Fn(Event) + Send + Sync + 'static,
        mtm: MainThreadMarker,
    ) -> Drag {
        let size = picture.size();
        let delegate: Retained<Delegate> =
            unsafe { msg_send![super(Delegate::alloc().set_ivars(Queue { queue: NSOperationQueue::new() })), init] };
        // Where each is as the drag begins: made already, all of them, they
        // go as the files themselves.
        let states: Vec<State> = items.iter().map(|item| item.state()).collect();
        let ready = states.iter().all(|state| matches!(state, State::Ready(_)));
        let hand = Arc::new(Hand {
            items,
            ready,
            on: Box::new(on),
            kept: Mutex::new(None),
            promises: Promises::default(),
            addressed: AtomicUsize::new(0),
            unanswered: AtomicBool::new(false),
            refused: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            failed: Mutex::new(None),
            ended: Mutex::new(None),
            told: AtomicBool::new(false),
        });
        let mut kept: Vec<Retained<AnyObject>> = vec![Retained::into_super(Retained::into_super(delegate.clone()))];
        let file_url = NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]);
        let dragged: Retained<NSMutableArray<NSDraggingItem>> = NSMutableArray::new();
        for (index, state) in states.into_iter().enumerate() {
            let frame = NSRect::new(
                NSPoint::new(at.x - size.width / 2. + 8. * index as f64, at.y - size.height / 2. - 8. * index as f64),
                size,
            );
            // Made already: the file itself, as any app takes one (beside a
            // promise, apps that read the older list of file names refused it).
            if let (true, State::Ready(path)) = (ready, state) {
                let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                let file = NSDraggingItem::initWithPasteboardWriter(
                    NSDraggingItem::alloc(),
                    ProtocolObject::<dyn NSPasteboardWriting>::from_ref(&*url),
                );
                unsafe { file.setDraggingFrame_contents(frame, Some(picture)) };
                dragged.addObject(&file);
                continue;
            }
            let file_type = NSString::from_str(&hand.items[index].file_type());
            let promise = Promise::new(Of { index, hand: hand.clone() }, &delegate, &file_type);
            let address = NSPasteboardItem::new();
            let provider: Retained<Address> = unsafe {
                msg_send![super(Address::alloc(mtm).set_ivars(AddressOf { index, hand: Arc::downgrade(&hand) })), init]
            };
            address.setDataProvider_forTypes(ProtocolObject::from_ref(&*provider), &file_url);
            kept.push(Retained::into_super(Retained::into_super(provider)));
            // The picture on the promise only: the address rides along unseen.
            let promise = NSDraggingItem::initWithPasteboardWriter(
                NSDraggingItem::alloc(),
                ProtocolObject::<dyn NSPasteboardWriting>::from_ref::<NSFilePromiseProvider>(&promise),
            );
            unsafe { promise.setDraggingFrame_contents(frame, Some(picture)) };
            dragged.addObject(&promise);
            let address = NSDraggingItem::initWithPasteboardWriter(
                NSDraggingItem::alloc(),
                ProtocolObject::<dyn NSPasteboardWriting>::from_ref(&*address),
            );
            unsafe { address.setDraggingFrame_contents(frame, None) };
            dragged.addObject(&address);
        }
        *hand.kept.lock().unwrap() = Some(Kept(kept));
        trace(&format!("{} files, {} items", hand.items.len(), dragged.count()));
        Drag { hand, items: dragged }
    }

    /// The dragging items, for `beginDraggingSessionWithItems:event:source:`.
    pub fn items(&self) -> &NSArray<NSDraggingItem> {
        &self.items
    }

    /// The session ended at `at` (screen points) with `operation`: call it
    /// from your source's `draggingSession:endedAtPoint:operation:`. Let go
    /// on another app, `on` hears [`Event::LetGo`] now; [`Event::Ended`]
    /// follows once every promise asked for is settled.
    pub fn ended(&self, at: NSPoint, operation: NSDragOperation, mtm: MainThreadMarker) {
        let hand = self.hand.clone();
        let made = hand.made();
        let (ending, receiver) = match window_owner(at, mtm) {
            // AppKit says the same (no operation) for a drop the app under
            // the pointer refused, for Escape and for a drop on nothing. Over
            // another app's window with a file not made yet, it's taken for a
            // refusal: that app may take the file once it's made (VLC,
            // CapCut's timeline), so it's named. Escape pressed over such a
            // window gives the same, harmlessly. Over your own window, the
            // desktop, the menu bar or the Dock: Escape or a miss.
            Some(Owner::App { name, window: true }) if operation == NSDragOperation::None && !made => {
                trace("no operation over another app's window: refused");
                hand.refused.store(true, Ordering::SeqCst);
                (Ending::Elsewhere, name)
            }
            _ if operation == NSDragOperation::None => (Ending::Nowhere, None),
            Some(Owner::Us) => (Ending::Back, None),
            Some(Owner::App { name, .. }) => (Ending::Elsewhere, name),
            None => (Ending::Elsewhere, None),
        };
        trace(&format!("session over: operation {}, {ending:?}, on {receiver:?}, made {made}", operation.0));
        *hand.ended.lock().unwrap() = Some((ending, receiver, made));
        if ending == Ending::Elsewhere {
            (hand.on)(Event::LetGo);
        }
        std::thread::spawn(move || {
            std::thread::sleep(GRACE);
            hand.settle();
        });
    }
}
