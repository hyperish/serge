//! The serge playground: the README and `docs/01-dx-and-dsl.md` examples as
//! one app, on the web (`hyve-serge preview --web`) and on the desktop
//! (`cargo run -p serge-playground`), calling an orbic backend
//! (`playground-server`) or, in the single-binary desktop build, the same
//! backend in process.
#![allow(missing_docs)]

mod screens;
mod server;

use std::sync::Arc;

use hyve::api::Transport;
use serge::prelude::*;

pub use screens::*;
pub use server::*;

/// The whole app, calling `#[server]` functions through `transport`.
pub fn app(transport: Arc<dyn Transport>) -> SergeApp {
    SergeApp::new()
        .title("serge playground")
        .provide(AuthState::new(false))
        .transport(transport)
        .layout(|screen| ui! { Shell { {screen} } })
        .feature(
            feature()
                .route("/", Home)
                .route("/counter", CounterScreen)
                .route("/todos", TodoScreen)
                .route("/telemetry", TelemetryScreen)
                .route("/login", Login),
        )
        .feature(project_feature().route("/projects/saved", SavedProjects))
}

/// The web entry: same-origin API under `/v1` (the dev server proxies it).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    app(RemoteTransport::new("/v1").into_arc()).run();
}
