//! Module-level doc comment for the sample.

// A free-floating TODO that documents nothing in particular.

use std::collections::HashMap;

/// A cached item.
#[derive(Debug, Clone)]
pub struct Item {
    /// The stored payload.
    pub payload: String,
    hits: u32,
}

/// Kinds of eviction policy.
pub enum Eviction {
    Lru,
    Ttl { seconds: u64 },
}

/// Anything that can store items.
pub trait Store {
    /// Required: fetch by key.
    fn get(&self, key: &str) -> Option<&Item>;

    /// Provided: fetch or panic.
    fn get_or_panic(&self, key: &str) -> &Item {
        self.get(key).expect("missing key")
    }
}

/// An in-memory cache.
pub struct Cache {
    map: HashMap<String, Item>,
    eviction: Eviction,
}

impl Cache {
    /// Build an empty cache.
    pub fn new(eviction: Eviction) -> Self {
        Cache { map: HashMap::new(), eviction }
    }

    // Not a doc comment, just an ordinary note.
    pub async fn warm(&mut self, keys: &[String]) {
        for k in keys {
            self.touch(k);
        }
    }

    const fn capacity() -> usize {
        1024
    }

    unsafe fn raw(&self) -> *const Item {
        std::ptr::null()
    }

    fn touch(&mut self, key: &str) {
        if let Some(item) = self.map.get_mut(key) {
            item.hits += 1;
        }
    }
}

impl Store for Cache {
    fn get(&self, key: &str) -> Option<&Item> {
        self.map.get(key)
    }
}

/// A free function that uses a closure.
pub fn count_hits(items: &[Item]) -> u32 {
    items.iter().map(|i| i.hits).sum()
}

/// A function containing a nested function.
fn outer() -> u32 {
    fn inner() -> u32 {
        7
    }
    inner()
}

/// Takes a function pointer: `fn` here is a TYPE, not a definition.
fn apply(f: fn(u32) -> u32, v: u32) -> u32 {
    f(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_counts_hits() {
        let c = Cache::new(Eviction::Lru);
        assert_eq!(count_hits(&[]), 0);
    }
}
