//! A canvas dedicated to an external frame producer. The 2D fallback has
//! its own backing store because canvas contexts cannot change type.
use hyve::state::on_cleanup;
use serge_core::{
    Point, View,
    viewport::{Extent, Pointer, Viewport},
};
use std::rc::Rc;
use wasm_bindgen::{Clamped, JsCast, prelude::*};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

#[wasm_bindgen(inline_js = r#"
export function webGpuProducer(device, render) {
  const configured = new WeakSet(), live = new Set();
  let lost = false;
  device.lost.then(() => {
    lost = true;
    for (const ref of live) ref.deref()?.dispatchEvent(new Event("serge-viewport-invalidated"));
    live.clear();
  });
  return (canvas, w, h) => {
    if (lost || !navigator.gpu) return false;
    const context = canvas.getContext("webgpu");
    if (!context) return false;
    if (!configured.has(canvas)) {
      context.configure({device, usage:GPUTextureUsage.RENDER_ATTACHMENT|GPUTextureUsage.COPY_SRC, format:navigator.gpu.getPreferredCanvasFormat(), colorSpace:"srgb", alphaMode:"premultiplied"});
      configured.add(canvas);
      for (const ref of live) if (!ref.deref()) live.delete(ref);
      live.add(new WeakRef(canvas));
    }
    render(device, context.getCurrentTexture(), w, h);
    return true;
  };
}
export function installViewport(host, label, maxPixels, draw, dirty, input) {
  const doc=host.ownerDocument, win=doc.defaultView;
  const canvas=doc.createElement("canvas"), fallback=doc.createElement("canvas");
  host.setAttribute("role","img");
  host.tabIndex=0;
  host.setAttribute("aria-label",label);
  for (const c of [canvas,fallback]) {
    c.style.cssText="position:absolute;inset:0;width:100%;height:100%;touch-action:none";
    host.prepend(c);
  }
  let requested=true, active=true, raf=0;
  const invalidate=()=>{requested=true;};
  canvas.addEventListener("serge-viewport-invalidated",invalidate);
  function frame() {
    if (!active) return;
    let w=Math.max(0,Math.round(host.clientWidth*win.devicePixelRatio));
    let h=Math.max(0,Math.round(host.clientHeight*win.devicePixelRatio));
    if (!Number.isSafeInteger(w*h) || w*h > maxPixels || !w || !h) w=h=0;
    if (requested || dirty() || canvas.width!==w || canvas.height!==h) {
      requested=false;
      if (canvas.width!==w || canvas.height!==h) {
        canvas.width=fallback.width=w; canvas.height=fallback.height=h;
      }
      const mode=draw(canvas,fallback,w,h);
      canvas.style.display=mode===1?"block":"none";
      fallback.style.display=mode===2?"block":"none";
      host.setAttribute("aria-label",mode===0 ? label+" (viewport unavailable)" : label);
    }
    raf=win.requestAnimationFrame(frame);
  }
  const events=["pointerdown","pointermove","pointerup","pointercancel"];
  const forward=(e)=>{
    if (e.target!==canvas && e.target!==fallback) return;
    const box=e.target.getBoundingClientRect();
    if (!box.width || !box.height) return;
    if (e.type==="pointerdown") { host.focus(); if (e.isTrusted) e.target.setPointerCapture(e.pointerId); }
    input(events.indexOf(e.type),(e.clientX-box.left)*canvas.width/box.width,(e.clientY-box.top)*canvas.height/box.height);
  };
  for (const e of events) host.addEventListener(e,forward);
  raf=win.requestAnimationFrame(frame);
  return ()=>{
    active=false; win.cancelAnimationFrame(raf);
    canvas.removeEventListener("serge-viewport-invalidated",invalidate);
    for (const e of events) host.removeEventListener(e,forward);
    canvas.remove(); fallback.remove();
  };
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name=installViewport)]
    fn install(
        host: &web_sys::Element,
        label: &str,
        max_pixels: u32,
        draw: &js_sys::Function,
        dirty: &js_sys::Function,
        input: &js_sys::Function,
    ) -> js_sys::Function;
    #[wasm_bindgen(js_name=webGpuProducer)]
    fn web_gpu(device: &JsValue, draw: &js_sys::Function) -> js_sys::Function;
}
type BrowserDraw = dyn Fn(&HtmlCanvasElement, Extent) -> bool;
/// Browser viewport backend for a producer with a WebGPU device.
pub struct WebViewport(Rc<BrowserDraw>);
impl WebViewport {
    /// Supplies the current canvas texture to a JS callback `(device,texture,width,height)`.
    pub fn webgpu(device: &JsValue, draw: &js_sys::Function) -> Self {
        let render = web_gpu(device, draw);
        Self(Rc::new(move |canvas, size| {
            render
                .call3(
                    &JsValue::NULL,
                    canvas,
                    &size.width.into(),
                    &size.height.into(),
                )
                .ok()
                .and_then(|v| v.as_bool())
                == Some(true)
        }))
    }
}
pub(super) fn mount(host: &web_sys::Element, producer: Viewport, view: &View) {
    let slot = producer.mount();
    let background = slot.fallback();
    let dirty = slot.clone();
    let dirty = Closure::<dyn Fn() -> bool>::new(move || dirty.dirty());
    let target = slot.clone();
    let view = view.clone();
    let input = Closure::<dyn Fn(u32, f32, f32)>::new(move |event, x, y| {
        let p = Point::new(x, y);
        let event = match event {
            0 => Pointer::Down(p),
            1 => Pointer::Move(p),
            2 => Pointer::Up(p),
            _ => Pointer::Cancel,
        };
        view.run(|| target.pointer(event));
    });
    let draw = Closure::<dyn Fn(HtmlCanvasElement, HtmlCanvasElement, u32, u32) -> u8>::new(
        move |gpu: HtmlCanvasElement, cpu: HtmlCanvasElement, w, h| {
            let size = Extent {
                width: w,
                height: h,
            };
            slot.frame(size);
            if size.byte_len().is_none() {
                slot.presented(false);
                return 0;
            }
            if slot
                .backend::<WebViewport>()
                .is_some_and(|draw| (draw.0)(&gpu, size))
            {
                slot.presented(true);
                return 1;
            }
            let fallback = || -> Option<()> {
                let mut bytes = slot.rgba()?;
                for p in bytes.as_chunks_mut::<4>().0 {
                    let a = u32::from(p[3]);
                    if a > 0 && a < 255 {
                        for v in &mut p[..3] {
                            *v = ((u32::from(*v) * 255 + a / 2) / a).min(255) as u8;
                        }
                    }
                }
                let ctx = cpu
                    .get_context("2d")
                    .ok()??
                    .dyn_into::<CanvasRenderingContext2d>()
                    .ok()?;
                let image =
                    ImageData::new_with_u8_clamped_array_and_sh(Clamped(&bytes), w, h).ok()?;
                ctx.put_image_data(&image, 0.0, 0.0).ok()
            };
            let ok = fallback().is_some();
            slot.presented(ok);
            if ok { 2 } else { 0 }
        },
    );
    if let Some(html) = host.dyn_ref::<web_sys::HtmlElement>() {
        let style = html.style();
        let _ = style.set_property("position", "relative");
        let _ = style.set_property(
            "background-color",
            &format!(
                "rgba({},{},{},{})",
                background.r,
                background.g,
                background.b,
                f32::from(background.a) / 255.0
            ),
        );
    }
    let cleanup = install(
        host,
        producer.label(),
        (Extent::MAX_BYTES / 4) as u32,
        draw.as_ref().unchecked_ref(),
        dirty.as_ref().unchecked_ref(),
        input.as_ref().unchecked_ref(),
    );
    on_cleanup(move || {
        let _ = cleanup.call0(&JsValue::NULL);
        drop((draw, dirty, input));
    });
}
