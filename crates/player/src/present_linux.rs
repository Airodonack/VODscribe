//! The surface on Linux: a native surface of its own over the window,
//! covering the player's box, drawn with its own EGL context. Under Wayland
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
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use gtk::prelude::*;
use khronos_egl as egl;
use wayland_client::backend::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_compositor::WlCompositor, wl_region::WlRegion, wl_registry::WlRegistry, wl_subcompositor::WlSubcompositor,
    wl_subsurface::WlSubsurface, wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use x11_dl::xlib;

use crate::Rect;
use crate::decode::Frame;

type Egl = egl::DynamicInstance<egl::EGL1_5>;

/// EGL, loaded once (libEGL, from the GPU's driver or Mesa).
fn egl() -> Result<&'static Egl> {
    static EGL: OnceLock<Option<Egl>> = OnceLock::new();
    EGL.get_or_init(|| unsafe { Egl::load_required() }.ok()).as_ref().context("EGL 1.5 isn't installed")
}

/// The surface in the app's window, to tell what to show.
#[derive(Clone)]
pub struct Screen {
    inner: Arc<Mutex<Inner>>,
    anchor: glib::SendWeakRef<gtk::Widget>,
}

struct Inner {
    rect: Option<Rect>,
    frame: Option<Arc<Frame>>,
    /// The surface and its GL, while there's a box (and the window is shown).
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
        let inner = Inner { rect: None, frame: None, drawn: None, size: (1, 1), painted: Default::default() };
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
        if let Err(e) = drawn.paint(self.size, self.frame.as_deref()) {
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

/// The native surface and the GL drawing on it. The GL goes first: its
/// EGL surface is made on the native one.
struct Drawn {
    gl: Gl,
    native: Native,
}

// SAFETY: the raw handles in it are used only under the screen's lock, by
// one thread at a time, and the EGL context is made current only for a paint
// and released after it.
unsafe impl Send for Drawn {}

impl Drawn {
    /// A surface over `window`'s (a GTK toplevel), at `place`.
    fn new(window: &gtk::gdk::Window, place: Place) -> Result<Drawn> {
        let egl = egl()?;
        let backend = window.display().type_().name();
        let (native, display, owns_display) = match backend {
            "GdkWaylandDisplay" => {
                let native = Wayland::new(window, place)?;
                // SAFETY: GTK's display, which lives as long as the program.
                let display = unsafe {
                    egl.get_platform_display(PLATFORM_WAYLAND, native.display as *mut c_void, &[egl::ATTRIB_NONE])
                }?;
                // Not terminated: GTK and WebKit may use EGL on this display too.
                (Native::Wayland(native), display, false)
            }
            "GdkX11Display" => {
                let (native, display) = X11::new(egl, window, place)?;
                (Native::X11(Box::new(native)), display, true)
            }
            other => bail!("no surface for {other}"),
        };
        let gl = Gl::new(egl, display, owns_display, &native)?;
        Ok(Drawn { gl, native })
    }

    fn paint(&mut self, size: (u32, u32), frame: Option<&Frame>) -> Result<()> {
        let gl = &mut self.gl;
        let egl = gl.egl;
        egl.make_current(gl.display, Some(gl.surface), Some(gl.surface), Some(gl.context))?;
        gl.painter.paint(size, frame);
        let swapped = egl.swap_buffers(gl.display, gl.surface);
        egl.make_current(gl.display, None, None, None)?;
        if let Native::Wayland(w) = &mut self.native {
            // Our queue only gets the surface's own news (outputs it enters).
            let _ = w.queue.dispatch_pending(&mut Ignore);
        }
        Ok(swapped?)
    }
}

const PLATFORM_WAYLAND: egl::Enum = 0x31D8;
const PLATFORM_X11: egl::Enum = 0x31D5;

/// EGL drawing on a native surface.
struct Gl {
    egl: &'static Egl,
    display: egl::Display,
    /// Whether the display is ours to terminate (X11: on our own connection).
    owns_display: bool,
    context: egl::Context,
    surface: egl::Surface,
    painter: Painter,
}

impl Gl {
    fn new(egl: &'static Egl, display: egl::Display, owns_display: bool, native: &Native) -> Result<Gl> {
        egl.initialize(display).context("EGL on the display")?;
        egl.bind_api(egl::OPENGL_API)?;
        let config = config(egl, display)?;
        let context = egl
            .create_context(
                display,
                config,
                None,
                &[
                    egl::CONTEXT_MAJOR_VERSION,
                    3,
                    egl::CONTEXT_MINOR_VERSION,
                    2,
                    egl::CONTEXT_OPENGL_PROFILE_MASK,
                    egl::CONTEXT_OPENGL_CORE_PROFILE_BIT,
                    egl::NONE,
                ],
            )
            .context("an OpenGL 3.2 context")?;
        let window = match native {
            Native::Wayland(w) => w.egl_window as egl::NativeWindowType,
            Native::X11(x) => x.window as egl::NativeWindowType,
        };
        // SAFETY: the native window lives as long as the surface (`Drawn`).
        let surface = unsafe { egl.create_window_surface(display, config, window, None) }?;
        egl.make_current(display, Some(surface), Some(surface), Some(context))?;
        // Each frame is shown as it comes, never waiting for the screen: a
        // hidden window's would otherwise wait for good under Wayland.
        let _ = egl.swap_interval(display, 0);
        load_gl(egl);
        let painter = Painter::new();
        egl.make_current(display, None, None, None)?;
        Ok(Gl { egl, display, owns_display, context, surface, painter })
    }
}

impl Drop for Gl {
    fn drop(&mut self) {
        let egl = self.egl;
        let _ = egl.make_current(self.display, Some(self.surface), Some(self.surface), Some(self.context));
        self.painter.delete();
        let _ = egl.make_current(self.display, None, None, None);
        let _ = egl.destroy_surface(self.display, self.surface);
        let _ = egl.destroy_context(self.display, self.context);
        if self.owns_display {
            let _ = egl.terminate(self.display);
        }
    }
}

/// An 8-bit RGB window config that takes desktop OpenGL.
fn config(egl: &Egl, display: egl::Display) -> Result<egl::Config> {
    let wanted = [
        egl::SURFACE_TYPE,
        egl::WINDOW_BIT,
        egl::RENDERABLE_TYPE,
        egl::OPENGL_BIT,
        egl::RED_SIZE,
        8,
        egl::GREEN_SIZE,
        8,
        egl::BLUE_SIZE,
        8,
        egl::NONE,
    ];
    egl.choose_first_config(display, &wanted)?.context("no EGL config for an RGB window with OpenGL")
}

/// GL's functions, found through EGL once (the same for every context of
/// the one driver).
fn load_gl(egl: &Egl) {
    static LOADED: OnceLock<()> = OnceLock::new();
    LOADED.get_or_init(|| {
        gl::load_with(|name| egl.get_proc_address(name).map_or(std::ptr::null(), |f| f as *const c_void));
    });
}

enum Native {
    Wayland(Wayland),
    X11(Box<X11>),
}

impl Native {
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
    egl_window: *mut wayland_sys::egl::wl_egl_window,
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
        let proxy = surface.id().as_ptr();
        let egl = wayland_sys::egl::wayland_egl_option().context("libwayland-egl isn't installed")?;
        // SAFETY: our surface, which outlives the EGL window (Drop).
        let egl_window = unsafe { (egl.wl_egl_window_create)(proxy, 1, 1) };
        if egl_window.is_null() {
            bail!("no EGL window on the subsurface");
        }
        let mut wayland = Wayland { display, conn, queue, surface, subsurface, egl_window };
        wayland.place(place);
        Ok(wayland)
    }

    fn place(&mut self, place: Place) {
        self.subsurface.set_position(place.x, place.y);
        self.surface.set_buffer_scale(place.scale);
        let egl = wayland_sys::egl::wayland_egl_handle();
        // SAFETY: our EGL window.
        unsafe {
            (egl.wl_egl_window_resize)(self.egl_window, place.width * place.scale, place.height * place.scale, 0, 0)
        };
        let _ = self.conn.flush();
    }
}

impl Drop for Wayland {
    fn drop(&mut self) {
        let egl = wayland_sys::egl::wayland_egl_handle();
        // SAFETY: made in `new`, destroyed once, after the EGL surface on it (`Drawn`).
        unsafe { (egl.wl_egl_window_destroy)(self.egl_window) };
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
    window: xlib::Window,
    colormap: xlib::Colormap,
}

impl X11 {
    fn new(egl: &Egl, window: &gtk::gdk::Window, place: Place) -> Result<(X11, egl::Display)> {
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
            let mut x11 = X11 { xlib, display, window: 0, colormap: 0 };
            let egl_display = egl.get_platform_display(PLATFORM_X11, display as *mut c_void, &[egl::ATTRIB_NONE])?;
            egl.initialize(egl_display).context("EGL on the X display")?;
            // The window takes the visual EGL draws with.
            let config = config(egl, egl_display)?;
            let id = egl.get_config_attrib(egl_display, config, egl::NATIVE_VISUAL_ID)?;
            let mut wanted: xlib::XVisualInfo = std::mem::zeroed();
            wanted.visualid = id as xlib::VisualID;
            let mut found = 0;
            let info = (x11.xlib.XGetVisualInfo)(display, xlib::VisualIDMask, &mut wanted, &mut found);
            if info.is_null() || found < 1 {
                bail!("no X visual for EGL's config");
            }
            let (visual, depth) = ((*info).visual, (*info).depth);
            (x11.xlib.XFree)(info as *mut _);
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
            Ok((x11, egl_display))
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
        // SAFETY: ours, made in `new`; the EGL display on the connection is
        // terminated first (`Gl`, dropped before this).
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

const VERTEX: &str = "#version 150
uniform vec4 box_;  // left, bottom, right, top in clip space
out vec2 uv;
void main() {
    vec2 corner = vec2(float(gl_VertexID & 1), float(gl_VertexID >> 1));
    uv = vec2(corner.x, 1.0 - corner.y);
    gl_Position = vec4(mix(box_.xy, box_.zw, corner), 0.0, 1.0);
}
";

const FRAGMENT: &str = "#version 150
uniform sampler2D picture;
in vec2 uv;
out vec4 color;
void main() {
    color = vec4(texture(picture, uv).rgb, 1.0);
}
";

/// The GL objects the surface draws with, made in its context.
struct Painter {
    program: u32,
    box_: i32,
    vao: u32,
    texture: u32,
    /// The frame in the texture (`Frame::id`).
    uploaded: Option<u64>,
}

impl Painter {
    fn new() -> Painter {
        // SAFETY: GL calls with the context current (`Gl::new`).
        unsafe {
            let program = gl::CreateProgram();
            for (kind, source) in [(gl::VERTEX_SHADER, VERTEX), (gl::FRAGMENT_SHADER, FRAGMENT)] {
                let shader = gl::CreateShader(kind);
                let (ptr, len) = (source.as_ptr() as *const i8, source.len() as i32);
                gl::ShaderSource(shader, 1, &ptr, &len);
                gl::CompileShader(shader);
                gl::AttachShader(program, shader);
                gl::DeleteShader(shader);
            }
            gl::LinkProgram(program);
            let box_ = gl::GetUniformLocation(program, c"box_".as_ptr());
            let mut vao = 0;
            gl::GenVertexArrays(1, &mut vao);
            let mut texture = 0;
            gl::GenTextures(1, &mut texture);
            gl::BindTexture(gl::TEXTURE_2D, texture);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
            Painter { program, box_, vao, texture, uploaded: None }
        }
    }

    /// Paints `frame` fitted in a surface of `size` pixels, centred, on black.
    fn paint(&mut self, size: (u32, u32), frame: Option<&Frame>) {
        let (width, height) = (size.0.max(1) as f64, size.1.max(1) as f64);
        // SAFETY: GL calls with the context current (`Drawn::paint`).
        unsafe {
            gl::Viewport(0, 0, width as i32, height as i32);
            gl::ClearColor(0.0, 0.0, 0.0, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
            let Some(frame) = frame else { return };
            gl::BindTexture(gl::TEXTURE_2D, self.texture);
            if self.uploaded != Some(frame.id) {
                gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    gl::RGB8 as i32,
                    frame.width as i32,
                    frame.height as i32,
                    0,
                    gl::RGB,
                    gl::UNSIGNED_BYTE,
                    frame.rgb.as_ptr() as *const _,
                );
                self.uploaded = Some(frame.id);
            }
            let fit = (width / frame.width as f64).min(height / frame.height as f64);
            let (fw, fh) = ((frame.width as f64 * fit).round(), (frame.height as f64 * fit).round());
            let (fx, fy) = (((width - fw) / 2.0).round(), ((height - fh) / 2.0).round());
            let clip = |v: f64, size: f64| (v / size * 2.0 - 1.0) as f32;
            gl::UseProgram(self.program);
            gl::Uniform4f(self.box_, clip(fx, width), clip(fy, height), clip(fx + fw, width), clip(fy + fh, height));
            gl::BindVertexArray(self.vao);
            gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
        }
    }

    /// Frees its GL objects (the context current).
    fn delete(&mut self) {
        // SAFETY: GL calls with the context current (`Gl::drop`).
        unsafe {
            gl::DeleteProgram(self.program);
            gl::DeleteVertexArrays(1, &self.vao);
            gl::DeleteTextures(1, &self.texture);
        }
    }
}
