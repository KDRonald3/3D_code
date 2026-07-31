//! Specimen: imports inside inline `mod tests` must be honoured.
//!
//! - `use super::*` sees a private parent function
//! - explicit `use super::name` resolves the same way
//! - a parent private `use` of a sibling free function is also visible via
//!   `use super::*` (rustc: private items are visible to descendants)

mod sibling {
    pub fn shared() -> u32 {
        3
    }
}

use sibling::shared;

fn parent_private() -> u32 {
    1
}

fn also_private() -> u32 {
    2
}

mod tests {
    use super::*;
    use super::also_private;

    fn via_glob() -> u32 {
        parent_private() + shared()
    }

    fn via_explicit() -> u32 {
        also_private()
    }
}
