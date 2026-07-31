//! Phase 2 specimen: module walk, cross-file paths, `super` hops, and a
//! module-level `const fn` call. No `use` imports — every resolvable edge is
//! reachable by module path alone.

mod child;

/// Module-level call site (no enclosing free function).
const fn compute_max() -> usize {
    64
}

const MAX: usize = compute_max();

pub fn root_fn() -> usize {
    let a = child::child_fn();
    let b = crate::child::grand::deep();
    let c = child::grand::deep();
    a + b + c + MAX
}

pub fn sibling_of_child() -> &'static str {
    "root"
}
