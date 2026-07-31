//! Embedded static UI assets.
//!
//! Compiled into the binary with `include_str!` so the tool is a single
//! self-contained executable with no runtime path assumptions.

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

pub const INDEX_HTML: &str = include_str!("../web/index.html");
pub const VIEWER_CSS: &str = include_str!("../web/viewer.css");
pub const VIEWER_JS: &str = include_str!("../web/viewer.js");
pub const DIAGNOSTICS_JS: &str = include_str!("../web/diagnostics.js");
pub const FUNCTION_DAG_JS: &str = include_str!("../web/function_dag.js");
/// Shared rail-aggressor fixture table (Rust oracle ↔ JS twin).
pub const RAIL_LAYOUT_CASES_JSON: &str = include_str!("../web/rail_layout_cases.json");

pub async fn index() -> Response {
    html(INDEX_HTML)
}

pub async fn viewer_css() -> Response {
    css(VIEWER_CSS)
}

pub async fn viewer_js() -> Response {
    js(VIEWER_JS)
}

pub async fn diagnostics_js() -> Response {
    js(DIAGNOSTICS_JS)
}

pub async fn function_dag_js() -> Response {
    js(FUNCTION_DAG_JS)
}

pub async fn rail_layout_cases() -> Response {
    json(RAIL_LAYOUT_CASES_JSON)
}

fn html(body: &'static str) -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )],
        body,
    )
        .into_response()
}

fn css(body: &'static str) -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/css; charset=utf-8"),
        )],
        body,
    )
        .into_response()
}

fn js(body: &'static str) -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/javascript; charset=utf-8"),
        )],
        body,
    )
        .into_response()
}

fn json(body: &'static str) -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        )],
        body,
    )
        .into_response()
}
