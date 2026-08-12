//! `server` binary — local web server for the seedfinder UI (§3, Phase 5).
//!
//! Serves the canvas UI, streams search results over SSE, and renders map tiles
//! server-side. Run with `cargo run -p server`. Default bind address `127.0.0.1:8080`.

use std::net::SocketAddr;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr: SocketAddr = std::env::var("SEEDFINDER_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    println!("seedfinder server listening on http://{addr}");
    server::serve(addr, server::AppState::default()).await
}
