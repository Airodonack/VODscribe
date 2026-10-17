//! The surface on Windows: a child window of the app's window, over the
//! page's WebView2 control, covering the player's box (`Place::Over` on
//! Linux; there's no other placement here), drawn into with Direct3D
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
    RegisterClassW, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos,
    WINDOW_EX_STYLE, WM_ERASEBKGND, WM_NCDESTROY, WM_NCHITTEST, WM_PAINT, WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS,
    WS_EX_LAYERED, WS_EX_TRANSPARENT, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

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
            unsafe { BeginPaint(hwnd, &mut ps) };
            // What GDI drew is gone where the window was covered: drawn
            // again (unless a paint is under way, which draws it anyway).
            if let Some(inner) = unsafe { screen_of(hwnd) }
                && let Ok(mut inner) = inner.try_lock()
                && matches!(inner.painter, Draw::Gdi(_))
            {
                inner.paint();
            }
            let _ = unsafe { EndPaint(hwnd, &ps) };
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let held = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            if held != 0 {
                // SAFETY: put there by `attach` from `Weak::into_raw`, taken back once.
                drop(unsafe { Weak::from_raw(held as *const Mutex<Inner>) });
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
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
unsafe fn screen_of(hwnd: HWND) -> Option<Arc<Mutex<Inner>>> {
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
        let scale = unsafe { GetDpiForWindow(HWND(inner.parent as *mut _)) } as f64 / 96.0;
        let (x, y, w, h) = match rect {
            Some(r) => (
                (r.x * scale).round() as i32,
                (r.y * scale).round() as i32,
                (r.width * scale).round().max(1.0) as i32,
                (r.height * scale).round().max(1.0) as i32,
            ),
            None => (-1, -1, 1, 1),
        };
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
    Gpu(Box<Painter>),
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
        Ok(Draw::Gpu(Box::new(Painter::new(instance, hwnd, (1, 1)).context("the screen's Direct3D")?)))
    }
}

/// Where `frame` goes in a box of `size` pixels: fitted and centred, as
/// x, y, width, height.
fn fit(frame: &Frame, size: (u32, u32)) -> (f64, f64, f64, f64) {
    let (w, h) = (size.0 as f64, size.1 as f64);
    let fit = (w / frame.width as f64).min(h / frame.height as f64);
    let (fw, fh) = ((frame.width as f64 * fit).round(), (frame.height as f64 * fit).round());
    (((w - fw) / 2.0).round(), ((h - fh) / 2.0).round(), fw, fh)
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

/// A texture drawn in a box, as a quad in clip space: the picture, fitted
/// and centred, opaque (`fs`); the overlay, premultiplied, blended (`fs_over`).
const SHADER: &str = "
struct Box_ { corners: vec4<f32> };
@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var<uniform> box_: Box_;

struct Out { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    let corner = vec2<f32>(f32(i & 1u), f32(i >> 1u));
    var out: Out;
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    out.pos = vec4<f32>(mix(box_.corners.xy, box_.corners.zw, corner), 0.0, 1.0);
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSample(picture, smp, in.uv).rgb, 1.0);
}

@fragment
fn fs_over(in: Out) -> @location(0) vec4<f32> {
    return textureSample(picture, smp, in.uv);
}
";

/// What the surface draws with.
struct Painter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    over_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    picture: Option<Quad>,
    over: Option<Quad>,
    /// RGB to RGBA, kept to reuse.
    rgba: Vec<u8>,
}

/// A texture and the box it's drawn in.
struct Quad {
    texture: wgpu::Texture,
    corners: wgpu::Buffer,
    bind: wgpu::BindGroup,
    size: (u32, u32),
    /// What's in it (`Frame::id`, `Overlay::id`).
    id: u64,
}

impl Painter {
    fn new(instance: wgpu::Instance, hwnd: isize, size: (u32, u32)) -> Result<Painter> {
        use raw_window_handle::{RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle};
        let window = Win32WindowHandle::new(std::num::NonZeroIsize::new(hwnd).context("no window")?);
        let target = wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(RawDisplayHandle::Windows(WindowsDisplayHandle::new())),
            raw_window_handle: RawWindowHandle::Win32(window),
        };
        // SAFETY: the window outlives the surface (both are the screen's).
        let surface = unsafe { instance.create_surface_unsafe(target) }?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
        let mut config = surface.get_default_config(&adapter, size.0.max(1), size.1.max(1)).context("no surface")?;
        config.format = config.format.remove_srgb_suffix();
        surface.configure(&device, &config);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("picture"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("picture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("picture"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |fragment: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fragment),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let (picture_pipeline, over_pipeline) =
            (pipeline("fs", None), pipeline("fs_over", Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)));
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Ok(Painter {
            device,
            queue,
            surface,
            config,
            pipeline: picture_pipeline,
            over_pipeline,
            layout,
            sampler,
            picture: None,
            over: None,
            rgba: Vec::new(),
        })
    }

    /// Draws `frame` fitted in the window of `size` pixels, on black, and
    /// `overlay` over it (its box in CSS pixels, `scale` device pixels each).
    fn paint(&mut self, size: (u32, u32), frame: Option<&Frame>, overlay: Option<(&Overlay, f64)>) {
        let (width, height) = (size.0.max(1), size.1.max(1));
        if (self.config.width, self.config.height) != (width, height) {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => return,
        };
        let view = output.texture.create_view(&Default::default());
        let (w, h) = (width as f64, height as f64);
        // A box in pixels from the top-left, in clip space (rows count from the bottom).
        let clip = |x: f64, y: f64, bw: f64, bh: f64| {
            let c = |v: f64, size: f64| (v / size * 2.0 - 1.0) as f32;
            [c(x, w), -c(y + bh, h), c(x + bw, w), -c(y, h)]
        };
        if let Some(frame) = frame {
            if self.picture.as_ref().is_none_or(|q| q.id != frame.id) {
                // RGB to RGBA: the texture takes four bytes a pixel.
                let pixels = frame.width as usize * frame.height as usize;
                self.rgba.resize(pixels * 4, 255);
                for (to, from) in self.rgba.as_chunks_mut::<4>().0.iter_mut().zip(frame.rgb.as_chunks::<3>().0) {
                    to[..3].copy_from_slice(from);
                }
                let rgba = std::mem::take(&mut self.rgba);
                let quad = self.picture.take();
                self.picture = Some(self.fill(quad, (frame.width, frame.height), &rgba, frame.id));
                self.rgba = rgba;
            }
            let (fx, fy, fw, fh) = fit(frame, (width, height));
            self.queue.write_buffer(&self.picture.as_ref().unwrap().corners, 0, bytemuck_cast(&clip(fx, fy, fw, fh)));
        }
        if let Some((overlay, scale)) = overlay {
            if self.over.as_ref().is_none_or(|q| q.id != overlay.id) {
                let quad = self.over.take();
                self.over = Some(self.fill(quad, overlay.size, &overlay.pixels, overlay.id));
            }
            let r = overlay.rect;
            let corners = clip(r.x * scale, r.y * scale, r.width * scale, r.height * scale);
            self.queue.write_buffer(&self.over.as_ref().unwrap().corners, 0, bytemuck_cast(&corners));
        }
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("picture"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            if let (Some(_), Some(quad)) = (frame, self.picture.as_ref()) {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &quad.bind, &[]);
                pass.draw(0..4, 0..1);
            }
            if let (Some(_), Some(quad)) = (overlay, self.over.as_ref()) {
                pass.set_pipeline(&self.over_pipeline);
                pass.set_bind_group(0, &quad.bind, &[]);
                pass.draw(0..4, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(output);
    }

    /// `quad` (made anew if there's none or its size differs) holding
    /// `rgba`, `size` pixels of four bytes, known as `id`.
    fn fill(&self, quad: Option<Quad>, size: (u32, u32), rgba: &[u8], id: u64) -> Quad {
        let mut quad = match quad {
            Some(quad) if quad.size == size => quad,
            _ => {
                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("quad"),
                    size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let corners = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("box"),
                    size: 16,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let view = texture.create_view(&Default::default());
                let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("quad"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                        wgpu::BindGroupEntry { binding: 2, resource: corners.as_entire_binding() },
                    ],
                });
                Quad { texture, corners, bind, size, id: u64::MAX }
            }
        };
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &quad.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size.0 * 4), rows_per_image: Some(size.1) },
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        );
        quad.id = id;
        quad
    }
}

/// Four floats as bytes, for a uniform buffer.
fn bytemuck_cast(values: &[f32; 4]) -> &[u8] {
    // SAFETY: f32s are plain bytes, and the slice covers exactly them.
    unsafe { std::slice::from_raw_parts(values.as_ptr() as *const u8, 16) }
}
