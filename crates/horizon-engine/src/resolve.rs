//! Path resolution within one crate, plus gated cross-crate edges.
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
//! bound of [`REEXPORT_HOP_LIMIT`] so a cycle cannot hang the tool. Exhausting
//! the bound yields [`CallTarget::Unresolved`] with a reason that names the
//! limit (within-crate and cross-crate).
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
//!   (a `pub fn` inside a private module is unreachable — module-chain check);
//! - **or** a `pub use` facade exposes the item under a public name (the
//!   named path need not mention private intermediate modules).
//!
//! Facade following consults the foreign crate's re-export table — including
//! `pub use` into *another* path crate that the foreign crate declares, and
//! `pub use glob::*` candidate sets. Chains A→B→C are followed with the same
//! [`REEXPORT_HOP_LIMIT`] as within-crate hops; exhausting it yields
//! `Unresolved` naming the limit so a cycle cannot hang the tool. Following a
//! facade must not bypass the dependency gate for *initial* entry: crate A
//! still cannot start a path at crate C unless A declares C. Once inside a
//! declared dependency B, B's own `pub use C::…` may be followed when B
//! declares C.
//!
//! An import that binds a **module** from a path dependency
//! (`use dep::mod;` / `use dep::mod as alias;`) is followed when that binding
//! appears as a qualified-call prefix (`mod::fn()` / `alias::fn()`). The
//! import target is recognised as a foreign module (not forced through the
//! "final segment is a function" path), and the remaining segments continue
//! inside the dependency under the same visibility rules.
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
//! applies: `use horizon_map::CallTarget` then `CallTarget::Resolved(..)` is a
//! constructor drop; `use std::fs` then `fs::write(..)` is an external drop.
//! Prefer known import / module / type facts over the leading-uppercase
//! type-name convention whenever the table can tell the truth.
//!
//! Calls through a **local binding** (a `let`-bound closure, function
//! parameter, etc.) are also dropped ([`ExclusionKind::LocalBinding`]) —
//! they are not free-function calls. Nested `fn` items inside a function body
//! remain real free functions and still resolve.
//!
//! # Function-body `use` limitation
//!
//! Imports extracted from inside a function body are attributed to the
//! enclosing module and therefore treated as module-wide, even though rustc
//! scopes them to the body. See `extract` module docs.

use crate::extract::{
    Import, ItemVisibility, MethodReceiverHint, PendingCall, TypeDef, TypeKind,
};
use horizon_map::{
    CallTarget, Conflict, Function, FunctionId, TypeConflict, TypeId, TypeTarget, UnresolvedCall,
    UnresolvedType,
};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

/// Maximum re-export / import-target hops before giving up.
///
/// Chosen high enough for real facade crates, low enough that a cycle cannot
/// hang the tool. Exhausting the bound yields `Unresolved` with a reason that
/// names the limit — both within-crate and cross-crate following.
pub const REEXPORT_HOP_LIMIT: usize = 32;

fn hop_limit_reason(path: &str) -> String {
    format!("re-export chain exceeded {REEXPORT_HOP_LIMIT} hops resolving `{path}`")
}

fn hop_limit_unresolved(path: &str) -> CallTarget {
    unresolved(hop_limit_reason(path))
}

fn is_hop_limit_reason(reason: &str) -> bool {
    reason.starts_with("re-export chain exceeded ")
}

/// Why a recognised CallExpr was deliberately omitted from the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExclusionKind {
    /// Enum variant or tuple-struct constructor — constructs a value, not a call.
    VariantOrConstructor,
    /// Associated function on a type (`Type::name`) — `impl` item, out of scope,
    /// or an untyped method call with no conflicting inherent candidates.
    AssociatedFunction,
    /// Call through a local binding (closure / `let` / parameter), not a free function.
    LocalBinding,
}

/// Result of resolving one call site.
#[derive(Debug, Clone)]
pub enum ResolveResult {
    /// Attach this target to the map's [`horizon_map::CallSite`].
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

/// One glob import / `pub use glob::*` in a module.
#[derive(Debug, Clone)]
struct GlobBinding {
    /// Absolutized globbed module path (`crate::format`, `engine_a`, …).
    target: String,
    visibility: ItemVisibility,
    /// Whether the glob is a `pub use` (visible as a cross-crate facade).
    is_reexport: bool,
}

/// Index of one path-dependency crate for cross-crate resolution.
///
/// Inserted into [`ResolveIndex::path_crates`] only when the *current* crate
/// declares that dependency — the dependency gate for initial entry. The same
/// indexes also live in [`ResolveIndex::all_crates`] (keyed by rustc name) so
/// facade `pub use other::…` chains can follow into crates the foreign crate
/// declares, without letting the consumer start a path at an undeclared sibling.
#[derive(Debug, Clone, Default)]
pub struct PathCrateIndex {
    pub rustc_name: String,
    pub modules: HashSet<String>,
    pub module_visibility: HashMap<String, ItemVisibility>,
    pub function_visibility: HashMap<FunctionId, ItemVisibility>,
    by_full_path: HashMap<String, Vec<FunctionId>>,
    by_parent_name: HashMap<(String, String), Vec<FunctionId>>,
    explicits: HashMap<(String, String), Vec<ExplicitBinding>>,
    /// Glob imports keyed by importing module path.
    globs: HashMap<String, Vec<GlobBinding>>,
    /// Import rustc name → real rustc name for this crate's path dependencies.
    /// Used when following `pub use dep::…` / `pub use dep::*` across crates.
    dep_aliases: HashMap<String, String>,
    type_by_module_name: HashMap<(String, String), usize>,
    types: Vec<TypeDef>,
}

impl PathCrateIndex {
    /// Build a foreign-crate index from that crate's extracted facts.
    ///
    /// `path_dep_aliases` maps each declared path-dependency's import rustc
    /// name (manifest rename when present) to that dependency's real rustc
    /// name. Without it, bare `pub use dep::item` targets are wrongly
    /// rewritten under `crate::` and facade following cannot start.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        rustc_name: impl Into<String>,
        functions: &[Function],
        modules: HashSet<String>,
        module_visibility: HashMap<String, ItemVisibility>,
        function_visibility: HashMap<FunctionId, ItemVisibility>,
        imports: &[Import],
        types: Vec<TypeDef>,
        path_dep_aliases: HashMap<String, String>,
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

        let path_dep_names: HashSet<String> = path_dep_aliases.keys().cloned().collect();
        let empty_externals = HashSet::new();
        let mut explicits: HashMap<(String, String), Vec<ExplicitBinding>> = HashMap::new();
        let mut globs: HashMap<String, Vec<GlobBinding>> = HashMap::new();
        for imp in imports {
            // Keep path-dep roots as `dep::…` (not `crate::dep::…`).
            let path = normalize_bare_import_path(
                &imp.path,
                &imp.module_path,
                &modules,
                &empty_externals,
                &path_dep_names,
            );
            if imp.is_glob {
                globs
                    .entry(imp.module_path.clone())
                    .or_default()
                    .push(GlobBinding {
                        target: path,
                        visibility: imp.visibility.clone(),
                        is_reexport: imp.is_public,
                    });
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
            globs,
            dep_aliases: path_dep_aliases,
            type_by_module_name,
            types,
        }
    }

    fn dep_rustc_name(&self, import_name: &str) -> Option<&str> {
        self.dep_aliases.get(import_name).map(String::as_str)
    }

    fn public_globs_in(&self, module: &str) -> impl Iterator<Item = &GlobBinding> {
        self.globs
            .get(module)
            .into_iter()
            .flatten()
            .filter(|g| g.is_reexport && matches!(g.visibility, ItemVisibility::Public))
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
    /// Path-dependency crates declared by the current crate, keyed by **import**
    /// rustc name (manifest rename when present). Absence of a workspace sibling
    /// here is the dependency gate for *initial* path entry.
    pub path_crates: HashMap<String, PathCrateIndex>,
    /// Every library crate in the repository, keyed by real rustc name.
    /// Consulted only when following a foreign crate's `pub use` into a crate
    /// that foreign crate declares — never as an initial entry point for the
    /// consumer.
    pub all_crates: HashMap<String, PathCrateIndex>,
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
    /// Local bindings (`let` / params) inside free functions, keyed by id.
    local_bindings: HashMap<FunctionId, HashSet<String>>,
}

impl ResolveIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build an index from crate-wide definitions, modules, types, and imports.
    ///
    /// `path_crates` must already be dependency-gated: only path dependencies
    /// declared by the crate being resolved (keyed by import name).
    /// `all_crates` holds every library index for facade hop following.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        functions: Vec<Function>,
        modules: HashSet<String>,
        external_crates: HashSet<String>,
        types: Vec<TypeDef>,
        imports: Vec<Import>,
        function_visibility: HashMap<FunctionId, ItemVisibility>,
        local_bindings: HashMap<FunctionId, HashSet<String>>,
        path_crates: HashMap<String, PathCrateIndex>,
        all_crates: HashMap<String, PathCrateIndex>,
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
            all_crates,
            types,
            by_parent_name,
            by_full_path,
            type_by_module_name,
            types_by_name,
            variant_in_module,
            function_visibility,
            explicits,
            globs,
            local_bindings,
        }
    }

    /// Whether `name` is bound locally inside the given free function.
    fn has_local_binding(&self, func: &FunctionId, name: &str) -> bool {
        self.local_bindings
            .get(func)
            .is_some_and(|names| names.contains(name))
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

    /// Import bindings of `name` in `module` visible from `from`.
    ///
    /// Includes private `use` items — a child module's `use super::*` must see
    /// the parent's private imports (rustc: private items are visible to
    /// descendants). Sibling globs still exclude them via [`is_visible_from`].
    fn visible_imports(&self, module: &str, name: &str, from: &str) -> Vec<String> {
        self.explicits_in(module, name)
            .iter()
            .filter(|b| is_visible_from(&b.visibility, module, from))
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

/// Resolve a type-path mention (field type, alias RHS) against `index`.
///
/// Returns `None` when the path names an external / language crate — those
/// mentions are omitted from the map (same honesty rule as dropped external
/// calls). Indexed types become [`TypeTarget`]; unknown local-looking names
/// stay [`TypeTarget::Unresolved`] with a reason.
pub fn resolve_type_mention(
    type_path: &str,
    from_module: &str,
    index: &ResolveIndex,
    id_by_full_path: &HashMap<String, TypeId>,
) -> Option<TypeTarget> {
    let segments: Vec<&str> = type_path
        .split("::")
        .map(strip_segment_generics)
        .filter(|s| !s.is_empty())
        .collect();
    if segments.is_empty() {
        return Some(TypeTarget::Unresolved(UnresolvedType {
            reason: format!("malformed type path `{type_path}`"),
        }));
    }

    if index.path_crates.contains_key(segments[0]) {
        // Type defs in path deps are indexed under the dependency's own map
        // nodes; cross-crate type edges are out of scope for this pass.
        return None;
    }
    if is_external_root(segments[0], index) {
        return None;
    }

    let mut full_paths = collect_type_full_paths(type_path, &segments, from_module, index);
    full_paths.sort();
    full_paths.dedup();

    match full_paths.as_slice() {
        [] => Some(TypeTarget::Unresolved(UnresolvedType {
            reason: format!(
                "no indexed type `{type_path}` visible from module `{from_module}`"
            ),
        })),
        [only] => match id_by_full_path.get(only) {
            Some(id) => Some(TypeTarget::Resolved(id.clone())),
            None => Some(TypeTarget::Unresolved(UnresolvedType {
                reason: format!("indexed type `{only}` has no TypeId"),
            })),
        },
        many => {
            let mut candidates = Vec::new();
            for p in many {
                if let Some(id) = id_by_full_path.get(p) {
                    candidates.push(id.clone());
                }
            }
            if candidates.is_empty() {
                return Some(TypeTarget::Unresolved(UnresolvedType {
                    reason: format!(
                        "no indexed type `{type_path}` visible from module `{from_module}`"
                    ),
                }));
            }
            if candidates.len() == 1 {
                return Some(TypeTarget::Resolved(candidates.remove(0)));
            }
            Some(TypeTarget::Conflict(TypeConflict {
                candidates,
                reason: format!(
                    "type path `{type_path}` is ambiguous from module `{from_module}`"
                ),
            }))
        }
    }
}

fn collect_type_full_paths(
    type_path: &str,
    segments: &[&str],
    from_module: &str,
    index: &ResolveIndex,
) -> Vec<String> {
    let mut out = Vec::new();

    // Absolute / already-rooted paths.
    if matches!(segments[0], "crate" | "self" | "super") {
        let abs = crate::extract::absolutize_type_path(segments, from_module);
        if index.find_type_at_path(&abs).is_some() {
            out.push(abs);
        }
        return out;
    }

    if segments.len() == 1 {
        let name = segments[0];
        if let Some(ty) = index.find_type(from_module, name) {
            out.push(ty.full_path());
        }
        // Explicit imports of this name that bind a type.
        let site = PendingCall {
            call_path: name.to_string(),
            line: 1,
            byte_start: 0,
            byte_end: 0,
            enclosing_function: None,
            module_path: from_module.to_string(),
            owner: crate::extract::CallOwnerKind::File,
            from_macro: false,
            method_receiver: None,
        };
        for binding in index.explicits_in(from_module, name) {
            match resolve_target_path(&binding.target, &site, index, 0) {
                TargetResolve::Type(p) => {
                    if !out.contains(&p) {
                        out.push(p);
                    }
                }
                _ => {}
            }
        }
        // Glob-imported types (same ambiguity rule as calls — list every hit).
        for globbed in index.globs_in(from_module) {
            if let Some(ty) = index.find_type(globbed, name) {
                let p = ty.full_path();
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        return out;
    }

    // Qualified: prefer crate-local under the importing module, then absolute
    // under `crate::`, then import of the leading segment as a module/type.
    let local = if from_module == "crate" {
        format!("crate::{}", segments.join("::"))
    } else {
        format!("{from_module}::{}", segments.join("::"))
    };
    if index.find_type_at_path(&local).is_some() {
        out.push(local);
        return out;
    }
    let under_crate = format!("crate::{}", segments.join("::"));
    if under_crate != type_path && index.find_type_at_path(&under_crate).is_some() {
        out.push(under_crate);
        return out;
    }
    if index.find_type_at_path(type_path).is_some() {
        out.push(type_path.to_string());
        return out;
    }

    // Leading segment may be an imported module: `map::Shape`.
    let site = PendingCall {
        call_path: type_path.to_string(),
        line: 1,
        byte_start: 0,
        byte_end: 0,
        enclosing_function: None,
        module_path: from_module.to_string(),
        owner: crate::extract::CallOwnerKind::File,
        from_macro: false,
            method_receiver: None,
    };
    for binding in index.explicits_in(from_module, segments[0]) {
        match resolve_target_path(&binding.target, &site, index, 0) {
            TargetResolve::Module(module) => {
                let rest = segments[1..].join("::");
                let full = extend_module_path(&module, &rest);
                // rest may be `Foo` or `inner::Foo`
                if index.find_type_at_path(&full).is_some() {
                    if !out.contains(&full) {
                        out.push(full);
                    }
                } else if segments.len() == 2 {
                    if let Some(ty) = index.find_type(&module, segments[1]) {
                        let p = ty.full_path();
                        if !out.contains(&p) {
                            out.push(p);
                        }
                    }
                }
            }
            TargetResolve::Type(p) if segments.len() == 1 => {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
            _ => {}
        }
    }

    out
}

/// Resolve a single pending call against `index`.
pub fn resolve_call(site: &PendingCall, index: &ResolveIndex) -> Result<ResolveResult> {
    if let Some(hint) = site.method_receiver.as_ref() {
        return Ok(resolve_method_call(site, hint, index));
    }

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
    if index.path_crates.contains_key(segments[0]) {
        return Ok(resolve_cross_crate_path(
            &segments[1..],
            segments[0],
            path,
            0,
            index,
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

/// Resolve `receiver.method(...)` with at most one-hop receiver typing.
fn resolve_method_call(
    site: &PendingCall,
    hint: &MethodReceiverHint,
    index: &ResolveIndex,
) -> ResolveResult {
    let method = site
        .call_path
        .strip_prefix('.')
        .unwrap_or(site.call_path.as_str());
    if method.is_empty() {
        return ResolveResult::Target(unresolved(format!(
            "malformed method call `{}`",
            site.call_path
        )));
    }

    let ty_path = match hint {
        MethodReceiverHint::TypePath(ty_path) => Some(ty_path.clone()),
        MethodReceiverHint::SelfField(field) => self_field_type(site, field, index),
        MethodReceiverHint::Unknown => None,
    };

    if let Some(ty_path) = ty_path {
        let full = if ty_path == "crate" {
            format!("crate::{method}")
        } else {
            format!("{ty_path}::{method}")
        };
        let ids = index.lookup_full(&full);
        if !ids.is_empty() {
            return match unique_or_conflict(
                ids,
                format!("multiple inherent methods `{full}`"),
            ) {
                Ok(r) => r,
                Err(r) => r,
            };
        }
        // Known receiver type but no inherent method — trait methods
        // (`.clone`, `.into_response`, …), prelude/external fields (`.as_str`
        // on `String`), and missing names share this drop.
        return ResolveResult::Excluded(ExclusionKind::AssociatedFunction);
    }

    // No certain receiver type. Do **not** Conflict on same-named inherent
    // methods elsewhere in the repo — that manufactures false problems for
    // common std names (`.as_str`, `.len`, `.clone`, …) whose real callee is
    // external. Without positive evidence the receiver is one of those types,
    // drop as associated. (Flooding Unresolved was tried and rejected.)
    ResolveResult::Excluded(ExclusionKind::AssociatedFunction)
}

/// Declared type of `self.field` on the inherent type enclosing `site`.
fn self_field_type(site: &PendingCall, field: &str, index: &ResolveIndex) -> Option<String> {
    let enc = site.enclosing_function.as_ref()?;
    let func = index.get(enc)?;
    let (parent, name) = split_parent_name(&func.module_path)?;
    if name != func.name {
        return None;
    }
    let ty = index.find_type_at_path(&parent)?;
    ty.fields
        .iter()
        .find(|(n, _)| n == field)
        .map(|(_, path)| path.clone())
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
    // Nested free functions live under the enclosing function's path and are
    // real free-function definitions — resolve them before considering locals.
    let mut nested = Vec::new();
    if let Some(enc_id) = site.enclosing_function.as_ref() {
        if let Some(enc) = index.get(enc_id) {
            nested = index.lookup_in_module(&enc.module_path, name);
        }
    }
    if !nested.is_empty() {
        return match nested.as_slice() {
            [only] => ResolveResult::Target(CallTarget::Resolved(only.clone())),
            _ => ResolveResult::Target(CallTarget::Conflict(Conflict {
                candidates: nested,
                reason: format!(
                    "multiple nested definitions of `{name}` visible in `{}`",
                    site.module_path
                ),
            })),
        };
    }

    // Closure / let / parameter binding in the enclosing function — not a
    // free-function call. Drop rather than report Unresolved.
    if let Some(enc_id) = site.enclosing_function.as_ref() {
        if index.has_local_binding(enc_id, name) {
            return ResolveResult::Excluded(ExclusionKind::LocalBinding);
        }
    }

    let locals = index.lookup_in_module(&site.module_path, name);

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
                TargetResolve::Type(_)
                | TargetResolve::Module(_)
                | TargetResolve::ForeignModule { .. } => {}
                TargetResolve::Unresolved | TargetResolve::HopLimit => {}
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
    // `Self(...)` in an inherent method is a constructor of the impl type.
    if name == "Self" {
        return ResolveResult::Excluded(ExclusionKind::VariantOrConstructor);
    }
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
    let mut saw_hop_limit = false;
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
            TargetResolve::Module(_) | TargetResolve::ForeignModule { .. } => {
                // Calling a module name is nonsense — unresolved.
            }
            TargetResolve::Excluded(kind) => exclusion = Some(kind),
            TargetResolve::HopLimit => saw_hop_limit = true,
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
    if saw_hop_limit {
        return ResolveResult::Target(hop_limit_unresolved(name));
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
                SegmentBind::ForeignModule { crate_key, module: foreign_mod } => {
                    // `use dep::mod;` / `use dep::mod as a;` then `mod::fn()` /
                    // `a::fn()` — continue inside the path dependency.
                    let after = &remaining[offset + 1..];
                    return resolve_from_foreign_module(
                        &crate_key,
                        &foreign_mod,
                        after,
                        &path,
                        0,
                        index,
                    );
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
        TargetResolve::Module(_) | TargetResolve::ForeignModule { .. } => {
            ResolveResult::Target(unresolved(format!(
                "call path `{path}` names a module, not a function"
            )))
        }
        TargetResolve::Excluded(kind) => ResolveResult::Excluded(kind),
        TargetResolve::HopLimit => {
            ResolveResult::Target(hop_limit_unresolved(&path))
        }
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

enum SegmentBind {
    Module(String),
    /// Module in a path-dependency crate (`use dep::mod` / `use dep::mod as a`).
    ForeignModule { crate_key: String, module: String },
    External,
    Type(String),
    /// Import binds a function used as a path prefix (not a free-fn call).
    /// Payload is retained for diagnostics but callers only need the kind.
    #[allow(dead_code)]
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
    let mut foreign: Option<(String, String)> = None;
    let mut ty: Option<String> = None;
    let mut fns = Vec::new();
    let mut external = false;

    for binding in bindings {
        match resolve_target_path(&binding.target, site, index, 0) {
            TargetResolve::Module(m) => {
                if module.as_ref().is_some_and(|x| x != &m) || foreign.is_some() {
                    return SegmentBind::Ambiguous;
                }
                module = Some(m);
            }
            TargetResolve::ForeignModule { crate_key, module: m } => {
                if foreign
                    .as_ref()
                    .is_some_and(|(k, mod_path)| k != &crate_key || mod_path != &m)
                    || module.is_some()
                {
                    return SegmentBind::Ambiguous;
                }
                foreign = Some((crate_key, m));
            }
            TargetResolve::External => external = true,
            TargetResolve::Type(t) => {
                if ty.as_ref().is_some_and(|x| x != &t) {
                    return SegmentBind::Ambiguous;
                }
                ty = Some(t);
            }
            TargetResolve::Functions(ids) => fns.extend(ids),
            TargetResolve::Excluded(_)
            | TargetResolve::Unresolved
            | TargetResolve::HopLimit => {}
        }
    }

    if let Some(m) = module {
        return SegmentBind::Module(m);
    }
    if let Some((crate_key, module)) = foreign {
        return SegmentBind::ForeignModule { crate_key, module };
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
            // Inherent associated function / method: `Type::name(...)`.
            let method_path = if ty_path == "crate" {
                format!("crate::{member}")
            } else {
                format!("{ty_path}::{member}")
            };
            let ids = index.lookup_full(&method_path);
            if !ids.is_empty() {
                return match unique_or_conflict(
                    ids,
                    format!("multiple inherent items `{method_path}`"),
                ) {
                    Ok(r) => r,
                    Err(r) => r,
                };
            }
            // Indexed type but no inherent item — typically a trait / derive
            // method (`Cli::parse`, `Type::default`). Same deliberate drop as
            // other out-of-scope assoc forms, not a missing free function.
            return ResolveResult::Excluded(ExclusionKind::AssociatedFunction);
        }
        if is_prelude_variant(member) {
            return ResolveResult::Excluded(ExclusionKind::VariantOrConstructor);
        }
        // Type not indexed (external / prelude) — still a deliberate drop.
        return ResolveResult::Excluded(ExclusionKind::AssociatedFunction);
    }
    ResolveResult::Excluded(ExclusionKind::AssociatedFunction)
}

/// Look up `name` in `module`: local fn, re-export, or type/module.
///
/// Visibility here is deliberately asymmetric (do not "unify"):
/// - **Re-exports** on a qualified path are filtered with
///   [`is_visible_from`] — private `use` in a foreign module must not leak.
/// - **Same-module private `use`** (`self::name` after navigating to this
///   module) is accepted without a descendant check: the call site is already
///   in `module`.
/// - **Direct within-crate edges** to local functions still do **not** filter
///   item visibility (Phase 2): a path the author wrote is worth mapping even
///   if rustc would reject it. Glob candidate sets and cross-crate reachability
///   enforce visibility elsewhere.
fn lookup_item_in_module(
    module: &str,
    name: &str,
    site: &PendingCall,
    index: &ResolveIndex,
    hops: usize,
) -> TargetResolve {
    if hops > REEXPORT_HOP_LIMIT {
        return TargetResolve::HopLimit;
    }

    let locals = index.lookup_full(&extend_module_path(module, name));
    if !locals.is_empty() {
        return TargetResolve::Functions(locals);
    }

    // Imports visible from the call site — public re-exports everywhere they
    // permit, private `use` in the defining module and its descendants
    // (so `super::imported_name` from a child works; siblings stay excluded).
    let mut targets = Vec::new();
    for binding in index.explicits_in(module, name) {
        if is_visible_from(&binding.visibility, module, &site.module_path) {
            targets.push(binding.target.clone());
        }
    }

    if targets.len() == 1 {
        return resolve_target_path(&targets[0], site, index, hops + 1);
    }
    if targets.len() > 1 {
        let mut ids = Vec::new();
        let mut saw_hop_limit = false;
        for t in &targets {
            match resolve_target_path(t, site, index, hops + 1) {
                TargetResolve::Functions(found) => {
                    for id in found {
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                }
                TargetResolve::HopLimit => saw_hop_limit = true,
                _ => {}
            }
        }
        if !ids.is_empty() {
            return TargetResolve::Functions(ids);
        }
        if saw_hop_limit {
            return TargetResolve::HopLimit;
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

enum TargetResolve {
    Functions(Vec<FunctionId>),
    /// Within-crate module path (`crate::format`).
    Module(String),
    /// Module in a path-dependency crate, reached via an import binding
    /// (`use text_engine::format` → foreign module `crate::format`).
    ForeignModule { crate_key: String, module: String },
    Type(String),
    External,
    /// Import target classified as a non-function form.
    Excluded(ExclusionKind),
    Unresolved,
    /// Re-export / import-target chain exceeded [`REEXPORT_HOP_LIMIT`].
    HopLimit,
}

fn resolve_target_path(
    target: &str,
    site: &PendingCall,
    index: &ResolveIndex,
    hops: usize,
) -> TargetResolve {
    if hops > REEXPORT_HOP_LIMIT {
        return TargetResolve::HopLimit;
    }

    let segments: Vec<&str> = target.split("::").filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return TargetResolve::Unresolved;
    }

    // Cross-crate import target (`text_engine::format::upper`, or a module
    // binding `text_engine::format` / bare `text_engine`). Initial entry still
    // requires a declared dependency of the *current* crate.
    if index.path_crates.contains_key(segments[0]) {
        // Prefer a module interpretation when every segment (including the
        // last) is a public module chain — otherwise `use dep::mod` collapses
        // to "names a module, not a function" and qualified calls through the
        // binding (`mod::fn()`) never leave the consumer crate.
        if let Some((crate_key, module)) =
            try_resolve_cross_crate_module(&segments[1..], segments[0], hops, index)
        {
            return TargetResolve::ForeignModule { crate_key, module };
        }
        return match resolve_cross_crate_path(&segments[1..], segments[0], target, hops, index)
        {
            ResolveResult::Target(CallTarget::Resolved(id)) => {
                TargetResolve::Functions(vec![id])
            }
            ResolveResult::Target(CallTarget::Conflict(c)) => {
                TargetResolve::Functions(c.candidates)
            }
            ResolveResult::External => TargetResolve::External,
            ResolveResult::Excluded(kind) => TargetResolve::Excluded(kind),
            ResolveResult::Target(CallTarget::Unresolved(u))
                if is_hop_limit_reason(&u.reason) =>
            {
                TargetResolve::HopLimit
            }
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

    // Follow imports at the target's parent (pub re-exports and private uses
    // visible from the call site — needed when `use super::name` absolutizes
    // to `parent::name` but `name` is itself an import in the parent).
    if let Some((parent, name)) = split_parent_name(target) {
        let targets = index.visible_imports(&parent, &name, &site.module_path);
        if targets.len() == 1 {
            return resolve_target_path(&targets[0], site, index, hops + 1);
        }
        if targets.len() > 1 {
            let mut ids = Vec::new();
            let mut saw_hop_limit = false;
            for t in &targets {
                match resolve_target_path(t, site, index, hops + 1) {
                    TargetResolve::Functions(found) => {
                        for id in found {
                            if !ids.contains(&id) {
                                ids.push(id);
                            }
                        }
                    }
                    TargetResolve::HopLimit => saw_hop_limit = true,
                    _ => {}
                }
            }
            if !ids.is_empty() {
                return TargetResolve::Functions(ids);
            }
            if saw_hop_limit {
                return TargetResolve::HopLimit;
            }
        }

        if index.modules.contains(target) {
            return TargetResolve::Module(target.to_string());
        }
    }

    TargetResolve::Unresolved
}

/// If `segments` (relative to `crate_key`'s root) name a public module chain
/// — including the empty path for the crate root itself — return
/// `(crate_key, module_path)`. Used so import bindings like
/// `use text_engine::format` / `use text_engine as eng` resolve to a foreign
/// module rather than "names a module, not a function".
fn try_resolve_cross_crate_module(
    segments: &[&str],
    crate_key: &str,
    hops: usize,
    index: &ResolveIndex,
) -> Option<(String, String)> {
    if hops > REEXPORT_HOP_LIMIT {
        return None;
    }
    // Confirm the crate is reachable before treating the empty path as its root.
    let _ = foreign_index(index, crate_key)?;
    if segments.is_empty() {
        return Some((crate_key.to_string(), "crate".to_string()));
    }

    let mut current_key = crate_key.to_string();
    let mut module = "crate".to_string();
    for seg in segments {
        let foreign = foreign_index(index, &current_key)?;
        match lookup_cross_crate_name(foreign, &module, seg, hops, index) {
            CrossLookup::Module(m) => module = m,
            CrossLookup::ForeignModule {
                crate_key: next_key,
                module: m,
            } => {
                current_key = next_key;
                module = m;
            }
            CrossLookup::Functions(_)
            | CrossLookup::Type
            | CrossLookup::None
            | CrossLookup::HopLimit => return None,
        }
    }
    Some((current_key, module))
}

/// Continue a qualified call inside a foreign module reached via an import
/// binding. `after` is the path suffix following the binding segment
/// (including the final function name).
fn resolve_from_foreign_module(
    crate_key: &str,
    module: &str,
    after: &[&str],
    full_path: &str,
    hops: usize,
    index: &ResolveIndex,
) -> ResolveResult {
    let mut combined: Vec<&str> = if module == "crate" {
        Vec::new()
    } else {
        module
            .strip_prefix("crate::")
            .unwrap_or(module)
            .split("::")
            .filter(|s| !s.is_empty())
            .collect()
    };
    combined.extend(after.iter().copied());
    if combined.is_empty() {
        return ResolveResult::Target(unresolved(format!(
            "call path `{full_path}` names a module, not a function"
        )));
    }
    resolve_cross_crate_path(&combined, crate_key, full_path, hops, index)
}

/// Resolve `segments` inside a foreign path-dependency crate.
///
/// `crate_key` is either an import name present in [`ResolveIndex::path_crates`]
/// (initial entry) or a real rustc name already validated via a facade hop.
///
/// Enforces cross-crate visibility: only `pub` items behind an all-`pub`
/// module chain are reachable by *naming* that path. `pub use` facades that
/// are themselves `pub` may expose an item without naming private intermediate
/// modules. Re-export hops — including into another crate the foreign crate
/// declares — share [`REEXPORT_HOP_LIMIT`] with within-crate following.
fn resolve_cross_crate_path(
    segments: &[&str],
    crate_key: &str,
    full_path: &str,
    hops: usize,
    index: &ResolveIndex,
) -> ResolveResult {
    if hops > REEXPORT_HOP_LIMIT {
        return ResolveResult::Target(hop_limit_unresolved(full_path));
    }
    let Some(foreign) = foreign_index(index, crate_key) else {
        return ResolveResult::Target(unresolved(format!(
            "no indexed path crate `{crate_key}` for `{full_path}`"
        )));
    };
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
        match lookup_cross_crate_name(foreign, &module, seg, hops, index) {
            CrossLookup::Module(m) => {
                module = m;
                continue;
            }
            CrossLookup::ForeignModule {
                crate_key: next_key,
                module: m,
            } => {
                // Path continues inside another crate reached via facade.
                let seg_pos = segments.iter().position(|s| *s == *seg).unwrap_or(0);
                let after: Vec<&str> = segments[seg_pos + 1..].to_vec();
                let m_segs: Vec<&str> = if m == "crate" {
                    Vec::new()
                } else {
                    m.strip_prefix("crate::")
                        .unwrap_or(&m)
                        .split("::")
                        .filter(|s| !s.is_empty())
                        .collect()
                };
                let mut combined: Vec<&str> = m_segs;
                combined.extend(after);
                return resolve_cross_crate_path(
                    &combined,
                    &next_key,
                    full_path,
                    hops + 1,
                    index,
                );
            }
            CrossLookup::Functions(_) => {
                return ResolveResult::Target(unresolved(format!(
                    "path `{full_path}` uses `{seg}` as a module, but it names a function"
                )));
            }
            CrossLookup::Type => {
                return ResolveResult::Excluded(ExclusionKind::AssociatedFunction);
            }
            CrossLookup::HopLimit => {
                return ResolveResult::Target(hop_limit_unresolved(full_path));
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
    match lookup_cross_crate_name(foreign, &module, name, hops, index) {
        CrossLookup::Functions(ids) => match ids.as_slice() {
            [only] => ResolveResult::Target(CallTarget::Resolved(only.clone())),
            _ if ids.len() > 1 => ResolveResult::Target(CallTarget::Conflict(Conflict {
                candidates: ids.clone(),
                reason: format!(
                    "`{name}` is ambiguous across public re-exports / definitions in `{}` \
                     (candidates: {})",
                    foreign.rustc_name,
                    ids.iter()
                        .map(|id| id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            })),
            _ => ResolveResult::Target(unresolved(format!(
                "no public free function `{name}` reachable in `{}` via `{full_path}`",
                foreign.rustc_name
            ))),
        },
        CrossLookup::Module(_) | CrossLookup::ForeignModule { .. } => {
            ResolveResult::Target(unresolved(format!(
                "path `{full_path}` names a module, not a function"
            )))
        }
        CrossLookup::Type => ResolveResult::Excluded(ExclusionKind::VariantOrConstructor),
        CrossLookup::HopLimit => ResolveResult::Target(hop_limit_unresolved(full_path)),
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

/// Look up a foreign crate index by import name (`path_crates`) or rustc name
/// (`all_crates`).
fn foreign_index<'a>(index: &'a ResolveIndex, key: &str) -> Option<&'a PathCrateIndex> {
    index
        .path_crates
        .get(key)
        .or_else(|| index.all_crates.get(key))
}

enum CrossLookup {
    Functions(Vec<FunctionId>),
    /// Module path inside the same foreign crate (`crate::format`).
    Module(String),
    /// Module reached in another crate via a facade re-export.
    ForeignModule { crate_key: String, module: String },
    Type,
    None,
    /// Re-export / facade chain exceeded [`REEXPORT_HOP_LIMIT`].
    HopLimit,
}

fn lookup_cross_crate_name(
    foreign: &PathCrateIndex,
    module: &str,
    name: &str,
    hops: usize,
    index: &ResolveIndex,
) -> CrossLookup {
    if hops > REEXPORT_HOP_LIMIT {
        return CrossLookup::HopLimit;
    }

    // Direct function definitions — must be `pub`, and parent module chain already checked.
    let mut fn_ids = Vec::new();
    for id in foreign.lookup_in_module(module, name) {
        if matches!(foreign.function_vis(&id), ItemVisibility::Public) {
            fn_ids.push(id);
        }
    }
    let full = extend_module_path(module, name);
    for id in foreign.lookup_full(&full) {
        if matches!(foreign.function_vis(&id), ItemVisibility::Public) && !fn_ids.contains(&id) {
            fn_ids.push(id);
        }
    }
    if !fn_ids.is_empty() {
        return CrossLookup::Functions(fn_ids);
    }

    // Explicit `pub use` / `pub use … as` outrank globs (same precedence as
    // within-crate import tables).
    let mut reexport_targets = Vec::new();
    for binding in foreign.explicits_in(module, name) {
        if binding.is_reexport && matches!(binding.visibility, ItemVisibility::Public) {
            reexport_targets.push(binding.target.clone());
        }
    }
    if reexport_targets.len() == 1 {
        return resolve_foreign_reexport(foreign, &reexport_targets[0], hops + 1, index);
    }
    if reexport_targets.len() > 1 {
        let mut ids = Vec::new();
        let mut saw_hop_limit = false;
        for t in &reexport_targets {
            match resolve_foreign_reexport(foreign, t, hops + 1, index) {
                CrossLookup::Functions(found) => {
                    for id in found {
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                }
                CrossLookup::HopLimit => saw_hop_limit = true,
                _ => {}
            }
        }
        if !ids.is_empty() {
            return CrossLookup::Functions(ids);
        }
        if saw_hop_limit {
            return CrossLookup::HopLimit;
        }
    }

    // Public glob re-exports (`pub use format::*;`, `pub use engine_a::*;`).
    // Two globs offering the same name → every candidate (Conflict upstream).
    match collect_foreign_glob_function_candidates(foreign, module, name, hops, index) {
        None => return CrossLookup::HopLimit,
        Some(glob_ids) if !glob_ids.is_empty() => return CrossLookup::Functions(glob_ids),
        Some(_) => {}
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

/// Follow a re-export target from inside a foreign crate.
///
/// Targets may be crate-local (`crate::format::upper`) or another path
/// dependency the *foreign* crate declares (`engine_a::upper`). The consumer's
/// dependency gate is not widened: only the foreign crate's own `dep_aliases`
/// authorize the hop into a third crate.
fn resolve_foreign_reexport(
    foreign: &PathCrateIndex,
    target: &str,
    hops: usize,
    index: &ResolveIndex,
) -> CrossLookup {
    if hops > REEXPORT_HOP_LIMIT {
        return CrossLookup::HopLimit;
    }
    let segments: Vec<&str> = target.split("::").filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return CrossLookup::None;
    }

    // Cross-crate facade: `engine_a::upper` / `alias::greet` / `grep_cli as cli`.
    if !matches!(segments[0], "crate" | "self" | "super") {
        let Some(dep_rustc) = foreign.dep_rustc_name(segments[0]) else {
            // Not a declared path dep of this foreign crate — refuse rather
            // than guess (external / typo / undeclared sibling).
            return CrossLookup::None;
        };
        let Some(other) = index.all_crates.get(dep_rustc) else {
            return CrossLookup::None;
        };
        if segments.len() == 1 {
            // `pub use engine_a;` / `pub use engine_a as alias` — binds the
            // dependency's crate root as a module path prefix.
            return CrossLookup::ForeignModule {
                crate_key: dep_rustc.to_string(),
                module: "crate".into(),
            };
        }
        // If every remaining segment is a module in the dependency, this
        // re-export binds a module (e.g. `pub use eng::cli`). Otherwise treat
        // the target as a function path.
        let mut module = "crate".to_string();
        let mut all_modules = true;
        for seg in &segments[1..] {
            let child = extend_module_path(&module, seg);
            if other.modules.contains(&child) {
                module = child;
            } else {
                all_modules = false;
                break;
            }
        }
        if all_modules {
            return CrossLookup::ForeignModule {
                crate_key: dep_rustc.to_string(),
                module,
            };
        }
        return match resolve_cross_crate_path(
            &segments[1..],
            dep_rustc,
            target,
            hops,
            index,
        ) {
            ResolveResult::Target(CallTarget::Resolved(id)) => {
                CrossLookup::Functions(vec![id])
            }
            ResolveResult::Target(CallTarget::Conflict(c)) => {
                CrossLookup::Functions(c.candidates)
            }
            ResolveResult::Target(CallTarget::Unresolved(u))
                if is_hop_limit_reason(&u.reason) =>
            {
                CrossLookup::HopLimit
            }
            ResolveResult::Target(CallTarget::Unresolved(_)) => CrossLookup::None,
            ResolveResult::External | ResolveResult::Excluded(_) => CrossLookup::None,
        };
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
            // Within-crate re-export follow: outsiders never name this module
            // when they used the facade name, so non-`pub` modules are allowed
            // on the internal walk. The final item must still be `pub`.
            module = child;
            continue;
        }
        match lookup_cross_crate_name(foreign, &module, seg, hops, index) {
            CrossLookup::Module(m) => module = m,
            CrossLookup::ForeignModule {
                crate_key,
                module: m,
            } => {
                let m_segs: Vec<&str> = if m == "crate" {
                    Vec::new()
                } else {
                    m.strip_prefix("crate::")
                        .unwrap_or(&m)
                        .split("::")
                        .filter(|s| !s.is_empty())
                        .collect()
                };
                let mut combined = m_segs;
                let seg_idx = remaining.iter().position(|s| *s == *seg).unwrap_or(0);
                combined.extend_from_slice(&remaining[seg_idx + 1..]);
                return match resolve_cross_crate_path(
                    &combined,
                    &crate_key,
                    target,
                    hops + 1,
                    index,
                ) {
                    ResolveResult::Target(CallTarget::Resolved(id)) => {
                        CrossLookup::Functions(vec![id])
                    }
                    ResolveResult::Target(CallTarget::Conflict(c)) => {
                        CrossLookup::Functions(c.candidates)
                    }
                    ResolveResult::Target(CallTarget::Unresolved(u))
                        if is_hop_limit_reason(&u.reason) =>
                    {
                        CrossLookup::HopLimit
                    }
                    _ => CrossLookup::None,
                };
            }
            CrossLookup::HopLimit => return CrossLookup::HopLimit,
            _ => return CrossLookup::None,
        }
    }

    let name = remaining[remaining.len() - 1];
    let mut fn_ids = Vec::new();
    for id in foreign.lookup_in_module(&module, name) {
        if matches!(foreign.function_vis(&id), ItemVisibility::Public) {
            fn_ids.push(id);
        }
    }
    if !fn_ids.is_empty() {
        return CrossLookup::Functions(fn_ids);
    }
    lookup_cross_crate_name(foreign, &module, name, hops, index)
}

/// Collect free-function candidates offered by `pub use …::*` in `module`.
///
/// Same discipline as within-crate glob sets: only `pub` items (and `pub`
/// re-exports) in the globbed module contribute. Distinct candidates from
/// two globs are all returned — never a guessed winner.
///
/// Returns `None` when the re-export hop bound is exhausted so callers can
/// surface a hop-limit `Unresolved` rather than a generic miss. `Some(vec)`
/// is the candidate set (possibly empty).
fn collect_foreign_glob_function_candidates(
    foreign: &PathCrateIndex,
    module: &str,
    name: &str,
    hops: usize,
    index: &ResolveIndex,
) -> Option<Vec<FunctionId>> {
    if hops > REEXPORT_HOP_LIMIT {
        return None;
    }
    let mut out = Vec::new();
    let mut saw_hop_limit = false;
    let mut seen_glob_targets = HashSet::new();
    for glob in foreign.public_globs_in(module) {
        if !seen_glob_targets.insert(glob.target.clone()) {
            continue;
        }
        let Some((crate_key, glob_mod)) =
            resolve_foreign_glob_module(foreign, &glob.target, hops, index)
        else {
            continue;
        };
        let Some(target_crate) = foreign_index(index, &crate_key) else {
            continue;
        };
        // Local pub functions in the globbed module.
        for id in target_crate.lookup_in_module(&glob_mod, name) {
            if matches!(target_crate.function_vis(&id), ItemVisibility::Public) && !out.contains(&id)
            {
                out.push(id);
            }
        }
        // Explicit pub re-exports under this name in the globbed module.
        for binding in target_crate.explicits_in(&glob_mod, name) {
            if !(binding.is_reexport && matches!(binding.visibility, ItemVisibility::Public)) {
                continue;
            }
            match resolve_foreign_reexport(target_crate, &binding.target, hops + 1, index) {
                CrossLookup::Functions(ids) => {
                    for id in ids {
                        if !out.contains(&id) {
                            out.push(id);
                        }
                    }
                }
                CrossLookup::HopLimit => saw_hop_limit = true,
                _ => {}
            }
        }
    }
    if out.is_empty() && saw_hop_limit {
        return None;
    }
    Some(out)
}

/// Resolve a glob target path to `(crate_key, module_path)` for candidate collection.
fn resolve_foreign_glob_module(
    foreign: &PathCrateIndex,
    target: &str,
    hops: usize,
    index: &ResolveIndex,
) -> Option<(String, String)> {
    // Hop exhaustion is reported by the caller (`collect_foreign_glob_…`);
    // this helper only navigates a single glob target path.
    if hops > REEXPORT_HOP_LIMIT {
        return None;
    }
    let segments: Vec<&str> = target.split("::").filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return None;
    }

    if !matches!(segments[0], "crate" | "self" | "super") {
        let dep_rustc = foreign.dep_rustc_name(segments[0])?;
        let other = index.all_crates.get(dep_rustc)?;
        if segments.len() == 1 {
            return Some((dep_rustc.to_string(), "crate".into()));
        }
        // Navigate remaining segments; only `pub` modules (named path into the dep).
        let mut module = "crate".to_string();
        for seg in &segments[1..] {
            let child = extend_module_path(&module, seg);
            if other.modules.contains(&child) {
                if !matches!(other.module_vis(&child), ItemVisibility::Public) {
                    return None;
                }
                module = child;
                continue;
            }
            return None;
        }
        return Some((dep_rustc.to_string(), module));
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
                module = parent_module(&module)?;
                i += 1;
            }
            _ => break,
        }
    }
    for seg in &segments[i..] {
        let child = extend_module_path(&module, seg);
        if foreign.modules.contains(&child) {
            // Glob is inside the same crate; module need not be pub for the
            // crate's own `pub use mod::*` to re-export its pub items.
            module = child;
        } else {
            return None;
        }
    }
    Some((foreign.rustc_name.clone(), module))
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
        // Imports in the globbed module under this name — including private
        // `use` when the importer is a descendant (`use super::*`).
        for target in index.visible_imports(glob_mod, name, from_module) {
            let site = PendingCall {
                call_path: name.into(),
                line: 0,
                byte_start: 0,
                byte_end: 0,
                enclosing_function: None,
                module_path: from_module.into(),
                owner: crate::extract::CallOwnerKind::File,
                from_macro: false,
                method_receiver: None,
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
            SegmentBind::ForeignModule { crate_key, module: foreign_mod } => {
                return resolve_from_foreign_module(
                    &crate_key,
                    &foreign_mod,
                    &suffix[1..],
                    full_path,
                    0,
                    index,
                );
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
                    TargetResolve::HopLimit => {
                        ResolveResult::Target(hop_limit_unresolved(full_path))
                    }
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
            return classify_from_type_path(&ty.full_path(), after_type, full_path, index);
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
    use horizon_map::FunctionId;

    fn fn_at(path: &str, line: u32) -> Function {
        Function {
            id: FunctionId::from_parts("demo", path, None),
            name: path.rsplit("::").next().unwrap().to_string(),
            module_path: path.to_string(),
            receiver_type: None,
            line,
            byte_start: 0,
            byte_end: 0,
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
                method_receiver: None,
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
            HashMap::new(),
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
            HashMap::new(),
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
            HashMap::new(),
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
    fn super_glob_sees_parent_private_use_of_sibling() {
        // Parent: `use crate::extract::allowlisted;` (private) + child `use super::*`.
        let allowlisted = fn_at("crate::extract::allowlisted", 1);
        let mut vis = HashMap::new();
        vis.insert(allowlisted.id.clone(), ItemVisibility::Public);
        let modules = HashSet::from([
            "crate".into(),
            "crate::extract".into(),
            "crate::modules".into(),
            "crate::modules::tests".into(),
        ]);
        let imports = vec![
            explicit("crate::modules", "crate::extract::allowlisted", None),
            glob("crate::modules::tests", "crate::modules"),
        ];
        let index = ResolveIndex::build(
            vec![allowlisted],
            modules,
            HashSet::new(),
            vec![],
            imports,
            vis,
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
        );
        let r = resolve_call(&site("allowlisted", "crate::modules::tests"), &index).unwrap();
        match r {
            ResolveResult::Target(CallTarget::Resolved(id)) => {
                assert_eq!(id.as_str(), "demo::extract::allowlisted");
            }
            other => panic!("use super::* must follow parent private use: {other:?}"),
        }
    }

    #[test]
    fn local_binding_is_dropped_not_unresolved() {
        let owner = fn_at("crate::tests::run", 1);
        let mut locals = HashMap::new();
        locals.insert(owner.id.clone(), HashSet::from(["by_name".into()]));
        let modules = HashSet::from(["crate".into(), "crate::tests".into()]);
        let index = ResolveIndex::build(
            vec![owner.clone()],
            modules,
            HashSet::new(),
            vec![],
            vec![],
            HashMap::new(),
            locals,
            HashMap::new(),
            HashMap::new(),
        );
        let mut call = site("by_name", "crate::tests");
        call.enclosing_function = Some(owner.id.clone());
        let r = resolve_call(&call, &index).unwrap();
        assert!(
            matches!(r, ResolveResult::Excluded(ExclusionKind::LocalBinding)),
            "expected LocalBinding drop, got {r:?}"
        );
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
            line: 1,
            byte_start: 0,
            byte_end: 1,
            doc_comments: vec![],
            pending_refs: vec![],
            fields: vec![],
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
    fn indexed_type_missing_assoc_is_dropped_not_unresolved() {
        let types = vec![TypeDef {
            name: "FunctionId".into(),
            module_path: "crate::map".into(),
            kind: TypeKind::Struct,
            variants: vec![],
            visibility: ItemVisibility::Public,
            line: 1,
            byte_start: 0,
            byte_end: 1,
            doc_comments: vec![],
            pending_refs: vec![],
            fields: vec![],
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
    fn within_crate_reexport_cycle_names_hop_limit() {
        // a → b → a … must not hang, and must surface the bound in the reason.
        let imports = vec![
            reexport("crate", "crate::b", Some("a")),
            reexport("crate", "crate::a", Some("b")),
        ];
        let index = index_with(vec![], HashSet::from(["crate".into()]), vec![], imports);
        let r = resolve_call(&site("a", "crate"), &index).unwrap();
        match r {
            ResolveResult::Target(CallTarget::Unresolved(u)) => {
                assert!(
                    is_hop_limit_reason(&u.reason),
                    "expected hop-limit reason, got {}",
                    u.reason
                );
                assert!(
                    u.reason.contains(&REEXPORT_HOP_LIMIT.to_string()),
                    "{}",
                    u.reason
                );
            }
            other => panic!("expected hop-limit Unresolved, got {other:?}"),
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

    /// Foreign path-dep index with a public `format` module and `upper` fn —
    /// the shape of `use horizon_engine::discover; discover::normalize_path`.
    fn path_dep_index() -> ResolveIndex {
        let upper = Function {
            id: FunctionId::from_parts("text_engine", "crate::format::upper", None),
            name: "upper".into(),
            module_path: "crate::format::upper".into(),
            receiver_type: None,
            line: 1,
            byte_start: 0,
            byte_end: 0,
            call_sites: Vec::new(),
            doc_comments: Vec::new(),
        };
        let version = Function {
            id: FunctionId::from_parts("text_engine", "crate::version", None),
            name: "version".into(),
            module_path: "crate::version".into(),
            receiver_type: None,
            line: 2,
            byte_start: 0,
            byte_end: 0,
            call_sites: Vec::new(),
            doc_comments: Vec::new(),
        };
        let mut fn_vis = HashMap::new();
        fn_vis.insert(upper.id.clone(), ItemVisibility::Public);
        fn_vis.insert(version.id.clone(), ItemVisibility::Public);
        let mut mod_vis = HashMap::new();
        mod_vis.insert("crate".into(), ItemVisibility::Public);
        mod_vis.insert("crate::format".into(), ItemVisibility::Public);
        let foreign = PathCrateIndex::build(
            "text_engine",
            &[upper, version],
            HashSet::from(["crate".into(), "crate::format".into()]),
            mod_vis,
            fn_vis,
            &[],
            vec![],
            HashMap::new(),
        );
        let mut path_crates = HashMap::new();
        path_crates.insert("text_engine".into(), foreign.clone());
        let mut all_crates = HashMap::new();
        all_crates.insert("text_engine".into(), foreign);
        ResolveIndex::build(
            vec![],
            HashSet::from(["crate".into()]),
            HashSet::new(),
            vec![],
            vec![
                explicit("crate", "text_engine::format", None),
                explicit("crate", "text_engine::format", Some("fmt")),
                explicit("crate", "text_engine", Some("eng")),
            ],
            HashMap::new(),
            HashMap::new(),
            path_crates,
            all_crates,
        )
    }

    #[test]
    fn imported_path_dep_module_resolves_qualified_call() {
        let index = path_dep_index();
        let r = resolve_call(&site("format::upper", "crate"), &index).unwrap();
        match r {
            ResolveResult::Target(CallTarget::Resolved(id)) => {
                assert_eq!(id.as_str(), "text_engine::format::upper");
            }
            other => panic!("imported module prefix must resolve: {other:?}"),
        }
    }

    #[test]
    fn renamed_imported_path_dep_module_resolves() {
        let index = path_dep_index();
        let r = resolve_call(&site("fmt::upper", "crate"), &index).unwrap();
        match r {
            ResolveResult::Target(CallTarget::Resolved(id)) => {
                assert_eq!(id.as_str(), "text_engine::format::upper");
            }
            other => panic!("renamed module import must resolve: {other:?}"),
        }
    }

    #[test]
    fn renamed_path_dep_crate_root_as_module_prefix() {
        let index = path_dep_index();
        let r = resolve_call(&site("eng::version", "crate"), &index).unwrap();
        match r {
            ResolveResult::Target(CallTarget::Resolved(id)) => {
                assert_eq!(id.as_str(), "text_engine::version");
            }
            other => panic!("renamed crate-root import must resolve: {other:?}"),
        }
    }
}
