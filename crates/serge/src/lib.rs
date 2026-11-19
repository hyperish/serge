//! `serge` — Unified cross-platform Web (DOM) and Native (GPU + mandatory CPU fallback)
//! UI framework for the Hyperish family.
//!
//! This initial pre-release establishes the target-neutral style IR, element tree,
//! and render target descriptors described in `serge/docs`.

/// Execution and rendering backend target supported by `serge`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderTarget {
    /// Browser DOM target with fine-grained reactive patching.
    WebDom,
    /// Native GPU batched renderer (Desktop, iOS, Android — never WebView).
    NativeGpu,
    /// Mandatory CPU software fallback renderer consuming the same scene primitives.
    NativeCpu,
}

/// Target-neutral length unit shared between Web CSS emission and Native layout solvers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    /// Automatic sizing.
    Auto,
    /// Device-independent logical pixels.
    Px(f32),
    /// Percentage relative to parent container.
    Percent(f32),
}

/// Minimal target-neutral style intermediate representation (`serge-style` foundation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Uniform inner padding.
    pub padding: Length,
    /// Spacing between child items.
    pub gap: Length,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            padding: Length::Px(0.0),
            gap: Length::Px(0.0),
        }
    }
}

/// Platform-neutral UI element node produced by `serge` components.
#[derive(Debug, Clone, PartialEq)]
pub enum Element {
    /// Text leaf node.
    Text(String),
    /// Container node carrying a tag, style IR, and children.
    Container {
        /// Semantic widget or element name (e.g. `"Card"`, `"Stack"`).
        tag: String,
        /// Platform-neutral style properties.
        style: Style,
        /// Ordered child elements.
        children: Vec<Element>,
    },
}

impl Element {
    /// Creates a text leaf [`Element`].
    #[must_use]
    pub fn text(content: impl Into<String>) -> Self {
        Self::Text(content.into())
    }

    /// Creates a container [`Element`] with style and children.
    #[must_use]
    pub fn container(tag: impl Into<String>, style: Style, children: Vec<Element>) -> Self {
        Self::Container {
            tag: tag.into(),
            style,
            children,
        }
    }
}

/// Commonly used types re-exported for ergonomic imports.
pub mod prelude {
    pub use crate::{Element, Length, RenderTarget, Style};
}

#[cfg(test)]
mod tests {
    use super::prelude::*;

    #[test]
    fn element_tree_builds_target_neutral_ir() {
        let card = Element::container(
            "Card",
            Style {
                padding: Length::Px(16.0),
                gap: Length::Px(12.0),
            },
            vec![Element::text("Count: 0")],
        );

        match card {
            Element::Container {
                tag,
                style,
                children,
            } => {
                assert_eq!(tag, "Card");
                assert_eq!(style.padding, Length::Px(16.0));
                assert_eq!(children.len(), 1);
            }
            Element::Text(_) => panic!("expected container"),
        }
        assert_ne!(RenderTarget::NativeGpu, RenderTarget::NativeCpu);
    }
}
