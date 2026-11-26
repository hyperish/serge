//! Adapted from Parley 0.13.0, `parley/src/layout/line.rs`, revision
//! 32a137efbb228ad0b1c6a46b979dfb55d4584eae, under the MIT license.
//! Emit the scene glyph directly while advancing through the run in visual order.
use parley::layout::GlyphRun;
use serge_core::Glyph;

pub(super) fn positioned(run: &GlyphRun<'_, ()>) -> Vec<Glyph> {
    let mut offset = run.offset();
    let baseline = run.baseline();
    run.glyphs()
        .map(|glyph| {
            let positioned = Glyph {
                id: glyph.id,
                x: glyph.x + offset,
                y: glyph.y + baseline,
            };
            offset += glyph.advance;
            positioned
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use parley::{FontContext, LayoutContext, PositionedLayoutItem};

    #[test]
    fn positions_match_parley_across_lines_and_writing_systems() {
        let mut fonts = FontContext::new();
        let mut layouts = LayoutContext::<()>::new();
        let sample = "Serge 한글 שלום café a\u{301} المزيد";
        let builder = layouts.ranged_builder(&mut fonts, sample, 1.0, true);
        let mut layout = builder.build(sample);
        layout.break_all_lines(Some(90.0));
        let mut glyph_count = 0;
        for item in layout.lines().flat_map(|line| line.items()) {
            if let PositionedLayoutItem::GlyphRun(run) = item {
                let expected: Vec<_> = run
                    .positioned_glyphs()
                    .map(|p| Glyph {
                        id: p.id,
                        x: p.x,
                        y: p.y,
                    })
                    .collect();
                glyph_count += expected.len();
                assert_eq!(positioned(&run), expected);
            }
        }
        assert!(glyph_count > 16);
    }
}
