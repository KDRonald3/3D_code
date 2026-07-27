//! Storage layer.

use std::collections::HashMap;

/// A cached item.
pub struct Item {
    pub payload: String,
    hits: u32,
}

/// Anything that can store items.
pub trait Store {
    /// Required: fetch by key.
    fn get(&self, key: &str) -> Option<&Item>;
}

/// An in-memory cache.
pub struct Cache {
    map: HashMap<String, Item>,
}

impl Cache {
    /// Build an empty cache.
    pub fn new() -> Self {
        Cache { map: HashMap::new() }
    }

    /// Inherent `get` -- collides with the trait `get` below.
    pub fn get(&self, key: &str) -> Option<&Item> {
        self.touch(key);
        self.map.get(key)
    }

    fn touch(&self, _key: &str) {}
}

impl Store for Cache {
    /// Trait `get` -- same name, same type, different origin.
    fn get(&self, key: &str) -> Option<&Item> {
        self.map.get(key)
    }
}

/// A free function whose name also exists in the other file.
pub fn describe(item: &Item) -> String {
    item.payload.clone()
}

/// Unique free function, should resolve cleanly across files.
pub fn count_hits(items: &[Item]) -> u32 {
    items.iter().map(|i| i.hits).sum()
}
