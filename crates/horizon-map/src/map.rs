//! The function-map node types.
//!
//! Containment is a tree:
//!
//! ```text
//! Repository → Crate → Folder → File
//!                              ├── CallSite (module-level: const/static init, …)
//!                              ├── DocComment (file-level `//!` / `#![doc]`)
//!                              ├── TypeItem (struct / enum / trait / type alias)
//!                              │     └── TypeRef → TypeTarget
//!                              │                    ├── Resolved(TypeId)
//!                              │                    ├── Conflict → several TypeIds
//!                              │                    └── Unresolved
//!                              └── Function
//!                                  ├── CallSite → CallTarget
//!                                  │                 ├── Resolved(FunctionId)
//!                                  │                 ├── Conflict  → several FunctionIds
//!                                  │                 └── Unresolved
//!                                  └── DocComment (`///` / `/**` / `#[doc]`)
//! ```
//!
//! Each [`CallSite`] is a call edge that ends at either a function, a
//! [`Conflict`] (several candidates), or [`CallTarget::Unresolved`] (none).
//! Resolved targets are **references** ([`FunctionId`]) to the one canonical
//! [`Function`] node, never copies — recursion is a pointer back to the same
//! id. The map never guesses: when a site cannot be resolved to exactly one
//! definition, the edge records every candidate (or none) rather than picking
//! a winner or dropping the site.
//!
//! Calls that sit outside any free function (e.g. a `const` initialiser
//! invoking a `const fn`) attach to the owning [`File`], reusing [`CallSite`].
//! Calls inside `impl` / `trait` items stay excluded until method support is
//! deliberately extended (see remaining-work W8). Type definitions and the
//! type paths they name (fields, alias RHS) are first-class map nodes.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Stable-enough identity of a single function definition in the map.
///
/// # Format
///
/// ```text
/// {crate_key}::{module_path}
/// {crate_key}::{module_path}#L{line}   // only when disambiguating
/// ```
///
/// - **`crate_key`** — identifies the compilation unit in the map. For a
///   **library** target this is the rustc crate name (Cargo target name with
///   `-` → `_`, e.g. `text-engine` → `text_engine`). For a **binary** target
///   it is `{rustc_name}[bin]` (e.g. `horizon[bin]`). The `[bin]` marker uses
///   characters that are illegal in Rust identifiers and Cargo package names,
///   so it cannot be mistaken for a real crate name (unlike a fabricated
///   suffix such as `horizon_bin`).
/// - **`module_path`** — the function's path *as written from outside the
///   crate*: module segments plus the function name, with the leading
///   rustc `crate` segment replaced by the crate key above. Internally we
///   still record module paths with a leading `crate` (e.g.
///   `crate::shapes::get`); [`from_parts`](Self::from_parts) performs the
///   substitution so the id reads `text_engine::shapes::get`.
/// - **`#L{line}`** — appended **only** when two or more definitions share
///   the same crate-key-plus-module path (typically `#[cfg]` duplicates).
///   Every colliding definition gets the suffix so the clash is visible;
///   unique paths keep a stable id that does not churn when unrelated edits
///   shift line numbers.
///
/// # Worked examples
///
/// | Situation | Id |
/// |---|---|
/// | `fn get` in `shapes.rs` of package `text-engine` (lib) | `text_engine::shapes::get` |
/// | Same name in another file of the same crate | `text_engine::text::get` |
/// | Same name in a different package | `render_util::shapes::get` |
/// | Inline `mod inner { fn helper() {} }` inside `lib.rs` | `text_engine::inner::helper` |
/// | Crate-root `fn run` in a library | `text_engine::run` |
/// | Package `horizon` library `fn extract_facts` | `horizon::extract::extract_facts` |
/// | Same package's binary `fn main` (lib+bin name clash) | `horizon[bin]::main` |
/// | Binary target named `cli` in package `tools` | `cli[bin]::main` |
/// | `#[cfg(unix)] fn open()` at line 10 (collides) | `fs_utils::open#L10` |
/// | `#[cfg(windows)] fn open()` at line 14 (collides) | `fs_utils::open#L14` |
///
/// # Stability tradeoff
///
/// Omitting the line suffix for unique paths keeps ids stable across edits
/// that only shift line numbers. When cfg duplicates force a tie-breaker,
/// line number is still the cheapest honest disambiguator (we do not parse
/// cfg predicates). Those colliding ids churn when lines move — accepted so
/// uniqueness stays cheap.
///
/// Prefer [`FunctionId::from_parts`] over hand-concatenating strings. Assign
/// ids only after all definitions in a **crate** are known, so collisions can
/// be detected crate-wide before choosing whether to suffix. Because every id
/// is prefixed by a crate key that is unique per compilation unit (libraries
/// by rustc name; binaries by `{name}[bin]`), definitions in different crates
/// cannot collide even when a package's lib and bin share a cargo target name
/// — crate-wide collision detection remains sufficient.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FunctionId(pub String);

impl FunctionId {
    /// Build an id from its canonical parts (see type-level docs for the format).
    ///
    /// `crate_key` is [`Crate::function_id_prefix`]: the rustc identifier for
    /// libraries, or `{rustc_name}[bin]` for binaries. `module_path` must
    /// include the function name and normally starts with `crate::` (e.g.
    /// `crate::shapes::get`); the leading `crate` segment is replaced by
    /// `crate_key`. Pass `line` only when this path collides with another
    /// definition and needs a `#L{line}` tie-breaker.
    pub fn from_parts(crate_key: &str, module_path: &str, line: Option<u32>) -> Self {
        let path = match module_path.strip_prefix("crate::") {
            Some(rest) => format!("{crate_key}::{rest}"),
            None if module_path == "crate" => crate_key.to_string(),
            None => format!("{crate_key}::{module_path}"),
        };
        match line {
            Some(n) => Self(format!("{path}#L{n}")),
            None => Self(path),
        }
    }

    /// Wrap an already-formatted id string.
    ///
    /// Prefer [`from_parts`](Self::from_parts) at extract time so the format
    /// stays consistent. This constructor is for tests and deserialization
    /// helpers that already hold a complete id.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for FunctionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Aggregate counts of incomplete and deliberately excluded call edges, so
/// map health is visible without walking the tree.
///
/// `unresolved` is only for genuine free-function calls that could not be
/// resolved. Deliberate exclusions (`external_dropped`,
/// `constructor_dropped`, `associated_dropped`, `local_dropped`) are counted
/// separately and do **not** appear as sites in the tree.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapSummary {
    /// Call sites whose target is a [`Conflict`] (several candidates).
    pub conflicts: usize,
    /// Call sites with no known target ([`CallTarget::Unresolved`]).
    pub unresolved: usize,
    /// Call sites into external crates (`std`, registry deps, …) that were
    /// deliberately dropped from the map — not unresolved, not drawn.
    pub external_dropped: usize,
    /// Enum-variant / tuple-struct constructor forms (`Ok(x)`,
    /// `CallTarget::Resolved(…)`) dropped because they are not function calls.
    pub constructor_dropped: usize,
    /// Associated functions on types (`Vec::new`, `FunctionId::from_parts`)
    /// dropped because `impl` items are out of scope (same rule as methods).
    pub associated_dropped: usize,
    /// Calls through a local binding (closure / `let` / parameter) rather than
    /// a free function — dropped because they are out of the map's scope.
    #[serde(default)]
    pub local_dropped: usize,
}

impl MapSummary {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn record_conflict(&mut self) {
        self.conflicts += 1;
    }

    pub fn record_unresolved(&mut self) {
        self.unresolved += 1;
    }

    pub fn record_external_dropped(&mut self) {
        self.external_dropped += 1;
    }

    pub fn record_constructor_dropped(&mut self) {
        self.constructor_dropped += 1;
    }

    pub fn record_associated_dropped(&mut self) {
        self.associated_dropped += 1;
    }

    pub fn record_local_dropped(&mut self) {
        self.local_dropped += 1;
    }
}

/// Root of one analysis run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    /// Absolute path of the analysed repository root.
    pub root: PathBuf,
    pub crates: Vec<Crate>,
    pub summary: MapSummary,
}

impl Repository {
    /// A valid but empty map for `root` (no crates discovered, or a caller
    /// constructing a placeholder before filling nodes).
    pub fn empty(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            crates: Vec::new(),
            summary: MapSummary::empty(),
        }
    }
}

/// How a dependency relates to the analysed repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    /// `path = "..."` dependency — may be walked for further crates.
    Path,
    /// Registry / git / otherwise external — calls into it are dropped.
    External,
}

/// A crate dependency listed in the owning crate's manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    /// Package name as declared (dashes), not the rustc identifier.
    pub name: String,
    /// Manifest rename alias when the dependency is declared as
    /// `alias = { package = "real-name", … }`. This is the name used in
    /// `use alias::…` paths. Absent when the package name is the import name.
    /// Taken from `cargo metadata`'s `rename` field — never guessed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rename: Option<String>,
    pub kind: DependencyKind,
    /// Absolute path when [`DependencyKind::Path`]; absent for externals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl Dependency {
    /// Rustc identifier for the name that appears in source (`use X::…`).
    ///
    /// Uses the rename alias when present, otherwise the package name, with
    /// `-` → `_` as rustc does.
    pub fn import_rustc_name(&self) -> String {
        let raw = self.rename.as_deref().unwrap_or(self.name.as_str());
        raw.replace('-', "_")
    }
}

/// One compilation unit under the repository (one lib or bin target).
///
/// Crates do not nest like folders: binaries in `src/bin/` are separate crate
/// roots that may share a directory with the library. Each target is its own
/// [`Crate`] node with a single primary root, so folder/file trees stay
/// independent and files are never interleaved across targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Crate {
    /// Package name from the manifest (dashes preserved).
    pub name: String,
    /// Real rustc / cargo target name (`-` → `_` already applied by cargo
    /// metadata). Used for dependency matching and cross-crate path roots.
    /// A package's library and a binary may share this string (e.g. both
    /// `horizon`); [`FunctionId`] disambiguates via [`function_id_prefix`].
    pub rustc_name: String,
    /// True when this node is the package's library-like target (not a binary).
    /// Includes ordinary libs and `proc-macro` crates. Path-dependency edges
    /// resolve into the dependency's library.
    #[serde(default)]
    pub is_library: bool,
    /// Edition from cargo metadata (`"2021"`, `"2024"`, …).
    pub edition: String,
    /// Absolute `src_path` of this compilation root (exactly one for Phase 4).
    pub roots: Vec<PathBuf>,
    pub dependencies: Vec<Dependency>,
    /// Immediate child folders under the crate's source layout.
    pub folders: Vec<Folder>,
    /// Files sitting at the crate source root (e.g. `lib.rs`, `main.rs`).
    pub files: Vec<File>,
}

impl Crate {
    /// Leading segment of [`FunctionId`] for this compilation unit.
    ///
    /// Libraries use [`Self::rustc_name`] unchanged. Binaries append `[bin]`
    /// so a same-named lib+bin pair cannot collide, and so the key cannot be
    /// confused with a real crate name (`[` / `]` are illegal in identifiers
    /// and Cargo package names).
    pub fn function_id_prefix(&self) -> String {
        if self.is_library {
            self.rustc_name.clone()
        } else {
            format!("{}[bin]", self.rustc_name)
        }
    }
}

/// A directory in the containment tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    /// Absolute directory path on disk.
    pub path: PathBuf,
    pub folders: Vec<Folder>,
    pub files: Vec<File>,
}

/// A source file that belongs to a crate (discovered by the `mod` walk).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    /// Absolute path on disk.
    pub path: PathBuf,
    /// Module path of this file (e.g. `crate`, `crate::shapes`).
    /// The crate root contributes no path segment beyond `crate`.
    pub module_path: String,
    /// Hex-encoded SHA-256 of the raw file bytes at extract time.
    ///
    /// Hashed as `std::fs::read` would return them — no newline normalisation
    /// — so a saved map can detect drift when the on-disk file changes
    /// (including CRLF edits on Windows). A source panel must re-hash the path
    /// and refuse to slice on mismatch rather than silently serving text from
    /// stale offsets. Format: lowercase hex, 64 digits, no algorithm prefix
    /// (see [`crate::content_hash`]); the algorithm is SHA-256 by contract.
    ///
    /// Deserialises to `""` when absent so maps written before this field
    /// existed still load. An empty string is the unavailable sentinel — a
    /// real digest is always 64 hex digits — so a consumer must treat `""` as
    /// "cannot verify staleness" rather than as a hash of empty content.
    /// Fresh extracts always emit a real digest; the default exists only for
    /// reading old documents.
    #[serde(default)]
    pub content_hash: String,
    pub functions: Vec<Function>,
    /// Data-structure definitions in this file (struct / enum / trait /
    /// type alias). Empty when the file defines none. Deserialises to `[]`
    /// when absent so maps written before types were emitted still load.
    #[serde(default)]
    pub types: Vec<TypeItem>,
    /// Call sites outside any free function (e.g. `const` / `static`
    /// initialisers). Same [`CallSite`] type as on [`Function`]; resolved the
    /// same way. Empty when every path-form call in the file sits inside a
    /// free function. Never holds calls from `impl` / `trait` items.
    #[serde(default)]
    pub call_sites: Vec<CallSite>,
    /// Inner doc comments for this file's module (`//!`, `/*! … */`,
    /// `#![doc = "…"]`). Outer docs attach to the item they document, not
    /// here. Empty when the file has no module-level documentation.
    #[serde(default)]
    pub doc_comments: Vec<DocComment>,
}

/// Kind of documentation comment attached to a definition or module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocCommentKind {
    /// Outer doc (`///`, `/** … */`, `#[doc = "…"]`) — documents the
    /// following item.
    Outer,
    /// Inner doc (`//!`, `/*! … */`, `#![doc = "…"]`) — documents the
    /// enclosing module/crate.
    Inner,
}

/// Documentation attached to what it documents — a [`Function`] (outer) or
/// [`File`] (inner module docs).
///
/// Consecutive outer (or inner) pieces on the same owner are joined into one
/// [`DocComment`] with `\n` between pieces, matching rustdoc's treatment of
/// several `///` lines as a single logical comment. Ordinary `//` / `/* */`
/// comments are never collected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocComment {
    pub kind: DocCommentKind,
    /// Comment body with markers stripped. A single leading ASCII space after
    /// `///` / `//!` (the conventional separator) is removed from each line;
    /// further indentation is preserved so fenced code blocks keep their
    /// relative indent.
    pub text: String,
}

/// Where a call edge ends — the completion of one site in the map.
///
/// Serialized with **adjacent tagging** (`kind` + `data`). That keeps the
/// document self-describing via `kind`, while letting each payload use its
/// natural shape: a bare [`FunctionId`] string for `resolved`, and objects for
/// `conflict` / `unresolved`. An externally tagged enum (`{"resolved": ...}`)
/// is less readable for frontends; a purely internally tagged form would force
/// the resolved case into a single-field object solely to share a tag.
///
/// `conflict` and `unresolved` are deliberately distinct: several possible
/// targets is not the same situation as no known target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum CallTarget {
    /// Exactly one definition — a reference to the canonical [`Function`].
    Resolved(FunctionId),
    /// Several candidate definitions; the edge ends at this conflict node.
    Conflict(Conflict),
    /// No indexed definition matches; the edge ends without a callee.
    Unresolved(UnresolvedCall),
}

/// Several candidate callees for one call site — a first-class map node, not
/// an error.
///
/// Holds candidate function references (never file references), because
/// `#[cfg]`-duplicated definitions in the same file must remain distinct.
/// Every function knows its parent file via containment, so files are still
/// derivable. The path text as written at the call site lives on the owning
/// [`CallSite`], not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    /// Candidate definitions. Always non-empty: zero candidates is
    /// [`CallTarget::Unresolved`], not a conflict.
    pub candidates: Vec<FunctionId>,
    /// Human-readable reason (glob clash, cfg duplicates, …).
    pub reason: String,
}

/// A call site with no known target in the indexed map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedCall {
    /// Human-readable reason (unknown name, …).
    pub reason: String,
}

/// One call expression in source order under its enclosing [`Function`].
///
/// Position: 1-based `line` for human navigation, plus a UTF-8 byte range for
/// editor highlighting. `ra_ap_syntax` yields byte offsets naturally; columns
/// are omitted because they require a second pass over line text (tabs /
/// multi-byte characters) and editors can derive them from the range when the
/// file is open. Two `u32`s of range are cheap in JSON relative to that utility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallSite {
    /// Path text exactly as written at the call site.
    pub call_path: String,
    /// 1-based line of the start of the call expression.
    pub line: u32,
    /// Byte offset (UTF-8) of the start of the call expression in the file.
    pub byte_start: u32,
    /// Byte offset (UTF-8) one past the end of the call expression.
    pub byte_end: u32,
    /// How the call edge completes: function, conflict, or unresolved.
    pub target: CallTarget,
    /// True when this site was recovered from a macro argument token tree
    /// rather than a real `CallExpr` in the source CST. Omitted from JSON when
    /// false so ordinary edges keep the previous shape; consumers that care
    /// about certainty can treat a present `true` as lower-confidence recovery.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_macro: bool,
}

/// Stable identity of a type definition in the map.
///
/// Same formatting rules as [`FunctionId`]: `{crate_key}::{module_path}` with
/// an optional `#L{line}` tie-breaker when two definitions share a path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TypeId(pub String);

impl TypeId {
    /// Build an id from its canonical parts (see [`FunctionId::from_parts`]).
    pub fn from_parts(crate_key: &str, module_path: &str, line: Option<u32>) -> Self {
        let path = match module_path.strip_prefix("crate::") {
            Some(rest) => format!("{crate_key}::{rest}"),
            None if module_path == "crate" => crate_key.to_string(),
            None => format!("{crate_key}::{module_path}"),
        };
        match line {
            Some(n) => Self(format!("{path}#L{n}")),
            None => Self(path),
        }
    }

    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TypeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Kind of data-structure item emitted on a [`File`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeKind {
    Struct,
    Enum,
    Trait,
    TypeAlias,
}

/// Where a type-path mention ends — mirrors [`CallTarget`] for types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum TypeTarget {
    Resolved(TypeId),
    Conflict(TypeConflict),
    Unresolved(UnresolvedType),
}

/// Several candidate types for one type-path mention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeConflict {
    pub candidates: Vec<TypeId>,
    pub reason: String,
}

/// A type-path mention with no known indexed target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedType {
    pub reason: String,
}

/// One type path named inside a [`TypeItem`] (field type, alias RHS, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeRef {
    /// Path text exactly as written (turbofish / generics stripped to the
    /// leading path when extraction could not keep them — see engine docs).
    pub type_path: String,
    pub line: u32,
    pub byte_start: u32,
    pub byte_end: u32,
    pub target: TypeTarget,
}

/// A struct / enum / trait / type-alias definition — first-class map node.
///
/// Inherent methods and trait impls are not attached here yet; type *paths*
/// this definition names are recorded in [`type_refs`] so the Types filter
/// and a future type DAG have honest edges without guessing receivers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeItem {
    pub id: TypeId,
    pub name: String,
    pub kind: TypeKind,
    /// Full module path including the type name (e.g. `crate::shapes::Shape`).
    pub module_path: String,
    /// 1-based line of the `struct` / `enum` / `trait` / `type` keyword.
    pub line: u32,
    /// Byte range of the full item syntax node (attrs + body), same sentinel
    /// rules as [`Function::byte_start`] / [`Function::byte_end`].
    #[serde(default)]
    pub byte_start: u32,
    #[serde(default)]
    pub byte_end: u32,
    /// Variant names when [`TypeKind::Enum`]; omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<String>,
    /// Types this definition names. External / primitive paths are omitted
    /// (same honesty rule as dropped external calls); indexed or local-looking
    /// paths resolve to [`TypeTarget`].
    #[serde(default)]
    pub type_refs: Vec<TypeRef>,
    #[serde(default)]
    pub doc_comments: Vec<DocComment>,
}

/// A free function definition — the only place canonical function identity lives.
///
/// Methods and `impl` items remain out of the function list until W8 method
/// support lands; associated-function call sites stay in
/// [`MapSummary::associated_dropped`] until then.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Function {
    pub id: FunctionId,
    pub name: String,
    /// Full module path of the function (e.g. `crate::shapes::get`).
    /// Distinguishes same-named free functions at different module depths
    /// within one file (inline modules are not separate map nodes).
    pub module_path: String,
    /// 1-based line of the `fn` keyword (disambiguates cfg duplicates).
    pub line: u32,
    /// Byte offset (UTF-8) of the start of this free-function item in the file.
    ///
    /// The range is the full `ast::Fn` syntax node — outer attributes, outer
    /// docs, signature, and body — so an auditor judging call attribution
    /// across `#[cfg]`-duplicated definitions can see the attributes that
    /// distinguish them. Starting at the `fn` keyword would hide those
    /// attributes. Doc text therefore appears both in a source-panel slice of
    /// this range and separately in [`doc_comments`]; that duplication is
    /// accepted rather than dropping attributes from the range.
    ///
    /// Deserialises to `0` when absent so maps written before this field
    /// existed still load. Together with [`byte_end`], a zero-length range
    /// (`byte_start == byte_end == 0`) is the unavailable sentinel: a real
    /// function can start at byte 0, but its syntax node can never have
    /// zero length, so the pair is unambiguous. A consumer must treat that
    /// sentinel as "source slice unavailable" rather than as a valid offset.
    /// Fresh extracts always emit a real range; the default exists only for
    /// reading old documents.
    #[serde(default)]
    pub byte_start: u32,
    /// Byte offset (UTF-8) one past the end of this free-function item.
    ///
    /// See [`byte_start`] for the zero-length unavailable sentinel shared by
    /// both ends when deserialising maps that predate these fields.
    #[serde(default)]
    pub byte_end: u32,
    /// Outgoing call sites in source order. Each ends at a [`CallTarget`].
    pub call_sites: Vec<CallSite>,
    pub doc_comments: Vec<DocComment>,
}
