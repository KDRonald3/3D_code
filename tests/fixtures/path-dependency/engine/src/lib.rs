//! A path dependency living inside the same repository.
//!
//! Folder is `engine/`, package is `text-engine`, code refers to `text_engine`.

/// Declared `pub`, so outsiders can traverse it.
pub mod format;

/// Declared WITHOUT `pub`. The file exists and compiles, but no outside crate
/// can name this module or anything in it.
mod secret;

/// Facade re-export: callers may write `text_engine::upper` and never mention
/// the `format` module at all.
pub use format::upper;

pub fn version() -> &'static str {
    "0.1.0"
}

/// Proves `secret` is usable from inside this crate, just not from outside.
pub fn internal_check() -> String {
    secret::hidden()
}
