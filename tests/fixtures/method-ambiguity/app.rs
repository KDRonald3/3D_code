//! Application layer, calling into the storage layer.

use crate::cache::{count_hits, Cache, Item};

/// A registry that also has a `get` method -- creates method ambiguity.
pub struct Registry {
    entries: Vec<Item>,
}

impl Registry {
    pub fn new() -> Self {
        Registry { entries: Vec::new() }
    }

    /// Second `get` method in the project, in a different type.
    pub fn get(&self, index: usize) -> Option<&Item> {
        self.entries.get(index)
    }

    /// Calls its own private helper: within-file, unambiguous.
    pub fn total(&self) -> u32 {
        self.tally()
    }

    fn tally(&self) -> u32 {
        count_hits(&self.entries)
    }
}

/// A free function named `describe`, colliding with the one in cache.rs.
pub fn describe(item: &Item) -> String {
    format!("item: {}", item.payload)
}

/// Exercises every interesting call form.
pub fn run() -> u32 {
    let cache = Cache::new();
    let registry = Registry::new();

    // Qualified associated function, cross-file: should be Certain.
    let _ = Cache::new();

    // Method call, name `get` defined on two different types: ambiguous.
    let _ = cache.get("k");
    let _ = registry.get(0);

    // Free function unique across the project: should resolve cross-file.
    let n = count_hits(&[]);

    // Free function defined in BOTH files -- same file should win.
    let _ = describe(&Item { payload: String::new(), hits: 0 });

    n + registry.total()
}
