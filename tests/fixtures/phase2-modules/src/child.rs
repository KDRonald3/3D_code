//! Declared from `lib.rs` as `mod child;`.

mod grand;

pub fn child_fn() -> usize {
    let a = grand::deep();
    let b = self::grand::deep();
    // `super` is the crate root (child was declared in lib.rs).
    let c = super::sibling_of_child().len();
    a + b + c
}

pub fn sibling() -> usize {
    1
}
