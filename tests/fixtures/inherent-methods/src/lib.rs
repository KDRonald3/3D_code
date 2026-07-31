//! Inherent methods and one-hop method-call resolution.

pub struct Cache {
    pub hits: u32,
}

impl Cache {
    pub fn new() -> Self {
        Cache { hits: 0 }
    }

    pub fn get(&self, _key: &str) -> u32 {
        self.hits
    }
}

pub struct Registry {
    pub n: u32,
}

impl Registry {
    pub fn new() -> Self {
        Registry { n: 0 }
    }

    pub fn get(&self, _i: usize) -> u32 {
        self.n
    }
}

fn identity<T>(t: T) -> T {
    t
}

pub fn run() -> u32 {
    // Associated functions — resolve to inherent methods.
    let cache = Cache::new();
    let registry = Registry::new();

    // One-hop from constructor RHS — certain.
    let a = cache.get("k");
    let b = registry.get(0);

    // Parameter annotations — certain.
    let c = typed(cache, registry);

    // Through a generic — no one-hop type → Conflict on `.get`.
    let d = conflict_demo();

    a + b + c + d
}

fn typed(x: Cache, y: Registry) -> u32 {
    x.get("k") + y.get(0)
}

pub fn conflict_demo() -> u32 {
    let x = identity(Cache::new());
    x.get("k")
}
