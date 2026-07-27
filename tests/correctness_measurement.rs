//! Standing correctness checks: fixture oracles + adversarial traps.
//!
//! Re-run the full harness (including optional LSIF) with:
//! `cargo run --bin measure_correctness -- all`

use horizon::correctness::{check_oracle, load_oracle};
use horizon::build_function_map;
use std::path::PathBuf;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn check_fixture(name: &str) {
    let dir = fixtures_root().join(name);
    let oracle = load_oracle(&dir).unwrap_or_else(|e| panic!("{name}: load oracle: {e}"));
    let map = build_function_map(&dir).unwrap_or_else(|e| panic!("{name}: build map: {e}"));
    let report = check_oracle(&map, &oracle);
    if report.passed != report.checked {
        let mut msg = format!(
            "{name}: {}/{} expectations failed\n",
            report.checked - report.passed,
            report.checked
        );
        for m in report
            .false_positives
            .iter()
            .chain(report.false_negatives.iter())
            .chain(report.other_mismatches.iter())
        {
            msg.push_str(&format!(
                "  {} {} {:?} expected={} actual={}\n",
                m.caller, m.call_path, m.line, m.expected, m.actual
            ));
        }
        panic!("{msg}");
    }
}

#[test]
fn oracle_phase1_single_file() {
    check_fixture("phase1-single-file");
}

#[test]
fn oracle_glob_ambiguity() {
    check_fixture("glob-ambiguity");
}

#[test]
fn oracle_glob_resolved() {
    check_fixture("glob-resolved");
}

#[test]
fn oracle_exclude_non_functions() {
    check_fixture("exclude-non-functions");
}

#[test]
fn oracle_impl_free_globs() {
    check_fixture("impl-free-globs");
}

#[test]
fn oracle_use_tree_syntax() {
    check_fixture("use-tree-syntax");
}

#[test]
fn oracle_adversarial_resolution() {
    check_fixture("adversarial-resolution");
}

#[test]
fn oracle_macro_hidden_calls() {
    check_fixture("macro-hidden-calls");
}

#[test]
fn oracle_renamed_path_dep() {
    check_fixture("renamed-path-dep");
}

#[test]
fn oracle_proc_macro_crate() {
    check_fixture("proc-macro-crate");
}

#[test]
fn macro_hidden_calls_mark_provenance_and_skip_traps() {
    let dir = fixtures_root().join("macro-hidden-calls");
    let map = build_function_map(&dir).expect("build map");
    let krate = map.crates.iter().find(|c| c.name == "macro-hidden-calls").unwrap();
    let file = krate.files.iter().find(|f| f.module_path == "crate").unwrap();
    let run = file
        .functions
        .iter()
        .find(|f| f.name == "run")
        .expect("run");

    let paths: Vec<&str> = run.call_sites.iter().map(|s| s.call_path.as_str()).collect();
    assert!(paths.contains(&"mean"));
    assert!(paths.contains(&"width_of"));
    assert!(paths.contains(&"nested_target"));
    assert!(!paths.contains(&"Some"), "matches! patterns must stay out");
    assert!(!paths.contains(&"Foo"), "tuple-struct ctors must stay out");

    let mean = run
        .call_sites
        .iter()
        .find(|s| s.call_path == "mean")
        .unwrap();
    assert!(mean.from_macro, "format! recovery must set from_macro");

    let visible_helpers: Vec<_> = run
        .call_sites
        .iter()
        .filter(|s| s.call_path == "helper" && !s.from_macro)
        .collect();
    assert_eq!(
        visible_helpers.len(),
        1,
        "only the non-macro helper() should be ordinary provenance"
    );

    let macro_helpers = run
        .call_sites
        .iter()
        .filter(|s| s.call_path == "helper" && s.from_macro)
        .count();
    assert!(
        macro_helpers >= 3,
        "assert_eq! (×2) + vec![helper()] should recover helper; got {macro_helpers}"
    );

    let uses = file
        .functions
        .iter()
        .find(|f| f.name == "uses_macro_rules")
        .unwrap();
    assert!(
        uses.call_sites.is_empty(),
        "macro_rules! template / user-macro args must not invent call sites"
    );
}
