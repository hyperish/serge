//! Responsive, server-renderable navigation and modal sheets built from Serge
//! elements. Enable `serge/ionic` to install their interactions on the web.
use serge_core::{Attr, AttrValue, Element, Handler, Kind, Node, Prop, Widget};
use std::rc::Rc;

/// Styles for custom SSR document heads. The DOM adapter installs these once.
pub const STYLESHEET: &str = include_str!("adaptive.css");

fn container(class: &str, role: Option<&str>, children: Vec<Element>) -> Widget {
    let mut w = Widget::new(Kind::View);
    w.web.push(("class", Prop::Static(class.into())));
    if let Some(r) = role {
        w.web.push(("role", Prop::Static(r.into())));
    }
    w.children = children.into_iter().map(|e| e.0).collect();
    w
}
fn element(w: Widget) -> Element {
    Element(Node::Widget(Box::new(w)))
}

/// Navigation stays beside the content on a wide screen and becomes a bottom
/// bar on compact screens. Both arrangements keep the same DOM and view scope.
pub fn shell(navigation: Element, content: Element) -> Element {
    let nav = element(container(
        "serge-ionic-nav",
        Some("navigation"),
        vec![navigation],
    ));
    let main = element(container("serge-ionic-main", Some("main"), vec![content]));
    element(container("serge-ionic-shell", None, vec![nav, main]))
}

/// Creates a labelled modal with an app-supplied dismiss button label. Mount
/// the sheet in a conditional region. The callback should close that region.
/// Escape, the backdrop and a downward handle swipe use that same callback.
pub fn sheet(
    label: &str,
    dismiss_label: &str,
    content: Element,
    dismiss: impl Fn() + 'static,
) -> Element {
    let mut handle = container("serge-ionic-grip", None, Vec::new());
    handle.web.push(("aria-hidden", Prop::Const("true".into())));
    let mut button = Widget::new(Kind::Button);
    button
        .web
        .push(("class", Prop::Const("serge-ionic-dismiss".into())));
    button.attrs.push((
        Attr::Text,
        Prop::Static(AttrValue::Str(dismiss_label.into())),
    ));
    button.handlers.push(Handler::Clicked(Rc::new(dismiss)));
    let mut panel = container(
        "serge-ionic-panel",
        Some("dialog"),
        vec![element(handle), content, element(button)],
    );
    for (key, value) in [
        ("aria-label", label),
        ("aria-modal", "true"),
        ("tabindex", "-1"),
    ] {
        panel.web.push((key, Prop::Static(value.into())));
    }
    element(container(
        "serge-ionic-backdrop",
        None,
        vec![element(panel)],
    ))
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(module = "/src/adaptive.js")]
    extern "C" {
        #[wasm_bindgen(js_name=mountAdaptive)]
        fn mount(root: &web_sys::Element, css: &str) -> js_sys::Function;
    }
    /// Owns DOM listeners and observers installed for one Serge root.
    pub struct Installed(js_sys::Function);
    impl Drop for Installed {
        fn drop(&mut self) {
            let _ = self.0.call0(&JsValue::UNDEFINED);
        }
    }
    /// Installs keyboard, visual viewport and touch handling for this root.
    pub fn install(root: &web_sys::Element) -> Installed {
        Installed(mount(root, super::STYLESHEET))
    }
}
#[cfg(target_arch = "wasm32")]
pub use browser::{Installed, install};
