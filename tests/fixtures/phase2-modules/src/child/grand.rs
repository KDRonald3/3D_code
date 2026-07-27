//! Declared from `child.rs` via `mod grand;` — not from the crate root.
//! Path: `crate::child::grand`.

pub fn deep() -> usize {
    let a = super::sibling();
    let b = super::super::sibling_of_child().len();
    a + b
}
