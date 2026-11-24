//! What `ui!` and `#[component]` expand to. Not for direct use; the names
//! are stable for macro output only.
//!
//! One closure rule covers everything `ui!` creates — property expressions,
//! regions, handlers, `paint`: each is `FnOnce + Clone` and every run calls a
//! fresh clone. Captured values must be `Clone` (handles like `Signal<T>` and
//! `Ctx<T>` are cheap to clone); state that must survive between runs lives
//! in signals.

use std::hash::Hash;
use std::rc::Rc;

use crate::element::{Element, Keyed, KeyedItem, Node, key_hash};

/// A literal or a plain path in property position: evaluated once.
pub struct Lit<V>(pub V);

/// Any other property expression: re-evaluated when the signals it reads
/// change.
pub struct Dyn<F>(pub F);

/// What a built-in widget's property accepts.
pub trait IntoProp<T> {
    /// The property.
    fn into_prop(self) -> crate::element::Prop<T>;
}

/// A value a property of type `T` takes: `T` itself, or a string slice for
/// `String`. Deliberately narrower than `Into`, so that unsuffixed literals
/// take the property's type (`font_weight: 700` is a `u16`, `grow: 1.0` an
/// `f32`) instead of defaulting to `i32`/`f64`.
pub trait PropValue<T> {
    /// The value.
    fn into_value(self) -> T;
}

impl<T> PropValue<T> for T {
    fn into_value(self) -> T {
        self
    }
}

impl PropValue<String> for &str {
    fn into_value(self) -> String {
        self.to_owned()
    }
}

impl PropValue<String> for &String {
    fn into_value(self) -> String {
        self.clone()
    }
}

impl<T, V: PropValue<T>> IntoProp<T> for Lit<V> {
    fn into_prop(self) -> crate::element::Prop<T> {
        crate::element::Prop::Static(self.0.into_value())
    }
}

impl<T: 'static, V: PropValue<T>, F: FnOnce() -> V + Clone + 'static> IntoProp<T> for Dyn<F> {
    fn into_prop(self) -> crate::element::Prop<T> {
        let f = self.0;
        crate::element::Prop::Dynamic(Rc::new(move || (f.clone())().into_value()))
    }
}

/// What a component's property accepts: evaluated when the component is
/// (re)built, inside its region, so reading a signal rebuilds it.
pub trait IntoArg<T> {
    /// The value.
    fn into_arg(self) -> T;
}

impl<T, V: PropValue<T>> IntoArg<T> for Lit<V> {
    fn into_arg(self) -> T {
        self.0.into_value()
    }
}

impl<T, V: PropValue<T>, F: FnOnce() -> V> IntoArg<T> for Dyn<F> {
    fn into_arg(self) -> T {
        (self.0)().into_value()
    }
}

/// A finished widget or component builder.
pub trait Build {
    /// Whether the builder is a component (rebuilt as a region when its
    /// properties change) rather than a built-in widget (whose properties
    /// update in place).
    const REGION: bool;

    /// The element.
    fn build(self) -> Element;
}

/// `Tag { .. }` in `ui!`.
pub fn element<B: Build, F: FnOnce() -> B + Clone + 'static>(f: F) -> Element {
    if B::REGION {
        Element(Node::Region(Rc::new(move || (f.clone())().build())))
    } else {
        f().build()
    }
}

/// `if` in `ui!` (and any reactive region).
pub fn region<F: FnOnce() -> Element + Clone + 'static>(f: F) -> Element {
    Element(Node::Region(Rc::new(move || (f.clone())())))
}

/// `for pat in items { key: k, .. }` in `ui!`. Items keep their view while
/// their key and value stay the same; a changed value under the same key
/// rebuilds that item only.
pub fn keyed<T, I, K, FI, FK, FR>(items: FI, key: FK, render: FR) -> Element
where
    T: Clone + PartialEq + 'static,
    I: IntoIterator<Item = T>,
    K: Hash,
    FI: FnOnce() -> I + Clone + 'static,
    FK: Fn(&T) -> K + 'static,
    FR: FnOnce(T) -> Element + Clone + 'static,
{
    Element(Node::Keyed(Rc::new(Keyed {
        items: Box::new(move || {
            let mut seen = std::collections::HashMap::<u64, u32>::new();
            (items.clone())()
                .into_iter()
                .map(|item| {
                    let mut k = key_hash(&key(&item));
                    // A repeated key gets a distinct identity per occurrence.
                    let n = seen.entry(k).or_default();
                    if *n > 0 {
                        k = key_hash(&(k, *n));
                    }
                    *n += 1;
                    let render = render.clone();
                    KeyedItem {
                        key: k,
                        value: Rc::new(item.clone()),
                        same: same::<T>,
                        build: Box::new(move || render(item)),
                    }
                })
                .collect()
        }),
    })))
}

fn same<T: PartialEq + 'static>(a: &dyn std::any::Any, b: &dyn std::any::Any) -> bool {
    matches!((a.downcast_ref::<T>(), b.downcast_ref::<T>()), (Some(a), Some(b)) if a == b)
}

/// Several `ui!` nodes.
pub fn fragment(children: Vec<Element>) -> Element {
    Element::fragment(children)
}

/// Typestate marker: a required component property not set yet.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unset;

/// Typestate marker: a required component property set.
#[derive(Debug, Clone, Copy, Default)]
pub struct Set;

/// Wraps a `FnOnce + Clone` closure as a reusable `Fn`.
pub fn reuse0(f: impl FnOnce() + Clone + 'static) -> Rc<dyn Fn()> {
    Rc::new(move || (f.clone())())
}

/// Wraps a one-argument `FnOnce + Clone` closure as a reusable `Fn`.
pub fn reuse1<A: 'static>(f: impl FnOnce(A) + Clone + 'static) -> Rc<dyn Fn(A)> {
    Rc::new(move |a| (f.clone())(a))
}
