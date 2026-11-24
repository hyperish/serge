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
    let b = find(&c, ".serge-button", "Increment");
    b.click();
    b.click();
    assert_eq!(texts(&c, ".serge-text"), ["Count: 2"]);
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
    sleep(50).await; // the canvas is sized by a ResizeObserver
    let canvas: web_sys::HtmlCanvasElement = c
        .query_selector("canvas")
        .unwrap()
        .unwrap()
        .unchecked_into();
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
