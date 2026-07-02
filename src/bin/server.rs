//! Local web server for the Codebase Visualizer.
//!
//! Serves the (unchanged) Design-Component UI at `/` and its browser runtime at
//! `/support.js`, and exposes the JSON scan API: `POST /api/scan` runs the Rust
//! analyzer over the files the browser uploads when the user adds a folder or
//! source file, and `GET /api/scan-path` scans a server-side directory or file
//! by path (local/testing convenience). The page starts empty and is populated
//! entirely from these endpoints. A liveness probe is available at
//! `GET /api/health`.
//!
//! Security posture: the server binds to 127.0.0.1 by default (set `HOST` to
//! override, e.g. `HOST=0.0.0.0` to expose on a LAN), serves no cross-origin
//! headers (the UI is same-origin), rejects requests whose `Host` header is
//! not local (blocking DNS-rebinding attacks against the filesystem-reading
//! scan API), and caps upload bodies to keep a hostile client from exhausting
//! memory.

use std::io::Read;
use std::net::SocketAddr;
use std::path::PathBuf;

use serde::Deserialize;
use tiny_http::{Header, Method, Response, Server};

use codebase_visualizer::{analyze, scan_path, InputFile};

/// Maximum accepted `POST /api/scan` body size (64 MiB). Uploads are source
/// text, so anything larger is either abuse or a mistake.
const MAX_BODY_BYTES: u64 = 64 * 1024 * 1024;

/// The Design-Component UI page, embedded into the binary at compile time and
/// served at `/`.
const INDEX_HTML: &str = include_str!("../../web/index.dc.html");
/// The browser-side runtime script, embedded at compile time and served at
/// `/support.js`.
const SUPPORT_JS: &str = include_str!("../../web/support.js");

/// JSON body of a `POST /api/scan` request: the project name and the list of
/// files the browser uploaded when the user added a folder.
#[derive(Deserialize)]
struct ScanRequest {
    /// Optional project name; defaults to `"project"` when missing or blank.
    #[serde(default)]
    name: Option<String>,
    /// The uploaded files to analyze.
    #[serde(default)]
    files: Vec<FileJson>,
}

/// A single uploaded file: its path and full text contents.
#[derive(Deserialize)]
struct FileJson {
    /// Path of the file relative to the uploaded folder.
    path: String,
    /// Full text contents of the file.
    text: String,
}

/// Bind the HTTP server and route requests: serve the UI (`/`), the runtime (`/support.js`), and the scan API (`POST /api/scan`, `GET /api/scan-path`).
fn main() {
    // Resolve the listen port from $PORT, falling back to 8787 if unset/invalid.
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8787);
    // Bind to loopback only by default so the scan API (which reads local
    // files) is not reachable from the network; $HOST overrides deliberately.
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let ip: std::net::IpAddr = host.parse().unwrap_or_else(|_| {
        eprintln!("invalid HOST {host:?}, expected an IP address");
        std::process::exit(1);
    });
    let addr = SocketAddr::from((ip, port));
    let server = Server::http(addr).unwrap_or_else(|e| {
        eprintln!("failed to bind {addr}: {e}");
        std::process::exit(1);
    });
    println!("Codebase Visualizer running at http://localhost:{port}");

    // Handle requests serially as they arrive.
    for mut request in server.incoming_requests() {
        let method = request.method().clone();
        let url = request.url().to_string();
        // Route on the path only, ignoring any `?query` portion.
        let path = url.split('?').next().unwrap_or("/");

        // Reject API requests carrying a non-local Host header: a DNS-rebinding
        // page resolves its own hostname to 127.0.0.1 to bypass same-origin
        // checks, but it cannot forge the Host header it connected with. The
        // check only applies to the default loopback bind — overriding $HOST
        // is an explicit opt-in to network exposure.
        let host_ok = !ip.is_loopback() || host_is_local(&request);

        // Dispatch on (method, path); unmatched routes fall through to 404.
        let response = match (&method, path) {
            // Static UI page and its runtime script.
            (Method::Get, "/") | (Method::Get, "/index.html") => html(INDEX_HTML),
            (Method::Get, "/support.js") => js(SUPPORT_JS),
            _ if path.starts_with("/api/") && !host_ok => {
                forbidden("request Host is not local")
            }
            // Liveness probe.
            (Method::Get, "/api/health") => json("{\"ok\":true}".to_string()),
            // Scan a server-side directory or single file passed via `?path=`.
            (Method::Get, "/api/scan-path") => handle_scan_path(&url),
            // Scan browser-uploaded files carried in the request body.
            (Method::Post, "/api/scan") => {
                // Refuse oversized uploads up front (or after the cap is hit
                // when no Content-Length is present) to bound memory use.
                let declared = request.body_length().map(|l| l as u64);
                if declared.map(|l| l > MAX_BODY_BYTES).unwrap_or(false) {
                    payload_too_large()
                } else {
                    let mut body = String::new();
                    let read = request
                        .as_reader()
                        .take(MAX_BODY_BYTES + 1)
                        .read_to_string(&mut body);
                    if read.is_err() {
                        bad_request("could not read request body")
                    } else if body.len() as u64 > MAX_BODY_BYTES {
                        payload_too_large()
                    } else {
                        handle_scan(&body)
                    }
                }
            }
            // Anything else is unknown.
            _ => Response::from_string("Not found").with_status_code(404),
        };

        let _ = request.respond(response);
    }
}

/// True when the request's `Host` header (ignoring any `:port`) is a loopback
/// name/address. Requests without a `Host` header are rejected.
fn host_is_local(request: &tiny_http::Request) -> bool {
    let Some(h) = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Host"))
    else {
        return false;
    };
    let value = h.value.as_str();
    // Strip the port: `[::1]:8787` keeps the bracketed address, `host:port`
    // keeps the host.
    let hostname = if let Some(rest) = value.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        value.rsplit_once(':').map(|(h, _)| h).unwrap_or(value)
    };
    matches!(hostname, "localhost" | "127.0.0.1" | "::1")
}

/// Parse a `POST /api/scan` JSON body (the folder files the browser uploaded), run the analyzer, and return the model JSON.
fn handle_scan(body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    // Parse the JSON body; reject malformed input with a 400.
    let req: ScanRequest = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => return bad_request(&format!("invalid JSON: {e}")),
    };
    // Use the supplied name, defaulting to "project" when missing or blank.
    let name = req
        .name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "project".to_string());
    // Convert the wire structs into the analyzer's `InputFile` type.
    let files: Vec<InputFile> = req
        .files
        .into_iter()
        .map(|f| InputFile {
            path: f.path,
            text: f.text,
        })
        .collect();
    // Run the analyzer and return its model as JSON.
    let model = analyze(&name, files);
    match serde_json::to_string(&model) {
        Ok(s) => json(s),
        Err(e) => bad_request(&format!("serialize error: {e}")),
    }
}

/// Scan a server-side directory or single source file given `?path=` -- a convenience endpoint for local and testing use.
fn handle_scan_path(url: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    // Isolate the raw query string after the first `?` (empty if absent).
    let query = url.split('?').nth(1).unwrap_or("");
    // Scan the `&`-separated pairs for `path=`, percent-decoding its value.
    let mut path = None;
    for pair in query.split('&') {
        if let Some(v) = pair.strip_prefix("path=") {
            path = Some(url_decode(v));
        }
    }
    // The path is required; without it the request is malformed.
    let Some(path) = path else {
        return bad_request("missing ?path=");
    };
    // Scan the file or directory (capping per-file size), then analyze and serialize.
    match scan_path(&PathBuf::from(&path), 500_000) {
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

/// Alias for the concrete `tiny_http` response type returned by the helpers below.
type Resp = Response<std::io::Cursor<Vec<u8>>>;

/// Build an HTTP header from a name/value pair.
fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
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

/// A 200 response carrying a JSON body.
fn json(body: String) -> Resp {
    Response::from_string(body)
        .with_header(header("Content-Type", "application/json; charset=utf-8"))
}

/// An error response with the given status carrying a JSON `{error}` message.
fn error_response(status: u16, msg: &str) -> Resp {
    let body = format!("{{\"error\":{}}}", json_string(msg));
    Response::from_string(body)
        .with_status_code(status)
        .with_header(header("Content-Type", "application/json; charset=utf-8"))
}

/// A 400 response carrying a JSON `{error}` message.
fn bad_request(msg: &str) -> Resp {
    error_response(400, msg)
}

/// A 403 response carrying a JSON `{error}` message.
fn forbidden(msg: &str) -> Resp {
    error_response(403, msg)
}

/// A 413 response for uploads exceeding [`MAX_BODY_BYTES`].
fn payload_too_large() -> Resp {
    error_response(413, "request body too large")
}

/// Minimal JSON string escaper for error messages: wraps `s` in quotes and
/// escapes the characters JSON requires, returning a quoted string literal.
fn json_string(s: &str) -> String {
    // Open the quoted literal.
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            // Characters with dedicated JSON escape sequences.
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Other control characters: emit a `\uXXXX` escape.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            // Everything else is safe to pass through verbatim.
            c => out.push(c),
        }
    }
    // Close the quoted literal.
    out.push('"');
    out
}

/// Percent-decode a query-string value: turns `+` into spaces and `%XX`
/// sequences into their byte values, decoding the result as (lossy) UTF-8.
fn url_decode(s: &str) -> String {
    // First expand `+` to spaces, the form-encoding convention for queries.
    let bytes = s.replace('+', " ");
    let mut out = Vec::new();
    let b = bytes.as_bytes();
    let mut i = 0;
    while i < b.len() {
        // On `%XX`, decode the two hex digits into one byte and skip past them.
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&bytes[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        // Otherwise copy the byte unchanged (including malformed `%` escapes).
        out.push(b[i]);
        i += 1;
    }
    // Interpret the decoded bytes as UTF-8, replacing any invalid sequences.
    String::from_utf8_lossy(&out).to_string()
}
