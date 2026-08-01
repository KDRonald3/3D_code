//! Extract → resolve-index construction for [`crate::build_type_map`].
//!
//! Reuses [`horizon_engine::discover`], [`horizon_engine::modules`], and
//! [`horizon_engine::parse`] so crate discovery and the `mod` walk are not
//! duplicated. Type/method fact extraction and indexing stay in this crate.

use horizon_engine::discover::{self, rustc_crate_name};
use crate::extract::{
    FileFacts, Import, ItemVisibility, TypeDef, assign_function_ids, extract_facts,
    remap_local_bindings, remap_method_receivers, remap_pending_calls,
};
use crate::map::{Function, FunctionId};
use horizon_map::{Crate, Dependency, DependencyKind};
use horizon_engine::modules::walk_modules;
use horizon_engine::parse::parse_source;
use crate::resolve::{PathCrateIndex, ResolveIndex};
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Convert an engine [`horizon_engine::extract::ItemVisibility`] into this crate's enum.
fn convert_item_visibility(v: horizon_engine::extract::ItemVisibility) -> ItemVisibility {
    match v {
        horizon_engine::extract::ItemVisibility::Public => ItemVisibility::Public,
        horizon_engine::extract::ItemVisibility::Crate => ItemVisibility::Crate,
        horizon_engine::extract::ItemVisibility::Super => ItemVisibility::Super,
        horizon_engine::extract::ItemVisibility::SelfMod => ItemVisibility::SelfMod,
        horizon_engine::extract::ItemVisibility::InPath(p) => ItemVisibility::InPath(p),
        horizon_engine::extract::ItemVisibility::Private => ItemVisibility::Private,
    }
}


/// Per-crate facts after extraction and FunctionId assignment, before resolve.
pub struct ExtractedCrate {
    pub krate: Crate,
    pub rustc_name: String,
    pub file_facts: Vec<(PathBuf, String, FileFacts)>,
    pub functions: Vec<Function>,
    pub function_visibility: HashMap<FunctionId, ItemVisibility>,
    pub module_paths: HashSet<String>,
    pub module_visibility: HashMap<String, ItemVisibility>,
    pub types: Vec<TypeDef>,
    pub imports: Vec<Import>,
    /// Local bindings (`let` / params) keyed by function id, for dropping
    /// closure calls at resolve time.
    pub local_bindings: HashMap<FunctionId, HashSet<String>>,
}

/// Discover every crate under `repo_root` and extract facts for each.
pub fn extract_repository(repo_root: &Path) -> Result<Vec<ExtractedCrate>> {
    let discovered = discover::discover_crates(repo_root)?;
    let mut extracted = Vec::with_capacity(discovered.len());
    for krate in discovered {
        extracted.push(extract_crate(krate)?);
    }
    Ok(extracted)
}

/// Walk `krate`'s modules, extract facts per file, and assign crate-wide [`FunctionId`]s.
pub fn extract_crate(krate: Crate) -> Result<ExtractedCrate> {
    let rustc_name = krate.rustc_name.clone();
    // FunctionIds use a compilation-unit key that may differ from the real
    // rustc name (binaries get `{name}[bin]` — see `Crate::function_id_prefix`).
    let id_prefix = krate.function_id_prefix();
    let walk = walk_modules(&krate)?;

    let mut file_facts: Vec<(PathBuf, String, FileFacts)> = Vec::new();
    for module_file in &walk.files {
        let path = module_file.file.path.clone();
        let module_path = module_file.file.module_path.clone();
        // Raw bytes first so `content_hash` matches a later `std::fs::read`
        // re-check (no newline normalisation — load-bearing on Windows).
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(err) => {
                eprintln!(
                    "horizon: could not read {}: {err} (file omitted from map)",
                    path.display()
                );
                continue;
            }
        };
        let content_hash = horizon_map::content_hash(&bytes);
        let source = match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(err) => {
                eprintln!(
                    "horizon: could not read {}: {err} (file omitted from map)",
                    path.display()
                );
                continue;
            }
        };

        let tree = parse_source(&source, &krate.edition)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        let mut facts = extract_facts(&tree, &source, &id_prefix, &module_path, &krate.edition)
            .with_context(|| format!("failed to extract {}", path.display()))?;
        facts.content_hash = content_hash;
        file_facts.push((path, module_path, facts));
    }

    let mut all_functions: Vec<Function> = Vec::new();
    let mut all_vis: Vec<ItemVisibility> = Vec::new();
    for (_, _, facts) in &file_facts {
        all_functions.extend(facts.functions.iter().cloned());
        all_vis.extend(facts.function_visibility.iter().cloned());
    }
    let remap = assign_function_ids(&id_prefix, &mut all_functions);

    let mut function_visibility: HashMap<FunctionId, ItemVisibility> = HashMap::new();
    for (func, vis) in all_functions.iter().zip(all_vis.iter()) {
        function_visibility.insert(func.id.clone(), vis.clone());
    }

    let mut func_offset = 0usize;
    let mut local_bindings: HashMap<FunctionId, HashSet<String>> = HashMap::new();
    for (_, _, facts) in &mut file_facts {
        let n = facts.functions.len();
        facts.functions = all_functions[func_offset..func_offset + n].to_vec();
        func_offset += n;
        remap_pending_calls(&mut facts.call_sites, &remap);
        remap_local_bindings(&mut facts.local_bindings, &remap);
        remap_method_receivers(&mut facts.method_receivers, &remap);
        for (id, names) in &facts.local_bindings {
            local_bindings
                .entry(id.clone())
                .or_default()
                .extend(names.iter().cloned());
        }
    }

    let types: Vec<TypeDef> = file_facts
        .iter()
        .flat_map(|(_, _, f)| f.types.iter().cloned())
        .collect();
    let imports: Vec<Import> = file_facts
        .iter()
        .flat_map(|(_, _, f)| f.imports.iter().cloned())
        .collect();

    Ok(ExtractedCrate {
        krate,
        rustc_name,
        file_facts,
        functions: all_functions,
        function_visibility,
        module_paths: walk.module_paths,
        module_visibility: walk.module_visibility.into_iter().map(|(k, v)| (k, convert_item_visibility(v))).collect(),
        types,
        imports,
        local_bindings,
    })
}

/// Build a [`ResolveIndex`] for `extracted[i]` with dependency-gated path crates.
pub fn resolve_index_for(extracted: &[ExtractedCrate], i: usize) -> ResolveIndex {
    let all_crates = all_library_indexes(extracted);
    let path_crates = path_crates_for(&extracted[i].krate, extracted, &all_crates);
    let external_crates = external_crate_names(&extracted[i].krate);
    ResolveIndex::build(
        extracted[i].functions.clone(),
        extracted[i].module_paths.clone(),
        external_crates,
        extracted[i].types.clone(),
        extracted[i].imports.clone(),
        extracted[i].function_visibility.clone(),
        extracted[i].local_bindings.clone(),
        path_crates,
        all_crates,
    )
}

/// Build indexes for every library crate, keyed by real rustc name.
///
/// These are consulted when following a foreign crate's `pub use other::…`
/// facade. Initial path entry still goes through [`path_crates_for`].
fn all_library_indexes(all: &[ExtractedCrate]) -> HashMap<String, PathCrateIndex> {
    let mut out = HashMap::new();
    for target in all {
        if !target.krate.is_library {
            continue;
        }
        let aliases = path_dep_aliases(&target.krate, all);
        let index = PathCrateIndex::build(
            target.rustc_name.clone(),
            &target.functions,
            target.module_paths.clone(),
            target.module_visibility.clone(),
            target.function_visibility.clone(),
            &target.imports,
            target.types.clone(),
            aliases,
        );
        out.insert(target.rustc_name.clone(), index);
    }
    out
}

/// Map each declared path dependency's import name → real rustc name.
fn path_dep_aliases(krate: &Crate, all: &[ExtractedCrate]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for dep in &krate.dependencies {
        if dep.kind != DependencyKind::Path {
            continue;
        }
        let Some(target) = find_path_dep_library(dep, all) else {
            continue;
        };
        out.insert(dep.import_rustc_name(), target.rustc_name.clone());
    }
    out
}

/// Build path-crate indexes for dependencies declared by `krate` only.
///
/// Keys are the **import** rustc names (manifest rename alias when present),
/// because that is what appears as the first segment of `use` / call paths.
fn path_crates_for(
    krate: &Crate,
    all: &[ExtractedCrate],
    all_crates: &HashMap<String, PathCrateIndex>,
) -> HashMap<String, PathCrateIndex> {
    let mut out = HashMap::new();
    for dep in &krate.dependencies {
        if dep.kind != DependencyKind::Path {
            continue;
        }
        let Some(target) = find_path_dep_library(dep, all) else {
            continue;
        };
        // Never index a crate as a path dep of itself (same compilation unit).
        if target.rustc_name == krate.rustc_name
            && target.krate.roots.first() == krate.roots.first()
        {
            continue;
        }
        let Some(index) = all_crates.get(&target.rustc_name).cloned() else {
            continue;
        };
        out.insert(dep.import_rustc_name(), index);
    }
    out
}

/// Find the extracted library (preferring `is_library`) that matches a path dependency.
fn find_path_dep_library<'a>(
    dep: &Dependency,
    all: &'a [ExtractedCrate],
) -> Option<&'a ExtractedCrate> {
    let rustc = rustc_crate_name(&dep.name);
    // Prefer the library target of the matching package.
    all.iter()
        .find(|c| {
            c.krate.is_library
                && (c.krate.name == dep.name
                    || c.rustc_name == rustc
                    || rustc_crate_name(&c.krate.name) == rustc)
        })
        .or_else(|| {
            all.iter().find(|c| {
                c.rustc_name == rustc
                    || c.krate.name == dep.name
                    || rustc_crate_name(&c.krate.name) == rustc
            })
        })
}

/// Import rustc names of registry / external dependencies declared by `krate`.
fn external_crate_names(krate: &Crate) -> HashSet<String> {
    krate
        .dependencies
        .iter()
        .filter(|d| d.kind == DependencyKind::External)
        .map(|d| d.import_rustc_name())
        .collect()
}
