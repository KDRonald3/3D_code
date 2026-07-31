//! Cross-crate call through a Cargo dependency rename.
//!
//! Manifest: `alias = { package = "text-engine", path = "engine" }`
//! Source uses `alias::…`; the real rustc crate name remains `text_engine`.

pub fn run(name: &str) -> String {
    alias::greet(name)
}
