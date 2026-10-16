//! The surface on Windows: a child window of the app's window, over the
//! page's WebView2 control, covering the player's box (`Place::Over` on
//! Linux; there's no other placement here), drawn into with Direct3D
//! through wgpu. Where Direct3D has only WARP (software: a VM, a remote
//! session, a CI runner), it's drawn with GDI instead: WARP took 160 ms a
//! 720p frame in a VM, a GDI blit a few. VODSCRIBE_DRAW=gpu or gdi picks.
//! Clicks and the pointer go through to the page: the window answers
//! hit-tests as transparent, so Windows hands them to the control below.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use anyhow::{Context, Result};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLACKNESS, BeginPaint, DIB_RGB_COLORS, EndPaint, GetDC, HALFTONE,
    PAINTSTRUCT, PatBlt, ReleaseDC, SRCCOPY, SetBrushOrgEx, SetStretchBltMode, StretchDIBits,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GWLP_USERDATA, GetWindowLongPtrW, HTTRANSPARENT, HWND_TOP, RegisterClassW,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos, WINDOW_EX_STYLE, WM_ERASEBKGND, WM_NCDESTROY,
    WM_NCHITTEST, WM_PAINT, WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

use crate::Layout;
use crate::decode::Frame;

/// The surface in the app's window, to tell what to show.
#[derive(Clone)]
pub struct Screen {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    /// The child window (an HWND, kept as a number: it's used from any thread).
    hwnd: isize,
    parent: isize,
    layout: Layout,
    frame: Option<Arc<Frame>>,
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
        let painter = Draw::new(hwnd.0 as isize)?;
        let inner = Inner {
            hwnd: hwnd.0 as isize,
            parent,
            layout: Layout::default(),
            frame: None,
            painter,
            size: (1, 1),
            painted: Default::default(),
        };
        let inner = Arc::new(Mutex::new(inner));
        // SAFETY: the window's own data, taken back at WM_NCDESTROY.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, Weak::into_raw(Arc::downgrade(&inner)) as isize) };
        Ok(Screen { inner })
    }

    /// Shows `layout`, from any thread: the window moves to its box.
    pub fn set(&self, layout: Layout) {
        let mut inner = self.inner.lock().unwrap();
        inner.layout = layout;
        let scale = unsafe { GetDpiForWindow(HWND(inner.parent as *mut _)) } as f64 / 96.0;
        let (x, y, w, h) = match layout.rect {
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

    /// Paints done, and the time they took in all.
    pub fn painted(&self) -> (u64, std::time::Duration) {
        self.inner.lock().unwrap().painted
    }
}

impl Inner {
    fn paint(&mut self) {
        let (size, frame) = (self.size, self.frame.clone());
        let began = std::time::Instant::now();
        match &mut self.painter {
            Draw::Gpu(painter) => painter.paint(size, frame.as_deref()),
            Draw::Gdi(gdi) => gdi.paint(self.hwnd, size, frame.as_deref()),
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

/// Drawing with GDI: the frame blitted from memory, scaled as it goes.
#[derive(Default)]
struct Gdi {
    /// The frame as 32-bit BGRX, which a DIB takes, and which frame it is.
    bgrx: Vec<u8>,
    id: Option<u64>,
}

impl Gdi {
    fn paint(&mut self, hwnd: isize, size: (u32, u32), frame: Option<&Frame>) {
        let hwnd = HWND(hwnd as *mut _);
        let (w, h) = (size.0 as i32, size.1 as i32);
        // SAFETY: the window's own DC, released below.
        let dc = unsafe { GetDC(Some(hwnd)) };
        if dc.is_invalid() {
            return;
        }
        let Some(frame) = frame else {
            let _ = unsafe { PatBlt(dc, 0, 0, w, h, BLACKNESS) };
            unsafe { ReleaseDC(Some(hwnd), dc) };
            return;
        };
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
        let (x, y, fw, fh) = (x as i32, y as i32, fw as i32, fh as i32);
        // The bars around the picture.
        unsafe {
            let _ = PatBlt(dc, 0, 0, w, y, BLACKNESS);
            let _ = PatBlt(dc, 0, y + fh, w, h - y - fh, BLACKNESS);
            let _ = PatBlt(dc, 0, y, x, fh, BLACKNESS);
            let _ = PatBlt(dc, x + fw, y, w - x - fw, fh, BLACKNESS);
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: frame.width as i32,
                // Rows from the top.
                biHeight: -(frame.height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: the pixels are `width` by `height` of 4 bytes, as `info` says.
        unsafe {
            SetStretchBltMode(dc, HALFTONE);
            let _ = SetBrushOrgEx(dc, 0, 0, None);
            StretchDIBits(
                dc,
                x,
                y,
                fw,
                fh,
                0,
                0,
                frame.width as i32,
                frame.height as i32,
                Some(self.bgrx.as_ptr() as *const _),
                &info,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
            ReleaseDC(Some(hwnd), dc);
        }
    }
}

/// The picture, fitted in the box and centred, as a quad in clip space.
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
";

/// What the surface draws with.
struct Painter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    corners: wgpu::Buffer,
    /// The frame's texture, and which frame is in it (`Frame::id`).
    texture: Option<(wgpu::Texture, wgpu::BindGroup, u64, (u32, u32))>,
    /// RGB to RGBA, kept to reuse.
    rgba: Vec<u8>,
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
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("picture"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(config.format.into())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let corners = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("box"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Painter {
            device,
            queue,
            surface,
            config,
            pipeline,
            layout,
            sampler,
            corners,
            texture: None,
            rgba: Vec::new(),
        })
    }

    /// Draws `frame` fitted in the window of `size` pixels, on black.
    fn paint(&mut self, size: (u32, u32), frame: Option<&Frame>) {
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
        if let Some(frame) = frame {
            self.upload(frame);
            let (w, h) = (width as f64, height as f64);
            let (fx, fy, fw, fh) = fit(frame, (width, height));
            let clip = |v: f64, size: f64| (v / size * 2.0 - 1.0) as f32;
            // Clip space counts rows from the bottom.
            let corners = [clip(fx, w), -clip(fy + fh, h), clip(fx + fw, w), -clip(fy, h)];
            self.queue.write_buffer(&self.corners, 0, bytemuck_cast(&corners));
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
            if let (Some(_), Some((_, bind, _, _))) = (frame, self.texture.as_ref()) {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, bind, &[]);
                pass.draw(0..4, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(output);
    }

    /// Puts `frame` in the texture, unless it's there.
    fn upload(&mut self, frame: &Frame) {
        let size = (frame.width, frame.height);
        if let Some((_, _, id, _)) = &self.texture
            && *id == frame.id
        {
            return;
        }
        if self.texture.as_ref().is_none_or(|t| t.3 != size) {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("frame"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("picture"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: self.corners.as_entire_binding() },
                ],
            });
            self.texture = Some((texture, bind, u64::MAX, size));
        }
        // RGB to RGBA: the texture takes four bytes a pixel.
        let pixels = size.0 as usize * size.1 as usize;
        self.rgba.resize(pixels * 4, 255);
        for (to, from) in self.rgba.as_chunks_mut::<4>().0.iter_mut().zip(frame.rgb.as_chunks::<3>().0) {
            to[..3].copy_from_slice(from);
        }
        let (texture, _, id, _) = self.texture.as_mut().unwrap();
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size.0 * 4), rows_per_image: Some(size.1) },
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        );
        *id = frame.id;
    }
}

/// Four floats as bytes, for a uniform buffer.
fn bytemuck_cast(values: &[f32; 4]) -> &[u8] {
    // SAFETY: f32s are plain bytes, and the slice covers exactly them.
    unsafe { std::slice::from_raw_parts(values.as_ptr() as *const u8, 16) }
}
