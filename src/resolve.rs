//! Stage 6: path resolution within one crate, plus gated cross-crate edges.
//!
//! Resolve each pending call to a [`CallTarget`]: exactly one definition,
//! a [`Conflict`] (several candidates), or unresolved (none). Refuse anything
//! rooted in an external crate (those calls are identified and dropped, not
//! drawn). Also drop forms that are not free-function calls: enum-variant /
//! tuple-struct constructors, and associated functions on types (`Type::f`).
//!
//! # Phase 3 — import table
//!
//! Unqualified names and path prefixes consult the import table built at
//! extract time. Precedence matches rustc:
//!
//! 1. A **locally defined** free function outranks any import (including globs).
//! 2. An **explicit** `use` outranks any glob import.
//! 3. **Two globs** offering the same name → [`Conflict`] (rustc `E0659`),
//!    listing every candidate — never a guessed winner.
//! 4. An explicit `use` colliding with a local definition of the same name is
//!    rustc `E0255`. Represented as a [`Conflict`] naming both the local
//!    definition(s) and the import target(s): the tool never picks a winner,
//!    and both names are real candidates the author wrote.
//!
//! Re-export chains (`pub use`, renamed `pub use`) are followed with a hop
//! bound of [`REEXPORT_HOP_LIMIT`] so a cycle cannot hang the tool.
//!
//! # Phase 4 — cross-crate
//!
//! Path-dependency crates present in [`ResolveIndex::path_crates`] may be
//! resolved into. Membership in that map is **dependency-gated** by the
//! pipeline: only crates the current crate declares as path dependencies are
//! inserted. Workspace siblings that define the same name but are not
//! dependencies never appear, so no wrong edge can form.
//!
//! Cross-crate reachability requires:
//! - the item itself is `pub` (not `pub(crate)` / private / …);
//! - every module on the path from the foreign crate root is `pub`
//!   (a `pub fn` inside a private module is unreachable — module-chain check).
//!
//! That is deliberately stricter than within-crate **direct** edges, which
//! still do not filter visibility (Phase 2): a call the author wrote is worth
//! mapping even if rustc would reject it. Cross-crate, visibility determines
//! whether a candidate exists at all.
//!
//! # Visibility (within-crate glob candidate sets)
//!
//! Phase 2's rule stands for **direct** call edges: a path the author wrote
//! still appears even when rustc would reject it for visibility. **Glob**
//! imports are different — a glob only brings in names that are visible from
//! the importing module through the globbed module. Including private items
//! would manufacture `E0659` conflicts rustc would never report. Therefore:
//!
//! - Glob candidate sets filter by [`ItemVisibility`]: private items are
//!   visible only in their defining module and its descendants (so
//!   `use super::*` in a child still sees the parent's private helpers);
//!   `pub` / `pub(crate)` are visible crate-wide; `pub(super)` / `pub(in …)`
//!   use best-effort path checks.
//! - Public re-exports in the globbed module contribute their local names
//!   when the re-export itself is visible from the importer.
//!
//! # Non-function CallExpr forms
//!
//! Same exclusions as Phase 2b. Once imports bind names, classification still
//! applies: `use crate::map::CallTarget` then `CallTarget::Resolved(..)` is a
//! constructor drop; `use std::fs` then `fs::write(..)` is an external drop.
//! Prefer known import / module / type facts over the leading-uppercase
//! type-name convention whenever the table can tell the truth.
//!
//! # Function-body `use` limitation
//!
//! Imports marked [`Import::scope_widened`] are treated as module-wide even
//! though rustc scopes them to the function body. See `extract` module docs.

use crate::extract::{Import, ItemVisibility, PendingCall, TypeDef, TypeKind};
use crate::map::{CallTarget, Conflict, Function, FunctionId, UnresolvedCall};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

/// Maximum re-export / import-target hops before giving up.
///
/// Chosen high enough for real facade crates, low enough that a cycle cannot
/// hang the tool. Exhausting the bound yields `Unresolved` with a reason that
/// names the limit.
pub const REEXPORT_HOP_LIMIT: usize = 32;

/// Why a recognised CallExpr was deliberately omitted from the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExclusionKind {
    /// Enum variant or tuple-struct constructor — constructs a value, not a call.
    VariantOrConstructor,
    /// Associated function on a type (`Type::name`) — `impl` item, out of scope.
    AssociatedFunction,
}

/// Result of resolving one call site.
#[derive(Debug, Clone)]
pub enum ResolveResult {
    /// Attach this target to the map's [`crate::map::CallSite`].
    Target(CallTarget),
    /// Call into `std` / a registry dependency — dropped from the map.
    External,
    /// Recognised non-mapped form — dropped, not unresolved.
    Excluded(ExclusionKind),
}

/// One explicit (non-glob) import binding in a module.
#[derive(Debug, Clone)]
struct ExplicitBinding {
    /// Absolutized target path (`crate::text::get`, `std::fs`, …).
    target: String,
    /// Visibility of the `use` item (re-export visibility).
    visibility: ItemVisibility,
    /// Whether the use has any `pub` — a re-export others can path to.
    is_reexport: bool,
}

/// Index of one path-dependency crate for cross-crate resolution.
///
/// Only inserted into [`ResolveIndex::path_crates`] when the current crate
/// declares that dependency — the dependency gate lives at construction time.
#[derive(Debug, Clone, Default)]
pub struct PathCrateIndex {
    pub rustc_name: String,
    pub modules: HashSet<String>,
    pub module_visibility: HashMap<String, ItemVisibility>,
    pub function_visibility: HashMap<FunctionId, ItemVisibility>,
    by_full_path: HashMap<String, Vec<FunctionId>>,
    by_parent_name: HashMap<(String, String), Vec<FunctionId>>,
    explicits: HashMap<(String, String), Vec<ExplicitBinding>>,
    type_by_module_name: HashMap<(String, String), usize>,
    types: Vec<TypeDef>,
}

impl PathCrateIndex {
    /// Build a foreign-crate index from that crate's extracted facts.
    pub fn build(
        rustc_name: impl Into<String>,
        functions: &[Function],
        modules: HashSet<String>,
        module_visibility: HashMap<String, ItemVisibility>,
        function_visibility: HashMap<FunctionId, ItemVisibility>,
        imports: &[Import],
        types: Vec<TypeDef>,
    ) -> Self {
        let rustc_name = rustc_name.into();
        let mut by_full_path: HashMap<String, Vec<FunctionId>> = HashMap::new();
        let mut by_parent_name: HashMap<(String, String), Vec<FunctionId>> = HashMap::new();
        for func in functions {
            by_full_path
                .entry(func.module_path.clone())
                .or_default()
                .push(func.id.clone());
            if let Some((parent, name)) = split_parent_name(&func.module_path) {
                by_parent_name
                    .entry((parent, name))
                    .or_default()
                    .push(func.id.clone());
            }
        }

        let mut type_by_module_name = HashMap::new();
        for (idx, ty) in types.iter().enumerate() {
            type_by_module_name.insert((ty.module_path.clone(), ty.name.clone()), idx);
        }

        let mut explicits: HashMap<(String, String), Vec<ExplicitBinding>> = HashMap::new();
        let empty = HashSet::new();
        for imp in imports {
            if imp.is_glob {
                continue;
            }
            let Some(local) = imp.local_name().map(str::to_string) else {
                continue;
            };
            // Absolutize bare re-export targets (`format::upper` → `crate::format::upper`).
            let path = normalize_bare_import_path(
                &imp.path,
                &imp.module_path,
                &modules,
                &empty,
                &empty,
            );
            explicits
                .entry((imp.module_path.clone(), local))
                .or_default()
                .push(ExplicitBinding {
                    target: path,
                    visibility: imp.visibility.clone(),
                    is_reexport: imp.is_public,
                });
        }

        Self {
            rustc_name,
            modules,
            module_visibility,
            function_visibility,
            by_full_path,
            by_parent_name,
            explicits,
            type_by_module_name,
            types,
        }
    }

    fn lookup_full(&self, full_path: &str) -> Vec<FunctionId> {
        self.by_full_path
            .get(full_path)
            .cloned()
            .unwrap_or_default()
    }

    fn lookup_in_module(&self, module: &str, name: &str) -> Vec<FunctionId> {
        self.by_parent_name
            .get(&(module.to_string(), name.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    fn function_vis(&self, id: &FunctionId) -> ItemVisibility {
        self.function_visibility
            .get(id)
            .cloned()
            .unwrap_or(ItemVisibility::Private)
    }

    fn module_vis(&self, module: &str) -> ItemVisibility {
        self.module_visibility
            .get(module)
            .cloned()
            .unwrap_or(ItemVisibility::Private)
    }

    fn explicits_in(&self, module: &str, name: &str) -> &[ExplicitBinding] {
        self.explicits
            .get(&(module.to_string(), name.to_string()))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn find_type(&self, module: &str, name: &str) -> Option<&TypeDef> {
        self.type_by_module_name
            .get(&(module.to_string(), name.to_string()))
            .and_then(|&idx| self.types.get(idx))
    }
}

/// Index of definitions, modules, types, and imports used during resolution.
#[derive(Debug, Default)]
pub struct ResolveIndex {
    pub functions: Vec<Function>,
    /// Known module paths (`crate`, `crate::shapes`, inline modules, …).
    pub modules: HashSet<String>,
    /// Rustc names of external dependencies (`serde`, …) plus `std`/`core`/`alloc`.
    pub external_crates: HashSet<String>,
    /// Path-dependency crates declared by the current crate, keyed by rustc name.
    /// Absence of a workspace sibling here is the dependency gate.
    pub path_crates: HashMap<String, PathCrateIndex>,
    /// Local type definitions collected at extract time.
    pub types: Vec<TypeDef>,
    /// `(parent_module_path, function_name)` → candidate ids.
    by_parent_name: HashMap<(String, String), Vec<FunctionId>>,
    /// Full function `module_path` → candidate ids.
    by_full_path: HashMap<String, Vec<FunctionId>>,
    /// `(module_path, type_name)` → index into `types`.
    type_by_module_name: HashMap<(String, String), usize>,
    /// `type_name` → indices into `types` (crate-wide, for imported type paths).
    types_by_name: HashMap<String, Vec<usize>>,
    /// `(module_path, variant_name)` → enum type indices declaring that variant.
    variant_in_module: HashMap<(String, String), Vec<usize>>,
    /// Function visibility keyed by id (glob filtering).
    function_visibility: HashMap<FunctionId, ItemVisibility>,
    /// Explicit imports: `(importing_module, local_name)` → bindings.
    explicits: HashMap<(String, String), Vec<ExplicitBinding>>,
    /// Glob imports: `importing_module` → globbed module paths.
    globs: HashMap<String, Vec<String>>,
}

impl ResolveIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build an index from crate-wide definitions, modules, types, and imports.
    ///
    /// `path_crates` must already be dependency-gated: only path dependencies
    /// declared by the crate being resolved.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        functions: Vec<Function>,
        modules: HashSet<String>,
        external_crates: HashSet<String>,
        types: Vec<TypeDef>,
        imports: Vec<Import>,
        function_visibility: HashMap<FunctionId, ItemVisibility>,
        path_crates: HashMap<String, PathCrateIndex>,
    ) -> Self {
        let mut by_parent_name: HashMap<(String, String), Vec<FunctionId>> = HashMap::new();
        let mut by_full_path: HashMap<String, Vec<FunctionId>> = HashMap::new();

        for func in &functions {
            by_full_path
                .entry(func.module_path.clone())
                .or_default()
                .push(func.id.clone());
            if let Some((parent, name)) = split_parent_name(&func.module_path) {
                by_parent_name
                    .entry((parent, name))
                    .or_default()
                    .push(func.id.clone());
            }
        }

        let mut type_by_module_name = HashMap::new();
        let mut types_by_name: HashMap<String, Vec<usize>> = HashMap::new();
        let mut variant_in_module: HashMap<(String, String), Vec<usize>> = HashMap::new();

        for (idx, ty) in types.iter().enumerate() {
            type_by_module_name.insert((ty.module_path.clone(), ty.name.clone()), idx);
            types_by_name
                .entry(ty.name.clone())
                .or_default()
                .push(idx);
            if ty.kind == TypeKind::Enum {
                for variant in &ty.variants {
                    variant_in_module
                        .entry((ty.module_path.clone(), variant.clone()))
                        .or_default()
                        .push(idx);
                }
            }
        }

        let mut explicits: HashMap<(String, String), Vec<ExplicitBinding>> = HashMap::new();
        let mut globs: HashMap<String, Vec<String>> = HashMap::new();
        let path_crate_names: HashSet<String> = path_crates.keys().cloned().collect();

        // Normalize bare import paths (`use numbers::mean`, `use std::fs`) now
        // that the module set and external crate names are known. Edition 2018+
        // resolves a bare first segment as a local child module when one
        // exists, otherwise as an external / path-dep crate (local shadows extern).
        for imp in imports {
            let path = normalize_bare_import_path(
                &imp.path,
                &imp.module_path,
                &modules,
                &external_crates,
                &path_crate_names,
            );
            if imp.is_glob {
                globs
                    .entry(imp.module_path.clone())
                    .or_default()
                    .push(path);
                continue;
            }
            let Some(local) = imp.local_name().map(str::to_string) else {
                continue;
            };
            explicits
                .entry((imp.module_path.clone(), local))
                .or_default()
                .push(ExplicitBinding {
                    target: path,
                    visibility: imp.visibility,
                    is_reexport: imp.is_public,
                });
        }

        Self {
            functions,
            modules,
            external_crates,
            path_crates,
            types,
            by_parent_name,
            by_full_path,
            type_by_module_name,
            types_by_name,
            variant_in_module,
            function_visibility,
            explicits,
            globs,
        }
    }

    /// Look up definitions by [`FunctionId`].
    pub fn get(&self, id: &FunctionId) -> Option<&Function> {
        self.functions.iter().find(|f| f.id == *id)
    }

    fn lookup_full(&self, full_path: &str) -> Vec<FunctionId> {
        self.by_full_path
            .get(full_path)
            .cloned()
            .unwrap_or_default()
    }

    fn lookup_in_module(&self, module: &str, name: &str) -> Vec<FunctionId> {
        self.by_parent_name
            .get(&(module.to_string(), name.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    fn function_vis(&self, id: &FunctionId) -> ItemVisibility {
        self.function_visibility
            .get(id)
            .cloned()
            .unwrap_or(ItemVisibility::Private)
    }

    /// Resolve a type name relative to `module`, then crate-wide if unique.
    fn find_type(&self, module: &str, name: &str) -> Option<&TypeDef> {
        if let Some(&idx) = self
            .type_by_module_name
            .get(&(module.to_string(), name.to_string()))
        {
            return self.types.get(idx);
        }
        let idxs = self.types_by_name.get(name)?;
        if idxs.len() == 1 {
            return self.types.get(idxs[0]);
        }
        None
    }

    fn find_type_at_path(&self, full_path: &str) -> Option<&TypeDef> {
        let (module, name) = split_parent_name(full_path)?;
        self.type_by_module_name
            .get(&(module, name))
            .and_then(|&idx| self.types.get(idx))
    }

    fn has_variant_in_module(&self, module: &str, name: &str) -> bool {
        self.variant_in_module
            .contains_key(&(module.to_string(), name.to_string()))
    }

    fn enum_has_variant(&self, ty: &TypeDef, variant: &str) -> bool {
        ty.kind == TypeKind::Enum && ty.variants.iter().any(|v| v == variant)
    }

    fn explicits_in(&self, module: &str, name: &str) -> &[ExplicitBinding] {
        self.explicits
            .get(&(module.to_string(), name.to_string()))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn globs_in(&self, module: &str) -> &[String] {
        self.globs
            .get(module)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Public re-exports of `name` in `module` that are visible from `from`.
    fn visible_reexports(&self, module: &str, name: &str, from: &str) -> Vec<String> {
        self.explicits_in(module, name)
            .iter()
            .filter(|b| b.is_reexport && is_visible_from(&b.visibility, module, from))
            .map(|b| b.target.clone())
            .collect()
    }
}

/// Absolutize a bare import path using the module tree and extern crate set.
///
/// Paths already rooted at `crate` / `self` / `super` are returned unchanged
/// (`self`/`super` were resolved at extract time). A bare first segment that
/// names a child of `importing_module` becomes a crate-local path; otherwise,
/// if it names an external crate it is left as-is; otherwise it is still
/// treated as crate-local (mid-edit tolerance — may later resolve as a module).
fn normalize_bare_import_path(
    path: &str,
    importing_module: &str,
    modules: &HashSet<String>,
    external_crates: &HashSet<String>,
    path_crates: &HashSet<String>,
) -> String {
    let segments: Vec<&str> = path.split("::").filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return path.to_string();
    }
    if matches!(segments[0], "crate" | "self" | "super") {
        return path.to_string();
    }
    let as_child = extend_module_path(importing_module, segments[0]);
    let local_full = if importing_module == "crate" {
        format!("crate::{}", segments.join("::"))
    } else {
        format!("{importing_module}::{}", segments.join("::"))
    };
    // Local module (or a path under one) wins over an external crate of the
    // same name — matching rustc.
    if modules.contains(&as_child) || modules.contains(&local_full) {
        return local_full;
    }
    // A function/type living directly under the importing module.
    if segments.len() == 1 {
        let item = extend_module_path(importing_module, segments[0]);
        if modules.iter().any(|m| m.starts_with(&format!("{item}::"))) {
            return local_full;
        }
    }
    if matches!(segments[0], "std" | "core" | "alloc" | "proc_macro")
        || external_crates.contains(segments[0])
        || path_crates.contains(segments[0])
    {
        // Keep path-dep roots as `text_engine::…` (not rewritten under `crate::`).
        return path.to_string();
    }
    // Unknown bare path: prefer crate-local (the fixtures' `pub use numbers::mean`
    // shape). External-only names are already caught above when declared.
    local_full
}

/// Resolve a single pending call against `index`.
pub fn resolve_call(site: &PendingCall, index: &ResolveIndex) -> Result<ResolveResult> {
    let path = site.call_path.as_str();
    let segments: Vec<&str> = path
        .split("::")
        .map(strip_segment_generics)
        .filter(|s| !s.is_empty())
        .collect();
    if segments.is_empty() {
        return Ok(ResolveResult::Target(unresolved(format!(
            "malformed call path `{path}`"
        ))));
    }

    // Path-dependency crate written at the call site (dependency-gated via index).
    if let Some(foreign) = index.path_crates.get(segments[0]) {
        return Ok(resolve_cross_crate_path(
            &segments[1..],
            foreign,
            path,
            0,
        ));
    }

    // External crate written at the call site.
    if is_external_root(segments[0], index) {
        return Ok(ResolveResult::External);
    }

    if segments.len() == 1 {
        return Ok(resolve_unqualified(segments[0], site, index));
    }

    Ok(resolve_qualified(&segments, site, index))
}

fn is_external_root(first: &str, index: &ResolveIndex) -> bool {
    if matches!(first, "std" | "core" | "alloc" | "proc_macro") {
        return true;
    }
    if matches!(first, "crate" | "self" | "super") {
        return false;
    }
    // Path deps are resolvable, not external.
    if index.path_crates.contains_key(first) {
        return false;
    }
    index.external_crates.contains(first)
}

fn resolve_unqualified(name: &str, site: &PendingCall, index: &ResolveIndex) -> ResolveResult {
    let mut locals = index.lookup_in_module(&site.module_path, name);

    // Nested free functions live under the enclosing function's path.
    if let Some(enc_id) = site.enclosing_function.as_ref() {
        if let Some(enc) = index.get(enc_id) {
            for id in index.lookup_in_module(&enc.module_path, name) {
                if !locals.contains(&id) {
                    locals.push(id);
                }
            }
        }
    }

    let explicits = index.explicits_in(&site.module_path, name);

    // E0255: local definition + explicit import of the same name.
    if !locals.is_empty() && !explicits.is_empty() {
        let mut candidates = locals;
        for binding in explicits {
            match resolve_target_path(&binding.target, site, index, 0) {
                TargetResolve::Functions(ids) => {
                    for id in ids {
                        if !candidates.contains(&id) {
                            candidates.push(id);
                        }
                    }
                }
                TargetResolve::External => {}
                TargetResolve::Type(_) | TargetResolve::Module(_) => {}
                TargetResolve::Unresolved => {}
                TargetResolve::Excluded(kind) => {
                    return ResolveResult::Excluded(kind);
                }
            }
        }
        return ResolveResult::Target(CallTarget::Conflict(Conflict {
            candidates,
            reason: format!(
                "name `{name}` is both a local definition and an explicit import \
                 in `{}` (rustc E0255)",
                site.module_path
            ),
        }));
    }

    if !locals.is_empty() {
        return match locals.as_slice() {
            [only] => ResolveResult::Target(CallTarget::Resolved(only.clone())),
            _ => ResolveResult::Target(CallTarget::Conflict(Conflict {
                candidates: locals,
                reason: format!(
                    "multiple definitions of `{name}` visible in module `{}`",
                    site.module_path
                ),
            })),
        };
    }

    // Explicit import outranks globs.
    if !explicits.is_empty() {
        return resolve_explicit_bindings(name, explicits, site, index);
    }

    // Glob imports — ambiguity at the point of use (E0659).
    let glob_fns = collect_glob_function_candidates(name, &site.module_path, index);
    if !glob_fns.is_empty() {
        return match unique_or_conflict(
            glob_fns,
            format!(
                "`{name}` is ambiguous because of multiple glob imports in `{}` \
                 (rustc E0659)",
                site.module_path
            ),
        ) {
            Ok(r) => r,
            Err(r) => r,
        };
    }

    // Glob may have imported a type of this name — constructor form `Name(...)`.
    if glob_brings_type(name, &site.module_path, index) {
        return ResolveResult::Excluded(ExclusionKind::VariantOrConstructor);
    }

    // No free function — classify constructors / prelude variants.
    if is_prelude_variant(name) {
        return ResolveResult::Excluded(ExclusionKind::VariantOrConstructor);
    }
    if index.find_type(&site.module_path, name).is_some() {
        return ResolveResult::Excluded(ExclusionKind::VariantOrConstructor);
    }
    if index.has_variant_in_module(&site.module_path, name) {
        return ResolveResult::Excluded(ExclusionKind::VariantOrConstructor);
    }

    ResolveResult::Target(unresolved(format!(
        "no free function `{name}` in module `{}`",
        site.module_path
    )))
}

fn resolve_explicit_bindings(
    name: &str,
    explicits: &[ExplicitBinding],
    site: &PendingCall,
    index: &ResolveIndex,
) -> ResolveResult {
    let mut fn_ids = Vec::new();
    let mut saw_external = false;
    let mut exclusion: Option<ExclusionKind> = None;

    for binding in explicits {
        match resolve_target_path(&binding.target, site, index, 0) {
            TargetResolve::Functions(ids) => {
                for id in ids {
                    if !fn_ids.contains(&id) {
                        fn_ids.push(id);
                    }
                }
            }
            TargetResolve::External => saw_external = true,
            TargetResolve::Type(_) => {
                exclusion = Some(ExclusionKind::VariantOrConstructor);
            }
            TargetResolve::Module(_) => {
                // Calling a module name is nonsense — unresolved.
            }
            TargetResolve::Excluded(kind) => exclusion = Some(kind),
            TargetResolve::Unresolved => {}
        }
    }

    if !fn_ids.is_empty() {
        return match unique_or_conflict(
            fn_ids,
            format!("multiple import targets for `{name}` in `{}`", site.module_path),
        ) {
            Ok(r) => r,
            Err(r) => r,
        };
    }
    if let Some(kind) = exclusion {
        return ResolveResult::Excluded(kind);
    }
    if saw_external {
        return ResolveResult::External;
    }
    ResolveResult::Target(unresolved(format!(
        "import `{name}` in `{}` did not resolve to a free function",
        site.module_path
    )))
}

fn resolve_qualified(segments: &[&str], site: &PendingCall, index: &ResolveIndex) -> ResolveResult {
    let path = segments.join("::");
    let mut module = site.module_path.clone();
    let mut i = 0usize;

    // Leading scope keywords.
    while i < segments.len() {
        match segments[i] {
            "crate" => {
                module = "crate".to_string();
                i += 1;
            }
            "self" => {
                i += 1;
            }
            "super" => {
                match parent_module(&module) {
                    Some(p) => module = p,
                    None => {
                        return ResolveResult::Target(unresolved(format!(
                            "`super` has no parent from module `{}` in `{path}`",
                            site.module_path
                        )));
                    }
                }
                i += 1;
            }
            _ => break,
        }
    }

    let remaining = &segments[i..];
    if remaining.is_empty() {
        return ResolveResult::Target(unresolved(format!(
            "call path `{path}` names a module, not a function"
        )));
    }

    // Navigate: each non-final segment is a module (child, import, or re-export).
    for (offset, seg) in remaining[..remaining.len() - 1].iter().enumerate() {
        let child = extend_module_path(&module, seg);
        if index.modules.contains(&child) {
            module = child;
            continue;
        }

        // Import binding for this segment in the current module.
        let bindings = index.explicits_in(&module, seg);
        if !bindings.is_empty() {
            // Prefer a unique module / external / type binding.
            match resolve_segment_binding(bindings, site, index) {
                SegmentBind::Module(m) => {
                    module = m;
                    continue;
                }
                SegmentBind::External => return ResolveResult::External,
                SegmentBind::Type(ty_path) => {
                    let suffix = &remaining[offset..];
                    return classify_from_type_path(&ty_path, &suffix[1..], &path, index);
                }
                SegmentBind::Functions(_) => {
                    // A function name used as a path prefix — not a free-fn call.
                    return ResolveResult::Target(unresolved(format!(
                        "path `{path}` uses `{seg}` as a module, but it names a function"
                    )));
                }
                SegmentBind::Ambiguous => {
                    return ResolveResult::Target(unresolved(format!(
                        "path `{path}`: segment `{seg}` has ambiguous import bindings in `{module}`"
                    )));
                }
                SegmentBind::None => {}
            }
        }

        // Re-export of a module under this name?
        let reexports = index.visible_reexports(&module, seg, &site.module_path);
        if reexports.len() == 1 && index.modules.contains(&reexports[0]) {
            module = reexports[0].clone();
            continue;
        }

        // Not a module — classify the suffix (type path / convention).
        let suffix = &remaining[offset..];
        return classify_non_module_suffix(suffix, &module, &path, site, index);
    }

    // Final segment = function name (or type constructor).
    let func_name = remaining[remaining.len() - 1];
    match lookup_item_in_module(&module, func_name, site, index, 0) {
        TargetResolve::Functions(ids) => match ids.as_slice() {
            [] => unreachable!(),
            [only] => ResolveResult::Target(CallTarget::Resolved(only.clone())),
            _ => ResolveResult::Target(CallTarget::Conflict(Conflict {
                candidates: ids,
                reason: format!("multiple definitions of `{func_name}` in `{module}`"),
            })),
        },
        TargetResolve::External => ResolveResult::External,
        TargetResolve::Type(_) => ResolveResult::Excluded(ExclusionKind::VariantOrConstructor),
        TargetResolve::Module(_) => ResolveResult::Target(unresolved(format!(
            "call path `{path}` names a module, not a function"
        ))),
        TargetResolve::Excluded(kind) => ResolveResult::Excluded(kind),
        TargetResolve::Unresolved => {
            if index.find_type(&module, func_name).is_some()
                || index.has_variant_in_module(&module, func_name)
                || is_prelude_variant(func_name)
                || looks_like_type_name(func_name)
            {
                return ResolveResult::Excluded(ExclusionKind::VariantOrConstructor);
            }
            ResolveResult::Target(unresolved(format!(
                "no free function `{func_name}` in module `{module}`"
            )))
        }
    }
}

#[allow(dead_code)] // Functions: import binds a fn used as a path prefix (rare).
enum SegmentBind {
    Module(String),
    External,
    Type(String),
    Functions(Vec<FunctionId>),
    Ambiguous,
    None,
}

fn resolve_segment_binding(
    bindings: &[ExplicitBinding],
    site: &PendingCall,
    index: &ResolveIndex,
) -> SegmentBind {
    let mut module: Option<String> = None;
    let mut ty: Option<String> = None;
    let mut fns = Vec::new();
    let mut external = false;

    for binding in bindings {
        match resolve_target_path(&binding.target, site, index, 0) {
            TargetResolve::Module(m) => {
                if module.as_ref().is_some_and(|x| x != &m) {
                    return SegmentBind::Ambiguous;
                }
                module = Some(m);
            }
            TargetResolve::External => external = true,
            TargetResolve::Type(t) => {
                if ty.as_ref().is_some_and(|x| x != &t) {
                    return SegmentBind::Ambiguous;
                }
                ty = Some(t);
            }
            TargetResolve::Functions(ids) => fns.extend(ids),
            TargetResolve::Excluded(_) | TargetResolve::Unresolved => {}
        }
    }

    if let Some(m) = module {
        return SegmentBind::Module(m);
    }
    if external {
        return SegmentBind::External;
    }
    if let Some(t) = ty {
        return SegmentBind::Type(t);
    }
    if !fns.is_empty() {
        return SegmentBind::Functions(fns);
    }
    SegmentBind::None
}

fn classify_from_type_path(
    ty_path: &str,
    after_type: &[&str],
    full_path: &str,
    index: &ResolveIndex,
) -> ResolveResult {
    let _ = full_path;
    if after_type.is_empty() {
        return ResolveResult::Excluded(ExclusionKind::VariantOrConstructor);
    }
    if after_type.len() == 1 {
        let member = after_type[0];
        if let Some(ty) = index.find_type_at_path(ty_path) {
            if index.enum_has_variant(ty, member) {
                return ResolveResult::Excluded(ExclusionKind::VariantOrConstructor);
            }
            return ResolveResult::Excluded(ExclusionKind::AssociatedFunction);
        }
        if is_prelude_variant(member) {
            return ResolveResult::Excluded(ExclusionKind::VariantOrConstructor);
        }
        return ResolveResult::Excluded(ExclusionKind::AssociatedFunction);
    }
    ResolveResult::Excluded(ExclusionKind::AssociatedFunction)
}

/// Look up `name` in `module`: local fn, visible re-export, or type/module.
fn lookup_item_in_module(
    module: &str,
    name: &str,
    site: &PendingCall,
    index: &ResolveIndex,
    hops: usize,
) -> TargetResolve {
    if hops > REEXPORT_HOP_LIMIT {
        return TargetResolve::Unresolved;
    }

    let locals = index.lookup_full(&extend_module_path(module, name));
    if !locals.is_empty() {
        return TargetResolve::Functions(locals);
    }

    // Visible re-exports (and same-module private uses when caller is in module).
    let mut targets = Vec::new();
    for binding in index.explicits_in(module, name) {
        let visible = if binding.is_reexport {
            is_visible_from(&binding.visibility, module, &site.module_path)
        } else {
            // Private `use` only binds inside the importing module.
            module == site.module_path
                || is_descendant(&site.module_path, module)
        };
        // For qualified paths like `crate::mean` from another module, only
        // re-exports apply. When already navigating inside `module` as the
        // path prefix, the call site may still be elsewhere — re-export
        // visibility is the right filter for path segments after the first.
        // Exception: if the path was built by walking into `module` from the
        // call site, names resolved *in* that module for the final segment
        // should include re-exports visible from the call site.
        let _ = visible;
        if binding.is_reexport {
            if is_visible_from(&binding.visibility, module, &site.module_path) {
                targets.push(binding.target.clone());
            }
        } else if module == site.module_path {
            // Unqualified already handled; for `self::name` after self→module,
            // private uses in the same module count.
            targets.push(binding.target.clone());
        }
    }

    // Also: when looking up via a fully qualified path into `module`, private
    // uses there must NOT be visible. Only the re-export branch above applies.
    // The `module == site.module_path` branch covers `self::foo` where foo is
    // imported privately in the same module.

    if targets.len() == 1 {
        return resolve_target_path(&targets[0], site, index, hops + 1);
    }
    if targets.len() > 1 {
        let mut ids = Vec::new();
        for t in &targets {
            if let TargetResolve::Functions(found) = resolve_target_path(t, site, index, hops + 1) {
                for id in found {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
        }
        if !ids.is_empty() {
            return TargetResolve::Functions(ids);
        }
    }

    if index.modules.contains(&extend_module_path(module, name)) {
        return TargetResolve::Module(extend_module_path(module, name));
    }
    if index.find_type(module, name).is_some() {
        return TargetResolve::Type(extend_module_path(module, name));
    }

    TargetResolve::Unresolved
}

#[allow(dead_code)] // Excluded: reserved when import targets classify as non-fn.
enum TargetResolve {
    Functions(Vec<FunctionId>),
    Module(String),
    Type(String),
    External,
    Excluded(ExclusionKind),
    Unresolved,
}

fn resolve_target_path(
    target: &str,
    site: &PendingCall,
    index: &ResolveIndex,
    hops: usize,
) -> TargetResolve {
    if hops > REEXPORT_HOP_LIMIT {
        return TargetResolve::Unresolved;
    }

    let segments: Vec<&str> = target.split("::").filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return TargetResolve::Unresolved;
    }

    // Cross-crate import target (`text_engine::format::upper`).
    if let Some(foreign) = index.path_crates.get(segments[0]) {
        return match resolve_cross_crate_path(&segments[1..], foreign, target, hops) {
            ResolveResult::Target(CallTarget::Resolved(id)) => {
                TargetResolve::Functions(vec![id])
            }
            ResolveResult::Target(CallTarget::Conflict(c)) => {
                TargetResolve::Functions(c.candidates)
            }
            ResolveResult::External => TargetResolve::External,
            ResolveResult::Excluded(kind) => TargetResolve::Excluded(kind),
            ResolveResult::Target(CallTarget::Unresolved(_)) => TargetResolve::Unresolved,
        };
    }

    if is_external_root(segments[0], index) {
        return TargetResolve::External;
    }

    // Absolutize relative roots against the *importing* module… but targets
    // are already absolutized at extract time. Still handle `crate::…`.
    if index.modules.contains(target) {
        return TargetResolve::Module(target.to_string());
    }
    if let Some(ty) = index.find_type_at_path(target) {
        let _ = ty;
        return TargetResolve::Type(target.to_string());
    }

    let direct = index.lookup_full(target);
    if !direct.is_empty() {
        return TargetResolve::Functions(direct);
    }

    // Follow re-export at the target's parent.
    if let Some((parent, name)) = split_parent_name(target) {
        let reexports = index.visible_reexports(&parent, &name, &site.module_path);
        // Also follow same-module private uses when resolving an import target
        // that itself points at a re-export name in its defining module.
        let mut targets = reexports;
        if targets.is_empty() {
            for binding in index.explicits_in(&parent, &name) {
                if binding.is_reexport {
                    targets.push(binding.target.clone());
                }
            }
        }
        if targets.len() == 1 {
            return resolve_target_path(&targets[0], site, index, hops + 1);
        }
        if targets.len() > 1 {
            let mut ids = Vec::new();
            for t in &targets {
                if let TargetResolve::Functions(found) =
                    resolve_target_path(t, site, index, hops + 1)
                {
                    for id in found {
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                }
            }
            if !ids.is_empty() {
                return TargetResolve::Functions(ids);
            }
        }

        if index.modules.contains(target) {
            return TargetResolve::Module(target.to_string());
        }
    }

    TargetResolve::Unresolved
}

/// Resolve `segments` inside a foreign path-dependency crate.
///
/// Enforces cross-crate visibility: only `pub` items behind an all-`pub`
/// module chain are reachable. Re-export hops stay within the foreign crate
/// and respect [`REEXPORT_HOP_LIMIT`].
fn resolve_cross_crate_path(
    segments: &[&str],
    foreign: &PathCrateIndex,
    full_path: &str,
    hops: usize,
) -> ResolveResult {
    if hops > REEXPORT_HOP_LIMIT {
        return ResolveResult::Target(unresolved(format!(
            "re-export chain exceeded {REEXPORT_HOP_LIMIT} hops resolving `{full_path}`"
        )));
    }
    if segments.is_empty() {
        return ResolveResult::Target(unresolved(format!(
            "path `{full_path}` names a crate, not a function"
        )));
    }

    let mut module = "crate".to_string();
    for seg in &segments[..segments.len() - 1] {
        let child = extend_module_path(&module, seg);
        if foreign.modules.contains(&child) {
            if !matches!(foreign.module_vis(&child), ItemVisibility::Public) {
                return ResolveResult::Target(unresolved(format!(
                    "module `{child}` in `{}` is not `pub` (cross-crate unreachable via `{full_path}`)",
                    foreign.rustc_name
                )));
            }
            module = child;
            continue;
        }

        // Public re-export of a module (or path prefix) under this name.
        match lookup_cross_crate_name(foreign, &module, seg, hops) {
            CrossLookup::Module(m) => {
                module = m;
                continue;
            }
            CrossLookup::Functions(_) => {
                return ResolveResult::Target(unresolved(format!(
                    "path `{full_path}` uses `{seg}` as a module, but it names a function"
                )));
            }
            CrossLookup::Type => {
                return ResolveResult::Excluded(ExclusionKind::AssociatedFunction);
            }
            CrossLookup::None => {
                return ResolveResult::Target(unresolved(format!(
                    "no public module `{seg}` in `{}` for `{full_path}`",
                    foreign.rustc_name
                )));
            }
        }
    }

    let name = segments[segments.len() - 1];
    match lookup_cross_crate_name(foreign, &module, name, hops) {
        CrossLookup::Functions(ids) => match ids.as_slice() {
            [only] => ResolveResult::Target(CallTarget::Resolved(only.clone())),
            _ if ids.len() > 1 => ResolveResult::Target(CallTarget::Conflict(Conflict {
                candidates: ids,
                reason: format!(
                    "multiple public definitions of `{name}` in `{}::{module}`",
                    foreign.rustc_name
                ),
            })),
            _ => ResolveResult::Target(unresolved(format!(
                "no public free function `{name}` reachable in `{}` via `{full_path}`",
                foreign.rustc_name
            ))),
        },
        CrossLookup::Module(_) => ResolveResult::Target(unresolved(format!(
            "path `{full_path}` names a module, not a function"
        ))),
        CrossLookup::Type => ResolveResult::Excluded(ExclusionKind::VariantOrConstructor),
        CrossLookup::None => {
            if foreign.find_type(&module, name).is_some() || looks_like_type_name(name) {
                return ResolveResult::Excluded(ExclusionKind::VariantOrConstructor);
            }
            ResolveResult::Target(unresolved(format!(
                "no public free function `{name}` reachable in `{}` via `{full_path}`",
                foreign.rustc_name
            )))
        }
    }
}

enum CrossLookup {
    Functions(Vec<FunctionId>),
    Module(String),
    Type,
    None,
}

fn lookup_cross_crate_name(
    foreign: &PathCrateIndex,
    module: &str,
    name: &str,
    hops: usize,
) -> CrossLookup {
    if hops > REEXPORT_HOP_LIMIT {
        return CrossLookup::None;
    }

    // Direct function definitions — must be `pub`, and parent module chain already checked.
    let mut fn_ids = Vec::new();
    for id in foreign.lookup_in_module(module, name) {
        if matches!(foreign.function_vis(&id), ItemVisibility::Public) {
            fn_ids.push(id);
        }
    }
    // Also match via full path (covers unusual layouts).
    let full = extend_module_path(module, name);
    for id in foreign.lookup_full(&full) {
        if matches!(foreign.function_vis(&id), ItemVisibility::Public) && !fn_ids.contains(&id) {
            fn_ids.push(id);
        }
    }
    if !fn_ids.is_empty() {
        return CrossLookup::Functions(fn_ids);
    }

    // Public re-exports only (`pub use` / `pub use … as`).
    let mut reexport_targets = Vec::new();
    for binding in foreign.explicits_in(module, name) {
        if binding.is_reexport && matches!(binding.visibility, ItemVisibility::Public) {
            reexport_targets.push(binding.target.clone());
        }
    }
    if reexport_targets.len() == 1 {
        return resolve_foreign_reexport(foreign, &reexport_targets[0], hops + 1);
    }
    if reexport_targets.len() > 1 {
        let mut ids = Vec::new();
        for t in &reexport_targets {
            if let CrossLookup::Functions(found) = resolve_foreign_reexport(foreign, t, hops + 1) {
                for id in found {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
        }
        if !ids.is_empty() {
            return CrossLookup::Functions(ids);
        }
    }

    let child = extend_module_path(module, name);
    if foreign.modules.contains(&child) {
        if matches!(foreign.module_vis(&child), ItemVisibility::Public) {
            return CrossLookup::Module(child);
        }
        return CrossLookup::None;
    }

    if foreign.find_type(module, name).is_some() {
        return CrossLookup::Type;
    }

    CrossLookup::None
}

fn resolve_foreign_reexport(foreign: &PathCrateIndex, target: &str, hops: usize) -> CrossLookup {
    if hops > REEXPORT_HOP_LIMIT {
        return CrossLookup::None;
    }
    let segments: Vec<&str> = target.split("::").filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return CrossLookup::None;
    }

    // Re-exports inside a foreign crate are crate-local (`crate::format::upper`).
    // They must not jump to a third crate from here without a declared dep edge
    // on that third crate (out of scope for this hop — treat as none).
    if !matches!(segments[0], "crate" | "self" | "super") {
        return CrossLookup::None;
    }

    let mut module = "crate".to_string();
    let mut i = 0usize;
    while i < segments.len() {
        match segments[i] {
            "crate" => {
                module = "crate".into();
                i += 1;
            }
            "self" => i += 1,
            "super" => {
                match parent_module(&module) {
                    Some(p) => module = p,
                    None => return CrossLookup::None,
                }
                i += 1;
            }
            _ => break,
        }
    }
    let remaining = &segments[i..];
    if remaining.is_empty() {
        return CrossLookup::Module(module);
    }

    for seg in &remaining[..remaining.len() - 1] {
        let child = extend_module_path(&module, seg);
        if foreign.modules.contains(&child) {
            // Within-crate re-export follow: module need not be pub to the
            // foreign crate's own code, but the *re-export* that exposed the
            // name to outsiders was already checked Public. Still require pub
            // modules on the path that outsiders traverse — for a facade
            // `pub use format::upper`, outsiders never name `format`, so the
            // chain check is on the re-export site only. Following internally
            // may walk through non-pub modules; that's fine for the target
            // item, which still must be `pub`.
            module = child;
            continue;
        }
        match lookup_cross_crate_name(foreign, &module, seg, hops) {
            CrossLookup::Module(m) => module = m,
            _ => return CrossLookup::None,
        }
    }

    let name = remaining[remaining.len() - 1];
    // Final item: must be pub (or a further pub re-export).
    let mut fn_ids = Vec::new();
    for id in foreign.lookup_in_module(&module, name) {
        if matches!(foreign.function_vis(&id), ItemVisibility::Public) {
            fn_ids.push(id);
        }
    }
    if !fn_ids.is_empty() {
        return CrossLookup::Functions(fn_ids);
    }
    lookup_cross_crate_name(foreign, &module, name, hops)
}

fn collect_glob_function_candidates(
    name: &str,
    from_module: &str,
    index: &ResolveIndex,
) -> Vec<FunctionId> {
    let mut out = Vec::new();
    let mut seen_globs = HashSet::new();
    for glob_mod in index.globs_in(from_module) {
        if !seen_globs.insert(glob_mod.clone()) {
            continue;
        }
        // Local functions in the globbed module, if visible.
        for id in index.lookup_in_module(glob_mod, name) {
            let vis = index.function_vis(&id);
            if is_visible_from(&vis, glob_mod, from_module) && !out.contains(&id) {
                out.push(id);
            }
        }
        // Re-exports in the globbed module under this name.
        for target in index.visible_reexports(glob_mod, name, from_module) {
            let site = PendingCall {
                call_path: name.into(),
                line: 0,
                byte_start: 0,
                byte_end: 0,
                enclosing_function: None,
                module_path: from_module.into(),
                owner: crate::extract::CallOwnerKind::File,
                from_macro: false,
            };
            if let TargetResolve::Functions(ids) = resolve_target_path(&target, &site, index, 0) {
                for id in ids {
                    if !out.contains(&id) {
                        out.push(id);
                    }
                }
            }
        }
    }
    out
}

fn glob_brings_type(name: &str, from_module: &str, index: &ResolveIndex) -> bool {
    for glob_mod in index.globs_in(from_module) {
        if let Some(ty) = index
            .type_by_module_name
            .get(&(glob_mod.clone(), name.to_string()))
            .and_then(|&idx| index.types.get(idx))
        {
            if is_visible_from(&ty.visibility, glob_mod, from_module) {
                return true;
            }
        }
        // Re-exported type names — if the re-export target is a type.
        for target in index.visible_reexports(glob_mod, name, from_module) {
            if index.find_type_at_path(&target).is_some() {
                return true;
            }
        }
    }
    false
}

/// Whether an item with `vis` defined in `item_module` is visible from `from`.
fn is_visible_from(vis: &ItemVisibility, item_module: &str, from: &str) -> bool {
    match vis {
        ItemVisibility::Public | ItemVisibility::Crate => true,
        ItemVisibility::Super => match parent_module(item_module) {
            Some(parent) => from == parent || is_descendant(from, &parent),
            None => false,
        },
        ItemVisibility::SelfMod | ItemVisibility::Private => {
            from == item_module || is_descendant(from, item_module)
        }
        ItemVisibility::InPath(p) => {
            // `pub(in path)`: visible in `path` and its descendants.
            // Path may be relative (`super`) or start with `crate`.
            let abs = absolutize_vis_path(p, item_module);
            from == abs || is_descendant(from, &abs)
        }
    }
}

fn absolutize_vis_path(path: &str, item_module: &str) -> String {
    let segments: Vec<&str> = path.split("::").filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return item_module.to_string();
    }
    if matches!(segments[0], "crate" | "self" | "super") {
        let mut module = item_module.to_string();
        let mut i = 0;
        while i < segments.len() {
            match segments[i] {
                "crate" => {
                    module = "crate".into();
                    i += 1;
                }
                "self" => i += 1,
                "super" => {
                    match parent_module(&module) {
                        Some(p) => module = p,
                        None => break,
                    }
                    i += 1;
                }
                _ => break,
            }
        }
        let rest: Vec<String> = segments[i..].iter().map(|s| (*s).to_string()).collect();
        if rest.is_empty() {
            module
        } else {
            join_path_owned(&module, &rest)
        }
    } else {
        path.to_string()
    }
}

fn is_descendant(child: &str, ancestor: &str) -> bool {
    child.starts_with(ancestor) && child[ancestor.len()..].starts_with("::")
}

fn unique_or_conflict(
    ids: Vec<FunctionId>,
    conflict_reason: String,
) -> Result<ResolveResult, ResolveResult> {
    match ids.as_slice() {
        [] => Err(ResolveResult::Target(unresolved("internal: empty candidate set"))),
        [only] => Ok(ResolveResult::Target(CallTarget::Resolved(only.clone()))),
        _ => Ok(ResolveResult::Target(CallTarget::Conflict(Conflict {
            candidates: ids,
            reason: conflict_reason,
        }))),
    }
}

/// Classify a path suffix that starts at a non-module segment.
fn classify_non_module_suffix(
    suffix: &[&str],
    module: &str,
    full_path: &str,
    site: &PendingCall,
    index: &ResolveIndex,
) -> ResolveResult {
    // Import binding for the first suffix segment in `module` / site module.
    let first = suffix[0];
    let bindings = {
        let in_mod = index.explicits_in(module, first);
        if !in_mod.is_empty() {
            in_mod
        } else {
            index.explicits_in(&site.module_path, first)
        }
    };
    if !bindings.is_empty() {
        match resolve_segment_binding(bindings, site, index) {
            SegmentBind::External => return ResolveResult::External,
            SegmentBind::Type(ty_path) => {
                return classify_from_type_path(&ty_path, &suffix[1..], full_path, index);
            }
            SegmentBind::Module(m) => {
                // Should have been handled by the navigation loop; treat remainder.
                if suffix.len() == 1 {
                    return ResolveResult::Target(unresolved(format!(
                        "call path `{full_path}` names a module, not a function"
                    )));
                }
                let rest = &suffix[1..];
                let func_name = rest[rest.len() - 1];
                let mut walk = m;
                for seg in &rest[..rest.len() - 1] {
                    let next = extend_module_path(&walk, seg);
                    if index.modules.contains(&next) {
                        walk = next;
                    } else {
                        return classify_non_module_suffix(
                            &rest[rest.iter().position(|s| *s == *seg).unwrap_or(0)..],
                            &walk,
                            full_path,
                            site,
                            index,
                        );
                    }
                }
                return match lookup_item_in_module(&walk, func_name, site, index, 0) {
                    TargetResolve::Functions(ids) => match ids.as_slice() {
                        [only] => ResolveResult::Target(CallTarget::Resolved(only.clone())),
                        _ if ids.len() > 1 => ResolveResult::Target(CallTarget::Conflict(
                            Conflict {
                                candidates: ids,
                                reason: format!("multiple definitions at `{walk}::{func_name}`"),
                            },
                        )),
                        _ => ResolveResult::Target(unresolved(format!(
                            "no free function `{func_name}` in `{walk}`"
                        ))),
                    },
                    TargetResolve::External => ResolveResult::External,
                    TargetResolve::Type(_) => {
                        ResolveResult::Excluded(ExclusionKind::VariantOrConstructor)
                    }
                    TargetResolve::Excluded(k) => ResolveResult::Excluded(k),
                    _ => ResolveResult::Target(unresolved(format!(
                        "no free function `{func_name}` in `{walk}`"
                    ))),
                };
            }
            _ => {}
        }
    }

    let type_idx = suffix.iter().position(|seg| {
        index.find_type(module, seg).is_some() || looks_like_type_name(seg)
    });

    let Some(type_idx) = type_idx else {
        return ResolveResult::Target(unresolved(format!(
            "path `{full_path}` uses `{first}` which is not a module in this crate \
             (from `{}`)",
            site.module_path
        )));
    };

    let type_name = suffix[type_idx];
    let after_type = &suffix[type_idx + 1..];

    if after_type.is_empty() {
        return ResolveResult::Excluded(ExclusionKind::VariantOrConstructor);
    }

    if after_type.len() == 1 {
        let member = after_type[0];
        if let Some(ty) = index.find_type(module, type_name) {
            if index.enum_has_variant(ty, member) {
                return ResolveResult::Excluded(ExclusionKind::VariantOrConstructor);
            }
            return ResolveResult::Excluded(ExclusionKind::AssociatedFunction);
        }
        if looks_like_type_name(type_name) {
            if is_prelude_variant(member) {
                return ResolveResult::Excluded(ExclusionKind::VariantOrConstructor);
            }
            return ResolveResult::Excluded(ExclusionKind::AssociatedFunction);
        }
    }

    ResolveResult::Excluded(ExclusionKind::AssociatedFunction)
}

fn is_prelude_variant(name: &str) -> bool {
    matches!(name, "Ok" | "Err" | "Some" | "None")
}

fn looks_like_type_name(name: &str) -> bool {
    if name == "Self" || is_primitive_type(name) {
        return true;
    }
    is_upper_camel_case(name)
}

fn is_primitive_type(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "char"
            | "str"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "f32"
            | "f64"
    )
}

/// True when `name` is UpperCamelCase (PascalCase) by Rust API-guideline shape.
///
/// Requires an ASCII uppercase start, only alphanumeric ASCII thereafter (no
/// `_`), and at least one lowercase letter so ALL-CAPS tokens are not treated
/// as types. Segments that fail this test fall through to `Unresolved` in
/// [`classify_non_module_suffix`] rather than a silent associated-function drop.
fn is_upper_camel_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    let mut saw_lower = false;
    for c in chars {
        if c.is_ascii_lowercase() {
            saw_lower = true;
        } else if !c.is_ascii_uppercase() && !c.is_ascii_digit() {
            return false;
        }
    }
    saw_lower
}

fn strip_segment_generics(seg: &str) -> &str {
    seg.split(['<', '\'']).next().unwrap_or(seg)
}

fn unresolved(reason: impl Into<String>) -> CallTarget {
    CallTarget::Unresolved(UnresolvedCall {
        reason: reason.into(),
    })
}

fn split_parent_name(module_path: &str) -> Option<(String, String)> {
    let (parent, name) = module_path.rsplit_once("::")?;
    Some((parent.to_string(), name.to_string()))
}

fn parent_module(module: &str) -> Option<String> {
    if module == "crate" {
        None
    } else {
        module.rsplit_once("::").map(|(p, _)| p.to_string())
    }
}

fn extend_module_path(parent: &str, name: &str) -> String {
    if parent == "crate" {
        format!("crate::{name}")
    } else {
        format!("{parent}::{name}")
    }
}

fn join_path_owned(parent: &str, parts: &[String]) -> String {
    if parts.is_empty() {
        return parent.to_string();
    }
    if parent == "crate" {
        format!("crate::{}", parts.join("::"))
    } else {
        format!("{parent}::{}", parts.join("::"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::{CallOwnerKind, Import, ItemVisibility, TypeDef, TypeKind};
    use crate::map::FunctionId;

    fn fn_at(path: &str, line: u32) -> Function {
        Function {
            id: FunctionId::from_parts("demo", path, None),
            name: path.rsplit("::").next().unwrap().to_string(),
            module_path: path.to_string(),
            line,
            call_sites: Vec::new(),
            doc_comments: Vec::new(),
        }
    }

    fn site(path: &str, module: &str) -> PendingCall {
        PendingCall {
            call_path: path.into(),
            line: 1,
            byte_start: 0,
            byte_end: 1,
            enclosing_function: None,
            module_path: module.into(),
            owner: CallOwnerKind::File,
            from_macro: false,
        }
    }

    fn index_with(
        funcs: Vec<Function>,
        modules: HashSet<String>,
        types: Vec<TypeDef>,
        imports: Vec<Import>,
    ) -> ResolveIndex {
        let vis: HashMap<FunctionId, ItemVisibility> = funcs
            .iter()
            .map(|f| (f.id.clone(), ItemVisibility::Public))
            .collect();
        ResolveIndex::build(
            funcs,
            modules,
            HashSet::new(),
            types,
            imports,
            vis,
            HashMap::new(),
        )
    }

    fn explicit(module: &str, path: &str, alias: Option<&str>) -> Import {
        Import {
            path: path.into(),
            alias: alias.map(str::to_string),
            is_glob: false,
            is_public: false,
            visibility: ItemVisibility::Private,
            module_path: module.into(),
            scope_widened: false,
        }
    }

    fn reexport(module: &str, path: &str, alias: Option<&str>) -> Import {
        Import {
            path: path.into(),
            alias: alias.map(str::to_string),
            is_glob: false,
            is_public: true,
            visibility: ItemVisibility::Public,
            module_path: module.into(),
            scope_widened: false,
        }
    }

    fn glob(module: &str, path: &str) -> Import {
        Import {
            path: path.into(),
            alias: None,
            is_glob: true,
            is_public: false,
            visibility: ItemVisibility::Private,
            module_path: module.into(),
            scope_widened: false,
        }
    }

    #[test]
    fn resolves_crate_path_and_super() {
        let funcs = vec![
            fn_at("crate::root_fn", 1),
            fn_at("crate::child::child_fn", 2),
            fn_at("crate::child::grand::deep", 3),
        ];
        let modules = HashSet::from([
            "crate".into(),
            "crate::child".into(),
            "crate::child::grand".into(),
        ]);
        let index = index_with(funcs, modules, vec![], vec![]);

        let r = resolve_call(&site("crate::child::grand::deep", "crate"), &index).unwrap();
        match r {
            ResolveResult::Target(CallTarget::Resolved(id)) => {
                assert_eq!(id.as_str(), "demo::child::grand::deep");
            }
            other => panic!("{other:?}"),
        }

        let r = resolve_call(
            &site("super::super::root_fn", "crate::child::grand"),
            &index,
        )
        .unwrap();
        match r {
            ResolveResult::Target(CallTarget::Resolved(id)) => {
                assert_eq!(id.as_str(), "demo::root_fn");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn drops_std_as_external() {
        let index = index_with(vec![], HashSet::from(["crate".into()]), vec![], vec![]);
        let r = resolve_call(&site("std::cmp::max", "crate"), &index).unwrap();
        assert!(matches!(r, ResolveResult::External));
    }

    #[test]
    fn glob_ambiguity_is_conflict() {
        let funcs = vec![fn_at("crate::shapes::get", 1), fn_at("crate::text::get", 2)];
        let modules = HashSet::from([
            "crate".into(),
            "crate::app".into(),
            "crate::shapes".into(),
            "crate::text".into(),
        ]);
        let imports = vec![
            glob("crate::app", "crate::shapes"),
            glob("crate::app", "crate::text"),
        ];
        let index = index_with(funcs, modules, vec![], imports);
        let r = resolve_call(&site("get", "crate::app"), &index).unwrap();
        match r {
            ResolveResult::Target(CallTarget::Conflict(c)) => {
                assert_eq!(c.candidates.len(), 2);
                let ids: Vec<_> = c.candidates.iter().map(|id| id.as_str()).collect();
                assert!(ids.contains(&"demo::shapes::get"));
                assert!(ids.contains(&"demo::text::get"));
                assert!(c.reason.contains("E0659"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn explicit_import_outranks_globs() {
        let funcs = vec![fn_at("crate::shapes::get", 1), fn_at("crate::text::get", 2)];
        let modules = HashSet::from([
            "crate".into(),
            "crate::app".into(),
            "crate::shapes".into(),
            "crate::text".into(),
        ]);
        let imports = vec![
            glob("crate::app", "crate::shapes"),
            glob("crate::app", "crate::text"),
            explicit("crate::app", "crate::text::get", None),
        ];
        let index = index_with(funcs, modules, vec![], imports);
        let r = resolve_call(&site("get", "crate::app"), &index).unwrap();
        match r {
            ResolveResult::Target(CallTarget::Resolved(id)) => {
                assert_eq!(id.as_str(), "demo::text::get");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn local_definition_outranks_glob() {
        let funcs = vec![
            fn_at("crate::app::describe", 1),
            fn_at("crate::shapes::describe", 2),
        ];
        let modules = HashSet::from([
            "crate".into(),
            "crate::app".into(),
            "crate::shapes".into(),
        ]);
        let imports = vec![glob("crate::app", "crate::shapes")];
        let index = index_with(funcs, modules, vec![], imports);
        let r = resolve_call(&site("describe", "crate::app"), &index).unwrap();
        match r {
            ResolveResult::Target(CallTarget::Resolved(id)) => {
                assert_eq!(id.as_str(), "demo::app::describe");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn private_fn_not_in_glob_from_sibling() {
        let shapes_get = fn_at("crate::shapes::get", 1);
        let shapes_hidden = fn_at("crate::shapes::hidden", 2);
        let mut vis = HashMap::new();
        vis.insert(shapes_get.id.clone(), ItemVisibility::Public);
        vis.insert(shapes_hidden.id.clone(), ItemVisibility::Private);
        let modules = HashSet::from([
            "crate".into(),
            "crate::app".into(),
            "crate::shapes".into(),
        ]);
        let imports = vec![glob("crate::app", "crate::shapes")];
        let index = ResolveIndex::build(
            vec![shapes_get, shapes_hidden],
            modules,
            HashSet::new(),
            vec![],
            imports,
            vis,
            HashMap::new(),
        );
        let r = resolve_call(&site("hidden", "crate::app"), &index).unwrap();
        assert!(
            matches!(r, ResolveResult::Target(CallTarget::Unresolved(_))),
            "private must not come through sibling glob: {r:?}"
        );
        let r = resolve_call(&site("get", "crate::app"), &index).unwrap();
        assert!(matches!(r, ResolveResult::Target(CallTarget::Resolved(_))));
    }

    #[test]
    fn super_glob_sees_parent_private() {
        let helper = fn_at("crate::app::helper", 1);
        let mut vis = HashMap::new();
        vis.insert(helper.id.clone(), ItemVisibility::Private);
        let modules = HashSet::from([
            "crate".into(),
            "crate::app".into(),
            "crate::app::tests".into(),
        ]);
        let imports = vec![glob("crate::app::tests", "crate::app")];
        let index = ResolveIndex::build(
            vec![helper],
            modules,
            HashSet::new(),
            vec![],
            imports,
            vis,
            HashMap::new(),
        );
        let r = resolve_call(&site("helper", "crate::app::tests"), &index).unwrap();
        match r {
            ResolveResult::Target(CallTarget::Resolved(id)) => {
                assert_eq!(id.as_str(), "demo::app::helper");
            }
            other => panic!("use super::* should see parent private: {other:?}"),
        }
    }

    #[test]
    fn follows_reexport_and_rename() {
        let funcs = vec![fn_at("crate::numbers::mean", 1), fn_at("crate::text::upper", 2)];
        let modules = HashSet::from([
            "crate".into(),
            "crate::numbers".into(),
            "crate::text".into(),
            "crate::app".into(),
        ]);
        let imports = vec![
            reexport("crate", "crate::numbers::mean", None),
            reexport("crate", "crate::text::upper", Some("shout_upper")),
            explicit("crate::app", "crate::shout_upper", None),
        ];
        let index = index_with(funcs, modules, vec![], imports);

        let r = resolve_call(&site("crate::mean", "crate::app"), &index).unwrap();
        match r {
            ResolveResult::Target(CallTarget::Resolved(id)) => {
                assert_eq!(id.as_str(), "demo::numbers::mean");
            }
            other => panic!("{other:?}"),
        }

        let r = resolve_call(&site("shout_upper", "crate::app"), &index).unwrap();
        match r {
            ResolveResult::Target(CallTarget::Resolved(id)) => {
                assert_eq!(id.as_str(), "demo::text::upper");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn imported_external_module_drops_calls() {
        let modules = HashSet::from(["crate".into()]);
        let imports = vec![explicit("crate", "std::fs", None)];
        let index = index_with(vec![], modules, vec![], imports);
        let r = resolve_call(&site("fs::write", "crate"), &index).unwrap();
        assert!(matches!(r, ResolveResult::External));
    }

    #[test]
    fn imported_type_still_drops_constructor() {
        let types = vec![TypeDef {
            name: "CallTarget".into(),
            module_path: "crate::map".into(),
            kind: TypeKind::Enum,
            variants: vec!["Resolved".into(), "Conflict".into()],
            visibility: ItemVisibility::Public,
        }];
        let modules = HashSet::from(["crate".into(), "crate::map".into(), "crate::resolve".into()]);
        let imports = vec![explicit(
            "crate::resolve",
            "crate::map::CallTarget",
            None,
        )];
        let index = index_with(vec![], modules, types, imports);
        let r = resolve_call(&site("CallTarget::Resolved", "crate::resolve"), &index).unwrap();
        assert!(matches!(
            r,
            ResolveResult::Excluded(ExclusionKind::VariantOrConstructor)
        ));
    }

    #[test]
    fn excludes_prelude_ok() {
        let index = index_with(vec![], HashSet::from(["crate".into()]), vec![], vec![]);
        let r = resolve_call(&site("Ok", "crate"), &index).unwrap();
        assert!(matches!(
            r,
            ResolveResult::Excluded(ExclusionKind::VariantOrConstructor)
        ));
    }

    #[test]
    fn excludes_local_associated_function() {
        let types = vec![TypeDef {
            name: "FunctionId".into(),
            module_path: "crate::map".into(),
            kind: TypeKind::Struct,
            variants: vec![],
            visibility: ItemVisibility::Public,
        }];
        let index = index_with(
            vec![],
            HashSet::from(["crate".into(), "crate::map".into()]),
            types,
            vec![],
        );
        let r = resolve_call(&site("FunctionId::from_parts", "crate"), &index).unwrap();
        assert!(matches!(
            r,
            ResolveResult::Excluded(ExclusionKind::AssociatedFunction)
        ));
    }

    #[test]
    fn genuine_unresolved_stays_unresolved() {
        let index = index_with(vec![], HashSet::from(["crate".into()]), vec![], vec![]);
        let r = resolve_call(&site("mystery", "crate"), &index).unwrap();
        match r {
            ResolveResult::Target(CallTarget::Unresolved(u)) => {
                assert!(u.reason.contains("mystery"), "{}", u.reason);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn upper_camel_case_requires_more_than_leading_capital() {
        assert!(is_upper_camel_case("Vec"));
        assert!(is_upper_camel_case("HashMap"));
        assert!(is_upper_camel_case("FunctionId"));
        assert!(!is_upper_camel_case("shapes"));
        assert!(!is_upper_camel_case("FOO_BAR"));
        assert!(!is_upper_camel_case("HTTP"));
        assert!(!is_upper_camel_case("A_module"));
    }

    #[test]
    fn non_upper_camel_capital_segment_is_unresolved_not_dropped() {
        // Leading capital alone must not silently classify as associated.
        let index = index_with(vec![], HashSet::from(["crate".into()]), vec![], vec![]);
        let r = resolve_call(&site("FOO_BAR::helper", "crate"), &index).unwrap();
        match r {
            ResolveResult::Target(CallTarget::Unresolved(_)) => {}
            ResolveResult::Excluded(_) => {
                panic!("must not silently drop a non-UpperCamelCase segment")
            }
            other => panic!("expected Unresolved, got {other:?}"),
        }
    }
}
