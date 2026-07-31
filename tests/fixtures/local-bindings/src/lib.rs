//! Specimen: local bindings (closures / params) vs nested free `fn` items.
//!
//! Closure calls must be dropped from the map (not Unresolved). Nested `fn`
//! items remain real free functions and must still resolve.

pub fn helper() -> u32 {
    1
}

pub fn run_with_closure() -> u32 {
    let by_name = |n: u32| n + 1;
    by_name(helper())
}

pub fn run_closure_in_macro() -> u32 {
    let by_local = |n: u32| n + 2;
    assert_eq!(by_local(1), 3);
    by_local(helper())
}

pub fn run_with_nested_fn() -> u32 {
    fn nested() -> u32 {
        7
    }
    nested() + helper()
}

pub fn run_with_param(cb: fn(u32) -> u32) -> u32 {
    cb(helper())
}
