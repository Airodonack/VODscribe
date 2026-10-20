//! The surface on macOS: a view in the page's WKWebView, covering the page
//! (or, under the page, beside the WKWebView and behind it: `set_under`),
//! that hosts a layer of its own, and in that layer the picture's layer,
//! moved to the page's player box. The picture is each frame as an image,
//! which Core Animation fits in the box (black around it) and draws. That
//! needs no Metal of ours, so it also works where Metal doesn't (a VM). The
//! view hit-tests as nothing, so clicks and the pointer reach the page.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Result, bail};
use dispatch2::MainThreadBound;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::NSView;
use objc2_core_foundation::{CFData, CFRetained, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapInfo, CGColor, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo, kCGColorSpaceSRGB,
};
use objc2_quartz_core::{CALayer, CATransaction, kCAGravityResizeAspect};

use crate::decode::Frame;
use crate::{Overlay, Rect};

define_class!(
    /// The view over the page: draws only through its layer, and is never
    /// what the pointer is over.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "VODscribeScreen"]
    struct Cover;

    impl Cover {
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _point: CGPoint) -> *mut NSView {
            std::ptr::null_mut()
        }

        // Measured from the top, as the page's boxes are.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl Cover {
    /// Lies where the page is: the WKWebView less its safe area. The view
    /// reaches under a see-through title bar, and the page starts below it,
    /// except in full screen. Over the page it's in the WKWebView, which
    /// doesn't resize the views in it; under, it's beside it in their shared
    /// superview. So this is done each time the page says where its box is
    /// (`set`), which it does whenever its layout changes.
    fn fill(&self, page: &NSView, under: bool) {
        let inset = page.safeAreaInsets();
        let (x, y, size) = if under {
            let frame = page.frame();
            // SAFETY: the views' shared superview, which holds them.
            let flipped = unsafe { page.superview() }.is_some_and(|s| s.isFlipped());
            (frame.origin.x, frame.origin.y + if flipped { inset.top } else { inset.bottom }, frame.size)
        } else {
            // WKWebView is flipped: its top is at its bounds' origin.
            let bounds = page.bounds();
            (bounds.origin.x, bounds.origin.y + inset.top, bounds.size)
        };
        self.setFrame(CGRect::new(
            CGPoint::new(x + inset.left, y),
            CGSize::new(size.width - inset.left - inset.right, size.height - inset.top - inset.bottom),
        ));
    }

    /// Puts the view over the page (in the WKWebView) or under it (behind
    /// the WKWebView, in their superview: the page draws no background, so
    /// it shows where the page is clear).
    fn stack(&self, page: &NSView, under: bool) {
        self.removeFromSuperview();
        // SAFETY: the WKWebView's superview (the window's content view).
        match unsafe { page.superview() } {
            // SAFETY: NSView's addSubview:positioned:relativeTo:, NSWindowBelow
            // (-1), relative to a subview of the same view.
            Some(parent) if under => {
                let () = unsafe { msg_send![&*parent, addSubview: self, positioned: -1isize, relativeTo: page] };
            }
            _ => page.addSubview(self),
        }
        self.fill(page, under);
    }
}

/// The surface in the app's window, to tell what to show.
#[derive(Clone)]
pub struct Screen {
    /// The view, and the page's WKWebView it's over or under.
    cover: Arc<MainThreadBound<(Retained<Cover>, Retained<NSView>)>>,
    /// Under the page, not over it (`set_under`).
    under: Arc<AtomicBool>,
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    picture: Layer,
    /// The overlay's layer, in the picture's.
    over: Layer,
    /// The frame shown, and its pixels as 32-bit BGRX, which Core Animation
    /// takes without converting.
    id: Option<u64>,
    bgrx: Vec<u8>,
    /// Paints done, and the time they took in all (for measuring).
    painted: (u64, std::time::Duration),
}

/// The picture's layer.
struct Layer(Retained<CALayer>);

// SAFETY: Core Animation's layers may be changed from any thread, inside a
// transaction (`changed`); the view, which is the main thread's, isn't kept.
unsafe impl Send for Layer {}

impl Screen {
    /// Puts a surface over `webview` (a WKWebView, as a pointer). Call on the
    /// main thread.
    pub fn attach(webview: *mut std::ffi::c_void) -> Result<Screen> {
        let Some(mtm) = MainThreadMarker::new() else { bail!("the screen is made on the main thread") };
        // SAFETY: the window's WKWebView, an NSView, alive while the window is.
        let Some(webview) = (unsafe { (webview as *mut NSView).as_ref() }) else { bail!("no page view") };
        // SAFETY: NSView's initialiser, on a freshly allocated view of a subclass that adds nothing to it.
        let cover: Retained<Cover> = unsafe { msg_send![Cover::alloc(mtm), initWithFrame: webview.bounds()] };
        // A layer the view hosts, not one AppKit backs it with: its sublayers
        // are the screen's own to arrange.
        let root = CALayer::new();
        let picture = CALayer::new();
        picture.setHidden(true);
        picture.setBackgroundColor(Some(&CGColor::new_generic_gray(0.0, 1.0)));
        // SAFETY: one of Core Animation's gravity names.
        picture.setContentsGravity(unsafe { kCAGravityResizeAspect });
        root.addSublayer(&picture);
        // Over the picture, in its box: captions and notes.
        let over = CALayer::new();
        over.setHidden(true);
        picture.addSublayer(&over);
        cover.setLayer(Some(&root));
        cover.setWantsLayer(true);
        // Flipped, so boxes measure from the top as the page's do; after
        // AppKit takes the layer, which sets it unflipped.
        root.setGeometryFlipped(true);
        cover.stack(webview, false);
        let webview: Retained<NSView> = webview.retain();
        let inner = Inner {
            picture: Layer(picture),
            over: Layer(over),
            id: None,
            bgrx: Vec::new(),
            painted: Default::default(),
        };
        let cover = Arc::new(MainThreadBound::new((cover, webview), mtm));
        Ok(Screen { cover, under: Arc::default(), inner: Arc::new(Mutex::new(inner)) })
    }

    /// Puts the picture under the page (true) or over it (false, as it
    /// starts), from any thread. Under, it shows only where the page is
    /// clear: what's in the page over the box (a site's player) stays on top.
    pub fn set_under(&self, under: bool) {
        if self.under.swap(under, Ordering::AcqRel) != under {
            self.cover.get_on_main(|(cover, webview)| cover.stack(webview, under));
        }
    }

    /// Shows the picture in `rect` (none: nowhere), from any thread.
    pub fn set(&self, rect: Option<Rect>) {
        let under = self.under.load(Ordering::Acquire);
        self.cover.get_on_main(|(cover, webview)| cover.fill(webview, under));
        let inner = self.inner.lock().unwrap();
        changed(|| match rect {
            Some(r) => {
                let frame = CGRect::new(CGPoint::new(r.x, r.y), CGSize::new(r.width, r.height));
                inner.picture.0.setFrame(frame);
                inner.picture.0.setHidden(false);
            }
            None => inner.picture.0.setHidden(true),
        });
    }

    /// Shows `frame` (none: black), from any thread.
    pub fn show(&self, frame: Option<Arc<Frame>>) {
        let mut inner = self.inner.lock().unwrap();
        let began = std::time::Instant::now();
        let image = frame.as_deref().and_then(|frame| inner.image(frame));
        changed(|| set_contents(&inner.picture.0, image.as_deref()));
        inner.painted.0 += 1;
        inner.painted.1 += began.elapsed();
    }

    /// Shows `overlay` over the picture (none: nothing), from any thread.
    pub fn overlay(&self, overlay: Option<Arc<Overlay>>) {
        let inner = self.inner.lock().unwrap();
        // RGBA in memory, premultiplied.
        let info = CGBitmapInfo(CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0);
        let image = overlay.as_deref().and_then(|o| Some((o.rect, image(o.size, &o.pixels, info)?)));
        changed(|| match image {
            Some((r, image)) => {
                inner.over.0.setFrame(CGRect::new(CGPoint::new(r.x, r.y), CGSize::new(r.width, r.height)));
                set_contents(&inner.over.0, Some(&image));
                inner.over.0.setHidden(false);
            }
            None => {
                inner.over.0.setHidden(true);
                set_contents(&inner.over.0, None);
            }
        });
    }

    /// Paints done, and the time they took in all.
    pub fn painted(&self) -> (u64, std::time::Duration) {
        self.inner.lock().unwrap().painted
    }
}

impl Inner {
    /// `frame` as an image.
    fn image(&mut self, frame: &Frame) -> Option<CFRetained<CGImage>> {
        if self.id != Some(frame.id) {
            self.bgrx.resize(frame.width as usize * frame.height as usize * 4, 0);
            for (to, from) in self.bgrx.as_chunks_mut::<4>().0.iter_mut().zip(frame.rgb.as_chunks::<3>().0) {
                to[0] = from[2];
                to[1] = from[1];
                to[2] = from[0];
            }
            self.id = Some(frame.id);
        }
        // Little-endian 32-bit with the top byte unused: B, G, R, X in memory.
        let info = CGBitmapInfo(CGImageAlphaInfo::NoneSkipFirst.0 | CGImageByteOrderInfo::Order32Little.0);
        image((frame.width, frame.height), &self.bgrx, info)
    }
}

/// `pixels`, `size.0` by `size.1` of 4 bytes laid out as `info` says, as an image.
fn image(size: (u32, u32), pixels: &[u8], info: CGBitmapInfo) -> Option<CFRetained<CGImage>> {
    let data = CFData::from_bytes(pixels);
    let provider = CGDataProvider::with_cf_data(Some(&data))?;
    // SAFETY: a constant name of a color space.
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    // SAFETY: the pixels are `size` of 4 bytes, as said; no decode array.
    unsafe {
        CGImage::new(
            size.0 as usize,
            size.1 as usize,
            8,
            32,
            size.0 as usize * 4,
            Some(&space),
            info,
            Some(&provider),
            std::ptr::null::<CGFloat>(),
            true,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
}

/// A layer's contents: an image, or none.
fn set_contents(layer: &CALayer, image: Option<&CGImage>) {
    // SAFETY: a CGImage is what a layer's contents may be.
    let contents = image.map(|image| unsafe { &*(image as *const CGImage).cast::<AnyObject>() });
    // SAFETY: an image, or none, as a layer's contents may be.
    unsafe { layer.setContents(contents) };
}

/// Changes layers at once and without Core Animation's fades, from any thread.
fn changed(f: impl FnOnce()) {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    f();
    CATransaction::commit();
}
