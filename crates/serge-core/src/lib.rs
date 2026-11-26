//! `serge-core` — the target-neutral core of serge: the view tree `ui!`
//! builds ([`Element`]), built-in widgets, view scopes ([`View`]), UI tasks
//! ([`spawn`]), and the drawing contract shared by native renderers
//! ([`ScenePrimitive`]) and the `Canvas` escape hatch ([`PaintCtx`]).

pub mod builder;
mod element;
mod nav;
pub mod paint;
mod route;
mod target_attrs;
mod view;
pub mod widgets;

pub use element::{
    Attr, AttrValue, Callback, Cursor, Element, Handler, IntoElement, Keyed, KeyedItem, Kind, Node,
    PaintFn, Prop, Site, Widget, key_hash,
};
pub use nav::{navigate, set_navigator};
pub use paint::{FontHandle, FrameSource, Glyph, PaintCmd, PaintCtx, Point, Rect, ScenePrimitive};
pub use route::{FromView, RouteComponent, RouteParams};
pub use target_attrs::{native, web};
#[cfg(not(target_arch = "wasm32"))]
pub use view::runtime;
pub use view::{
    Executor, LocalFuture, TaskHandle, View, inject, install_executor, sleep, spawn, with_executor,
};

pub use serge_style as style;

pub mod viewport;
