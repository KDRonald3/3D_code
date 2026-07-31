//! Specimen for deliberate exclusions vs genuine unresolved free-function calls.
//!
//! Exercises: prelude variant `Ok`, qualified local enum variant, associated
//! function on a local type, associated function on an external type (`Vec`),
//! and a bare name that remains `Unresolved` (Phase 3 / unknown).

pub enum Target {
    Ready(u32),
    Done,
}

pub struct LocalId(pub u32);

impl LocalId {
    pub fn make(n: u32) -> Self {
        LocalId(n)
    }
}

pub fn helper() -> u32 {
    1
}

pub fn run() -> Result<u32, ()> {
    // Group 1 — constructors / variants (must be absent from the map).
    let _ = Ok(1u32);
    let _ = Target::Ready(2);

    // Group 2 — associated functions (must be absent from the map).
    let _ = LocalId::make(3);
    let _v: Vec<u32> = Vec::new();

    // Genuine free-function call that cannot be resolved yet.
    mystery();

    // Control: same-module free function still resolves.
    helper();

    Ok(0)
}
