//! `serge` — the Hyperish family's UI framework: one `ui!` markup for the
//! web (real DOM) and native targets (GPU/CPU rendering, no WebView).
//!
//! ```ignore
//! use hyve::prelude::*;
//! use serge::prelude::*;
//!
//! #[component]
//! pub fn CounterCard() -> Element {
//!     let mut count = signal(0);
//!     ui! {
//!         Card { padding: 16px, gap: 12px,
//!             Text { "Count: {count()}" }
//!             Button { text: "Increment", clicked => { count += 1; } }
//!         }
//!     }
//! }
//! ```

pub use serge_core::{
    Callback, Cursor, Element, FrameSource, IntoElement, Kind, PaintCmd, PaintCtx, Point, Rect,
    RouteComponent, RouteParams, ScenePrimitive, View, spawn,
};
pub use serge_macros::{component, ui};
pub use serge_router::{
    Assembled, AssemblyError, AuthState, Guard, History, MemoryHistory, RequireAuth, Router,
    SergeApp, UiFeature, back, feature, navigate,
};

/// Runs an app on the current target: a window on desktop, the page on the
/// web.
pub trait Run {
    /// Assembles and runs the app until its window closes (desktop) or for
    /// the page's lifetime (web).
    ///
    /// # Panics
    ///
    /// Panics with the assembly problems when the app is invalid, and on
    /// desktop exits with an error message when no window can be opened.
    fn run(self);
}

impl Run for SergeApp {
    fn run(self) {
        let app = self.assemble().unwrap_or_else(|e| panic!("{e}"));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let options = serge_platform_desktop::WindowOptions {
                title: app.title.clone(),
                ..Default::default()
            };
            let scope = app.scope.clone();
            if let Err(e) = serge_platform_desktop::run(options, scope, move || app.element()) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    }
}
pub use serge_style::{Color, Layout, Length, Theme, pct, px, rem};

/// Testing without a window: the native runtime with simulated input
/// (`Headless`).
#[cfg(not(target_arch = "wasm32"))]
pub mod testing {
    pub use serge_native::{Headless, Key};
}

/// Built-in widgets and their style properties.
pub mod widgets {
    pub use serge_core::widgets::*;
}

/// Everything most UI code needs: `use serge::prelude::*;` (next to
/// `use hyve::prelude::*;`).
pub mod prelude {
    pub use crate::widgets::{
        Button, Canvas, Card, Checkbox, Input, Link, ProgressBar, Slider, Stack, Styled, Text,
        View, center, end, h_stack, space_between, start, stretch, v_stack,
    };
    pub use crate::{
        AuthState, Callback, Color, Element, FrameSource, PaintCtx, Point, Rect, RequireAuth, Run,
        SergeApp, UiFeature, back, component, feature, navigate, px, spawn, ui,
    };
    pub use serge_core::{native, web};
}

#[doc(hidden)]
pub mod __private {
    pub use hyve::state::untrack;
    pub use serge_core::builder::{
        Build, Dyn, IntoArg, IntoProp, Lit, Set, Unset, element, fragment, keyed, region,
    };
    pub use serge_core::widgets::Styled;
    pub use serge_core::{Callback, Element, FromView, IntoElement, native, spawn, web};
    pub use serge_style::{pct, px, rem};
}
