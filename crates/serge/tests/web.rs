//! The contract examples on the web target, in a real browser
//! (`wasm-bindgen-test`, headless Chrome): signals update the DOM, regions
//! and keyed lists follow them, events reach handlers, canvases paint, and
//! the router follows links and the browser history.
#![cfg(target_arch = "wasm32")]
#![allow(missing_docs)]

use hyve::prelude::*;
use serge::prelude::*;
use serge::testing::mount;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;
use web_sys::{Element as DomElement, HtmlElement, HtmlInputElement};

wasm_bindgen_test_configure!(run_in_browser);

fn container() -> DomElement {
    let doc = web_sys::window().unwrap().document().unwrap();
    let el = doc.create_element("div").unwrap();
    doc.body().unwrap().append_child(&el).unwrap();
    el
}

fn texts(root: &DomElement, selector: &str) -> Vec<String> {
    let list = root.query_selector_all(selector).unwrap();
    (0..list.length())
        .filter_map(|i| list.item(i))
        .map(|n| n.text_content().unwrap_or_default())
        .collect()
}

fn find(root: &DomElement, selector: &str, text: &str) -> HtmlElement {
    let list = root.query_selector_all(selector).unwrap();
    (0..list.length())
        .filter_map(|i| list.item(i))
        .find(|n| n.text_content().as_deref() == Some(text))
        .unwrap_or_else(|| panic!("no {selector} shows {text:?}"))
        .unchecked_into()
}

async fn sleep(ms: i32) {
    let p = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .unwrap();
    });
    wasm_bindgen_futures::JsFuture::from(p).await.unwrap();
}

#[component]
pub fn CounterCard() -> Element {
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

#[wasm_bindgen_test]
fn the_counter_counts_in_the_dom() {
    let c = container();
    let m = mount(&c, Scope::root(), || ui! { CounterCard {} });
    assert_eq!(texts(&c, ".serge-text"), ["Count: 0"]);
    let label = c.query_selector(".serge-text").unwrap().unwrap();
    let text_node = label.first_child().unwrap();
    let b = find(&c, ".serge-button", "Increment");
    b.click();
    b.click();
    assert_eq!(texts(&c, ".serge-text"), ["Count: 2"]);
    assert!(text_node.is_same_node(label.first_child().as_ref()));
    // Real elements with the card's style.
    let card: HtmlElement = c
        .query_selector(".serge-card")
        .unwrap()
        .unwrap()
        .unchecked_into();
    assert!(
        card.get_attribute("style")
            .unwrap()
            .contains("padding: 16px 16px")
    );
    m.unmount();
    assert_eq!(c.child_element_count(), 0);
}

#[derive(Clone, PartialEq, Debug)]
pub struct Todo {
    pub id: u32,
    pub title: String,
    pub done: bool,
}

#[component]
pub fn Todos() -> Element {
    let todos = signal(vec![
        Todo {
            id: 1,
            title: "write spec".into(),
            done: false,
        },
        Todo {
            id: 2,
            title: "ship".into(),
            done: false,
        },
    ]);
    let draft = signal(String::new());
    let next = signal(3u32);
    let show_done = signal(true);
    ui! {
        Stack {
            Input {
                value: draft(),
                placeholder: "New task",
                changed(t) => { draft.set(t); },
                submitted => {
                    let title = draft.get();
                    if !title.is_empty() {
                        todos.update(|v| v.push(Todo { id: next.get(), title, done: false }));
                        next.update(|n| *n += 1);
                        draft.set(String::new());
                    }
                }
            }
            for todo in todos() {
                key: todo.id,
                if show_done() || !todo.done {
                    Checkbox {
                        text: todo.title.clone(),
                        checked: todo.done,
                        changed(c) => {
                            let id = todo.id;
                            todos.update(|v| if let Some(t) = v.iter_mut().find(|t| t.id == id) { t.done = c; });
                        }
                    }
                }
            }
            Button {
                text: if show_done() { "Hide done" } else { "Show done" },
                clicked => { show_done.set(!show_done.get()); }
            }
            Button { text: "Reverse", clicked => { todos.update(|v| v.reverse()); } }
            Text { "{todos.with(|v| v.iter().filter(|t| !t.done).count())} left" }
        }
    }
}

fn type_and_enter(input: &HtmlInputElement, text: &str) {
    input.set_value(text);
    input
        .dispatch_event(&web_sys::Event::new("input").unwrap())
        .unwrap();
    let init = web_sys::KeyboardEventInit::new();
    init.set_key("Enter");
    let e = web_sys::KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap();
    input.dispatch_event(&e).unwrap();
}

#[wasm_bindgen_test]
fn regions_keyed_lists_and_inputs_follow_signals() {
    let c = container();
    let _m = mount(&c, Scope::root(), || ui! { Todos {} });
    let labels = || texts(&c, ".serge-checkbox span");
    assert_eq!(labels(), ["write spec", "ship"]);
    let input: HtmlInputElement = c
        .query_selector(".serge-input")
        .unwrap()
        .unwrap()
        .unchecked_into();
    assert_eq!(input.placeholder(), "New task");
    type_and_enter(&input, "test it");
    assert_eq!(labels(), ["write spec", "ship", "test it"]);
    assert_eq!(input.value(), "", "the draft was cleared");
    assert_eq!(texts(&c, ".serge-text"), ["3 left"]);

    // Toggling a checkbox (a real click) updates the count.
    let ship: HtmlElement = find(&c, ".serge-checkbox", "ship");
    let check: HtmlElement = ship
        .query_selector("input")
        .unwrap()
        .unwrap()
        .unchecked_into();
    check.click();
    assert_eq!(texts(&c, ".serge-text"), ["2 left"]);

    // Keyed items whose value is unchanged move instead of being rebuilt
    // (a changed value under the same key, like the toggle above, rebuilds
    // that item).
    let ship_node = find(&c, ".serge-checkbox", "ship");
    find(&c, ".serge-button", "Reverse").click();
    assert_eq!(labels(), ["test it", "ship", "write spec"]);
    let ship_again = find(&c, ".serge-checkbox", "ship");
    assert!(
        ship_again.is_same_node(Some(&ship_node)),
        "the same DOM node was moved"
    );

    find(&c, ".serge-button", "Hide done").click();
    assert_eq!(labels(), ["test it", "write spec"]);
    find(&c, ".serge-button", "Show done").click();
    assert_eq!(labels(), ["test it", "ship", "write spec"]);
}

#[component]
pub fn Exposure() -> Element {
    let exposure = signal(0.0_f32);
    ui! {
        Card {
            Text { "Exposure: {exposure():.2}" }
            Slider { range: -5.0..=5.0, value: exposure(), changed(v) => { exposure.set(v); } }
            Canvas {
                height: 64px,
                paint(bounds, ctx) => {
                    let w = bounds.width * (exposure() + 5.0) / 10.0;
                    ctx.fill_rect(Rect::new(0.0, 0.0, w, bounds.height), Color::hex(0x3b5bdb));
                }
            }
        }
    }
}

fn pixel(canvas: &web_sys::HtmlCanvasElement, x: f64, y: f64) -> [u8; 4] {
    let ctx: web_sys::CanvasRenderingContext2d =
        canvas.get_context("2d").unwrap().unwrap().unchecked_into();
    let d = ctx.get_image_data(x, y, 1.0, 1.0).unwrap().data();
    [d[0], d[1], d[2], d[3]]
}

#[wasm_bindgen_test]
async fn sliders_drive_text_and_canvas_paint() {
    let c = container();
    let _m = mount(&c, Scope::root(), || ui! { Exposure {} });
    assert_eq!(texts(&c, ".serge-text"), ["Exposure: 0.00"]);
    let canvas: web_sys::HtmlCanvasElement = c
        .query_selector("canvas")
        .unwrap()
        .unwrap()
        .unchecked_into();
    // Wait for the first ResizeObserver paint, including on a software GPU.
    for _ in 0..60 {
        let (w, h) = (f64::from(canvas.width()), f64::from(canvas.height()));
        if w > 0.0 && h > 0.0 && pixel(&canvas, w * 0.25, h * 0.5) == [0x3b, 0x5b, 0xdb, 255] {
            break;
        }
        sleep(16).await;
    }
    assert!(canvas.width() > 0, "sized");
    let w = f64::from(canvas.width());
    let y = f64::from(canvas.height()) / 2.0;
    assert_eq!(pixel(&canvas, w * 0.25, y), [0x3b, 0x5b, 0xdb, 255]);
    assert_eq!(pixel(&canvas, w * 0.75, y)[3], 0, "right half empty");

    let slider: HtmlInputElement = c
        .query_selector(".serge-slider")
        .unwrap()
        .unwrap()
        .unchecked_into();
    slider.set_value("5");
    slider
        .dispatch_event(&web_sys::Event::new("input").unwrap())
        .unwrap();
    assert_eq!(texts(&c, ".serge-text"), ["Exposure: 5.00"]);
    assert_eq!(
        pixel(&canvas, w * 0.75, y),
        [0x3b, 0x5b, 0xdb, 255],
        "repainted"
    );
}

#[component]
pub fn Home() -> Element {
    ui! {
        Stack {
            Text { "Home" }
            Link { to: "/projects/7?tab=files", "Open project 7" }
        }
    }
}

#[component]
pub fn ProjectScreen(id: Path<u32>) -> Element {
    let id = *id;
    ui! {
        Stack {
            Text { "Project {id}" }
            Button { text: "Back", clicked => { back(); } }
        }
    }
}

#[wasm_bindgen_test]
async fn links_and_history_drive_the_router() {
    let start = web_sys::window().unwrap().location().pathname().unwrap();
    let app = SergeApp::new()
        .feature(
            feature()
                .route("/", Home)
                .route("/projects/{id}", ProjectScreen),
        )
        .start_path("/")
        .assemble()
        .unwrap();
    let c = container();
    let _m = mount(&c, app.scope.clone(), move || app.element());
    assert_eq!(texts(&c, ".serge-text"), ["Home"]);
    find(&c, ".serge-link", "Open project 7").click();
    assert_eq!(texts(&c, ".serge-text"), ["Project 7"]);
    find(&c, ".serge-button", "Back").click();
    assert_eq!(texts(&c, ".serge-text"), ["Home"]);
    // The test page's own address is untouched (the memory history is used
    // without `serge_web_dom::run`).
    assert_eq!(
        web_sys::window().unwrap().location().pathname().unwrap(),
        start
    );
}

#[model]
#[derive(Debug, Clone, PartialEq)]
pub struct Wait {
    pub millis: u32,
}

/// In this (client) build only the call stub remains.
#[server(POST "/slow")]
pub async fn slow_call(input: Wait) -> HyveResult<String> {
    Ok(format!("{}", input.millis))
}

#[component]
pub fn SlowButton() -> Element {
    let status = signal(String::from("idle"));
    ui! {
        Stack {
            Text { "{status()}" }
            Button {
                text: "Call",
                clicked => async move {
                    status.set("calling".into());
                    let r = slow_call(Wait { millis: 5000 }).await;
                    status.set(format!("{r:?}"));
                }
            }
        }
    }
}

#[wasm_bindgen_test]
async fn unmounting_a_view_aborts_its_fetch() {
    // A fetch that never answers and records its abort signal.
    js_sys::eval(
        "window.__serge_fetch = window.fetch; window.__aborted = false; window.__url = null;
         window.fetch = (req) => new Promise((_, reject) => {
           window.__url = req.url;
           req.signal.addEventListener('abort', () => { window.__aborted = true; reject(new DOMException('aborted', 'AbortError')); });
         });",
    )
    .unwrap();
    let scope = Scope::root();
    hyve::api::provide_transport(&scope, serge::RemoteTransport::new("/v1").into_arc());
    let c = container();
    let m = mount(&c, scope, || ui! { SlowButton {} });
    find(&c, ".serge-button", "Call").click();
    sleep(20).await;
    assert_eq!(texts(&c, ".serge-text"), ["calling"]);
    let url = js_sys::eval("window.__url").unwrap().as_string().unwrap();
    assert!(url.ends_with("/v1/slow"), "{url}");
    assert_eq!(
        js_sys::eval("window.__aborted").unwrap().as_bool(),
        Some(false)
    );
    m.unmount();
    sleep(20).await;
    let aborted = js_sys::eval("window.__aborted").unwrap().as_bool();
    js_sys::eval("window.fetch = window.__serge_fetch;").unwrap();
    assert_eq!(
        aborted,
        Some(true),
        "unmounting aborted the in-flight fetch"
    );
}

#[derive(Clone, PartialEq)]
struct Row {
    id: u32,
    name: String,
    done: bool,
}

/// List items are clones of one template per place: creating a hundred rows
/// creates no element per row, and every clone still gets its own text,
/// per-row values written as variables, live input state and handlers.
#[wasm_bindgen_test]
fn list_items_are_cloned_from_templates() {
    let root = container();
    let rows = signal(
        (0..100)
            .map(|id| Row {
                id,
                name: format!("row {id}"),
                done: id % 2 == 0,
            })
            .collect::<Vec<_>>(),
    );
    let clicked = signal(None::<u32>);
    let m = mount(&root, Scope::root(), move || {
        ui! {
            Stack { gap: 4px,
                for row in rows() { key: row.id,
                    Card { padding: 8px, gap: 4px,
                        Text { "{row.name}" }
                        Checkbox { text: "Done", checked: row.done }
                        Input { value: "draft", placeholder: "Note" }
                        Button { text: "Pick", clicked => { clicked.set(Some(row.id)); } }
                    }
                }
            }
        }
    });
    let s = m.stats();
    // The Stack, then the row template: Card, Text, Checkbox, Input, Button.
    assert_eq!(s.templates, 2, "{s:?}");
    assert_eq!(s.created, 6, "{s:?}");
    assert_eq!(s.cloned, 101, "{s:?}");
    let names = texts(&root, ".serge-text");
    assert_eq!(names.len(), 100);
    assert_eq!(names[7], "row 7");
    let boxes = root.query_selector_all(".serge-checkbox input").unwrap();
    let checked = |i: u32| -> bool {
        boxes
            .item(i)
            .unwrap()
            .unchecked_into::<HtmlInputElement>()
            .checked()
    };
    assert!(checked(0) && !checked(1) && checked(2));
    assert_eq!(texts(&root, ".serge-checkbox span")[3], "Done");
    let inputs = root.query_selector_all(".serge-input").unwrap();
    let input: HtmlInputElement = inputs.item(42).unwrap().unchecked_into();
    assert_eq!(input.value(), "draft");
    assert_eq!(input.placeholder(), "Note");
    // Literal styles are in the template.
    let card: HtmlElement = root
        .query_selector(".serge-card")
        .unwrap()
        .unwrap()
        .unchecked_into();
    assert!(
        card.get_attribute("style").unwrap().contains("padding"),
        "{:?}",
        card.get_attribute("style")
    );
    // Handlers are per clone.
    let buttons = root.query_selector_all(".serge-button").unwrap();
    buttons
        .item(13)
        .unwrap()
        .unchecked_into::<HtmlElement>()
        .click();
    assert_eq!(clicked.get(), Some(13));
    // New rows are clones too.
    rows.update(|r| {
        r.push(Row {
            id: 100,
            name: "row 100".into(),
            done: false,
        })
    });
    let s = m.stats();
    assert_eq!((s.created, s.cloned), (6, 102), "{s:?}");
    assert_eq!(texts(&root, ".serge-text").last().unwrap(), "row 100");
    m.unmount();
}

/// Values written as variables differ between widgets built at the same
/// place, so they are applied to each clone, not baked into the template.
#[wasm_bindgen_test]
fn per_instance_values_are_not_baked() {
    let root = container();
    let _m = mount(&root, Scope::root(), move || {
        let item = |label: &'static str, pad: serge::Length| ui! { Card { padding: pad, Text { "{label}" } } };
        ui! {
            Stack {
                {item("small", serge::px(2.0))}
                {item("large", serge::px(30.0))}
            }
        }
    });
    let cards = root.query_selector_all(".serge-card").unwrap();
    let style = |i| {
        cards
            .item(i)
            .unwrap()
            .unchecked_into::<DomElement>()
            .get_attribute("style")
            .unwrap()
    };
    assert!(style(0).contains("padding: 2px"), "{}", style(0));
    assert!(style(1).contains("padding: 30px"), "{}", style(1));
    assert_eq!(texts(&root, ".serge-text"), ["small", "large"]);
}

#[cfg(feature = "ionic")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function adaptiveFixture() {
    const frame = document.createElement('iframe');
    frame.style.cssText = 'width:960px;height:480px;border:0';
    document.body.append(frame);
    const root = frame.contentDocument.createElement('div');
    frame.contentDocument.body.append(root);
    return root;
}
export function layoutOf(root) {
    const win = root.ownerDocument.defaultView;
    return win.getComputedStyle(root.querySelector('.serge-ionic-shell')).gridTemplateAreas;
}
export function resizeRoot(root) { root.ownerDocument.defaultView.frameElement.style.width='390px'; }
export function keyIn(root, key, shift) {
    const win = root.ownerDocument.defaultView;
    root.ownerDocument.activeElement.dispatchEvent(new win.KeyboardEvent('keydown',{key,shiftKey:shift,bubbles:true,cancelable:true}));
}
export function swipe(root) {
    const win=root.ownerDocument.defaultView, el=root.querySelector('.serge-ionic-grip');
    for (const [name,y] of [['pointerdown',12],['pointerup',100]]) {
        el.dispatchEvent(new win.PointerEvent(name,{isPrimary:true,pointerId:4,clientX:80,clientY:y,bubbles:true}));
    }
}
export function removeFrame(root) { root.ownerDocument.defaultView.frameElement.remove(); }
"#)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name=adaptiveFixture)]
    fn adaptive_fixture() -> DomElement;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name=layoutOf)]
    fn layout_of(root: &DomElement) -> String;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name=resizeRoot)]
    fn resize_root(root: &DomElement);
    #[wasm_bindgen::prelude::wasm_bindgen(js_name=keyIn)]
    fn key_in(root: &DomElement, key: &str, shift: bool);
    fn swipe(root: &DomElement);
    #[wasm_bindgen::prelude::wasm_bindgen(js_name=removeFrame)]
    fn remove_frame(root: &DomElement);
}

#[cfg(feature = "ionic")]
#[wasm_bindgen_test(async)]
async fn adaptive_navigation_and_sheet_preserve_dom_focus_and_actions() {
    let root = adaptive_fixture();
    let mounted = mount(&root, Scope::root(), || {
        let open = signal(false);
        serge::ionic::shell(
            ui! {Text{"Notes"}},
            ui! {View{
                Button{text:"Edit", clicked=>{open.set(true);}}
                if open.get() { {serge::ionic::sheet("Edit note","Close",ui!{ Input{ placeholder:"Title" } },move ||open.set(false))} }
            }},
        )
    });
    assert_eq!(layout_of(&root), r#""nav main""#);
    let nav = root.query_selector("[role=navigation]").unwrap().unwrap();
    resize_root(&root);
    sleep(30).await;
    assert_eq!(layout_of(&root), r#""main" "nav""#);
    assert!(nav.is_same_node(Some(
        &root.query_selector("[role=navigation]").unwrap().unwrap()
    )));
    let trigger: HtmlElement = root
        .query_selector("button")
        .unwrap()
        .unwrap()
        .unchecked_into();
    trigger.focus().unwrap();
    trigger.click();
    sleep(20).await;
    let doc = root.owner_document().unwrap();
    assert_eq!(doc.active_element().unwrap().tag_name(), "INPUT");
    key_in(&root, "Tab", true);
    assert_eq!(
        doc.active_element().unwrap().text_content().unwrap(),
        "Close"
    );
    key_in(&root, "Tab", false);
    assert_eq!(doc.active_element().unwrap().tag_name(), "INPUT");
    key_in(&root, "Escape", false);
    sleep(20).await;
    assert!(root.query_selector("[role=dialog]").unwrap().is_none());
    assert!(trigger.is_same_node(doc.active_element().as_deref()));
    trigger.click();
    sleep(20).await;
    swipe(&root);
    sleep(20).await;
    assert!(root.query_selector("[role=dialog]").unwrap().is_none());
    mounted.unmount();
    assert_eq!(root.get_attribute("style").unwrap_or_default(), "");
    remove_frame(&root);
}

#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export async function testViewportDevice() {
  const adapter=await navigator.gpu?.requestAdapter();
  return adapter ? await adapter.requestDevice() : null;
}
export function testViewportDraw() {
  let resolvePixel, rejectPixel;
  const pixel = new Promise((resolve, reject) => { resolvePixel = resolve; rejectPixel = reject; });
  const draw = (device, texture)=>{
    const encoder=device.createCommandEncoder();
    const pass=encoder.beginRenderPass({colorAttachments:[{view:texture.createView(),clearValue:{r:1,g:0,b:0,a:1},loadOp:"clear",storeOp:"store"}]});
    pass.end();
    const buffer=device.createBuffer({size:256,usage:GPUBufferUsage.COPY_DST|GPUBufferUsage.MAP_READ});
    encoder.copyTextureToBuffer({texture},{buffer,bytesPerRow:256},[1,1]);
    device.queue.submit([encoder.finish()]);
    buffer.mapAsync(GPUMapMode.READ).then(()=>{
      const p=new Uint8Array(buffer.getMappedRange());
      resolvePixel(texture.format.startsWith("bgra")?p[2]:p[0]);
      buffer.unmap();buffer.destroy();
    }, rejectPixel);
  };
  draw.pixel = pixel;
  return draw;
}
export async function testViewportPixel(draw) {
  let timer;
  const deadline = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error("timed out waiting for WebGPU pixel readback")), 10000);
  });
  try { return await Promise.race([draw.pixel, deadline]); }
  finally { clearTimeout(timer); }
}
export function testViewportDestroy(device){device.destroy();}
"#)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name=testViewportDevice)]
    fn viewport_device() -> js_sys::Promise;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name=testViewportDraw)]
    fn viewport_draw() -> js_sys::Function;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name=testViewportPixel)]
    fn viewport_pixel(draw: &js_sys::Function) -> js_sys::Promise;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name=testViewportDestroy)]
    fn viewport_destroy(device: &wasm_bindgen::JsValue);
}

#[wasm_bindgen_test]
async fn external_viewport_webgpu_and_cpu_fallback_follow_resize_and_unmount() {
    use serge::viewport::{StyleProp, Viewport, WebViewport};
    let root = container();
    let device = wasm_bindgen_futures::JsFuture::from(viewport_device())
        .await
        .unwrap();
    let mut producer =
        Viewport::new("3D preview").cpu(|size| Some([0, 100, 0, 255].repeat(size.byte_len()? / 4)));
    let gpu = !device.is_null();
    let draw = viewport_draw();
    if gpu {
        producer = producer.backend(WebViewport::webgpu(&device, &draw));
    } else if option_env!("SERGE_REQUIRE_WEBGPU") == Some("1") {
        panic!("WebGPU device required");
    }
    let m = mount(&root, Scope::root(), move || {
        producer.element([StyleProp::Width(px(80.)), StyleProp::Height(px(40.))])
    });
    let host = root
        .query_selector("[role=img]")
        .unwrap()
        .unwrap()
        .dyn_into::<HtmlElement>()
        .unwrap();
    let fallback = host
        .query_selector("canvas")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .unwrap();
    if gpu {
        let pixel = wasm_bindgen_futures::JsFuture::from(viewport_pixel(&draw))
            .await
            .expect("read back the first rendered WebGPU pixel");
        assert_eq!(pixel.as_f64(), Some(255.0));
        viewport_destroy(&device);
    }
    for _ in 0..60 {
        sleep(16).await;
        if fallback.style().get_property_value("display").unwrap() == "block" {
            break;
        }
    }
    assert_eq!(pixel(&fallback, 1., 1.), [0, 100, 0, 255]);
    host.style().set_property("width", "110px").unwrap();
    sleep(70).await;
    assert_eq!(
        fallback.width(),
        (110. * web_sys::window().unwrap().device_pixel_ratio()).round() as u32
    );
    assert_eq!(
        host.get_attribute("aria-label").as_deref(),
        Some("3D preview")
    );
    m.unmount();
    assert!(!fallback.is_connected());
    root.remove();
}
