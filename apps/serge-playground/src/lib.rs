//! The serge playground: the README and `docs/01-dx-and-dsl.md` examples as
//! one app, on the web (`hyve-serge dev --web`) and on the desktop
//! (`cargo run -p serge-playground`), calling an orbic backend
//! (`playground-server`) or, in the single-binary desktop build, the same
//! backend in process.
#![allow(missing_docs)]

pub mod platform_widgets;
mod screens;
mod server;
mod session;

use std::sync::Arc;

use serge::prelude::*;

pub use screens::*;
pub use server::*;
pub use session::PlaygroundSession;

/// The whole app, calling `#[server]` functions through its session.
pub fn app(session: PlaygroundSession) -> SergeApp {
    SergeApp::new()
        .title("serge playground")
        .provide(AuthState::new(false))
        .transport(Arc::new(session.clone()))
        .provide(session)
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
    app(PlaygroundSession::remote(RemoteTransport::new("/v1"))).run();
}

/// The Android NativeActivity entry.
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
fn android_main(host: serge::android::AndroidApp) {
    if let Err(error) = serge::android::launch(
        host,
        app(PlaygroundSession::remote(RemoteTransport::new(
            option_env!("SERGE_API_URL").unwrap_or("http://127.0.0.1:3000/v1"),
        ))),
    ) {
        eprintln!("Serge activity failed: {error}");
    }
}
