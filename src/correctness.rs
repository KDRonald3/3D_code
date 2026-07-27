//! Correctness measurement: fixture oracles, exclusion audits, LSIF comparison.
//!
//! This module is intentionally separate from the analysis pipeline so it can
//! be re-run as the map shifts under concurrent work. It never guesses on
//! behalf of the resolver; it only compares Horizon's output to ground truth.

use crate::discover;
use crate::extract::PendingCall;
use crate::map::{CallSite, CallTarget, Crate, File, Folder, FunctionId, Repository};
use crate::pipeline::{extract_repository, resolve_index_for};
use crate::resolve::{ExclusionKind, ResolveIndex, ResolveResult, resolve_call};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// How one extracted call site was classified after resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallOutcomeKind {
    Resolved,
    Conflict,
    Unresolved,
    ExternalDropped,
    ConstructorDropped,
    AssociatedDropped,
}

/// One call after extract+resolve, including deliberate drops.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallOutcome {
    pub file: PathBuf,
    pub call_path: String,
    pub line: u32,
    pub byte_start: u32,
    pub byte_end: u32,
    pub enclosing_function: Option<String>,
    pub kind: CallOutcomeKind,
    /// Target FunctionId when resolved; candidate ids when conflict.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Expected completion of one call site in a fixture oracle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExpectedTarget {
    Resolved {
        /// Suffix or full FunctionId (matched with [`id_matches`]).
        id: String,
    },
    Conflict {
        /// Each candidate must appear (suffix match allowed).
        candidates: Vec<String>,
    },
    Unresolved,
    /// Site must be absent from the emitted map (deliberate drop).
    Absent,
}

/// One annotated expectation for a fixture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedEdge {
    /// FunctionId suffix of the caller, or `"(file)"` for file-level sites.
    pub caller: String,
    pub call_path: String,
    /// Optional 1-based line disambiguator when the same path appears twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub expect: ExpectedTarget,
}

/// Hand-written ground truth for one fixture crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureOracle {
    pub fixture: String,
    pub edges: Vec<ExpectedEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeMismatch {
    pub caller: String,
    pub call_path: String,
    pub line: Option<u32>,
    pub expected: String,
    pub actual: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OracleReport {
    pub fixture: String,
    pub checked: usize,
    pub passed: usize,
    pub false_positives: Vec<EdgeMismatch>,
    pub false_negatives: Vec<EdgeMismatch>,
    pub other_mismatches: Vec<EdgeMismatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LsifMatch {
    pub file: String,
    pub line: u32,
    pub call_path: String,
    pub horizon: String,
    pub lsif: String,
}

/// LSIF comparison cohort (ordinary syntax vs macro-recovered edges).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LsifCohort {
    pub compared: usize,
    pub matched: usize,
    pub false_positives: Vec<LsifMatch>,
    pub lsif_impl_only: Vec<LsifMatch>,
    pub unmatched: Vec<LsifMatch>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LsifReport {
    /// Resolved Horizon edges under `src/` that were compared (all provenances).
    pub compared: usize,
    pub matched: usize,
    pub false_positives: Vec<LsifMatch>,
    /// Horizon resolved to a free fn; LSIF only offered `::impl::` symbols.
    pub lsif_impl_only: Vec<LsifMatch>,
    pub unmatched: Vec<LsifMatch>,
    /// Edges recovered from macro argument token trees (`CallSite.from_macro`).
    #[serde(default)]
    pub from_macro: LsifCohort,
    /// Edges from ordinary `CallExpr` syntax (`from_macro == false`).
    #[serde(default)]
    pub ordinary: LsifCohort,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExclusionAudit {
    pub external_dropped: usize,
    pub constructor_dropped: usize,
    pub associated_dropped: usize,
    pub suspicious: Vec<CallOutcome>,
    pub samples: ExclusionSamples,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExclusionSamples {
    pub external: Vec<CallOutcome>,
    pub constructor: Vec<CallOutcome>,
    pub associated: Vec<CallOutcome>,
}

/// Load `expected-edges.json` from a fixture directory.
pub fn load_oracle(fixture_dir: &Path) -> Result<FixtureOracle> {
    let path = fixture_dir.join("expected-edges.json");
    let bytes = fs::read(&path)
        .with_context(|| format!("read oracle {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))
}

/// Compare a built map against a fixture oracle.
pub fn check_oracle(map: &Repository, oracle: &FixtureOracle) -> OracleReport {
    let mut report = OracleReport {
        fixture: oracle.fixture.clone(),
        ..OracleReport::default()
    };

    let sites = collect_map_sites(map);

    for exp in &oracle.edges {
        report.checked += 1;
        let actual = find_site(&sites, &exp.caller, &exp.call_path, exp.line);
        match (&exp.expect, actual) {
            (ExpectedTarget::Absent, None) => report.passed += 1,
            (ExpectedTarget::Absent, Some(site)) => {
                report.false_positives.push(EdgeMismatch {
                    caller: exp.caller.clone(),
                    call_path: exp.call_path.clone(),
                    line: exp.line.or(Some(site.line)),
                    expected: "absent (dropped)".into(),
                    actual: format_site(site),
                });
            }
            (_, None) => {
                report.false_negatives.push(EdgeMismatch {
                    caller: exp.caller.clone(),
                    call_path: exp.call_path.clone(),
                    line: exp.line,
                    expected: format_expected(&exp.expect),
                    actual: "missing from map".into(),
                });
            }
            (ExpectedTarget::Resolved { id }, Some(site)) => match &site.target {
                CallTarget::Resolved(got) if id_matches(got.as_str(), id) => {
                    report.passed += 1;
                }
                CallTarget::Resolved(got) => {
                    report.false_positives.push(EdgeMismatch {
                        caller: exp.caller.clone(),
                        call_path: exp.call_path.clone(),
                        line: Some(site.line),
                        expected: format!("resolved → {id}"),
                        actual: format!("resolved → {}", got.as_str()),
                    });
                }
                other => {
                    report.other_mismatches.push(EdgeMismatch {
                        caller: exp.caller.clone(),
                        call_path: exp.call_path.clone(),
                        line: Some(site.line),
                        expected: format!("resolved → {id}"),
                        actual: format_target(other),
                    });
                }
            },
            (ExpectedTarget::Conflict { candidates }, Some(site)) => match &site.target {
                CallTarget::Conflict(c) if conflict_matches(&c.candidates, candidates) => {
                    report.passed += 1;
                }
                CallTarget::Conflict(c) => {
                    report.other_mismatches.push(EdgeMismatch {
                        caller: exp.caller.clone(),
                        call_path: exp.call_path.clone(),
                        line: Some(site.line),
                        expected: format!("conflict {candidates:?}"),
                        actual: format!(
                            "conflict {:?}",
                            c.candidates.iter().map(|id| id.as_str()).collect::<Vec<_>>()
                        ),
                    });
                }
                CallTarget::Resolved(got) => {
                    report.false_positives.push(EdgeMismatch {
                        caller: exp.caller.clone(),
                        call_path: exp.call_path.clone(),
                        line: Some(site.line),
                        expected: format!("conflict {candidates:?}"),
                        actual: format!("resolved → {} (guessed)", got.as_str()),
                    });
                }
                other => {
                    report.other_mismatches.push(EdgeMismatch {
                        caller: exp.caller.clone(),
                        call_path: exp.call_path.clone(),
                        line: Some(site.line),
                        expected: format!("conflict {candidates:?}"),
                        actual: format_target(other),
                    });
                }
            },
            (ExpectedTarget::Unresolved, Some(site)) => match &site.target {
                CallTarget::Unresolved(_) => report.passed += 1,
                other => {
                    report.other_mismatches.push(EdgeMismatch {
                        caller: exp.caller.clone(),
                        call_path: exp.call_path.clone(),
                        line: Some(site.line),
                        expected: "unresolved".into(),
                        actual: format_target(other),
                    });
                }
            },
        }
    }

    report
}

/// Walk every pending call through resolve and record outcomes (including drops).
///
/// Uses the same extract → resolve-index path as [`crate::build_function_map`],
/// so exclusion counts cannot drift from the CLI pipeline.
pub fn collect_call_outcomes(repo_root: &Path) -> Result<Vec<CallOutcome>> {
    let root = discover::normalize_path(repo_root);
    let extracted = extract_repository(&root)?;

    let mut out = Vec::new();
    for i in 0..extracted.len() {
        let index = resolve_index_for(&extracted, i);
        for (path, _, facts) in &extracted[i].file_facts {
            for pending in &facts.call_sites {
                out.push(outcome_for(pending, path, &index)?);
            }
        }
    }
    Ok(out)
}

/// Stratified exclusion audit with heuristic "suspicious drop" flags.
pub fn audit_exclusions(outcomes: &[CallOutcome], sample_n: usize) -> ExclusionAudit {
    let mut audit = ExclusionAudit::default();
    let mut externals = Vec::new();
    let mut constructors = Vec::new();
    let mut associated = Vec::new();

    // Known free-function names in this run (from resolved edges).
    let known_fns: HashSet<String> = outcomes
        .iter()
        .filter(|o| matches!(o.kind, CallOutcomeKind::Resolved))
        .flat_map(|o| o.targets.iter().cloned())
        .map(|id| id.rsplit("::").next().unwrap_or(&id).to_string())
        .collect();

    for o in outcomes {
        match o.kind {
            CallOutcomeKind::ExternalDropped => {
                audit.external_dropped += 1;
                externals.push(o.clone());
                if looks_suspicious_external(o, &known_fns) {
                    audit.suspicious.push(o.clone());
                }
            }
            CallOutcomeKind::ConstructorDropped => {
                audit.constructor_dropped += 1;
                constructors.push(o.clone());
                if looks_suspicious_constructor(o, &known_fns) {
                    audit.suspicious.push(o.clone());
                }
            }
            CallOutcomeKind::AssociatedDropped => {
                audit.associated_dropped += 1;
                associated.push(o.clone());
                if looks_suspicious_associated(o, &known_fns) {
                    audit.suspicious.push(o.clone());
                }
            }
            _ => {}
        }
    }

    audit.samples.external = take_spread(&externals, sample_n);
    audit.samples.constructor = take_spread(&constructors, sample_n);
    audit.samples.associated = take_spread(&associated, sample_n);
    audit
}

/// Compare Horizon resolved edges under `src/` to an LSIF index from rust-analyzer.
///
/// LSIF monikers for free functions look like `horizon::discover::find_manifests`.
/// Associated/method symbols contain `::impl::` and are out of Horizon's universe.
///
/// Macro-recovered edges (`CallSite.from_macro`) are scored in
/// [`LsifReport::from_macro`] as well as in the aggregate counters, so a
/// regression in recovery precision cannot hide inside the ordinary cohort.
pub fn compare_lsif(map: &Repository, lsif_path: &Path) -> Result<LsifReport> {
    let lsif = load_lsif(lsif_path)?;
    let mut report = LsifReport::default();

    for (file, _caller, site) in iter_resolved_sites(map) {
        if file.components().any(|c| c.as_os_str() == "tests") {
            // Skip integration-test crates / fixtures directories.
            continue;
        }
        let Ok(src) = fs::read(&file) else {
            continue;
        };
        let name = site
            .call_path
            .rsplit("::")
            .next()
            .unwrap_or(site.call_path.as_str());
        let Some((sc, ec)) = name_utf16_span(&src, site.byte_start, site.byte_end, name) else {
            continue;
        };
        let line0 = site.line.saturating_sub(1);
        let key = discover::normalize_path(&file);
        let cands: Vec<&LsifOcc> = lsif
            .get(&key)
            .map(|v| {
                v.iter()
                    .filter(|o| o.line0 == line0 && !(o.ec <= sc || o.sc >= ec))
                    .collect()
            })
            .unwrap_or_default();

        let cands = if cands.is_empty() {
            lsif.get(&key)
                .map(|v| {
                    v.iter()
                        .filter(|o| o.line0 == line0 && o.moniker.rsplit("::").next() == Some(name))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            cands
        };

        let cohort = if site.from_macro {
            &mut report.from_macro
        } else {
            &mut report.ordinary
        };

        report.compared += 1;
        cohort.compared += 1;
        if cands.is_empty() {
            let m = LsifMatch {
                file: file.display().to_string(),
                line: site.line,
                call_path: site.call_path.clone(),
                horizon: func_target(site),
                lsif: "(no overlapping LSIF moniker)".into(),
            };
            cohort.unmatched.push(m.clone());
            report.unmatched.push(m);
            continue;
        }

        // Only trust LSIF occurrences whose identifier equals the callee name.
        // Overlapping ranges for other names on the same line (fields, `Some`,
        // macros) must not be scored as Horizon false positives.
        let named: Vec<_> = cands
            .iter()
            .filter(|o| o.moniker.rsplit("::").next() == Some(name))
            .copied()
            .collect();

        let horizon_id = match &site.target {
            CallTarget::Resolved(id) => id.as_str().to_string(),
            _ => continue,
        };

        if named.is_empty() {
            let m = LsifMatch {
                file: file.display().to_string(),
                line: site.line,
                call_path: site.call_path.clone(),
                horizon: horizon_id,
                lsif: "(no same-name LSIF moniker in span)".into(),
            };
            cohort.unmatched.push(m.clone());
            report.unmatched.push(m);
            continue;
        }

        let free: Vec<_> = named.iter().filter(|o| !o.is_impl).copied().collect();
        let impls: Vec<_> = named.iter().filter(|o| o.is_impl).copied().collect();

        if !free.is_empty() {
            let mons: HashSet<&str> = free.iter().map(|o| o.moniker.as_str()).collect();
            if mons.iter().any(|m| ids_agree(&horizon_id, m)) {
                report.matched += 1;
                cohort.matched += 1;
            } else {
                let m = LsifMatch {
                    file: file.display().to_string(),
                    line: site.line,
                    call_path: site.call_path.clone(),
                    horizon: horizon_id,
                    lsif: format!("{mons:?}"),
                };
                cohort.false_positives.push(m.clone());
                report.false_positives.push(m);
            }
        } else if !impls.is_empty() {
            let m = LsifMatch {
                file: file.display().to_string(),
                line: site.line,
                call_path: site.call_path.clone(),
                horizon: horizon_id,
                lsif: impls
                    .iter()
                    .map(|o| o.moniker.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            };
            cohort.lsif_impl_only.push(m.clone());
            report.lsif_impl_only.push(m);
        } else {
            let m = LsifMatch {
                file: file.display().to_string(),
                line: site.line,
                call_path: site.call_path.clone(),
                horizon: horizon_id,
                lsif: "(empty)".into(),
            };
            cohort.unmatched.push(m.clone());
            report.unmatched.push(m);
        }
    }

    Ok(report)
}

// --- internals -------------------------------------------------------------

struct MapSite<'a> {
    caller: String,
    site: &'a CallSite,
}

fn collect_map_sites(map: &Repository) -> Vec<MapSite<'_>> {
    let mut out = Vec::new();
    for krate in &map.crates {
        for file in all_files(krate) {
            for func in &file.functions {
                for site in &func.call_sites {
                    out.push(MapSite {
                        caller: func.id.as_str().to_string(),
                        site,
                    });
                }
            }
            for site in &file.call_sites {
                out.push(MapSite {
                    caller: "(file)".into(),
                    site,
                });
            }
        }
    }
    out
}

fn all_files(krate: &Crate) -> Vec<&File> {
    let mut out = Vec::new();
    let mut stack: Vec<&Folder> = krate.folders.iter().collect();
    out.extend(krate.files.iter());
    while let Some(folder) = stack.pop() {
        out.extend(folder.files.iter());
        stack.extend(folder.folders.iter());
    }
    out
}

fn find_site<'a>(
    sites: &'a [MapSite<'a>],
    caller: &str,
    call_path: &str,
    line: Option<u32>,
) -> Option<&'a CallSite> {
    sites.iter().find_map(|s| {
        if !id_matches(&s.caller, caller) {
            return None;
        }
        if s.site.call_path != call_path {
            return None;
        }
        if let Some(l) = line {
            if s.site.line != l {
                return None;
            }
        }
        Some(s.site)
    })
}

fn id_matches(actual: &str, expected: &str) -> bool {
    if actual == expected || actual.ends_with(expected) || expected.ends_with(actual) {
        return true;
    }
    // Oracle shorthand: `crate::open#L` matches `crate::open#L20`.
    expected.ends_with("#L") && actual.starts_with(expected)
}

/// Agree that Horizon's FunctionId and an LSIF moniker name the same definition.
///
/// Differences we forgive (not wrong edges):
/// - Horizon binaries use `{crate}[bin]::…`; LSIF uses `{crate}::…`
/// - Nested free functions: Horizon keeps the enclosing fn segment
///   (`mod::outer::inner`); rust-analyzer monikers often omit it (`mod::inner`)
fn ids_agree(horizon: &str, lsif: &str) -> bool {
    let h = horizon.replace("[bin]", "");
    if h == lsif {
        return true;
    }
    let h_parts: Vec<&str> = h.split("::").collect();
    let l_parts: Vec<&str> = lsif.split("::").collect();
    if h_parts.len() == l_parts.len() + 1 && h_parts.last() == l_parts.last() {
        let mut stripped = h_parts.clone();
        stripped.remove(stripped.len() - 2);
        return stripped == l_parts;
    }
    false
}

fn conflict_matches(actual: &[FunctionId], expected: &[String]) -> bool {
    if actual.len() != expected.len() {
        return false;
    }
    // Every expected pattern must match some actual, and every actual must
    // match some expected (bijection up to suffix/`#L` prefix matching).
    expected.iter().all(|exp| {
        actual
            .iter()
            .any(|got| id_matches(got.as_str(), exp))
    }) && actual.iter().all(|got| {
        expected
            .iter()
            .any(|exp| id_matches(got.as_str(), exp))
    })
}

fn format_expected(exp: &ExpectedTarget) -> String {
    match exp {
        ExpectedTarget::Resolved { id } => format!("resolved → {id}"),
        ExpectedTarget::Conflict { candidates } => format!("conflict {candidates:?}"),
        ExpectedTarget::Unresolved => "unresolved".into(),
        ExpectedTarget::Absent => "absent".into(),
    }
}

fn format_site(site: &CallSite) -> String {
    format!("L{} {} → {}", site.line, site.call_path, format_target(&site.target))
}

fn format_target(t: &CallTarget) -> String {
    match t {
        CallTarget::Resolved(id) => format!("resolved → {}", id.as_str()),
        CallTarget::Conflict(c) => format!(
            "conflict {:?}",
            c.candidates.iter().map(|id| id.as_str()).collect::<Vec<_>>()
        ),
        CallTarget::Unresolved(u) => format!("unresolved ({})", u.reason),
    }
}

fn func_target(site: &CallSite) -> String {
    match &site.target {
        CallTarget::Resolved(id) => id.as_str().to_string(),
        other => format_target(other),
    }
}

fn iter_resolved_sites(map: &Repository) -> Vec<(PathBuf, String, &CallSite)> {
    let mut out = Vec::new();
    for krate in &map.crates {
        for file in all_files(krate) {
            for func in &file.functions {
                for site in &func.call_sites {
                    if matches!(site.target, CallTarget::Resolved(_)) {
                        out.push((file.path.clone(), func.id.as_str().to_string(), site));
                    }
                }
            }
            for site in &file.call_sites {
                if matches!(site.target, CallTarget::Resolved(_)) {
                    out.push((file.path.clone(), "(file)".into(), site));
                }
            }
        }
    }
    out
}

fn outcome_for(pending: &PendingCall, path: &Path, index: &ResolveIndex) -> Result<CallOutcome> {
    let (kind, targets, reason) = match resolve_call(pending, index)? {
        ResolveResult::Target(CallTarget::Resolved(id)) => {
            (CallOutcomeKind::Resolved, vec![id.as_str().to_string()], None)
        }
        ResolveResult::Target(CallTarget::Conflict(c)) => (
            CallOutcomeKind::Conflict,
            c.candidates.iter().map(|id| id.as_str().to_string()).collect(),
            Some(c.reason.clone()),
        ),
        ResolveResult::Target(CallTarget::Unresolved(u)) => {
            (CallOutcomeKind::Unresolved, vec![], Some(u.reason.clone()))
        }
        ResolveResult::External => (CallOutcomeKind::ExternalDropped, vec![], None),
        ResolveResult::Excluded(ExclusionKind::VariantOrConstructor) => {
            (CallOutcomeKind::ConstructorDropped, vec![], None)
        }
        ResolveResult::Excluded(ExclusionKind::AssociatedFunction) => {
            (CallOutcomeKind::AssociatedDropped, vec![], None)
        }
    };

    Ok(CallOutcome {
        file: path.to_path_buf(),
        call_path: pending.call_path.clone(),
        line: pending.line,
        byte_start: pending.byte_start,
        byte_end: pending.byte_end,
        enclosing_function: pending
            .enclosing_function
            .as_ref()
            .map(|id| id.as_str().to_string()),
        kind,
        targets,
        reason,
    })
}

fn looks_suspicious_external(o: &CallOutcome, known: &HashSet<String>) -> bool {
    let first = o.call_path.split("::").next().unwrap_or("");
    // External drops should root in std/core/alloc/proc_macro or a dep name.
    // A bare known local function name classified as external is suspicious.
    !matches!(first, "std" | "core" | "alloc" | "proc_macro")
        && known.contains(first)
        && !o.call_path.contains("::")
}

fn looks_suspicious_constructor(o: &CallOutcome, known: &HashSet<String>) -> bool {
    let name = o.call_path.rsplit("::").next().unwrap_or("");
    // Lowercase bare name that is a known free function — constructors are usually UpperCamel / prelude.
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && known.contains(name)
        && !matches!(name, "ok" | "err" | "some" | "none")
}

fn looks_suspicious_associated(o: &CallOutcome, known: &HashSet<String>) -> bool {
    // `module::free_fn` where free_fn is known and first segment is lowercase — may be a module path.
    let segs: Vec<&str> = o.call_path.split("::").collect();
    if segs.len() == 2 {
        let (a, b) = (segs[0], segs[1]);
        a.starts_with(|c: char| c.is_ascii_lowercase())
            && b.starts_with(|c: char| c.is_ascii_lowercase())
            && known.contains(b)
    } else {
        false
    }
}

fn take_spread(items: &[CallOutcome], n: usize) -> Vec<CallOutcome> {
    if items.is_empty() || n == 0 {
        return Vec::new();
    }
    if items.len() <= n {
        return items.to_vec();
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let idx = i * (items.len() - 1) / (n - 1);
        out.push(items[idx].clone());
    }
    out
}

// --- LSIF -----------------------------------------------------------------

#[derive(Debug, Clone)]
struct LsifOcc {
    line0: u32,
    sc: u32,
    ec: u32,
    moniker: String,
    is_impl: bool,
}

fn load_lsif(path: &Path) -> Result<HashMap<PathBuf, Vec<LsifOcc>>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut verts: HashMap<u64, serde_json::Value> = HashMap::new();
    let mut edges: Vec<serde_json::Value> = Vec::new();

    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line)
            .with_context(|| format!("LSIF line {}", i + 1))?;
        match v.get("type").and_then(|t| t.as_str()) {
            Some("vertex") => {
                let id = v["id"].as_u64().context("vertex id")?;
                verts.insert(id, v);
            }
            Some("edge") => edges.push(v),
            _ => {}
        }
    }

    let mut out_edges: HashMap<u64, Vec<&serde_json::Value>> = HashMap::new();
    for e in &edges {
        if let Some(out_v) = e.get("outV").and_then(|x| x.as_u64()) {
            out_edges.entry(out_v).or_default().push(e);
        }
    }

    let mut docs: HashMap<u64, PathBuf> = HashMap::new();
    for (id, v) in &verts {
        if v.get("label").and_then(|l| l.as_str()) == Some("document") {
            if let Some(uri) = v.get("uri").and_then(|u| u.as_str()) {
                docs.insert(*id, uri_to_path(uri)?);
            }
        }
    }

    let mut ranges: HashMap<u64, (u32, u32, u32, Option<u64>)> = HashMap::new();
    for (id, v) in &verts {
        if v.get("label").and_then(|l| l.as_str()) == Some("range") {
            let start = &v["start"];
            let end = &v["end"];
            ranges.insert(
                *id,
                (
                    start["line"].as_u64().unwrap_or(0) as u32,
                    start["character"].as_u64().unwrap_or(0) as u32,
                    end["character"].as_u64().unwrap_or(0) as u32,
                    None,
                ),
            );
        }
    }

    for e in &edges {
        if e.get("label").and_then(|l| l.as_str()) != Some("contains") {
            continue;
        }
        let Some(out_v) = e.get("outV").and_then(|x| x.as_u64()) else {
            continue;
        };
        if !docs.contains_key(&out_v) {
            continue;
        }
        if let Some(in_vs) = e.get("inVs").and_then(|x| x.as_array()) {
            for rid in in_vs {
                if let Some(id) = rid.as_u64() {
                    if let Some(r) = ranges.get_mut(&id) {
                        r.3 = Some(out_v);
                    }
                }
            }
        }
    }

    let mut by_path: HashMap<PathBuf, Vec<LsifOcc>> = HashMap::new();
    for (rid, (line0, sc, ec, doc)) in &ranges {
        let Some(doc_id) = doc else { continue };
        let Some(path) = docs.get(doc_id) else { continue };
        let Some(rs) = follow_next_label(&out_edges, &verts, *rid, "resultSet") else {
            continue;
        };
        let Some(mon) = moniker_of(&out_edges, &verts, rs) else {
            continue;
        };
        by_path.entry(path.clone()).or_default().push(LsifOcc {
            line0: *line0,
            sc: *sc,
            ec: *ec,
            is_impl: mon.contains("::impl::"),
            moniker: mon,
        });
    }

    if by_path.is_empty() {
        bail!("LSIF contained no moniker occurrences");
    }
    Ok(by_path)
}

fn follow_next_label(
    out_edges: &HashMap<u64, Vec<&serde_json::Value>>,
    verts: &HashMap<u64, serde_json::Value>,
    from: u64,
    label: &str,
) -> Option<u64> {
    for e in out_edges.get(&from)? {
        if e.get("label").and_then(|l| l.as_str()) != Some("next") {
            continue;
        }
        let id = e.get("inV")?.as_u64()?;
        if verts.get(&id)?.get("label").and_then(|l| l.as_str()) == Some(label) {
            return Some(id);
        }
    }
    None
}

fn moniker_of(
    out_edges: &HashMap<u64, Vec<&serde_json::Value>>,
    verts: &HashMap<u64, serde_json::Value>,
    result_set: u64,
) -> Option<String> {
    if let Some(mid) = follow_next_label(out_edges, verts, result_set, "moniker") {
        return verts
            .get(&mid)?
            .get("identifier")
            .and_then(|i| i.as_str())
            .map(str::to_string);
    }
    for e in out_edges.get(&result_set)? {
        if e.get("label").and_then(|l| l.as_str()) == Some("moniker") {
            let id = e.get("inV")?.as_u64()?;
            return verts
                .get(&id)?
                .get("identifier")
                .and_then(|i| i.as_str())
                .map(str::to_string);
        }
    }
    None
}

fn uri_to_path(uri: &str) -> Result<PathBuf> {
    let rest = uri
        .strip_prefix("file:///")
        .or_else(|| uri.strip_prefix("file://"))
        .unwrap_or(uri);
    let path = if cfg!(windows) {
        PathBuf::from(rest.replace('/', "\\"))
    } else {
        PathBuf::from(format!("/{rest}"))
    };
    Ok(discover::normalize_path(&path))
}

fn name_utf16_span(src: &[u8], byte_start: u32, byte_end: u32, name: &str) -> Option<(u32, u32)> {
    let start = byte_start as usize;
    let end = (byte_end as usize).min(src.len());
    if start >= src.len() || start > end {
        return None;
    }
    let call = &src[start..end];
    let name_b = name.as_bytes();
    let idx = memchr_rfind(call, name_b)?;
    let name_byte = start + idx;
    let sc = byte_to_utf16_col(src, name_byte)?;
    let ec = sc + utf16_len(name);
    Some((sc, ec))
}

fn memchr_rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    for i in (0..=hay.len() - needle.len()).rev() {
        if &hay[i..i + needle.len()] == needle {
            return Some(i);
        }
    }
    None
}

fn byte_to_utf16_col(src: &[u8], byte_off: usize) -> Option<u32> {
    let line_start = match src[..byte_off].iter().rposition(|&b| b == b'\n') {
        Some(i) => i + 1,
        None => 0,
    };
    let prefix = std::str::from_utf8(&src[line_start..byte_off]).ok()?;
    Some(utf16_len(prefix))
}

fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}
