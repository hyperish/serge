//! `web::*` and `native::*` attributes. Both are checked (name and type) on
//! every target; the other target ignores them.

/// `web::name: value` in `ui!`: HTML attributes of the web target.
pub mod web {
    use crate::builder::IntoProp;
    use crate::element::Prop;

    /// A web attribute.
    pub struct WebAttr(pub &'static str, pub Prop<String>);

    /// `class`.
    pub fn class(v: impl IntoProp<String>) -> WebAttr {
        WebAttr("class", v.into_prop())
    }
    /// `id`.
    pub fn id(v: impl IntoProp<String>) -> WebAttr {
        WebAttr("id", v.into_prop())
    }
    /// `title` (tooltip).
    pub fn title(v: impl IntoProp<String>) -> WebAttr {
        WebAttr("title", v.into_prop())
    }
    /// `aria-label`.
    pub fn aria_label(v: impl IntoProp<String>) -> WebAttr {
        WebAttr("aria-label", v.into_prop())
    }
    /// `role`.
    pub fn role(v: impl IntoProp<String>) -> WebAttr {
        WebAttr("role", v.into_prop())
    }
}

/// `native::name: value` in `ui!`: attributes of native targets.
pub mod native {
    use crate::builder::Lit;
    use crate::element::Cursor;

    /// A native attribute.
    pub enum NativeAttr {
        /// The mouse cursor over the widget.
        Cursor(Cursor),
    }

    /// `native::cursor: pointer` (`default`, `pointer`, `text`).
    pub fn cursor(v: Lit<Cursor>) -> NativeAttr {
        NativeAttr::Cursor(v.0)
    }

    /// Cursor names for `native::cursor`.
    pub use Cursor::{Default as default, Pointer as pointer, Text as text};
}
