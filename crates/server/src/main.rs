//! `server` binary — local web server for the seedfinder UI (§3, Phase 5).
//!
//! Production: build with the UI embedded (see `scripts/build-release.ps1`) so this is a
//! single self-contained `.exe`. On startup it binds the port and opens the default
//! browser to the UI automatically, so a non-technical user just double-clicks the exe.
//!
//! Run with `cargo run -p server`. Default bind address `127.0.0.1:8080`.
//!
//! Env vars:
//! - `SEEDFINDER_ADDR` — bind address (default `127.0.0.1:8080`).
//! - `SEEDFINDER_NO_OPEN=1` — skip opening the browser (headless/dev/CI).
//! - `SEEDFINDER_UI_DIR` — only used in dev mode (when the UI is not embedded).

use std::net::SocketAddr;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr: SocketAddr = std::env::var("SEEDFINDER_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    println!("seedfinder server listening on http://{addr}");

    // Open the browser once the port is bound, unless disabled.
    let no_open = std::env::var("SEEDFINDER_NO_OPEN")
        .map(|v| v == "1")
        .unwrap_or(false);
    let on_ready: Option<Box<dyn FnOnce(SocketAddr) + Send>> = if no_open {
        None
    } else {
        Some(Box::new(|bound: SocketAddr| {
            let url = format!("http://{bound}");
            println!("opening browser at {url}");
            server::open_browser(&url);
        }))
    };

    server::serve_with(addr, server::AppState::default(), on_ready).await
}
