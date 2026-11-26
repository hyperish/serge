//! Built-in widgets. In `ui!`, `Card { padding: 16px, .. }` calls the
//! builder methods below; an unknown property is a "no method" error at its
//! span. Every widget takes the style properties of [`Styled`].

use std::ops::RangeInclusive;
use std::rc::Rc;

use serge_style::{Align, Color, Layout, Length, StyleProp};

use crate::builder::{Build, IntoProp};
use crate::element::{Attr, AttrValue, Cursor, Element, Handler, Kind, Node, Prop, Widget};
use crate::paint::{PaintCtx, Rect};

/// Style properties shared by every built-in widget.
pub trait Styled: Sized {
    /// The widget being built.
    fn widget(&mut self) -> &mut Widget;

    #[doc(hidden)]
    fn __style<T: Clone + 'static>(mut self, v: impl IntoProp<T>, f: fn(T) -> StyleProp) -> Self {
        self.widget().style.push(v.into_prop().map(f));
        self
    }

    /// Width (`200px`, `50%`).
    fn width(self, v: impl IntoProp<Length>) -> Self {
        self.__style(v, StyleProp::Width)
    }
    /// Height.
    fn height(self, v: impl IntoProp<Length>) -> Self {
        self.__style(v, StyleProp::Height)
    }
    /// Minimum width.
    fn min_width(self, v: impl IntoProp<Length>) -> Self {
        self.__style(v, StyleProp::MinWidth)
    }
    /// Maximum width.
    fn max_width(self, v: impl IntoProp<Length>) -> Self {
        self.__style(v, StyleProp::MaxWidth)
    }
    /// Padding on every side.
    fn padding(self, v: impl IntoProp<Length>) -> Self {
        self.__style(v, StyleProp::Padding)
    }
    /// Left and right padding.
    fn padding_x(self, v: impl IntoProp<Length>) -> Self {
        self.__style(v, StyleProp::PaddingX)
    }
    /// Top and bottom padding.
    fn padding_y(self, v: impl IntoProp<Length>) -> Self {
        self.__style(v, StyleProp::PaddingY)
    }
    /// Space between children.
    fn gap(self, v: impl IntoProp<Length>) -> Self {
        self.__style(v, StyleProp::Gap)
    }
    /// Child placement (`v_stack`, `h_stack`).
    fn layout(self, v: impl IntoProp<Layout>) -> Self {
        self.__style(v, StyleProp::Layout)
    }
    /// Cross-axis alignment (`start`, `center`, `end`, `stretch`).
    fn align(self, v: impl IntoProp<Align>) -> Self {
        self.__style(v, StyleProp::Align)
    }
    /// Main-axis alignment (`start`, `center`, `end`, `space_between`).
    fn justify(self, v: impl IntoProp<Align>) -> Self {
        self.__style(v, StyleProp::Justify)
    }
    /// Background color.
    fn background(self, v: impl IntoProp<Color>) -> Self {
        self.__style(v, StyleProp::Background)
    }
    /// Text color.
    fn color(self, v: impl IntoProp<Color>) -> Self {
        self.__style(v, StyleProp::Color)
    }
    /// Corner radius.
    fn radius(self, v: impl IntoProp<Length>) -> Self {
        self.__style(v, StyleProp::Radius)
    }
    /// Border width.
    fn border_width(self, v: impl IntoProp<Length>) -> Self {
        self.__style(v, StyleProp::BorderWidth)
    }
    /// Border color.
    fn border_color(self, v: impl IntoProp<Color>) -> Self {
        self.__style(v, StyleProp::BorderColor)
    }
    /// Font size.
    fn font_size(self, v: impl IntoProp<Length>) -> Self {
        self.__style(v, StyleProp::FontSize)
    }
    /// Font weight (100–900).
    fn font_weight(self, v: impl IntoProp<u16>) -> Self {
        self.__style(v, StyleProp::FontWeight)
    }
    /// Share of the free main-axis space.
    fn grow(self, v: impl IntoProp<f32>) -> Self {
        self.__style(v, StyleProp::Grow)
    }

    #[doc(hidden)]
    fn __web(mut self, attr: crate::web::WebAttr) -> Self {
        self.widget().web.push((attr.0, attr.1));
        self
    }

    #[doc(hidden)]
    fn __native(mut self, attr: crate::native::NativeAttr) -> Self {
        match attr {
            crate::native::NativeAttr::Cursor(c) => self.widget().cursor = Some(c),
        }
        self
    }
}

macro_rules! widgets {
    ($($(#[$doc:meta])* $name:ident => $kind:ident),* $(,)?) => {$(
        $(#[$doc])*
        pub struct $name(Widget);

        impl $name {
            #[doc(hidden)]
            pub fn __new() -> Self {
                $name(Widget::new(Kind::$kind))
            }
        }

        impl Styled for $name {
            fn widget(&mut self) -> &mut Widget {
                &mut self.0
            }
        }

        impl crate::builder::Sited for $name {
            fn __site(mut self, site: &'static crate::element::Site) -> Self {
                self.0.site = Some(site);
                self
            }
        }

        impl Build for $name {
            const REGION: bool = false;
            fn build(self) -> Element {
                Element(Node::Widget(Box::new(self.0)))
            }
        }
    )*};
}

widgets! {
    /// A plain container (column by default).
    View => View,
    /// A container with 8 px between children (column by default).
    Stack => Stack,
    /// A surface: padding, border, rounded corners.
    Card => Card,
    /// Text: `Text { "Count: {count()}" }`.
    Text => Text,
    /// A push button: `Button { text: "Save", clicked => { .. } }`.
    Button => Button,
    /// A slider: `Slider { range: 0.0..=1.0, value: v(), changed(x) => { .. } }`.
    Slider => Slider,
    /// A text field: `Input { value: name(), changed(t) => { name.set(t) } }`.
    Input => Input,
    /// A checkbox: `Checkbox { text: "Done", checked: done(), changed(c) => { .. } }`.
    Checkbox => Checkbox,
    /// A custom-drawn area: `Canvas { height: 64px, paint(bounds, ctx) => { .. } }`.
    Canvas => Canvas,
    /// Progress from 0.0 to 1.0: `ProgressBar { value: p() }`.
    ProgressBar => ProgressBar,
    /// Navigation: `Link { to: "/projects", "Projects" }`.
    Link => Link,
}

macro_rules! containers {
    ($($name:ident),*) => {$(
        impl crate::builder::Children for $name {
            fn __child(mut self, child: Element) -> Self {
                self.0.children.push(child.0);
                self
            }
        }
    )*};
}

containers!(View, Stack, Card);

fn attr<T: Clone + 'static>(w: &mut Widget, a: Attr, v: impl IntoProp<T>, f: fn(T) -> AttrValue) {
    w.attrs.push((a, v.into_prop().map(f)));
}

impl Text {
    #[doc(hidden)]
    pub fn __text(mut self, v: impl IntoProp<String>) -> Self {
        attr(&mut self.0, Attr::Text, v, AttrValue::Str);
        self
    }
}

impl Link {
    #[doc(hidden)]
    pub fn __text(mut self, v: impl IntoProp<String>) -> Self {
        attr(&mut self.0, Attr::Text, v, AttrValue::Str);
        self
    }

    /// The route to open.
    pub fn to(mut self, v: impl IntoProp<String>) -> Self {
        attr(&mut self.0, Attr::Href, v, AttrValue::Str);
        self
    }
}

impl Button {
    /// The label.
    pub fn text(mut self, v: impl IntoProp<String>) -> Self {
        attr(&mut self.0, Attr::Text, v, AttrValue::Str);
        self
    }

    /// Whether it ignores clicks.
    pub fn disabled(mut self, v: impl IntoProp<bool>) -> Self {
        attr(&mut self.0, Attr::Disabled, v, AttrValue::Bool);
        self
    }

    /// `clicked => { .. }`.
    pub fn clicked(mut self, f: impl FnOnce() + Clone + 'static) -> Self {
        self.0
            .handlers
            .push(Handler::Clicked(crate::builder::reuse0(f)));
        self
    }
}

impl Slider {
    /// The range (`-5.0..=5.0`).
    pub fn range(mut self, v: impl IntoProp<RangeInclusive<f32>>) -> Self {
        let p = v.into_prop();
        let lo = p.clone().map(|r| AttrValue::Num(*r.start()));
        let hi = p.map(|r| AttrValue::Num(*r.end()));
        self.0.attrs.push((Attr::Min, lo));
        self.0.attrs.push((Attr::Max, hi));
        self
    }

    /// The value.
    pub fn value(mut self, v: impl IntoProp<f32>) -> Self {
        attr(&mut self.0, Attr::Value, v, AttrValue::Num);
        self
    }

    /// The step between values (0 for continuous, the default).
    pub fn step(mut self, v: impl IntoProp<f32>) -> Self {
        attr(&mut self.0, Attr::Step, v, AttrValue::Num);
        self
    }

    /// `changed(v) => { .. }`.
    pub fn changed(mut self, f: impl FnOnce(f32) + Clone + 'static) -> Self {
        self.0
            .handlers
            .push(Handler::ChangedNum(crate::builder::reuse1(f)));
        self
    }
}

impl Input {
    /// The text.
    pub fn value(mut self, v: impl IntoProp<String>) -> Self {
        attr(&mut self.0, Attr::InputText, v, AttrValue::Str);
        self
    }

    /// Shown while empty.
    pub fn placeholder(mut self, v: impl IntoProp<String>) -> Self {
        attr(&mut self.0, Attr::Placeholder, v, AttrValue::Str);
        self
    }

    /// `changed(text) => { .. }` on every edit.
    pub fn changed(mut self, f: impl FnOnce(String) + Clone + 'static) -> Self {
        self.0
            .handlers
            .push(Handler::ChangedText(crate::builder::reuse1(f)));
        self
    }

    /// `submitted => { .. }` on Enter.
    pub fn submitted(mut self, f: impl FnOnce() + Clone + 'static) -> Self {
        self.0
            .handlers
            .push(Handler::Submitted(crate::builder::reuse0(f)));
        self
    }
}

impl Checkbox {
    /// The label.
    pub fn text(mut self, v: impl IntoProp<String>) -> Self {
        attr(&mut self.0, Attr::Text, v, AttrValue::Str);
        self
    }

    /// Whether it is checked.
    pub fn checked(mut self, v: impl IntoProp<bool>) -> Self {
        attr(&mut self.0, Attr::Checked, v, AttrValue::Bool);
        self
    }

    /// `changed(checked) => { .. }`.
    pub fn changed(mut self, f: impl FnOnce(bool) + Clone + 'static) -> Self {
        self.0
            .handlers
            .push(Handler::ChangedBool(crate::builder::reuse1(f)));
        self
    }
}

impl Canvas {
    /// `paint(bounds, ctx) => { .. }`: runs again when the signals (or frame
    /// sources) it reads change and when the canvas is resized.
    pub fn paint(mut self, f: impl FnOnce(Rect, &mut PaintCtx) + Clone + 'static) -> Self {
        self.0
            .handlers
            .push(Handler::Paint(Rc::new(move |b, ctx| (f.clone())(b, ctx))));
        self
    }
}

impl ProgressBar {
    /// Progress from 0.0 to 1.0.
    pub fn value(mut self, v: impl IntoProp<f32>) -> Self {
        attr(&mut self.0, Attr::Value, v, AttrValue::Num);
        self
    }
}

/// `layout: v_stack`.
#[allow(non_upper_case_globals)]
pub const v_stack: Layout = Layout::Column;
/// `layout: h_stack`.
#[allow(non_upper_case_globals)]
pub const h_stack: Layout = Layout::Row;
/// `align: start`.
#[allow(non_upper_case_globals)]
pub const start: Align = Align::Start;
/// `align: center`.
#[allow(non_upper_case_globals)]
pub const center: Align = Align::Center;
/// `align: end`.
#[allow(non_upper_case_globals)]
pub const end: Align = Align::End;
/// `align: stretch`.
#[allow(non_upper_case_globals)]
pub const stretch: Align = Align::Stretch;
/// `justify: space_between`.
#[allow(non_upper_case_globals)]
pub const space_between: Align = Align::SpaceBetween;

/// Keeps the `Prop` name used by builder signatures reachable.
#[doc(hidden)]
pub type __Prop<T> = Prop<T>;

/// The cursor values for `native::cursor`.
pub use crate::element::Cursor as CursorKind;
#[doc(hidden)]
pub fn __cursor(c: Cursor) -> Cursor {
    c
}
