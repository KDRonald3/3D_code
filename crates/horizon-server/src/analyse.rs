//! `POST /api/analyse` — run the function-map pipeline in process.
//!
//! The server already depends on `horizon-engine`. This endpoint accepts a
//! repository path, runs [`horizon_engine::build_function_map`] on a blocking
//! thread, and stores the result in the shared map slot. Only one analysis
//! runs at a time; the UI polls [`get_analyse`] so a long run never looks like
//! a hung page.
//!
//! Loopback bind + [`crate::host_guard`] still apply — this is not a remote
//! scan API.

use crate::state::{AnalyseStatus, AppState};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Deserialize)]
pub struct AnalyseRequest {
    /// Absolute (or canonicalize-able) path to a Cargo repository root.
    pub path: String,
}

/// Snapshot of the in-flight / last analysis for the UI progress poll.
pub async fn get_analyse(State(state): State<AppState>) -> Json<Value> {
    let guard = state.analyse.read().await;
    Json(status_to_json(&guard))
}

/// Start an in-process analysis of `path`. Returns immediately with
/// `status: "running"`; poll [`get_analyse`] until `done` or `failed`.
///
/// Concurrent starts while a job is running are rejected with 409 so the UI
/// cannot stack analyses.
pub async fn post_analyse(
    State(state): State<AppState>,
    Json(body): Json<AnalyseRequest>,
) -> Response {
    let raw = body.path.trim();
    if raw.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "path must be a non-empty directory" })),
        )
            .into_response();
    }

    let path = PathBuf::from(raw);
    let path = match canonicalize_repo(&path) {
        Ok(p) => p,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": err })),
            )
                .into_response();
        }
    };

    {
        let mut slot = state.analyse.write().await;
        if matches!(*slot, AnalyseStatus::Running { .. }) {
            // Echo the running status at the top level so the UI can attach
            // without unwrapping a nested object.
            let mut body = status_to_json(&slot);
            if let Some(obj) = body.as_object_mut() {
                obj.insert(
                    "error".into(),
                    json!("an analysis is already running"),
                );
            }
            return (StatusCode::CONFLICT, Json(body)).into_response();
        }
        *slot = AnalyseStatus::Running {
            path: path.clone(),
            started: Instant::now(),
        };
    }

    let state_bg = state.clone();
    let path_bg = path.clone();
    tokio::spawn(async move {
        let path_for_blocking = path_bg.clone();
        let outcome =
            tokio::task::spawn_blocking(move || horizon_engine::build_function_map(&path_for_blocking))
                .await;

        match outcome {
            Ok(Ok(repo)) => {
                {
                    let mut map = state_bg.map.write().await;
                    *map = Some(repo);
                }
                let mut slot = state_bg.analyse.write().await;
                *slot = AnalyseStatus::Done {
                    path: path_bg,
                    finished: Instant::now(),
                };
            }
            Ok(Err(err)) => {
                let mut slot = state_bg.analyse.write().await;
                *slot = AnalyseStatus::Failed {
                    path: path_bg,
                    error: format!("{err:#}"),
                    finished: Instant::now(),
                };
            }
            Err(join_err) => {
                let mut slot = state_bg.analyse.write().await;
                *slot = AnalyseStatus::Failed {
                    path: path_bg,
                    error: format!("analysis task failed: {join_err}"),
                    finished: Instant::now(),
                };
            }
        }
    });

    let guard = state.analyse.read().await;
    (
        StatusCode::ACCEPTED,
        Json(status_to_json(&guard)),
    )
        .into_response()
}

fn canonicalize_repo(path: &Path) -> Result<PathBuf, String> {
    if !path.exists() {
        return Err(format!("path does not exist: {}", path.display()));
    }
    let canon = path
        .canonicalize()
        .map_err(|e| format!("cannot resolve {}: {e}", path.display()))?;
    if !canon.is_dir() {
        return Err(format!("path is not a directory: {}", canon.display()));
    }
    Ok(canon)
}

fn status_to_json(status: &AnalyseStatus) -> Value {
    match status {
        AnalyseStatus::Idle => json!({ "status": "idle" }),
        AnalyseStatus::Running { path, started } => json!({
            "status": "running",
            "path": path.to_string_lossy(),
            "elapsed_ms": started.elapsed().as_millis() as u64,
        }),
        AnalyseStatus::Done { path, finished } => json!({
            "status": "done",
            "path": path.to_string_lossy(),
            // Age since completion — UI can clear the banner after a beat.
            "completed_ms_ago": finished.elapsed().as_millis() as u64,
        }),
        AnalyseStatus::Failed { path, error, finished } => json!({
            "status": "failed",
            "path": path.to_string_lossy(),
            "error": error,
            "completed_ms_ago": finished.elapsed().as_millis() as u64,
        }),
    }
}
