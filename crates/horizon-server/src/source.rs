//! `GET /api/source` — serve a highlighted function slice from disk.
//!
//! # Wire format (permanent contract for any front-end)
//!
//! ## Query parameters
//!
//! | Param | Type | Meaning |
//! |---|---|---|
//! | `path` | string | Absolute file path; must equal a [`File::path`](horizon_map::File) in the loaded map |
//! | `byte_start` | u32 | UTF-8 byte offset of the slice start (inclusive) |
//! | `byte_end` | u32 | UTF-8 byte offset of the slice end (exclusive) |
//! | `expected_hash` | string | Hex SHA-256 from [`File::content_hash`](horizon_map::File); empty = unverifiable |
//!
//! ## Success (`200`)
//!
//! ```json
//! { "tokens": [ ["fn", "kw"], [" ", ""], ["name", "fn"], … ] }
//! ```
//!
//! Each token is `[text, class]`. Concatenating every `text` reconstructs the
//! exact slice. `class` is one of:
//!
//! | Class | CSS | Role |
//! |---|---|---|
//! | `kw` | `.tok-kw` | keywords (`fn`, `let`, `pub`, …) |
//! | `fn` | `.tok-fn` | function / call / macro names |
//! | `ty` | `.tok-ty` | type names (including primitives) |
//! | `c` | `.tok-c` | comments and doc comments |
//! | `str` | `.tok-str` | string / char / byte / C-string literals |
//! | `num` | `.tok-num` | integer and float literals |
//! | `""` | `.tok` / unstyled | punctuation, whitespace, lifetimes, other idents |
//!
//! ## Error body
//!
//! Every failure is JSON `{ "error": "<kind>", … }` with a distinct `error`
//! string the UI can switch on:
//!
//! | `error` | Status | When |
//! |---|---|---|
//! | `no_map` | 404 | No map loaded in server state |
//! | `not_in_map` | 403 | `path` is not a `File.path` in the loaded map (never reads disk) |
//! | `unverifiable` | 400 | `expected_hash` is empty (map predates hashing) |
//! | `no_source` | 400 | `byte_start == byte_end == 0` (sentinel: range unavailable) |
//! | `missing` | 404 | File is in the map but absent on disk |
//! | `stale` | 409 | On-disk hash ≠ `expected_hash` (slice not served) |
//! | `range` | 400 | Offsets out of bounds or empty non-sentinel range |
//! | `bad_request` | 400 | Missing / unparseable query parameters |
//!
//! # Security
//!
//! The handler never calls `std::fs::read` on the raw query string. It looks
//! the path up in the loaded map's file set; only an exact match yields the
//! trusted [`PathBuf`](std::path::PathBuf) used for the read. Paths outside
//! that set are refused with `not_in_map` — no canonicalize-and-hope.

use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use horizon_map::{content_hash, File, Folder, Repository};
use ra_ap_syntax::{AstNode, Edition, SourceFile, SyntaxKind, WalkEvent};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Query string for `GET /api/source`.
#[derive(Debug, Deserialize)]
pub struct SourceQuery {
    pub path: String,
    pub byte_start: u32,
    pub byte_end: u32,
    /// Hex SHA-256 from the map. Empty string = map predates hashing.
    #[serde(default)]
    pub expected_hash: String,
}

/// Serve a highlighted source slice, or a structured error (see module docs).
pub async fn get_source(State(state): State<AppState>, Query(q): Query<SourceQuery>) -> Response {
    let guard = state.map.read().await;
    let Some(repo) = guard.as_ref() else {
        return err(StatusCode::NOT_FOUND, "no_map", "no map loaded");
    };

    match load_highlighted_source(repo, &q) {
        Ok(tokens) => Json(json!({ "tokens": tokens })).into_response(),
        Err(e) => e.into_response(),
    }
}

/// Token as a two-element JSON array `[text, class]`.
type TokenPair = [Value; 2];

#[derive(Debug)]
struct SourceError {
    status: StatusCode,
    kind: &'static str,
    message: String,
}

impl SourceError {
    fn new(status: StatusCode, kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            kind,
            message: message.into(),
        }
    }
}

impl IntoResponse for SourceError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({ "error": self.kind, "message": self.message })),
        )
            .into_response()
    }
}

fn err(status: StatusCode, kind: &'static str, message: impl Into<String>) -> Response {
    SourceError::new(status, kind, message).into_response()
}

/// Core logic — used by the handler and unit-tested directly.
fn load_highlighted_source(
    repo: &Repository,
    q: &SourceQuery,
) -> Result<Vec<TokenPair>, SourceError> {
    // Zero-length sentinel: map predates byte ranges — no slice is possible.
    if q.byte_start == 0 && q.byte_end == 0 {
        return Err(SourceError::new(
            StatusCode::BAD_REQUEST,
            "no_source",
            "function has no source range (map predates byte_start/byte_end)",
        ));
    }

    if q.byte_end < q.byte_start {
        return Err(SourceError::new(
            StatusCode::BAD_REQUEST,
            "range",
            "byte_end is before byte_start",
        ));
    }

    if q.byte_start == q.byte_end {
        return Err(SourceError::new(
            StatusCode::BAD_REQUEST,
            "range",
            "empty byte range",
        ));
    }

    // Empty hash sentinel: cannot verify staleness — refuse rather than guess.
    if q.expected_hash.is_empty() {
        return Err(SourceError::new(
            StatusCode::BAD_REQUEST,
            "unverifiable",
            "expected_hash is empty; map predates content hashing",
        ));
    }

    let requested = PathBuf::from(&q.path);
    let Some(file) = find_file_in_map(repo, &requested) else {
        return Err(SourceError::new(
            StatusCode::FORBIDDEN,
            "not_in_map",
            "path is not a File.path in the loaded map",
        ));
    };

    // Read via the path stored in the map — never the raw query string alone.
    let bytes = match std::fs::read(&file.path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(SourceError::new(
                StatusCode::NOT_FOUND,
                "missing",
                format!("file not found on disk: {}", file.path.display()),
            ));
        }
        Err(e) => {
            return Err(SourceError::new(
                StatusCode::NOT_FOUND,
                "missing",
                format!("failed to read {}: {e}", file.path.display()),
            ));
        }
    };

    let actual = content_hash(&bytes);
    if actual != q.expected_hash {
        return Err(SourceError::new(
            StatusCode::CONFLICT,
            "stale",
            "file content hash differs from expected_hash; re-analyse",
        ));
    }

    let start = q.byte_start as usize;
    let end = q.byte_end as usize;
    if end > bytes.len() || start > bytes.len() {
        return Err(SourceError::new(
            StatusCode::BAD_REQUEST,
            "range",
            format!(
                "byte range [{start}, {end}) exceeds file length {}",
                bytes.len()
            ),
        ));
    }

    // Offsets are UTF-8 byte offsets from ra_ap_syntax; reject mid-codepoint cuts.
    let slice = match std::str::from_utf8(&bytes[start..end]) {
        Ok(s) => s,
        Err(_) => {
            return Err(SourceError::new(
                StatusCode::BAD_REQUEST,
                "range",
                "byte range is not valid UTF-8 (offset mid-codepoint?)",
            ));
        }
    };

    Ok(highlight_tokens(slice))
}

/// Exact membership check against every [`File::path`] in the map.
///
/// No canonicalize, no prefix-of-root fallback: the query path must equal a
/// stored path. Comparison uses [`Path`] equality (platform-aware).
pub fn find_file_in_map<'a>(repo: &'a Repository, path: &Path) -> Option<&'a File> {
    for krate in &repo.crates {
        for file in &krate.files {
            if file.path == path {
                return Some(file);
            }
        }
        for folder in &krate.folders {
            if let Some(file) = find_file_in_folder(folder, path) {
                return Some(file);
            }
        }
    }
    None
}

fn find_file_in_folder<'a>(folder: &'a Folder, path: &Path) -> Option<&'a File> {
    for file in &folder.files {
        if file.path == path {
            return Some(file);
        }
    }
    for child in &folder.folders {
        if let Some(file) = find_file_in_folder(child, path) {
            return Some(file);
        }
    }
    None
}

/// Lex `source` with `ra_ap_syntax` and classify each token into the wire
/// vocabulary (`kw` / `fn` / `ty` / `c` / `str` / `num` / `""`).
pub fn highlight_tokens(source: &str) -> Vec<TokenPair> {
    let edition = Edition::CURRENT;
    let tree = SourceFile::parse(source, edition).tree();

    // Collect tokens first so we can peek ahead for call/macro heuristics.
    let mut raw: Vec<(SyntaxKind, String)> = Vec::new();
    for event in tree.syntax().preorder_with_tokens() {
        let WalkEvent::Enter(element) = event else {
            continue;
        };
        let Some(token) = element.into_token() else {
            continue;
        };
        raw.push((token.kind(), token.text().to_string()));
    }

    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let (kind, text) = &raw[i];
        let class = classify_token(*kind, text, edition, &raw, i);
        out.push([Value::String(text.clone()), Value::String(class.to_string())]);
        i += 1;
    }
    out
}

fn classify_token(
    kind: SyntaxKind,
    text: &str,
    edition: Edition,
    tokens: &[(SyntaxKind, String)],
    index: usize,
) -> &'static str {
    if kind == SyntaxKind::COMMENT {
        return "c";
    }
    if kind == SyntaxKind::STRING
        || kind == SyntaxKind::BYTE_STRING
        || kind == SyntaxKind::C_STRING
        || kind == SyntaxKind::CHAR
        || kind == SyntaxKind::BYTE
    {
        return "str";
    }
    if kind == SyntaxKind::INT_NUMBER || kind == SyntaxKind::FLOAT_NUMBER {
        return "num";
    }
    if kind.is_keyword(edition) {
        return "kw";
    }
    // Lifetimes (`'a`) — no dedicated wire class; leave unstyled.
    if kind == SyntaxKind::LIFETIME_IDENT {
        return "";
    }
    if kind == SyntaxKind::IDENT {
        if is_primitive_ty(text) {
            return "ty";
        }
        // Name immediately after `fn` / `struct` / `enum` / `trait` / `type`.
        if let Some(prev) = prev_non_trivia(tokens, index) {
            match prev {
                SyntaxKind::FN_KW => return "fn",
                SyntaxKind::STRUCT_KW
                | SyntaxKind::ENUM_KW
                | SyntaxKind::TRAIT_KW
                | SyntaxKind::TYPE_KW
                | SyntaxKind::UNION_KW => return "ty",
                _ => {}
            }
        }
        // Call or macro: ident followed (past trivia) by `(` or `!`.
        if let Some(next) = next_non_trivia(tokens, index) {
            if next == SyntaxKind::L_PAREN || next == SyntaxKind::BANG {
                return "fn";
            }
        }
        // Capitalised idents look like types in Rust source.
        if text.starts_with(|c: char| c.is_ascii_uppercase()) {
            return "ty";
        }
        return "";
    }
    // Punctuation, whitespace, errors, etc.
    ""
}

fn prev_non_trivia(tokens: &[(SyntaxKind, String)], index: usize) -> Option<SyntaxKind> {
    let mut i = index;
    while i > 0 {
        i -= 1;
        if !tokens[i].0.is_trivia() {
            return Some(tokens[i].0);
        }
    }
    None
}

fn next_non_trivia(tokens: &[(SyntaxKind, String)], index: usize) -> Option<SyntaxKind> {
    let mut i = index + 1;
    while i < tokens.len() {
        if !tokens[i].0.is_trivia() {
            return Some(tokens[i].0);
        }
        i += 1;
    }
    None
}

fn is_primitive_ty(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "char"
            | "str"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "f32"
            | "f64"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_classifies_keywords_comments_and_strings() {
        let src = "/// docs\n#[inline]\nfn demo(x: u32) -> &'static str { \"hi\" }\n";
        let tokens = highlight_tokens(src);
        let joined: String = tokens
            .iter()
            .map(|t| t[0].as_str().unwrap())
            .collect();
        assert_eq!(joined, src, "tokens must round-trip the slice text");

        let classes: Vec<&str> = tokens
            .iter()
            .map(|t| t[1].as_str().unwrap())
            .collect();
        assert!(classes.contains(&"kw"), "expected keywords: {classes:?}");
        assert!(classes.contains(&"c"), "expected comment: {classes:?}");
        assert!(classes.contains(&"str"), "expected string: {classes:?}");
        assert!(classes.contains(&"ty"), "expected type: {classes:?}");
        assert!(classes.contains(&"fn"), "expected fn name: {classes:?}");
    }
}
