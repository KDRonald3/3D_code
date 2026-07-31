//! Inherent methods and one-hop method-call resolution.

pub struct Cache {
    pub hits: u32,
    pub label: String,
}

impl Cache {
    pub fn new() -> Self {
        Cache {
            hits: 0,
            label: String::new(),
        }
    }

    pub fn get(&self, _key: &str) -> u32 {
        self.hits
    }

    pub fn as_str(&self) -> &str {
        self.label.as_str()
    }

    pub fn hits_via_self(&self) -> u32 {
        // Bare `self` receiver — must resolve to Cache::get, not Conflict
        // with Registry::get.
        self.get("k")
    }

    pub fn via_self_path(&self) -> u32 {
        Self::get(self, "k")
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

    pub fn as_str(&self) -> &str {
        "registry"
    }
}

fn identity<T>(t: T) -> T {
    t
}

/// Declared return type is a certain one-hop hint for `let Some(x) = …`.
fn wrap_cache() -> Option<Cache> {
    Some(Cache::new())
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

    // Through a generic — no one-hop type → associated drop (not Conflict).
    let d = untyped_demo();

    // Local fn return type through `let Some` — certain.
    let e = from_return();

    a + b + c + d + e
}

fn typed(x: Cache, y: Registry) -> u32 {
    x.get("k") + y.get(0)
}

pub fn untyped_demo() -> u32 {
    let x = identity(Cache::new());
    // Unknown receiver: must NOT Conflict across Cache/Registry::get.
    x.get("k")
}

pub fn from_return() -> u32 {
    let Some(c) = wrap_cache() else {
        return 0;
    };
    c.get("k")
}
