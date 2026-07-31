//! Router construction for the Horizon map viewer server.

use crate::host_guard::reject_non_local_host;
use crate::routes::{get_map, health, post_map};
use crate::state::AppState;
use crate::static_files::{index, viewer_css, viewer_js};
use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::routing::get;
use axum::Router;

/// 64 MiB — large enough for scale-corpus maps; matches the abandoned branch cap.
const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

/// Build the application router with the given shared state.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/static/viewer.css", get(viewer_css))
        .route("/static/viewer.js", get(viewer_js))
        .route("/api/health", get(health))
        .route("/api/map", get(get_map).post(post_map))
        .layer(middleware::from_fn(reject_non_local_host))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}
