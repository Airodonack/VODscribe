//! The native surface the picture is drawn on.
//!
//! On Linux it's a GtkGLArea (`Canvas`: a frame drawn into one with GL,
//! letterboxed in its box). In the app's window, which is a GTK widget tree
//! with the page's WebKitWebView in a box, the view is moved into a
//! GtkOverlay with the area (`Screen`), placed one of two ways (`Place`).
//! Over the page, the area covers just the player's box, where the page
//! can't draw: that works. Under the page, the area paints the page's
//! background everywhere and the picture in the box, and the page is drawn
//! transparent over it, so it could draw over the picture; but with the area
//! in the tree GTK 3 draws the window through GL, and the transparent view
//! then shows nothing at all (WebKitGTK 2.54 on X11, with and without its
//! DMA-BUF renderer). It's kept to try elsewhere. Other platforms have no
//! surface yet.

#[cfg(target_os = "linux")]
pub use linux::{Canvas, Place, Screen};

#[cfg(target_os = "linux")]
mod linux {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex, OnceLock};

    use anyhow::{Context, Result, anyhow};
    use gtk::prelude::*;

    use crate::Layout;
    use crate::decode::Frame;

    /// Where the surface is in the window's widget tree, relative to the page.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Place {
        /// Under the whole page, which is drawn transparent over it.
        Under,
        /// Over the page, covering only the picture's box; the page stays opaque.
        Over,
    }

    /// A GL area a frame is drawn into: in the layout's box (`Place::Under`,
    /// the rest painted the background) or filling it (`Place::Over`).
    #[derive(Clone)]
    pub struct Canvas {
        place: Place,
        layout: Arc<Mutex<Layout>>,
        frame: Arc<Mutex<Option<Arc<Frame>>>>,
        area: glib::SendWeakRef<gtk::GLArea>,
        /// Paints done, and the time they took in all (for measuring).
        painted: Arc<Mutex<(u64, std::time::Duration)>>,
    }

    impl Canvas {
        /// Makes `area` a canvas.
        pub fn new(area: &gtk::GLArea, place: Place) -> Canvas {
            area.set_has_alpha(false);
            area.set_auto_render(true);
            let layout = Arc::new(Mutex::new(Layout::default()));
            let frame = Arc::new(Mutex::new(None));
            let painter: Rc<RefCell<Option<Painter>>> = Rc::default();
            let painted: Arc<Mutex<(u64, std::time::Duration)>> = Arc::default();
            let (shown, picture, drawn, timed) = (layout.clone(), frame.clone(), painter.clone(), painted.clone());
            area.connect_render(move |area, _| {
                let began = std::time::Instant::now();
                let mut painter = drawn.borrow_mut();
                let painter = painter.get_or_insert_with(Painter::new);
                painter.paint(area, *shown.lock().unwrap(), picture.lock().unwrap().as_deref(), place);
                let mut timed = timed.lock().unwrap();
                timed.0 += 1;
                timed.1 += began.elapsed();
                glib::Propagation::Stop
            });
            // Its GL objects die with the context.
            area.connect_unrealize(move |_| *painter.borrow_mut() = None);
            Canvas { place, layout, frame, area: glib::SendWeakRef::from(area.downgrade()), painted }
        }

        /// Paints done, and the time they took in all.
        pub fn painted(&self) -> (u64, std::time::Duration) {
            *self.painted.lock().unwrap()
        }

        pub fn layout(&self) -> Layout {
            *self.layout.lock().unwrap()
        }

        /// Shows `layout`, from any thread.
        pub fn set_layout(&self, layout: Layout) {
            *self.layout.lock().unwrap() = layout;
            self.redraw();
        }

        /// Shows `frame` (none: black), from any thread.
        pub fn show(&self, frame: Option<Arc<Frame>>) {
            *self.frame.lock().unwrap() = frame;
            self.redraw();
        }

        fn redraw(&self) {
            let area = self.area.clone();
            glib::MainContext::default().invoke(move || {
                if let Some(area) = area.upgrade() {
                    area.queue_render();
                }
            });
        }
    }

    /// The surface in the app's window, to tell what to show.
    #[derive(Clone)]
    pub struct Screen {
        canvas: Canvas,
        overlay: glib::SendWeakRef<gtk::Overlay>,
    }

    impl Screen {
        /// Puts a surface at `place` in `window`. Call on the main thread,
        /// once the window has its page.
        pub fn attach(window: &gtk::Window, place: Place) -> Result<Screen> {
            let page = find_webview(window.upcast_ref()).ok_or_else(|| anyhow!("no page in the window"))?;
            let parent =
                page.parent().and_then(|p| p.downcast::<gtk::Box>().ok()).context("the page isn't in a box")?;
            let area = gtk::GLArea::new();
            let canvas = Canvas::new(&area, place);
            let overlay = gtk::Overlay::new();
            parent.remove(&page);
            match place {
                Place::Under => {
                    overlay.add(&area);
                    overlay.add_overlay(&page);
                }
                Place::Over => {
                    overlay.add(&page);
                    overlay.add_overlay(&area);
                    // Clicks, the pointer and the wheel go to the page, as
                    // if the area weren't there: the overlay's pass-through
                    // flag doesn't do it (the events still stop at the
                    // area), so each is handed to the page's window itself.
                    overlay.set_overlay_pass_through(&area, true);
                    area.add_events(
                        gtk::gdk::EventMask::BUTTON_PRESS_MASK
                            | gtk::gdk::EventMask::BUTTON_RELEASE_MASK
                            | gtk::gdk::EventMask::POINTER_MOTION_MASK
                            | gtk::gdk::EventMask::SCROLL_MASK
                            | gtk::gdk::EventMask::SMOOTH_SCROLL_MASK,
                    );
                    let to_page = page.clone();
                    area.connect_button_press_event(move |area, e| hand_on(area, &to_page, e));
                    let to_page = page.clone();
                    area.connect_button_release_event(move |area, e| hand_on(area, &to_page, e));
                    let to_page = page.clone();
                    area.connect_motion_notify_event(move |area, e| hand_on(area, &to_page, e));
                    let to_page = page.clone();
                    area.connect_scroll_event(move |area, e| hand_on(area, &to_page, e));
                }
            }
            parent.pack_start(&overlay, true, true, 0);
            overlay.show_all();
            Ok(Screen { canvas, overlay: glib::SendWeakRef::from(overlay.downgrade()) })
        }

        /// Shows `layout`, from any thread.
        pub fn set(&self, layout: Layout) {
            self.canvas.set_layout(layout);
            if self.canvas.place == Place::Over {
                let overlay = self.overlay.clone();
                glib::MainContext::default().invoke(move || {
                    if let Some(overlay) = overlay.upgrade() {
                        overlay.queue_resize();
                    }
                });
            }
        }

        /// Shows `frame`, from any thread.
        pub fn show(&self, frame: Option<Arc<Frame>>) {
            self.canvas.show(frame);
        }
    }

    /// Hands pointer event `e`, had by `area`, on to `page` as the page's
    /// own, at the same place.
    fn hand_on(area: &gtk::GLArea, page: &gtk::Widget, e: &gtk::gdk::Event) -> glib::Propagation {
        use gtk::glib::translate::{ToGlibPtr, ToGlibPtrMut};
        let (Some(from), Some(to)) = (area.window(), page.window()) else { return glib::Propagation::Proceed };
        let (Some((x, y)), Some(device)) = (e.coords(), e.device()) else { return glib::Propagation::Proceed };
        // Both windows sit in the same parent: the area's place in the page.
        let (ax, ay) = from.position();
        let (px, py) = to.position();
        let mut event = e.clone();
        // SAFETY: a pointer event (button, motion or scroll), whose window,
        // send_event, time, x and y fields lie the same way in each; the
        // window is referenced for the event, which unreferences it when freed.
        unsafe {
            let raw = ToGlibPtrMut::<*mut gtk::gdk::ffi::GdkEvent>::to_glib_none_mut(&mut event).0
                as *mut gtk::gdk::ffi::GdkEventButton;
            glib::gobject_ffi::g_object_unref((*raw).window as *mut _);
            (*raw).window = ToGlibPtr::<*mut gtk::gdk::ffi::GdkWindow>::to_glib_full(&to);
            (*raw).send_event = 1;
            (*raw).x = x + (ax - px) as f64;
            (*raw).y = y + (ay - py) as f64;
        }
        event.set_device(Some(&device));
        gtk::main_do_event(&mut event);
        glib::Propagation::Stop
    }

    /// The page's WebKitGTK view, inside `widget`.
    fn find_webview(widget: &gtk::Widget) -> Option<gtk::Widget> {
        if widget.type_().name() == "WebKitWebView" {
            return Some(widget.clone());
        }
        widget.downcast_ref::<gtk::Container>()?.children().iter().find_map(find_webview)
    }

    /// Loads GL once, from libepoxy as GTK itself draws with. It exports each
    /// GL function as a variable `epoxy_<name>` holding a pointer to it, which
    /// finds the driver's function the first time it's called.
    fn load_gl() {
        static LOADED: OnceLock<()> = OnceLock::new();
        LOADED.get_or_init(|| {
            // Kept open for the process: the function pointers point into it.
            let library = Box::leak(Box::new(
                unsafe { libloading::os::unix::Library::new("libepoxy.so.0") }.expect("libepoxy, which GTK links"),
            ));
            gl::load_with(|name| {
                let name = format!("epoxy_{name}");
                unsafe { library.get::<*const *const std::ffi::c_void>(name.as_bytes()) }
                    .map_or(std::ptr::null(), |s| unsafe { **s })
            });
        });
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

    /// The GL objects a canvas draws with, made in its context.
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
            load_gl();
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

        /// Paints `layout` into `area`: under the page, its background and the
        /// picture's box; over it, the area is the box. `frame` goes in the
        /// box, letterboxed, on black.
        fn paint(&mut self, area: &gtk::GLArea, layout: Layout, frame: Option<&Frame>, place: Place) {
            let scale = area.scale_factor() as f64;
            let (width, height) = (area.allocated_width() as f64 * scale, area.allocated_height() as f64 * scale);
            // The box, in pixels from the bottom left.
            let (bx, by, bw, bh) = match (place, layout.rect) {
                (Place::Under, Some(r)) => {
                    (r.x * scale, height - (r.y + r.height) * scale, r.width * scale, r.height * scale)
                }
                (Place::Under, None) => (0.0, 0.0, 0.0, 0.0),
                (Place::Over, _) => (0.0, 0.0, width, height),
            };
            let [r, g, b] = if place == Place::Over { [0.0; 3] } else { layout.background };
            unsafe {
                gl::Disable(gl::SCISSOR_TEST);
                gl::ClearColor(r, g, b, 1.0);
                gl::Clear(gl::COLOR_BUFFER_BIT);
                if bw < 1.0 || bh < 1.0 {
                    return;
                }
                gl::Enable(gl::SCISSOR_TEST);
                gl::Scissor(bx.round() as i32, by.round() as i32, bw.round() as i32, bh.round() as i32);
                gl::ClearColor(0.0, 0.0, 0.0, 1.0);
                gl::Clear(gl::COLOR_BUFFER_BIT);
                gl::Disable(gl::SCISSOR_TEST);
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
                // The frame fitted in the box, centred.
                let fit = (bw / frame.width as f64).min(bh / frame.height as f64);
                let (fw, fh) = ((frame.width as f64 * fit).round(), (frame.height as f64 * fit).round());
                let (fx, fy) = (bx + ((bw - fw) / 2.0).round(), by + ((bh - fh) / 2.0).round());
                let clip = |v: f64, size: f64| (v / size * 2.0 - 1.0) as f32;
                gl::UseProgram(self.program);
                gl::Uniform4f(
                    self.box_,
                    clip(fx, width),
                    clip(fy, height),
                    clip(fx + fw, width),
                    clip(fy + fh, height),
                );
                gl::BindVertexArray(self.vao);
                gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
            }
        }
    }
}
