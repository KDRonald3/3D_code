//! Consuming a path dependency. Folder `engine/`, package `text-engine`,
//! crate name in code `text_engine`.

// WORKS: crate name, then a `pub mod`, then a `pub fn`.
use text_engine::format::upper;

// WORKS: nested public inline module.
use text_engine::format::deep::buried;

// WORKS: the facade re-export declared in the dependency's lib.rs.
use text_engine::upper as engine_upper;

// WORKS: a function defined directly in the dependency's lib.rs, so there is
// no module segment at all.
use text_engine::version;

// E0603 "module `secret` is private": declared as `mod secret;` not `pub mod`.
// The `pub fn hidden` inside it is irrelevant.
//     use text_engine::secret::hidden;

// E0603 "function `trim_inner` is private": it is `pub(crate)` in text-engine,
// so it is public within that crate only.
//     use text_engine::format::trim_inner;

// E0432 "could not find `text_engine` in the crate root": `crate::` means OUR
// crate. A dependency is never under `crate::`.
//     use crate::text_engine::version as bad_version;

// E0433 "cannot find module or crate `engine`": `engine` is the FOLDER name.
// The crate name comes from `[package] name`.
//     use engine::format::upper as folder_named;

// FAILS to even parse: dashes are not valid in a path. The crate is
// `text_engine` in code.
//     use text-engine::format::upper as dashed;

pub fn demo(s: &str) -> String {
    // Calls sit outside `format!` so they appear as CallExpr nodes (macro
    // token-tree recovery is a separate pipeline concern).
    let a = upper(s);
    let b = buried(s);
    let c = engine_upper(s);
    let d = version();
    format!("{a} {b} {c} {d}")
}
