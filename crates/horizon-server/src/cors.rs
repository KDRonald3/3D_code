//! CORS for the IDE workbench renderer.
//!
//! The Horizon IDE calls this sidecar with `fetch` from the Electron workbench,
//! whose origin is the opaque `vscode-file://vscode-app` — so every request is
//! cross-origin and a JSON `POST /api/analyse` triggers a preflight. Without
//! these headers the browser drops the response before the IDE ever sees it.
//!
//! Permissive by design: [`reject_non_local_host`](crate::host_guard) already
//! confines the surface to loopback, and no credentials are ever sent.

use axum::body::Body;
use axum::http::{header, HeaderValue, Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

/// Axum middleware: answer preflights and tag every response with CORS headers.
pub async fn allow_cross_origin(request: Request<Body>, next: Next) -> Response {
    let is_preflight = request.method() == Method::OPTIONS;

    let mut response = if is_preflight {
        // Never dispatch a preflight to a route: OPTIONS has no handler here and
        // would 405 with headers the browser then refuses to read.
        let mut preflight = Response::new(Body::empty());
        *preflight.status_mut() = StatusCode::NO_CONTENT;
        preflight
    } else {
        next.run(request).await
    };

    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("content-type"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("600"),
    );

    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    fn app() -> Router {
        Router::new()
            .route("/api/health", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(allow_cross_origin))
    }

    #[tokio::test]
    async fn tags_normal_responses_with_allow_origin() {
        let res = app()
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "*"
        );
    }

    #[tokio::test]
    async fn answers_preflight_without_hitting_a_route() {
        let res = app()
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    // No OPTIONS route exists; the middleware must short-circuit.
                    .uri("/api/analyse")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
                .unwrap(),
            "content-type"
        );
    }
}
