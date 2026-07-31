//! DNS-rebinding guard: reject `/api/*` requests whose `Host` is not loopback.
//!
//! Precedent: abandoned `my-local-name` visualizer server. Because the bind
//! address is always loopback in v1, every API request must carry a local Host.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Axum middleware: 403 JSON `{error}` when an `/api/*` request's Host is not local.
pub async fn reject_non_local_host(request: Request<Body>, next: Next) -> Response {
    let path = request.uri().path();
    if path.starts_with("/api/") && !host_is_local(request.headers()) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "request Host is not local" })),
        )
            .into_response();
    }
    next.run(request).await
}

/// True when the request's `Host` header (ignoring any `:port`) is a loopback
/// name/address. Requests without a `Host` header are rejected.
pub fn host_is_local(headers: &axum::http::HeaderMap) -> bool {
    let Some(value) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    // Strip the port:
    // - `[::1]:8787` → `::1`
    // - `127.0.0.1:12345` / `localhost:8080` → host before the last `:`
    // - bare `::1` (no brackets) must not be split on `:`
    let hostname = if let Some(rest) = value.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else if value.matches(':').count() > 1 {
        // Unbracketed IPv6 — no port form we accept without brackets.
        value
    } else {
        value.rsplit_once(':').map(|(h, _)| h).unwrap_or(value)
    };
    matches!(hostname, "localhost" | "127.0.0.1" | "::1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    fn host_map(host: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, host.parse().unwrap());
        headers
    }

    #[test]
    fn accepts_loopback_hosts() {
        assert!(host_is_local(&host_map("127.0.0.1")));
        assert!(host_is_local(&host_map("127.0.0.1:12345")));
        assert!(host_is_local(&host_map("localhost")));
        assert!(host_is_local(&host_map("localhost:8080")));
        assert!(host_is_local(&host_map("[::1]")));
        assert!(host_is_local(&host_map("[::1]:8787")));
        assert!(host_is_local(&host_map("::1")));
    }

    #[test]
    fn rejects_non_local_and_missing() {
        assert!(!host_is_local(&HeaderMap::new()));
        assert!(!host_is_local(&host_map("evil.example")));
        assert!(!host_is_local(&host_map("evil.example:80")));
        assert!(!host_is_local(&host_map("192.168.1.1")));
    }
}
