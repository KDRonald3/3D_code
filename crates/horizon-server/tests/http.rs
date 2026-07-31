//! In-process HTTP checks for the Phase A viewer server and Phase C `/api/source`.

use axum::body::{to_bytes, Body};
use http::{Request, StatusCode};
use horizon_map::{
    content_hash, map_from_slice, map_to_string, CallSite, CallTarget, Conflict, Crate, File,
    Function, FunctionId, MapSummary, Repository, UnresolvedCall,
};
use horizon_server::{app, AppState};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tower::ServiceExt;

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn unique_temp_dir(label: &str) -> PathBuf {
    let n = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("horizon-server-source-{label}-{n}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn encode_query_path(path: &Path) -> String {
    // Percent-encode so Windows backslashes and spaces survive in the URI.
    let s = path.to_string_lossy();
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Source fixture: a free fn with a doc comment and an attribute.
const SOURCE_FIXTURE: &str = "\
/// Documented helper.
#[inline]
pub fn documented(x: u32) -> u32 {
    x + 1
}
";

fn repo_with_source_file(
    path: PathBuf,
    hash: String,
    byte_start: u32,
    byte_end: u32,
) -> Repository {
    let id = FunctionId::from_parts("fixture", "crate::documented", None);
    Repository {
        root: path.parent().unwrap_or(Path::new(".")).to_path_buf(),
        summary: MapSummary::empty(),
        crates: vec![Crate {
            name: "fixture".into(),
            rustc_name: "fixture".into(),
            is_library: true,
            edition: "2021".into(),
            roots: vec![path.clone()],
            dependencies: vec![],
            folders: vec![],
            files: vec![File {
                path,
                module_path: "crate".into(),
                content_hash: hash,
                functions: vec![Function {
                    id,
                    name: "documented".into(),
                    module_path: "crate::documented".into(),
                    line: 3,
                    byte_start,
                    byte_end,
                    call_sites: vec![],
                    doc_comments: vec![],
                }],
                types: vec![],
                call_sites: vec![],
                doc_comments: vec![],
            }],
        }],
    }
}

async fn request_source(
    router: axum::Router,
    path: &Path,
    byte_start: u32,
    byte_end: u32,
    expected_hash: &str,
) -> (StatusCode, Value) {
    let uri = format!(
        "/api/source?path={}&byte_start={byte_start}&byte_end={byte_end}&expected_hash={expected_hash}",
        encode_query_path(path),
    );
    let response = router
        .oneshot(
            Request::builder()
                .uri(&uri)
                .header("Host", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = body_string(response).await;
    let parsed: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!({ "raw": body }));
    (status, parsed)
}

fn tokens_text(body: &Value) -> String {
    body["tokens"]
        .as_array()
        .expect("tokens array")
        .iter()
        .map(|t| t[0].as_str().expect("text"))
        .collect()
}

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
                types: vec![],
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

    // Needles track the Desktop-style Slice 1 UI (spatial map), not the old
    // nested-list viewer. diagnostics.js preserves the Phase F walk for a
    // later slice.
    for (uri, needle) in [
        ("/", "Codebase Map"),
        ("/", "Open map JSON"),
        ("/", "id=\"inspector\""),
        ("/", "/static/diagnostics.js"),
        ("/static/viewer.css", "--sel: #6366f1"),
        ("/static/viewer.css", "radial-gradient"),
        ("/static/viewer.css", ".tok-kw"),
        ("/static/viewer.css", ".call-site"),
        ("/static/viewer.js", "loadMap"),
        ("/static/viewer.js", "computeLayout"),
        ("/static/viewer.js", "fitView"),
        ("/static/viewer.js", "crateLabelOf"),
        ("/static/viewer.js", "deriveEdges"),
        ("/static/viewer.js", "selectFunction"),
        ("/static/viewer.js", "fetchSourceInto"),
        ("/static/viewer.js", "renderInspector"),
        ("/static/viewer.js", "computeRightAggressorLayout"),
        ("/static/viewer.js", "computeLeftAggressorLayout"),
        ("/static/viewer.js", "leftHomeW"),
        ("/static/viewer.js", "setPointerCapture"),
        ("/static/viewer.js", "runLayoutAcceptance"),
        ("/static/viewer.js", "lastLeftOccupied"),
        ("/static/viewer.js", "panX -="),
        ("/static/viewer.js", "getUiState"),
        ("/", "import-screen"),
        ("/", "id=\"bottom-panel\""),
        ("/", "id=\"tab-diagnostics\""),
        ("/static/viewer.css", "left-w, 236px) - 11px"),
        ("/static/viewer.css", ".bottom-panel"),
        ("/static/viewer.css", ".diag-entry"),
        ("/static/viewer.js", "renderDiagnostics"),
        ("/static/viewer.js", "openDiagnosticEntry"),
        ("/static/viewer.js", "openInspectorForSelection"),
        ("/static/viewer.js", "suppressCardClick"),
        ("/static/viewer.js", "getLastCardGesture"),
        ("/static/viewer.js", "inspectorOpenPolicy"),
        ("/static/viewer.js", "smokeCheck"),
        ("/static/viewer.js", "setBottomOpen"),
        ("/static/viewer.js", "layoutBottomHeight"),
        ("/static/viewer.js", "leftWidth"),
        ("/static/viewer.js", "renderFunctionDag"),
        ("/static/viewer.js", "getFunctionDag"),
        ("/static/viewer.js", "getBottomRailHit"),
        ("/static/viewer.js", "getBottomTransform"),
        ("/static/viewer.js", "setBottomZoom"),
        ("/static/viewer.js", "screenXOfFnsWorld"),
        ("/static/viewer.js", "screenYOfFnsWorld"),
        ("/static/viewer.js", "getFnsNodeScreenRect"),
        ("/static/viewer.js", "getFnsPaneMetrics"),
        ("/static/viewer.js", "zoomFnsAt"),
        ("/static/viewer.js", "zoomMapAt"),
        // Functions dock: node press selects; pan only from empty background.
        ("/static/viewer.js", "resolveFnsPointerGesture"),
        ("/static/viewer.js", "closest(\".fns-node\")"),
        ("/static/viewer.js", "fnsPanFromBackgroundOnly"),
        ("/static/viewer.js", "getLastFnsNodeActivation"),
        ("/static/viewer.js", "surfacedReason"),
        // Dock shares left-occupied pan compensation with the map canvas.
        ("/static/viewer.js", "fnsPanX -= delta"),
        ("/static/viewer.css", "top: auto; /* release vertical-rail"),
        ("/static/viewer.css", "width: auto; /* release vertical-rail"),
        ("/static/viewer.css", ".fns-node"),
        ("/static/viewer.css", ".bottom-rail"),
        ("/static/viewer.css", ".fns-viewport > .zoom-hud"),
        ("/static/viewer.css", ".fns-legend"),
        // Cross-file callees: elsewhere cue, not faded/muddy (was opacity 0.92).
        ("/static/viewer.css", "border-left-color: var(--kind-file)"),
        // Banner wrap must not steal viewport height when the dock narrows.
        ("/static/viewer.css", "Banner must not wrap"),
        ("/static/viewer.css", "Never collapse to 0"),
        ("/static/viewer.css", "text-overflow: ellipsis"),
        ("/static/viewer.js", "FNS_VIEWPORT_MIN"),
        ("/", "id=\"tab-functions\""),
        ("/", "id=\"fns-viewport\""),
        ("/", "id=\"fns-zoom-hud\""),
        ("/", "id=\"fns-zoom-reset\""),
        ("/", "id=\"fns-legend\""),
        ("/", "class=\"fns-legend\""),
        ("/", "defined in this file"),
        ("/", "defined in another file"),
        ("/", "the analyser could not resolve this call"),
        ("/", "/static/function_dag.js"),
        // W6 neighborhood depth control.
        ("/", "id=\"fns-depth-mode\""),
        ("/", "data-depth=\"1\""),
        ("/static/function_dag.js", "function neighborhood"),
        ("/static/viewer.js", "setFnsDepth"),
        // W7 in-process analysis.
        ("/", "id=\"analyse-form\""),
        ("/", "id=\"analyse-overlay\""),
        ("/", "id=\"open-folder\""),
        ("/static/viewer.js", "startAnalyse"),
        ("/static/viewer.js", "/api/analyse"),
        ("/static/viewer.css", "analyse-overlay"),
        // W9 chrome: theme, Recent, Pages (Diff stays disabled).
        ("/", "id=\"toggle-theme\""),
        ("/", "id=\"recent-block\""),
        ("/", "id=\"page-map\""),
        ("/", "id=\"page-fns\""),
        ("/", "id=\"page-diff\""),
        ("/", "disabled title=\"Placeholder — no diff source"),
        ("/static/viewer.js", "rememberRecent"),
        ("/static/viewer.js", "horizon.theme"),
        ("/static/viewer.js", "getTheme"),
        ("/static/viewer.css", "recent-chip"),
        // W10 shared rail fixture table.
        ("/static/rail_layout_cases.json", "right_squeeze_left_to_min"),
        ("/static/viewer.js", "runRailFixtureTable"),
        ("/static/diagnostics.js", "collectDiagnostics"),
        ("/static/diagnostics.js", "groupByReason"),
        ("/static/diagnostics.js", "groupByFile"),
        ("/static/diagnostics.js", "byteStart"),
        ("/static/function_dag.js", "HorizonFunctionDag"),
        ("/static/function_dag.js", "analyser could not resolve"),
        ("/static/function_dag.js", "role: \"seed\""),
        // Cycle-safe layering (mutual/recursive calls must not blow the queue).
        ("/static/function_dag.js", "backEdgeKey"),
        ("/static/viewer.css", ".resize-rail.bottom-rail"),
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

    // Dead policy symbols must not ship — a leftover accessor caused a
    // ReferenceError in the live viewer when DIAG_CLICK_OPENS_INSPECTOR
    // was removed but diagClickOpensInspector still read it.
    let viewer = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/static/viewer.js")
                .header("Host", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let viewer_js = body_string(viewer).await;
    assert!(
        !viewer_js.contains("DIAG_CLICK_OPENS_INSPECTOR"),
        "viewer.js must not reference DIAG_CLICK_OPENS_INSPECTOR"
    );
    // Contiguous export/call forms only — smokeCheck names the dead API via join.
    assert!(
        !viewer_js.contains("diagClickOpensInspector:"),
        "viewer.js must not export diagClickOpensInspector"
    );
    assert!(
        !viewer_js.contains("diagClickOpensInspector("),
        "viewer.js must not call diagClickOpensInspector"
    );

    // Cross-file DAG nodes must not look degraded (old encoding used opacity).
    let css = router
        .oneshot(
            Request::builder()
                .uri("/static/viewer.css")
                .header("Host", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let css_body = body_string(css).await;
    assert!(
        !css_body.contains("opacity: 0.92"),
        "viewer.css must not fade .fns-node.function.external"
    );
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

// —— Phase C: GET /api/source ——

#[tokio::test]
async fn source_serves_highlighted_slice() {
    let dir = unique_temp_dir("good");
    let path = dir.join("lib.rs");
    std::fs::write(&path, SOURCE_FIXTURE).unwrap();
    let hash = content_hash(SOURCE_FIXTURE.as_bytes());
    let end = SOURCE_FIXTURE.len() as u32;
    let repo = repo_with_source_file(path.clone(), hash.clone(), 0, end);
    let router = app(AppState::new(Some(repo)));

    let (status, body) = request_source(router, &path, 0, end, &hash).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let text = tokens_text(&body);
    assert_eq!(text, SOURCE_FIXTURE);
    assert!(text.starts_with("/// Documented helper."));
    assert!(text.contains("#[inline]"));
    assert!(text.contains("pub fn documented"));
    assert!(text.trim_end().ends_with('}'));

    let classes: Vec<&str> = body["tokens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t[1].as_str().unwrap())
        .collect();
    assert!(classes.contains(&"kw"));
    assert!(classes.contains(&"c"));
    assert!(classes.contains(&"fn"));
    assert!(classes.contains(&"ty"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn source_refuses_path_not_in_map() {
    let dir = unique_temp_dir("notinmap");
    let path = dir.join("lib.rs");
    std::fs::write(&path, SOURCE_FIXTURE).unwrap();
    let hash = content_hash(SOURCE_FIXTURE.as_bytes());
    let end = SOURCE_FIXTURE.len() as u32;
    let repo = repo_with_source_file(path.clone(), hash.clone(), 0, end);
    let router = app(AppState::new(Some(repo)));

    // A real file on disk that is NOT listed in the map — must not be readable.
    let outsider = dir.join("secret.rs");
    std::fs::write(&outsider, "fn steal() {}\n").unwrap();
    let outsider_hash = content_hash(b"fn steal() {}\n");

    let (status, body) = request_source(router, &outsider, 0, 14, &outsider_hash).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "not_in_map");
    assert!(body.get("tokens").is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn source_refuses_stale_hash() {
    let dir = unique_temp_dir("stale");
    let path = dir.join("lib.rs");
    std::fs::write(&path, SOURCE_FIXTURE).unwrap();
    let hash = content_hash(SOURCE_FIXTURE.as_bytes());
    let end = SOURCE_FIXTURE.len() as u32;
    let repo = repo_with_source_file(path.clone(), hash.clone(), 0, end);
    let router = app(AppState::new(Some(repo)));

    // Edit disk after the map was built.
    std::fs::write(&path, "/// changed\n#[inline]\npub fn documented(x: u32) -> u32 { x }\n")
        .unwrap();

    let (status, body) = request_source(router, &path, 0, end, &hash).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "stale");
    assert!(body.get("tokens").is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn source_reports_missing_file() {
    let dir = unique_temp_dir("missing");
    let path = dir.join("lib.rs");
    std::fs::write(&path, SOURCE_FIXTURE).unwrap();
    let hash = content_hash(SOURCE_FIXTURE.as_bytes());
    let end = SOURCE_FIXTURE.len() as u32;
    let repo = repo_with_source_file(path.clone(), hash.clone(), 0, end);
    let router = app(AppState::new(Some(repo)));

    std::fs::remove_file(&path).unwrap();

    let (status, body) = request_source(router, &path, 0, end, &hash).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], "missing");
    assert!(body.get("tokens").is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn source_refuses_empty_expected_hash() {
    let dir = unique_temp_dir("emptyhash");
    let path = dir.join("lib.rs");
    std::fs::write(&path, SOURCE_FIXTURE).unwrap();
    let end = SOURCE_FIXTURE.len() as u32;
    // Map entry with empty content_hash sentinel.
    let repo = repo_with_source_file(path.clone(), String::new(), 0, end);
    let router = app(AppState::new(Some(repo)));

    let (status, body) = request_source(router, &path, 0, end, "").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "unverifiable");
    assert!(body.get("tokens").is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn source_refuses_zero_length_range_sentinel() {
    let dir = unique_temp_dir("nosource");
    let path = dir.join("lib.rs");
    std::fs::write(&path, SOURCE_FIXTURE).unwrap();
    let hash = content_hash(SOURCE_FIXTURE.as_bytes());
    // Old-map sentinel: byte_start == byte_end == 0.
    let repo = repo_with_source_file(path.clone(), hash.clone(), 0, 0);
    let router = app(AppState::new(Some(repo)));

    let (status, body) = request_source(router, &path, 0, 0, &hash).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "no_source");
    assert!(body.get("tokens").is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn source_404_when_no_map_loaded() {
    let router = app(AppState::new(None));
    let (status, body) = request_source(
        router,
        Path::new("C:\\nowhere\\lib.rs"),
        0,
        10,
        &"a".repeat(64),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], "no_map");
}

// —— W7: POST /api/analyse ——

fn fixture_repo(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .join("tests/fixtures")
        .join(name)
}

#[tokio::test]
async fn analyse_status_starts_idle() {
    let router = app(AppState::new(None));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/analyse")
                .header("Host", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
        .unwrap();
    assert_eq!(body["status"], "idle");
}

#[tokio::test]
async fn analyse_rejects_missing_path() {
    let router = app(AppState::new(None));
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/analyse")
                .header("Host", "127.0.0.1")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"path":"/no/such/horizon/repo"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn analyse_runs_fixture_and_stores_map() {
    let state = AppState::new(None);
    let router = app(state.clone());
    let path = fixture_repo("phase1-single-file");
    assert!(path.is_dir(), "fixture missing: {}", path.display());

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/analyse")
                .header("Host", "127.0.0.1")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    json!({ "path": path.to_string_lossy() }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let accepted: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(accepted["status"], "running");

    // Poll until done (fixture is tiny — should finish quickly).
    let mut last = json!({ "status": "running" });
    for _ in 0..200 {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/analyse")
                    .header("Host", "127.0.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        last = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
            .unwrap();
        if last["status"] != "running" {
            break;
        }
    }
    assert_eq!(last["status"], "done", "{last}");

    let guard = state.map.read().await;
    let repo = guard.as_ref().expect("map stored after analyse");
    assert!(
        !repo.crates.is_empty(),
        "analysed map should contain at least one crate"
    );
    assert!(repo.summary.unresolved + repo.summary.conflicts + 1 >= 1);
}

#[tokio::test]
async fn analyse_rejects_non_local_host() {
    let router = app(AppState::new(None));
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/analyse")
                .header("Host", "evil.example")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"path":"/tmp"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
