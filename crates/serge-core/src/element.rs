//! The view tree `ui!` builds: a description that each target mounts (DOM
//! nodes on the web, a retained node tree on native).

use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use hyve::Scope;
use serge_style::{Style, StyleProp, Theme};

use crate::paint::{PaintCtx, Rect};

/// A piece of UI. What `ui!` and `#[component]` functions return.
#[must_use = "an Element does nothing until it is mounted"]
#[derive(Clone)]
pub struct Element(pub Node);

impl Element {
    /// Nothing.
    pub fn empty() -> Element {
        Element(Node::Empty)
    }

    /// Several elements in order.
    pub fn fragment(children: Vec<Element>) -> Element {
        Element(Node::Fragment(children.into_iter().map(|e| e.0).collect()))
    }

    /// `inner`, mounted in a view whose scope is a child of `scope` (a
    /// screen under its feature's scope).
    pub fn scoped(scope: Scope, inner: Element) -> Element {
        Element(Node::Scoped(scope, Box::new(inner.0)))
    }
}

impl fmt::Debug for Element {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A node of the view tree.
#[derive(Clone)]
pub enum Node {
    /// Nothing.
    Empty,
    /// A built-in widget.
    Widget(Box<Widget>),
    /// Children in order.
    Fragment(Vec<Node>),
    /// A reactive region (`if`, a component whose properties read signals):
    /// rebuilt, in a fresh view, when what the closure read changes.
    Region(Rc<dyn Fn() -> Element>),
    /// A keyed list (`for`): items are kept, moved or dropped by key.
    Keyed(Rc<Keyed>),
    /// A subtree mounted in a view under the given scope.
    Scoped(Scope, Box<Node>),
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Node::Empty => f.write_str("Empty"),
            Node::Widget(w) => f
                .debug_struct("Widget")
                .field("kind", &w.kind)
                .field("children", &w.children)
                .finish_non_exhaustive(),
            Node::Fragment(c) => f.debug_tuple("Fragment").field(c).finish(),
            Node::Region(_) => f.write_str("Region(..)"),
            Node::Keyed(_) => f.write_str("Keyed(..)"),
            Node::Scoped(_, n) => f.debug_tuple("Scoped").field(n).finish(),
        }
    }
}

/// One item of a keyed list.
pub struct KeyedItem {
    /// Identity: items with the same key are kept (and moved) across updates.
    pub key: u64,
    /// The item's value, to notice when it changed under the same key.
    pub value: Rc<dyn std::any::Any>,
    /// Whether two values (of this item's type) are equal.
    pub same: fn(&dyn std::any::Any, &dyn std::any::Any) -> bool,
    /// Builds the item's view (new keys and changed values).
    pub build: Box<dyn FnOnce() -> Element>,
}

/// A keyed list: `items` is evaluated reactively.
pub struct Keyed {
    /// The current items in order.
    pub items: Box<dyn Fn() -> Vec<KeyedItem>>,
}

/// The built-in widgets.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    /// A plain container.
    View,
    /// A container with spacing between children.
    Stack,
    /// A surface with padding, border and rounded corners.
    Card,
    /// Text.
    Text,
    /// A push button.
    Button,
    /// A horizontal slider over a range.
    Slider,
    /// A single-line text field.
    Input,
    /// A checkbox with a label.
    Checkbox,
    /// A custom-drawn area (`paint(bounds, ctx)`).
    Canvas,
    /// A horizontal bar showing 0.0–1.0.
    ProgressBar,
    /// Text that navigates to a route when activated.
    Link,
}

impl Kind {
    /// The DOM element and class used by the browser and server renderers.
    pub fn html(self) -> (&'static str, &'static str) {
        match self {
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

    /// The style a widget of this kind starts from.
    pub fn base_style(self, theme: &Theme) -> Style {
        use serge_style::{Align, Length::Px};
        let mut s = Style {
            color: theme.text,
            ..Style::default()
        };
        match self {
            Kind::View | Kind::Text => {}
            Kind::Stack => s.gap = Px(8.0),
            Kind::Card => {
                s.padding_x = Px(16.0);
                s.padding_y = Px(16.0);
                s.gap = Px(8.0);
                s.background = theme.surface;
                s.radius = Px(12.0);
                s.border_width = Px(1.0);
                s.border_color = theme.border;
            }
            Kind::Button => {
                s.padding_x = Px(14.0);
                s.padding_y = Px(8.0);
                s.radius = Px(8.0);
                s.background = theme.primary;
                s.color = theme.on_primary;
                s.font_weight = 600;
                s.align = Align::Center;
                s.justify = Align::Center;
            }
            Kind::Slider => {
                s.height = Px(24.0);
                s.min_width = Px(120.0);
            }
            Kind::Input => {
                s.padding_x = Px(10.0);
                s.padding_y = Px(6.0);
                s.radius = Px(6.0);
                s.border_width = Px(1.0);
                s.border_color = theme.border;
                s.background = theme.surface;
                s.min_width = Px(160.0);
            }
            Kind::Checkbox => {
                s.layout = serge_style::Layout::Row;
                s.gap = Px(8.0);
                s.align = Align::Center;
            }
            Kind::Canvas => s.height = Px(120.0),
            Kind::ProgressBar => {
                s.height = Px(8.0);
                s.radius = Px(4.0);
                s.background = theme.border;
            }
            Kind::Link => s.color = theme.primary,
        }
        s
    }
}

/// A property value: fixed, or recomputed (and re-applied) when the signals
/// it reads change.
pub enum Prop<T> {
    /// Fixed for this widget (evaluated once; may differ between widgets
    /// built at the same place, like `padding: gap`).
    Static(T),
    /// A literal in the source (`padding: 16px`, `"Save"`): the same for
    /// every widget built at the same place, so the web target bakes it
    /// into that place's template.
    Const(T),
    /// Recomputed reactively.
    Dynamic(Rc<dyn Fn() -> T>),
}

impl<T: Clone + 'static> Prop<T> {
    /// The current value (tracked when called inside an effect).
    pub fn get(&self) -> T {
        match self {
            Prop::Static(v) | Prop::Const(v) => v.clone(),
            Prop::Dynamic(f) => f(),
        }
    }

    /// Transforms the value.
    pub fn map<U: 'static>(self, f: impl Fn(T) -> U + 'static) -> Prop<U> {
        match self {
            Prop::Static(v) => Prop::Static(f(v)),
            Prop::Const(v) => Prop::Const(f(v)),
            Prop::Dynamic(g) => Prop::Dynamic(Rc::new(move || f(g()))),
        }
    }

    /// Whether the value can change.
    pub fn is_dynamic(&self) -> bool {
        matches!(self, Prop::Dynamic(_))
    }

    /// Whether the value is a source literal ([`Prop::Const`]).
    pub fn is_const(&self) -> bool {
        matches!(self, Prop::Const(_))
    }
}

impl<T: Clone> Clone for Prop<T> {
    fn clone(&self) -> Self {
        match self {
            Prop::Static(v) => Prop::Static(v.clone()),
            Prop::Const(v) => Prop::Const(v.clone()),
            Prop::Dynamic(f) => Prop::Dynamic(f.clone()),
        }
    }
}

/// The place in the source a `ui!` widget was written (one `static` per
/// widget node). Widgets built at the same place have the same shape and
/// the same [`Prop::Const`] values; `parent` is the enclosing widget's place
/// when this one is its direct child in the same `ui!`.
#[derive(Debug)]
pub struct Site {
    /// The enclosing widget's place.
    pub parent: Option<&'static Site>,
}

impl Site {
    /// A place under `parent`.
    pub const fn new(parent: Option<&'static Site>) -> Site {
        Site { parent }
    }

    /// Whether `self` is a direct child written inside `parent`.
    pub fn is_child_of(&self, parent: &Site) -> bool {
        self.parent.is_some_and(|p| std::ptr::eq(p, parent))
    }
}

/// A widget attribute other than style.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Attr {
    /// Button, checkbox and text label; link text.
    Text,
    /// Slider value, progress value.
    Value,
    /// Slider minimum.
    Min,
    /// Slider maximum.
    Max,
    /// Slider step (0 for continuous).
    Step,
    /// Checkbox state.
    Checked,
    /// Input text.
    InputText,
    /// Input placeholder.
    Placeholder,
    /// Link target.
    Href,
    /// Disabled state.
    Disabled,
}

/// An attribute value.
#[derive(Clone, PartialEq, Debug)]
pub enum AttrValue {
    /// Text.
    Str(String),
    /// A number.
    Num(f32),
    /// A flag.
    Bool(bool),
}

impl AttrValue {
    /// The text, or `""`.
    pub fn as_str(&self) -> &str {
        match self {
            AttrValue::Str(s) => s,
            _ => "",
        }
    }

    /// The number, or 0.
    pub fn as_num(&self) -> f32 {
        match self {
            AttrValue::Num(n) => *n,
            _ => 0.0,
        }
    }

    /// The flag, or false.
    pub fn as_bool(&self) -> bool {
        matches!(self, AttrValue::Bool(true))
    }
}

/// A paint callback.
pub type PaintFn = Rc<dyn Fn(Rect, &mut PaintCtx)>;

/// An event handler of a built-in widget.
#[derive(Clone)]
pub enum Handler {
    /// `clicked` (buttons, links).
    Clicked(Rc<dyn Fn()>),
    /// `changed(v)` of a slider.
    ChangedNum(Rc<dyn Fn(f32)>),
    /// `changed(text)` of an input.
    ChangedText(Rc<dyn Fn(String)>),
    /// `changed(checked)` of a checkbox.
    ChangedBool(Rc<dyn Fn(bool)>),
    /// `submitted` of an input (Enter).
    Submitted(Rc<dyn Fn()>),
    /// `paint(bounds, ctx)` of a canvas.
    Paint(PaintFn),
    /// An external producer mounted inside a canvas region.
    Viewport(crate::viewport::Viewport),
}

/// The mouse cursor over a widget on native targets (`native::cursor`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Cursor {
    /// The platform default.
    #[default]
    Default,
    /// A pointing hand.
    Pointer,
    /// A text beam.
    Text,
}

/// A built-in widget with its properties.
#[derive(Clone)]
pub struct Widget {
    /// What it is.
    pub kind: Kind,
    /// Style properties in order (later ones win).
    pub style: Vec<Prop<StyleProp>>,
    /// Other attributes.
    pub attrs: Vec<(Attr, Prop<AttrValue>)>,
    /// Event handlers.
    pub handlers: Vec<Handler>,
    /// `web::*` attributes (HTML attribute name, value); ignored on native.
    pub web: Vec<(&'static str, Prop<String>)>,
    /// `native::cursor`; ignored on the web.
    pub cursor: Option<Cursor>,
    /// Children.
    pub children: Vec<Node>,
    /// Where it was written in a `ui!` (none when built in code).
    pub site: Option<&'static Site>,
}

impl Widget {
    /// An empty widget of `kind`.
    pub fn new(kind: Kind) -> Widget {
        Widget {
            kind,
            style: Vec::new(),
            attrs: Vec::new(),
            handlers: Vec::new(),
            web: Vec::new(),
            cursor: None,
            children: Vec::new(),
            site: None,
        }
    }

    /// The attribute's property, if set (the last one wins).
    pub fn attr(&self, a: Attr) -> Option<&Prop<AttrValue>> {
        self.attrs
            .iter()
            .rev()
            .find(|(k, _)| *k == a)
            .map(|(_, v)| v)
    }

    /// The resolved style now (tracked inside an effect).
    pub fn resolve_style(kind: Kind, theme: &Theme, props: &[Prop<StyleProp>]) -> Style {
        let mut s = kind.base_style(theme);
        for p in props {
            s.apply(&p.get());
        }
        s
    }
}

/// A callback a component takes as an event (`Callback<Args>` argument; the
/// parent writes `name(args) => { .. }`).
pub struct Callback<A = ()>(Rc<dyn Fn(A)>);

impl<A> Callback<A> {
    /// A callback running `f`.
    pub fn new(f: impl Fn(A) + 'static) -> Self {
        Callback(Rc::new(f))
    }

    /// Calls it.
    pub fn call(&self, args: A) {
        (self.0)(args)
    }
}

impl<A> Clone for Callback<A> {
    fn clone(&self) -> Self {
        Callback(self.0.clone())
    }
}

impl<A> fmt::Debug for Callback<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Callback(..)")
    }
}

/// What can stand where an element is expected (`{expr}` in `ui!`).
pub trait IntoElement {
    /// The element.
    fn into_element(self) -> Element;
}

impl IntoElement for Element {
    fn into_element(self) -> Element {
        self
    }
}

impl IntoElement for () {
    fn into_element(self) -> Element {
        Element::empty()
    }
}

impl<E: IntoElement> IntoElement for Option<E> {
    fn into_element(self) -> Element {
        self.map_or_else(Element::empty, IntoElement::into_element)
    }
}

impl<E: IntoElement> IntoElement for Vec<E> {
    fn into_element(self) -> Element {
        Element::fragment(self.into_iter().map(IntoElement::into_element).collect())
    }
}

/// Hashes a `for` key.
pub fn key_hash<K: Hash>(key: &K) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    h.finish()
}
