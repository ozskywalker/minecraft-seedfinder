//! `server` — the local web server (§3, Phase 5).
//!
//! A single `.exe` that embeds and serves the static canvas UI, exposes a search
//! endpoint that streams results over SSE, and renders map tiles server-side (which
//! avoids WASM entirely — a simplification available only because this is localhost,
//! §3.2).
//!
//! Endpoints:
//! - `POST /api/search` — JSON body `{ dsl, low_start, low_end, high_start, high_end,
//!   max_per_candidate, include_biomes }`; responds with an `text/event-stream` of
//!   `SearchEvent`s (mode, then results as found, then done).
//! - `GET /api/tile/{seed}/{tx}/{tz}/{lod}` — a 512-block PNG tile.
//! - `GET /` — the static UI (served from `ui/dist` if present, else a placeholder).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Router,
};
use futures::stream::Stream;
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::search::SearchJob;
use crate::tiles::TileCache;

pub mod assets;
pub mod search;
pub mod tiles;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub tiles: Arc<TileCache>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            tiles: Arc::new(TileCache::new(256)),
        }
    }
}

/// The search request body.
#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub dsl: String,
    #[serde(default)]
    pub low_start: u32,
    #[serde(default = "default_low_end")]
    pub low_end: u32,
    #[serde(default)]
    pub high_start: u32,
    #[serde(default = "default_high_end")]
    pub high_end: u32,
    #[serde(default)]
    pub max_per_candidate: usize,
    #[serde(default = "default_true")]
    pub include_biomes: bool,
}

fn default_low_end() -> u32 {
    1000
}
fn default_high_end() -> u32 {
    100
}
fn default_true() -> bool {
    true
}

/// Build the application router.
///
/// Two serving modes:
/// - **Production (single `.exe`):** if the UI is embedded (see [`assets`]), all
///   non-API requests are served from the binary with an SPA fallback to `index.html`.
/// - **Dev:** otherwise the UI is served from the `SEEDFINDER_UI_DIR` dir (default
///   `../../ui/dist` anchored to the crate), falling back to a placeholder page.
pub fn app(state: AppState) -> Router {
    let api = Router::new()
        .route("/api/search", post(search_handler))
        .route("/api/tile/{seed}/{tx}/{tz}/{lod}", get(tile_handler))
        .route("/api/catalog", get(catalog_handler));

    if assets::EMBEDDED {
        api.fallback(assets::embedded_ui_handler).with_state(state)
    } else {
        // Default UI dir is anchored to the crate so it works regardless of cwd (tests
        // run from crates/server; the binary may be run from anywhere). Overridable.
        let ui_dir = std::env::var("SEEDFINDER_UI_DIR")
            .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/dist").to_string());
        let static_service = tower_http::services::ServeDir::new(&ui_dir).not_found_service(
            tower_http::services::ServeFile::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/index_placeholder.html"
            )),
        );
        api.fallback_service(static_service).with_state(state)
    }
}

/// The structure catalog: the authoritative list of structures (keys + biome gates +
/// shared-slot partners) from the embedded version table. The UI uses this to populate
/// the route-builder dropdowns without duplicating version data in JS.
async fn catalog_handler() -> Response {
    let v = be_struct::Version::builtin_1_21_40();
    let structures: Vec<serde_json::Value> = v
        .structures
        .iter()
        .map(|(key, s)| {
            serde_json::json!({
                "key": key,
                "biomes": s.biomes,
                "shares_slot_with": s.shares_slot_with,
            })
        })
        .collect();
    let body = serde_json::json!({
        "version": v.version,
        "seed_bits": v.seed_bits,
        "structures": structures,
    });
    (
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

/// The search endpoint: parse the job, then stream events over SSE.
async fn search_handler(
    State(_state): State<AppState>,
    axum::Json(req): axum::Json<SearchRequest>,
) -> Response {
    let job = match SearchJob::from_dsl(
        &req.dsl,
        req.low_start,
        req.low_end,
        req.high_start,
        req.high_end,
        req.max_per_candidate,
        req.include_biomes,
    ) {
        Ok(Ok(job)) => job,
        Ok(Err(reasons)) => {
            // Infeasible: stream the reasons as a note, then finish.
            let (tx, rx) = mpsc::channel::<search::SearchEvent>(4);
            let _ = tx
                .send(search::SearchEvent::Note(format!(
                    "query is infeasible: {}",
                    reasons.join("; ")
                )))
                .await;
            let _ = tx.send(search::SearchEvent::Done { count: 0 }).await;
            return sse_response(rx);
        }
        Err(e) => {
            return (StatusCode::BAD_REQUEST, e).into_response();
        }
    };

    let (tx, rx) = mpsc::channel::<search::SearchEvent>(32);
    let job = Arc::new(job);
    let tx2 = tx.clone();
    tokio::task::spawn_blocking(move || search::run_search(job, tx2));

    sse_response(rx)
}

/// Convert a channel of events into an SSE response.
fn sse_response(rx: mpsc::Receiver<search::SearchEvent>) -> Response {
    let stream = tokio_stream_rx(rx);
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// Adapt a `tokio::sync::mpsc::Receiver` into a `Stream<Item = Result<Event, ...>>`.
fn tokio_stream_rx(
    mut rx: mpsc::Receiver<search::SearchEvent>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    async_stream::stream! {
        while let Some(ev) = rx.recv().await {
            let payload = match ev {
                search::SearchEvent::Mode { mode, complete } => {
                    format!("{{\"type\":\"mode\",\"mode\":\"{mode}\",\"complete\":{complete}}}")
                }
                search::SearchEvent::Result { seed, positions } => {
                    let positions: Vec<String> = positions
                        .iter()
                        .map(|(n, x, z)| format!("{{\"name\":\"{n}\",\"x\":{x},\"z\":{z}}}"))
                        .collect();
                    format!("{{\"type\":\"result\",\"seed\":\"{seed:016x}\",\"positions\":[{}]}}", positions.join(","))
                }
                search::SearchEvent::Done { count } => {
                    format!("{{\"type\":\"done\",\"count\":{count}}}")
                }
                search::SearchEvent::Note(s) => {
                    format!("{{\"type\":\"note\",\"message\":\"{}\"}}", s.replace('"', "\\\""))
                }
            };
            yield Ok(Event::default().data(payload));
        }
    }
}

/// Serve a rendered tile PNG.
async fn tile_handler(
    State(state): State<AppState>,
    Path((seed, tx, tz, lod)): Path<(u64, i64, i64, u32)>,
) -> Response {
    let bytes = tiles::cached_tile(&state.tiles, seed, tx, tz, lod);
    (
        [(header::CONTENT_TYPE, "image/png")],
        bytes.as_ref().clone(),
    )
        .into_response()
}

/// Run the server on `addr`, returning when it's ready (for tests) — otherwise the
/// caller awaits forever.
pub async fn serve(addr: SocketAddr, state: AppState) -> std::io::Result<()> {
    serve_with(addr, state, None).await
}

/// Run the server, invoking `on_ready` (with the bound address) once the listener is up.
/// `main` uses this to open the default browser in the background. Tests pass `None`.
pub async fn serve_with(
    addr: SocketAddr,
    state: AppState,
    on_ready: Option<Box<dyn FnOnce(SocketAddr) + Send>>,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    if let Some(f) = on_ready {
        f(bound);
    }
    axum::serve(listener, app(state))
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))
}

/// The command (program + args) used to open a URL in the platform's default browser.
/// Pure so it's unit-testable; `open_browser` spawns it.
pub fn browser_command(url: &str) -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        // Pass each token as its own argument. If we embedded the whole
        // `start "" "url"` as one string, Rust's Windows quoting would wrap it and
        // escape the inner `"` as `\"`, which `cmd` then sees as literal backslashes
        // (`start \ \http://...\`) and errors on. Splitting the tokens keeps the
        // empty window-title and the URL unquoted so `cmd` parses them correctly.
        vec![
            "cmd".to_string(),
            "/C".to_string(),
            "start".to_string(),
            String::new(), // empty window title — without it `start` treats the URL as the title
            url.to_string(),
        ]
    }
    #[cfg(target_os = "macos")]
    {
        vec!["open".to_string(), url.to_string()]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        vec!["xdg-open".to_string(), url.to_string()]
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
    {
        let _ = url;
        vec![]
    }
}

/// Open `url` in the default browser (best-effort, detached, never blocks the server).
pub fn open_browser(url: &str) {
    let cmd = browser_command(url);
    if cmd.is_empty() {
        return;
    }
    let mut c = std::process::Command::new(&cmd[0]);
    c.args(&cmd[1..]);
    let _ = c.spawn(); // best-effort; ignore failure (e.g. headless CI)
}

// Re-export not needed; `async-stream` is used directly via the macro.

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn default_state_is_reasonable() {
        let state = AppState::default();
        // A sane tile cache default: enough to hold a modest viewport without unbounded
        // memory growth.
        assert!(state.tiles.get("0:0:0:0").is_none());
    }

    #[test]
    fn browser_command_opens_default_browser() {
        #[cfg(target_os = "windows")]
        {
            let cmd = browser_command("http://127.0.0.1:8080");
            assert_eq!(cmd[0], "cmd");
            assert_eq!(cmd[1], "/C");
            assert_eq!(cmd[2], "start");
            // Empty window title, then the URL, as separate tokens.
            assert_eq!(cmd[3], "");
            assert!(cmd[4].contains("http://127.0.0.1:8080"));
        }
        #[cfg(target_os = "macos")]
        {
            assert_eq!(
                browser_command("http://x"),
                vec!["open".to_string(), "http://x".to_string()]
            );
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            assert_eq!(
                browser_command("http://x"),
                vec!["xdg-open".to_string(), "http://x".to_string()]
            );
        }
    }

    #[tokio::test]
    async fn serve_with_invokes_ready_with_bound_addr() {
        use std::net::SocketAddr;
        use tokio::sync::oneshot;

        let (tx, rx) = oneshot::channel::<SocketAddr>();
        let state = AppState::default();
        let task = tokio::spawn(async move {
            let _ = serve_with(
                "127.0.0.1:0".parse().unwrap(),
                state,
                Some(Box::new(move |addr| {
                    // oneshot send is non-blocking, safe to call from the async runtime.
                    let _ = tx.send(addr);
                })),
            )
            .await;
        });
        let bound = tokio::time::timeout(std::time::Duration::from_secs(5), rx)
            .await
            .expect("ready callback fired")
            .expect("received addr");
        assert!(bound.port() != 0, "bound to an ephemeral port");
        task.abort();
    }

    #[tokio::test]
    async fn search_endpoint_streams_mode_and_done() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let state = AppState::default();
        let app = app(state);
        let req = Request::builder()
            .method("POST")
            .uri("/api/search")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"dsl":"village v1 @origin <= 800","low_start":0,"low_end":20,"include_biomes":false}"#,
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let headers = resp.headers();
        assert!(
            headers
                .get("content-type")
                .map(|v| v.to_str().unwrap().contains("text/event-stream"))
                .unwrap_or(false),
            "expected text/event-stream, got {:?}",
            headers.get("content-type")
        );
        // Read a bit of the body to confirm events flow.
        let body = resp.into_body();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("\"type\":\"mode\""), "body: {text}");
        assert!(text.contains("\"type\":\"done\""), "body: {text}");
    }

    #[tokio::test]
    async fn tile_endpoint_returns_png() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let state = AppState::default();
        let app = app(state);
        let req = Request::builder()
            .uri("/api/tile/42/0/0/0")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ct.contains("image/png"), "content-type: {ct}");
        let _ = Duration::new(0, 0);
    }

    #[tokio::test]
    async fn index_serves_html() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let state = AppState::default();
        let app = app(state);
        let req = Request::builder().uri("/").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ct.contains("text/html"), "content-type: {ct}");
    }

    #[tokio::test]
    async fn catalog_endpoint_lists_structures() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let state = AppState::default();
        let app = app(state);
        let req = Request::builder()
            .uri("/api/catalog")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ct.contains("application/json"), "content-type: {ct}");
        let body = resp.into_body();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let structures = json["structures"].as_array().unwrap();
        let keys: Vec<&str> = structures
            .iter()
            .map(|s| s["key"].as_str().unwrap())
            .collect();
        assert!(
            keys.contains(&"village"),
            "catalog must list village, got {keys:?}"
        );
        assert!(
            keys.contains(&"desert_pyramid"),
            "catalog must list desert_pyramid, got {keys:?}"
        );
    }

    #[tokio::test]
    async fn infeasible_search_returns_note() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let state = AppState::default();
        let app = app(state);
        let req = Request::builder()
            .method("POST")
            .uri("/api/search")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"dsl":"desert_pyramid a @origin <= 2000\njungle_pyramid b @a <= 400","low_end":10}"#,
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = resp.into_body();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("infeasible"), "body: {text}");
    }
}
