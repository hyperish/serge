//! `serge-native-cpu` — draws a [`ScenePrimitive`] list into a pixel buffer
//! with `vello_cpu` (pinned, `docs/02-architecture.md` §3). It is the
//! required fallback when no GPU device can be created, and what headless
//! tests and CI snapshot (`docs/03-adapters-and-pipeline.md` §1.2).

use serge_core::{Point, Rect, ScenePrimitive};
use serge_native_text::Font;
use serge_style::Color;
use vello_cpu::kurbo::{self, Affine, BezPath, Shape, Stroke};

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
                    ctx.set_paint(color(*c));
                    ctx.set_stroke(Stroke::new(f64::from(*w)));
                    // Inside the rectangle, like a CSS border.
                    let inner = r.inset(w / 2.0);
                    let rr = kurbo::RoundedRect::from_rect(
                        rect(inner),
                        f64::from((radius - w / 2.0).max(0.0)),
                    );
                    ctx.stroke_path(&rr.to_path(0.1));
                }
                ScenePrimitive::Line {
                    points,
                    color: c,
                    width: w,
                } => {
                    let mut path = BezPath::new();
                    for (i, p) in points.iter().enumerate() {
                        if i == 0 {
                            path.move_to(pt(*p));
                        } else {
                            path.line_to(pt(*p));
                        }
                    }
                    ctx.set_paint(color(*c));
                    ctx.set_stroke(
                        Stroke::new(f64::from(*w))
                            .with_caps(kurbo::Cap::Round)
                            .with_join(kurbo::Join::Round),
                    );
                    ctx.stroke_path(&path);
                }
                ScenePrimitive::Polygon { points, color: c } => {
                    let mut path = BezPath::new();
                    for (i, p) in points.iter().enumerate() {
                        if i == 0 {
                            path.move_to(pt(*p));
                        } else {
                            path.line_to(pt(*p));
                        }
                    }
                    path.close_path();
                    ctx.set_paint(color(*c));
                    ctx.fill_path(&path);
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
