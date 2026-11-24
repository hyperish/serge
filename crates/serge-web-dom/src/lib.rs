//! `serge-web-dom` — the web target: mounts `Element`s as real DOM and keeps
//! it in sync with signals at the finest grain the tree describes (one
//! effect per dynamic property; `if` regions and keyed `for` lists between
//! comment markers). Canvases draw their `PaintCtx` commands with the 2D
//! context; the router uses the browser history.
//!
//! Phase 1 prototype limits: nodes are created with DOM calls (template
//! cloning is not done yet), and the theme is chosen once from
//! `prefers-color-scheme`.
#![cfg(target_arch = "wasm32")]

mod canvas;
mod exec;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use hyve::Scope;
use hyve::state::{Signal, effect, on_cleanup, untrack};
use serge_core::paint::track_frame_sources;
use serge_core::{
    Attr, AttrValue, Element, Handler, Kind, Node, PaintCtx, Prop, Rect, View, Widget,
};
use serge_style::Theme;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{Document, HtmlElement, HtmlInputElement};

pub use exec::WebExecutor;

const BASE_CSS: &str = r#"
html, body { margin: 0; padding: 0; }
body {
  background: var(--serge-background); color: var(--serge-text);
  font-family: system-ui, -apple-system, "Segoe UI", Roboto, "Noto Sans KR", "Apple SD Gothic Neo", sans-serif;
  font-size: 16px; line-height: 1.25; -webkit-font-smoothing: antialiased;
}
#serge-root { display: flex; flex-direction: column; min-height: 100vh; box-sizing: border-box; }
.serge-button { cursor: pointer; font: inherit; line-height: 1.25; }
.serge-button:hover { filter: brightness(1.08); }
.serge-button:active { filter: brightness(0.92); }
.serge-button:disabled { opacity: 0.5; cursor: default; filter: none; }
.serge-input { font: inherit; outline: none; }
.serge-input:focus { border-color: var(--serge-primary) !important; box-shadow: 0 0 0 1px var(--serge-primary); }
.serge-input::placeholder { color: var(--serge-muted); }
.serge-slider { accent-color: var(--serge-primary); margin: 0; cursor: pointer; }
.serge-checkbox { cursor: pointer; user-select: none; }
.serge-checkbox input { accent-color: var(--serge-primary); width: 18px; height: 18px; margin: 0; cursor: pointer; }
.serge-link { cursor: pointer; text-decoration: none; }
.serge-link:hover { text-decoration: underline; }
.serge-progress > div { height: 100%; border-radius: inherit; background: var(--serge-primary); }
.serge-canvas { position: relative; min-width: 0; }
.serge-canvas > canvas { position: absolute; inset: 0; width: 100%; height: 100%; display: block; }
"#;

fn document() -> Document {
    web_sys::window()
        .and_then(|w| w.document())
        .expect("serge: no document")
}

/// The theme the page prefers.
pub fn preferred_theme() -> Theme {
    let dark = web_sys::window()
        .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
        .is_some_and(|m| m.matches());
    if dark { Theme::DARK } else { Theme::LIGHT }
}

/// Adds the base stylesheet and theme variables once.
fn install_styles(theme: &Theme) {
    let doc = document();
    if doc.get_element_by_id("serge-style").is_some() {
        return;
    }
    let style = doc.create_element("style").expect("style");
    style.set_id("serge-style");
    style.set_text_content(Some(&format!(
        ":root {{ {} color-scheme: {}; }}\n{BASE_CSS}",
        theme.css_variables(),
        if theme.background.r < 128 {
            "dark"
        } else {
            "light"
        }
    )));
    if let Some(head) = doc.head() {
        let _ = head.append_child(&style);
    }
}

struct Ctx {
    doc: Document,
    theme: Theme,
}

/// A mounted UI.
#[derive(Debug)]
pub struct Mounted {
    view: View,
    root: web_sys::Element,
}

impl Mounted {
    /// The element the UI is mounted in.
    pub fn root(&self) -> &web_sys::Element {
        &self.root
    }

    /// Unmounts: disposes the views (tasks and calls are cancelled) and
    /// removes the nodes.
    pub fn unmount(&self) {
        self.view.dispose();
        self.root.set_inner_html("");
    }
}

/// Installs the executor (once per page).
fn install_executor() {
    thread_local!(static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    if !DONE.with(|d| d.replace(true)) {
        serge_core::install_executor(Rc::new(WebExecutor));
    }
}

/// Mounts `root()` into `container` under `scope`.
pub fn mount(
    container: &web_sys::Element,
    scope: Scope,
    root: impl FnOnce() -> Element,
) -> Mounted {
    install_executor();
    let theme = preferred_theme();
    install_styles(&theme);
    let cx = Rc::new(Ctx {
        doc: document(),
        theme,
    });
    let view = View::root(scope);
    let parent: web_sys::Node = container.clone().into();
    view.run(|| {
        let el = untrack(root);
        mount_node(&cx, el.0, &parent, None, &view);
    });
    Mounted {
        view,
        root: container.clone(),
    }
}

/// The page's root element (`#serge-root`, created in `<body>` if missing).
pub fn page_root() -> web_sys::Element {
    let doc = document();
    if let Some(e) = doc.get_element_by_id("serge-root") {
        return e;
    }
    let e = doc.create_element("div").expect("div");
    e.set_id("serge-root");
    doc.body().expect("body").append_child(&e).expect("append");
    e
}

/// Runs an assembled app in the page: browser history, title, and the
/// router's screens in `#serge-root`. The app stays mounted for the page's
/// lifetime.
pub fn run(app: serge_router::Assembled) {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&format!("serge: {info}").into());
    }));
    let doc = document();
    doc.set_title(&app.title);
    let history = Rc::new(BrowserHistory);
    let app = app.with_history(history);
    let router = app.router();
    let on_pop = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        router.set_location(&BrowserHistory::address());
    });
    if let Some(w) = web_sys::window() {
        let _ = w.add_event_listener_with_callback("popstate", on_pop.as_ref().unchecked_ref());
    }
    on_pop.forget();
    let scope = app.scope.clone();
    let mounted = mount(&page_root(), scope, move || app.element());
    std::mem::forget(mounted);
}

/// The browser's history (`pushState`, `popstate`).
#[derive(Debug)]
pub struct BrowserHistory;

impl BrowserHistory {
    fn address() -> String {
        let loc = web_sys::window().expect("window").location();
        let path = loc.pathname().unwrap_or_else(|_| "/".into());
        let search = loc.search().unwrap_or_default();
        format!("{path}{search}")
    }
}

impl serge_router::History for BrowserHistory {
    fn current(&self) -> String {
        BrowserHistory::address()
    }
    fn push(&self, path: &str) {
        if let Ok(h) = web_sys::window().expect("window").history() {
            let _ = h.push_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(path));
        }
    }
    fn replace(&self, path: &str) {
        if let Ok(h) = web_sys::window().expect("window").history() {
            let _ = h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(path));
        }
    }
    fn back(&self) -> Option<String> {
        if let Ok(h) = web_sys::window().expect("window").history() {
            let _ = h.back();
        }
        // `popstate` reports the new address.
        None
    }
}

fn insert(parent: &web_sys::Node, node: &web_sys::Node, before: Option<&web_sys::Node>) {
    let _ = parent.insert_before(node, before);
}

/// A pair of comment markers delimiting a dynamic range.
fn markers(
    cx: &Ctx,
    parent: &web_sys::Node,
    before: Option<&web_sys::Node>,
    what: &str,
) -> (web_sys::Node, web_sys::Node) {
    let start: web_sys::Node = cx.doc.create_comment(what).into();
    let end: web_sys::Node = cx.doc.create_comment(&format!("/{what}")).into();
    insert(parent, &start, before);
    insert(parent, &end, before);
    let (s, e) = (start.clone(), end.clone());
    on_cleanup(move || {
        remove_range(&s, &e);
        if let Some(p) = s.parent_node() {
            let _ = p.remove_child(&s);
        }
        if let Some(p) = e.parent_node() {
            let _ = p.remove_child(&e);
        }
    });
    (start, end)
}

/// Removes the nodes strictly between `start` and `end`.
fn remove_range(start: &web_sys::Node, end: &web_sys::Node) {
    let Some(parent) = start.parent_node() else {
        return;
    };
    while let Some(n) = start.next_sibling() {
        if n.is_same_node(Some(end)) {
            break;
        }
        let _ = parent.remove_child(&n);
    }
}

/// Moves `start..=end` before `before`.
fn move_range(start: &web_sys::Node, end: &web_sys::Node, before: &web_sys::Node) {
    let Some(parent) = before.parent_node() else {
        return;
    };
    let mut nodes = vec![start.clone()];
    let mut cur = start.next_sibling();
    while let Some(n) = cur {
        let last = n.is_same_node(Some(end));
        cur = n.next_sibling();
        nodes.push(n);
        if last {
            break;
        }
    }
    for n in nodes {
        let _ = parent.insert_before(&n, Some(before));
    }
}

fn mount_node(
    cx: &Rc<Ctx>,
    node: Node,
    parent: &web_sys::Node,
    before: Option<&web_sys::Node>,
    view: &View,
) {
    match node {
        Node::Empty => {}
        Node::Fragment(children) => {
            for c in children {
                mount_node(cx, c, parent, before, view);
            }
        }
        Node::Widget(w) => mount_widget(cx, *w, parent, before, view),
        Node::Region(f) => {
            let (_, end) = markers(cx, parent, before, "region");
            let (cx, v, parent) = (cx.clone(), view.clone(), parent.clone());
            effect(move || {
                v.enter(|| {
                    let child = v.child();
                    let el = child.run(|| f());
                    child.run(|| untrack(|| mount_node(&cx, el.0, &parent, Some(&end), &child)));
                })
            });
        }
        Node::Keyed(k) => {
            let (_, end) = markers(cx, parent, before, "for");
            type Entry = (View, web_sys::Node, web_sys::Node, Rc<dyn std::any::Any>);
            let items: Rc<RefCell<HashMap<u64, Entry>>> = Rc::default();
            let (cx, v, parent) = (cx.clone(), view.clone(), parent.clone());
            effect(move || {
                v.enter(|| {
                    let list = (k.items)();
                    let mut alive = HashMap::with_capacity(list.len());
                    let mut order = Vec::with_capacity(list.len());
                    let mut items = items.borrow_mut();
                    for item in list {
                        let key = item.key;
                        let kept = match items.remove(&key) {
                            Some(e) if (item.same)(&*e.3, &*item.value) => Some(e),
                            Some((iv, _, _, _)) => {
                                iv.dispose();
                                None
                            }
                            None => None,
                        };
                        let entry = match kept {
                            Some(e) => e,
                            None => {
                                let build = item.build;
                                let iv = v.owner().with(|| v.child());
                                let (s, e) = iv.run(|| markers(&cx, &parent, Some(&end), "item"));
                                let el = untrack(|| iv.run(build));
                                iv.run(|| {
                                    untrack(|| mount_node(&cx, el.0, &parent, Some(&e), &iv))
                                });
                                (iv, s, e, item.value)
                            }
                        };
                        order.push((entry.1.clone(), entry.2.clone()));
                        alive.insert(key, entry);
                    }
                    for (_, (iv, _, _, _)) in items.drain() {
                        iv.dispose();
                    }
                    *items = alive;
                    // Put the items in order, last to first: each item ends
                    // right before the next one (or the list's end marker).
                    let mut anchor = end.clone();
                    for (s, e) in order.iter().rev() {
                        if !e
                            .next_sibling()
                            .is_some_and(|n| n.is_same_node(Some(&anchor)))
                        {
                            move_range(s, e, &anchor);
                        }
                        anchor = s.clone();
                    }
                })
            });
        }
        Node::Scoped(scope, inner) => {
            let v = view.child_in(&scope);
            v.run(|| mount_node(cx, *inner, parent, before, &v));
        }
    }
}

fn tag(kind: Kind) -> (&'static str, &'static str) {
    match kind {
        Kind::View => ("div", "serge-view"),
        Kind::Stack => ("div", "serge-stack"),
        Kind::Card => ("div", "serge-card"),
        Kind::Text => ("div", "serge-text"),
        Kind::Button => ("button", "serge-button"),
        Kind::Slider => ("input", "serge-slider"),
        Kind::Input => ("input", "serge-input"),
        Kind::Checkbox => ("label", "serge-checkbox"),
        Kind::Canvas => ("div", "serge-canvas"),
        Kind::ProgressBar => ("div", "serge-progress"),
        Kind::Link => ("a", "serge-link"),
    }
}

/// Listens for `event` on `target` while the current owner lives.
fn listen(
    target: &web_sys::EventTarget,
    event: &'static str,
    f: impl FnMut(web_sys::Event) + 'static,
) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(f);
    let _ = target.add_event_listener_with_callback(event, closure.as_ref().unchecked_ref());
    let t = target.clone();
    on_cleanup(move || {
        let _ = t.remove_event_listener_with_callback(event, closure.as_ref().unchecked_ref());
        drop(closure);
    });
}

fn fmt_num(v: f32) -> String {
    format!("{v}")
}

fn mount_widget(
    cx: &Rc<Ctx>,
    w: Widget,
    parent: &web_sys::Node,
    before: Option<&web_sys::Node>,
    view: &View,
) {
    let Widget {
        kind,
        style,
        attrs,
        handlers,
        web,
        cursor: _,
        children,
    } = w;
    let (tag_name, class) = tag(kind);
    let el = cx.doc.create_element(tag_name).expect("create element");
    let html: HtmlElement = el.clone().unchecked_into();
    let _ = el.set_attribute("class", class);

    // The element that carries input state and the node text goes into.
    let (input, label): (Option<HtmlInputElement>, Option<web_sys::Element>) = match kind {
        Kind::Slider => {
            let i: HtmlInputElement = el.clone().unchecked_into();
            i.set_type("range");
            i.set_step("any");
            (Some(i), None)
        }
        Kind::Input => {
            let i: HtmlInputElement = el.clone().unchecked_into();
            i.set_type("text");
            (Some(i), None)
        }
        Kind::Checkbox => {
            let i: HtmlInputElement = cx
                .doc
                .create_element("input")
                .expect("input")
                .unchecked_into();
            i.set_type("checkbox");
            let _ = el.append_child(&i);
            let span = cx.doc.create_element("span").expect("span");
            let _ = el.append_child(&span);
            (Some(i), Some(span))
        }
        Kind::ProgressBar => {
            let fill = cx.doc.create_element("div").expect("div");
            let _ = el.append_child(&fill);
            (None, Some(fill))
        }
        Kind::Button => {
            let _ = el.set_attribute("type", "button");
            (None, None)
        }
        _ => (None, None),
    };

    // Style: inline declarations, recomputed when a dynamic property changes.
    let theme = cx.theme;
    let apply_style = {
        let html = html.clone();
        move |css: String| {
            let _ = html.set_attribute("style", &css);
        }
    };
    if style.iter().any(Prop::is_dynamic) {
        let v = view.clone();
        effect(move || {
            v.enter(|| apply_style(Widget::resolve_style(kind, &theme, &style).to_css()))
        });
    } else {
        apply_style(Widget::resolve_style(kind, &theme, &style).to_css());
    }

    // Attributes.
    for (a, p) in attrs {
        let set = {
            let el = el.clone();
            let input = input.clone();
            let label = label.clone();
            move |v: AttrValue| set_attr(kind, &el, input.as_ref(), label.as_ref(), a, &v)
        };
        match p {
            Prop::Static(v) => set(v),
            Prop::Dynamic(f) => {
                let v = view.clone();
                effect(move || v.enter(|| set(f())));
            }
        }
    }
    for (name, p) in web {
        let set = {
            let el = el.clone();
            move |v: String| {
                if name == "class" {
                    let _ = el.set_attribute("class", &format!("{class} {v}"));
                } else {
                    let _ = el.set_attribute(name, &v);
                }
            }
        };
        match p {
            Prop::Static(v) => set(v),
            Prop::Dynamic(f) => {
                let v = view.clone();
                effect(move || v.enter(|| set(f())));
            }
        }
    }

    // Events.
    let target: web_sys::EventTarget = el.clone().into();
    for h in handlers {
        let v = view.clone();
        match h {
            Handler::Clicked(f) => listen(&target, "click", move |_| v.run(|| f())),
            Handler::ChangedNum(f) => {
                let i = input.clone().expect("slider input");
                listen(&target, "input", move |_| {
                    let x = i.value_as_number() as f32;
                    v.run(|| f(x));
                });
            }
            Handler::ChangedText(f) => {
                let i = input.clone().expect("text input");
                listen(&target, "input", move |_| {
                    let x = i.value();
                    v.run(|| f(x));
                });
            }
            Handler::ChangedBool(f) => {
                let i = input.clone().expect("checkbox");
                let t: web_sys::EventTarget = i.clone().into();
                listen(&t, "change", move |_| {
                    let x = i.checked();
                    v.run(|| f(x));
                });
            }
            Handler::Submitted(f) => listen(&target, "keydown", move |e| {
                let enter = e
                    .dyn_ref::<web_sys::KeyboardEvent>()
                    .is_some_and(|k| k.key() == "Enter" && !k.is_composing());
                if enter {
                    e.prevent_default();
                    v.run(|| f());
                }
            }),
            Handler::Paint(paint) => {
                canvas::mount(&el, view, paint);
            }
        }
    }
    if kind == Kind::Link {
        let el2 = el.clone();
        let v = view.clone();
        listen(&target, "click", move |e| {
            let plain = e.dyn_ref::<web_sys::MouseEvent>().is_none_or(|m| {
                !(m.ctrl_key() || m.meta_key() || m.shift_key() || m.button() != 0)
            });
            if plain && let Some(href) = el2.get_attribute("href") {
                e.prevent_default();
                v.run(|| serge_core::navigate(&href));
            }
        });
    }

    insert(parent, &el, before);
    let e2 = el.clone();
    on_cleanup(move || e2.remove());

    let node: web_sys::Node = el.into();
    for c in children {
        mount_node(cx, c, &node, None, view);
    }
}

fn set_attr(
    kind: Kind,
    el: &web_sys::Element,
    input: Option<&HtmlInputElement>,
    label: Option<&web_sys::Element>,
    a: Attr,
    v: &AttrValue,
) {
    match (kind, a) {
        (Kind::Checkbox, Attr::Text) => {
            if let Some(l) = label {
                l.set_text_content(Some(v.as_str()));
            }
        }
        (Kind::Checkbox, Attr::Checked) => {
            if let Some(i) = input {
                i.set_checked(v.as_bool());
            }
        }
        (Kind::ProgressBar, Attr::Value) => {
            if let Some(fill) = label {
                let pct = (v.as_num().clamp(0.0, 1.0) * 100.0).to_string();
                let _ = fill.set_attribute("style", &format!("width: {pct}%"));
            }
        }
        (_, Attr::Text) => el.set_text_content(Some(v.as_str())),
        (_, Attr::InputText) => {
            if let Some(i) = input
                && i.value() != v.as_str()
            {
                i.set_value(v.as_str());
            }
        }
        (_, Attr::Placeholder) => {
            if let Some(i) = input {
                i.set_placeholder(v.as_str());
            }
        }
        (_, Attr::Value) => {
            if let Some(i) = input {
                let s = fmt_num(v.as_num());
                if i.value() != s {
                    i.set_value(&s);
                }
            }
        }
        (_, Attr::Min) => {
            if let Some(i) = input {
                i.set_min(&fmt_num(v.as_num()));
            }
        }
        (_, Attr::Max) => {
            if let Some(i) = input {
                i.set_max(&fmt_num(v.as_num()));
            }
        }
        (_, Attr::Step) => {
            if let Some(i) = input {
                let s = v.as_num();
                i.set_step(&if s > 0.0 { fmt_num(s) } else { "any".into() });
            }
        }
        (_, Attr::Href) => {
            let _ = el.set_attribute("href", v.as_str());
        }
        (_, Attr::Disabled) => {
            if v.as_bool() {
                let _ = el.set_attribute("disabled", "");
            } else {
                let _ = el.remove_attribute("disabled");
            }
        }
        (_, Attr::Checked) => {}
    }
}

/// What canvases need from the frame loop.
pub(crate) struct CanvasFrame {
    pub(crate) bounds: Signal<Rect>,
    pub(crate) tick: Signal<u64>,
}

pub(crate) fn paint_into(
    v: &View,
    paint: &serge_core::PaintFn,
    canvas: &web_sys::HtmlCanvasElement,
    f: &CanvasFrame,
    sources: &RefCell<Vec<(serge_core::FrameSource, u64)>>,
) {
    v.enter(|| {
        let b = f.bounds.get();
        let _ = f.tick.get();
        if b.width <= 0.0 || b.height <= 0.0 {
            return;
        }
        let mut ctx = PaintCtx::new(b.width, b.height);
        let ((), found) = track_frame_sources(|| paint(ctx.bounds(), &mut ctx));
        *sources.borrow_mut() = found;
        canvas::draw(canvas, b, &ctx.into_commands());
    });
}
