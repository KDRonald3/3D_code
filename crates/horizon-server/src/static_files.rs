//! Embedded static UI assets.
//!
//! Compiled into the binary with `include_str!` so the tool is a single
//! self-contained executable with no runtime path assumptions.

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

pub const INDEX_HTML: &str = include_str!("../web/index.html");
pub const VIEWER_CSS: &str = include_str!("../web/viewer.css");
pub const VIEWER_JS: &str = include_str!("../web/viewer.js");

pub async fn index() -> Response {
    html(INDEX_HTML)
}

pub async fn viewer_css() -> Response {
    css(VIEWER_CSS)
}

pub async fn viewer_js() -> Response {
    js(VIEWER_JS)
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
