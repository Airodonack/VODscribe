//! AppKit's drag session of files out of the window, with
//! `dragndrop_source::macos`'s dragging items, begun from the web view's
//! press. Its source gives that press its release back once it's over.

use anyhow::{Context, Result};
use dragndrop_source::macos::{Drag, button_held};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSApp, NSDragOperation, NSDraggingContext, NSDraggingSession, NSDraggingSource, NSEvent, NSEventModifierFlags,
    NSEventType, NSImage, NSView,
};
use objc2_foundation::{NSData, NSPoint, NSProcessInfo};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{Event, Item, ReleasedEarly};

/// The drag's files, and the view that was pressed (where the drag began, in
/// its window's coordinates), which the drag took the release from.
struct Pressed {
    drag: Drag,
    view: Retained<NSView>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FileDragSource"]
    #[ivars = Pressed]
    struct Source;

    unsafe impl NSObjectProtocol for Source {}

    unsafe impl NSDraggingSource for Source {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn mask(&self, _session: &NSDraggingSession, _context: NSDraggingContext) -> NSDragOperation {
            Drag::operations()
        }

        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn ended(&self, _session: &NSDraggingSession, at: NSPoint, operation: NSDragOperation) {
            release(&self.ivars().view);
            self.ivars().drag.ended(at, operation, self.mtm());
        }
    }
);

/// The release the drag took, given to the view that was pressed: it never
/// sees one, and takes the next press for the drag going on (a click after a
/// drag out was lost; so is Linux's, linux::release_button).
fn release(view: &NSView) {
    let Some(window) = view.window() else { return };
    let at = window.mouseLocationOutsideOfEventStream();
    let up = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        NSEventType::LeftMouseUp,
        at,
        NSEventModifierFlags::empty(),
        NSProcessInfo::processInfo().systemUptime(),
        window.windowNumber(),
        None,
        0,
        1,
        0.0,
    );
    if let Some(up) = up {
        super::trace(&format!("released for {}", view.class().name().to_string_lossy()));
        view.mouseUp(&up);
    }
}

/// The web view in `view` (a WKWebView), which took the press: the window's
/// views wrap it (Tauri's in a parent of wry's).
fn web_view(view: &NSView) -> Option<Retained<NSView>> {
    let class = objc2::runtime::AnyClass::get(c"WKWebView")?;
    if view.isKindOfClass(class) {
        return Some(view.retain());
    }
    view.subviews().iter().find_map(|sub| web_view(&sub))
}

/// Begins AppKit's drag of `items` from `window`, showing `image` (a PNG or
/// another image AppKit reads), while its mouse button is down. On the main
/// thread. `on` hears how it goes.
pub(crate) fn start(
    window: &tauri::WebviewWindow,
    items: Vec<Box<dyn Item>>,
    image: &[u8],
    on: impl Fn(Event) + Send + Sync + 'static,
) -> Result<()> {
    // The window itself (not its web view) takes file drops in tao, which
    // unwraps the dropped files' names: a file not made yet has none, and the
    // app panicked as the drag crossed the window's edge (tao 0.37.1's
    // draggingEntered). Tauri hears drops from the web view, which stays
    // registered for them.
    let ns_window = window.ns_window()? as *const objc2_app_kit::NSWindow;
    // SAFETY: Tauri's window, alive through this call on the main thread.
    unsafe { (*ns_window).unregisterDraggedTypes() };
    super::traced();
    let mtm = MainThreadMarker::new().context("not on the main thread")?;
    let RawWindowHandle::AppKit(handle) = window.window_handle()?.as_raw() else {
        anyhow::bail!("not an AppKit window")
    };
    // SAFETY: the handle's view is alive while its window is, through this call.
    let view = unsafe { &*(handle.ns_view.as_ptr() as *const NSView) };
    let ns_window = view.window().context("no window")?;
    let content = ns_window.contentView().context("no view")?;
    // A drag begun with no button held never ends: the release may have come
    // in the moment between the page asking and this.
    if !button_held() {
        return Err(ReleasedEarly.into());
    }
    let at = ns_window.mouseLocationOutsideOfEventStream();
    let picture = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(image)).context("no picture")?;
    let drag = Drag::new(items, &picture, at, on, mtm);
    let timestamp = NSApp(mtm).currentEvent().map(|e| e.timestamp()).unwrap_or(0.0);
    let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        NSEventType::LeftMouseDragged,
        at,
        NSEventModifierFlags::empty(),
        timestamp,
        ns_window.windowNumber(),
        None,
        0,
        1,
        1.0,
    )
    .context("no event")?;
    // The view the press went to (the web view, inside the content view).
    let pressed = web_view(&content).unwrap_or(content.clone());
    super::trace(&format!("pressed: {}", pressed.class().name().to_string_lossy()));
    let source: Retained<Source> =
        unsafe { msg_send![super(Source::alloc(mtm).set_ivars(Pressed { drag, view: pressed })), init] };
    let items = source.ivars().drag.items();
    super::trace(&format!("begun: {} items, buttons {}", items.count(), NSEvent::pressedMouseButtons()));
    content.beginDraggingSessionWithItems_event_source(items, &event, ProtocolObject::from_ref(&*source));
    Ok(())
}
