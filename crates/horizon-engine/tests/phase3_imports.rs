//! Integration tests: Phase 3 import table and glob precedence.

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
fn glob_ambiguity_yields_conflict_naming_both_gets() {
    let map = build_function_map(fixture("glob-ambiguity")).expect("build map");
    let json = map_to_string(&map).expect("serialize");
    assert!(serde_json::from_str::<serde_json::Value>(&json).is_ok());

    let krate = &map.crates[0];
    assert_eq!(krate.name, "glob-ambiguity");
    let files = all_files(krate);
    let run = find_fn(&files, "glob_ambiguity::app::run");

    let get = run
        .call_sites
        .iter()
        .find(|c| c.call_path == "get")
        .expect("get(3) call");
    match &get.target {
        CallTarget::Conflict(c) => {
            let names: HashSet<&str> = c.candidates.iter().map(|id| id.as_str()).collect();
            assert_eq!(
                names,
                HashSet::from([
                    "glob_ambiguity::text::get",
                    "glob_ambiguity::shapes::get",
                ])
            );
            assert!(
                c.reason.contains("E0659") || c.reason.to_lowercase().contains("ambiguous"),
                "reason should cite ambiguity: {}",
                c.reason
            );
        }
        other => panic!("expected Conflict, got {other:?}"),
    }
    assert_eq!(map.summary.conflicts, 1);
}

#[test]
fn glob_resolved_explicit_import_wins() {
    let map = build_function_map(fixture("glob-resolved")).expect("build map");
    let krate = &map.crates[0];
    let files = all_files(krate);
    let run = find_fn(&files, "glob_resolved::app::run");

    let get = run
        .call_sites
        .iter()
        .find(|c| c.call_path == "get")
        .expect("get(3)");
    match &get.target {
        CallTarget::Resolved(id) => {
            assert_eq!(id.as_str(), "glob_resolved::text::get");
        }
        other => panic!("explicit import must win over globs, got {other:?}"),
    }
    assert_eq!(map.summary.conflicts, 0);
}

#[test]
fn impl_free_globs_resolves_imports_and_reexports() {
    let map = build_function_map(fixture("impl-free-globs")).expect("build map");
    let krate = &map.crates[0];
    let files = all_files(krate);
    let run = find_fn(&files, "impl_free_globs::app::run");

    let expect_resolved = |path: &str, id_suffix: &str| {
        let edge = run
            .call_sites
            .iter()
            .find(|c| c.call_path == path)
            .unwrap_or_else(|| panic!("missing call {path}"));
        match &edge.target {
            CallTarget::Resolved(id) => {
                assert!(
                    id.as_str().ends_with(id_suffix),
                    "{path} → {} (expected suffix {id_suffix})",
                    id.as_str()
                );
            }
            other => panic!("{path} should resolve, got {other:?}"),
        }
    };

    expect_resolved("mean", "numbers::mean");
    expect_resolved("total", "numbers::sum_all");
    expect_resolved("text::upper", "text::upper");
    expect_resolved("text::case::snake", "text::case::snake");
    expect_resolved("crate::mean", "numbers::mean");
    expect_resolved("shout_upper", "text::upper");
    expect_resolved("area", "shapes::area");
    expect_resolved("describe", "app::describe"); // local beats glob
    expect_resolved("summarize", "summarize");
    expect_resolved("crate::width_of", "width_of");

    assert!(
        !run.call_sites
            .iter()
            .any(|c| c.call_path.starts_with("std::")),
        "std:: must remain dropped"
    );
    assert!(map.summary.external_dropped >= 1);
}

#[test]
fn use_tree_syntax_resolves_nested_braces() {
    let map = build_function_map(fixture("use-tree-syntax")).expect("build map");
    let krate = &map.crates[0];
    assert_eq!(krate.name, "use-tree-syntax");
    let files = all_files(krate);
    let drive = find_fn(&files, "use_tree_syntax::drive");

    let expect = |path: &str, id_end: &str| {
        let edge = drive
            .call_sites
            .iter()
            .find(|c| c.call_path == path)
            .unwrap_or_else(|| panic!("missing {path}, sites: {:?}",
                drive.call_sites.iter().map(|c| &c.call_path).collect::<Vec<_>>()));
        match &edge.target {
            CallTarget::Resolved(id) => {
                assert!(
                    id.as_str().ends_with(id_end),
                    "{path} → {} (want …{id_end})",
                    id.as_str()
                );
            }
            other => panic!("{path} should resolve, got {other:?}"),
        }
    };

    expect("one", "alpha::one");
    expect("second", "alpha::two");
    expect("three", "beta::three");
    expect("four", "alpha::four");
    expect("buried", "beta::deep::buried");
    expect("alpha::one", "alpha::one");

    // HashMap::new is associated — dropped, not unresolved.
    assert!(
        !drive
            .call_sites
            .iter()
            .any(|c| c.call_path.contains("HashMap")),
        "HashMap methods must be absent"
    );
}

#[test]
fn private_fn_not_reachable_by_sibling_glob() {
    // Unit-level coverage also exists in resolve.rs; this guards the
    // path-dependency engine shapes: private `never_visible` must not appear
    // as a glob candidate when a sibling module glob-imports `format`.
    use horizon_engine::extract::{CallOwnerKind, PendingCall, assign_function_ids, extract_facts};
    use horizon_engine::parse::parse_source;
    use horizon_engine::resolve::{ResolveIndex, ResolveResult, resolve_call};
    use std::collections::{HashMap, HashSet};

    let source = r#"
pub mod format {
    pub fn upper() {}
    pub(crate) fn trim_inner() {}
    fn never_visible() {}
}
mod client {
    use crate::format::*;
    pub fn run() {
        upper();
        trim_inner();
        never_visible();
    }
}
"#;
    let tree = parse_source(source, &"2021".into()).unwrap();
    let mut facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();
    assign_function_ids("demo", &mut facts.functions);

    let mut vis = HashMap::new();
    for (f, v) in facts.functions.iter().zip(facts.function_visibility.iter()) {
        vis.insert(f.id.clone(), v.clone());
    }
    let modules = HashSet::from([
        "crate".into(),
        "crate::format".into(),
        "crate::client".into(),
    ]);
    let index = ResolveIndex::build(
        facts.functions.clone(),
        modules,
        HashSet::new(),
        facts.types,
        facts.imports,
        vis,
        facts.local_bindings,
        Default::default(),
        Default::default(),
    );

    let site = |name: &str| PendingCall {
        call_path: name.into(),
        line: 1,
        byte_start: 0,
        byte_end: 1,
        enclosing_function: None,
        module_path: "crate::client".into(),
        owner: CallOwnerKind::File,
        from_macro: false,
        method_receiver: None,
    };

    match resolve_call(&site("upper"), &index).unwrap() {
        ResolveResult::Target(CallTarget::Resolved(id)) => {
            assert_eq!(id.as_str(), "demo::format::upper");
        }
        other => panic!("pub fn via glob: {other:?}"),
    }
    match resolve_call(&site("trim_inner"), &index).unwrap() {
        ResolveResult::Target(CallTarget::Resolved(id)) => {
            assert_eq!(id.as_str(), "demo::format::trim_inner");
        }
        other => panic!("pub(crate) via same-crate glob: {other:?}"),
    }
    match resolve_call(&site("never_visible"), &index).unwrap() {
        ResolveResult::Target(CallTarget::Unresolved(_)) => {}
        other => panic!("private must not come through sibling glob: {other:?}"),
    }
}

#[test]
fn inline_mod_imports_honour_super_glob_and_explicit() {
    let map = build_function_map(fixture("inline-mod-imports")).expect("build map");
    let krate = &map.crates[0];
    assert_eq!(krate.name, "inline-mod-imports");
    let files = all_files(krate);

    let via_glob = find_fn(&files, "inline_mod_imports::tests::via_glob");
    for (path, suffix) in [
        ("parent_private", "inline_mod_imports::parent_private"),
        ("shared", "inline_mod_imports::sibling::shared"),
    ] {
        let edge = via_glob
            .call_sites
            .iter()
            .find(|c| c.call_path == path)
            .unwrap_or_else(|| {
                panic!(
                    "missing {path}; sites: {:?}",
                    via_glob
                        .call_sites
                        .iter()
                        .map(|c| &c.call_path)
                        .collect::<Vec<_>>()
                )
            });
        match &edge.target {
            CallTarget::Resolved(id) => {
                assert_eq!(id.as_str(), suffix, "{path}");
            }
            other => panic!("use super::* must resolve {path}, got {other:?}"),
        }
    }

    let via_explicit = find_fn(&files, "inline_mod_imports::tests::via_explicit");
    let edge = via_explicit
        .call_sites
        .iter()
        .find(|c| c.call_path == "also_private")
        .expect("also_private via explicit use super::");
    match &edge.target {
        CallTarget::Resolved(id) => {
            assert_eq!(id.as_str(), "inline_mod_imports::also_private");
        }
        other => panic!("explicit use super::name must resolve, got {other:?}"),
    }

    assert_eq!(map.summary.unresolved, 0);
    assert_eq!(map.summary.conflicts, 0);
}
