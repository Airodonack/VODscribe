//! The native surface the picture is drawn on, under the page.
//!
//! On Linux the window is a GTK widget tree with the page's WebKitWebView in
//! a box. The view is moved into a GtkOverlay with a GtkGLArea, placed one of
//! two ways (`Place`). Over the page, the area covers just the player's box,
//! where the page can't draw: that works. Under the page, the area paints
//! the page's background everywhere and the picture in the box, and the
//! page is drawn transparent over it, so it could draw over the picture;
//! but with the area in the tree GTK 3 draws the window through GL, and the
//! transparent view then shows nothing at all (WebKitGTK 2.54 on X11, with
//! and without its DMA-BUF renderer). It's kept to try elsewhere. Other
//! platforms have no surface yet.

#[cfg(target_os = "linux")]
pub use linux::{Place, Screen};

#[cfg(target_os = "linux")]
mod linux {
    use std::sync::{Arc, Mutex, OnceLock};

    use anyhow::{Context, Result, anyhow};
    use gtk::prelude::*;

    use crate::Layout;

    /// Where the surface is in the window's widget tree, relative to the page.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Place {
        /// Under the whole page, which is drawn transparent over it.
        Under,
        /// Over the page, covering only the picture's box; the page stays opaque.
        Over,
    }

    /// The surface, to tell what to show.
    #[derive(Clone)]
    pub struct Screen {
        place: Place,
        layout: Arc<Mutex<Layout>>,
        area: glib::SendWeakRef<gtk::GLArea>,
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
            area.set_has_alpha(false);
            area.set_auto_render(true);
            let layout = Arc::new(Mutex::new(Layout::default()));
            let shown = layout.clone();
            area.connect_render(move |area, _| {
                paint(area, *shown.lock().unwrap(), place);
                glib::Propagation::Stop
            });
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
                    overlay.set_overlay_pass_through(&area, true);
                    let placed = layout.clone();
                    let box_ = area.clone();
                    // With no box open, a pixel outside the overlay (clipped
                    // away): hidden, or left to GTK's own placement (the whole
                    // overlay), the area blacks out the page.
                    overlay.connect_get_child_position(move |_, child| {
                        if *child != box_ {
                            return None;
                        }
                        let Some(rect) = placed.lock().unwrap().rect else {
                            return Some(gtk::Allocation::new(-1, -1, 1, 1));
                        };
                        let (x, y) = (rect.x.round() as i32, rect.y.round() as i32);
                        Some(gtk::Allocation::new(
                            x,
                            y,
                            rect.width.round().max(1.0) as i32,
                            rect.height.round().max(1.0) as i32,
                        ))
                    });
                }
            }
            parent.pack_start(&overlay, true, true, 0);
            overlay.show_all();
            Ok(Screen {
                place,
                layout,
                area: glib::SendWeakRef::from(area.downgrade()),
                overlay: glib::SendWeakRef::from(overlay.downgrade()),
            })
        }

        /// Shows `layout`, from any thread.
        pub fn set(&self, layout: Layout) {
            *self.layout.lock().unwrap() = layout;
            let (area, overlay, place) = (self.area.clone(), self.overlay.clone(), self.place);
            glib::MainContext::default().invoke(move || {
                let Some(area) = area.upgrade() else { return };
                if place == Place::Over
                    && let Some(overlay) = overlay.upgrade()
                {
                    overlay.queue_resize();
                }
                area.queue_render();
            });
        }
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

    /// Paints `layout` into `area`: under the page, its background and the
    /// picture's box; over it, the area is the box.
    fn paint(area: &gtk::GLArea, layout: Layout, place: Place) {
        load_gl();
        // Nothing to show yet: black, as a screen with no picture.
        let picture = [0.0, 0.0, 0.0];
        let [r, g, b] = if place == Place::Over { picture } else { layout.background };
        let scale = area.scale_factor() as f64;
        let height = area.allocated_height() as f64 * scale;
        unsafe {
            gl::Disable(gl::SCISSOR_TEST);
            gl::ClearColor(r, g, b, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
            if let (Place::Under, Some(rect)) = (place, layout.rect) {
                // GL counts rows from the bottom; the page from the top.
                let x = (rect.x * scale).round() as i32;
                let y = (height - (rect.y + rect.height) * scale).round() as i32;
                let w = (rect.width * scale).round() as i32;
                let h = (rect.height * scale).round() as i32;
                gl::Enable(gl::SCISSOR_TEST);
                gl::Scissor(x, y, w, h);
                let [r, g, b] = picture;
                gl::ClearColor(r, g, b, 1.0);
                gl::Clear(gl::COLOR_BUFFER_BIT);
                gl::Disable(gl::SCISSOR_TEST);
            }
        }
    }
}
