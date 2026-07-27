//! Crate root. Declares the module tree, so every `crate::..` path written
//! elsewhere in this demo actually resolves to a declared module.
//!
//! There is no `impl` anywhere in this crate. Every call is a free function
//! call, a module-qualified call, or a call through a re-export.

pub mod app;
pub mod numbers;
pub mod selfdecl;
pub mod uses_engine;
pub mod shapes;
pub mod text;

/// Facade re-export: callers can write `crate::mean(..)` and never name the
/// module the function actually lives in.
/// The module name and the filename need not match at all.
#[path = "renamed_on_disk.rs"]
pub mod tidy_name;

pub use numbers::mean;

/// Re-export under a DIFFERENT name. The local name and the target name differ.
pub use text::upper as shout_upper;

/// With `mod selfdecl;` present, the file's own `pub mod selfdecl` NESTS rather
/// than merging: the inner function needs the segment twice.
pub fn try_selfdecl() -> String {
    let outer = selfdecl::outer_fn();
    let inner = selfdecl::selfdecl::inner_fn();
    format!("{outer} {inner}")
}

/// Calls made from the crate root itself, down into declared modules.
pub fn summarize(v: &[f64]) -> String {
    // module-qualified, one level down from the root
    let m = numbers::mean(v);

    // module-qualified into a different module
    let t = text::upper("summary");

    // NESTED module, two levels down from the root
    let d = text::case::snake(&t);

    // calls a sibling free function declared in this same crate root
    let w = width_of(&d);

    format!("{d} {m} {w}")
}

/// Free function living in the crate root, called from other modules as
/// `crate::width_of(..)`.
pub fn width_of(s: &str) -> usize {
    s.len()
}
