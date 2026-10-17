//! The surface on Linux: a native surface of its own over the window,
//! covering the player's box, drawn by the compositor (compose.rs) through
//! wgpu: Vulkan, or OpenGL where there's no Vulkan adapter. Under Wayland
//! it's a subsurface of the window's surface; under X11, a child window of
//! the window's. Either way GTK and the page know nothing of it: drawing a
//! GtkGLArea in the window's widget tree instead put GTK 3 on drawing the
//! whole window through GL, and with a real compositor or GPU driver
//! (GNOME on Wayland; X11 on NVIDIA) the page then showed black.
//!
//! The surface takes no input (an empty input region), so clicks, the
//! pointer and the wheel go to the page under it. It's made on the main
//! thread when a box is first set (and the window is on screen), dropped
//! when the box goes, and painted from any thread.

use std::ffi::{CString, c_void};
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use gtk::prelude::*;
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, XlibDisplayHandle, XlibWindowHandle,
};
use wayland_client::backend::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_compositor::WlCompositor, wl_region::WlRegion, wl_registry::WlRegistry, wl_subcompositor::WlSubcompositor,
    wl_subsurface::WlSubsurface, wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use x11_dl::xlib;

use crate::compose::Compositor;
use crate::decode::Frame;
use crate::{Overlay, Rect};

/// The surface in the app's window, to tell what to show.
#[derive(Clone)]
pub struct Screen {
    inner: Arc<Mutex<Inner>>,
    anchor: glib::SendWeakRef<gtk::Widget>,
}

struct Inner {
    rect: Option<Rect>,
    frame: Option<Arc<Frame>>,
    overlay: Option<Arc<Overlay>>,
    /// The surface and its drawing, while there's a box (and the window is shown).
    drawn: Option<Drawn>,
    /// The surface's size in pixels.
    size: (u32, u32),
    /// Paints done, and the time they took in all (for measuring).
    painted: (u64, Duration),
}

impl Screen {
    /// A surface over `anchor`, a widget in a window: the box it's set to is in
    /// the anchor's coordinates (for the app, the page's view). Call on the
    /// main thread. Nothing shows until a box is set.
    pub fn attach(anchor: &gtk::Widget) -> Screen {
        let inner =
            Inner { rect: None, frame: None, overlay: None, drawn: None, size: (1, 1), painted: Default::default() };
        let screen = Screen { inner: Arc::new(Mutex::new(inner)), anchor: glib::SendWeakRef::from(anchor.downgrade()) };
        // A box set before the window was on screen is placed once it is.
        let again = screen.clone();
        anchor.connect_map(move |_| {
            let rect = again.inner.lock().unwrap().rect;
            again.set(rect);
        });
        screen
    }

    /// Shows the picture in `rect` (in the anchor's coordinates), from any
    /// thread: the surface goes to that box, or goes away with none.
    pub fn set(&self, rect: Option<Rect>) {
        self.inner.lock().unwrap().rect = rect;
        let (inner, anchor) = (self.inner.clone(), self.anchor.clone());
        glib::MainContext::default().invoke(move || {
            let Some(anchor) = anchor.upgrade() else { return };
            let mut inner = inner.lock().unwrap();
            if let Err(e) = inner.place(&anchor) {
                eprintln!("The player's screen: {e:#}");
                inner.drawn = None;
            }
        });
    }

    /// Shows `frame` (none: black), from any thread.
    pub fn show(&self, frame: Option<Arc<Frame>>) {
        let mut inner = self.inner.lock().unwrap();
        inner.frame = frame;
        inner.paint();
    }

    /// Shows `overlay` over the picture (none: nothing), from any thread.
    pub fn overlay(&self, overlay: Option<Arc<Overlay>>) {
        let mut inner = self.inner.lock().unwrap();
        inner.overlay = overlay;
        inner.paint();
    }

    /// Paints done, and the time they took in all.
    pub fn painted(&self) -> (u64, Duration) {
        self.inner.lock().unwrap().painted
    }
}

impl Inner {
    /// Puts the surface where the box is (on the main thread).
    fn place(&mut self, anchor: &gtk::Widget) -> Result<()> {
        let Some(rect) = self.rect.filter(|r| r.width >= 1.0 && r.height >= 1.0) else {
            self.drawn = None;
            return Ok(());
        };
        if !anchor.is_mapped() {
            // Placed when it's mapped (`attach`).
            self.drawn = None;
            return Ok(());
        }
        let toplevel = anchor.toplevel().context("the anchor isn't in a window")?;
        let window = toplevel.window().context("the window isn't realized")?;
        // The box in the window's surface: GTK's widget coordinates in a
        // toplevel are its surface's (decorations and shadow included).
        let (ax, ay) = anchor.translate_coordinates(&toplevel, 0, 0).context("the anchor isn't in its window")?;
        let scale = toplevel.scale_factor().max(1);
        let place = Place {
            x: ax + rect.x.round() as i32,
            y: ay + rect.y.round() as i32,
            width: rect.width.round() as i32,
            height: rect.height.round() as i32,
            scale,
        };
        match self.drawn.as_mut() {
            Some(drawn) => drawn.native.place(place),
            None => self.drawn = Some(Drawn::new(&window, place)?),
        }
        if matches!(self.drawn.as_ref().map(|d| &d.native), Some(Native::Wayland(_))) {
            // A subsurface's position takes effect when its parent commits.
            toplevel.queue_draw();
        }
        self.size = ((place.width * scale) as u32, (place.height * scale) as u32);
        self.paint();
        Ok(())
    }

    fn paint(&mut self) {
        let Some(drawn) = self.drawn.as_mut() else { return };
        let began = std::time::Instant::now();
        // Device pixels to the page's CSS pixels.
        let scale = self.rect.map_or(1.0, |r| self.size.0 as f64 / r.width.max(1.0));
        let overlay = self.overlay.as_deref().map(|o| (o, scale));
        if let Err(e) = drawn.paint(self.size, self.frame.as_deref(), overlay) {
            eprintln!("The player's screen couldn't draw: {e:#}");
            return;
        }
        self.painted.0 += 1;
        self.painted.1 += began.elapsed();
    }
}

/// Where the surface goes in its window's surface, in logical pixels, and
/// the window's scale.
#[derive(Clone, Copy, Debug)]
struct Place {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    scale: i32,
}

/// The native surface and the drawing on it. The drawing goes first: its
/// wgpu surface is made on the native one.
struct Drawn {
    painter: Painter,
    native: Native,
}

// SAFETY: the raw handles in it are used only under the screen's lock, by
// one thread at a time.
unsafe impl Send for Drawn {}

impl Drawn {
    /// A surface over `window`'s (a GTK toplevel), at `place`.
    fn new(window: &gtk::gdk::Window, place: Place) -> Result<Drawn> {
        let native = match window.display().type_().name() {
            "GdkWaylandDisplay" => Native::Wayland(Wayland::new(window, place)?),
            "GdkX11Display" => Native::X11(Box::new(X11::new(window, place)?)),
            other => bail!("no surface for {other}"),
        };
        let size = ((place.width * place.scale).max(1) as u32, (place.height * place.scale).max(1) as u32);
        let painter = Painter::new(&native, size)?;
        Ok(Drawn { painter, native })
    }

    fn paint(&mut self, size: (u32, u32), frame: Option<&Frame>, overlay: Option<(&Overlay, f64)>) -> Result<()> {
        self.painter.paint(size, frame, overlay)?;
        if let Native::Wayland(w) = &mut self.native {
            // Our queue only gets the surface's own news (outputs it enters).
            let _ = w.queue.dispatch_pending(&mut Ignore);
        }
        Ok(())
    }
}

/// The compositor drawing into a wgpu surface on the native one.
struct Painter {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    compositor: Compositor,
}

impl Painter {
    fn new(native: &Native, size: (u32, u32)) -> Result<Painter> {
        let (display, window) = native.handles()?;
        // Vulkan where there's an adapter for it (a GPU's driver, or Mesa's
        // lavapipe), else OpenGL.
        let mut last = None;
        for backends in [wgpu::Backends::VULKAN, wgpu::Backends::GL] {
            let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
            desc.backends = backends;
            let instance = wgpu::Instance::new(desc);
            let target =
                wgpu::SurfaceTargetUnsafe::RawHandle { raw_display_handle: Some(display), raw_window_handle: window };
            // SAFETY: the native surface outlives this one (`Drawn` drops the painter first).
            let surface = match unsafe { instance.create_surface_unsafe(target) } {
                Ok(surface) => surface,
                Err(e) => {
                    last = Some(anyhow::Error::from(e));
                    continue;
                }
            };
            let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            }));
            let adapter = match adapter {
                Ok(adapter) => adapter,
                Err(e) => {
                    last = Some(e.into());
                    continue;
                }
            };
            let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
            let caps = surface.get_capabilities(&adapter);
            let mut config = surface.get_default_config(&adapter, size.0, size.1).context("no surface format")?;
            config.format = config.format.remove_srgb_suffix();
            // Each frame shown as it comes, never waiting for the screen: a
            // hidden subsurface's would otherwise wait for good under Wayland.
            config.present_mode = [wgpu::PresentMode::Immediate, wgpu::PresentMode::Mailbox]
                .into_iter()
                .find(|m| caps.present_modes.contains(m))
                .unwrap_or(wgpu::PresentMode::Fifo);
            config.alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
                wgpu::CompositeAlphaMode::Opaque
            } else {
                caps.alpha_modes[0]
            };
            surface.configure(&device, &config);
            let compositor = Compositor::with_device(device, queue, config.format, adapter.get_info());
            return Ok(Painter { surface, config, compositor });
        }
        Err(last.unwrap_or_else(|| anyhow::anyhow!("no GPU adapter"))).context("the screen's GPU")
    }

    fn paint(&mut self, size: (u32, u32), frame: Option<&Frame>, overlay: Option<(&Overlay, f64)>) -> Result<()> {
        let (width, height) = (size.0.max(1), size.1.max(1));
        if (self.config.width, self.config.height) != (width, height) {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(self.compositor.device(), &self.config);
        }
        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(self.compositor.device(), &self.config);
                return Ok(());
            }
            // Hidden, or the screen is busy: the next frame draws.
            _ => return Ok(()),
        };
        let view = output.texture.create_view(&Default::default());
        let drawn = self.compositor.draw(&view, (width, height), frame, overlay);
        self.compositor.submit(drawn);
        self.compositor.present(output);
        Ok(())
    }
}

enum Native {
    Wayland(Wayland),
    X11(Box<X11>),
}

impl Native {
    /// Its display and window, for wgpu.
    fn handles(&self) -> Result<(RawDisplayHandle, RawWindowHandle)> {
        Ok(match self {
            Native::Wayland(w) => (
                RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
                    NonNull::new(w.display as *mut c_void).context("no Wayland display")?,
                )),
                RawWindowHandle::Wayland(WaylandWindowHandle::new(
                    NonNull::new(w.surface.id().as_ptr() as *mut c_void).context("no Wayland surface")?,
                )),
            ),
            Native::X11(x) => (
                RawDisplayHandle::Xlib(XlibDisplayHandle::new(NonNull::new(x.display as *mut c_void), x.screen)),
                RawWindowHandle::Xlib(XlibWindowHandle::new(x.window)),
            ),
        })
    }

    fn place(&mut self, place: Place) {
        match self {
            Native::Wayland(w) => w.place(place),
            Native::X11(x) => x.place(place),
        }
    }
}

/// Wayland's events for our objects, which nothing here needs.
struct Ignore;

delegate_noop!(Ignore: WlCompositor);
delegate_noop!(Ignore: WlSubcompositor);
delegate_noop!(Ignore: WlRegion);
delegate_noop!(Ignore: WlSubsurface);
delegate_noop!(Ignore: ignore WlSurface);

impl Dispatch<WlRegistry, GlobalListContents> for Ignore {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// A subsurface of the window's surface, on GTK's own connection (a
/// subsurface must be on its parent's), with its own event queue.
struct Wayland {
    display: *mut wayland_sys::client::wl_display,
    conn: Connection,
    queue: EventQueue<Ignore>,
    surface: WlSurface,
    subsurface: WlSubsurface,
}

impl Wayland {
    fn new(window: &gtk::gdk::Window, place: Place) -> Result<Wayland> {
        use gtk::glib::translate::ToGlibPtr;
        let gdk_display: *mut gtk::gdk::ffi::GdkDisplay = window.display().to_glib_none().0;
        let gdk_window: *mut gtk::gdk::ffi::GdkWindow = window.to_glib_none().0;
        // SAFETY: a Wayland display and window of GTK's (checked by the caller).
        let (display, parent) = unsafe {
            (
                gdk_wayland_sys::gdk_wayland_display_get_wl_display(gdk_display as *mut _)
                    as *mut wayland_sys::client::wl_display,
                gdk_wayland_sys::gdk_wayland_window_get_wl_surface(gdk_window as *mut _)
                    as *mut wayland_sys::client::wl_proxy,
            )
        };
        if display.is_null() || parent.is_null() {
            bail!("the window has no Wayland surface yet");
        }
        // SAFETY: GTK's display, alive as long as the program; the backend
        // makes its own event queue on it and doesn't close it.
        let conn = Connection::from_backend(unsafe { Backend::from_foreign_display(display) });
        let (globals, queue) = registry_queue_init::<Ignore>(&conn).context("Wayland's globals")?;
        let qh = queue.handle();
        let compositor: WlCompositor = globals.bind(&qh, 1..=4, ()).context("wl_compositor")?;
        let subcompositor: WlSubcompositor = globals.bind(&qh, 1..=1, ()).context("wl_subcompositor")?;
        // SAFETY: GTK's surface for the window, alive while it's shown.
        let parent = WlSurface::from_id(&conn, unsafe { ObjectId::from_ptr(WlSurface::interface(), parent) }?)?;
        let surface = compositor.create_surface(&qh, ());
        let subsurface = subcompositor.get_subsurface(&surface, &parent, &qh, ());
        // Frames go up as they're drawn, not when GTK next draws the window.
        subsurface.set_desync();
        let nothing = compositor.create_region(&qh, ());
        surface.set_input_region(Some(&nothing));
        nothing.destroy();
        let mut wayland = Wayland { display, conn, queue, surface, subsurface };
        wayland.place(place);
        Ok(wayland)
    }

    fn place(&mut self, place: Place) {
        // Its size is its buffers', which wgpu makes the box's in device pixels.
        self.subsurface.set_position(place.x, place.y);
        self.surface.set_buffer_scale(place.scale);
        let _ = self.conn.flush();
    }
}

impl Drop for Wayland {
    fn drop(&mut self) {
        // After wgpu's surface on it (`Drawn` drops the painter first).
        self.subsurface.destroy();
        self.surface.destroy();
        let _ = self.conn.flush();
    }
}

/// A child window of the window's, on a connection of our own (so it's
/// used from any thread without GTK's).
struct X11 {
    xlib: xlib::Xlib,
    display: *mut xlib::Display,
    screen: i32,
    window: xlib::Window,
    colormap: xlib::Colormap,
}

impl X11 {
    fn new(window: &gtk::gdk::Window, place: Place) -> Result<X11> {
        use gtk::glib::translate::ToGlibPtr;
        let xlib = xlib::Xlib::open().context("libX11")?;
        let xfixes = x11_dl::xfixes::Xlib::open().context("libXfixes")?;
        let gdk_window: *mut gtk::gdk::ffi::GdkWindow = window.to_glib_none().0;
        // SAFETY: an X11 window of GTK's (checked by the caller).
        let parent = unsafe { gdk_x11_sys::gdk_x11_window_get_xid(gdk_window as *mut _) };
        let name = CString::new(window.display().name().as_str())?;
        // SAFETY: plain Xlib calls on our own connection, checked as they go.
        unsafe {
            let display = (xlib.XOpenDisplay)(name.as_ptr());
            if display.is_null() {
                bail!("can't connect to X display {name:?}");
            }
            let screen = (xlib.XDefaultScreen)(display);
            let mut x11 = X11 { xlib, display, screen, window: 0, colormap: 0 };
            // The screen's own opaque visual (GTK's window may have an RGBA one).
            let (visual, depth) =
                ((x11.xlib.XDefaultVisual)(display, screen), (x11.xlib.XDefaultDepth)(display, screen));
            x11.colormap = (x11.xlib.XCreateColormap)(display, parent, visual, xlib::AllocNone);
            let mut attributes: xlib::XSetWindowAttributes = std::mem::zeroed();
            attributes.colormap = x11.colormap;
            attributes.background_pixmap = 0;
            attributes.border_pixel = 0;
            x11.window = (x11.xlib.XCreateWindow)(
                display,
                parent,
                place.x * place.scale,
                place.y * place.scale,
                (place.width * place.scale).max(1) as u32,
                (place.height * place.scale).max(1) as u32,
                0,
                depth,
                xlib::InputOutput as u32,
                visual,
                xlib::CWColormap | xlib::CWBackPixmap | xlib::CWBorderPixel,
                &mut attributes,
            );
            if x11.window == 0 {
                bail!("no child window");
            }
            // No input: the pointer goes to the window under it, the page's.
            const SHAPE_INPUT: i32 = 2;
            let nothing = (xfixes.XFixesCreateRegion)(display, std::ptr::null_mut(), 0);
            (xfixes.XFixesSetWindowShapeRegion)(display, x11.window, SHAPE_INPUT, 0, 0, nothing);
            (xfixes.XFixesDestroyRegion)(display, nothing);
            (x11.xlib.XMapWindow)(display, x11.window);
            (x11.xlib.XSync)(display, 0);
            Ok(x11)
        }
    }

    fn place(&mut self, place: Place) {
        // SAFETY: our window, on our connection.
        unsafe {
            (self.xlib.XMoveResizeWindow)(
                self.display,
                self.window,
                place.x * place.scale,
                place.y * place.scale,
                (place.width * place.scale).max(1) as u32,
                (place.height * place.scale).max(1) as u32,
            );
            (self.xlib.XFlush)(self.display);
        }
    }
}

impl Drop for X11 {
    fn drop(&mut self) {
        // SAFETY: ours, made in `new`; wgpu's surface on the window goes
        // first (`Drawn` drops the painter before this).
        unsafe {
            if self.window != 0 {
                (self.xlib.XDestroyWindow)(self.display, self.window);
            }
            if self.colormap != 0 {
                (self.xlib.XFreeColormap)(self.display, self.colormap);
            }
            (self.xlib.XCloseDisplay)(self.display);
        }
    }
}
