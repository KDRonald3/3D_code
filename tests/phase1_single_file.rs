//! Integration test: Phase 1 walking skeleton against `phase1-single-file`.

use horizon::{CallTarget, build_function_map, map_to_string};
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/phase1-single-file")
}

#[test]
fn phase1_fixture_map_edges() {
    let map = build_function_map(fixture_root()).expect("build map");
    let json = map_to_string(&map).expect("serialize");
    assert!(serde_json::from_str::<serde_json::Value>(&json).is_ok());

    assert_eq!(map.crates.len(), 1);
    let krate = &map.crates[0];
    assert_eq!(krate.name, "phase1-single-file");
    assert_eq!(krate.files.len(), 1);

    let file = &krate.files[0];
    assert_eq!(file.module_path, "crate");
    assert_eq!(file.functions.len(), 4, "alpha, beta, and two cfg open defs");

    let alpha = file
        .functions
        .iter()
        .find(|f| f.name == "alpha")
        .expect("alpha");
    let beta = file
        .functions
        .iter()
        .find(|f| f.name == "beta")
        .expect("beta");
    let opens: Vec<_> = file.functions.iter().filter(|f| f.name == "open").collect();
    assert_eq!(opens.len(), 2);

    assert_eq!(alpha.id.as_str(), "phase1_single_file::alpha");
    assert_eq!(beta.id.as_str(), "phase1_single_file::beta");
    for open in &opens {
        assert!(
            open.id.as_str().starts_with("phase1_single_file::open#L"),
            "cfg collision must suffix both ids, got {}",
            open.id
        );
    }

    // alpha: beta(), alpha(), mystery(), open(), crate::beta()
    assert_eq!(alpha.call_sites.len(), 5);

    match &alpha.call_sites[0].target {
        CallTarget::Resolved(id) => assert_eq!(id, &beta.id),
        other => panic!("beta() should resolve, got {other:?}"),
    }
    assert_eq!(alpha.call_sites[0].call_path, "beta");

    match &alpha.call_sites[1].target {
        CallTarget::Resolved(id) => assert_eq!(id, &alpha.id),
        other => panic!("recursive alpha() should resolve, got {other:?}"),
    }

    match &alpha.call_sites[2].target {
        CallTarget::Unresolved(u) => {
            assert!(u.reason.contains("mystery"), "got {}", u.reason);
        }
        other => panic!("mystery() should be unresolved, got {other:?}"),
    }
    assert_eq!(alpha.call_sites[2].call_path, "mystery");

    match &alpha.call_sites[3].target {
        CallTarget::Conflict(c) => {
            assert_eq!(c.candidates.len(), 2);
            for open in &opens {
                assert!(c.candidates.contains(&open.id));
            }
        }
        other => panic!("open() should conflict, got {other:?}"),
    }
    assert_eq!(alpha.call_sites[3].call_path, "open");

    match &alpha.call_sites[4].target {
        CallTarget::Resolved(id) => assert_eq!(id, &beta.id),
        other => panic!("crate::beta() should resolve in phase 2, got {other:?}"),
    }
    assert_eq!(alpha.call_sites[4].call_path, "crate::beta");

    // beta: alpha()
    assert_eq!(beta.call_sites.len(), 1);
    match &beta.call_sites[0].target {
        CallTarget::Resolved(id) => assert_eq!(id, &alpha.id),
        other => panic!("alpha() in beta should resolve, got {other:?}"),
    }

    assert_eq!(map.summary.conflicts, 1);
    assert_eq!(map.summary.unresolved, 1); // mystery only
}
