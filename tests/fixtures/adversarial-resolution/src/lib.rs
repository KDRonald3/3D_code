//! Adversarial resolution traps: same names at several depths, local-vs-glob,
//! rename re-export chains, cfg twins, module/function name collision, and
//! nested-inline shadowing. Built to catch confident wrong edges.

pub mod alpha;
pub mod beta;
pub mod chain;
pub mod deep;

/// Same free-function name at crate root — competes with `deep::helper` and
/// `deep::inner::helper` when callers write qualified paths.
pub fn helper() -> u32 {
    1
}

/// Renamed re-export chain entry point: `original` → `hop1` here → `hop2` in beta.
pub use chain::original as hop1;

/// Module whose name equals a free function inside it (`shadow::shadow`).
pub mod shadow {
    pub fn shadow() -> u32 {
        9
    }
}

#[cfg(unix)]
fn twin() -> u32 {
    10
}

#[cfg(windows)]
fn twin() -> u32 {
    11
}

/// Local definition that must beat a glob import of the same name.
pub fn collide() -> u32 {
    2
}

use crate::alpha::*;

pub fn drive() -> u32 {
    // Depth-qualified same names — must not collapse to the wrong helper.
    let a = helper();
    let b = deep::helper();
    let c = deep::inner::helper();

    // Rename chain: written `hop2`, defines as `chain::original`.
    let d = beta::hop2();

    // Module / function name collision.
    let e = shadow::shadow();

    // Local `collide` beats `use crate::alpha::*` which also offers `collide`.
    let f = collide();

    // Nested inline module: inner `probe` sees local `name`, not globbed `alpha::name`.
    let g = nested::probe();

    // Cfg twins — both present in the syntax tree → Conflict.
    let h = twin();

    a + b + c + d + e + f + g + h
}

mod nested {
    use crate::alpha::*;

    /// Local `name` must win over the glob-imported `alpha::name`.
    fn name() -> u32 {
        7
    }

    pub fn probe() -> u32 {
        name()
    }
}
