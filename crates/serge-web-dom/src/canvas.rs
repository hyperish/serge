//! `Canvas` on the web: a sized box with a `<canvas>` inside, repainted with
//! the 2D context when the paint's signals, its size or a frame source it
//! read change.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use hyve::state::{effect, on_cleanup, signal};
use serge_core::{FrameSource, PaintCmd, PaintFn, Point, Rect, View};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use crate::{CanvasFrame, paint_into};

type Sources = Rc<RefCell<Vec<(FrameSource, u64)>>>;
type FrameCallback = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;

struct Live {
    sources: Sources,
    frame: CanvasFrame,
    alive: Rc<Cell<bool>>,
}

thread_local! {
    static LIVE: RefCell<Vec<Live>> = const { RefCell::new(Vec::new()) };
    static LOOP: Cell<bool> = const { Cell::new(false) };
}

/// Bumps canvases whose frame sources changed, once per animation frame,
/// while any canvas is mounted.
fn frame_loop() {
    if LOOP.with(|l| l.replace(true)) {
        return;
    }
    let cb: FrameCallback = Rc::default();
    let cb2 = cb.clone();
    *cb.borrow_mut() = Some(Closure::new(move || {
        let ticks: Vec<_> = LIVE.with(|l| {
            let mut l = l.borrow_mut();
            l.retain(|c| c.alive.get());
            l.iter()
                .filter(|c| serge_core::paint::sources_changed(&c.sources.borrow()))
                .map(|c| c.frame.tick)
                .collect()
        });
        for t in ticks {
            t.update(|n| *n += 1);
        }
        let more = LIVE.with(|l| !l.borrow().is_empty());
        if more {
            if let (Some(w), Some(c)) = (web_sys::window(), cb2.borrow().as_ref()) {
                let _ = w.request_animation_frame(c.as_ref().unchecked_ref());
            }
        } else {
            LOOP.with(|l| l.set(false));
            cb2.borrow_mut().take();
        }
    }));
    if let (Some(w), Some(c)) = (web_sys::window(), cb.borrow().as_ref()) {
        let _ = w.request_animation_frame(c.as_ref().unchecked_ref());
    }
}

pub(crate) fn mount(host: &web_sys::Element, view: &View, paint: PaintFn) {
    let doc = host.owner_document().expect("document");
    let canvas: HtmlCanvasElement = doc
        .create_element("canvas")
        .expect("canvas")
        .unchecked_into();
    let _ = host.append_child(&canvas);
    let frame = CanvasFrame {
        bounds: signal(Rect::default()),
        tick: signal(0u64),
    };
    let sources: Sources = Rc::default();

    // Size: the host box, observed.
    let bounds = frame.bounds;
    let host2 = host.clone();
    let on_resize = Closure::<dyn FnMut()>::new(move || {
        let w = host2.client_width() as f32;
        let h = host2.client_height() as f32;
        let r = Rect::new(0.0, 0.0, w, h);
        if bounds.get_untracked() != r {
            bounds.set(r);
        }
    });
    if let Ok(obs) = web_sys::ResizeObserver::new(on_resize.as_ref().unchecked_ref()) {
        obs.observe(host);
        on_cleanup(move || {
            obs.disconnect();
            drop(on_resize);
        });
    }

    let alive = Rc::new(Cell::new(true));
    let a2 = alive.clone();
    on_cleanup(move || a2.set(false));
    LIVE.with(|l| {
        l.borrow_mut().push(Live {
            sources: sources.clone(),
            frame: CanvasFrame {
                bounds: frame.bounds,
                tick: frame.tick,
            },
            alive,
        })
    });
    frame_loop();

    let v = view.clone();
    effect(move || paint_into(&v, &paint, &canvas, &frame, &sources));
}

fn css(c: serge_style::Color) -> String {
    c.css()
}

fn path(ctx: &CanvasRenderingContext2d, points: &[Point], close: bool) {
    ctx.begin_path();
    for (i, p) in points.iter().enumerate() {
        if i == 0 {
            ctx.move_to(p.x.into(), p.y.into());
        } else {
            ctx.line_to(p.x.into(), p.y.into());
        }
    }
    if close {
        ctx.close_path();
    }
}

fn rounded(ctx: &CanvasRenderingContext2d, r: Rect, radius: f32) {
    let rad = f64::from(radius.min(r.width / 2.0).min(r.height / 2.0));
    let (x, y, w, h) = (
        f64::from(r.x),
        f64::from(r.y),
        f64::from(r.width),
        f64::from(r.height),
    );
    ctx.begin_path();
    ctx.move_to(x + rad, y);
    let _ = ctx.arc_to(x + w, y, x + w, y + h, rad);
    let _ = ctx.arc_to(x + w, y + h, x, y + h, rad);
    let _ = ctx.arc_to(x, y + h, x, y, rad);
    let _ = ctx.arc_to(x, y, x + w, y, rad);
    ctx.close_path();
}

/// Draws `commands` into `canvas` sized to `bounds` (CSS pixels) at the
/// device pixel ratio.
pub(crate) fn draw(canvas: &HtmlCanvasElement, bounds: Rect, commands: &[PaintCmd]) {
    let dpr = web_sys::window().map_or(1.0, |w| w.device_pixel_ratio());
    let (pw, ph) = (
        (f64::from(bounds.width) * dpr).round() as u32,
        (f64::from(bounds.height) * dpr).round() as u32,
    );
    if canvas.width() != pw {
        canvas.set_width(pw);
    }
    if canvas.height() != ph {
        canvas.set_height(ph);
    }
    let Some(ctx) = canvas
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|c| c.dyn_into::<CanvasRenderingContext2d>().ok())
    else {
        return;
    };
    let _ = ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
    ctx.clear_rect(0.0, 0.0, bounds.width.into(), bounds.height.into());
    for cmd in commands {
        match cmd {
            PaintCmd::FillRect {
                rect,
                color,
                radius,
            } => {
                ctx.set_fill_style_str(&css(*color));
                if *radius > 0.0 {
                    rounded(&ctx, *rect, *radius);
                    ctx.fill();
                } else {
                    ctx.fill_rect(
                        rect.x.into(),
                        rect.y.into(),
                        rect.width.into(),
                        rect.height.into(),
                    );
                }
            }
            PaintCmd::StrokeRect { rect, color, width } => {
                ctx.set_stroke_style_str(&css(*color));
                ctx.set_line_width((*width).into());
                let r = rect.inset(width / 2.0);
                ctx.stroke_rect(r.x.into(), r.y.into(), r.width.into(), r.height.into());
            }
            PaintCmd::Polyline {
                points,
                color,
                width,
            } => {
                ctx.set_stroke_style_str(&css(*color));
                ctx.set_line_width((*width).into());
                ctx.set_line_cap("round");
                ctx.set_line_join("round");
                path(&ctx, points, false);
                ctx.stroke();
            }
            PaintCmd::FillPolygon { points, color } => {
                ctx.set_fill_style_str(&css(*color));
                path(&ctx, points, true);
                ctx.fill();
            }
            PaintCmd::Text {
                origin,
                text,
                size,
                color,
            } => {
                ctx.set_fill_style_str(&css(*color));
                ctx.set_font(&format!("{size}px system-ui, -apple-system, sans-serif"));
                ctx.set_text_baseline("top");
                let _ = ctx.fill_text(text, origin.x.into(), origin.y.into());
            }
        }
    }
}
