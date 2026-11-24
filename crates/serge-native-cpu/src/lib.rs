//! `serge-native-cpu` — draws a [`ScenePrimitive`] list into a pixel buffer
//! with `vello_cpu` (pinned, `docs/02-architecture.md` §3). It is the
//! required fallback when no GPU device can be created, and what headless
//! tests and CI snapshot (`docs/03-adapters-and-pipeline.md` §1.2).

use serge_core::{Point, Rect, ScenePrimitive};
use serge_native_text::Font;
use serge_style::Color;
use vello_cpu::kurbo::{self, Affine, BezPath, Shape, Stroke};

use vello_cpu::peniko::Fill;
use vello_cpu::peniko::color::{AlphaColor, Srgb};
use vello_cpu::{Glyph, Pixmap, RenderContext, Resources};

/// A rendered frame: premultiplied RGBA8, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// Width in physical pixels.
    pub width: u16,
    /// Height in physical pixels.
    pub height: u16,
    /// Premultiplied RGBA, 4 bytes per pixel.
    pub rgba: Vec<u8>,
}

impl Frame {
    /// The pixel at `(x, y)` as premultiplied RGBA.
    pub fn pixel(&self, x: u16, y: u16) -> [u8; 4] {
        let i = (usize::from(y) * usize::from(self.width) + usize::from(x)) * 4;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }

    /// `0x00RRGGBB` words for a window surface (alpha dropped; frames are
    /// opaque because the background is drawn first).
    pub fn to_xrgb(&self, out: &mut Vec<u32>) {
        out.clear();
        out.extend(
            self.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2])),
        );
    }
}

/// A reusable CPU renderer.
pub struct CpuRenderer {
    ctx: RenderContext,
    resources: Resources,
}

impl std::fmt::Debug for CpuRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CpuRenderer").finish_non_exhaustive()
    }
}

impl Default for CpuRenderer {
    fn default() -> Self {
        Self::new()
    }
}

fn color(c: Color) -> AlphaColor<Srgb> {
    AlphaColor::from_rgba8(c.r, c.g, c.b, c.a)
}

fn rect(r: Rect) -> kurbo::Rect {
    kurbo::Rect::new(
        f64::from(r.x),
        f64::from(r.y),
        f64::from(r.right()),
        f64::from(r.bottom()),
    )
}

fn pt(p: Point) -> kurbo::Point {
    kurbo::Point::new(f64::from(p.x), f64::from(p.y))
}

/// The path through `points`, closed for a polygon.
fn polyline(points: &[Point], close: bool) -> BezPath {
    let mut path = BezPath::new();
    for (i, p) in points.iter().enumerate() {
        if i == 0 {
            path.move_to(pt(*p));
        } else {
            path.line_to(pt(*p));
        }
    }
    if close {
        path.close_path();
    }
    path
}

/// A border's ring: the outer rounded rectangle and, inside it, the inner
/// one (filled even-odd).
fn border_path(r: Rect, width: f32, radius: f32) -> BezPath {
    let mut path = kurbo::RoundedRect::from_rect(rect(r), f64::from(radius.max(0.0))).to_path(0.1);
    if r.width > 2.0 * width && r.height > 2.0 * width {
        let inner = kurbo::RoundedRect::from_rect(
            rect(r.inset(width)),
            f64::from((radius - width).max(0.0)),
        );
        path.extend(inner.path_elements(0.1));
    }
    path
}

/// How a `Line` is stroked.
fn line_stroke(width: f32) -> Stroke {
    Stroke::new(f64::from(width))
        .with_caps(kurbo::Cap::Round)
        .with_join(kurbo::Join::Round)
}

impl CpuRenderer {
    /// A renderer.
    pub fn new() -> CpuRenderer {
        CpuRenderer {
            ctx: RenderContext::new(1, 1),
            resources: Resources::new(),
        }
    }

    /// Draws `scene` (logical pixels) at `scale` physical pixels per logical
    /// pixel into a `width` × `height` frame.
    pub fn render(
        &mut self,
        scene: &[ScenePrimitive],
        width: u16,
        height: u16,
        scale: f32,
    ) -> Frame {
        let (width, height) = (width.max(1), height.max(1));
        if self.ctx.width() != width || self.ctx.height() != height {
            self.ctx = RenderContext::new(width, height);
        } else {
            self.ctx.reset();
        }
        let ctx = &mut self.ctx;
        let base = Affine::scale(f64::from(scale));
        ctx.set_transform(base);
        for p in scene {
            match p {
                ScenePrimitive::Quad {
                    rect: r,
                    color: c,
                    radius,
                } => {
                    ctx.set_paint(color(*c));
                    if *radius > 0.0 {
                        let rr = kurbo::RoundedRect::from_rect(rect(*r), f64::from(*radius));
                        ctx.fill_path(&rr.to_path(0.1));
                    } else {
                        ctx.fill_rect(&rect(*r));
                    }
                }
                ScenePrimitive::Border {
                    rect: r,
                    color: c,
                    width: w,
                    radius,
                } => {
                    // Inside the rectangle, like a CSS border: the outer
                    // edge has `radius`, the inner one `radius - width`.
                    if *w > 0.0 {
                        ctx.set_paint(color(*c));
                        ctx.set_fill_rule(Fill::EvenOdd);
                        ctx.fill_path(&border_path(*r, *w, *radius));
                        ctx.set_fill_rule(Fill::NonZero);
                    }
                }
                ScenePrimitive::Line {
                    points,
                    color: c,
                    width: w,
                } => {
                    ctx.set_paint(color(*c));
                    ctx.set_stroke(line_stroke(*w));
                    ctx.stroke_path(&polyline(points, false));
                }
                ScenePrimitive::Polygon { points, color: c } => {
                    ctx.set_paint(color(*c));
                    ctx.fill_path(&polyline(points, true));
                }
                ScenePrimitive::Glyphs {
                    origin,
                    font,
                    size,
                    color: c,
                    glyphs,
                } => {
                    let Some(font) = font.0.downcast_ref::<Font>() else {
                        continue;
                    };
                    ctx.set_paint(color(*c));
                    ctx.set_transform(
                        base * Affine::translate((f64::from(origin.x), f64::from(origin.y))),
                    );
                    let run = glyphs.iter().map(|g| Glyph {
                        id: g.id,
                        x: g.x,
                        y: g.y,
                    });
                    let _ = ctx
                        .glyph_run(&mut self.resources, font)
                        .font_size(*size)
                        .hint(true)
                        .fill_glyphs(run);
                    ctx.set_transform(base);
                }
                ScenePrimitive::PushClip(r) => ctx.push_clip_layer(&rect(*r).to_path(0.1)),
                ScenePrimitive::PopClip => ctx.pop_layer(),
            }
        }
        ctx.flush();
        let mut pixmap = Pixmap::new(width, height);
        ctx.render(&mut pixmap, &mut self.resources);
        let rgba = pixmap
            .data()
            .iter()
            .flat_map(|p| [p.r, p.g, p.b, p.a])
            .collect();
        Frame {
            width,
            height,
            rgba,
        }
    }
}

/// A coverage mask in physical pixels: `rgba` is `width` × `height`
/// premultiplied pixels, row-major, with its top-left corner at (`x`, `y`)
/// in the frame. Drawn in white, so outline coverage is `(a, a, a, a)`;
/// color glyphs (emoji) keep their colors.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    /// Left edge in the frame.
    pub x: i32,
    /// Top edge in the frame.
    pub y: i32,
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
    /// Premultiplied RGBA, 4 bytes per pixel.
    pub rgba: Vec<u8>,
}

impl Mask {
    /// Whether any pixel is not gray-scale coverage (a color glyph).
    pub fn has_color(&self) -> bool {
        self.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0] != p[3] || p[1] != p[3] || p[2] != p[3])
    }
}

/// The largest mask side; bigger paths are clipped to it.
const MAX_MASK: f64 = 8192.0;

/// Rasterizes single paths and glyphs into coverage masks with the same
/// rasterizer, strokes and transforms as [`CpuRenderer`], shifted by whole
/// pixels. A renderer that composites the masks at their positions (the GPU
/// path's atlases) therefore draws what the CPU path draws.
pub struct MaskRasterizer {
    ctx: RenderContext,
    resources: Resources,
}

impl std::fmt::Debug for MaskRasterizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MaskRasterizer").finish_non_exhaustive()
    }
}

impl Default for MaskRasterizer {
    fn default() -> Self {
        Self::new()
    }
}

impl MaskRasterizer {
    /// A rasterizer.
    pub fn new() -> MaskRasterizer {
        MaskRasterizer {
            ctx: RenderContext::new(1, 1),
            resources: Resources::new(),
        }
    }

    fn draw(
        &mut self,
        x: i32,
        y: i32,
        width: u16,
        height: u16,
        f: impl FnOnce(&mut RenderContext, &mut Resources),
    ) -> Mask {
        if self.ctx.width() != width || self.ctx.height() != height {
            self.ctx = RenderContext::new(width, height);
        } else {
            self.ctx.reset();
        }
        self.ctx
            .set_paint(AlphaColor::<Srgb>::from_rgba8(255, 255, 255, 255));
        f(&mut self.ctx, &mut self.resources);
        self.ctx.flush();
        let mut pixmap = Pixmap::new(width, height);
        self.ctx.render(&mut pixmap, &mut self.resources);
        Mask {
            x,
            y,
            width,
            height,
            rgba: pixmap
                .data()
                .iter()
                .flat_map(|p| [p.r, p.g, p.b, p.a])
                .collect(),
        }
    }

    /// The coverage of a `Line` or `Polygon` drawn at `scale`; `None` for
    /// other primitives and empty paths.
    pub fn path(&mut self, p: &ScenePrimitive, scale: f32) -> Option<Mask> {
        let (path, stroke) = match p {
            ScenePrimitive::Line { points, width, .. } => {
                (polyline(points, false), Some(line_stroke(*width)))
            }
            ScenePrimitive::Polygon { points, .. } => (polyline(points, true), None),
            _ => return None,
        };
        let device = Affine::scale(f64::from(scale));
        let mut b = (device * path.clone()).bounding_box();
        let grow = stroke
            .as_ref()
            .map_or(0.0, |s| s.width * f64::from(scale) / 2.0)
            + 1.0;
        b = b.inflate(grow, grow);
        if !(b.x0.is_finite() && b.y0.is_finite() && b.x1.is_finite() && b.y1.is_finite()) {
            return None;
        }
        let (x0, y0) = (b.x0.floor(), b.y0.floor());
        let w = (b.x1.ceil() - x0).clamp(0.0, MAX_MASK);
        let h = (b.y1.ceil() - y0).clamp(0.0, MAX_MASK);
        if w < 1.0 || h < 1.0 {
            return None;
        }
        Some(
            self.draw(x0 as i32, y0 as i32, w as u16, h as u16, |ctx, _| {
                ctx.set_transform(Affine::translate((-x0, -y0)) * device);
                match stroke {
                    Some(s) => {
                        ctx.set_stroke(s);
                        ctx.stroke_path(&path);
                    }
                    None => ctx.fill_path(&path),
                }
            }),
        )
    }

    /// One glyph of `font` at `size` logical pixels whose origin lands at
    /// device position (`x`, `y`) — where [`CpuRenderer`] would draw it
    /// (`scale × (run origin + glyph offset)`). `None` for an empty glyph.
    pub fn glyph(
        &mut self,
        font: &Font,
        size: f32,
        id: u32,
        scale: f32,
        x: f64,
        y: f64,
    ) -> Option<Mask> {
        let em = f64::from(size * scale);
        // Room around the origin; the mask is cropped to the ink below.
        let left = (em * 1.5).ceil() + 2.0;
        let top = (em * 2.5).ceil() + 2.0;
        let w = (left + em * 4.0 + 2.0).ceil().min(MAX_MASK);
        let h = (top + em * 1.5 + 2.0).ceil().min(MAX_MASK);
        let (fx, fy) = (x.floor(), y.floor());
        let (dx, dy) = (x - fx, y - fy);
        let full = self.draw(
            fx as i32 - left as i32,
            fy as i32 - top as i32,
            w as u16,
            h as u16,
            |ctx, resources| {
                // The CPU path's transform shape (scale, then translate) and
                // the same fractional position: hinting rounds the device y
                // the same way, shifted by whole pixels.
                ctx.set_transform(
                    Affine::translate((left + dx, top + dy)) * Affine::scale(f64::from(scale)),
                );
                let _ = ctx
                    .glyph_run(resources, font)
                    .font_size(size)
                    .hint(true)
                    .fill_glyphs(std::iter::once(Glyph { id, x: 0.0, y: 0.0 }));
            },
        );
        crop(full)
    }
}

/// `m` cropped to its non-empty pixels.
fn crop(m: Mask) -> Option<Mask> {
    let (w, h) = (usize::from(m.width), usize::from(m.height));
    let ink = |x: usize, y: usize| m.rgba[(y * w + x) * 4 + 3] != 0;
    let rows: Vec<usize> = (0..h).filter(|&y| (0..w).any(|x| ink(x, y))).collect();
    let (&top, &bottom) = (rows.first()?, rows.last()?);
    let cols: Vec<usize> = (0..w)
        .filter(|&x| (top..=bottom).any(|y| ink(x, y)))
        .collect();
    let (&left, &right) = (cols.first()?, cols.last()?);
    let (cw, ch) = (right - left + 1, bottom - top + 1);
    let mut rgba = Vec::with_capacity(cw * ch * 4);
    for y in top..=bottom {
        rgba.extend_from_slice(&m.rgba[(y * w + left) * 4..(y * w + right + 1) * 4]);
    }
    Some(Mask {
        x: m.x + left as i32,
        y: m.y + top as i32,
        width: cw as u16,
        height: ch as u16,
        rgba,
    })
}
