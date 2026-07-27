/// Only reachable from crates that declare `dep_a` as a dependency.
pub fn shared() -> i32 {
    1
}
