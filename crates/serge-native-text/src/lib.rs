//! `serge-native-text` — text shaping, line breaking and measurement for
//! native targets on `parley` (pinned, `docs/02-architecture.md` §3), with
//! system fonts and fallback (CJK included). Shaped runs become
//! [`ScenePrimitive::Glyphs`]; renderers rasterize the glyph outlines.

// Linux: fontconfig is loaded at run time (see Cargo.toml).
#[cfg(target_os = "linux")]
use fontique as _;
use std::collections::HashMap;
use std::sync::Arc;

mod glyph_run;

use parley::{FontContext, FontWeight, LayoutContext, PositionedLayoutItem, StyleProperty};
use serge_core::{FontHandle, Glyph, Point, ScenePrimitive};
use serge_style::Color;

/// The font type behind [`FontHandle`] on native targets.
pub use parley::FontData as Font;

/// A shaped run.
#[derive(Clone, Debug)]
pub struct Run {
    /// The font.
    pub font: FontHandle,
    /// Size in logical pixels.
    pub size: f32,
    /// Glyphs relative to the layout's top-left corner.
    pub glyphs: Vec<Glyph>,
}

/// A laid-out paragraph.
#[derive(Clone, Debug, Default)]
pub struct TextLayout {
    /// Width of the widest line.
    pub width: f32,
    /// Total height.
    pub height: f32,
    /// The runs.
    pub runs: Vec<Run>,
}

impl TextLayout {
    /// The paragraph drawn with its top-left corner at `origin`.
    pub fn primitives(
        &self,
        origin: Point,
        color: Color,
    ) -> impl Iterator<Item = ScenePrimitive> + '_ {
        self.runs.iter().map(move |r| ScenePrimitive::Glyphs {
            origin,
            font: r.font.clone(),
            size: r.size,
            color,
            glyphs: r.glyphs.clone(),
        })
    }
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct Key {
    size: u32,
    weight: u16,
    max_width: Option<u32>,
}

/// Fonts, scratch space and a layout cache. One per UI thread.
pub struct TextSystem {
    fonts: FontContext,
    layouts: LayoutContext<()>,
    cache: HashMap<Box<str>, HashMap<Key, TextLayout>>,
    cached_layouts: usize,
    handles: HashMap<(u64, u32), FontHandle>,
}

impl Default for TextSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for TextSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextSystem").finish_non_exhaustive()
    }
}

impl TextSystem {
    /// A text system over the platform's fonts.
    pub fn new() -> TextSystem {
        TextSystem {
            fonts: FontContext::new(),
            layouts: LayoutContext::new(),
            cache: HashMap::new(),
            cached_layouts: 0,
            handles: HashMap::new(),
        }
    }

    /// Lays out `text` (wrapped at `max_width` when given).
    pub fn layout(
        &mut self,
        text: &str,
        size: f32,
        weight: u16,
        max_width: Option<f32>,
    ) -> TextLayout {
        self.layout_ref(text, size, weight, max_width).clone()
    }

    /// Borrows the shaped paragraph until the next mutable access to the text system.
    pub fn layout_ref(
        &mut self,
        text: &str,
        size: f32,
        weight: u16,
        max_width: Option<f32>,
    ) -> &TextLayout {
        // Key on the actual line-breaking width, including its half-pixel rounding allowance.
        let wrap = max_width.map(|w| (w.max(1.0) + 0.5).ceil());
        let key = Key {
            size: size.to_bits(),
            weight,
            max_width: wrap.map(f32::to_bits),
        };
        if self
            .cache
            .get(text)
            .is_some_and(|styles| styles.contains_key(&key))
        {
            return &self.cache[text][&key];
        }
        if self.cached_layouts >= 2048 {
            self.cache.clear();
            self.cached_layouts = 0;
        }
        let mut b = self
            .layouts
            .ranged_builder(&mut self.fonts, text, 1.0, true);
        b.push_default(StyleProperty::FontSize(size));
        b.push_default(StyleProperty::FontWeight(FontWeight::new(f32::from(
            weight,
        ))));
        // The web target's `line-height: 1.25`.
        b.push_default(StyleProperty::LineHeight(
            parley::LineHeight::FontSizeRelative(1.25),
        ));
        let mut layout: parley::Layout<()> = b.build(text);
        layout.break_all_lines(wrap);
        let mut runs = Vec::new();
        for line in layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(g) = item {
                    let run = g.run();
                    let font = run.font();
                    let handle = self
                        .handles
                        .entry((font.data.id(), font.index))
                        .or_insert_with(|| FontHandle(Arc::new(font.clone())))
                        .clone();
                    runs.push(Run {
                        font: handle,
                        size: run.font_size(),
                        glyphs: glyph_run::positioned(&g),
                    });
                }
            }
        }
        let out = TextLayout {
            width: layout.width(),
            height: layout.height().max(size * 1.2),
            runs,
        };
        self.cached_layouts += 1;
        self.cache
            .entry(text.into())
            .or_default()
            .entry(key)
            .or_insert(out)
    }

    /// The size of `text` (wrapped at `max_width` when given).
    pub fn measure(
        &mut self,
        text: &str,
        size: f32,
        weight: u16,
        max_width: Option<f32>,
    ) -> (f32, f32) {
        let l = self.layout_ref(text, size, weight, max_width);
        (l.width.ceil(), l.height.ceil())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wider_text_measures_wider_and_wraps() {
        let mut t = TextSystem::new();
        let (w1, h1) = t.measure("Count: 0", 16.0, 400, None);
        let (w2, _) = t.measure("Count: 0 and more words here", 16.0, 400, None);
        assert!(w1 > 0.0 && h1 > 0.0 && w2 > w1, "{w1} {w2}");
        let (_, wrapped) = t.measure("Count: 0 and more words here", 16.0, 400, Some(60.0));
        assert!(wrapped > h1, "wrapping adds lines");
    }
}
