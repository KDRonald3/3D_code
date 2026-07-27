//! Dependency-gate specimen: `dep_a` is declared; `dep_b` is a workspace
//! sibling with the same `shared` name but is not a dependency.

use dep_a::shared;

pub fn run() -> i32 {
    shared()
}

/// Names the undeclared sibling. Must not resolve to `dep_b::shared`.
pub fn wrong_sibling() -> i32 {
    dep_b::shared()
}
