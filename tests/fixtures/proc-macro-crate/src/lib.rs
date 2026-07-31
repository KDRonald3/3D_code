//! Proc-macro crate whose free functions must appear in the map.
//!
//! `cargo metadata` reports this target as kind `proc-macro` (not `lib`).
//! Discovery must include it — ordinary Rust source with ordinary free
//! functions, even though the crate is a procedural macro to Cargo.

use proc_macro::TokenStream;

/// Helper free function inside a proc-macro crate.
pub fn expand_name(input: &str) -> String {
    format!("expanded_{input}")
}

/// Another free function so the map has more than a single leaf.
pub fn sanitize(input: &str) -> String {
    expand_name(&input.replace('-', "_"))
}

#[proc_macro]
pub fn identity(input: TokenStream) -> TokenStream {
    let _ = sanitize("probe");
    input
}
