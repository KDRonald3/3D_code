//! This file tries to declare ITSELF as a module. `lib.rs` says nothing about it.

pub mod selfdecl {
    pub fn inner_fn() -> &'static str {
        "inner"
    }
}

pub fn outer_fn() -> &'static str {
    "outer"
}
