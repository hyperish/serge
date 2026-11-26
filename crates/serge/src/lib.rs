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

/// `#[derive(Pod, Zeroable)]` for [`SharedRingBuf`] samples, with
/// `#[bytemuck(crate = "serge::bytemuck")]`.
pub use hyve::bytemuck;
#[cfg(feature = "realtime")]
pub use serge_adapter_client::realtime;
pub use serge_adapter_client::{RemoteTransport, SharedRingBuf};
/// The `tokio` runtime native UI tasks and remote transports run on.
#[cfg(not(target_arch = "wasm32"))]
pub use serge_core::runtime;
pub use serge_core::{
    Callback, Cursor, Element, FrameSource, IntoElement, Kind, PaintCmd, PaintCtx, Point, Rect,
    RouteComponent, RouteParams, ScenePrimitive, View, sleep, spawn,
};
pub use serge_macros::{component, render_value, ui};
/// Dispatches a macro-checked, capture-free render calculation.
#[doc(hidden)]
#[inline(always)]
pub fn __render_value(f: impl Fn() -> u32 + Copy + 'static) -> u32 {
    #[cfg(all(
        feature = "native-hotpatch",
        debug_assertions,
        target_os = "macos",
        target_arch = "aarch64"
    ))]
    {
        serge_native_hotpatch::evaluate(f)
    }
    #[cfg(not(all(
        feature = "native-hotpatch",
        debug_assertions,
        target_os = "macos",
        target_arch = "aarch64"
    )))]
    {
        f()
    }
}
#[cfg(feature = "ionic")]
pub use serge_web_ionic as ionic;

/// Native system clipboard, file selection, alerts and application data.
#[cfg(all(feature = "native-system", not(target_arch = "wasm32")))]
pub use serge_native_sys as sys;
#[cfg(all(feature = "native-system", not(target_arch = "wasm32")))]
/// Hardware interfaces selected for this native target.
pub mod hw {
    #[cfg(all(feature = "native-camera", target_vendor = "apple"))]
    pub use serge_native_gpu::camera::{
        MetalCamera, MetalCameraFrame, MetalCameraTextures, request_camera_permission,
    };
    #[allow(unused_imports)]
    pub use serge_native_sys::hw::*;
}

/// Native multi-window event loop and window controllers.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(any(target_os = "ios", target_os = "android"))
))]
pub mod desktop {
    #[cfg(all(
        feature = "desktop-system",
        any(target_os = "macos", target_os = "windows", target_os = "linux")
    ))]
    pub use serge_platform_desktop::system;
    pub use serge_platform_desktop::{
        DesktopError, Force, NativeEvent, Touch, TouchPhase, WindowEvent, WindowHandle,
        WindowOptions, WindowPlacement, WindowSpec, dpi, run_windows,
    };
}

/// Static widget state to pass across a mobile host bridge.
#[derive(Clone, Debug)]
pub struct PlatformWidget {
    /// Generated widget type selected by the host.
    pub kind: &'static str,
    /// UTF-8 JSON containing the fields referenced by the widget.
    pub state_json: String,
}
impl PlatformWidget {
    #[doc(hidden)]
    pub fn from_fields(
        kind: &'static str,
        fields: impl IntoIterator<Item = (&'static str, hyve::Value)>,
    ) -> Self {
        let value = hyve::Value::Map(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect());
        Self {
            kind,
            state_json: hyve::serde::to_json(&value),
        }
    }
}
/// Native host events for applications embedding platform controls or handling lifecycle.
#[cfg(not(target_arch = "wasm32"))]
pub use serge_platform_desktop::{NativeEvent, WindowEvent};
/// Output of an iOS live activity declaration.
pub type LiveActivity = PlatformWidget;
#[cfg(target_os = "ios")]
mod ios_widgets;
/// Apple widget declarations and the native iOS window hosting them.
pub mod ios {
    #[cfg(target_os = "ios")]
    pub use crate::ios_widgets::{end_activity, publish_widget, start_activity, update_activity};
    pub use serge_macros::{home_widget as widget, live_activity};
    #[cfg(target_os = "ios")]
    pub use serge_platform_desktop::{WindowHandle, WindowOptions, WindowSpec, run_windows};
}
/// Android platform widget attributes.
pub mod android {
    #[cfg(target_os = "android")]
    pub use serge_adapter_client::android::initialize_platform_verifier;
    #[cfg(target_os = "android")]
    pub use serge_platform_desktop::AndroidApp;

    /// Starts a Serge application from a NativeActivity entry point.
    #[cfg(target_os = "android")]
    pub fn launch(
        host: AndroidApp,
        app: super::SergeApp,
    ) -> Result<(), Box<dyn std::error::Error>> {
        launch_with_events(host, app, |_| {})
    }

    /// Starts a NativeActivity and observes window, input, resume and suspend events.
    #[cfg(target_os = "android")]
    pub fn launch_with_events(
        host: AndroidApp,
        app: super::SergeApp,
        observer: impl FnMut(super::NativeEvent<'_>) + 'static,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use serge_platform_desktop::{WindowOptions, WindowSpec, run_android};
        initialize_platform_verifier(&host)?;
        let app = app.assemble()?;
        let options = WindowOptions {
            title: app.title.clone(),
            ..Default::default()
        };
        let scope = app.scope.clone();
        Ok(run_android(
            host,
            WindowSpec::new(options, scope, move || app.element()).on_event(observer),
        )?)
    }

    pub use serge_macros::{glance_widget as widget, ongoing_notification};
}
pub use serge_router::{
    Assembled, AssemblyError, AuthState, Guard, History, MemoryHistory, RequireAuth, Router,
    SergeApp, UiFeature, back, feature, location, navigate,
};

/// Opt-in bridge for a local webview slot.
#[cfg(feature = "webview")]
pub use serge_adapter_webview as webview;

mod signals;
pub use signals::dev_signal;
#[cfg(target_arch = "wasm32")]
pub use signals::persistent_signal;

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
            if let Some(output) = std::env::var_os("SERGE_EXPORT_OUTPUT") {
                if let Err(e) = export_page(app, std::path::Path::new(&output)) {
                    eprintln!("serge page export failed: {e}");
                    std::process::exit(1);
                }
                return;
            }
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
#[cfg(not(target_arch = "wasm32"))]
pub use serge_web_ssr as ssr;

/// Restores an explicitly exported public model after checking the build and
/// schema identities, archive integrity and model validation. Falls back to
/// the given initial value when the document belongs to another build.
pub fn hydrated<T: hyve::api::Wire + hyve::validate::Validate>(
    key: &str,
    initial: impl FnOnce() -> T,
) -> T {
    #[cfg(target_arch = "wasm32")]
    {
        use base64::Engine;
        let restore = || {
            let expected = option_env!("SERGE_BUILD_ID")?;
            let doc = web_sys::window()?.document()?;
            let node = doc.get_element_by_id(&format!("serge-state-{key}"))?;
            if node.tag_name() != "SCRIPT"
                || node.get_attribute("type").as_deref() != Some("application/octet-stream")
                || node.get_attribute("data-build").as_deref() != Some(expected)
                || node.get_attribute("data-schema")? != format!("{:016x}", T::FINGERPRINT)
            {
                return None;
            }
            let data = node.text_content()?;
            if data.len() > 1_398_104 {
                return None;
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .ok()?;
            if bytes.len() > 1024 * 1024 {
                return None;
            }
            let value = T::from_binary(&bytes).ok()?;
            hyve::validate::Valid::new(value)
                .ok()
                .map(|v| v.into_inner())
        };
        if let Some(value) = restore() {
            return value;
        }
    }
    let _ = key;
    initial()
}

pub use serge_style::{Color, Layout, Length, Theme, pct, px, rem};

#[cfg(not(target_arch = "wasm32"))]
fn export_page(app: serge_router::Assembled, output: &std::path::Path) -> std::io::Result<()> {
    let result = if let Ok(path) = std::env::var("SERGE_EXPORT_PATH") {
        let app = app.with_history(std::rc::Rc::new(serge_router::MemoryHistory::new(&path)));
        serge_web_ssr::render_element(&app.scope, &Theme::LIGHT, || app.element())
    } else {
        let mut routes = app.routes().collect::<Vec<_>>();
        if app.has_root() && !routes.iter().any(|r| r.0 == "/") {
            routes.push(("/", false));
        }
        serde_json::to_string(&routes)?
    };
    std::fs::write(output, result)
}

/// Testing without a window: the native runtime with simulated input
/// (`Headless`).
#[cfg(not(target_arch = "wasm32"))]
pub mod testing {
    pub use serge_native::{Headless, Key};
    pub use serge_native_cpu::{CpuRenderer, Frame};
    pub use serge_native_gpu::{GpuRenderer, Parity, test_renderer as gpu_test_renderer};

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
    pub use crate::dev_signal;
    #[cfg(target_arch = "wasm32")]
    pub use crate::persistent_signal;
    pub use crate::{LiveActivity, PlatformWidget, android, ios};
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
    pub use hyve::Value as WidgetValue;
    pub use hyve::state::untrack;
    pub use serge_core::Site;
    pub use serge_core::builder::{
        Build, Children, Const, Dyn, IntoArg, IntoProp, Lit, Set, Sited, Unset, child, element,
        fragment, keyed, region, site,
    };
    pub use serge_core::widgets::Styled;
    pub use serge_core::{Callback, Element, FromView, IntoElement, native, spawn, web};
    pub use serge_style::{pct, px, rem};
}

/// Local CPU, GPU and browser frame producers hosted within a Serge canvas.
pub mod viewport {
    pub use serge_core::viewport::{Extent, Pointer, Viewport};
    #[cfg(not(target_arch = "wasm32"))]
    pub use serge_platform_desktop::viewport::{GpuFrame, GpuViewport, wgpu};
    pub use serge_style::StyleProp;
    #[cfg(target_arch = "wasm32")]
    pub use serge_web_dom::WebViewport;
}
