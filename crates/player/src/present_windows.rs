//! The surface on Windows: a child window of the app's window, over the
//! page's WebView2 control, covering the player's box, drawn into with Direct3D
//! through wgpu. Where Direct3D has only WARP (software: a VM, a remote
//! session, a CI runner), it's drawn with GDI instead: WARP took 160 ms a
//! 720p frame in a VM, a GDI blit a few. VODSCRIBE_DRAW=gpu or gdi picks.
//! The window is layered and transparent to the pointer (`layer`), so it's
//! composed over WebView2 and clicks and the pointer go through to the page.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use anyhow::{Context, Result};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, AlphaBlend, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLACKNESS, BLENDFUNCTION, BeginPaint,
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject,
    EndPaint, GetDC, HALFTONE, HDC, PAINTSTRUCT, PatBlt, ReleaseDC, SRCCOPY, SelectObject, SetBrushOrgEx,
    SetStretchBltMode, StretchDIBits,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GWL_EXSTYLE, GWLP_USERDATA, GetWindowLongPtrW, HTTRANSPARENT, HWND_TOP, LWA_ALPHA,
    RegisterClassW, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, WINDOW_EX_STYLE, WM_ERASEBKGND, WM_NCDESTROY,
    WM_NCHITTEST, WM_PAINT, WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_EX_TRANSPARENT, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

use crate::compose::SurfacePainter;
use crate::decode::Frame;
use crate::{Overlay, Rect};

/// The surface in the app's window, to tell what to show.
#[derive(Clone)]
pub struct Screen {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    /// The child window (an HWND, kept as a number: it's used from any thread).
    hwnd: isize,
    parent: isize,
    frame: Option<Arc<Frame>>,
    overlay: Option<Arc<Overlay>>,
    painter: Draw,
    /// The window's size in pixels, as last set.
    size: (u32, u32),
    /// Paints done, and the time they took in all (for measuring).
    painted: (u64, std::time::Duration),
}

/// The class the surface's windows are of, registered once.
static CLASS: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // The pointer is the page's: Windows gives it to the control below.
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            // SAFETY: Windows' own window, in its WM_PAINT; ended below.
            unsafe { BeginPaint(hwnd, &mut ps) };
            // What GDI drew is gone where the window was covered: drawn
            // again (unless a paint is under way, which draws it anyway).
            // SAFETY: one of the screen's windows (only they have this procedure).
            if let Some(inner) = unsafe { screen_of(hwnd) }
                && let Ok(mut inner) = inner.try_lock()
                && matches!(inner.painter, Draw::Gdi(_))
            {
                inner.paint();
            }
            // SAFETY: the paint begun above.
            let _ = unsafe { EndPaint(hwnd, &ps) };
            LRESULT(0)
        }
        WM_NCDESTROY => {
            // SAFETY: the window's own slot, emptied as it goes.
            let held = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            if held != 0 {
                // SAFETY: put there by `attach` from `Weak::into_raw`, taken back once.
                drop(unsafe { Weak::from_raw(held as *const Mutex<Inner>) });
            }
            // SAFETY: the message as Windows gave it.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        // SAFETY: the message as Windows gave it.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Makes `hwnd` a layered window that the pointer goes through. A plain
/// child's GDI drawing goes to its parent's surface, under WebView2, which
/// draws through DirectComposition; a layered child is composed on its own,
/// in its place among its siblings. And hit-testing as transparent only hands
/// the pointer on to windows of the same thread, while WebView2's take it in
/// another process; a layered window that's also WS_EX_TRANSPARENT is passed
/// over by hit-testing altogether. Windows allows layered children only to a
/// program whose manifest says it's for Windows 8 or later (the app's does);
/// otherwise the window stays plain, which does where nothing is over its
/// parent (the dev binary).
fn layer(hwnd: HWND) {
    // SAFETY: the screen's own window.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | (WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0) as isize);
        if SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA).is_err() {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
        }
    }
}

/// The screen whose window `hwnd` is, if it's still there.
///
/// # Safety
///
/// `hwnd` is a window of the screen's class, whose user data is null or a
/// `Weak` put there by `attach`.
unsafe fn screen_of(hwnd: HWND) -> Option<Arc<Mutex<Inner>>> {
    // SAFETY: reading a window's slot is sound for any window.
    let held = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Mutex<Inner>;
    if held.is_null() {
        return None;
    }
    // SAFETY: a `Weak` kept in the window until WM_NCDESTROY; borrowed, not taken.
    let weak = std::mem::ManuallyDrop::new(unsafe { Weak::from_raw(held) });
    weak.upgrade()
}

fn class() -> Result<PCWSTR> {
    let name = w!("VODscribeScreen");
    if CLASS.load(Ordering::Acquire) == 0 {
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            // SAFETY: the running program's own module, not kept past it.
            hInstance: unsafe { GetModuleHandleW(None)? }.into(),
            lpszClassName: name,
            ..Default::default()
        };
        // SAFETY: a class with a window procedure of the right signature.
        if unsafe { RegisterClassW(&class) } == 0 {
            anyhow::bail!("the screen's window class couldn't be registered");
        }
        CLASS.store(1, Ordering::Release);
    }
    Ok(name)
}

impl Screen {
    /// Puts a surface in the window `parent` (an HWND), a pixel in size
    /// until there's a box to cover. Call on the window's thread.
    pub fn attach(parent: isize) -> Result<Screen> {
        let class = class()?;
        // SAFETY: a child of a window the app owns; the class is registered.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!("VODscribe screen"),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                -1,
                -1,
                1,
                1,
                Some(HWND(parent as *mut _)),
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )
        }?;
        layer(hwnd);
        let painter = Draw::new(hwnd.0 as isize)?;
        let inner = Inner {
            hwnd: hwnd.0 as isize,
            parent,
            frame: None,
            overlay: None,
            painter,
            size: (1, 1),
            painted: Default::default(),
        };
        let inner = Arc::new(Mutex::new(inner));
        // SAFETY: the window's own data, taken back at WM_NCDESTROY.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, Weak::into_raw(Arc::downgrade(&inner)) as isize) };
        Ok(Screen { inner })
    }

    /// Shows the picture in `rect` (none: nowhere), from any thread: the
    /// window moves to that box.
    pub fn set(&self, rect: Option<Rect>) {
        let mut inner = self.inner.lock().unwrap();
        // SAFETY: the app's window, there while the screen is.
        let scale = unsafe { GetDpiForWindow(HWND(inner.parent as *mut _)) } as f64 / 96.0;
        let Some(r) = rect else {
            // Nowhere: hidden, till a box comes.
            // SAFETY: hiding a window the screen made; fine from any thread.
            let _ = unsafe {
                SetWindowPos(
                    HWND(inner.hwnd as *mut _),
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_HIDEWINDOW,
                )
            };
            return;
        };
        let (x, y, w, h) = (
            (r.x * scale).round() as i32,
            (r.y * scale).round() as i32,
            (r.width * scale).round().max(1.0) as i32,
            (r.height * scale).round().max(1.0) as i32,
        );
        // SAFETY: moving a window the screen made; fine from any thread.
        let _ = unsafe {
            SetWindowPos(HWND(inner.hwnd as *mut _), Some(HWND_TOP), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW)
        };
        inner.size = (w as u32, h as u32);
        inner.paint();
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
    pub fn painted(&self) -> (u64, std::time::Duration) {
        self.inner.lock().unwrap().painted
    }
}

impl Inner {
    fn paint(&mut self) {
        let (size, frame, overlay) = (self.size, self.frame.clone(), self.overlay.clone());
        // Device pixels to the page's CSS pixels.
        // SAFETY: the app's window, there while the screen is.
        let scale = unsafe { GetDpiForWindow(HWND(self.parent as *mut _)) } as f64 / 96.0;
        let overlay = overlay.as_deref().map(|o| (o, scale));
        let began = std::time::Instant::now();
        match &mut self.painter {
            Draw::Gpu(painter) => painter.paint(size, frame.as_deref(), overlay),
            Draw::Gdi(gdi) => gdi.paint(self.hwnd, size, frame.as_deref(), overlay),
        }
        self.painted.0 += 1;
        self.painted.1 += began.elapsed();
    }
}

/// How the window is drawn.
enum Draw {
    Gpu(Box<SurfacePainter>),
    Gdi(Gdi),
}

impl Draw {
    fn new(hwnd: isize) -> Result<Draw> {
        let choice = std::env::var("VODSCRIBE_DRAW").unwrap_or_default();
        if choice == "gdi" {
            return Ok(Draw::Gdi(Gdi::default()));
        }
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::DX12;
        let instance = wgpu::Instance::new(desc);
        let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::DX12));
        let hardware = adapters.iter().any(|a| a.get_info().device_type != wgpu::DeviceType::Cpu);
        if !hardware && choice != "gpu" {
            return Ok(Draw::Gdi(Gdi::default()));
        }
        Ok(Draw::Gpu(Box::new(painter(instance, hwnd, (1, 1)).context("the screen's Direct3D")?)))
    }
}

/// Where `frame` goes in a box of `size` pixels: fitted and centred, as
/// x, y, width, height.
fn fit(frame: &Frame, size: (u32, u32)) -> (f64, f64, f64, f64) {
    crate::compose::fit(frame, (size.0 as f64, size.1 as f64))
}

/// Drawing with GDI: the frame stretched from memory into a bitmap the
/// window's size, the overlay blended over it, and the bitmap copied to the
/// window at once (drawn straight onto the window, the overlay would flicker).
#[derive(Default)]
struct Gdi {
    /// The frame as 32-bit BGRX, which a DIB takes, and which frame it is.
    bgrx: Vec<u8>,
    id: Option<u64>,
    /// The overlay as premultiplied BGRA, which AlphaBlend takes, and which it is.
    over: Vec<u8>,
    over_id: Option<u64>,
}

/// A DIB's header for `size` pixels of 32 bits, rows from the top.
fn dib(size: (u32, u32)) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size.0 as i32,
            biHeight: -(size.1 as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

impl Gdi {
    fn paint(&mut self, hwnd: isize, size: (u32, u32), frame: Option<&Frame>, overlay: Option<(&Overlay, f64)>) {
        let hwnd = HWND(hwnd as *mut _);
        let (w, h) = (size.0 as i32, size.1 as i32);
        // SAFETY: the window's own DC, and a memory DC and bitmap made here,
        // all released below.
        unsafe {
            let dc = GetDC(Some(hwnd));
            if dc.is_invalid() {
                return;
            }
            let back = CreateCompatibleDC(Some(dc));
            let bitmap = CreateCompatibleBitmap(dc, w, h);
            let old = SelectObject(back, bitmap.into());
            let _ = PatBlt(back, 0, 0, w, h, BLACKNESS);
            if let Some(frame) = frame {
                self.picture(back, size, frame);
            }
            if let Some((overlay, scale)) = overlay {
                self.overlay(back, overlay, scale);
            }
            let _ = BitBlt(dc, 0, 0, w, h, Some(back), 0, 0, SRCCOPY);
            SelectObject(back, old);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(back);
            ReleaseDC(Some(hwnd), dc);
        }
    }

    /// Stretches `frame` into `dc`, fitted in `size` and centred.
    fn picture(&mut self, dc: HDC, size: (u32, u32), frame: &Frame) {
        if self.id != Some(frame.id) {
            self.bgrx.resize(frame.width as usize * frame.height as usize * 4, 0);
            for (to, from) in self.bgrx.as_chunks_mut::<4>().0.iter_mut().zip(frame.rgb.as_chunks::<3>().0) {
                to[0] = from[2];
                to[1] = from[1];
                to[2] = from[0];
            }
            self.id = Some(frame.id);
        }
        let (x, y, fw, fh) = fit(frame, size);
        let info = dib((frame.width, frame.height));
        // SAFETY: the pixels are `width` by `height` of 4 bytes, as `info` says.
        unsafe {
            SetStretchBltMode(dc, HALFTONE);
            let _ = SetBrushOrgEx(dc, 0, 0, None);
            StretchDIBits(
                dc,
                x as i32,
                y as i32,
                fw as i32,
                fh as i32,
                0,
                0,
                frame.width as i32,
                frame.height as i32,
                Some(self.bgrx.as_ptr() as *const _),
                &info,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
        }
    }

    /// Blends `overlay` over `dc`, its box in CSS pixels of `scale` device pixels.
    fn overlay(&mut self, dc: HDC, overlay: &Overlay, scale: f64) {
        if self.over_id != Some(overlay.id) {
            self.over.clear();
            self.over.extend(overlay.pixels.as_chunks::<4>().0.iter().flat_map(|p| [p[2], p[1], p[0], p[3]]));
            self.over_id = Some(overlay.id);
        }
        let r = overlay.rect;
        let (x, y) = ((r.x * scale).round() as i32, (r.y * scale).round() as i32);
        let (w, h) = ((r.width * scale).round() as i32, (r.height * scale).round() as i32);
        let info = dib(overlay.size);
        // SAFETY: a DIB section the overlay's size, filled from `over` (its
        // size times 4 bytes), selected into a memory DC, all released here.
        unsafe {
            let mut bits = std::ptr::null_mut();
            let Ok(section) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) else { return };
            std::ptr::copy_nonoverlapping(self.over.as_ptr(), bits as *mut u8, self.over.len());
            let src = CreateCompatibleDC(Some(dc));
            let old = SelectObject(src, section.into());
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = AlphaBlend(dc, x, y, w, h, src, 0, 0, overlay.size.0 as i32, overlay.size.1 as i32, blend);
            SelectObject(src, old);
            let _ = DeleteDC(src);
            let _ = DeleteObject(section.into());
        }
    }
}

/// Drawing with Direct3D: the compositor (compose.rs) drawing into the
/// window's surface.
fn painter(instance: wgpu::Instance, hwnd: isize, size: (u32, u32)) -> Result<SurfacePainter> {
    use raw_window_handle::{RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle};
    let window = Win32WindowHandle::new(std::num::NonZeroIsize::new(hwnd).context("no window")?);
    let target = wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: Some(RawDisplayHandle::Windows(WindowsDisplayHandle::new())),
        raw_window_handle: RawWindowHandle::Win32(window),
    };
    // SAFETY: the window outlives the surface (both are the screen's).
    let surface = unsafe { instance.create_surface_unsafe(target) }?;
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
    let device = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
    let mut config = surface.get_default_config(&adapter, size.0.max(1), size.1.max(1)).context("no surface")?;
    config.format = config.format.remove_srgb_suffix();
    Ok(SurfacePainter::new(surface, config, device, adapter.get_info()))
}
