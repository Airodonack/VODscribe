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
use crate::decode::Frame;

/// A texture drawn in a box, as a quad in clip space: the picture, opaque
/// (`fs`); the overlay, premultiplied, blended over it (`fs_over`).
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
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quad"),
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
            label: Some("quad"),
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
        let (picture, over) =
            (pipeline("fs", None), pipeline("fs_over", Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)));
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
            sampler,
            picture: None,
            over: None,
            rgba: Vec::new(),
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
            if self.picture.as_ref().is_none_or(|q| q.id != frame.id) {
                // RGB to RGBA: the texture takes four bytes a pixel.
                let pixels = frame.width as usize * frame.height as usize;
                self.rgba.resize(pixels * 4, 255);
                for (to, from) in self.rgba.as_chunks_mut::<4>().0.iter_mut().zip(frame.rgb.as_chunks::<3>().0) {
                    to[..3].copy_from_slice(from);
                }
                let (rgba, quad) = (std::mem::take(&mut self.rgba), self.picture.take());
                self.picture = Some(self.fill(quad, (frame.width, frame.height), &rgba, frame.id));
                self.rgba = rgba;
            }
            let (fx, fy, fw, fh) = fit(frame, (w, h));
            self.queue.write_buffer(&self.picture.as_ref().unwrap().corners, 0, &bytes(clip(fx, fy, fw, fh)));
        }
        if let Some((overlay, scale)) = overlay {
            if self.over.as_ref().is_none_or(|q| q.id != overlay.id) {
                let quad = self.over.take();
                self.over = Some(self.fill(quad, overlay.size, &overlay.pixels, overlay.id));
            }
            let r = overlay.rect;
            let corners = clip(r.x * scale, r.y * scale, r.width * scale, r.height * scale);
            self.queue.write_buffer(&self.over.as_ref().unwrap().corners, 0, &bytes(corners));
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
            quad.texture.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size.0 * 4), rows_per_image: Some(size.1) },
            extent,
        );
        quad.id = id;
        quad
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

/// Where `frame` goes in a box of `size` pixels: fitted and centred, as
/// x, y, width, height in whole pixels.
pub fn fit(frame: &Frame, size: (f64, f64)) -> (f64, f64, f64, f64) {
    let (w, h) = size;
    let fit = (w / frame.width as f64).min(h / frame.height as f64);
    let (fw, fh) = ((frame.width as f64 * fit).round(), (frame.height as f64 * fit).round());
    (((w - fw) / 2.0).round(), ((h - fh) / 2.0).round(), fw, fh)
}

/// Four floats as bytes, for a uniform buffer.
fn bytes(values: [f32; 4]) -> [u8; 16] {
    let mut out = [0; 16];
    for (to, v) in out.as_chunks_mut::<4>().0.iter_mut().zip(values) {
        *to = v.to_ne_bytes();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rect;

    /// A frame `w` by `h` filled by `color(x, y)`.
    fn frame(id: u64, w: u32, h: u32, color: impl Fn(u32, u32) -> [u8; 3]) -> Frame {
        let rgb = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).flat_map(|(x, y)| color(x, y)).collect();
        Frame { id, pts: 0.0, width: w, height: h, rgb }
    }

    fn at(image: &Image, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * image.width + x) * 4) as usize;
        image.rgba[i..i + 4].try_into().unwrap()
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
    fn a_frame_at_its_own_size_reads_back_as_it_is() {
        let Some(mut c) = compositor() else { return };
        assert_eq!(c.adapter().device_type, wgpu::DeviceType::Cpu, "never a real GPU in tests");
        let f = frame(1, 64, 36, |x, y| [(x * 4) as u8, (y * 7) as u8, ((x + y) * 3) as u8]);
        let image = c.render((64, 36), Some(&f), None).unwrap();
        let want: Vec<u8> = f.rgb.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
        assert_eq!(image.rgba, want, "pixel for pixel");
    }

    #[test]
    fn a_frame_is_fitted_on_black_and_the_overlay_blended_over_it() {
        let Some(mut c) = compositor() else { return };
        // A wide frame in a square box: black bars above and below.
        let f = frame(2, 40, 20, |_, _| [200, 100, 50]);
        let image = c.render((40, 40), Some(&f), None).unwrap();
        assert_eq!(at(&image, 20, 2), [0, 0, 0, 255], "bar above");
        assert_eq!(at(&image, 20, 20), [200, 100, 50, 255], "picture");
        assert_eq!(at(&image, 20, 37), [0, 0, 0, 255], "bar below");
        // Half-transparent white over the top-left quarter (premultiplied).
        let overlay =
            Overlay::new(Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 }, (1, 1), vec![128, 128, 128, 128]);
        let image = c.render((40, 40), Some(&f), Some((&overlay, 2.0))).unwrap();
        let [r, g, b, _] = at(&image, 5, 15);
        assert!(
            (r as i32 - 228).abs() <= 2 && (g as i32 - 178).abs() <= 2 && (b as i32 - 153).abs() <= 2,
            "{r} {g} {b}"
        );
        assert_eq!(at(&image, 30, 15), [200, 100, 50, 255], "outside it, as it was");
    }
}
