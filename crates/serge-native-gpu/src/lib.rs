//! `serge-native-gpu` — draws a [`ScenePrimitive`] list with `wgpu`
//! (pinned, `docs/02-architecture.md` §3): one instance buffer, one
//! pipeline and one draw call per frame (`docs/03-adapters-and-pipeline.md`
//! §1.2). Rectangles and borders are anti-aliased in the shader; glyphs and
//! paths are coverage masks from the CPU path's rasterizer
//! ([`MaskRasterizer`]) in texture atlases, so they match the CPU frame.
//!
//! [`GpuRenderer::render`] draws offscreen and reads the frame back (tests,
//! parity checks); [`GpuSurface`] presents to a window.

#[cfg(all(feature = "camera", target_vendor = "apple"))]
pub mod camera;
#[cfg(all(feature = "native-metal", target_vendor = "apple"))]
mod metal;
pub mod viewport;
use serge_core::viewport::{Slot as ViewSlot, TextureKey};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serge_core::{Rect, ScenePrimitive};
use serge_native_cpu::{Frame, Mask, MaskRasterizer};
use serge_native_text::Font;
use serge_style::Color;

/// The `wgpu` this renderer is built on.
pub use wgpu;

const SHADER: &str = include_str!("shader.wgsl");

const FILL: f32 = 0.0;
const BORDER: f32 = 1.0;
const GLYPH: f32 = 2.0;
const COLOR_GLYPH: f32 = 3.0;
const PATH: f32 = 4.0;

/// Floats per instance: rect, color, clip, extra.
const INSTANCE_FLOATS: usize = 16;
const INSTANCE_BYTES: u64 = (INSTANCE_FLOATS * 4) as u64;
/// Side of the glyph atlas.
const GLYPH_ATLAS: u32 = 2048;

/// Why the GPU path could not start or draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuError(pub String);

impl std::fmt::Display for GpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GpuError {}

fn err(e: impl std::fmt::Display) -> GpuError {
    GpuError(e.to_string())
}

/// What the last frame took.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Composite draw calls, split at external texture boundaries.
    pub draw_calls: u32,
    /// Instances drawn.
    pub instances: u32,
    /// Glyph masks rasterized (cache misses).
    pub glyphs_rasterized: u32,
    /// Path masks rasterized.
    pub paths: u32,
}

/// A shelf packer.
#[derive(Clone, Copy, Debug)]
struct Shelf {
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    row: u32,
}

impl Shelf {
    fn new(width: u32, height: u32) -> Shelf {
        Shelf {
            width,
            height,
            x: 0,
            y: 0,
            row: 0,
        }
    }

    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w > self.width {
            return None;
        }
        if self.x + w > self.width {
            self.y += self.row;
            self.x = 0;
            self.row = 0;
        }
        if self.y + h > self.height {
            return None;
        }
        let at = (self.x, self.y);
        // One pixel of padding keeps neighbors apart.
        self.x += w + 1;
        self.row = self.row.max(h + 1);
        Some(at)
    }

    fn used_height(&self) -> u32 {
        self.y + self.row
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct GlyphKey {
    font: u64,
    index: u32,
    glyph: u32,
    size: u32,
    scale: u32,
    /// The fractional device position in 1/64 px.
    sub_x: u8,
    sub_y: u8,
}

/// Glyph positions are quantized to 1/64 px: `vello_cpu` draws glyphs at
/// their exact position, and a cached mask is reused only within 1/128 px.
const SUBPIXEL: f64 = 64.0;

fn quantize(v: f64) -> (f64, u8) {
    let q = (v * SUBPIXEL).round() / SUBPIXEL;
    (q, ((q - q.floor()) * SUBPIXEL).round() as u8)
}

/// A glyph in the atlas, relative to the floor of its device origin.
#[derive(Clone, Copy, Debug)]
struct Slot {
    atlas_x: u32,
    atlas_y: u32,
    width: u32,
    height: u32,
    dx: i32,
    dy: i32,
    color: bool,
}

/// Something went wrong on the device after it was created.
type DeviceErrors = Arc<Mutex<Option<String>>>;

/// Draws scenes on a GPU device.
pub struct GpuRenderer {
    #[cfg(all(feature = "native-metal", target_vendor = "apple"))]
    metal: Option<metal::MetalBatch>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter: String,
    external: HashMap<TextureKey, wgpu::Texture>,
    shader: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    globals: wgpu::Buffer,
    instances: wgpu::Buffer,
    glyph_atlas: wgpu::Texture,
    glyph_shelf: Shelf,
    glyphs: HashMap<GlyphKey, Option<Slot>>,
    path_atlas: wgpu::Texture,
    raster: MaskRasterizer,
    offscreen: Option<wgpu::Texture>,
    stats: Stats,
    errors: DeviceErrors,
    warned: bool,
}

impl std::fmt::Debug for GpuRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuRenderer")
            .field("adapter", &self.adapter)
            .finish_non_exhaustive()
    }
}

/// Runs a future to completion on this thread (device setup and readback).
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    use std::task::{Context, Poll, Wake, Waker};
    struct Unpark(std::thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park();
    }
}

fn instance() -> wgpu::Instance {
    // `WGPU_BACKEND` and friends can narrow the backends.
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
}

fn texture(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

fn push_f32s(out: &mut Vec<u8>, v: [f32; INSTANCE_FLOATS]) {
    for f in v {
        out.extend_from_slice(&f.to_ne_bytes());
    }
}

fn rgba(c: Color) -> [f32; 4] {
    [
        f32::from(c.r) / 255.0,
        f32::from(c.g) / 255.0,
        f32::from(c.b) / 255.0,
        f32::from(c.a) / 255.0,
    ]
}

fn scaled(r: Rect, s: f32) -> [f32; 4] {
    [r.x * s, r.y * s, r.width * s, r.height * s]
}

/// A path mask placed in this frame's path atlas.
struct Placed {
    mask: Mask,
    at: (u32, u32),
}

/// One frame's instances and path masks.
struct Encoded {
    instances: Vec<u8>,
    paths: Vec<Placed>,
    external: Vec<(u32, TextureKey)>,
    shelf: Shelf,
}

impl Encoded {
    fn new(path_width: u32, max_side: u32) -> Encoded {
        Encoded {
            instances: Vec::new(),
            paths: Vec::new(),
            external: Vec::new(),
            shelf: Shelf::new(path_width, max_side),
        }
    }
}

/// Where a frame goes.
#[derive(Clone, Copy)]
struct Target<'a> {
    view: &'a wgpu::TextureView,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
}

impl GpuRenderer {
    /// A renderer on the default adapter, for offscreen frames.
    pub fn headless() -> Result<GpuRenderer, GpuError> {
        GpuRenderer::headless_on(instance())
    }

    /// A renderer on `backends` only (tests: a backend that is not built in
    /// makes device creation fail on any machine).
    #[doc(hidden)]
    pub fn headless_with(backends: wgpu::Backends) -> Result<GpuRenderer, GpuError> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = backends;
        GpuRenderer::headless_on(wgpu::Instance::new(desc))
    }

    fn headless_on(instance: wgpu::Instance) -> Result<GpuRenderer, GpuError> {
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .map_err(|e| GpuError(format!("no GPU adapter: {e}")))?;
        GpuRenderer::new(&adapter)
    }

    fn new(adapter: &wgpu::Adapter) -> Result<GpuRenderer, GpuError> {
        let info = adapter.get_info();
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("serge"),
            ..Default::default()
        }))
        .map_err(|e| GpuError(format!("no GPU device: {e}")))?;
        let errors: DeviceErrors = Arc::default();
        let sink = errors.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            if let Ok(mut slot) = sink.lock()
                && slot.is_none()
            {
                *slot = Some(e.to_string());
            }
        }));
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("serge"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("serge"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex(1),
                tex(2),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("serge"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("serge globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("serge instances"),
            size: INSTANCE_BYTES * 1024,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let atlas_usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        let glyph_atlas = texture(
            &device,
            "serge glyphs",
            GLYPH_ATLAS,
            GLYPH_ATLAS,
            wgpu::TextureFormat::Rgba8Unorm,
            atlas_usage,
        );
        let path_atlas = texture(
            &device,
            "serge paths",
            256,
            256,
            wgpu::TextureFormat::R8Unorm,
            atlas_usage,
        );
        Ok(GpuRenderer {
            #[cfg(all(feature = "native-metal", target_vendor = "apple"))]
            metal: metal::MetalBatch::new(&device)?,
            device,
            queue,
            external: HashMap::new(),
            adapter: format!("{} ({:?})", info.name, info.backend),
            shader,
            layout,
            pipeline_layout,
            pipelines: HashMap::new(),
            globals,
            instances,
            glyph_atlas,
            glyph_shelf: Shelf::new(GLYPH_ATLAS, GLYPH_ATLAS),
            glyphs: HashMap::new(),
            path_atlas,
            raster: MaskRasterizer::new(),
            offscreen: None,
            stats: Stats::default(),
            errors,
            warned: false,
        })
    }

    /// The adapter's name and backend.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// What the last frame took.
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Whether this renderer emits native Metal batches on the selected Apple device.
    #[cfg(all(feature = "native-metal", target_vendor = "apple"))]
    pub fn uses_native_metal(&self) -> bool {
        self.metal.is_some()
    }

    fn pipeline(&mut self, format: wgpu::TextureFormat) -> &wgpu::RenderPipeline {
        let (device, shader, layout) = (&self.device, &self.shader, &self.pipeline_layout);
        self.pipelines.entry(format).or_insert_with(|| {
            let attrs = wgpu::vertex_attr_array![
                0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4
            ];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("serge"),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: INSTANCE_BYTES,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &attrs,
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    /// The glyph's atlas slot, rasterizing it on a miss. `Err` when the
    /// atlas is full.
    fn glyph_slot(
        &mut self,
        font: &Font,
        size: f32,
        id: u32,
        scale: f32,
        x: f64,
        y: f64,
    ) -> Result<Option<Slot>, ()> {
        let key = GlyphKey {
            font: font.data.id(),
            index: font.index,
            glyph: id,
            size: size.to_bits(),
            scale: scale.to_bits(),
            sub_x: quantize(x).1,
            sub_y: quantize(y).1,
        };
        if let Some(slot) = self.glyphs.get(&key) {
            return Ok(*slot);
        }
        self.stats.glyphs_rasterized += 1;
        let Some(mask) = self.raster.glyph(font, size, id, scale, x, y) else {
            self.glyphs.insert(key, None);
            return Ok(None);
        };
        let (w, h) = (u32::from(mask.width), u32::from(mask.height));
        let (ax, ay) = self.glyph_shelf.alloc(w, h).ok_or(())?;
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.glyph_atlas,
                mip_level: 0,
                origin: wgpu::Origin3d { x: ax, y: ay, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &mask.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let slot = Slot {
            atlas_x: ax,
            atlas_y: ay,
            width: w,
            height: h,
            dx: mask.x - x.floor() as i32,
            dy: mask.y - y.floor() as i32,
            color: mask.has_color(),
        };
        self.glyphs.insert(key, Some(slot));
        Ok(Some(slot))
    }

    /// The scene as instances, plus this frame's path masks. `Err` when the
    /// glyph atlas filled up mid-frame.
    fn encode(
        &mut self,
        scene: &[ScenePrimitive],
        (width, height, scale): (u32, u32, f32),
        frame: &mut Encoded,
    ) -> Result<(), ()> {
        let Encoded {
            instances: out,
            paths,
            shelf: path_shelf,
            external,
        } = frame;
        let full = [0.0, 0.0, width as f32, height as f32];
        let mut clips = vec![full];
        for p in scene {
            let clip = *clips.last().unwrap_or(&full);
            match p {
                ScenePrimitive::ExternalTexture { key, rect } => {
                    let Some(slot) = ViewSlot::resolve(*key) else {
                        continue;
                    };
                    let [x, y, w, h] = scaled(*rect, scale);
                    if w <= 0.0 || h <= 0.0 {
                        continue;
                    }
                    let (kind, color) = if self.external.contains_key(key) {
                        external.push(((out.len() as u64 / INSTANCE_BYTES) as u32, *key));
                        (5.0, [1.0; 4])
                    } else {
                        (FILL, rgba(slot.fallback()))
                    };
                    let [r, g, b, a] = color;
                    push_f32s(
                        out,
                        [
                            x, y, w, h, r, g, b, a, clip[0], clip[1], clip[2], clip[3], 0.0, 0.0,
                            kind, 0.0,
                        ],
                    );
                }
                ScenePrimitive::Quad {
                    rect,
                    color,
                    radius,
                } => {
                    if color.a == 0 {
                        continue;
                    }
                    let [r, g, b, a] = rgba(*color);
                    let [x, y, w, h] = scaled(*rect, scale);
                    push_f32s(
                        out,
                        [
                            x,
                            y,
                            w,
                            h,
                            r,
                            g,
                            b,
                            a,
                            clip[0],
                            clip[1],
                            clip[2],
                            clip[3],
                            radius * scale,
                            0.0,
                            FILL,
                            0.0,
                        ],
                    );
                }
                ScenePrimitive::Border {
                    rect,
                    color,
                    width: bw,
                    radius,
                } => {
                    if color.a == 0 || *bw <= 0.0 {
                        continue;
                    }
                    let [r, g, b, a] = rgba(*color);
                    let [x, y, w, h] = scaled(*rect, scale);
                    push_f32s(
                        out,
                        [
                            x,
                            y,
                            w,
                            h,
                            r,
                            g,
                            b,
                            a,
                            clip[0],
                            clip[1],
                            clip[2],
                            clip[3],
                            radius * scale,
                            bw * scale,
                            BORDER,
                            0.0,
                        ],
                    );
                }
                ScenePrimitive::Line { color, .. } | ScenePrimitive::Polygon { color, .. } => {
                    if color.a == 0 {
                        continue;
                    }
                    let Some(mask) = self.raster.path(p, scale) else {
                        continue;
                    };
                    self.stats.paths += 1;
                    let (w, h) = (u32::from(mask.width), u32::from(mask.height));
                    let Some(at) = path_shelf.alloc(w, h) else {
                        if !self.warned {
                            eprintln!(
                                "serge: a frame's paths do not fit the GPU path atlas; some are not drawn"
                            );
                            self.warned = true;
                        }
                        continue;
                    };
                    let [r, g, b, a] = rgba(*color);
                    push_f32s(
                        out,
                        [
                            mask.x as f32,
                            mask.y as f32,
                            w as f32,
                            h as f32,
                            r,
                            g,
                            b,
                            a,
                            clip[0],
                            clip[1],
                            clip[2],
                            clip[3],
                            at.0 as f32,
                            at.1 as f32,
                            PATH,
                            0.0,
                        ],
                    );
                    paths.push(Placed { mask, at });
                }
                ScenePrimitive::Glyphs {
                    origin,
                    font,
                    size,
                    color,
                    glyphs,
                } => {
                    let Some(font) = font.0.downcast_ref::<Font>() else {
                        continue;
                    };
                    if color.a == 0 {
                        continue;
                    }
                    let [r, g, b, a] = rgba(*color);
                    let s = f64::from(scale);
                    for gl in glyphs {
                        let x = quantize(s * (f64::from(origin.x) + f64::from(gl.x))).0;
                        let y = quantize(s * (f64::from(origin.y) + f64::from(gl.y))).0;
                        let Some(slot) = self.glyph_slot(font, *size, gl.id, scale, x, y)? else {
                            continue;
                        };
                        let (gx, gy) = (x.floor() as i32 + slot.dx, y.floor() as i32 + slot.dy);
                        push_f32s(
                            out,
                            [
                                gx as f32,
                                gy as f32,
                                slot.width as f32,
                                slot.height as f32,
                                r,
                                g,
                                b,
                                a,
                                clip[0],
                                clip[1],
                                clip[2],
                                clip[3],
                                slot.atlas_x as f32,
                                slot.atlas_y as f32,
                                if slot.color { COLOR_GLYPH } else { GLYPH },
                                0.0,
                            ],
                        );
                    }
                }
                ScenePrimitive::PushClip(r) => {
                    let [x, y, w, h] = scaled(*r, scale);
                    clips.push([
                        clip[0].max(x),
                        clip[1].max(y),
                        clip[2].min(x + w),
                        clip[3].min(y + h),
                    ]);
                }
                ScenePrimitive::PopClip => {
                    if clips.len() > 1 {
                        clips.pop();
                    }
                }
            }
        }
        Ok(())
    }

    /// Draws `scene` into `target` (`format`, `width` × `height`).
    fn draw(
        &mut self,
        scene: &[ScenePrimitive],
        target: Target<'_>,
        scale: f32,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<Vec<wgpu::CommandBuffer>, GpuError> {
        #[cfg(all(feature = "native-metal", target_vendor = "apple"))]
        let metal_target = target;
        let Target {
            view: target,
            format,
            width,
            height,
        } = target;
        self.stats = Stats::default();
        let max_side = self.device.limits().max_texture_dimension_2d;
        let path_width = 2048.min(max_side);
        viewport::prepare(
            &mut self.external,
            &self.device,
            &self.queue,
            encoder,
            scene,
        );
        let mut frame = Encoded::new(path_width, max_side);
        if self
            .encode(scene, (width, height, scale), &mut frame)
            .is_err()
        {
            // The glyph atlas is full: start it over and encode again.
            self.glyphs.clear();
            self.glyph_shelf = Shelf::new(GLYPH_ATLAS, GLYPH_ATLAS);
            frame = Encoded::new(path_width, max_side);
            if self
                .encode(scene, (width, height, scale), &mut frame)
                .is_err()
                && !self.warned
            {
                eprintln!(
                    "serge: a frame's glyphs do not fit the GPU glyph atlas; some are not drawn"
                );
                self.warned = true;
            }
        }
        let Encoded {
            instances: out,
            paths,
            shelf: path_shelf,
            external,
        } = frame;
        // This frame's path masks.
        let need_h = path_shelf.used_height().max(1);
        let size = self.path_atlas.size();
        if size.width < path_width || size.height < need_h {
            let h = need_h.next_power_of_two().clamp(256, max_side);
            self.path_atlas = texture(
                &self.device,
                "serge paths",
                path_width,
                h,
                wgpu::TextureFormat::R8Unorm,
                wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            );
        }
        for Placed { mask, at } in &paths {
            let alpha: Vec<u8> = mask.rgba.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.path_atlas,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: at.0,
                        y: at.1,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &alpha,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(u32::from(mask.width)),
                    rows_per_image: Some(u32::from(mask.height)),
                },
                wgpu::Extent3d {
                    width: u32::from(mask.width),
                    height: u32::from(mask.height),
                    depth_or_array_layers: 1,
                },
            );
        }
        let count = (out.len() as u64 / INSTANCE_BYTES) as u32;
        #[cfg(all(feature = "native-metal", target_vendor = "apple"))]
        if let Some(metal) = &mut self.metal {
            let scene = metal::Scene {
                instances: &out,
                glyphs: &self.glyph_atlas,
                paths: &self.path_atlas,
                viewports: &external,
                textures: &self.external,
            };
            let (batch, calls) = metal.encode(&self.device, encoder, metal_target, scene)?;
            self.stats.instances = count;
            self.stats.draw_calls = calls;
            let readback = self.device.create_command_encoder(&Default::default());
            let prepared = std::mem::replace(encoder, readback);
            return Ok(vec![prepared.finish(), batch]);
        }
        if (out.len() as u64) > self.instances.size() {
            self.instances = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("serge instances"),
                size: (out.len() as u64).next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !out.is_empty() {
            self.queue.write_buffer(&self.instances, 0, &out);
        }
        let mut globals = Vec::with_capacity(16);
        for f in [width as f32, height as f32, 0.0, 0.0] {
            globals.extend_from_slice(&f.to_ne_bytes());
        }
        self.queue.write_buffer(&self.globals, 0, &globals);
        let glyph_view = self.glyph_atlas.create_view(&Default::default());
        let path_view = self.path_atlas.create_view(&Default::default());
        let group = |image: &wgpu::TextureView| {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("serge"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.globals.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(image),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&path_view),
                    },
                ],
            })
        };
        let bind = group(&glyph_view);
        let viewport_groups: Vec<_> = external
            .into_iter()
            .map(|(index, key)| {
                let image = self.external[&key].create_view(&Default::default());
                (index, group(&image))
            })
            .collect();
        let pipeline = self.pipeline(format).clone();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("serge"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        if count > 0 {
            pass.set_pipeline(&pipeline);
            pass.set_vertex_buffer(0, self.instances.slice(..));
            let mut start = 0;
            for (index, group) in &viewport_groups {
                if start < *index {
                    pass.set_bind_group(0, &bind, &[]);
                    pass.draw(0..4, start..*index);
                    self.stats.draw_calls += 1;
                }
                pass.set_bind_group(0, group, &[]);
                pass.draw(0..4, *index..*index + 1);
                self.stats.draw_calls += 1;
                start = *index + 1;
            }
            if start < count {
                pass.set_bind_group(0, &bind, &[]);
                pass.draw(0..4, start..count);
                self.stats.draw_calls += 1;
            }
        }
        self.stats.instances = count;
        Ok(Vec::new())
    }

    fn check(&self) -> Result<(), GpuError> {
        match self.errors.lock().ok().and_then(|mut e| e.take()) {
            Some(e) => Err(GpuError(e)),
            None => Ok(()),
        }
    }

    /// Draws `scene` (logical pixels) at `scale` into a `width` × `height`
    /// offscreen frame and reads it back: premultiplied RGBA, like
    /// [`serge_native_cpu::CpuRenderer::render`].
    pub fn render(
        &mut self,
        scene: &[ScenePrimitive],
        width: u16,
        height: u16,
        scale: f32,
    ) -> Result<Frame, GpuError> {
        let (w, h) = (u32::from(width.max(1)), u32::from(height.max(1)));
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let reuse = self
            .offscreen
            .as_ref()
            .is_some_and(|t| t.width() == w && t.height() == h);
        if !reuse {
            self.offscreen = Some(texture(
                &self.device,
                "serge offscreen",
                w,
                h,
                format,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            ));
        }
        let target = self.offscreen.clone().expect("created above");
        let view = target.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let to = Target {
            view: &view,
            format,
            width: w,
            height: h,
        };
        let batches = self.draw(scene, to, scale, &mut encoder)?;
        let rgba = self.read_back(&target, batches, encoder, false)?;
        Ok(Frame {
            width: w as u16,
            height: h as u16,
            rgba,
        })
    }

    /// Copies `texture` out (after the commands in `encoder`), as RGBA.
    fn read_back(
        &mut self,
        texture: &wgpu::Texture,
        batches: Vec<wgpu::CommandBuffer>,
        mut encoder: wgpu::CommandEncoder,
        bgra: bool,
    ) -> Result<Vec<u8>, GpuError> {
        let (w, h) = (texture.width(), texture.height());
        let row = w * 4;
        let padded =
            row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("serge readback"),
            size: u64::from(padded) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue
            .submit(batches.into_iter().chain([encoder.finish()]));
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(err)?;
        rx.recv().map_err(err)?.map_err(err)?;
        self.check()?;
        let data = slice.get_mapped_range().map_err(err)?;
        let mut out = Vec::with_capacity((row * h) as usize);
        for y in 0..h as usize {
            let line = &data[y * padded as usize..y * padded as usize + row as usize];
            if bgra {
                for p in line.as_chunks::<4>().0 {
                    out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
                }
            } else {
                out.extend_from_slice(line);
            }
        }
        drop(data);
        buffer.unmap();
        Ok(out)
    }
}

/// A window's GPU surface and renderer.
pub struct GpuSurface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    view_format: wgpu::TextureFormat,
    renderer: GpuRenderer,
    readable: bool,
}

impl std::fmt::Debug for GpuSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuSurface")
            .field("adapter", &self.renderer.adapter)
            .field("format", &self.view_format)
            .finish_non_exhaustive()
    }
}

impl GpuSurface {
    /// A surface on `target` (a window) sized `width` × `height` physical
    /// pixels, on an adapter that can present to it.
    pub fn new(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Result<GpuSurface, GpuError> {
        Self::with_software_adapter(target, width, height, true)
    }

    /// A window surface with an explicit policy for software Vulkan adapters.
    pub fn with_software_adapter(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
        allow_software: bool,
    ) -> Result<GpuSurface, GpuError> {
        let instance = instance();
        let surface = instance
            .create_surface(target)
            .map_err(|e| GpuError(format!("no GPU surface for the window: {e}")))?;
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| GpuError(format!("no GPU adapter for the window: {e}")))?;
        if !allow_software && adapter.get_info().device_type == wgpu::DeviceType::Cpu {
            return Err(GpuError("platform only offers a software GPU".into()));
        }
        let caps = surface.get_capabilities(&adapter);
        // Blend in sRGB-encoded values like the CPU path: a non-sRGB format,
        // or a non-sRGB view of an sRGB one.
        let plain = |f: &wgpu::TextureFormat| {
            matches!(
                f,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
            )
        };
        let (format, view_format) = match caps.formats.iter().find(|f| plain(f)) {
            Some(f) => (*f, *f),
            None => {
                let f = caps
                    .formats
                    .iter()
                    .find(|f| plain(&f.remove_srgb_suffix()))
                    .ok_or_else(|| {
                        GpuError(format!(
                            "no 8-bit RGBA surface format in {:?}",
                            caps.formats
                        ))
                    })?;
                (*f, f.remove_srgb_suffix())
            }
        };
        let readable = caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        if readable {
            usage |= wgpu::TextureUsages::COPY_SRC;
        }
        let config = wgpu::SurfaceConfiguration {
            usage,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: if view_format == format {
                vec![]
            } else {
                vec![view_format]
            },
        };
        let renderer = GpuRenderer::new(&adapter)?;
        surface.configure(&renderer.device, &config);
        renderer.check()?;
        Ok(GpuSurface {
            surface,
            config,
            view_format,
            renderer,
            readable,
        })
    }

    /// The adapter's name and backend.
    pub fn adapter(&self) -> &str {
        self.renderer.adapter()
    }

    /// What the last frame took.
    pub fn stats(&self) -> Stats {
        self.renderer.stats()
    }

    /// Draws and presents `scene` at `scale` on a surface of `width` ×
    /// `height` physical pixels. With `read_back`, also returns the
    /// presented pixels (when the surface allows copying out).
    pub fn render(
        &mut self,
        scene: &[ScenePrimitive],
        width: u32,
        height: u32,
        scale: f32,
        read_back: bool,
    ) -> Result<Option<Frame>, GpuError> {
        let (w, h) = (width.max(1), height.max(1));
        if (self.config.width, self.config.height) != (w, h) {
            self.config.width = w;
            self.config.height = h;
            self.surface.configure(&self.renderer.device, &self.config);
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                // Present this one; configure again for the next frame.
                self.config.width = 0;
                t
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(None);
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.renderer.device, &self.config);
                return Ok(None);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                self.renderer.check()?;
                return Err(GpuError("the surface texture failed validation".into()));
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.view_format),
            ..Default::default()
        });
        let mut encoder = self
            .renderer
            .device
            .create_command_encoder(&Default::default());
        let batches = self.renderer.draw(
            scene,
            Target {
                view: &view,
                format: self.view_format,
                width: w,
                height: h,
            },
            scale,
            &mut encoder,
        )?;
        let pixels = if read_back && self.readable {
            let bgra = matches!(self.view_format, wgpu::TextureFormat::Bgra8Unorm);
            let rgba = self
                .renderer
                .read_back(&frame.texture, batches, encoder, bgra)?;
            Some(Frame {
                width: w.min(u32::from(u16::MAX)) as u16,
                height: h.min(u32::from(u16::MAX)) as u16,
                rgba,
            })
        } else {
            self.renderer
                .queue
                .submit(batches.into_iter().chain([encoder.finish()]));
            None
        };
        self.renderer.queue.present(frame);
        self.renderer.check()?;
        Ok(pixels)
    }
}

/// How far two frames of the same size are apart
/// (`docs/03-adapters-and-pipeline.md` §1.2, 렌더러 대칭의 허용 오차). Per
/// pixel, `d` is the largest RGBA channel difference (0–255).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parity {
    /// Mean `d` over all pixels.
    pub mean: f64,
    /// Share of pixels with `d > 32` (0–1).
    pub over_32: f64,
    /// The largest `d`.
    pub max: u8,
}

impl Parity {
    /// Compares `a` and `b`, which must have the same size.
    pub fn of(a: &Frame, b: &Frame) -> Parity {
        assert_eq!(
            (a.width, a.height),
            (b.width, b.height),
            "frames of different sizes"
        );
        let (mut sum, mut over, mut max) = (0u64, 0u64, 0u8);
        let pixels = a
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(b.rgba.as_chunks::<4>().0);
        let mut n = 0u64;
        for (p, q) in pixels {
            let d = (0..4).map(|i| p[i].abs_diff(q[i])).max().unwrap_or(0);
            sum += u64::from(d);
            over += u64::from(d > 32);
            max = max.max(d);
            n += 1;
        }
        let n = n.max(1) as f64;
        Parity {
            mean: sum as f64 / n,
            over_32: over as f64 / n,
            max,
        }
    }

    /// Mean `d` at most 0.5 and at most 0.1% of pixels with `d > 32`.
    pub fn within_tolerance(&self) -> bool {
        self.mean <= 0.5 && self.over_32 <= 0.001
    }
}

/// For tests: a headless renderer, or `None` (with a loud note) when the
/// machine has no adapter. With `HYPERISH_TEST_GPU=require` a missing
/// adapter fails instead, so a CI job that has a GPU cannot pass vacuously.
#[doc(hidden)]
pub fn test_renderer() -> Option<GpuRenderer> {
    match GpuRenderer::headless() {
        Ok(r) => Some(r),
        Err(e) if std::env::var("HYPERISH_TEST_GPU").as_deref() == Ok("require") => {
            panic!("HYPERISH_TEST_GPU=require, but {e}")
        }
        Err(e) => {
            eprintln!(
                "\n*** GPU checks SKIPPED: {e}. Set HYPERISH_TEST_GPU=require where a GPU (or a software Vulkan) exists. ***\n"
            );
            None
        }
    }
}
