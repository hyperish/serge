//! The playground backend: `cargo run -p serge-playground --features server
//! --bin playground-server` (listens on `SERGE_PLAYGROUND_ADDR`, default
//! `127.0.0.1:3000`; plain HTTP is loopback only).

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = std::env::var("SERGE_PLAYGROUND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!(
        "playground backend on http://{}/v1 (Ctrl-C stops)",
        listener.local_addr()?
    );
    orbic::serve(serge_playground::backend(), listener, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
}
