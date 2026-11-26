//! The playground backend: `cargo run -p serge-playground --features server
//! --bin playground-server` (listens on `SERGE_PLAYGROUND_ADDR`, default
//! `127.0.0.1:3000`; plain HTTP is loopback only).

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = serge_playground::backend(orbic::Db::from_config().await?).await?;
    #[cfg(feature = "web-host")]
    let app = if let Some(root) = std::env::var_os("SERGE_BUNDLE_DIR") {
        serge_playground::with_web_bundle(app, std::path::Path::new(&root))?
    } else {
        app
    };
    #[cfg(all(feature = "ipc", any(unix, windows)))]
    if let Some(path) = std::env::var_os("SERGE_PLAYGROUND_IPC") {
        let listener = orbic::ipc::Listener::bind(std::path::Path::new(&path))?;
        println!("playground backend on local IPC");
        orbic::serve(app, listener, async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
        return Ok(());
    }
    let addr = std::env::var("SERGE_PLAYGROUND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!(
        "playground backend on http://{}/v1 (Ctrl-C stops)",
        listener.local_addr()?
    );
    orbic::serve(app, listener, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}
