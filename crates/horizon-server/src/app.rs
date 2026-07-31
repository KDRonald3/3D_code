//! Router construction for the Horizon map viewer server.

use crate::analyse::{get_analyse, post_analyse};
use crate::host_guard::reject_non_local_host;
use crate::routes::{get_map, health, post_map};
use crate::source::get_source;
use crate::state::AppState;
use crate::static_files::{diagnostics_js, function_dag_js, index, viewer_css, viewer_js};
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
        .route("/static/diagnostics.js", get(diagnostics_js))
        .route("/static/function_dag.js", get(function_dag_js))
        .route("/api/health", get(health))
        .route("/api/map", get(get_map).post(post_map))
        .route("/api/analyse", get(get_analyse).post(post_analyse))
        .route("/api/source", get(get_source))
        .layer(middleware::from_fn(reject_non_local_host))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}
