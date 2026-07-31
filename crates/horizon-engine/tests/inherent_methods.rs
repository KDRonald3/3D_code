//! W8b: inherent methods and one-hop method-call resolution.

use horizon_engine::{CallTarget, build_function_map};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn fixture(name: &str) -> PathBuf {
    workspace_root().join("tests/fixtures").join(name)
}

#[test]
fn inherent_methods_resolve_assoc_and_one_hop_calls() {
    let map = build_function_map(fixture("inherent-methods")).expect("build map");
    let file = &map.crates[0].files[0];

    let methods: Vec<&str> = file
        .functions
        .iter()
        .filter(|f| f.receiver_type.is_some())
        .map(|f| f.module_path.as_str())
        .collect();
    assert!(
        methods.iter().any(|p| p.ends_with("Cache::new")),
        "expected Cache::new method: {methods:?}"
    );
    assert!(
        methods.iter().any(|p| p.ends_with("Cache::get")),
        "expected Cache::get method: {methods:?}"
    );
    assert!(
        methods.iter().any(|p| p.ends_with("Registry::get")),
        "expected Registry::get method: {methods:?}"
    );

    let run = file
        .functions
        .iter()
        .find(|f| f.name == "run")
        .expect("run");

    let resolved_paths: Vec<&str> = run
        .call_sites
        .iter()
        .filter_map(|s| match &s.target {
            CallTarget::Resolved(id) => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        resolved_paths.iter().any(|p| p.ends_with("Cache::new")),
        "Cache::new should resolve: {resolved_paths:?} sites={:?}",
        run.call_sites
    );
    assert!(
        resolved_paths.iter().any(|p| p.ends_with("Registry::new")),
        "Registry::new should resolve: {resolved_paths:?}"
    );
    assert!(
        resolved_paths.iter().any(|p| p.ends_with("Cache::get")),
        "cache.get should resolve: {resolved_paths:?}"
    );
    assert!(
        resolved_paths.iter().any(|p| p.ends_with("Registry::get")),
        "registry.get should resolve: {resolved_paths:?}"
    );

    // Bare `self.method` inside inherent impl — certain Self type.
    let via_self = file
        .functions
        .iter()
        .find(|f| f.name == "hits_via_self")
        .expect("hits_via_self");
    assert!(
        via_self.call_sites.iter().any(|s| {
            matches!(&s.target, CallTarget::Resolved(id) if id.as_str().ends_with("Cache::get"))
        }),
        "self.get inside Cache impl must resolve: {:?}",
        via_self.call_sites
    );

    // `Self::method` rewritten to the impl type.
    let via_path = file
        .functions
        .iter()
        .find(|f| f.name == "via_self_path")
        .expect("via_self_path");
    assert!(
        via_path.call_sites.iter().any(|s| {
            matches!(&s.target, CallTarget::Resolved(id) if id.as_str().ends_with("Cache::get"))
        }),
        "Self::get inside Cache impl must resolve: {:?}",
        via_path.call_sites
    );

    // `self.field.as_str()` on a String field must drop, not Conflict on
    // Cache::as_str / Registry::as_str.
    let as_str_method = file
        .functions
        .iter()
        .find(|f| f.module_path.ends_with("Cache::as_str"))
        .expect("Cache::as_str");
    assert!(
        as_str_method
            .call_sites
            .iter()
            .all(|s| !matches!(s.target, CallTarget::Conflict(_))),
        "String field .as_str must not Conflict: {:?}",
        as_str_method.call_sites
    );

    // Untyped receiver with colliding inherent names → associated drop, not Conflict.
    let untyped = file
        .functions
        .iter()
        .find(|f| f.name == "untyped_demo")
        .expect("untyped_demo");
    assert!(
        untyped
            .call_sites
            .iter()
            .filter(|s| s.call_path == ".get")
            .all(|s| !matches!(s.target, CallTarget::Conflict(_))),
        "untyped .get must not Conflict: {:?}",
        untyped.call_sites
    );

    // `let Some(c) = wrap_cache()` — return type is a certain hint.
    let from_ret = file
        .functions
        .iter()
        .find(|f| f.name == "from_return")
        .expect("from_return");
    assert!(
        from_ret.call_sites.iter().any(|s| {
            matches!(&s.target, CallTarget::Resolved(id) if id.as_str().ends_with("Cache::get"))
        }),
        "return-type hint should resolve c.get: {:?}",
        from_ret.call_sites
    );

    assert_eq!(map.summary.unresolved, 0);
    assert_eq!(map.summary.conflicts, 0);
}
