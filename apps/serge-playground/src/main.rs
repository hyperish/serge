//! The playground on the desktop: `cargo run -p serge-playground`.
//!
//! - `SERGE_API_URL=http://127.0.0.1:3000/v1` calls a running
//!   `playground-server`.
//! - With `--features server` and no `SERGE_API_URL`, the backend runs in
//!   this process (one binary, no port).
//! - Otherwise it calls `http://127.0.0.1:3000/v1`.

use std::sync::Arc;

use hyve::api::Transport;
use serge::prelude::*;

fn transport() -> Arc<dyn Transport> {
    if let Ok(url) = std::env::var("SERGE_API_URL") {
        eprintln!("serge playground: calling {url}");
        return RemoteTransport::new(&url).into_arc();
    }
    #[cfg(feature = "server")]
    {
        eprintln!("serge playground: backend in process");
        let pipeline = serge_playground::backend()
            .build()
            .unwrap_or_else(|e| panic!("{e}"));
        Arc::new(orbic::InProcess::new(pipeline))
    }
    #[cfg(not(feature = "server"))]
    {
        eprintln!(
            "serge playground: calling http://127.0.0.1:3000/v1 (set SERGE_API_URL to change)"
        );
        RemoteTransport::new("http://127.0.0.1:3000/v1").into_arc()
    }
}

fn main() {
    serge_playground::app(transport()).run();
}
