//! HTTP handlers for health and map load/validate.

use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use horizon_map::map_from_slice;
use serde_json::json;

/// Liveness probe: `{ "ok": true }`.
pub async fn health() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

/// Return the currently loaded map, or 404 when none is loaded.
pub async fn get_map(State(state): State<AppState>) -> Response {
    let guard = state.map.read().await;
    match guard.as_ref() {
        Some(repo) => Json(repo).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "no map loaded; pass --map <file>, POST /api/map, or POST /api/analyse"
            })),
        )
            .into_response(),
    }
}

/// Validate a JSON map body via [`map_from_slice`], store it, and echo it back.
pub async fn post_map(State(state): State<AppState>, body: axum::body::Bytes) -> Response {
    let repo = match map_from_slice(&body) {
        Ok(repo) => repo,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("{err:#}") })),
            )
                .into_response();
        }
    };
    {
        let mut guard = state.map.write().await;
        *guard = Some(repo);
    }
    // Echo from the stored copy rather than cloning the parsed map: bodies run
    // to the 64 MiB limit, and a clone doubled peak memory for the whole
    // request just to serialize the same bytes back.
    let guard = state.map.read().await;
    match guard.as_ref() {
        Some(repo) => Json(repo).into_response(),
        // Unreachable in practice: only this function clears the slot, and it
        // never stores None.
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "map slot emptied while storing" })),
        )
            .into_response(),
    }
}
