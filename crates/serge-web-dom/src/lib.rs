//! `serge-web-dom` — the web target: mounts `Element`s as real DOM and keeps
//! it in sync with signals at the finest grain the tree describes (one
//! effect per dynamic property; `if` regions and keyed `for` lists between
//! comment markers). Canvases draw their `PaintCtx` commands with the 2D
//! context; the router uses the browser history.
//!
//! Widgets written in `ui!` are cloned from a template per place in the
//! source (`Site`): the first widget built there creates the template — its
//! element with every source literal (`Prop::Const`) baked in, the direct
//! children written in the same `ui!` nested, an empty comment for every
//! other child — and every widget built there is a `cloneNode(true)` of it,
//! patched with only its other properties, handlers and children. Widgets
//! built in code are created with DOM calls.
//!
//! Phase 1 prototype limit: the theme is chosen once from
//! `prefers-color-scheme`.
#![cfg(target_arch = "wasm32")]

mod canvas;
mod text;
mod viewport;
pub use viewport::WebViewport;
mod exec;

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use hyve::Scope;
use hyve::state::{Signal, effect, on_cleanup, untrack};
use serge_core::paint::track_frame_sources;
use serge_core::{
    Attr, AttrValue, Element, Handler, Kind, Node, PaintCtx, Prop, Rect, Site, View, Widget,
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
.serge-text, .serge-button, .serge-link, .serge-checkbox span { white-space: pre-wrap; }
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
    /// Templates by place (`Site` address).
    templates: RefCell<HashMap<usize, web_sys::Element>>,
    prerendered: RefCell<VecDeque<web_sys::Element>>,
    /// Elements created and cloned (tests).
    counts: std::cell::Cell<(u32, u32)>,
}

/// A mounted UI.
pub struct Mounted {
    view: View,
    root: web_sys::Element,
    cx: Rc<Ctx>,
}

impl std::fmt::Debug for Mounted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mounted")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

/// How widgets were made so far: elements created with DOM calls (widgets
/// built in code, and each new template), and template clones.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DomStats {
    /// Elements created with `createElement` for widgets.
    pub created: u32,
    /// Widgets cloned from a template (`cloneNode(true)` of a whole place).
    pub cloned: u32,
    /// Templates (one per place in the source).
    pub templates: u32,
}

impl Mounted {
    /// How widgets were made so far.
    pub fn stats(&self) -> DomStats {
        let (created, cloned) = self.cx.counts.get();
        DomStats {
            created,
            cloned,
            templates: self.cx.templates.borrow().len() as u32,
        }
    }

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

fn prerendered(root: &web_sys::Element) -> VecDeque<web_sys::Element> {
    if !root.has_attribute("data-serge-prerendered") {
        return VecDeque::new();
    }
    let document_build = document()
        .query_selector("meta[name='serge-build']")
        .ok()
        .flatten()
        .and_then(|e| e.get_attribute("content"));
    if option_env!("SERGE_BUILD_ID").is_none()
        || document_build.as_deref() != option_env!("SERGE_BUILD_ID")
    {
        root.set_text_content(None);
        return VecDeque::new();
    }
    let nodes = root
        .query_selector_all("[data-serge-node]")
        .expect("fixed selector");
    (0..nodes.length())
        .filter_map(|i| nodes.item(i)?.dyn_into().ok())
        .collect()
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
        templates: RefCell::default(),
        prerendered: RefCell::new(prerendered(container)),
        counts: std::cell::Cell::new((0, 0)),
    });
    let view = View::root(scope);
    #[cfg(feature = "ionic")]
    view.owner().with(|| {
        let installed = serge_web_ionic::install(container);
        on_cleanup(move || drop(installed));
    });
    let parent: web_sys::Node = container.clone().into();
    view.run(|| {
        let el = untrack(root);
        mount_node(&cx, el.0, &parent, None, &view);
    });
    for node in cx.prerendered.take() {
        node.remove();
    }
    let _ = container.remove_attribute("data-serge-prerendered");
    Mounted {
        view,
        root: container.clone(),
        cx,
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
    let root = page_root();
    let mounted = mount(&root, scope, move || app.element());
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

/// A new element for `kind` with its fixed structure (tag, class, the
/// checkbox's input and label, the progress fill, input types).
fn create(cx: &Ctx, kind: Kind) -> web_sys::Element {
    let (c, k) = cx.counts.get();
    cx.counts.set((c + 1, k));
    let (tag_name, class) = kind.html();
    let el = cx.doc.create_element(tag_name).expect("create element");
    let _ = el.set_attribute("class", class);
    match kind {
        Kind::Slider => {
            let i: &HtmlInputElement = el.unchecked_ref();
            i.set_type("range");
            i.set_step("any");
        }
        Kind::Input => el.unchecked_ref::<HtmlInputElement>().set_type("text"),
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
        }
        Kind::ProgressBar => {
            let fill = cx.doc.create_element("div").expect("div");
            let _ = el.append_child(&fill);
        }
        Kind::Button => {
            let _ = el.set_attribute("type", "button");
        }
        _ => {}
    }
    el
}

/// The element that carries input state and the node text goes into.
fn parts(
    kind: Kind,
    el: &web_sys::Element,
) -> (Option<HtmlInputElement>, Option<web_sys::Element>) {
    match kind {
        Kind::Slider | Kind::Input => (Some(el.clone().unchecked_into()), None),
        Kind::Checkbox => (
            el.first_element_child().map(JsCast::unchecked_into),
            el.last_element_child(),
        ),
        Kind::ProgressBar => (None, el.first_element_child()),
        _ => (None, None),
    }
}

/// Nodes `create` puts inside an element before its children.
fn inner_nodes(kind: Kind) -> u32 {
    match kind {
        Kind::Checkbox => 2,
        Kind::ProgressBar => 1,
        _ => 0,
    }
}

/// Whether attribute `i` is in its place's template: a source literal
/// that is not overridden later and lives in HTML (cloned with the node),
/// not in a live property (an input's value, a checkbox's state).
fn baked_attr(kind: Kind, attrs: &[(Attr, Prop<AttrValue>)], i: usize) -> bool {
    let (a, p) = &attrs[i];
    let html = match a {
        Attr::InputText | Attr::Checked => false,
        Attr::Value => kind == Kind::ProgressBar,
        _ => true,
    };
    html && p.is_const() && !attrs[i + 1..].iter().any(|(b, _)| b == a)
}

fn baked_web(web: &[(&'static str, Prop<String>)], i: usize) -> bool {
    let (n, p) = &web[i];
    p.is_const() && !web[i + 1..].iter().any(|(m, _)| m == n)
}

fn set_web(el: &web_sys::Element, class: &str, name: &str, v: &str) {
    if name == "class" {
        let _ = el.set_attribute("class", &format!("{class} {v}"));
    } else {
        let _ = el.set_attribute(name, v);
    }
}

/// Whether `child` is written directly inside the widget at `site` (so it
/// is part of that place's template).
fn inlined(child: &Widget, site: &Site) -> bool {
    child.site.is_some_and(|s| s.is_child_of(site))
}

/// The template for `w`'s place.
fn skeleton(cx: &Ctx, w: &Widget, site: &Site) -> web_sys::Element {
    let el = create(cx, w.kind);
    let (_, class) = w.kind.html();
    let (input, label) = parts(w.kind, &el);
    if w.style.iter().all(Prop::is_const) {
        let css = untrack(|| Widget::resolve_style(w.kind, &cx.theme, &w.style).to_css());
        let _ = el.set_attribute("style", &css);
    }
    for (i, (a, p)) in w.attrs.iter().enumerate() {
        if baked_attr(w.kind, &w.attrs, i) {
            set_attr(w.kind, &el, input.as_ref(), label.as_ref(), *a, &p.get());
        }
    }
    for (i, (name, p)) in w.web.iter().enumerate() {
        if baked_web(&w.web, i) {
            set_web(&el, class, name, &p.get());
        }
    }
    for c in &w.children {
        match c {
            Node::Widget(cw) if inlined(cw, site) => {
                let _ = el.append_child(&skeleton(cx, cw, cw.site.expect("inlined")));
            }
            _ => {
                let _ = el.append_child(&cx.doc.create_comment(""));
            }
        }
    }
    el
}

fn mount_widget(
    cx: &Rc<Ctx>,
    w: Widget,
    parent: &web_sys::Node,
    before: Option<&web_sys::Node>,
    view: &View,
) {
    // Server and client traverse widgets in declaration order. Markers and
    // event handlers are installed by the same path used for a fresh mount.
    let (tag, class) = w.kind.html();
    let candidate = {
        let mut remaining = cx.prerendered.borrow_mut();
        let index = remaining.iter().position(|el| {
            el.parent_node()
                .is_some_and(|p| p.is_same_node(Some(parent)))
                && el.tag_name().eq_ignore_ascii_case(tag)
                && el.get_attribute("data-serge-node").as_deref() == Some(class)
        });
        index.and_then(|i| remaining.remove(i))
    };
    if let Some(el) = candidate {
        let fixed = match w.kind {
            Kind::Checkbox => {
                el.first_element_child()
                    .is_some_and(|n| n.tag_name() == "INPUT")
                    && el
                        .last_element_child()
                        .is_some_and(|n| n.tag_name() == "SPAN")
            }
            Kind::ProgressBar => el
                .first_element_child()
                .is_some_and(|n| n.tag_name() == "DIV"),
            _ => true,
        };
        if fixed {
            let _ = el.remove_attribute("data-serge-node");
            wire(cx, w, &el, None, view);
            insert(parent, &el, before);
            return;
        }
        el.remove();
    }
    let (el, site) = match w.site {
        Some(site) => {
            let key = std::ptr::from_ref(site) as usize;
            let cached = cx.templates.borrow().get(&key).cloned();
            let template = match cached {
                Some(t) => t,
                None => {
                    let t = skeleton(cx, &w, site);
                    cx.templates.borrow_mut().insert(key, t.clone());
                    t
                }
            };
            let el: web_sys::Element = template
                .clone_node_with_deep(true)
                .expect("clone")
                .unchecked_into();
            let (c, k) = cx.counts.get();
            cx.counts.set((c, k + 1));
            (el, Some(site))
        }
        None => (create(cx, w.kind), None),
    };
    wire(cx, w, &el, site, view);
    insert(parent, &el, before);
}

/// Applies `w` to `el`: everything when `site` is `None` (a new element),
/// or only what its place's template does not hold (a clone).
fn wire(cx: &Rc<Ctx>, w: Widget, el: &web_sys::Element, site: Option<&'static Site>, view: &View) {
    let Widget {
        kind,
        style,
        attrs,
        handlers,
        web,
        cursor: _,
        children,
        site: _,
    } = w;
    let (_, class) = kind.html();
    let html: HtmlElement = el.clone().unchecked_into();
    let (input, label) = parts(kind, el);
    let cloned = site.is_some();

    // Style: inline declarations, recomputed when a dynamic property changes.
    let theme = cx.theme;
    let apply_style = {
        let html = html.clone();
        move |css: String| {
            let _ = html.set_attribute("style", &css);
        }
    };
    if cloned && style.iter().all(Prop::is_const) {
        // In the template.
    } else if style.iter().any(Prop::is_dynamic) {
        let v = view.clone();
        effect(move || {
            v.enter(|| apply_style(Widget::resolve_style(kind, &theme, &style).to_css()))
        });
    } else {
        apply_style(Widget::resolve_style(kind, &theme, &style).to_css());
    }

    // Attributes.
    let text_target =
        if children.is_empty() && attrs.iter().filter(|(a, _)| *a == Attr::Text).count() == 1 {
            match kind {
                Kind::Checkbox => label.as_ref(),
                Kind::Text | Kind::Button | Kind::Link => Some(el),
                _ => None,
            }
        } else {
            None
        };
    let baked: Vec<bool> = (0..attrs.len())
        .map(|i| cloned && baked_attr(kind, &attrs, i))
        .collect();
    for ((a, p), baked) in attrs.into_iter().zip(baked) {
        if baked {
            continue;
        }
        if a == Attr::Text
            && let (Some(target), Prop::Dynamic(read)) = (text_target, &p)
        {
            let mut slot = text::TextSlot::adopt(target, &cx.doc);
            let read = read.clone();
            let view = view.clone();
            effect(move || view.enter(|| slot.rebuild(read())));
            continue;
        }
        let set = {
            let el = el.clone();
            let input = input.clone();
            let label = label.clone();
            move |v: AttrValue| set_attr(kind, &el, input.as_ref(), label.as_ref(), a, &v)
        };
        match p {
            Prop::Static(v) | Prop::Const(v) => set(v),
            Prop::Dynamic(f) => {
                let v = view.clone();
                effect(move || v.enter(|| set(f())));
            }
        }
    }
    let baked: Vec<bool> = (0..web.len())
        .map(|i| cloned && baked_web(&web, i))
        .collect();
    for ((name, p), baked) in web.into_iter().zip(baked) {
        if baked {
            continue;
        }
        let set = {
            let el = el.clone();
            move |v: String| set_web(&el, class, name, &v)
        };
        match p {
            Prop::Static(v) | Prop::Const(v) => set(v),
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
            Handler::Viewport(producer) => viewport::mount(el, producer, view),
            Handler::Paint(paint) => {
                canvas::mount(el, view, paint);
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

    let e2 = el.clone();
    on_cleanup(move || e2.remove());

    let node: web_sys::Node = el.clone().into();
    match site {
        // The template has a node per child: the inlined widget's element,
        // or an empty comment that the child is mounted before.
        Some(site) => {
            let mut slot = node.first_child();
            for _ in 0..inner_nodes(kind) {
                slot = slot.and_then(|n| n.next_sibling());
            }
            for c in children {
                let here = slot.expect("a template node per child");
                slot = here.next_sibling();
                match c {
                    Node::Widget(cw) if inlined(&cw, site) => {
                        let s = cw.site;
                        wire(cx, *cw, here.unchecked_ref(), s, view);
                    }
                    other => {
                        mount_node(cx, other, &node, Some(&here), view);
                        let _ = node.remove_child(&here);
                    }
                }
            }
        }
        None => {
            for c in children {
                mount_node(cx, c, &node, None, view);
            }
        }
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
