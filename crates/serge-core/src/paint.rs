//! Drawing: [`PaintCtx`] (the `Canvas { paint(bounds, ctx) => .. }` escape
//! hatch, one concrete type on every target) and [`ScenePrimitive`] (what
//! native renderers consume, GPU and CPU alike).

use std::any::Any;
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serge_style::Color;

/// A point in logical pixels.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Point {
    /// X.
    pub x: f32,
    /// Y.
    pub y: f32,
}

impl Point {
    /// A point.
    pub const fn new(x: f32, y: f32) -> Point {
        Point { x, y }
    }
}

/// A rectangle in logical pixels.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Rect {
    /// Left.
    pub x: f32,
    /// Top.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

impl Rect {
    /// A rectangle.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// Right edge.
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// Bottom edge.
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// Whether `p` is inside.
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// Moved by `(dx, dy)`.
    pub fn offset(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    /// Shrunk by `d` on every side.
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(
            self.x + d,
            self.y + d,
            (self.width - 2.0 * d).max(0.0),
            (self.height - 2.0 * d).max(0.0),
        )
    }

    /// The center.
    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// One drawing command recorded by a [`PaintCtx`], in the canvas's local
/// coordinates (origin at its top-left corner).
#[derive(Clone, PartialEq, Debug)]
pub enum PaintCmd {
    /// A filled (optionally rounded) rectangle.
    FillRect {
        /// Where.
        rect: Rect,
        /// Fill.
        color: Color,
        /// Corner radius.
        radius: f32,
    },
    /// A rectangle outline.
    StrokeRect {
        /// Where.
        rect: Rect,
        /// Stroke color.
        color: Color,
        /// Stroke width.
        width: f32,
    },
    /// A connected line through `points`.
    Polyline {
        /// The points.
        points: Vec<Point>,
        /// Stroke color.
        color: Color,
        /// Stroke width.
        width: f32,
    },
    /// A filled polygon.
    FillPolygon {
        /// The corners.
        points: Vec<Point>,
        /// Fill.
        color: Color,
    },
    /// Text with its baseline-left corner at `origin`'s top-left (the text's
    /// top is at `origin.y`).
    Text {
        /// Top-left corner.
        origin: Point,
        /// The text.
        text: String,
        /// Font size in logical pixels.
        size: f32,
        /// Color.
        color: Color,
    },
}

/// Records drawing for one `Canvas` paint. The same commands are drawn by the
/// web target's `<canvas>` 2D context and by the native renderers.
#[derive(Debug, Default)]
pub struct PaintCtx {
    width: f32,
    height: f32,
    commands: Vec<PaintCmd>,
}

impl PaintCtx {
    /// A context for a canvas of `width` × `height` logical pixels.
    pub fn new(width: f32, height: f32) -> PaintCtx {
        PaintCtx {
            width,
            height,
            commands: Vec::new(),
        }
    }

    /// The canvas size as a rectangle at the origin (also what `paint`
    /// receives as `bounds`).
    pub fn bounds(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width, self.height)
    }

    /// Fills `rect`.
    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.commands.push(PaintCmd::FillRect {
            rect,
            color,
            radius: 0.0,
        });
    }

    /// Fills `rect` with rounded corners.
    pub fn fill_rounded_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        self.commands.push(PaintCmd::FillRect {
            rect,
            color,
            radius,
        });
    }

    /// Outlines `rect`.
    pub fn stroke_rect(&mut self, rect: Rect, width: f32, color: Color) {
        self.commands
            .push(PaintCmd::StrokeRect { rect, color, width });
    }

    /// A line from `a` to `b`.
    pub fn line(&mut self, a: Point, b: Point, width: f32, color: Color) {
        self.polyline(&[a, b], width, color);
    }

    /// A connected line through `points`.
    pub fn polyline(&mut self, points: &[Point], width: f32, color: Color) {
        if points.len() >= 2 {
            self.commands.push(PaintCmd::Polyline {
                points: points.to_vec(),
                color,
                width,
            });
        }
    }

    /// Fills the polygon with corners `points`.
    pub fn fill_polygon(&mut self, points: &[Point], color: Color) {
        if points.len() >= 3 {
            self.commands.push(PaintCmd::FillPolygon {
                points: points.to_vec(),
                color,
            });
        }
    }

    /// Draws `text` with its top-left corner at `origin`.
    pub fn text(&mut self, origin: Point, text: impl Into<String>, size: f32, color: Color) {
        self.commands.push(PaintCmd::Text {
            origin,
            text: text.into(),
            size,
            color,
        });
    }

    /// The recorded commands.
    pub fn commands(&self) -> &[PaintCmd] {
        &self.commands
    }

    /// Takes the recorded commands.
    pub fn into_commands(self) -> Vec<PaintCmd> {
        self.commands
    }
}

/// A shaped glyph: font-specific id and position relative to the run's
/// origin.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Glyph {
    /// Glyph id in the font.
    pub id: u32,
    /// X offset.
    pub x: f32,
    /// Y offset (baseline).
    pub y: f32,
}

/// A font as the native text system loaded it. Opaque here; renderers
/// downcast it to their font type.
#[derive(Clone)]
pub struct FontHandle(pub Arc<dyn Any + Send + Sync>);

impl std::fmt::Debug for FontHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FontHandle(..)")
    }
}

impl PartialEq for FontHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// A platform-neutral drawing primitive in window coordinates (logical
/// pixels), in paint order. Native GPU and CPU renderers draw the same list
/// (`docs/03-adapters-and-pipeline.md` §1.2).
#[derive(Clone, PartialEq, Debug)]
pub enum ScenePrimitive {
    /// A mounted local surface, composited in scene order.
    ExternalTexture {
        /// Slot and generation resolved by the local renderer.
        key: crate::viewport::TextureKey,
        /// Destination rectangle in logical window coordinates.
        rect: Rect,
    },
    /// A filled rectangle with rounded corners.
    Quad {
        /// Where.
        rect: Rect,
        /// Fill.
        color: Color,
        /// Corner radius.
        radius: f32,
    },
    /// A rectangle outline (inside the rectangle).
    Border {
        /// Where.
        rect: Rect,
        /// Stroke color.
        color: Color,
        /// Stroke width.
        width: f32,
        /// Corner radius.
        radius: f32,
    },
    /// A stroked polyline.
    Line {
        /// The points.
        points: Vec<Point>,
        /// Stroke color.
        color: Color,
        /// Stroke width.
        width: f32,
    },
    /// A filled polygon.
    Polygon {
        /// The corners.
        points: Vec<Point>,
        /// Fill.
        color: Color,
    },
    /// A run of glyphs of one font, size and color.
    Glyphs {
        /// The run's origin (glyph positions are relative to it).
        origin: Point,
        /// The font.
        font: FontHandle,
        /// Font size in logical pixels.
        size: f32,
        /// Color.
        color: Color,
        /// The glyphs.
        glyphs: Vec<Glyph>,
    },
    /// Clip what follows to `rect` until the matching [`PopClip`](Self::PopClip).
    PushClip(Rect),
    /// Ends the innermost clip.
    PopClip,
}

/// A source of frames outside the reactive graph — a ring buffer an engine
/// thread writes to. Reading one inside `paint` makes the canvas repaint on
/// the next frame after the source changes.
#[derive(Clone, Default)]
pub struct FrameSource {
    local: Arc<AtomicU64>,
    external: Option<Arc<dyn Fn() -> u64 + Send + Sync>>,
}

impl std::fmt::Debug for FrameSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameSource")
            .field("version", &self.version())
            .finish()
    }
}

impl FrameSource {
    /// A new source.
    pub fn new() -> FrameSource {
        FrameSource::default()
    }

    /// Observes an external producer’s revision when the renderer polls a frame.
    pub fn with_revision(read: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        Self {
            external: Some(Arc::new(read)),
            ..Self::default()
        }
    }

    /// Marks new data (any thread).
    pub fn bump(&self) {
        self.local.fetch_add(1, Ordering::Release);
    }

    /// The change counter.
    pub fn version(&self) -> u64 {
        self.local
            .load(Ordering::Acquire)
            .wrapping_add(self.external.as_ref().map_or(0, |read| read()))
    }

    /// Records that the paint running on this thread read this source.
    pub fn track(&self) {
        TRACKED.with(|t| {
            if let Some(list) = t.borrow_mut().as_mut() {
                list.push((self.clone(), self.version()));
            }
        });
    }
}

thread_local! {
    static TRACKED: RefCell<Option<Vec<(FrameSource, u64)>>> = const { RefCell::new(None) };
}

/// Runs `f` and returns the frame sources it read, with the versions seen
/// (renderers call this around `paint`).
pub fn track_frame_sources<R>(f: impl FnOnce() -> R) -> (R, Vec<(FrameSource, u64)>) {
    let saved = TRACKED.with(|t| t.borrow_mut().replace(Vec::new()));
    let r = f();
    let list = TRACKED.with(|t| std::mem::replace(&mut *t.borrow_mut(), saved));
    (r, list.unwrap_or_default())
}

/// Whether any tracked source changed since it was read.
pub fn sources_changed(sources: &[(FrameSource, u64)]) -> bool {
    sources.iter().any(|(s, seen)| s.version() != *seen)
}
