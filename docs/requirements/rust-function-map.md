# Rust Function Map

**Status:** agreed; Phases 1–5a in force (through doc comments); allowlisted
expression-macro call recovery and allowlisted item-macro `mod` recovery in
force; proc-macro / library-kind discovery in force; Cargo dependency renaming
supported; FunctionId uses `{name}[bin]` for binary compilation units (not a
fabricated crate name); JSON contract in
[`docs/json-output.md`](../json-output.md)
**Date:** 26 July 2026
**Branch:** `feat/ast-system`

## Summary

Horizon builds a *function map* of a Rust codebase: a navigable structure showing
every free function, the free functions it calls, and the documentation attached
to it. This document is the specification of record for that map.

The map is produced by a Rust library with a command-line front end that writes
JSON. The visual frontend is out of scope for now and will consume that JSON
later.

## Goals

- Represent every free function in a Rust repository, and every call between them.
- Resolve calls exactly where the language permits it, and mark them explicitly
  where it does not, rather than guessing or dropping them.
- Work on real repositories: multiple crates, workspaces, path dependencies,
  editions 2021 and 2024.
- Never refuse to produce output. A codebase mid-edit still yields a map.
- Emit JSON that a frontend can render without needing to understand Rust.

## Non-goals

These are deliberate exclusions, each with a cost recorded in
[`docs/deferred-scope.tex`](../deferred-scope.tex).

- **Methods and `impl` blocks.** Excluded entirely, both declarations and
  `x.foo()` call sites — **permanently out of scope** for this tool. This is
  the largest exclusion by volume: of Horizon's 1,394 call sites, **1,053 are
  method calls**. Resolving them needs type inference rather than name lookup.
  A future type-aware product would be a separate project, not an incremental
  extension of this free-function map.
- **Associated functions on types.** `Type::name(…)` forms (`Vec::new()`,
  `FunctionId::from_parts(…)`, `ast::Fn::cast(…)`) are `impl` items. They are
  excluded for the same reason as methods: permanently out of scope, absent
  from the tree, counted in `MapSummary.associated_dropped`. They must not be
  recorded as `Unresolved` (that would mis-diagnose a type as a missing
  module).
- **Enum variants and tuple-struct constructors.** Syntactically `CallExpr` in
  `ra_ap_syntax`, but they construct a value rather than invoke a free
  function (`Ok(x)`, `Some(v)`, `CallTarget::Resolved(id)`, `FunctionId(s)`).
  Absent from the tree; counted in `MapSummary.constructor_dropped`.
- **Data structures as map nodes.** The containment tree still does not hold
  `struct` / `enum` / `trait` nodes or data-structure nesting. Extraction does
  collect type *names* (and enum variants) internally so constructors and
  associated functions can be classified and dropped — that index is not part
  of the emitted map.
- **External calls as nodes.** Calls into `std` / `core` / `alloc` / `proc_macro`,
  or into a Cargo dependency whose kind is `DependencyKind::External`, are
  **deliberately excluded** from the map — not drawn as `Unresolved`, not given
  a new variant. They are counted in `MapSummary.external_dropped` so the
  exclusion is visible. This is intentional and must not be "fixed" by reading
  the never-silently-missing rule as requiring every call to appear as a site:
  external callees are outside the indexed universe by design. Phase 2
  identifies a cheap subset: a path whose first segment matches an external
  dependency's rustc name (dashes → underscores), plus the language crates
  above.   Path dependencies are not dropped by this rule (they are
  `DependencyKind::Path`); Phase 4 walks them and may resolve into them when
  the dependency gate and cross-crate visibility allow.
- **Indirect calls.** `f(s)` where `f` is a parameter, or `k(label)` where `k` is
  a closure binding. Needs dataflow analysis.
- **Functions passed as values.** `apply(&n, normalize)` records the call to
  `apply` but not the reference to `normalize`.
- **Live incremental updating.** The map is rebuilt when a codebase is uploaded,
  not continuously while typing.
- **The visual frontend.** Later.

## Data structure

Containment is the only true parent-child nesting, and it is a tree because
containment does not cycle:

```
Repository
└── Crate
    └── Folder
        └── File
            ├── CallSite  -> CallTarget   (module-level: const/static init, …)
            ├── DocComment                (file-level `//!` / `#![doc]`)
            └── Function
                ├── CallSite  -> CallTarget
                │                  ├── Resolved   -> reference to a Function
                │                  ├── Conflict   -> several candidate Functions
                │                  └── Unresolved -> no known target
                └── DocComment            (`///` / `/**` / `#[doc = …]`)
```

Each function holds a single ordered list of `CallSite`s (source order). A call
site is a call edge: it records the path text as written, its position in the
file, and a `CallTarget` saying how the edge completes. Resolved targets are
**references to the one canonical `Function` object**, never copies. This is
what makes recursion tractable: `countdown` calling `countdown` is a pointer
back to the same node, not an infinite expansion. A function called from thirty
places exists once and is referenced thirty times.

Calls that sit outside any free function — typically a `const` or `static`
initialiser invoking a `const fn` — attach to the owning **`File`**, reusing the
same `CallSite` type and the same resolver. There is no synthetic
pseudo-function. Calls inside `impl` / `trait` items remain excluded (methods
are out of scope) and must not be swept into the file-level list.

### Node types

| Node | Children | Notes |
|---|---|---|
| `Repository` | crates | one per analysis run; carries a summary of incomplete edges |
| `Crate` | folders, files | package name, rustc name, edition, one root per target, dependency list |
| `Folder` | folders, files | directory position on disk |
| `File` | functions, call sites, doc comments | path, module path; file-level call sites; inner module docs |
| `Function` | call sites, doc comments | canonical identity lives here (`FunctionId`) |
| `CallSite` | — (holds a `CallTarget`) | path text, line, byte range; source-ordered |
| `Conflict` | — (terminal) | several candidate `Function` references |
| `UnresolvedCall` | — (terminal) | no indexed definition matches |
| `DocComment` | — (terminal) | outer on functions; inner on files |

### Completeness

A branch is complete when it terminates in a resolved reference, a `Conflict`,
an `Unresolved` marker, or a doc comment. The rule is that **nothing is
silently missing** *within the indexed universe*: every free-function /
module-level path-form call site either points at a function or carries a
marker saying why it could not. **Deliberate exclusions** (external-crate
calls, enum-variant / tuple-struct constructors, associated functions on
types — see Non-goals) are absent from the tree and tallied under the matching
`MapSummary` counters (`external_dropped`, `constructor_dropped`,
`associated_dropped`), not forced into `Unresolved`. `Unresolved` means only:
this is a free-function call we genuinely could not resolve.

### Call targets: conflict versus unresolved

A call edge ends in one of three ways. **Several possible targets** and **no
known target** mean different things, so they are not collapsed into one kind:

- **Resolved.** Exactly one definition. The edge holds a `FunctionId`.
- **Conflict.** A first-class node, not an error. Two or more candidate
  `Function` references and a reason — for example two glob imports supplying
  the same name:

```rust
use crate::shapes::*;
use crate::text::*;
get(3)                  // -> Conflict [text::get, shapes::get]
```

  Also `#[cfg]`-duplicated definitions, where one path has two real definitions
  and which is live depends on a build configuration nobody has chosen.

- **Unresolved.** A name not defined anywhere indexed. This is not a conflict
  between alternatives; the edge simply has no callee.

`Conflict` holds *function* references rather than file references, because
`#[cfg(unix)] fn open()` and `#[cfg(windows)] fn open()` can sit in the same file
— storing files would collapse them and lose the distinction. Every function
knows its parent file, so the file list is still derivable. The path text as
written at the call site lives on the `CallSite`, not on the conflict.

Each `CallSite` records a 1-based line plus a UTF-8 byte range so a frontend or
editor can navigate to and highlight the call. Columns are omitted: byte
offsets come naturally from the parser, and editors derive display columns from
the open file.

### Function identity

`FunctionId` uniquely names one definition across the whole repository:

```text
{crate_key}::{module_path}
{crate_key}::{module_path}#L{line}   // only when disambiguating
```

`crate_key` identifies the compilation unit:

| Target | `crate_key` | Example |
|---|---|---|
| Library | rustc target name (`-` → `_`) | `horizon::extract::extract_facts` |
| Binary | `{rustc_target_name}[bin]` | `horizon[bin]::main` |

The `[bin]` marker uses characters that are **illegal in Rust identifiers and
Cargo package names**, so it cannot be confused with a real crate name. An
earlier approach that rewrote the binary's name to `horizon_bin` is rejected:
that string looks like a real crate and could collide with a package actually
named `horizon-bin` / `horizon_bin`. `Crate.rustc_name` always stores the
**real** cargo target name (both lib and bin may be `horizon`); only the
FunctionId prefix carries the `[bin]` disambiguator.

The module path is the function's path as written from outside the crate —
module segments plus the function name — so a definition recorded internally
as `crate::shapes::get` becomes `text_engine::shapes::get`. The leading rustc
`crate` segment is replaced by the crate key rather than prepended, so the id
does not name the crate twice.

The `#L{line}` suffix is appended **only** when two or more definitions share
the same crate-key-plus-module path (typically `#[cfg]` duplicates). Every
colliding definition gets the suffix (e.g. `fs_utils::open#L10` versus
`fs_utils::open#L14`); unique paths keep a clean id. That keeps ordinary ids
stable when unrelated edits shift line numbers, while still disambiguating
without encoding cfg predicates. Ids are assigned after **all definitions in
the crate** are collected so collisions are detected crate-wide (ids are
crate-scoped). Because every id is prefixed by a crate key that is unique per
compilation unit, definitions in different crates cannot collide in one
repository map — including a package's same-named lib and bin.

Internally, `Function.module_path` keeps a leading `crate` segment
(`crate::shapes::get`). `FunctionId` substitutes the crate key for that
segment (`text_engine::shapes::get` / `horizon[bin]::main`). Preserve that
spelling distinction deliberately.

The run also emits summary counts of conflicts, unresolved sites,
external-dropped sites, constructor-dropped sites, and associated-dropped
sites, so map health is visible without walking the tree — and so "excluded
by design" is never conflated with "could not resolve".

## Pipeline

1. **Discover crates.** Find every `Cargo.toml` under the given root. Run
   `cargo metadata --no-deps --format-version 1 --offline` per manifest.
   When analysing the Horizon repository itself, skip `tests/fixtures/` —
   those crates are deliberately broken/ambiguous fixtures, not real source.
   Emit one [`Crate`](../../src/map.rs) node per **mapped** target:
   library-like kinds (`lib`, `rlib`, `dylib`, `cdylib`, `staticlib`,
   `proc-macro`) and `bin`. **Deliberately excluded** (not an accident):
   `example`, `test`, `bench` (secondary surfaces that inflate maps and hit
   the `[dev-dependencies]` blind spot), and `custom-build` (`build.rs`).
   Follow `path` dependencies by recursing into each dependency directory and
   running metadata there (`--no-deps` lists only workspace members; see
   Decisions). Honour manifest rename aliases
   (`alias = { package = "real-name", path = "…" }`) for import roots and the
   dependency gate. Registry / git dependencies are never walked.
2. **Read crate facts.** One absolute `src_path` root per target, real
   `edition`, rustc target name, and dependencies split by whether they carry
   a `path` key. Same-package binaries get an implicit path dependency on
   their package's library (matching Cargo).
3. **Walk the module tree.** From each target's root, follow `mod`
   declarations transitively (declaration-driven, never a directory glob).
   Honour `#[path]`. Inline `mod name { ... }` adds a module-path level but
   no file. Record each module's visibility (`pub mod` vs `mod`, …). Missing
   module files are reported and skipped. Cycles / repeated declarations
   terminate that branch — **first declaration wins**, so two `#[cfg]` twins
   that declare the same module name with different `#[path]` targets keep
   only the first file (one module path → one file; real limitation).
   Undeclared files (e.g. `orphan.rs`) do not appear. `mod` decls inside
   **allowlisted** item macros (`cfg_if!`, names starting `cfg_`) are
   recovered by re-parsing the **invocation** token tree (depth ≤ 8); all
   `cfg_if!` branches are **unioned** (same reasoning as keeping
   `#[cfg]`-duplicated functions — the tool does not choose a build
   configuration). Macros whose modules live only in a `macro_rules!`
   **definition** body (e.g. serde `crate_root!()`) stay closed.
4. **Parse.** `ra_ap_syntax`, at the crate's real edition.
5. **Extract.** Free function definitions (with visibility), local type names
   (struct / enum / trait / type alias) with enum variants, the import table
   from `use` / `pub use` trees, path-form call sites (including module-level
   calls), and doc comments (`///` / `//!` / block forms / `#[doc = "…"]`).
   Macro-hidden calls inside **allowlisted** expression macros (`format!`,
   `println!`, `assert_eq!`, `vec!`, …) are recovered by re-parsing the
   token-tree interior and walking real `CallExpr` nodes; see Decisions.
   Assign `FunctionId`s crate-wide after extraction. Pass 1 extracts every
   discovered crate before any resolve, so callees in path deps exist.
6. **Resolve (Phase 4).** Same-module unqualified names; module-qualified,
   `crate::`, `self::`, and `super::` paths; the import table (explicit `use`,
   aliases, globs, re-exports) with rustc precedence; **and** paths into
   declared path-dependency crates. Drop external roots; drop constructors
   and associated-function forms (see Non-goals). Within-crate direct edges
   still do **not** filter visibility; glob candidate sets do; **cross-crate**
   reachability requires `pub` items behind an all-`pub` module chain (see
   Decisions).
7. **Build the map.** Repository → crates → folders → files → functions, with
   call sites in source order on functions and on files. Each compilation
   unit builds its folder tree from its own root independently — a library
   and its `src/bin/` binaries sharing a directory do not duplicate or
   interleave files. Folder hierarchy is derived only from paths the module
   walk found (`Crate.files` = source-root files; deeper files under
   `Folder`). Each file appears exactly once **per crate node**.
8. **Emit.** JSON from the CLI; the same structure available as a library API.

### Phase 3 resolves

| Call form | Phase 3 |
|---|---|
| Unqualified name defined in the same module (or nested `fn` in the enclosing function) | Resolve / Conflict |
| `module::…::fn`, `crate::…`, `self::…`, `super::…` (+ multi-`super`) against the module tree | Resolve / Conflict / Unresolved |
| Bare name or path prefix via explicit `use` / alias / `self` in a brace list | Resolve through the binding (then re-export hops) |
| Bare name supplied by exactly one glob | Resolve |
| Bare name supplied by two or more globs, no explicit / local winner | `Conflict` (rustc `E0659`) |
| Facade / renamed `pub use` (`crate::mean`, `shout_upper`) | Resolve by following re-exports (bounded hops) |
| `use text_engine::format::upper` / `text_engine::…` at the call site, when `text-engine` is a declared path dependency | Resolve cross-crate (visibility-filtered) |
| Cross-crate facade: `facade::upper` where facade has `pub use eng::upper` (plain / renamed / multi-crate chain / `pub use eng::*`) | Resolve to the defining `FunctionId` (hop-bounded); two foreign globs for the same name → `Conflict` |
| `pub(crate) use` at a foreign crate root, or a path that *names* a private foreign module | `Unresolved` (not reachable from outside) |
| Path into a workspace sibling **not** declared as a dependency | `Unresolved` (dependency gate — never a wrong edge); facade following must not bypass this for *initial* entry |
| `use std::fs` then `fs::write` (import roots in an external crate) | Dropped (`external_dropped`) |
| `std::…` / external dependency root written at the call site | Dropped (`external_dropped`) |
| Prelude / local enum variants and tuple-struct constructors (`Ok`, `Enum::V`, `Tuple(…)`) | Dropped (`constructor_dropped`) |
| Associated functions on types (`Vec::new`, `Local::assoc`, `u32::from`, `ast::Fn::cast`) — including after `use` binds the type name | Dropped (`associated_dropped`) |
| Methods / `impl` call sites (including calls *inside* `impl` / `trait` bodies) | Out of scope (absent) |

### Import forms (Phase 3)

Fully supported (flattened from the `UseTree` AST, not by string-splitting):

| Form | Example |
|---|---|
| Plain path | `use crate::text::upper;` |
| Brace list | `use crate::{shapes, text};` |
| Nested braces | `use crate::text::{case::snake, upper};` |
| Alias | `use crate::text::upper as shout;` |
| `self` in a brace list | `use crate::text::{self, upper};` — binds module `text` and function `upper` |
| Glob | `use crate::text::*;` |
| Re-export | `pub use numbers::mean;` |
| Renamed re-export | `pub use text::upper as shout_upper;` |
| Glob re-export | `pub use format::*;` (including across path crates) |
| `extern crate` rename facade | `pub extern crate grep_cli as cli;` — same as binding the dep crate under `cli` |
| Relative roots | `use self::…`, `use super::…`, `use crate::…` |
| External roots | `use std::fs;`, `use serde::Serialize;` |
| Discard | `use crate::beta::Marker as _;` — no binding |

Partial / deferred:

| Form | Behaviour |
|---|---|
| Bare first segment (`use numbers::mean`) | Normalized at index-build time: local child module of the importing module wins over an external crate of the same name (edition 2018+); otherwise kept as an external root when the name is a known dependency / language crate |
| `pub(in path)` on globbed items | Best-effort path check; not a full rustc privacy lattice |
| `use` inside a function body | Recorded against the enclosing **module** with `scope_widened = true` — see Decisions |

Not handled (remain absent or unresolved as appropriate):

| Form | Notes |
|---|---|
| Macro-expanded `use` | Macros are still opaque token trees |
| `$crate` in macros | Macro hygiene out of scope |

## Inputs and outputs

- **Input:** a path to a repository.
- **Output:** JSON describing the map, written by the CLI.
- **Library:** the same structure, exposed for programmatic use.

## Decisions, with reasoning

### No compile gate; build the tree always

Earlier this was "only map code that compiles," which would have required
`cargo check --message-format=json`, `rust-analyzer diagnostics`, or
`rust-analyzer unresolved-references`. That was replaced with: build the tree
always, and mark conflicts.

*Why:* it removes the toolchain dependency entirely (cargo is still needed for
crate discovery, which is a different job), it means the tool works on
in-progress code, and it restores the error tolerance that motivated choosing
`ra_ap_syntax` over `syn` in the first place. Problems become visible in the map
instead of blocking it.

### `ra_ap_syntax` as the parser

Chosen for error tolerance, comment preservation, and incremental parsing. `syn`
discards comments and cannot parse broken code. Measured extraction throughput on
the spike was roughly 1.2 MB/s single-core.

### `cargo metadata` for crate discovery, with `--no-deps --offline`

Verified: it performs no compilation and took **322 ms** on Horizon. It resolves
workspace globs, `[patch]`/`[replace]`, and relative dependency paths, which is
work not worth reimplementing.

Verified lockfile behaviour, which matters because we analyse repositories we do
not own:

| Invocation | Writes `Cargo.lock`? |
|---|---|
| `cargo metadata --no-deps` | no |
| `cargo metadata` | **yes — creates it** |
| `cargo metadata --locked` | no; errors instead |

**Phase 4 keeps `--no-deps`.** Dropping it would let metadata create or update
`Cargo.lock` in the target repository — forbidden for a read-only analyser.
`--no-deps` lists only workspace members, so path dependencies that are not
already members are found by recursing into each dependency's `path` directory
and running metadata there. Nested path deps under a workspace root (e.g.
`path-dependency/engine`) may already appear as workspace members; recursion
covers path deps outside that set.

### File discovery by module walk, not directory glob

The previous approach globbed directories. That is wrong, verified against the
compiler in four ways:

| Case | Module | File |
|---|---|---|
| Ordinary | `text` | `text.rs` |
| Inline module | `text::case` | none — written inside `text.rs` |
| Undeclared file | none | `orphan.rs` — never compiled |
| `#[path]` | `tidy_name` | `renamed_on_disk.rs` |
| Crate root | the root itself | `lib.rs` — contributes no path segment |

A file containing pure syntactic garbage compiles fine as long as no `mod`
declaration names it, so globbing would index files that are not part of the
crate at all.

### Crate nodes above folders

Crates do not nest like folders. `C:\Users\kouat\Research\Cellular Automata`
holds two independent crates with no manifest at the repository root, and
`src/bin/*.rs` files are separate crate roots sharing a directory with the
library. `crate::` is crate-relative, so the same path text means different
things in different crates.

*Consequence:* the dependency graph becomes a hard constraint on resolution. If
crate A does not depend on crate B, no name in A can resolve into B — a far more
reliable filter than name uniqueness.

### Calls reference functions, not files

A file can hold two functions of the same name at different module depths, so
file-plus-name is not a unique identity. Referencing the function object is
exact, and the file remains reachable by walking up the containment tree.

### Doc comments

Documentation attached to what it documents:

| Form | Supported | Attachment |
|---|---|---|
| `///` line outer | yes | following free function (several consecutive lines → one `DocComment`) |
| `//!` line inner | yes | enclosing file / module (`File.doc_comments`) |
| `/** … */` / `/*! … */` block | yes | same as line forms |
| `#[doc = "…"]` / `#![doc = "…"]` | yes | equivalent to `///` / `//!`; common in generated code |
| Ordinary `//` / `/* */` | no | not documentation |
| Other `#[doc(…)]` (e.g. `hidden`) | no | metadata, not doc text |

`ra_ap_syntax` keeps outer docs and attributes as children of the item, so
`/// docs` then `#[inline]` then `fn foo` still attaches to `foo`. Whitespace:
strip exactly one leading ASCII space per line after the marker (the
conventional `/// ` separator); preserve further indentation for fenced code
blocks. Consecutive pieces of the same kind on one owner are joined with `\n`
into a single `DocComment` — matching rustdoc's one-logical-comment rule.

### Macro-hidden calls (allowlisted reparse)

The parser leaves macro arguments as unstructured token trees, so `mean(v)`
inside `format!("{}", mean(v))` is invisible to a normal `CallExpr` walk.
Recovery opens only an **allowlist** of std/core macros whose arguments are
expression positions (`format!` / `print!` / `println!` / `eprint!` /
`eprintln!` / `write!` / `writeln!` / `assert!` / `assert_eq!` / `assert_ne!` /
`debug_assert*` / `vec!` / `dbg!` / `panic!` / `unreachable!` /
`unimplemented!` / `todo!`). For each, the token-tree interior is re-parsed as
Rust (parenthesised args become arguments of a synthetic callee; `[…]` becomes
an array literal) and path-form `CallExpr`s are collected with ranges mapped
back into the original file. Nested allowlisted macros found after reparse are
opened recursively (depth-bounded). Recovered sites resolve through the normal
path (`Conflict` / `Unresolved` unchanged) and set `CallSite.from_macro`
(omitted from JSON when false).

**Deliberately not opened** (prefer a miss over a fabricated edge):

| Construct | Why omitted |
|---|---|
| `matches!` / pattern-position macros | `Some(x)` is a pattern, not a call |
| `stringify!` / `concat!` / `include*!` / `env!` / `option_env!` | Tokens are not evaluated as calls |
| `cfg!` | Cfg predicates, not calls |
| `quote!` / similar codegen macros | Describe code to emit, not to run |
| `macro_rules!` / `macro` definition bodies | Templates / metavariables, not call sites |
| User-defined macros | Argument structure unknown — stay opaque |
| Attribute / derive token trees | Not expression `MacroCall`s |

Tuple-struct / enum constructors recovered inside allowlisted macros still hit
the existing classifier and are dropped (`constructor_dropped`), not emitted as
free-function edges.

### Macro-hidden `mod` declarations (allowlisted item reparse)

Item-pasting macros hide `mod` declarations the same way expression macros
hide calls. Recovery opens only:

| Allowlist | Reasoning |
|---|---|
| `cfg_if!` | Canonical feature-gate crate; **all branches are unioned** because the tool does not choose a build configuration (same spirit as keeping `#[cfg]`-duplicated functions) |
| Names starting with `cfg_` | Tokio-style item macros (`cfg_fs!`, `cfg_rt!`, …) that paste `$($item:item)*` |

Nested allowlisted macros are opened up to depth 8. Missing files on a branch
are skipped without panicking. `stringify!` and user macros stay closed.

**Not recovered — serde `crate_root!()`:** the real modules live in a
`macro_rules!` **definition** body; the invocation is an empty `crate_root!()`.
Expanding definition bodies is a different and riskier problem than re-parsing
an invocation’s token tree. Prefer a missing subtree over a fabricated one.
Measured consequence: serde’s main library stays thin even after proc-macro
crates are discovered (see [`docs/scale-validation.md`](../scale-validation.md)).

### Build our own resolver rather than adopt SCIP

`rust-analyzer scip .` produces a full semantic index with real name resolution
and type inference — measured at 73 s / 2.1 GB on ruff with **81% of references
resolved**, including methods. The hand-built approach resolves roughly 55% of
path-form calls with methods excluded.

*Why build our own anyway:* decided for now. SCIP needs the rust-analyzer
component installed, produces a batch snapshot, and its handling of comments as
first-class nodes is unverified. Recorded in the deferred-scope document so the
tradeoff is not forgotten.

### Visibility: direct edges vs glob candidate sets vs cross-crate

`pub`, `pub(crate)`, and private items affect what rustc would accept. Three
distinct policies:

**Direct within-crate call edges** (Phase 2 rule, retained): the tool **does
not** filter by visibility. A path the author wrote still appears even when the
compiler would reject it (`E0603`, etc.). Mid-edit code must stay mappable; the
never-guess rule forbids picking among candidates — it does not require
pretending a written call does not exist.

**Glob candidate sets** (Phase 3): a glob only brings in names that are
*visible from the importing module* through the globbed module. Including
private items would manufacture `E0659` conflicts rustc would never report.
Rules used:

| Visibility | Visible from |
|---|---|
| `pub` / `pub(crate)` | Anywhere in the analysed crate |
| `pub(super)` | Parent of the defining module and its descendants |
| `pub(self)` / private | Defining module and its descendants (so `use super::*` in a child still sees the parent's private helpers) |
| `pub(in path)` | Best-effort: the named path and its descendants |

Public re-exports inside a globbed module contribute their local names when the
re-export itself is visible from the importer.

**Cross-crate reachability** (Phase 4): visibility determines whether a
candidate exists at all. Crate A may resolve into crate B only when A declares
B as a path dependency (dependency gate). Then:

1. Every module segment on the path from B's crate root must be `pub`
   (not `pub(crate)`, not private). A `pub fn` inside a private module is
   unreachable *by naming that module*
   (`text_engine::secret::hidden`).
2. The target item itself must be `pub` (`pub(crate)` / private are not
   reachable from another crate — `text_engine::format::trim_inner`).
3. `pub use` facades that are themselves `pub` may expose an item without
   naming private intermediate modules at the call site — including
   `pub use private_mod::item` (Rust-correct: the *re-export name* is public
   even when the module is not). `pub(crate) use` is not visible outside.
4. Facade targets may live in *another* path crate that B declares
   (`pub use eng::upper`, renamed forms, `pub use eng::*`, and
   `pub extern crate eng as name`). Following that hop does **not** require
   A to declare `eng`; it does require B to declare `eng`. A's initial entry
   still needs A→B. Two foreign glob re-exports offering the same name →
   `Conflict` (never a pick).
5. Re-export hops — within or across crates — stay bounded by
   [`REEXPORT_HOP_LIMIT`](../../src/resolve.rs) (32). Exhausting the bound
   yields `Unresolved` with a reason that names the limit, so a cycle
   (A re-exports B which re-exports A) cannot hang the tool. Each hop
   increments the same counter; there is no separate unbounded walk.

*Why follow across crates at all:* published Rust libraries conventionally
present a flat crate-root API and treat internal crates / modules as private.
Leaving facade paths `Unresolved` was the largest remaining resolution gap
on multi-crate corpora (ripgrep's `grep::cli::…` barrels, which use
`pub extern crate grep_cli as cli`). Following only `pub` re-exports /
`pub extern crate` renames, gated by the foreign crate's own dependency list,
recovers those edges without inventing a path into an undeclared sibling.

Keep the asymmetry deliberate: within-crate direct edges map what the author
wrote; cross-crate edges map what is actually nameable from outside.

### Import precedence (Phase 3)

Matches rustc, verified against `glob-ambiguity` (`E0659`) and `glob-resolved`:

1. A **locally defined** free function outranks any import (including globs).
2. An **explicit** `use` outranks any glob import.
3. **Two globs** offering the same name at the point of use → `Conflict`
   naming every candidate (never a guessed winner).
4. Explicit `use` colliding with a local definition of the same name is rustc
   `E0255`. Represented as a `Conflict` naming both the local definition(s)
   and the import target(s) — the tool never picks a winner.

### Re-export hop bound

Re-export / import-target chains are followed at most **32** hops
(`resolve::REEXPORT_HOP_LIMIT`). Exhausting the bound yields `Unresolved`
with a reason that names the limit, so a cycle cannot hang the tool.

### Function-body `use` scoping (known limitation)

In Rust, a `use` inside a function body is scoped to that body. Extraction
still attributes such imports to the enclosing **module** and sets
`Import::scope_widened`. Resolution therefore treats them as module-wide.
This is wider than rustc; the flag exists so the limitation is never silent.
Module-level `use` (including inside inline `mod` blocks) is attributed to
that module correctly.

### File-level call sites (no synthetic function)

Module-level calls (`const MAX: usize = compute_max();`) attach to the `File`
node. Inventing a pseudo-function was rejected: it would pollute the function
list and invent an identity the source does not have.

### External calls are dropped — deliberate exclusion

See Non-goals. The never-silently-missing rule applies to calls in the indexed
universe; external callees are outside it. Do not "restore" them as
`Unresolved`.

### Constructors and associated functions are dropped — deliberate exclusion

`CallExpr` is broader than "free function call." Enum variants, tuple-struct
constructors, and `Type::associated_fn(…)` are recognised and omitted from the
map (summary counters only). Rationale: variants/constructors are not
functions; associated functions are `impl` items and share the permanent
methods exclusion.

**Classification:**

| Signal | Certainty |
|---|---|
| Prelude names `Ok` / `Err` / `Some` / `None` (when no same-module free function shadows them) | Certain |
| Path segment matches a collected local type or enum variant | Certain |
| Primitive type segment (`u32`, `str`, …) or `Self` | Certain |
| Unknown segment matches **strict UpperCamelCase** (ASCII uppercase start, alphanumeric only, at least one lowercase — not `ALL_CAPS`, not `snake_Case`) | Convention — strong, not guaranteed |

Segments that fail the UpperCamelCase test are **not** silently dropped: they
fall through to ordinary resolution and typically surface as `Unresolved`.
If a **module** is UpperCamelCase *and* is absent from the crate module tree,
a call through that name may still be mis-classified as an
associated-function / constructor path and dropped. Modules that *are* in the
tree are navigated as modules regardless of capitalisation. Do not "fix"
genuine associated / constructor exclusions by recording them as
`Unresolved`.

## What gets deleted

A full overhaul. Everything that is code, used by code, or generated by code:

| Path | What | Recoverable |
|---|---|---|
| [`src/`](../../src) — `lib.rs` 1,295 lines, `lang.rs` 1,231, `main.rs` 107, `bin/server.rs` 312 | the old analyser | yes, tracked at `3a72a2c` |
| [`web/`](../../web) — `index.dc.html`, `support.js`, 163 KB | frontend served by `server.rs` | yes, tracked |
| `target/` — 1,480 files | build output | regenerable |
| `Cargo.lock` | generated | regenerable |
| `Cargo.toml` | rewritten, not deleted | tracked |

**Not deleted:** `docs/`, `Theory/`, `prompt.md`, `LICENSE`, `README.md`. The
first three are untracked, so git could not restore them, and none are code.

## Edge cases and constraints

Handled:

- Package name differs from folder name; dashes become underscores in code
  (`text-engine` → `text_engine`).
- A binary in `src/bin/` is a separate crate node reaching its own library by
  package name, exactly as an external dependency would. Folder trees for lib
  and bin are built independently from each root (no interleaved files).
- Visibility is a conjunction along the whole path for *rustc*. Direct
  within-crate call edges still do not filter on it; glob candidate sets do;
  cross-crate edges enforce `pub` + module-chain visibility (see Decisions).
- Dependency gate: crate A resolves into crate B only when A declares B as a
  path dependency (workspace siblings alone are not enough).
- `self::`, `super::`, and `crate::` are scope keywords, not module names.
- Import aliases at both the `use` site and the `pub use` re-export site.
- Once an import binds a name, Phase 2b exclusions still apply: an imported
  type used as `Type::…` is still a constructor / associated-function drop;
  an imported external module root still drops calls into it. Prefer known
  import / module / type facts over the UpperCamelCase convention whenever
  the table can tell the truth.

Known limitations to accept:

- **Modules are not nodes.** The spine is folders and files, so inline modules
  vanish. Two functions at different module depths in one file are distinguished
  only by their recorded path string.
- **Cfg-twinned `#[path]` modules — first wins.** Two `#[cfg]` arms that declare
  the same module name with different `#[path]` targets keep only the first
  declaration’s file. One module path maps to one file.
- **Function-body `use`.** Imports inside function bodies are attributed to the
  enclosing module (`scope_widened`); wider than rustc's real scope. Inline
  module-level `use` is attributed to that module correctly.
- **Re-export chains** are followed at most 32 hops.
- **Feature-gated and target-specific dependencies** cannot be resolved without
  choosing a feature set and target.
- **Macro-hidden calls** outside the expression-macro allowlist (user macros,
  pattern macros, `stringify!`, `macro_rules!` bodies, …) stay absent. Even
  allowlisted recovery is marked `from_macro` because it is not a real CST
  `CallExpr`.
- **Macro-hidden modules** outside the item-macro allowlist, and modules that
  exist only inside `macro_rules!` definition bodies (`crate_root!()`), stay
  absent.
- **Consumer-side cross-crate globs** (`use text_engine::*` in the *calling*
  crate) are not specially expanded; explicit path / import targets into path
  deps are the supported form. Foreign crates' own `pub use dep::*` facades
  *are* followed (see Cross-crate reachability).
- **Examples / tests / benches** are not discovered as crates (deliberate).

## Closed questions

1. **JSON schema.** Closed. The consumer contract is
   [`docs/json-output.md`](../json-output.md), matching the serde types in
   [`src/map.rs`](../../src/map.rs) (adjacent-tagged `CallTarget`, summary
   drop counters, optional `from_macro`, `{name}[bin]` FunctionIds).
2. **Correctness measurement.** Closed as far as the standing harness reaches.
   Fixture oracles (including adversarial, rename, and proc-macro fixtures)
   report **zero false positives**; an LSIF comparison against Horizon itself
   also reports **zero** Horizon `Resolved` edges that disagree with a
   rust-analyzer free-function moniker (including a separate `from_macro`
   cohort). Methodology, re-run commands, and snapshot tables live in
   [`docs/correctness-measurement.md`](../correctness-measurement.md).
   **Limitation (do not over-read the result):** everything measured so far is
   Horizon's own small, clean source tree plus purpose-built fixtures, plus a
   hand sample on unfamiliar code in
   [`docs/scale-validation.md`](../scale-validation.md). That is not LSIF-grade
   evidence about large, messy corpora. Re-run after resolver changes; do not
   treat snapshot counts as frozen product KPIs.
3. **Manifest dependency renaming.** Closed. `alias = { package = "real-name",
   path = "…" }` is honoured: discovery records `Dependency.rename`, and
   cross-crate roots / the dependency gate use the import rustc name. Fixture
   `renamed-path-dep` plus oracle edge.
4. **Target kinds to discover.** Closed for v1. Include library-like
   (`lib` / `rlib` / `dylib` / `cdylib` / `staticlib` / `proc-macro`) and
   `bin`. Exclude `example` / `test` / `bench` / `custom-build` deliberately
   (secondary surfaces; `build.rs` is build machinery). Reasoning in Discover
   pipeline step and [`src/discover.rs`](../../src/discover.rs) module docs.
5. **Item-macro `mod` recovery scope.** Closed for the allowlist
   (`cfg_if!` + `cfg_*`) with union-of-branches and depth bound 8. Explicitly
   **not** recovering definition-body expanders (`crate_root!()`) — see
   Decisions. Scale evidence: Tokio 114→492 map files; serde main lib still
   thin.

## Open questions

1. **Update trigger.** Still open. The tool itself is a **single-shot batch**
   analysis: no on-disk cache, no incremental state, no file watcher. Live
   incremental updating while typing is already a non-goal (see Non-goals).
   What remains unspecified is the *host* product's policy — when a codebase
   is considered "uploaded" / stale, whether prior JSON is retained between
   runs, and who invokes `horizon` again. None of that is in this repository.
2. **Parallelism.** Still open; never redesigned. The pipeline today is fully
   sequential: `cargo metadata` subprocesses, then per-crate module walk +
   parse + extract file-by-file, then resolve. Scale validation shows ~21 s /
   ~0.8 MB/s analysed on rust-analyzer and projects ~10–20 minutes for a
   50k-file dense tree — parallelism is the obvious lever, not a correctness
   blocker. Any parallel extract must still finish Pass 1 for every crate
   before Pass 2 resolve, because cross-crate edges need callees present.
3. **Broader macro recovery.** User-defined macros and definition-body
   expanders stay opaque by design. A future opt-in (known-safe crates, or a
   carefully bounded definition-body walker) would need its own false-positive
   measurement; expression recovery already sets `from_macro` so provenance is
   visible.
4. **Nested free-function recursion by unqualified name.** Observed while
   building the harness: an unqualified recursive call inside a nested `fn`
   can surface as `Unresolved` even when the nested function is the intended
   callee. Product code currently avoids the pattern; no dedicated fixture
   yet. See "What remains unmeasured" in the correctness doc.
5. **Cross-crate precision beyond fixtures.** `path-dependency` /
   `workspace-dep-gate` / `renamed-path-dep` / `cross-crate-facade` cover the
   dependency gate, renames, visibility, and facade following, but LSIF
   breadth comparison is same-repo only. A multi-crate LSIF filter for foreign
   `FunctionId`s is not built — newly resolved facade edges on large corpora
   still need hand spot-checks (see scale-validation).
6. **UpperCamelCase modules absent from the module tree.** The strict
   UpperCamelCase heuristic can still drop a free-function path as associated /
   constructor when a module looks like a type and was not discovered.
   Documented risk; no adversarial specimen yet. Segments that *fail* the
   heuristic now surface as `Unresolved` rather than silent drops.
7. **Dev-dependencies as external roots.** Scale validation shows test-only
   externals becoming `Unresolved` instead of `external_dropped` because
   discovery omits `[dev-dependencies]`. Including them as `external` (not
   walked) would clean counters without expanding the indexed universe —
   product decision, not yet taken.
8. **`PathCrateIndex` vs `ResolveIndex` shape.** `PathCrateIndex` is a
   foreign-crate subset of the resolve tables. Further consolidation was
   judged behaviourally risky (different visibility rules, glob tables only on
   the local index). Left as a possible cleanup, not a correctness issue.

## Deferred

See [`docs/deferred-scope.tex`](../deferred-scope.tex) for the full record of
what was considered and set aside, including measured costs.
