//! The retained node tree native targets mount `Element`s into. Reactive
//! properties are effects that write into nodes and ask for a frame;
//! regions and keyed lists are layout-transparent groups whose children are
//! rebuilt or reordered.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use hyve::state::{Signal, effect, on_cleanup, signal, untrack};
use serge_core::paint::track_frame_sources;
use serge_core::{
    Attr, AttrValue, Cursor, FrameSource, Handler, Kind, Node, PaintCmd, PaintCtx, Prop, Rect,
    View, Widget,
};
use serge_style::{Style, Theme};

/// What a node is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NKind {
    /// The window's root.
    Root,
    /// A layout-transparent group (fragment, region, keyed list, item).
    Group,
    /// A built-in widget.
    Widget(Kind),
}

pub(crate) struct CanvasState {
    pub bounds: Signal<Rect>,
    pub tick: Signal<u64>,
    pub commands: Vec<PaintCmd>,
    pub sources: Vec<(FrameSource, u64)>,
}

pub(crate) struct NNode {
    pub kind: NKind,
    pub style: Style,
    pub attrs: Vec<(Attr, AttrValue)>,
    pub handlers: Vec<Handler>,
    pub cursor: Option<Cursor>,
    pub children: Vec<usize>,
    pub parent: Option<usize>,
    /// Border box in content coordinates (before scrolling).
    pub rect: Rect,
    pub view: View,
    /// An input's text.
    pub input: String,
    pub canvas: Option<CanvasState>,
}

impl NNode {
    pub fn attr(&self, a: Attr) -> Option<&AttrValue> {
        self.attrs.iter().find(|(k, _)| *k == a).map(|(_, v)| v)
    }

    pub fn text(&self) -> &str {
        self.attr(Attr::Text).map_or("", AttrValue::as_str)
    }

    pub fn num(&self, a: Attr, default: f32) -> f32 {
        self.attr(a).map_or(default, AttrValue::as_num)
    }

    pub fn flag(&self, a: Attr) -> bool {
        self.attr(a).is_some_and(AttrValue::as_bool)
    }

    fn set_attr(&mut self, a: Attr, v: AttrValue) {
        if a == Attr::InputText {
            self.input = v.as_str().to_owned();
        }
        match self.attrs.iter_mut().find(|(k, _)| *k == a) {
            Some(slot) => slot.1 = v,
            None => self.attrs.push((a, v)),
        }
    }
}

/// Mutable UI state behind the tree.
pub(crate) struct Tree {
    pub nodes: Vec<Option<NNode>>,
    free: Vec<usize>,
    pub root: usize,
    pub dirty: bool,
    pub focus: Option<usize>,
    pub hover: Option<usize>,
    pub pressed: Option<usize>,
    pub drag: Option<usize>,
    pub scroll_y: f32,
    pub content_height: f32,
    pub size: (f32, f32),
}

impl Tree {
    pub fn new(root_view: View) -> Tree {
        let mut t = Tree {
            nodes: Vec::new(),
            free: Vec::new(),
            root: 0,
            dirty: true,
            focus: None,
            hover: None,
            pressed: None,
            drag: None,
            scroll_y: 0.0,
            content_height: 0.0,
            size: (0.0, 0.0),
        };
        t.root = t.alloc(NKind::Root, None, root_view);
        t
    }

    pub fn node(&self, id: usize) -> Option<&NNode> {
        self.nodes.get(id).and_then(Option::as_ref)
    }

    pub fn node_mut(&mut self, id: usize) -> Option<&mut NNode> {
        self.nodes.get_mut(id).and_then(Option::as_mut)
    }

    fn alloc(&mut self, kind: NKind, parent: Option<usize>, view: View) -> usize {
        let node = NNode {
            kind,
            style: Style::default(),
            attrs: Vec::new(),
            handlers: Vec::new(),
            cursor: None,
            children: Vec::new(),
            parent,
            rect: Rect::default(),
            view,
            input: String::new(),
            canvas: None,
        };
        let id = match self.free.pop() {
            Some(id) => {
                self.nodes[id] = Some(node);
                id
            }
            None => {
                self.nodes.push(Some(node));
                self.nodes.len() - 1
            }
        };
        if let Some(p) = parent.and_then(|p| self.node_mut(p)) {
            p.children.push(id);
        }
        self.dirty = true;
        id
    }

    /// Removes `id` and its subtree.
    pub fn remove(&mut self, id: usize) {
        let Some(node) = self.nodes.get_mut(id).and_then(Option::take) else {
            return;
        };
        if let Some(p) = node.parent.and_then(|p| self.node_mut(p)) {
            p.children.retain(|c| *c != id);
        }
        for c in node.children {
            if let Some(n) = self.node_mut(c) {
                n.parent = None;
            }
            self.remove(c);
        }
        for slot in [
            &mut self.focus,
            &mut self.hover,
            &mut self.pressed,
            &mut self.drag,
        ] {
            if *slot == Some(id) {
                *slot = None;
            }
        }
        self.free.push(id);
        self.dirty = true;
    }

    /// Visits `id`'s subtree in paint order.
    pub fn walk(&self, id: usize, f: &mut impl FnMut(usize, &NNode)) {
        if let Some(n) = self.node(id) {
            f(id, n);
            for &c in &n.children {
                self.walk(c, f);
            }
        }
    }
}

/// The shared handle effects and handlers use.
pub(crate) struct Shared {
    pub tree: RefCell<Tree>,
    pub theme: Theme,
    pub request_frame: RefCell<Rc<dyn Fn()>>,
}

impl Shared {
    pub fn changed(&self) {
        if let Ok(mut t) = self.tree.try_borrow_mut() {
            t.dirty = true;
        }
        let r = self.request_frame.borrow().clone();
        r();
    }

    fn with<R>(&self, f: impl FnOnce(&mut Tree) -> R) -> R {
        let r = f(&mut self.tree.borrow_mut());
        self.changed();
        r
    }
}

/// Mounts `node` as children of `parent`, inside `view` (the caller runs
/// this with `view` current).
pub(crate) fn mount(ui: &Rc<Shared>, node: Node, parent: usize, view: &View) {
    match node {
        Node::Empty => {}
        Node::Fragment(children) => {
            for c in children {
                mount(ui, c, parent, view);
            }
        }
        Node::Widget(w) => mount_widget(ui, *w, parent, view),
        Node::Region(f) => {
            let group = group(ui, parent, view);
            let (ui, v) = (ui.clone(), view.clone());
            effect(move || {
                v.enter(|| {
                    let child = v.child();
                    let el = child.run(|| f());
                    child.run(|| untrack(|| mount(&ui, el.0, group, &child)));
                    ui.changed();
                })
            });
        }
        Node::Keyed(k) => {
            let group = group(ui, parent, view);
            type Entry = (View, usize, Rc<dyn std::any::Any>);
            let items: Rc<RefCell<HashMap<u64, Entry>>> = Rc::default();
            let (ui, v) = (ui.clone(), view.clone());
            effect(move || {
                v.enter(|| {
                    let list = (k.items)();
                    let mut order = Vec::with_capacity(list.len());
                    let mut alive = HashMap::with_capacity(list.len());
                    let mut items = items.borrow_mut();
                    for item in list {
                        let key = item.key;
                        let kept = match items.remove(&key) {
                            Some(e) if (item.same)(&*e.2, &*item.value) => Some(e),
                            Some((iv, _, _)) => {
                                iv.dispose();
                                None
                            }
                            None => None,
                        };
                        let entry = match kept {
                            Some(e) => e,
                            None => {
                                let build = item.build;
                                // Items outlive the effect's reruns: their
                                // views belong to the list's view.
                                let iv = v.owner().with(|| v.child());
                                let slot =
                                    ui.with(|t| t.alloc(NKind::Group, Some(group), iv.clone()));
                                let ui2 = ui.clone();
                                iv.owner()
                                    .with(|| on_cleanup(move || ui2.with(|t| t.remove(slot))));
                                let el = untrack(|| iv.run(build));
                                iv.run(|| untrack(|| mount(&ui, el.0, slot, &iv)));
                                (iv, slot, item.value)
                            }
                        };
                        order.push(entry.1);
                        alive.insert(key, entry);
                    }
                    for (_, (iv, _, _)) in items.drain() {
                        iv.dispose();
                    }
                    *items = alive;
                    ui.with(|t| {
                        if let Some(g) = t.node_mut(group) {
                            g.children = order;
                        }
                    });
                })
            });
        }
        Node::Scoped(scope, inner) => {
            let v = view.child_in(&scope);
            v.run(|| mount(ui, *inner, parent, &v));
        }
    }
}

fn group(ui: &Rc<Shared>, parent: usize, view: &View) -> usize {
    let id = ui.with(|t| t.alloc(NKind::Group, Some(parent), view.clone()));
    let ui2 = ui.clone();
    on_cleanup(move || ui2.with(|t| t.remove(id)));
    id
}

fn mount_widget(ui: &Rc<Shared>, w: Widget, parent: usize, view: &View) {
    let Widget {
        kind,
        style,
        attrs,
        handlers,
        web: _,
        cursor,
        children,
        site: _,
    } = w;
    let paint = handlers.iter().find_map(|h| match h {
        Handler::Paint(p) => Some(p.clone()),
        _ => None,
    });
    let id = ui.with(|t| {
        let id = t.alloc(NKind::Widget(kind), Some(parent), view.clone());
        let n = t.node_mut(id).expect("just allocated");
        n.handlers = handlers;
        n.cursor = cursor;
        id
    });
    let ui2 = ui.clone();
    on_cleanup(move || ui2.with(|t| t.remove(id)));

    // Style: computed once when fixed, else an effect.
    let theme = ui.theme;
    if style.iter().any(Prop::is_dynamic) {
        let (ui, v) = (ui.clone(), view.clone());
        effect(move || {
            v.enter(|| {
                let s = Widget::resolve_style(kind, &theme, &style);
                ui.with(|t| {
                    if let Some(n) = t.node_mut(id) {
                        n.style = s;
                    }
                });
            })
        });
    } else {
        let s = Widget::resolve_style(kind, &theme, &style);
        ui.with(|t| t.node_mut(id).expect("alive").style = s);
    }

    for (a, p) in attrs {
        match p {
            Prop::Static(v) | Prop::Const(v) => {
                ui.with(|t| t.node_mut(id).expect("alive").set_attr(a, v))
            }
            Prop::Dynamic(f) => {
                let (ui, v) = (ui.clone(), view.clone());
                effect(move || {
                    v.enter(|| {
                        let value = f();
                        ui.with(|t| {
                            if let Some(n) = t.node_mut(id) {
                                n.set_attr(a, value);
                            }
                        });
                    })
                });
            }
        }
    }

    if let Some(paint) = paint {
        let bounds = signal(Rect::default());
        let tick = signal(0u64);
        ui.with(|t| {
            t.node_mut(id).expect("alive").canvas = Some(CanvasState {
                bounds,
                tick,
                commands: Vec::new(),
                sources: Vec::new(),
            })
        });
        let (ui, v) = (ui.clone(), view.clone());
        effect(move || {
            v.enter(|| {
                let b = bounds.get();
                let _ = tick.get();
                if b.width <= 0.0 || b.height <= 0.0 {
                    return;
                }
                let mut ctx = PaintCtx::new(b.width, b.height);
                let ((), sources) = track_frame_sources(|| paint(ctx.bounds(), &mut ctx));
                let commands = ctx.into_commands();
                ui.with(|t| {
                    if let Some(c) = t.node_mut(id).and_then(|n| n.canvas.as_mut()) {
                        c.commands = commands;
                        c.sources = sources;
                    }
                });
            })
        });
    }

    for c in children {
        mount(ui, c, id, view);
    }
}
