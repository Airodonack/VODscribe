//! The compositor: what the player shows, drawn with the GPU (wgpu) into a
//! target the size of the box: the picture fitted and centred on black,
//! then what's drawn over it (captions, notes: `Overlay`). It draws into a
//! window's surface (the screen on Linux and Windows) or into a texture of its own that's
//! read back (`render`): stills, and tests that check frames by their
//! pixels, come from the same drawing as the screen. Effects and tracks will
//! be more passes here.
//!
//! Where there's no GPU (a VM, CI), wgpu's software adapters draw the same
//! (WARP, lavapipe), slowly. `Gpu::Software` asks for one only: tests use it,
//! so they never take a machine's real GPU.

use anyhow::{Context, Result};

use crate::Overlay;
use crate::decode::{self, Frame};

/// Textures drawn in a box, as a quad in clip space: the picture, opaque,
/// from its three planes of YUV turned into RGB by its matrix (`fs_picture`:
/// `decode::rgb_matrix`); the overlay, premultiplied RGBA, blended over it
/// (`fs_over`).
const SHADER: &str = "
struct Out { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

fn quad(i: u32, corners: vec4<f32>) -> Out {
    let corner = vec2<f32>(f32(i & 1u), f32(i >> 1u));
    var out: Out;
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    out.pos = vec4<f32>(mix(corners.xy, corners.zw, corner), 0.0, 1.0);
    return out;
}

struct Picture { corners: vec4<f32>, r: vec4<f32>, g: vec4<f32>, b: vec4<f32> };
@group(0) @binding(0) var luma: texture_2d<f32>;
@group(0) @binding(1) var cb: texture_2d<f32>;
@group(0) @binding(2) var cr: texture_2d<f32>;
@group(0) @binding(3) var planes: sampler;
@group(0) @binding(4) var<uniform> picture: Picture;

@vertex
fn vs_picture(@builtin(vertex_index) i: u32) -> Out {
    return quad(i, picture.corners);
}

@fragment
fn fs_picture(in: Out) -> @location(0) vec4<f32> {
    let yuv = vec4<f32>(
        textureSample(luma, planes, in.uv).r,
        textureSample(cb, planes, in.uv).r,
        textureSample(cr, planes, in.uv).r,
        1.0,
    );
    let rgb = vec3<f32>(dot(picture.r, yuv), dot(picture.g, yuv), dot(picture.b, yuv));
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

struct Box_ { corners: vec4<f32> };
@group(0) @binding(0) var over: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var<uniform> box_: Box_;

@vertex
fn vs_over(@builtin(vertex_index) i: u32) -> Out {
    return quad(i, box_.corners);
}

@fragment
fn fs_over(in: Out) -> @location(0) vec4<f32> {
    return textureSample(over, smp, in.uv);
}
";

/// Which adapter to draw with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gpu {
    /// A real GPU if there's one, else a software one.
    Any,
    /// A software one only (tests, and wherever a real GPU mustn't be used).
    Software,
}

/// An image read back: `width` by `height` pixels of 8-bit RGBA, opaque.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The GPU and what's drawn with it, for one target format.
pub struct Compositor {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    adapter: wgpu::AdapterInfo,
    pipeline: wgpu::RenderPipeline,
    over_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    over_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    picture: Option<Planes>,
    over: Option<Quad>,
}

/// A frame's three planes, as textures of one 8-bit value each, and the
/// uniform with the box it's drawn in and its matrix to RGB.
struct Planes {
    textures: [wgpu::Texture; 3],
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    size: (u32, u32),
    /// What's in it (`Frame::id`).
    id: u64,
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

/// The format `render` draws in: what's read back is RGBA as is.
const OFFSCREEN: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl Compositor {
    /// A compositor of its own, drawing offscreen (`render`).
    pub fn new(gpu: Gpu) -> Result<Compositor> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            force_fallback_adapter: gpu == Gpu::Software,
            ..Default::default()
        }))
        .context("no GPU adapter (nor a software one)")?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
        Ok(Compositor::with_device(device, queue, OFFSCREEN, adapter.get_info()))
    }

    /// A compositor on `device`, drawing into targets of `format`.
    pub fn with_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        adapter: wgpu::AdapterInfo,
    ) -> Compositor {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("compose"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let uniform = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("picture"),
            entries: &[
                texture(0),
                texture(1),
                texture(2),
                sampler(3),
                uniform(4, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let over_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("over"),
            entries: &[texture(0), sampler(1), uniform(2, wgpu::ShaderStages::VERTEX)],
        });
        let pipeline = |name: &str, layout: &wgpu::BindGroupLayout, blend: Option<wgpu::BlendState>| {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(name),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(name),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(&format!("vs_{name}")),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(&format!("fs_{name}")),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
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
        let picture = pipeline("picture", &layout, None);
        let over = pipeline("over", &over_layout, Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING));
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Compositor {
            device,
            queue,
            format,
            adapter,
            pipeline: picture,
            over_pipeline: over,
            layout,
            over_layout,
            sampler,
            picture: None,
            over: None,
        }
    }

    /// The adapter it draws with (its name, and whether it's software).
    pub fn adapter(&self) -> &wgpu::AdapterInfo {
        &self.adapter
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Draws `frame` fitted in `target` (`size` pixels) on black, and
    /// `overlay` over it (its box in CSS pixels, `scale` device pixels each).
    pub fn draw(
        &mut self,
        target: &wgpu::TextureView,
        size: (u32, u32),
        frame: Option<&Frame>,
        overlay: Option<(&Overlay, f64)>,
    ) -> wgpu::CommandBuffer {
        let (w, h) = (size.0.max(1) as f64, size.1.max(1) as f64);
        // A box in pixels from the top-left, in clip space (rows count from the bottom).
        let clip = |x: f64, y: f64, bw: f64, bh: f64| {
            let c = |v: f64, size: f64| (v / size * 2.0 - 1.0) as f32;
            [c(x, w), -c(y + bh, h), c(x + bw, w), -c(y, h)]
        };
        if let Some(frame) = frame {
            if self.picture.as_ref().is_none_or(|p| p.id != frame.id) {
                let planes = self.picture.take();
                self.picture = Some(self.fill_planes(planes, frame));
            }
            let (fx, fy, fw, fh) = fit(frame, (w, h));
            let [r, g, b] = decode::rgb_matrix(frame.colour);
            let uniform = [clip(fx, fy, fw, fh), r, g, b].concat();
            self.queue.write_buffer(&self.picture.as_ref().unwrap().uniform, 0, &bytes(&uniform));
        }
        if let Some((overlay, scale)) = overlay {
            if self.over.as_ref().is_none_or(|q| q.id != overlay.id) {
                let quad = self.over.take();
                self.over = Some(self.fill(quad, overlay.size, &overlay.pixels, overlay.id));
            }
            let r = overlay.rect;
            let corners = clip(r.x * scale, r.y * scale, r.width * scale, r.height * scale);
            self.queue.write_buffer(&self.over.as_ref().unwrap().corners, 0, &bytes(&corners));
        }
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("compose"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            if let (Some(_), Some(planes)) = (frame, self.picture.as_ref()) {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &planes.bind, &[]);
                pass.draw(0..4, 0..1);
            }
            if let (Some(_), Some(quad)) = (overlay, self.over.as_ref()) {
                pass.set_pipeline(&self.over_pipeline);
                pass.set_bind_group(0, &quad.bind, &[]);
                pass.draw(0..4, 0..1);
            }
        }
        encoder.finish()
    }

    /// Draws as `draw` does into an image of its own, `size` pixels, and
    /// reads it back. Only for a compositor made by `new` (its format).
    pub fn render(
        &mut self,
        size: (u32, u32),
        frame: Option<&Frame>,
        overlay: Option<(&Overlay, f64)>,
    ) -> Result<Image> {
        anyhow::ensure!(self.format == OFFSCREEN, "this compositor draws for a window");
        let (width, height) = (size.0.max(1), size.1.max(1));
        let extent = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("render"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OFFSCREEN,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let drawn = self.draw(&target.create_view(&Default::default()), (width, height), frame, overlay);
        // Rows are copied out padded to the copy alignment.
        let row = width * 4;
        let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: padded as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            extent,
        );
        self.queue.submit([drawn, encoder.finish()]);
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        rx.recv()?.context("reading the image back")?;
        let mapped = slice.get_mapped_range()?;
        let mut rgba = Vec::with_capacity((row * height) as usize);
        for line in mapped.chunks(padded as usize) {
            rgba.extend_from_slice(&line[..row as usize]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(Image { width, height, rgba })
    }

    /// `quad` (made anew if there's none or its size differs) holding
    /// `rgba`, `size` pixels of four bytes, known as `id`.
    fn fill(&self, quad: Option<Quad>, size: (u32, u32), rgba: &[u8], id: u64) -> Quad {
        let extent = wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 };
        let mut quad = match quad {
            Some(quad) if quad.size == size => quad,
            _ => {
                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("quad"),
                    size: extent,
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
                    layout: &self.over_layout,
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
            quad.texture.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size.0 * 4), rows_per_image: Some(size.1) },
            extent,
        );
        quad.id = id;
        quad
    }

    /// `planes` (made anew if there are none or their size differs)
    /// holding `frame`'s.
    fn fill_planes(&self, planes: Option<Planes>, frame: &Frame) -> Planes {
        let size = (frame.width, frame.height);
        let (cw, ch) = frame.chroma();
        let extents = [size, (cw, ch), (cw, ch)].map(|(width, height)| wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        });
        let mut planes = match planes {
            Some(planes) if planes.size == size => planes,
            _ => {
                let textures = extents.map(|size| {
                    self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("plane"),
                        size,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::R8Unorm,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    })
                });
                let uniform = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("picture"),
                    size: 64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let views = textures.each_ref().map(|t| t.create_view(&Default::default()));
                let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("picture"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&views[0]) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[1]) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&views[2]) },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                        wgpu::BindGroupEntry { binding: 4, resource: uniform.as_entire_binding() },
                    ],
                });
                Planes { textures, uniform, bind, size, id: u64::MAX }
            }
        };
        let (y, u, v) = frame.planes();
        for ((texture, data), extent) in planes.textures.iter().zip([y, u, v]).zip(extents) {
            self.queue.write_texture(
                texture.as_image_copy(),
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(extent.width),
                    rows_per_image: Some(extent.height),
                },
                extent,
            );
        }
        planes.id = frame.id;
        planes
    }

    /// Submits what `draw` made.
    pub fn submit(&self, drawn: wgpu::CommandBuffer) {
        self.queue.submit([drawn]);
    }

    /// Shows a window surface's texture drawn into.
    pub fn present(&self, output: wgpu::SurfaceTexture) {
        self.queue.present(output);
    }
}

/// The compositor drawing into a window's wgpu surface (Linux's and
/// Windows' screens; each makes the surface and sets it up its own way).
pub struct SurfacePainter {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    compositor: Compositor,
}

impl SurfacePainter {
    /// Draws on `surface` with `device`, configured as `config` says.
    pub fn new(
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
        (device, queue): (wgpu::Device, wgpu::Queue),
        info: wgpu::AdapterInfo,
    ) -> SurfacePainter {
        surface.configure(&device, &config);
        let compositor = Compositor::with_device(device, queue, config.format, info);
        SurfacePainter { surface, config, compositor }
    }

    /// Draws `frame` fitted in the window of `size` pixels, on black, and
    /// `overlay` over it (its box in CSS pixels, `scale` device pixels
    /// each). A surface out of date (the window's display or size changed
    /// under it) is set up again, and the next frame draws.
    pub fn paint(&mut self, size: (u32, u32), frame: Option<&Frame>, overlay: Option<(&Overlay, f64)>) {
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
                return;
            }
            // Hidden, or the screen is busy: the next frame draws.
            _ => return,
        };
        let view = output.texture.create_view(&Default::default());
        let drawn = self.compositor.draw(&view, (width, height), frame, overlay);
        self.compositor.submit(drawn);
        self.compositor.present(output);
    }
}

/// Where `frame` goes in a box of `size` pixels: fitted and centred, as
/// x, y, width, height in whole pixels.
pub fn fit(frame: &Frame, size: (f64, f64)) -> (f64, f64, f64, f64) {
    let (w, h) = size;
    let fit = (w / frame.width as f64).min(h / frame.height as f64);
    let (fw, fh) = ((frame.width as f64 * fit).round(), (frame.height as f64 * fit).round());
    (((w - fw) / 2.0).round(), ((h - fh) / 2.0).round(), fw, fh)
}

/// Floats as bytes, for a uniform buffer.
fn bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_ne_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rect;
    use vodscribe_core::local::{Colour, Matrix};

    const BT709: Colour = Colour { matrix: Matrix::Bt709, full_range: false };
    const JPEG: Colour = Colour { matrix: Matrix::Bt601, full_range: true };

    /// A frame `w` by `h` of luma `luma(x, y)` and of chroma
    /// `chroma(x, y)` for each two by two pixels from (x, y).
    fn frame(
        id: u64,
        (w, h): (u32, u32),
        colour: Colour,
        luma: impl Fn(u32, u32) -> u8,
        chroma: impl Fn(u32, u32) -> [u8; 2],
    ) -> Frame {
        let points = |w: u32, h: u32, k: u32| (0..h).flat_map(move |y| (0..w).map(move |x| (x * k, y * k)));
        let mut yuv: Vec<u8> = points(w, h, 1).map(|(x, y)| luma(x, y)).collect();
        let (cw, ch) = decode::chroma(w, h);
        yuv.extend(points(cw, ch, 2).map(|(x, y)| chroma(x, y)[0]));
        yuv.extend(points(cw, ch, 2).map(|(x, y)| chroma(x, y)[1]));
        Frame { id, pts: 0.0, width: w, height: h, yuv, colour }
    }

    /// A frame of one colour, `yuv`.
    fn flat(id: u64, size: (u32, u32), colour: Colour, [y, u, v]: [u8; 3]) -> Frame {
        frame(id, size, colour, |_, _| y, |_, _| [u, v])
    }

    fn at(image: &Image, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * image.width + x) * 4) as usize;
        image.rgba[i..i + 4].try_into().unwrap()
    }

    fn near(got: [u8; 4], want: [u8; 3], within: i32) -> bool {
        got[3] == 255 && got.iter().zip(want).all(|(&g, w)| (g as i32 - w as i32).abs() <= within)
    }

    /// A software adapter, or None where there isn't one (skipped then,
    /// unless VODSCRIBE_TESTS_NEED_ALL is set, as in CI).
    fn compositor() -> Option<Compositor> {
        match Compositor::new(Gpu::Software) {
            Ok(c) => Some(c),
            Err(e) if std::env::var_os("VODSCRIBE_TESTS_NEED_ALL").is_none() => {
                eprintln!("skipped: {e:#}");
                None
            }
            Err(e) => panic!("{e:#}"),
        }
    }

    #[test]
    fn the_cpu_turns_yuv_into_the_colours_meant() {
        // Each matrix and range's red, green, blue, white and black.
        let cases = [
            (BT709, [63, 102, 240], [255, 0, 0]),
            (BT709, [173, 42, 26], [0, 255, 0]),
            (BT709, [32, 240, 118], [0, 0, 255]),
            (BT709, [235, 128, 128], [255, 255, 255]),
            (BT709, [16, 128, 128], [0, 0, 0]),
            (Colour::UNTAGGED, [81, 90, 240], [255, 0, 0]),
            (Colour::UNTAGGED, [145, 54, 34], [0, 255, 0]),
            (Colour::UNTAGGED, [41, 240, 110], [0, 0, 255]),
            (JPEG, [76, 85, 255], [255, 0, 0]),
            (JPEG, [150, 44, 21], [0, 255, 0]),
            (JPEG, [255, 128, 128], [255, 255, 255]),
            (JPEG, [0, 128, 128], [0, 0, 0]),
        ];
        for (colour, yuv, want) in cases {
            let mut rgb = Vec::new();
            flat(0, (2, 2), colour, yuv).to_rgb(decode::Layout::Rgb, &mut rgb);
            let got = [rgb[0], rgb[1], rgb[2], 255];
            assert!(near(got, want, 2), "{colour:?} {yuv:?}: {got:?}, not {want:?}");
            let mut bgrx = Vec::new();
            flat(0, (2, 2), colour, yuv).to_rgb(decode::Layout::Bgrx, &mut bgrx);
            assert_eq!([bgrx[2], bgrx[1], bgrx[0]], [rgb[0], rgb[1], rgb[2]], "the same, laid out the other way");
        }
    }

    #[test]
    fn a_frame_at_its_own_size_reads_back_as_the_cpu_turns_it() {
        let Some(mut c) = compositor() else { return };
        assert_eq!(c.adapter().device_type, wgpu::DeviceType::Cpu, "never a real GPU in tests");
        // Luma that changes from pixel to pixel; one chroma, as the GPU
        // blends chroma between its samples where the CPU doesn't.
        // An odd size too: its chroma planes are half each side rounded up.
        for (id, size) in [(1, (64, 36)), (5, (33, 17))] {
            let f = frame(id, size, BT709, |x, y| (16 + (x * 3 + y * 5) % 220) as u8, |_, _| [90, 200]);
            let image = c.render(size, Some(&f), None).unwrap();
            let mut rgb = Vec::new();
            f.to_rgb(decode::Layout::Rgb, &mut rgb);
            assert_eq!(image.rgba.len() / 4, rgb.len() / 3);
            for (i, (got, want)) in image.rgba.as_chunks::<4>().0.iter().zip(rgb.as_chunks::<3>().0).enumerate() {
                assert!(near(*got, *want, 1), "{size:?}, pixel {i}: {got:?}, not {want:?}");
            }
        }
    }

    #[test]
    fn the_gpu_turns_yuv_into_the_colours_meant() {
        let Some(mut c) = compositor() else { return };
        // Four quarters: red, green, blue and white in BT.709's limited range.
        let quarters = [[63, 102, 240], [173, 42, 26], [32, 240, 118], [235, 128, 128]];
        let quarter = |x: u32, y: u32| quarters[(x / 16 + y / 16 * 2) as usize];
        let f = frame(3, (32, 32), BT709, |x, y| quarter(x, y)[0], |x, y| [quarter(x, y)[1], quarter(x, y)[2]]);
        let image = c.render((32, 32), Some(&f), None).unwrap();
        for ((x, y), want) in
            [((8, 8), [255, 0, 0]), ((24, 8), [0, 255, 0]), ((8, 24), [0, 0, 255]), ((24, 24), [255; 3])]
        {
            assert!(near(at(&image, x, y), want, 2), "at {x}, {y}: {:?}, not {want:?}", at(&image, x, y));
        }
        // Full range (a JPEG's): red.
        let image = c.render((8, 8), Some(&flat(4, (8, 8), JPEG, [76, 85, 255])), None).unwrap();
        assert!(near(at(&image, 4, 4), [255, 0, 0], 2), "{:?}", at(&image, 4, 4));
    }

    #[test]
    fn a_frame_is_fitted_on_black_and_the_overlay_blended_over_it() {
        let Some(mut c) = compositor() else { return };
        // A wide frame in a square box: black bars above and below.
        // (200, 100, 50) in full-range BT.601.
        let f = flat(2, (40, 20), JPEG, [124, 86, 182]);
        let image = c.render((40, 40), Some(&f), None).unwrap();
        assert_eq!(at(&image, 20, 2), [0, 0, 0, 255], "bar above");
        assert!(near(at(&image, 20, 20), [200, 100, 50], 1), "picture: {:?}", at(&image, 20, 20));
        assert_eq!(at(&image, 20, 37), [0, 0, 0, 255], "bar below");
        // Half-transparent white over the top-left quarter (premultiplied).
        let overlay =
            Overlay::new(Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 }, (1, 1), vec![128, 128, 128, 128]);
        let image = c.render((40, 40), Some(&f), Some((&overlay, 2.0))).unwrap();
        assert!(near(at(&image, 5, 15), [228, 178, 153], 2), "{:?}", at(&image, 5, 15));
        assert!(near(at(&image, 30, 15), [200, 100, 50], 1), "outside it, as it was");
    }
}
