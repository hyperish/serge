//! Playground binary demonstrating `serge` single-syntax Web DOM + Native CPU/GPU rendering.

use hyve::prelude::*;
use serge::prelude::*;

#[component]
fn DemoCard() -> Element {
    let mut count = signal(0);
    ui! {
        Card { padding: 16px, gap: 12px,
            Text { "Count: {count()}" }
            Button {
                text: "Increment",
                clicked => { count += 1; }
            }
        }
    }
}

fn main() {
    let card = DemoCard();
    let mut dom = DomTemplate::compile(&card).mount();
    let _ = card.click_first_button("Increment");
    let patches = dom.patch_dirty();

    let batch = layout_and_paint(&card, 320, 200, ColorTokens::dark());
    let gpu = GpuBatchRenderer::prepare_frame(&batch);
    let cpu = CpuFallbackRenderer::render(&batch);
    let backend = select_render_backend(true);

    println!(
        "serge-playground ready: html={} patches={} backend={:?} draw_calls={} cpu_pixels={}",
        dom.current_html(),
        patches.len(),
        backend,
        gpu.draw_calls,
        cpu.painted_pixel_count()
    );
}
