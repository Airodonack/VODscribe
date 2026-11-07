//! GTK's drag of files out of the window, begun from the window itself with
//! `dragndrop_source::linux`'s targets, its signals passed on to it.
//!
//! While the drag holds an ask for files still being made (the crate's
//! "The hold"), GTK's main loop runs inside the ask, and Tauri's event loop
//! waits: closures run on the main thread through Tauri wait for the end of
//! it. What other threads send the page goes as a GLib source, which the
//! hold runs (page.rs).
//!
//! A drag watched for DaVinci Resolve (resolve.rs) refuses it the files as
//! it asks, where X11 names it; on Wayland, which doesn't, the watch hears
//! so.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use dragndrop_source::linux::{self, Drag};
use gtk::glib;
use gtk::prelude::*;

use super::{Event, Item, ReleasedEarly};

/// Begins GTK's drag of `items` from `window`, showing `image` (a PNG),
/// while its mouse button is down. On the main thread. `on` hears how it
/// goes; `resolve` watches it for DaVinci Resolve.
pub(crate) fn start(
    window: &tauri::WebviewWindow,
    items: Vec<Box<dyn Item>>,
    image: &[u8],
    resolve: Arc<crate::resolve::Watch>,
    on: impl Fn(Event) + Send + Sync + 'static,
) -> Result<()> {
    // A drag begun while the last one's answer waits would run inside it.
    if linux::holding() {
        anyhow::bail!("Still handing over the last drag.");
    }
    super::traced();
    let source = window.gtk_window()?;
    let page = crate::webview::page(window).ok();
    let drag = Drag::new(items, on);
    let targets = drag.targets();
    if resolve.applies() {
        if linux::names_receivers(&source.display()) {
            drag.refuse(move |asker| resolve.refuses(asker.class.as_deref(), asker.program.as_deref()));
        } else {
            resolve.unnamed();
        }
    }
    let handlers: Rc<RefCell<Vec<(gtk::Widget, glib::SignalHandlerId)>>> = Rc::default();
    let widget: gtk::Widget = source.clone().upcast();
    let asked = drag.clone();
    let data = source.connect_drag_data_get(move |_, context, data, info, _| asked.data_get(context, data, info));
    handlers.borrow_mut().push((widget.clone(), data));
    // Over the app's own page: an ask then is its own (as X11 tells by the
    // window asking; Wayland doesn't). GTK leaves the page just before a drop
    // on it, so the drop says so again.
    if let Some(page) = &page {
        let over = drag.clone();
        let motion = page.connect_drag_motion(move |_, _, _, _, _| {
            over.over_own(true);
            false
        });
        let over = drag.clone();
        let dropped = page.connect_drag_drop(move |_, _, _, _, _| {
            over.over_own(true);
            false
        });
        let over = drag.clone();
        let left = page.connect_drag_leave(move |_, _, _| over.over_own(false));
        handlers.borrow_mut().extend([(page.clone(), motion), (page.clone(), dropped), (page.clone(), left)]);
    }
    // Dropped where nothing takes it: no animation of it flying back, which
    // holds on to the pointer (losing the next press) until it's done.
    let failing = drag.clone();
    let on_failed = source.connect_drag_failed(move |_, _, result| {
        failing.failed(result);
        glib::Propagation::Stop
    });
    handlers.borrow_mut().push((widget.clone(), on_failed));
    // Once it's over (and no answer waits), the window stops answering for
    // it, and the page has its button back.
    let (ending, finished, told) = (drag.clone(), handlers.clone(), window.clone());
    let end = source.connect_drag_end(move |_, context| {
        let (handlers, told) = (finished.clone(), told.clone());
        ending.end(context, move || {
            for (widget, handler) in handlers.borrow_mut().drain(..) {
                widget.disconnect(handler);
            }
            // The press that started the drag never comes back up in the
            // page: the drag took the release. Without one, its next press
            // is lost.
            if let Ok(page) = crate::webview::page(&told) {
                release_button(&page);
            }
        });
    });
    handlers.borrow_mut().push((widget.clone(), end));
    // Its picture, set as it begins: else GTK sets its own icon then, loading
    // it from the icon theme through gdk-pixbuf, and aborts the app if no
    // loader can read it (an SVG, where the loaders are sandboxed away).
    // (One that can't be decoded: none to see, rather than a theme icon.)
    let picture =
        drag_picture(image).or_else(|| gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, 1, 1).ok());
    let begun = drag.clone();
    let begin = source.connect_drag_begin(move |_, context| {
        if let Some(surface) = &picture {
            context.drag_set_icon_surface(surface);
        }
        let dropped = begun.clone();
        context.connect_drop_performed(move |context, _| dropped.drop_performed(context));
    });
    handlers.borrow_mut().push((widget, begin));
    let undo = || {
        for (widget, handler) in handlers.borrow_mut().drain(..) {
            widget.disconnect(handler);
        }
        drag.withdraw();
    };
    // A drag begun with no button held never ends (nothing releases it), and
    // the file it was for is never forgotten: the release may have come in
    // the moment between the page asking and this. Refused, so the page
    // treats it as a drag that didn't happen.
    if source.window().and_then(|w| linux::button_held(&w)) == Some(false) {
        undo();
        return Err(ReleasedEarly.into());
    }
    if source.drag_begin_with_coordinates(&targets, gtk::gdk::DragAction::COPY, 1, None, -1, -1).is_none() {
        undo();
        anyhow::bail!("The drag couldn't start.");
    }
    Ok(())
}

/// The PNG `png` as a cairo surface for a drag's picture, decoded here.
fn drag_picture(png: &[u8]) -> Option<gtk::cairo::ImageSurface> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut pixels = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut pixels).ok()?;
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::GrayscaleAlpha => 2,
        _ => return None,
    };
    let (width, height) = (i32::try_from(info.width).ok()?, i32::try_from(info.height).ok()?);
    let mut surface = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, width, height).ok()?;
    let stride = surface.stride() as usize;
    {
        let mut data = surface.data().ok()?;
        for (y, row) in pixels.chunks(info.line_size).take(info.height as usize).enumerate() {
            for (x, px) in row.chunks(channels).take(info.width as usize).enumerate() {
                let (r, g, b, a) =
                    if channels == 4 { (px[0], px[1], px[2], px[3]) } else { (px[0], px[0], px[0], px[1]) };
                // Cairo's ARGB32: premultiplied, in the machine's byte order (little-endian here: B, G, R, A).
                let pre = |c: u8| (u16::from(c) * u16::from(a) / 255) as u8;
                let at = y * stride + x * 4;
                data[at..at + 4].copy_from_slice(&[pre(b), pre(g), pre(r), a]);
            }
        }
    }
    Some(surface)
}

/// Sends `page` the release of the first mouse button, where the pointer is.
fn release_button(page: &gtk::Widget) {
    use gtk::glib::translate::{ToGlibPtr, ToGlibPtrMut};
    let (Some(window), Some(device)) = (page.window(), page.display().default_seat().and_then(|s| s.pointer())) else {
        return;
    };
    let (_, x, y, _) = window.device_position_double(&device);
    let (_, root_x, root_y) = device.position_double();
    let mut event = gtk::gdk::Event::new(gtk::gdk::EventType::ButtonRelease);
    // SAFETY: a button event just made, of the type its fields are written as;
    // the window is referenced for the event, which unreferences it when freed.
    unsafe {
        let button = ToGlibPtrMut::<*mut gtk::gdk::ffi::GdkEvent>::to_glib_none_mut(&mut event).0
            as *mut gtk::gdk::ffi::GdkEventButton;
        (*button).window = ToGlibPtr::<*mut gtk::gdk::ffi::GdkWindow>::to_glib_full(&window);
        (*button).send_event = 1;
        (*button).time = gtk::gdk::ffi::GDK_CURRENT_TIME as u32;
        (*button).x = x;
        (*button).y = y;
        (*button).x_root = root_x;
        (*button).y_root = root_y;
        (*button).button = 1;
        (*button).state = gtk::gdk::ffi::GDK_BUTTON1_MASK;
    }
    event.set_device(Some(&device));
    gtk::main_do_event(&mut event);
}
