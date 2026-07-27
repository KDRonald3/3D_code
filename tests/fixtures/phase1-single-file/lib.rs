//! Phase 1 specimen: one file, free functions only, unqualified calls.
//!
//! Exercises resolved edges, recursion, an undefined name, a qualified path
//! (deferred), and `#[cfg]`-duplicated `open` (FunctionId collision + call
//! Conflict).

fn alpha() {
    beta();
    alpha();
    mystery();
    open();
    crate::beta();
}

fn beta() {
    alpha();
}

#[cfg(unix)]
fn open() {}

#[cfg(windows)]
fn open() {}
