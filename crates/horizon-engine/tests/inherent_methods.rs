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

    let conflict_fn = file
        .functions
        .iter()
        .find(|f| f.name == "conflict_demo")
        .expect("conflict_demo");
    let has_conflict = conflict_fn.call_sites.iter().any(|s| {
        matches!(&s.target, CallTarget::Conflict(c) if c.candidates.len() >= 2)
    });
    assert!(
        has_conflict,
        "untyped .get should Conflict across Cache/Registry: {:?}",
        conflict_fn.call_sites
    );

    // Indexed Type::new / Type::get resolve; the only drops here should be
    // untyped non-conflicting method calls (none in this fixture's free fns
    // beyond the Conflict case, which is retained).
    assert_eq!(map.summary.unresolved, 0);
}
