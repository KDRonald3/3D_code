//! Local web server for the Codebase Visualizer.
//!
//! Serves the (unchanged) Design-Component UI and exposes a single JSON API,
//! `POST /api/scan`, which runs the Rust analyzer over the files the browser
//! uploads when the user adds a folder. The page starts empty and is populated
//! entirely from this endpoint.

use std::net::SocketAddr;
use std::path::PathBuf;

use serde::Deserialize;
use tiny_http::{Header, Method, Response, Server};

use codebase_visualizer::{analyze, scan_dir, InputFile};

const INDEX_HTML: &str = include_str!("../../web/index.dc.html");
const SUPPORT_JS: &str = include_str!("../../web/support.js");

#[derive(Deserialize)]
struct ScanRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    files: Vec<FileJson>,
}

#[derive(Deserialize)]
struct FileJson {
    path: String,
    text: String,
}

/// Bind the HTTP server and route requests: serve the UI (`/`), the runtime (`/support.js`), and the scan API (`POST /api/scan`, `GET /api/scan-path`).
fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8787);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let server = Server::http(addr).unwrap_or_else(|e| {
        eprintln!("failed to bind {addr}: {e}");
        std::process::exit(1);
    });
    println!("Codebase Visualizer running at http://localhost:{port}");

    for mut request in server.incoming_requests() {
        let method = request.method().clone();
        let url = request.url().to_string();
        let path = url.split('?').next().unwrap_or("/");

        let response = match (&method, path) {
            (Method::Get, "/") | (Method::Get, "/index.html") => html(INDEX_HTML),
            (Method::Get, "/support.js") => js(SUPPORT_JS),
            (Method::Get, "/api/health") => json("{\"ok\":true}".to_string()),
            (Method::Get, "/api/scan-path") => handle_scan_path(&url),
            (Method::Post, "/api/scan") => {
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    bad_request("could not read request body")
                } else {
                    handle_scan(&body)
                }
            }
            (Method::Options, _) => Response::from_string("")
                .with_status_code(204)
                .with_header(cors()),
            _ => Response::from_string("Not found").with_status_code(404),
        };

        let _ = request.respond(response);
    }
}

/// Parse a `POST /api/scan` JSON body (the folder files the browser uploaded), run the analyzer, and return the model JSON.
fn handle_scan(body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let req: ScanRequest = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => return bad_request(&format!("invalid JSON: {e}")),
    };
    let name = req
        .name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "project".to_string());
    let files: Vec<InputFile> = req
        .files
        .into_iter()
        .map(|f| InputFile {
            path: f.path,
            text: f.text,
        })
        .collect();
    let model = analyze(&name, files);
    match serde_json::to_string(&model) {
        Ok(s) => json(s),
        Err(e) => bad_request(&format!("serialize error: {e}")),
    }
}

/// Scan a server-side directory given `?path=` -- a convenience endpoint for local and testing use.
fn handle_scan_path(url: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let query = url.split('?').nth(1).unwrap_or("");
    let mut path = None;
    for pair in query.split('&') {
        if let Some(v) = pair.strip_prefix("path=") {
            path = Some(url_decode(v));
        }
    }
    let Some(path) = path else {
        return bad_request("missing ?path=");
    };
    match scan_dir(&PathBuf::from(&path), 500_000) {
        Ok((name, files)) => {
            let model = analyze(&name, files);
            match serde_json::to_string(&model) {
                Ok(s) => json(s),
                Err(e) => bad_request(&format!("serialize error: {e}")),
            }
        }
        Err(e) => bad_request(&format!("scan error: {e}")),
    }
}

// ── response helpers ────────────────────────────────────────────────────────

type Resp = Response<std::io::Cursor<Vec<u8>>>;

/// Build an HTTP header from a name/value pair.
fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
}

/// An `Access-Control-Allow-Origin: *` header.
fn cors() -> Header {
    header("Access-Control-Allow-Origin", "*")
}

/// A 200 response carrying an HTML body.
fn html(body: &str) -> Resp {
    Response::from_string(body)
        .with_header(header("Content-Type", "text/html; charset=utf-8"))
}

/// A 200 response carrying a JavaScript body.
fn js(body: &str) -> Resp {
    Response::from_string(body)
        .with_header(header("Content-Type", "text/javascript; charset=utf-8"))
}

/// A 200 response carrying a JSON body (with CORS).
fn json(body: String) -> Resp {
    Response::from_string(body)
        .with_header(header("Content-Type", "application/json; charset=utf-8"))
        .with_header(cors())
}

/// A 400 response carrying a JSON `{error}` message.
fn bad_request(msg: &str) -> Resp {
    let body = format!("{{\"error\":{}}}", json_string(msg));
    Response::from_string(body)
        .with_status_code(400)
        .with_header(header("Content-Type", "application/json; charset=utf-8"))
        .with_header(cors())
}

/// Minimal JSON string escaper for error messages.
fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Percent-decode a query-string value.
fn url_decode(s: &str) -> String {
    let bytes = s.replace('+', " ");
    let mut out = Vec::new();
    let b = bytes.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&bytes[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}
