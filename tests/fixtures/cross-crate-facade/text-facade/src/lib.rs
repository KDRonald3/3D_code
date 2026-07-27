//! Facade crate: flat public API over several path dependencies.
//!
//! Internal modules stay private; outsiders name only the re-exported surface.

mod private_mod;

/// `extern crate` rename facade (ripgrep-style): binds the dependency as a
/// public module path prefix (`text_facade::fmt_eng::…`).
pub extern crate format_engine as fmt_eng;

/// Plain facade: definition lives in `format_engine`.
pub use format_engine::upper;

/// Renamed facade: local name `split`, definition `parse_engine::tokenize`.
pub use parse_engine::tokenize as split;

/// Chain: this crate → mid-crate → leaf-crate.
pub use mid_crate::chained;

/// Glob facade: brings `area` (and `version`) from format-engine's root.
pub use format_engine::*;

/// Two globs both offering `get` → genuine ambiguity (Conflict).
pub use shapes_eng::*;
pub use text_eng::*;

/// `pub(crate)` re-export — visible inside this crate only, not to consumers.
pub(crate) use format_engine::version as crate_only_version;

/// Facade over a private module's public item (Rust-correct; name is public).
pub use private_mod::from_private;
