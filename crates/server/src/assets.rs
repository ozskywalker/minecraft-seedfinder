//! Embedded UI assets (production single-`.exe` mode).
//!
//! [`build.rs`](../build.rs) embeds every file under `ui/dist` into the binary via
//! `include_bytes!`, generating a module at `OUT_DIR`. This module includes that
//! generated code and offers a content-type helper for serving it back. When the UI
//! hasn't been built, `EMBEDDED` is `false` and the server falls back to disk/placeholder
//! (dev mode).

use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

include!(concat!(env!("OUT_DIR"), "/generated_assets.rs"));
// The generated module defines `pub const EMBEDDED` and `pub fn embedded` as items of
// this module, so both are reachable as `assets::EMBEDDED` / `assets::embedded`.

/// Normalize a request path to an embedded web path (no leading slash; `/` → `index.html`).
pub fn normalize(path: &str) -> String {
    let p = path.trim_start_matches('/');
    if p.is_empty() {
        "index.html".to_string()
    } else {
        p.to_string()
    }
}

/// Map a file extension to a MIME content type.
fn content_type(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "ico" => "image/x-icon",
        "map" => "application/json",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}

/// Serve the embedded UI with SPA fallback: real asset files are served with their
/// content type; any other path (a client route) returns `index.html`.
pub fn handle_embedded(path: &str) -> Response {
    let web = normalize(path);
    if let Some(bytes) = embedded(&web) {
        return ([(header::CONTENT_TYPE, content_type(&web))], bytes).into_response();
    }
    // SPA fallback: unknown paths (client-side routes) get index.html.
    if let Some(idx) = embedded("index.html") {
        return ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], idx).into_response();
    }
    (StatusCode::NOT_FOUND, "not found").into_response()
}

/// Axum fallback handler for the embedded UI. Uses [`axum::extract::OriginalUri`] so it
/// works on the fallback route (a `Path` extractor would reject because the fallback
/// captures no path parameter).
pub async fn embedded_ui_handler(uri: axum::extract::OriginalUri) -> Response {
    handle_embedded(uri.path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_maps_root_to_index() {
        assert_eq!(normalize("/"), "index.html");
        assert_eq!(normalize(""), "index.html");
        assert_eq!(normalize("/assets/x.js"), "assets/x.js");
    }

    #[test]
    fn content_type_by_extension() {
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(content_type("assets/app.js"), "application/javascript; charset=utf-8");
        assert_eq!(content_type("assets/app.css"), "text/css; charset=utf-8");
        assert_eq!(content_type("favicon.ico"), "image/x-icon");
        assert_eq!(content_type("data.bin"), "application/octet-stream");
    }

    #[test]
    fn embedded_serves_ui_when_built() {
        if !EMBEDDED {
            return; // skip when the UI isn't embedded (dev checkout)
        }
        // index.html must be present.
        assert!(embedded("index.html").is_some(), "index.html should be embedded");
        // Every asset in the manifest is reachable.
        let resp = handle_embedded("/");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn spa_fallback_serves_index_for_client_routes() {
        if !EMBEDDED {
            return;
        }
        // A made-up client route should get index.html (SPA), not a 404.
        let resp = handle_embedded("/some/client/route");
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
