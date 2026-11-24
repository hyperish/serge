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

pub use serge_adapter_client::{RemoteTransport, SharedRingBuf};
/// The `tokio` runtime native UI tasks and remote transports run on.
#[cfg(not(target_arch = "wasm32"))]
pub use serge_core::runtime;
pub use serge_core::{
    Callback, Cursor, Element, FrameSource, IntoElement, Kind, PaintCmd, PaintCtx, Point, Rect,
    RouteComponent, RouteParams, ScenePrimitive, View, sleep, spawn,
};
pub use serge_macros::{component, ui};
pub use serge_router::{
    Assembled, AssemblyError, AuthState, Guard, History, MemoryHistory, RequireAuth, Router,
    SergeApp, UiFeature, back, feature, location, navigate,
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
        #[cfg(target_arch = "wasm32")]
        serge_web_dom::run(app);
    }
}
pub use serge_style::{Color, Layout, Length, Theme, pct, px, rem};

/// Testing without a window: the native runtime with simulated input
/// (`Headless`).
#[cfg(not(target_arch = "wasm32"))]
pub mod testing {
    pub use serge_native::{Headless, Key};
    pub use serge_native_cpu::{CpuRenderer, Frame};

    /// Writes `frame` as a binary PPM (any image viewer, `sips`, ImageMagick).
    pub fn write_ppm(frame: &Frame, path: &std::path::Path) -> std::io::Result<()> {
        let mut out = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
        for p in frame.rgba.as_chunks::<4>().0 {
            out.extend_from_slice(&p[..3]);
        }
        std::fs::write(path, out)
    }
}

/// Testing in a browser: mount into an element and drive the real DOM.
#[cfg(target_arch = "wasm32")]
pub mod testing {
    pub use serge_web_dom::{Mounted, mount, page_root};
}

/// Built-in widgets and their style properties.
pub mod widgets {
    pub use serge_core::widgets::*;
}

/// Everything most UI code needs: `use serge::prelude::*;` (next to
/// `use hyve::prelude::*;`).
pub mod prelude {
    // Alignment is `Align::Center` etc.: lowercase constants such as
    // `start`/`end` would turn ordinary `let start = ..` bindings into
    // constant patterns in every file that imports the prelude.
    pub use crate::widgets::{
        Button, Canvas, Card, Checkbox, Input, Link, ProgressBar, Slider, Stack, Styled, Text,
        View, h_stack, v_stack,
    };
    pub use crate::{
        AuthState, Callback, Color, Element, FrameSource, PaintCtx, Point, Rect, RemoteTransport,
        RequireAuth, Run, SergeApp, SharedRingBuf, UiFeature, back, component, feature, location,
        navigate, px, sleep, spawn, ui,
    };
    pub use serge_core::{native, web};
    pub use serge_style::{Align, Layout};
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
