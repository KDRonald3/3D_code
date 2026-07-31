//! Integration tests: Phase 4 multi-crate discovery and cross-crate resolve.

use horizon_engine::{CallTarget, File, Folder, build_function_map, map_to_string};
use std::collections::HashSet;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is nested under crates/<name>")
        .to_path_buf()
}

fn fixture(name: &str) -> PathBuf {
    workspace_root().join("tests/fixtures")
        .join(name)
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

fn find_crate<'a>(map: &'a horizon_engine::Repository, name: &str) -> &'a horizon_engine::Crate {
    map.crates
        .iter()
        .find(|c| c.name == name || c.rustc_name == name)
        .unwrap_or_else(|| panic!("missing crate {name}; have {:?}",
            map.crates.iter().map(|c| (&c.name, &c.rustc_name)).collect::<Vec<_>>()))
}

fn find_fn<'a>(files: &[&'a File], id: &str) -> &'a horizon_engine::Function {
    for file in files {
        for func in &file.functions {
            if func.id.as_str() == id || func.id.as_str().ends_with(id) {
                return func;
            }
        }
    }
    panic!(
        "missing function {id}; have {:?}",
        files
            .iter()
            .flat_map(|f| f.functions.iter().map(|fn_| fn_.id.as_str()))
            .collect::<Vec<_>>()
    );
}

fn count_sites(map: &horizon_engine::Repository) -> (usize, usize, usize) {
    let mut resolved = 0usize;
    let mut conflict = 0usize;
    let mut unresolved = 0usize;
    let mut walk_files = |files: &[File], folders: &[Folder]| {
        fn go(
            files: &[File],
            folders: &[Folder],
            resolved: &mut usize,
            conflict: &mut usize,
            unresolved: &mut usize,
        ) {
            for file in files {
                for site in &file.call_sites {
                    match &site.target {
                        CallTarget::Resolved(_) => *resolved += 1,
                        CallTarget::Conflict(_) => *conflict += 1,
                        CallTarget::Unresolved(_) => *unresolved += 1,
                    }
                }
                for func in &file.functions {
                    for site in &func.call_sites {
                        match &site.target {
                            CallTarget::Resolved(_) => *resolved += 1,
                            CallTarget::Conflict(_) => *conflict += 1,
                            CallTarget::Unresolved(_) => *unresolved += 1,
                        }
                    }
                }
            }
            for folder in folders {
                go(
                    &folder.files,
                    &folder.folders,
                    resolved,
                    conflict,
                    unresolved,
                );
            }
        }
        go(files, folders, &mut resolved, &mut conflict, &mut unresolved);
    };
    for krate in &map.crates {
        walk_files(&krate.files, &krate.folders);
    }
    (resolved, conflict, unresolved)
}

#[test]
fn path_dependency_discovers_both_crates_and_cross_crate_edge() {
    let map = build_function_map(fixture("path-dependency")).expect("build map");
    let json = map_to_string(&map).expect("serialize");
    assert!(serde_json::from_str::<serde_json::Value>(&json).is_ok());

    let names: HashSet<&str> = map.crates.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.contains("freecrate") && names.contains("text-engine"),
        "expected freecrate + text-engine, got {names:?}"
    );

    let freecrate = find_crate(&map, "freecrate");
    let engine = find_crate(&map, "text-engine");
    assert_eq!(engine.rustc_name, "text_engine");
    assert!(engine.is_library);

    // Files stay independent: engine's format.rs is not under freecrate.
    let free_files = all_files(freecrate);
    let eng_files = all_files(engine);
    assert!(
        !free_files
            .iter()
            .any(|f| f.path.ends_with("format.rs")),
        "freecrate must not own engine files"
    );
    assert!(
        eng_files.iter().any(|f| f.path.ends_with("format.rs")),
        "text-engine must own format.rs"
    );

    let demo = find_fn(&free_files, "freecrate::uses_engine::demo");
    let upper = demo
        .call_sites
        .iter()
        .find(|c| c.call_path == "upper")
        .expect("upper() call from uses_engine::demo");
    match &upper.target {
        CallTarget::Resolved(id) => {
            assert_eq!(id.as_str(), "text_engine::format::upper");
        }
        other => panic!("expected cross-crate Resolved to text_engine::format::upper, got {other:?}"),
    }

    // Excerpt for the report: a real cross-crate edge in JSON form.
    let excerpt = serde_json::json!({
        "call_path": upper.call_path,
        "target": upper.target,
    });
    let excerpt_s = serde_json::to_string_pretty(&excerpt).unwrap();
    assert!(
        excerpt_s.contains("text_engine::format::upper"),
        "JSON excerpt should name the foreign id:\n{excerpt_s}"
    );

    // Re-export / other public path-dep edges also resolve.
    for (path, expect) in [
        ("buried", "text_engine::format::deep::buried"),
        ("engine_upper", "text_engine::format::upper"),
        ("version", "text_engine::version"),
    ] {
        let site = demo
            .call_sites
            .iter()
            .find(|c| c.call_path == path)
            .unwrap_or_else(|| panic!("missing call {path}"));
        match &site.target {
            CallTarget::Resolved(id) => assert_eq!(id.as_str(), expect, "{path}"),
            other => panic!("{path} should resolve to {expect}, got {other:?}"),
        }
    }

    // Qualified calls through an imported path-dep module (plain, renamed
    // submodule, renamed crate root) — the W11 shape.
    let via = find_fn(&free_files, "freecrate::uses_engine::via_imported_module");
    for (path, expect) in [
        ("format::upper", "text_engine::format::upper"),
        ("nested::buried", "text_engine::format::deep::buried"),
        ("eng::version", "text_engine::version"),
    ] {
        let site = via
            .call_sites
            .iter()
            .find(|c| c.call_path == path)
            .unwrap_or_else(|| panic!("missing call {path}"));
        match &site.target {
            CallTarget::Resolved(id) => assert_eq!(id.as_str(), expect, "{path}"),
            other => panic!(
                "imported path-dep module prefix `{path}` should resolve to {expect}, got {other:?}"
            ),
        }
    }

    // Private module: pub fn hidden must NOT be reachable — no resolved edge
    // to text_engine::secret::hidden anywhere in freecrate.
    for file in &free_files {
        for func in &file.functions {
            for site in &func.call_sites {
                if let CallTarget::Resolved(id) = &site.target {
                    assert!(
                        !id.as_str().contains("secret::hidden"),
                        "pub-in-private-module must be unreachable: {} → {}",
                        site.call_path,
                        id.as_str()
                    );
                }
            }
        }
    }

    // pub(crate) trim_inner must not appear as a resolved freecrate edge.
    for file in &free_files {
        for func in &file.functions {
            for site in &func.call_sites {
                if let CallTarget::Resolved(id) = &site.target {
                    assert!(
                        !id.as_str().ends_with("trim_inner"),
                        "pub(crate) must be cross-crate unreachable: {}",
                        id.as_str()
                    );
                }
            }
        }
    }

    let (resolved, conflicts, unresolved) = count_sites(&map);
    eprintln!(
        "path-dependency stats: {} crates, {resolved} resolved, {conflicts} conflicts, {unresolved} unresolved; summary={:?}",
        map.crates.len(),
        map.summary
    );
    assert!(resolved >= 4, "at least the uses_engine public edges");
    assert_eq!(conflicts, map.summary.conflicts);
    assert_eq!(unresolved, map.summary.unresolved);
}

#[test]
fn proc_macro_crate_is_discovered_with_free_functions() {
    let map = build_function_map(fixture("proc-macro-crate")).expect("build map");
    assert_eq!(map.crates.len(), 1, "proc-macro target must yield a crate");
    let krate = find_crate(&map, "proc-macro-crate");
    assert!(krate.is_library, "proc-macro is a library-like target");
    assert_eq!(krate.rustc_name, "proc_macro_crate");

    let files = all_files(krate);
    let expand = find_fn(&files, "proc_macro_crate::expand_name");
    assert_eq!(expand.name, "expand_name");
    let sanitize = find_fn(&files, "proc_macro_crate::sanitize");
    assert_eq!(sanitize.name, "sanitize");
    let identity = find_fn(&files, "proc_macro_crate::identity");
    assert_eq!(identity.name, "identity");

    let site = sanitize
        .call_sites
        .iter()
        .find(|c| c.call_path == "expand_name")
        .expect("expand_name call");
    match &site.target {
        CallTarget::Resolved(id) => assert_eq!(id.as_str(), "proc_macro_crate::expand_name"),
        other => panic!("expected resolved expand_name, got {other:?}"),
    }
}

#[test]
fn renamed_path_dependency_resolves_through_alias() {
    let map = build_function_map(fixture("renamed-path-dep")).expect("build map");
    let consumer = find_crate(&map, "renamed-path-dep");
    let engine = find_crate(&map, "text-engine");
    assert_eq!(engine.rustc_name, "text_engine");

    let dep = consumer
        .dependencies
        .iter()
        .find(|d| d.name == "text-engine")
        .expect("package name from cargo metadata");
    assert_eq!(dep.rename.as_deref(), Some("alias"));
    assert_eq!(dep.import_rustc_name(), "alias");

    let files = all_files(consumer);
    let run = find_fn(&files, "renamed_path_dep::run");
    let site = run
        .call_sites
        .iter()
        .find(|c| c.call_path == "alias::greet")
        .expect("alias::greet call");
    match &site.target {
        CallTarget::Resolved(id) => assert_eq!(id.as_str(), "text_engine::greet"),
        other => panic!("renamed path dep must resolve, got {other:?}"),
    }
}

#[test]
fn workspace_dep_gate_blocks_undeclared_sibling() {
    let map = build_function_map(fixture("workspace-dep-gate")).expect("build map");

    let names: HashSet<&str> = map.crates.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        HashSet::from(["consumer", "dep_a", "dep_b"]),
        "workspace members"
    );

    let consumer = find_crate(&map, "consumer");
    let files = all_files(consumer);
    let run = find_fn(&files, "consumer::run");
    let shared = run
        .call_sites
        .iter()
        .find(|c| c.call_path == "shared")
        .expect("shared()");
    match &shared.target {
        CallTarget::Resolved(id) => assert_eq!(id.as_str(), "dep_a::shared"),
        other => panic!("declared dep must resolve, got {other:?}"),
    }

    let wrong = find_fn(&files, "consumer::wrong_sibling");
    let bad = wrong
        .call_sites
        .iter()
        .find(|c| c.call_path == "dep_b::shared")
        .expect("dep_b::shared call");
    match &bad.target {
        CallTarget::Resolved(id) => panic!(
            "dependency gate failed: resolved undeclared sibling to {}",
            id.as_str()
        ),
        CallTarget::Conflict(c) => panic!("dependency gate failed: conflict {c:?}"),
        CallTarget::Unresolved(_) => {
            // Expected: dep_b is in the repo map but not a declared dependency.
        }
    }
}
