//! Native layout solver, `SceneBatch` collector, GPU batching, and mandatory CPU software fallback.

use crate::core::{Element, PaintCtx, PropValue, Rect, ScenePrimitive};
use crate::style::{Color, ColorTokens, LayoutDirection};

/// Selected native rendering backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderBackendKind {
    /// Instanced GPU pipeline (`wgpu` / native HAL).
    Gpu,
    /// Mandatory symmetric CPU software rasterizer fallback.
    CpuFallback,
}

/// Resolves the native rendering backend from hardware availability and `HYPERISH_RENDER_BACKEND`.
#[must_use]
pub fn select_render_backend(gpu_available: bool) -> RenderBackendKind {
    let env_val = std::env::var("HYPERISH_RENDER_BACKEND").ok();
    select_render_backend_with_override(gpu_available, env_val.as_deref())
}

/// Pure helper resolving backend choice with an explicit optional override string (`"cpu"` / `"gpu"`).
#[must_use]
pub fn select_render_backend_with_override(
    gpu_available: bool,
    override_backend: Option<&str>,
) -> RenderBackendKind {
    if matches!(override_backend, Some(s) if s.eq_ignore_ascii_case("cpu")) {
        return RenderBackendKind::CpuFallback;
    }
    if !gpu_available {
        return RenderBackendKind::CpuFallback;
    }
    RenderBackendKind::Gpu
}

/// Complete sorted frame scene emitted by layout + paint, consumed identically by GPU & CPU renderers.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneBatch {
    /// Target viewport width in pixels.
    pub viewport_width: u32,
    /// Target viewport height in pixels.
    pub viewport_height: u32,
    /// Viewport clear color.
    pub clear_color: Color,
    /// Flat ordered primitive draw list.
    pub primitives: Vec<ScenePrimitive>,
}

/// Performs deterministic layout calculation and collects [`ScenePrimitive`] items for `element`.
#[must_use]
pub fn layout_and_paint(
    element: &Element,
    viewport_width: u32,
    viewport_height: u32,
    tokens: ColorTokens,
) -> SceneBatch {
    let mut primitives = Vec::new();
    let root_bounds = Rect::new(0.0, 0.0, viewport_width as f32, viewport_height as f32);
    layout_node(element, root_bounds, tokens, &mut primitives);
    SceneBatch {
        viewport_width,
        viewport_height,
        clear_color: tokens.background,
        primitives,
    }
}

fn layout_node(
    el: &Element,
    bounds: Rect,
    tokens: ColorTokens,
    out: &mut Vec<ScenePrimitive>,
) -> f32 {
    if matches!(el.tag.as_str(), "Empty" | "Title" | "Meta") {
        return 0.0;
    }

    let padding = el.props.get("padding").map_or(0.0, |p| p.as_px(0.0));
    let gap = el.props.get("gap").map_or(8.0, |p| p.as_px(8.0));
    let explicit_h = el.props.get("height").map(|p| p.as_px(40.0));
    let direction = match el.props.get("layout") {
        Some(PropValue::Layout(dir)) => *dir,
        _ => LayoutDirection::VStack,
    };

    match el.tag.as_str() {
        "Card" | "Stack" | "Fragment" => {
            let children = el.resolved_children();
            let inner_x = bounds.x + padding;
            let mut cursor_y = bounds.y + padding;
            let mut cursor_x = inner_x;
            let inner_w = (bounds.width - padding * 2.0).max(16.0);
            let mut child_prims = Vec::new();

            for (idx, child) in children.iter().enumerate() {
                if idx > 0 {
                    match direction {
                        LayoutDirection::VStack => cursor_y += gap,
                        LayoutDirection::HStack => cursor_x += gap,
                        LayoutDirection::ZStack => {}
                    }
                }
                let slot = Rect::new(cursor_x, cursor_y, inner_w, 48.0);
                let consumed_h = layout_node(child, slot, tokens, &mut child_prims);
                if direction == LayoutDirection::VStack {
                    cursor_y += consumed_h;
                }
            }

            let total_h = explicit_h.unwrap_or((cursor_y - bounds.y + padding).max(32.0));
            if el.tag == "Card" {
                out.push(ScenePrimitive::Rect {
                    bounds: Rect::new(bounds.x, bounds.y, bounds.width, total_h),
                    fill: tokens.surface,
                    border: Some(tokens.border),
                    border_width: 1.0,
                    radius: 8.0,
                });
            }
            out.extend(child_prims);
            total_h
        }
        "Text" => {
            let h = explicit_h.unwrap_or(24.0);
            if let Some(txt) = el.resolved_text() {
                out.push(ScenePrimitive::TextRun {
                    bounds: Rect::new(bounds.x, bounds.y, bounds.width, h),
                    text: txt,
                    color: tokens.text,
                    font_size: 16.0,
                });
            }
            h
        }
        "Button" => {
            let h = explicit_h.unwrap_or(36.0);
            let btn_rect = Rect::new(bounds.x, bounds.y, bounds.width.min(180.0), h);
            out.push(ScenePrimitive::Rect {
                bounds: btn_rect,
                fill: tokens.accent,
                border: None,
                border_width: 0.0,
                radius: 6.0,
            });
            if let Some(txt) = el.resolved_text() {
                out.push(ScenePrimitive::TextRun {
                    bounds: Rect::new(btn_rect.x + 12.0, btn_rect.y + 8.0, btn_rect.width, 20.0),
                    text: txt,
                    color: Color::WHITE,
                    font_size: 14.0,
                });
            }
            h
        }
        "Slider" => {
            let h = explicit_h.unwrap_or(28.0);
            let val = el.resolved_value().and_then(|v| v.as_f32()).unwrap_or(0.0);
            let (min, max) = match el.props.get("range") {
                Some(PropValue::Range(a, b)) if b > a => (*a, *b),
                _ => (0.0, 1.0),
            };
            let norm = ((val - min) / (max - min)).clamp(0.0, 1.0);
            out.push(ScenePrimitive::ProgressBarFill {
                bounds: Rect::new(bounds.x, bounds.y + 8.0, bounds.width, 10.0),
                progress: norm,
                track: tokens.border,
                fill: tokens.accent,
            });
            h
        }
        "ProgressBar" => {
            let h = explicit_h.unwrap_or(18.0);
            let norm = el
                .resolved_value()
                .and_then(|v| v.as_f32())
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            out.push(ScenePrimitive::ProgressBarFill {
                bounds: Rect::new(bounds.x, bounds.y + 4.0, bounds.width, 10.0),
                progress: norm,
                track: tokens.border,
                fill: tokens.accent,
            });
            h
        }
        "Canvas" => {
            let h = explicit_h.unwrap_or(64.0);
            let canvas_rect = Rect::new(bounds.x, bounds.y, bounds.width, h);
            let mut pctx = PaintCtx::new();
            el.paint_into(canvas_rect, &mut pctx);
            out.extend(pctx.into_primitives());
            h
        }
        _ => explicit_h.unwrap_or(24.0),
    }
}

/// Statistics produced by the batched GPU submission pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuFrameStats {
    /// Number of batched instanced GPU draw calls emitted (1-3 in Phase 1).
    pub draw_calls: u32,
    /// Number of rectangle/bar instances packed into the instance buffer.
    pub rect_instances: usize,
    /// Number of sparkline line vertices emitted.
    pub line_segments: usize,
    /// Number of glyph atlas text runs emitted.
    pub text_runs: usize,
}

/// Instanced GPU batching renderer (`serge-native-gpu` PoC pipeline).
pub struct GpuBatchRenderer;

impl GpuBatchRenderer {
    /// Packs `batch` primitives into instanced GPU draw batches (minimal draw-call overhead).
    #[must_use]
    pub fn prepare_frame(batch: &SceneBatch) -> GpuFrameStats {
        let mut rect_instances = 0usize;
        let mut line_segments = 0usize;
        let mut text_runs = 0usize;
        for prim in &batch.primitives {
            match prim {
                ScenePrimitive::Rect { .. }
                | ScenePrimitive::HistogramBar { .. }
                | ScenePrimitive::ProgressBarFill { .. }
                | ScenePrimitive::ExternalTexture { .. } => rect_instances += 1,
                ScenePrimitive::SparklineSegment { .. } => line_segments += 1,
                ScenePrimitive::TextRun { .. } => text_runs += 1,
            }
        }
        let mut draw_calls = 0u32;
        if rect_instances > 0 {
            draw_calls += 1;
        }
        if line_segments > 0 {
            draw_calls += 1;
        }
        if text_runs > 0 {
            draw_calls += 1;
        }
        GpuFrameStats {
            draw_calls,
            rect_instances,
            line_segments,
            text_runs,
        }
    }
}

/// RGBA8 pixel buffer produced by [`CpuFallbackRenderer`].
#[derive(Debug, Clone)]
pub struct CpuFrameBuffer {
    /// Buffer width in pixels.
    pub width: u32,
    /// Buffer height in pixels.
    pub height: u32,
    /// Clear background color used for pixel comparison.
    pub clear_color: Color,
    /// Tightly packed RGBA8 pixel rows (`width * height * 4`).
    pub pixels: Vec<u8>,
}

impl CpuFrameBuffer {
    /// Reads the RGBA8 pixel at `(x, y)`.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let idx = ((y * self.width + x) * 4) as usize;
        Some([
            self.pixels[idx],
            self.pixels[idx + 1],
            self.pixels[idx + 2],
            self.pixels[idx + 3],
        ])
    }

    /// Counts pixels that differ from [`Self::clear_color`], verifying geometry was rasterized.
    #[must_use]
    pub fn painted_pixel_count(&self) -> usize {
        let bg = self.clear_color.to_bytes();
        self.pixels.chunks_exact(4).filter(|p| *p != bg).count()
    }
}

/// Pure-Rust CPU software fallback rasterizer consuming the exact same [`SceneBatch`] as GPU.
pub struct CpuFallbackRenderer;

impl CpuFallbackRenderer {
    /// Rasterizes `batch` into an offscreen RGBA8 [`CpuFrameBuffer`].
    #[must_use]
    pub fn render(batch: &SceneBatch) -> CpuFrameBuffer {
        let total = (batch.viewport_width as usize) * (batch.viewport_height as usize);
        let bg = batch.clear_color.to_bytes();
        let mut pixels = Vec::with_capacity(total * 4);
        for _ in 0..total {
            pixels.extend_from_slice(&bg);
        }

        let mut fb = CpuFrameBuffer {
            width: batch.viewport_width,
            height: batch.viewport_height,
            clear_color: batch.clear_color,
            pixels,
        };

        for prim in &batch.primitives {
            match prim {
                ScenePrimitive::Rect { bounds, fill, .. } => fill_rect(&mut fb, *bounds, *fill),
                ScenePrimitive::HistogramBar { bounds, color, .. } => {
                    fill_rect(&mut fb, *bounds, *color);
                }
                ScenePrimitive::ProgressBarFill {
                    bounds,
                    progress,
                    track,
                    fill,
                } => {
                    fill_rect(&mut fb, *bounds, *track);
                    let active_w = bounds.width * progress.clamp(0.0, 1.0);
                    fill_rect(
                        &mut fb,
                        Rect::new(bounds.x, bounds.y, active_w, bounds.height),
                        *fill,
                    );
                }
                ScenePrimitive::SparklineSegment {
                    x0,
                    y0,
                    x1,
                    y1,
                    color,
                } => draw_line(&mut fb, *x0, *y0, *x1, *y1, *color),
                ScenePrimitive::TextRun {
                    bounds,
                    text,
                    color,
                    ..
                } => {
                    // Software fallback paints crisp glyph-width baseline blocks per character.
                    let glyph_w = 7.0_f32;
                    for (i, _) in text.chars().enumerate() {
                        let gx = bounds.x + i as f32 * glyph_w;
                        if gx + glyph_w > bounds.x + bounds.width {
                            break;
                        }
                        fill_rect(
                            &mut fb,
                            Rect::new(gx, bounds.y + 4.0, 5.0, (bounds.height - 8.0).max(4.0)),
                            *color,
                        );
                    }
                }
                ScenePrimitive::ExternalTexture { bounds, .. } => {
                    fill_rect(&mut fb, *bounds, Color::EMERALD_500);
                }
            }
        }
        fb
    }
}

fn fill_rect(fb: &mut CpuFrameBuffer, rect: Rect, color: Color) {
    let x0 = (rect.x.floor().max(0.0) as u32).min(fb.width);
    let y0 = (rect.y.floor().max(0.0) as u32).min(fb.height);
    let x1 = ((rect.x + rect.width).ceil().max(0.0) as u32).min(fb.width);
    let y1 = ((rect.y + rect.height).ceil().max(0.0) as u32).min(fb.height);
    let rgba = color.to_bytes();
    for y in y0..y1 {
        for x in x0..x1 {
            let idx = ((y * fb.width + x) * 4) as usize;
            fb.pixels[idx..idx + 4].copy_from_slice(&rgba);
        }
    }
}

fn draw_line(fb: &mut CpuFrameBuffer, x0: f32, y0: f32, x1: f32, y1: f32, color: Color) {
    let steps = ((x1 - x0).abs().max((y1 - y0).abs()).ceil() as usize).max(1);
    let rgba = color.to_bytes();
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = (x0 + (x1 - x0) * t).round() as i32;
        let y = (y0 + (y1 - y0) * t).round() as i32;
        if x >= 0 && y >= 0 && (x as u32) < fb.width && (y as u32) < fb.height {
            let idx = (((y as u32) * fb.width + (x as u32)) * 4) as usize;
            fb.pixels[idx..idx + 4].copy_from_slice(&rgba);
        }
    }
}
