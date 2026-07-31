//! HTTP handlers for health and map load/validate.

use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use horizon_map::map_from_slice;
use serde_json::json;

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
        *guard = Some(repo.clone());
    }
    Json(repo).into_response()
}
