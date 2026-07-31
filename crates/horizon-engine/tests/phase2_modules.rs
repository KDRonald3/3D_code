//! Integration tests: Phase 2 module walk and within-crate resolution.

use horizon_engine::{CallTarget, File, Folder, build_function_map, map_to_string};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is nested under crates/<name>")
        .to_path_buf()
}

fn fixture(name: &str) -> PathBuf {
    workspace_root().join("tests/fixtures").join(name)
}

fn all_files(krate: &horizon_engine::Crate) -> Vec<&File> {
    let mut out = Vec::new();
    fn walk<'a>(files: &'a [File], folders: &'a [Folder], out: &mut Vec<&'a File>) {
        out.extend(files.iter());
        for folder in folders {
            walk(&folder.files, &folder.folders, out);
        }
    }
    walk(&krate.files, &krate.folders, &mut out);
    out
}

fn find_fn<'a>(files: &[&'a File], id_suffix: &str) -> &'a horizon_engine::Function {
    for file in files {
        for func in &file.functions {
            if func.id.as_str() == id_suffix || func.id.as_str().ends_with(id_suffix) {
                return func;
            }
        }
    }
    panic!("missing function ending with {id_suffix}");
}

#[test]
fn phase2_modules_fixture_resolves_cross_file_and_file_level() {
    let map = build_function_map(fixture("phase2-modules")).expect("build map");
    let _ = map_to_string(&map).expect("serialize");

    assert_eq!(map.crates.len(), 1);
    let krate = &map.crates[0];
    assert_eq!(krate.name, "phase2-modules");

    let files = all_files(krate);
    let paths: Vec<&str> = files
        .iter()
        .filter_map(|f| f.path.file_name().and_then(|n| n.to_str()))
        .collect();

    assert!(paths.contains(&"lib.rs"));
    assert!(paths.contains(&"child.rs"));
    assert!(paths.contains(&"grand.rs"));
    assert!(
        !paths.contains(&"orphan.rs"),
        "undeclared orphan.rs must be absent, got {paths:?}"
    );

    // Folder hierarchy: grand.rs under src/child/
    assert!(
        !krate.folders.is_empty(),
        "expected a Folder for src/child"
    );
    let child_folder = krate
        .folders
        .iter()
        .find(|f| f.path.file_name().and_then(|n| n.to_str()) == Some("child"))
        .expect("child folder");
    assert!(
        child_folder
            .files
            .iter()
            .any(|f| f.path.file_name().and_then(|n| n.to_str()) == Some("grand.rs"))
    );

    let root_fn = find_fn(&files, "phase2_modules::root_fn");
    let child_fn = find_fn(&files, "phase2_modules::child::child_fn");
    let deep = find_fn(&files, "phase2_modules::child::grand::deep");
    let sibling = find_fn(&files, "phase2_modules::child::sibling");
    let compute_max = find_fn(&files, "phase2_modules::compute_max");

    // Cross-file: root_fn → child::child_fn
    let edge = root_fn
        .call_sites
        .iter()
        .find(|c| c.call_path == "child::child_fn")
        .expect("child::child_fn call");
    match &edge.target {
        CallTarget::Resolved(id) => assert_eq!(id, &child_fn.id),
        other => panic!("expected resolved, got {other:?}"),
    }

    // crate:: path
    let edge = root_fn
        .call_sites
        .iter()
        .find(|c| c.call_path == "crate::child::grand::deep")
        .expect("crate::child::grand::deep");
    match &edge.target {
        CallTarget::Resolved(id) => assert_eq!(id, &deep.id),
        other => panic!("expected resolved, got {other:?}"),
    }

    // super:: from child up to crate root
    let edge = child_fn
        .call_sites
        .iter()
        .find(|c| c.call_path == "super::sibling_of_child")
        .expect("super::sibling_of_child");
    match &edge.target {
        CallTarget::Resolved(id) => {
            assert_eq!(id.as_str(), "phase2_modules::sibling_of_child");
        }
        other => panic!("expected resolved, got {other:?}"),
    }

    // super::super:: from grand
    let edge = deep
        .call_sites
        .iter()
        .find(|c| c.call_path == "super::super::sibling_of_child")
        .expect("super::super::sibling_of_child");
    match &edge.target {
        CallTarget::Resolved(id) => {
            assert_eq!(id.as_str(), "phase2_modules::sibling_of_child");
        }
        other => panic!("expected resolved, got {other:?}"),
    }

    // super::sibling from grand
    let edge = deep
        .call_sites
        .iter()
        .find(|c| c.call_path == "super::sibling")
        .expect("super::sibling");
    match &edge.target {
        CallTarget::Resolved(id) => assert_eq!(id, &sibling.id),
        other => panic!("expected resolved, got {other:?}"),
    }

    // File-level const call
    let lib = files
        .iter()
        .find(|f| f.path.file_name().and_then(|n| n.to_str()) == Some("lib.rs"))
        .expect("lib.rs");
    assert_eq!(lib.call_sites.len(), 1);
    assert_eq!(lib.call_sites[0].call_path, "compute_max");
    match &lib.call_sites[0].target {
        CallTarget::Resolved(id) => assert_eq!(id, &compute_max.id),
        other => panic!("file-level call should resolve, got {other:?}"),
    }
}

#[test]
fn phase1_fixture_crate_path_now_resolves() {
    let map = build_function_map(fixture("phase1-single-file")).expect("build map");
    let krate = &map.crates[0];
    let file = &krate.files[0];
    let alpha = file.functions.iter().find(|f| f.name == "alpha").unwrap();
    let beta = file.functions.iter().find(|f| f.name == "beta").unwrap();

    let crate_beta = alpha
        .call_sites
        .iter()
        .find(|c| c.call_path == "crate::beta")
        .expect("crate::beta");
    match &crate_beta.target {
        CallTarget::Resolved(id) => assert_eq!(id, &beta.id),
        other => panic!("Phase 2 should resolve crate::beta, got {other:?}"),
    }

    assert_eq!(map.summary.conflicts, 1);
    assert_eq!(map.summary.unresolved, 1); // mystery only
}

#[test]
fn path_dependency_excludes_orphan_and_handles_path_attr() {
    let map = build_function_map(fixture("path-dependency")).expect("build map");
    let krate = &map.crates[0];
    assert_eq!(krate.name, "freecrate");

    let files = all_files(krate);
    assert!(
        files
            .iter()
            .all(|f| f.path.file_name().and_then(|n| n.to_str()) != Some("orphan.rs")),
        "orphan.rs must not appear in the map"
    );

    let tidy = files
        .iter()
        .find(|f| {
            f.path
                .file_name()
                .and_then(|n| n.to_str())
                == Some("renamed_on_disk.rs")
        })
        .expect("#[path] file renamed_on_disk.rs");
    assert_eq!(tidy.module_path, "crate::tidy_name");

    let helper = files
        .iter()
        .find(|f| f.path.ends_with(Path::new("app").join("helper.rs")))
        .or_else(|| {
            files.iter().find(|f| {
                f.path.file_name().and_then(|n| n.to_str()) == Some("helper.rs")
            })
        })
        .expect("non-root mod helper.rs");
    assert_eq!(helper.module_path, "crate::app::helper");

    // Module-qualified within-crate edge that needs no import.
    let lib = files
        .iter()
        .find(|f| f.module_path == "crate")
        .expect("crate root file");
    let summarize = lib
        .functions
        .iter()
        .find(|f| f.name == "summarize")
        .expect("summarize");
    let mean_edge = summarize
        .call_sites
        .iter()
        .find(|c| c.call_path == "numbers::mean")
        .expect("numbers::mean");
    match &mean_edge.target {
        CallTarget::Resolved(id) => {
            assert_eq!(id.as_str(), "freecrate::numbers::mean");
        }
        other => panic!("expected resolved cross-file edge, got {other:?}"),
    }
}

#[test]
fn excludes_constructors_and_associated_functions() {
    let map = build_function_map(fixture("exclude-non-functions")).expect("build map");
    let krate = &map.crates[0];
    let files = all_files(krate);
    let lib = files
        .iter()
        .find(|f| f.module_path == "crate")
        .expect("lib");
    let run = lib.functions.iter().find(|f| f.name == "run").expect("run");

    let paths: Vec<&str> = run.call_sites.iter().map(|c| c.call_path.as_str()).collect();

    // Group 1 — not function calls.
    assert!(
        !paths.iter().any(|p| *p == "Ok" || p.ends_with("::Ok")),
        "prelude Ok must be absent, got {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.contains("Target::Ready")),
        "local enum variant must be absent, got {paths:?}"
    );

    // Group 2 — impl / associated functions.
    assert!(
        !paths.iter().any(|p| p.contains("LocalId::make")),
        "local associated function must be absent, got {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.contains("Vec::new") || *p == "Vec::new"),
        "external associated function must be absent, got {paths:?}"
    );

    // Genuine unresolved free-function call remains.
    let mystery = run
        .call_sites
        .iter()
        .find(|c| c.call_path == "mystery")
        .expect("mystery should remain as Unresolved");
    match &mystery.target {
        CallTarget::Unresolved(u) => {
            assert!(
                u.reason.contains("no free function") || u.reason.contains("mystery"),
                "unexpected reason: {}",
                u.reason
            );
            assert!(
                !u.reason.contains("needs module"),
                "must not describe a type as a missing module: {}",
                u.reason
            );
        }
        other => panic!("mystery should be Unresolved, got {other:?}"),
    }

    // Control edge still resolves.
    let helper = run
        .call_sites
        .iter()
        .find(|c| c.call_path == "helper")
        .expect("helper");
    assert!(matches!(helper.target, CallTarget::Resolved(_)));

    assert!(map.summary.constructor_dropped >= 2);
    assert!(map.summary.associated_dropped >= 2);
    assert_eq!(map.summary.unresolved, 1);
}

#[test]
fn impl_free_globs_resolves_module_paths_and_keeps_std_dropped() {
    let map = build_function_map(fixture("impl-free-globs")).expect("build map");
    let krate = &map.crates[0];
    let files = all_files(krate);

    let lib = files
        .iter()
        .find(|f| f.module_path == "crate")
        .expect("lib");
    let summarize = lib
        .functions
        .iter()
        .find(|f| f.name == "summarize")
        .expect("summarize");

    // These need no use table.
    for path in ["numbers::mean", "text::upper", "text::case::snake"] {
        let edge = summarize
            .call_sites
            .iter()
            .find(|c| c.call_path == path)
            .unwrap_or_else(|| panic!("missing call {path}"));
        assert!(
            matches!(edge.target, CallTarget::Resolved(_)),
            "{path} should resolve, got {:?}",
            edge.target
        );
    }

    let app = files
        .iter()
        .find(|f| f.module_path == "crate::app")
        .expect("app");
    let run = app.functions.iter().find(|f| f.name == "run").expect("run");

    // Phase 3: bare import-dependent names resolve through the import table.
    let mean_bare = run
        .call_sites
        .iter()
        .find(|c| c.call_path == "mean")
        .expect("bare mean");
    match &mean_bare.target {
        CallTarget::Resolved(id) => {
            assert_eq!(id.as_str(), "impl_free_globs::numbers::mean");
        }
        other => panic!("Phase 3 should resolve imported mean, got {other:?}"),
    }

    // std:: dropped
    assert!(
        !run.call_sites.iter().any(|c| c.call_path.starts_with("std::")),
        "std:: calls must be absent from the map"
    );
    assert!(map.summary.external_dropped >= 1);
}

#[test]
fn macro_modules_recovers_cfg_if_and_cfg_star() {
    let map = build_function_map(fixture("macro-modules")).expect("build map");
    let _ = map_to_string(&map).expect("serialize");

    assert_eq!(map.crates.len(), 1);
    let files = all_files(&map.crates[0]);
    let by_stem: Vec<&str> = files
        .iter()
        .filter_map(|f| f.path.file_stem().and_then(|s| s.to_str()))
        .collect();

    for name in ["lib", "net", "alt", "fallback", "gated", "literal"] {
        assert!(
            by_stem.contains(&name),
            "expected recovered/literal module file {name}, got {by_stem:?}"
        );
    }
    assert!(
        !by_stem.contains(&"must_not_appear"),
        "stringify! must stay closed: {by_stem:?}"
    );
    assert!(
        !by_stem.contains(&"absent_file"),
        "missing module file must not appear: {by_stem:?}"
    );

    let paths: Vec<&str> = files.iter().map(|f| f.module_path.as_str()).collect();
    assert!(paths.contains(&"crate::net"));
    assert!(paths.contains(&"crate::alt"));
    assert!(paths.contains(&"crate::fallback"));
    assert!(paths.contains(&"crate::gated"));
    assert!(paths.contains(&"crate::literal"));

    let entry = find_fn(&files, "macro_modules::entry");
    for (call, suffix) in [
        ("net::net_fn", "macro_modules::net::net_fn"),
        ("alt::alt_fn", "macro_modules::alt::alt_fn"),
        ("fallback::fallback_fn", "macro_modules::fallback::fallback_fn"),
        ("gated::gated_fn", "macro_modules::gated::gated_fn"),
        ("literal::literal_fn", "macro_modules::literal::literal_fn"),
    ] {
        let edge = entry
            .call_sites
            .iter()
            .find(|c| c.call_path == call)
            .unwrap_or_else(|| panic!("missing call {call}"));
        match &edge.target {
            CallTarget::Resolved(id) => assert!(
                id.as_str().ends_with(suffix),
                "{call} -> {} (expected …{suffix})",
                id.as_str()
            ),
            other => panic!("{call}: expected resolved, got {other:?}"),
        }
    }
}
