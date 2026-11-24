//! CPU frame-buffer snapshots of the README counter (font-independent: colors
//! at known places, and ink inside the text's box).
#![cfg(not(target_arch = "wasm32"))]
#![allow(missing_docs)]

use hyve::prelude::*;
use serge::prelude::*;
use serge::testing::Headless;
use serge::{Rect, Theme};
use serge_native_cpu::{CpuRenderer, Frame};

#[component]
pub fn CounterCard() -> Element {
    let mut count = signal(0);
    ui! {
        Card { padding: 16px, gap: 12px,
            Text { "Count: {count()}" }
            Button { text: "Increment", clicked => { count += 1; } }
        }
    }
}

/// With `SERGE_SNAPSHOT_DIR` set, frames are written there as PPM files to
/// look at.
fn dump(f: &Frame, name: &str) {
    if let Ok(dir) = std::env::var("SERGE_SNAPSHOT_DIR") {
        let mut out = format!("P6\n{} {}\n255\n", f.width, f.height).into_bytes();
        for p in f.rgba.as_chunks::<4>().0 {
            out.extend_from_slice(&p[..3]);
        }
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.ppm")), out).unwrap();
    }
}

fn rgb(f: &Frame, x: f32, y: f32, scale: f32) -> [u8; 3] {
    let p = f.pixel((x * scale) as u16, (y * scale) as u16);
    [p[0], p[1], p[2]]
}

fn ink(f: &Frame, r: Rect, scale: f32, bg: [u8; 3]) -> usize {
    let mut n = 0;
    for y in (r.y * scale) as u16..(r.bottom() * scale) as u16 {
        for x in (r.x * scale) as u16..(r.right() * scale) as u16 {
            let p = f.pixel(x, y);
            if [p[0], p[1], p[2]] != bg {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn the_counter_renders_on_the_cpu() {
    for scale in [1.0_f32, 2.0] {
        let h = Headless::new(320.0, 200.0, || ui! { CounterCard {} });
        let mut r = CpuRenderer::new();
        let frame = r.render(
            &h.settle(),
            (320.0 * scale) as u16,
            (200.0 * scale) as u16,
            scale,
        );
        dump(&frame, &format!("counter@{scale}x"));
        let t = Theme::LIGHT;
        let c = |c: Color| [c.r, c.g, c.b];
        // Window background outside the card, card surface inside it.
        let text = h.rect_of("Count: 0").expect("text");
        let button = h.rect_of("Increment").expect("button");
        assert_eq!(rgb(&frame, 318.0, 198.0, scale), c(t.background));
        assert_eq!(rgb(&frame, text.x - 8.0, text.y + 2.0, scale), c(t.surface));
        // The button is the primary color away from its label and corners.
        assert_eq!(
            rgb(&frame, button.x + 4.0, button.center().y, scale),
            c(t.primary)
        );
        // Glyphs leave ink inside the text box (any font).
        assert!(
            ink(&frame, text, scale, c(t.surface)) > 20,
            "no text ink at scale {scale}"
        );

        // A click repaints the new count in the same place.
        h.click_text("Increment");
        let after = r.render(
            &h.settle(),
            (320.0 * scale) as u16,
            (200.0 * scale) as u16,
            scale,
        );
        let text2 = h.rect_of("Count: 1").expect("updated text");
        assert_eq!((text2.x, text2.y), (text.x, text.y));
        assert_ne!(frame, after, "the frame changed");
    }
}
