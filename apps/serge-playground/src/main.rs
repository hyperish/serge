//! The playground on the desktop: `cargo run -p serge-playground`.
//!
//! - `SERGE_API_URL=http://127.0.0.1:3000/v1` calls a running
//!   `playground-server`.
//! - With `--features server` and no `SERGE_API_URL`, the backend runs in
//!   this process (one binary, no port).
//! - Otherwise it calls `http://127.0.0.1:3000/v1`.

use serge::prelude::*;
use serge_playground::PlaygroundSession;

fn session() -> Result<PlaygroundSession, Box<dyn std::error::Error>> {
    #[cfg(all(feature = "ipc", any(unix, windows), not(target_arch = "wasm32")))]
    if let Some(path) = std::env::var_os("SERGE_PLAYGROUND_IPC") {
        return Ok(PlaygroundSession::remote(RemoteTransport::local(
            path, "/v1",
        )));
    }
    if let Ok(url) = std::env::var("SERGE_API_URL") {
        eprintln!("serge playground: calling {url}");
        return Ok(PlaygroundSession::remote(RemoteTransport::new(&url)));
    }
    #[cfg(feature = "server")]
    {
        eprintln!("serge playground: backend in process");
        let app = serge::runtime()
            .block_on(async { serge_playground::backend(orbic::Db::from_config().await?).await })?;
        Ok(PlaygroundSession::in_process(app.build()?))
    }
    #[cfg(not(feature = "server"))]
    {
        eprintln!(
            "serge playground: calling http://127.0.0.1:3000/v1 (set SERGE_API_URL to change)"
        );
        Ok(PlaygroundSession::remote(RemoteTransport::new(
            "http://127.0.0.1:3000/v1",
        )))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    serge_playground::app(session()?).run();
    Ok(())
}
