//! Fixture: recover genuine free-function calls inside allowlisted macros,
//! and refuse every look-alike trap (patterns, stringify, macro_rules bodies,
//! tuple-struct construction).

pub fn mean(v: &[i32]) -> i32 {
    v.iter().sum()
}

pub fn width_of(s: &str) -> usize {
    s.len()
}

pub fn helper() -> i32 {
    1
}

pub fn nested_target() -> i32 {
    2
}

/// Tuple struct — `Foo(1)` is construction, not a free-function call.
pub struct Foo(pub i32);

pub fn run(v: &[i32], s: &str) {
    // Genuine calls hidden in token trees (must be recovered).
    let _ = format!("{}", mean(v));
    println!("{}", width_of(s));
    assert_eq!(helper(), helper());
    let _ = vec![helper()];
    // Nested allowlisted macros: outer println, inner format.
    let _ = println!("{}", format!("{}", nested_target()));

    // Traps — must stay absent from the map.
    let _ = matches!(Some(1), Some(_y));
    let _ = stringify!(helper());
    let _ = cfg!(feature = "never");
    let _ = vec![Foo(1), Foo(2)];

    // Visible (non-macro) call still works.
    let _ = helper();
}

/// Template body is not executed code — `$name($args)` must not become a site.
macro_rules! identity_call {
    ($name:ident($args:expr)) => {
        $name($args)
    };
}

pub fn uses_macro_rules(v: &[i32]) -> i32 {
    identity_call!(mean(v))
}
