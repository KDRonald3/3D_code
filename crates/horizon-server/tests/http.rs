//! In-process HTTP checks for the Phase A viewer server.

use axum::body::{to_bytes, Body};
use http::{Request, StatusCode};
use horizon_map::{
    map_from_slice, map_to_string, CallSite, CallTarget, Conflict, Crate, File, Function,
    FunctionId, MapSummary, Repository, UnresolvedCall,
};
use horizon_server::{app, AppState};
use std::path::PathBuf;
use tower::ServiceExt;

fn sample_repo() -> Repository {
    let get_shapes = FunctionId::from_parts("glob_ambiguity", "crate::shapes::get", None);
    let get_text = FunctionId::from_parts("glob_ambiguity", "crate::text::get", None);
    let run = FunctionId::from_parts("glob_ambiguity", "crate::app::run", None);

    let mut summary = MapSummary::empty();
    summary.record_conflict();
    summary.record_unresolved();

    Repository {
        root: PathBuf::from("C:\\tmp\\glob-ambiguity"),
        summary,
        crates: vec![Crate {
            name: "glob-ambiguity".into(),
            rustc_name: "glob_ambiguity".into(),
            is_library: true,
            edition: "2021".into(),
            roots: vec![PathBuf::from("C:\\tmp\\glob-ambiguity\\src\\lib.rs")],
            dependencies: vec![],
            folders: vec![],
            files: vec![File {
                path: PathBuf::from("C:\\tmp\\glob-ambiguity\\src\\app.rs"),
                module_path: "crate::app".into(),
                content_hash: "0".repeat(64),
                functions: vec![Function {
                    id: run,
                    name: "run".into(),
                    module_path: "crate::app::run".into(),
                    line: 3,
                    byte_start: 0,
                    byte_end: 80,
                    call_sites: vec![
                        CallSite {
                            call_path: "get".into(),
                            line: 5,
                            byte_start: 40,
                            byte_end: 43,
                            target: CallTarget::Conflict(Conflict {
                                candidates: vec![get_shapes, get_text],
                                reason: "ambiguous glob imports".into(),
                            }),
                            from_macro: false,
                        },
                        CallSite {
                            call_path: "mystery".into(),
                            line: 6,
                            byte_start: 50,
                            byte_end: 57,
                            target: CallTarget::Unresolved(UnresolvedCall {
                                reason: "no indexed definition".into(),
                            }),
                            from_macro: false,
                        },
                    ],
                    doc_comments: vec![],
                }],
                call_sites: vec![],
                doc_comments: vec![],
            }],
        }],
    }
}

async fn body_string(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.expect("body");
    String::from_utf8(bytes.to_vec()).expect("utf8")
}

#[tokio::test]
async fn health_ok_with_local_host() {
    let router = app(AppState::new(None));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .header("Host", "127.0.0.1:12345")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response).await;
    assert!(body.contains("\"ok\":true") || body.contains("\"ok\": true"));
}

#[tokio::test]
async fn api_rejects_non_local_host() {
    let router = app(AppState::new(None));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .header("Host", "evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = body_string(response).await;
    assert!(body.contains("not local"));
}

#[tokio::test]
async fn serves_index_and_static_assets() {
    let router = app(AppState::new(None));

    for (uri, needle) in [
        ("/", "Horizon Map Viewer"),
        ("/static/viewer.css", "--resolved: #5dce8a"),
        ("/static/viewer.js", "loadMap"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header("Host", "127.0.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        let body = body_string(response).await;
        assert!(body.contains(needle), "{uri} missing {needle}");
    }
}

#[tokio::test]
async fn get_map_404_when_empty_and_returns_loaded() {
    let empty = app(AppState::new(None));
    let missing = empty
        .oneshot(
            Request::builder()
                .uri("/api/map")
                .header("Host", "localhost")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let repo = sample_repo();
    let loaded = app(AppState::new(Some(repo.clone())));
    let response = loaded
        .oneshot(
            Request::builder()
                .uri("/api/map")
                .header("Host", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response).await;
    let restored = map_from_slice(body.as_bytes()).expect("deserialize");
    assert_eq!(restored.root, repo.root);
    assert_eq!(restored.summary.conflicts, 1);
}

#[tokio::test]
async fn post_map_validates_and_stores() {
    let state = AppState::new(None);
    let router = app(state.clone());
    let json = map_to_string(&sample_repo()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/map")
                .header("Host", "127.0.0.1")
                .header("Content-Type", "application/json")
                .body(Body::from(json))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let guard = state.map.read().await;
    assert!(guard.is_some());
    assert_eq!(guard.as_ref().unwrap().summary.conflicts, 1);
}

#[tokio::test]
async fn post_map_rejects_invalid_json() {
    let router = app(AppState::new(None));
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/map")
                .header("Host", "127.0.0.1")
                .header("Content-Type", "application/json")
                .body(Body::from("{not a map"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
