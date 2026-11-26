//! Frame initialization adapted from vello_cpu 0.4.0 `RenderContext::render_with`,
//! b7e8f523b45c07a5edb806bb7241ef5afe34a882 (MIT, see LICENSE-MIT).
use super::{Color, Frame, ScenePrimitive};

/// An opaque, square-cornered first quad covering the full target can supply the
/// rasterizer's clear color. Only the first command is removed so clip and external
/// texture presentation retain their original ordering and side effects.
pub(super) fn background(
    scene: &[ScenePrimitive],
    width: u16,
    height: u16,
    scale: f32,
) -> (Color, &[ScenePrimitive]) {
    if let Some((
        ScenePrimitive::Quad {
            rect,
            color,
            radius,
        },
        rest,
    )) = scene.split_first()
        && *radius <= 0.
        && color.a == 255
        && scale.is_finite()
        && scale > 0.
        && rect.x <= 0.
        && rect.y <= 0.
        && rect.x.is_finite()
        && rect.y.is_finite()
        && rect.right().is_finite()
        && rect.bottom().is_finite()
        && f64::from(rect.right()) * f64::from(scale) >= f64::from(width)
        && f64::from(rect.bottom()) * f64::from(scale) >= f64::from(height)
    {
        (*color, rest)
    } else {
        (Color::TRANSPARENT, scene)
    }
}

/// The target is owned, sized exactly for the frame, and has no stride padding.
/// The caller supplies either transparent black or a fully opaque fill.
pub(super) fn solid(width: u16, height: u16, color: Color) -> Frame {
    let length = (usize::from(width) * usize::from(height))
        .checked_mul(4)
        .expect("frame exceeds address space");
    let mut rgba = vec![0; length];
    let pixel = [color.r, color.g, color.b, color.a];
    if pixel != [0; 4] {
        for dest in rgba.as_chunks_mut::<4>().0 {
            dest.copy_from_slice(&pixel);
        }
    }
    Frame {
        width,
        height,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CpuRenderer, RenderContext, Resources, color, kurbo, raster_rgba};
    use serge_core::Rect;

    fn quad(rect: Rect, color: Color, radius: f32) -> ScenePrimitive {
        ScenePrimitive::Quad {
            rect,
            color,
            radius,
        }
    }
    fn original(width: u16, height: u16, scale: f32, scene: &[ScenePrimitive]) -> Vec<u8> {
        let mut ctx = RenderContext::new(width, height);
        ctx.set_transform(kurbo::Affine::scale(f64::from(scale)));
        for p in scene {
            let ScenePrimitive::Quad {
                rect,
                color: c,
                radius,
            } = p
            else {
                unreachable!()
            };
            ctx.set_paint(color(*c));
            let r = super::super::rect(*rect);
            if *radius > 0. {
                use kurbo::Shape;
                ctx.fill_path(&kurbo::RoundedRect::from_rect(r, f64::from(*radius)).to_path(0.1));
            } else {
                ctx.fill_rect(&r);
            }
        }
        raster_rgba(&mut ctx, &mut Resources::new(), Color::TRANSPARENT)
    }
    #[test]
    fn target_clear_matches_vello_through_edges_and_resizes() {
        let mut cpu = CpuRenderer::new();
        let opaque = Color::rgb(121, 37, 188);
        for scale in [1., 1.25, 2.] {
            for (w, h) in [(1u16, 1u16), (37, 28), (65, 67)] {
                let size = Rect::new(
                    -2.,
                    -1.,
                    f32::from(w) / scale + 3.,
                    f32::from(h) / scale + 2.,
                );
                let layer = || {
                    quad(
                        Rect::new(0.5, 2.75, 29., 14.25),
                        Color::rgba(22, 110, 35, 143),
                        2.,
                    )
                };
                for scene in [
                    vec![],
                    vec![quad(size, opaque, 0.)],
                    vec![quad(size, opaque, 0.), layer()],
                    vec![quad(size, Color::rgba(121, 37, 188, 227), 0.), layer()],
                    vec![quad(size, opaque, 3.)],
                    vec![quad(
                        Rect::new(0.25, 0., size.width, size.height),
                        opaque,
                        0.,
                    )],
                ] {
                    assert_eq!(
                        cpu.render(&scene, w, h, scale).rgba,
                        original(w, h, scale, &scene)
                    );
                }
            }
        }
    }
    #[test]
    #[ignore = "local frame comparison, run with --release --ignored --nocapture"]
    fn compare_background() {
        use std::{hint::black_box, time::Instant};
        let (w, h) = (641u16, 361u16);
        let bg = Color::rgb(38, 57, 77);
        let scene = [quad(Rect::new(0., 0., w.into(), h.into()), bg, 0.)];
        let mut cpu = CpuRenderer::new();
        let mut ctx = RenderContext::new(w, h);
        let mut resources = Resources::new();
        let mut runs = [Vec::new(), Vec::new()];
        for trial in 0..21 {
            for path in [trial % 2, 1 - trial % 2] {
                let start = Instant::now();
                for _ in 0..50 {
                    let frame = if path == 0 {
                        cpu.render(&scene, w, h, 1.).rgba
                    } else {
                        ctx.reset();
                        ctx.set_paint(color(bg));
                        ctx.fill_rect(&kurbo::Rect::new(0., 0., w.into(), h.into()));
                        raster_rgba(&mut ctx, &mut resources, Color::TRANSPARENT)
                    };
                    black_box(frame);
                }
                runs[path].push(start.elapsed().as_nanos() / 50);
            }
        }
        for (name, mut ns) in ["solid target", "Vello background quad"]
            .into_iter()
            .zip(runs)
        {
            ns.sort_unstable();
            println!("{name} ns per frame {} ({}..{})", ns[10], ns[0], ns[20]);
        }
    }
}
