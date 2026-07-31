//! The other glob-imported definition of `get`.
//! The explicit import in `app.rs` selects this one.

pub fn get(n: usize) -> usize {
    n + 1
}
