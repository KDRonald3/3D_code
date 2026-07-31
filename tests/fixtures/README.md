# Function-map test fixtures

Salvaged from throwaway spikes under `%TEMP%` (`horizon-ast-spike`,
`horizon-freecrate`) during the design of Horizon's Rust function map. Each
fixture isolates one resolution hazard (or a tightly related cluster). They are
**specimen inputs** for the map tool, not part of the Horizon build.

The map tool parses with `ra_ap_syntax` (no compile) and must resolve every free
function call to a single `Function`, or emit a `Conflict` node with named
candidates. Method calls and `impl` blocks are **permanently out of scope**
(see `docs/requirements/rust-function-map.md`); fixtures that probe methods are
kept as specimens for any future type-aware tool, not as Horizon v1 work.

Do not commit `target/` or `Cargo.lock` for these crates (see `.gitignore`).

Fixtures are **not** required to compile. The tool analyses code mid-edit, and
`parser-spike/broken.rs` exists specifically for parser-error tolerance. Every
fixture's compile status must be **intentional and documented** — never
accidental breakage.

Nested fixture crates declare an empty `[workspace]` table so they are not
claimed by Horizon's root workspace when checked via `--manifest-path`.

---

## Layout

| Directory | Origin | Hazard | Compile status |
|---|---|---|---|
| [`phase1-single-file/`](phase1-single-file/) | new | Phase 1 walking skeleton: same-file free functions, recursion, undefined name, cfg-duplicate `open` | **Compiles** (exit 0; see `expected-compile-ok.txt`) |
| [`phase2-modules/`](phase2-modules/) | new | Phase 2: `mod` walk, `super` hops, folder nesting, file-level `const fn` call, undeclared `orphan.rs` | **Compiles** (exit 0; see `expected-compile-ok.txt`) |
| [`parser-spike/`](parser-spike/) | `horizon-ast-spike` root | CST dump / call-extractor tooling + multi-construct specimens | N/A as a single lib (bins + specimens); `broken.rs` deliberately fails to parse cleanly |
| [`method-ambiguity/`](method-ambiguity/) | `horizon-ast-spike/demo` | Inherent vs trait `get`, plus same name on two types | No `Cargo.toml` (specimen sources only) |
| [`impl-free-globs/`](impl-free-globs/) | `horizon-ast-spike/free` | Impl-free call forms, re-exports, local vs glob | **Compiles** (exit 0; see `expected-compile-ok.txt`) |
| [`glob-ambiguity/`](glob-ambiguity/) | new | Glob-vs-glob `get` → rustc `E0659` | **Fails** with `E0659` (see `expected-rustc-error.txt`) |
| [`glob-resolved/`](glob-resolved/) | new | Same as above + explicit `use` that wins | **Compiles** (exit 0; see `expected-compile-ok.txt`) |
| [`path-dependency/`](path-dependency/) | `horizon-freecrate` | Path deps, visibility, module discovery; also has resolved `get` | **Compiles** (exit 0; see `expected-compile-ok.txt`) |
| [`workspace-dep-gate/`](workspace-dep-gate/) | new | Workspace members + dependency gate: same `shared` name in `dep_a` / `dep_b`; consumer depends only on `dep_a` | **Intentional mid-edit** (`dep_b::shared` is E0433; see `expected-compile-ok.txt`) |
| [`use-tree-syntax/`](use-tree-syntax/) | `horizon-ast-spike/usetree` | Nested `use` trees, `self`, `as _` | **Compiles** as a tiny lib (`Cargo.toml` added for Phase 3 map runs) |
| [`doc-comments/`](doc-comments/) | new | Outer / inner / block / `#[doc]` docs; ordinary `//` must stay out | **Compiles** (exit 0; see `expected-compile-ok.txt`) |
| [`lib-and-bin/`](lib-and-bin/) | new | Same-named lib + bin: FunctionId uses `name[bin]`, not a fabricated crate name | **Compiles** (exit 0; see `expected-compile-ok.txt`) |
| [`parser-dumps/`](parser-dumps/) | `horizon-ast-spike/out` | Saved CST / tree / edge dumps from the spike runs | N/A (outputs, not a crate) |
| [`exclude-non-functions/`](exclude-non-functions/) | new | Constructors / associated fns must be absent; `mystery` Unresolved; `helper` Resolved | **Fails** (undefined `mystery`; see `expected-rustc-error.txt`) |
| [`local-bindings/`](local-bindings/) | new | Closure / param calls must be dropped (`local_dropped`); nested `fn` items still resolve | **Compiles** (exit 0; see `expected-compile-ok.txt`) |
| [`inline-mod-imports/`](inline-mod-imports/) | new | Inline `mod tests` with `use super::*` / `use super::name` must resolve parent (incl. private) and parent private `use` bindings | **Compiles** (exit 0; see `expected-compile-ok.txt`) |
| [`adversarial-resolution/`](adversarial-resolution/) | new | False-positive traps: same name at several depths, local-vs-glob, rename chains, cfg twins, module/function name collision, nested-inline shadowing | **Compiles** (exit 0; see `expected-compile-ok.txt`) |
| [`macro-hidden-calls/`](macro-hidden-calls/) | new | Calls inside allowlisted macro token trees (`format!` / `println!` / `assert_eq!` / `vec!` / nested) vs traps (`matches!`, `stringify!`, `macro_rules!`, tuple-struct ctors) | **Compiles** (exit 0; see `expected-compile-ok.txt`) |
| [`macro-modules/`](macro-modules/) | new | `mod` decls inside allowlisted item macros (`cfg_if!` multi-branch union, `cfg_fs!`) vs missing-file branch and `stringify!` trap | **Compiles** (exit 0; see `expected-compile-ok.txt`) |
| [`renamed-path-dep/`](renamed-path-dep/) | new | Cargo manifest rename (`alias = { package = "text-engine", path = "engine" }`) must resolve `alias::greet` | **Compiles** (exit 0; see `expected-compile-ok.txt`) |
| [`proc-macro-crate/`](proc-macro-crate/) | new | `proc-macro = true` library: free functions must appear (metadata kind is `proc-macro`, not `lib`) | **Compiles** (exit 0; see `expected-compile-ok.txt`) |
| [`cross-crate-facade/`](cross-crate-facade/) | new | Cross-crate `pub use` facades: plain, renamed, multi-crate chain, glob, glob Conflict; negatives for `pub(crate)`, private module path, undeclared dep | **Intentional mid-edit** (E0659 / E0603 / E0433; see `expected-compile-ok.txt`) |

Fixtures that participate in the standing correctness harness also carry
`expected-edges.json` (hand-annotated ground truth). Re-run with
`cargo run -p horizon-correctness --bin measure_correctness -- fixtures` or
`cargo test --test correctness_measurement`. See
[`docs/correctness-measurement.md`](../../docs/correctness-measurement.md).

---

## `exclude-non-functions/`

**Hazard:** Deliberate exclusion categories vs a genuine `Unresolved` free-function
call. Ensures constructors / associated functions are absent from the map (not
recorded as unresolved), while an unknown name still becomes `Unresolved`.

**Compile status:** intentional **failure** (`mystery` is undefined). Recorded in
`expected-rustc-error.txt`.

---

## `proc-macro-crate/`

**Hazard:** `cargo metadata` reports procedural-macro libraries with target
kind `proc-macro` (not `lib`). Discovery that only accepts `lib` / `bin`
silently omits the entire crate — including ordinary free functions inside it.

**Contents:** `Cargo.toml` (`[lib] proc-macro = true`, empty `[workspace]`),
`src/lib.rs` with free functions `expand_name` / `sanitize` and a
`#[proc_macro] identity` entry point that calls `sanitize`.

**What the function map must produce:**

| Item | Expected |
|---|---|
| Crate `proc-macro-crate` | Present, `is_library: true` |
| `proc_macro_crate::expand_name` / `sanitize` / `identity` | Present as `Function` nodes |
| `sanitize` → `expand_name` | Resolved |
| `identity` → `sanitize` | Resolved |

**Compile status:** intentional **success**. Recorded in
`expected-compile-ok.txt`. Participates in the standing oracle harness.

---

## `cross-crate-facade/`

**Hazard:** Cross-crate facade re-exports — the dominant published-Rust pattern
where a crate root re-exports definitions that live in other path crates.
Within-crate facades are covered by [`impl-free-globs/`](impl-free-globs/);
this fixture is the *across* crate-boundary case.

**Contents:** workspace with `consumer` (depends only on `text-facade`) plus
`text-facade`, `format-engine`, `parse-engine`, `mid-crate`, `leaf-crate`,
`shapes-eng`, `text-eng`.

| Call / situation | Expected |
|---|---|
| `upper` via `pub use format_engine::upper` | Resolved → `format_engine::upper` |
| `split` via `pub use parse_engine::tokenize as split` | Resolved → `parse_engine::tokenize` |
| `chained` via facade → mid → leaf | Resolved → `leaf_crate::chained` |
| `area` via `pub use format_engine::*` | Resolved → `format_engine::area` |
| `from_private` via `pub use private_mod::from_private` | Resolved → `text_facade::private_mod::from_private` (Rust-correct facade over a private module) |
| `text_facade::fmt_eng::upper` via `pub extern crate format_engine as fmt_eng` | Resolved → `format_engine::upper` (ripgrep-style crate rename facade) |
| `text_facade::get` via two `pub use …::*` globs | **Conflict** `[shapes_eng::get, text_eng::get]` |
| `text_facade::crate_only_version` (`pub(crate) use`) | **Unresolved** |
| `text_facade::private_mod::from_private` (named private module) | **Unresolved** |
| `format_engine::upper` (consumer does not declare it) | **Unresolved** (dependency gate) |

**Compile status:** intentional **mid-edit** — conflict + privacy + undeclared
crate keep the consumer from compiling. Recorded in `expected-compile-ok.txt`.
Participates in the standing oracle harness.

---

## `renamed-path-dep/`

**Hazard:** Manifest dependency renaming —
`alias = { package = "text-engine", path = "engine" }`. Code says `alias::…`;
the package on disk is `text-engine`. Discovery and the dependency gate must
use the import name without losing the path edge.

**Contents:** consumer package plus `engine/` (`text-engine`), empty
`[workspace]` on both.

**What the function map must produce:** `alias::greet` from the consumer
resolves to `text_engine::greet`.

**Compile status:** intentional **success**. Recorded in
`expected-compile-ok.txt`. Participates in the standing oracle harness.

---

## `macro-modules/`

**Hazard:** Module trees declared inside item-pasting macros (`cfg_if!`,
Tokio-style `cfg_*!`) are invisible to a literal `mod` walk. Recovery must open
only allowlisted macros, union `cfg_if!` branches (no build-cfg choice), skip
missing files without panicking, and leave `stringify!` closed.

**Not covered here (deliberate):** serde-style `crate_root!()` where modules
live in a `macro_rules!` *definition* body rather than the invocation token
tree — that pattern stays closed (see requirements / deferred-scope).

**Contents:** `Cargo.toml` (package `macro-modules`, empty `[workspace]`),
`src/lib.rs` plus `net`, `alt`, `fallback`, `gated`, `literal`.

| Declaration | Expected in map |
|---|---|
| `mod net` / `alt` / `fallback` inside `cfg_if!` branches | Present (union), correct `crate::…` paths |
| `pub mod gated` inside `cfg_fs!` | Present |
| `mod literal` (ordinary) | Present |
| `mod absent_file` (branch, file missing) | Module path recorded; **no** file node |
| `mod must_not_appear` inside `stringify!` | **Absent** |

**Compile status:** intentional **success**. Multi-branch `cfg_if!` expands to
the union of branches (aligned with Horizon); `cfg_missing!` is a no-op so
`absent_file` is Horizon-only. Recorded in `expected-compile-ok.txt`.

---

## `macro-hidden-calls/`

**Hazard:** Macro argument token trees hide real free-function calls from a
normal CST walk, but the same token shape also appears in patterns, stringify
arguments, and `macro_rules!` templates. Recovering the former must never invent
the latter.

**Contents:** `Cargo.toml` (package `macro-hidden-calls`, empty `[workspace]`),
`src/lib.rs`.

| Site | Expected |
|---|---|
| `mean` in `format!`, `width_of` in `println!`, `helper` in `assert_eq!` / `vec![]` | Resolved (`from_macro: true`) |
| `nested_target` inside `println!("{}", format!(…))` | Resolved (nested allowlisted recovery) |
| `Some(…)` in `matches!`, `helper` in `stringify!`, `Foo(…)` in `vec![]` | **Absent** |
| `mean` inside `macro_rules!` / user-macro `identity_call!` | **Absent** |
| Ordinary `helper()` outside macros | Resolved (`from_macro` omitted / false) |

**Compile status:** intentional **success**. Recorded in `expected-compile-ok.txt`.

---

## `adversarial-resolution/`

**Hazard:** Confident wrong edges — the failure mode the no-guessing rule exists
to prevent.

**Contents:** `Cargo.toml` (package `adversarial-resolution`, empty `[workspace]`),
`src/lib.rs` plus `alpha`, `beta`, `chain`, `deep`.

| Trap | Expected |
|---|---|
| `helper` / `deep::helper` / `deep::inner::helper` | Distinct Resolved targets (no collapse) |
| `beta::hop2` through rename chain | Resolved → `chain::original` |
| `shadow::shadow` (module name = function name) | Resolved → `shadow::shadow` |
| Local `collide` vs glob from `alpha` | Resolved → local `collide` |
| Nested inline `name` vs glob `alpha::name` | Resolved → `nested::name` |
| `twin()` cfg duplicates | Conflict naming both `#L…` ids |

**Compile status:** intentional **success**. Recorded in `expected-compile-ok.txt`.

---

## `doc-comments/`

**Hazard:** Doc-comment extraction — exact text, kind, and attachment.

**Contents:** `Cargo.toml` (package `doc-comments`, empty `[workspace]`),
`src/lib.rs` with:

- File-level `//!` inner docs (two lines → one joined `DocComment` on `File`)
- `///` outer on `alpha` (single line)
- Several consecutive `///` lines on `beta` (joined with `\n`)
- `///` separated from `gamma` by `#[inline]` (still attaches)
- `/** … */` block outer on `delta`
- Ordinary `//` above `epsilon` (must **not** appear)
- `#[doc = "…"]` on `zeta`

**Compile status:** intentional **success**. Recorded in
`expected-compile-ok.txt`.

**What the function map must produce:**

| Item | Expected |
|---|---|
| `File.doc_comments` | one Inner: `File-level module docs…\nSecond inner line.` |
| `alpha` | Outer: `Single-line outer docs on alpha.` |
| `beta` | Outer: two lines joined by `\n` |
| `gamma` | Outer still attached despite intervening attribute |
| `delta` | Outer from block form |
| `epsilon` | empty `doc_comments` |
| `zeta` | Outer from `#[doc = …]` |

Whitespace: one leading space after `///` / `//!` is stripped per line;
further indentation is preserved.

---

## `lib-and-bin/`

**Hazard:** Package with library and binary targets that share the cargo
target name `lib_and_bin`. FunctionIds must not collide and must not invent
a fake crate name such as `lib_and_bin_bin`.

**Contents:** `Cargo.toml` (empty `[workspace]`), `src/lib.rs` (`helper`),
`src/main.rs` (`main` calling `lib_and_bin::helper`).

**Compile status:** intentional **success**. Recorded in
`expected-compile-ok.txt`.

**What the function map must produce:**

| Target | `rustc_name` | FunctionId |
|---|---|---|
| library | `lib_and_bin` | `lib_and_bin::helper` |
| binary | `lib_and_bin` (real name) | `lib_and_bin[bin]::main` |

---

## `phase1-single-file/`

**Hazard:** End-to-end Phase 1 subset — nothing more. One crate, one root file,
free functions only, unqualified same-file resolution.

**Contents:** `Cargo.toml` (package `phase1-single-file`, empty `[workspace]`),
`lib.rs` with:

- `alpha` → `beta` (resolved), `alpha` (recursion, resolved), `mystery`
  (unresolved), `open` (Conflict between cfg duplicates), `crate::beta`
  (resolved in Phase 2+)
- `beta` → `alpha` (resolved)
- `#[cfg(unix)] fn open` / `#[cfg(windows)] fn open` — both present in the
  syntax tree; FunctionIds get `#L{line}` suffixes; the call is a Conflict

**Compile status:** intentional **success** (the inactive cfg arm is simply
absent for rustc). Recorded in `expected-compile-ok.txt`.

**What the function map must produce:**

| Call site | Expected |
|---|---|
| `beta()` in `alpha` | Resolved → `phase1_single_file::beta` |
| `alpha()` in `alpha` | Resolved → `phase1_single_file::alpha` (recursion) |
| `mystery()` | Unresolved |
| `open()` | Conflict `[…::open#L…, …::open#L…]` |
| `crate::beta()` | Resolved → `phase1_single_file::beta` (Phase 2+) |
| `alpha()` in `beta` | Resolved → `phase1_single_file::alpha` |

---

## `phase2-modules/`

**Hazard:** Phase 2 subset without `use` — module walk, cross-file qualified
paths, `self` / `super` / multi-`super`, nested folder `child/grand.rs`,
module-level `const fn` call on the `File` node, and an undeclared `orphan.rs`.

**Contents:** `Cargo.toml` (package `phase2-modules`, empty `[workspace]`),
`src/lib.rs`, `src/child.rs`, `src/child/grand.rs`, `src/orphan.rs` (garbage,
never declared).

**Compile status:** intentional **success**. Recorded in
`expected-compile-ok.txt`.

**What the function map must produce:**

| Situation | Expected |
|---|---|
| `orphan.rs` | Absent from the map |
| `child::child_fn` / `crate::child::grand::deep` from `root_fn` | Resolved cross-file |
| `super::sibling_of_child` from `child` | Resolved |
| `super::sibling` / `super::super::sibling_of_child` from `grand` | Resolved |
| `const MAX = compute_max()` | File-level `CallSite` on `lib.rs`, Resolved → `compute_max` |
| Folder `src/child/` | Holds `grand.rs`; `child.rs` stays in `Crate.files` |

---

## `parser-spike/`

**Hazard:** Parser behaviour and extractor scaffolding, not one call-resolution
case. This is the original `ast-spike` crate.

**Contents:**

- `Cargo.toml` — package `ast-spike`, two bins: `dump` (`src/main.rs`) and
  `calls` (`src/bin/calls.rs`), depends on `ra_ap_syntax`.
- `sample.rs` — one file exercising structs, enums, traits with default methods,
  inherent `impl` (including `async` / `const` / `unsafe`), trait `impl`, free
  functions, nested functions, `fn` as a parameter type, and a `#[cfg(test)]`
  module.
- `broken.rs` — deliberately malformed (unclosed generics, unfinished `fn new(`,
  truncated `pub fn`). Proves the parser still yields a tree with error nodes.
- `paths.rs` — several `use` forms (plain, deep path, brace list, rename,
  nested `self`) for measuring CST nesting depth. Not a complete crate.
- `rust.ungram` — rust-analyzer's grammar reference used while exploring node
  kinds.

**Expected for the function map:**

- On `sample.rs`: free functions `count_hits`, `outer`/`inner`, `apply` appear as
  `Function` nodes. Calls like `inner()` resolve within-file. Method calls
  (`self.get`, `self.touch`, `HashMap` methods) are out of scope for v1 and
  should not be invented as free-function edges.
- On `broken.rs`: still produce a map (or partial file node); never refuse.
  Incomplete items may yield `Conflict` / unknown-name markers rather than a
  crash.
- The `dump` / `calls` binaries are historical probe tools, not Horizon product
  code. Keep them so CST / resolution experiments remain reproducible.

---

## `method-ambiguity/`

**Hazard:** Method-name collision that name lookup alone cannot decide.

**Contents:** two files, no `Cargo.toml` (specimen directory for `calls`).

- `cache.rs` — `Cache` has an **inherent** `get` and a **trait** `Store::get`
  with the same signature. Also free functions `describe` and `count_hits`.
- `app.rs` — `Registry` also has `get` (different type). `run()` calls
  `cache.get(...)` and `registry.get(...)`, plus free-function forms.

**What `rustc` does:**

- `cache.get("k")` is a method call. Both the inherent method and the trait
  method are candidates; with identical signatures, UFCS / turbofish /
  disambiguation may be required depending on context. This is exactly why
  method resolution is deferred: it needs types, not just names.
- Free function `describe` is defined in **both** files. From `app.rs`, the
  local `describe` wins over an imported one (here `describe` is not imported
  from `cache`; only `count_hits`, `Cache`, `Item` are). So `describe(...)` in
  `run` is the local definition — a single resolved edge.
- `count_hits` is unique and imported — single resolved edge to `cache.rs`.

**What the function map should produce (v1, free functions only):**

| Call site | Expected |
|---|---|
| `count_hits(&[])` | Resolved → `cache::count_hits` |
| `describe(...)` in `app` | Resolved → `app::describe` (local) |
| `Cache::new()` / `Registry::new()` | Out of scope (associated / method) or deferred |
| `cache.get` / `registry.get` / `self.tally` | Out of scope for v1; when methods land, `get` on unknown/`Cache` receivers is a prime `Conflict` candidate: inherent `Cache::get` vs `<Cache as Store>::get`, and/or `Registry::get` if the receiver type is unknown |

---

## `impl-free-globs/`

**Hazard:** Impl-free call surface — re-exports, globs, nested modules, recursion,
closures, external crates — without a path dependency. Deliberately **does not**
reproduce glob-vs-glob `E0659` (that lives in [`glob-ambiguity/`](glob-ambiguity/)
/ [`glob-resolved/`](glob-resolved/)).

**Origin:** `horizon-ast-spike/free/`. Now a real crate (`Cargo.toml` with
`[lib] path = "lib.rs"`) so compile status can be checked.

**Why it was repaired this way:** After salvage, `app.rs` still called `get(3)`
with type `int`, but neither `shapes` nor `text` defined `get`. That was
accidental breakage, not a chosen hazard. The ambiguous-`get` story is covered
by the dedicated pair below, so this fixture keeps its real value: every
impl-free call form (qualified paths, re-exports, renamed `pub use`, recursion,
indirect/`fn` parameter, closure body, `std::…` external, local name beating a
glob). The broken `get`/`int` lines were removed; `use crate::text;` was added
so `text::upper` / `text::case::snake` resolve (matching the pattern already in
`path-dependency/`).

**Compile status:** intentional **success**. Recorded in
`expected-compile-ok.txt` (`cargo check` exit 0; unused-import / dead-code
warnings only).

**Call forms exercised in `app::run`:** within-file (including callee defined
later), aliased import (`sum_all as total`), module-qualified, nested
`text::case::snake`, `crate::numbers::mean`, re-export `crate::mean`, renamed
re-export `shout_upper`, glob-imported `area`, `self::normalize`, inline module
`helpers::indent`, recursion `countdown`, function-pointer argument to `apply`
(the *call* through `f` is indirect — no name at the site), call inside a
closure body, `std::cmp::max` (external — drop / refuse), and local `describe`
shadowing a glob-imported `shapes::describe`.

**What the function map should produce:**

| Call / situation | Expected |
|---|---|
| `normalize`, `countdown`, `helpers::indent`, `self::normalize` | Resolved within `app` |
| `mean`, `total`→`sum_all`, `text::upper`, `text::case::snake`, `crate::numbers::mean` | Resolved to defining free functions |
| `crate::mean` / `shout_upper` | Resolved through re-exports to `numbers::mean` / `text::upper` |
| `area` via `use crate::shapes::*` | Resolved → `shapes::area` |
| `describe(&n)` | Resolved → **local** `app::describe` (explicit local beats glob) |
| `apply(..., normalize)` | Record call to `apply`; do **not** treat `normalize` as a call edge |
| `f(s)` inside `apply` | Indirect — `Conflict` / skip (no resolvable name) |
| `std::cmp::max` | External — drop, not a node |

---

## `glob-ambiguity/` and `glob-resolved/`

**Hazard:** Glob-versus-glob name clash — the single most important
no-guessing case for `Conflict`.

**Choice of layout:** two sibling fixture crates (not variants inside one
directory). Each has its own `Cargo.toml`, so a test can point
`cargo check --manifest-path …` (or the map tool) at exactly one compile status
without feature flags or swapping files.

### `glob-ambiguity/` — fails with `E0659`

Minimal crate:

- `shapes::get` and `text::get` both defined
- `app.rs` has `use crate::shapes::*; use crate::text::*;` and calls `get(3)`
  with **no** explicit import

**What `rustc` does:** rejects the crate with **`E0659`**: `` `get` is ambiguous ``
because of multiple glob imports of a name in the same module. Candidates named
in the diagnostic are the two glob imports (`shapes::*` and `text::*`).

Verbatim diagnostic (machine-specific `Checking …` line stripped; body unchanged)
is in [`glob-ambiguity/expected-rustc-error.txt`](glob-ambiguity/expected-rustc-error.txt).

**What the function map must produce:** a `Conflict` node at `get(3)` holding
both candidates `[shapes::get, text::get]` (order unspecified). It must **not**
pick one.

### `glob-resolved/` — explicit import wins

Same modules and both globs, plus:

```rust
use crate::text::get;
```

**What `rustc` does:** compiles successfully. The explicit import outranks the
globs, so `get(3)` resolves to **`text::get`**. (Cargo may warn that both globs
are unused for “use” purposes — the names they contribute are not attributed as
uses once the explicit import supplies `get`. The globs remain present for the
resolution story.)

Recorded in [`glob-resolved/expected-compile-ok.txt`](glob-resolved/expected-compile-ok.txt).

**What the function map must produce:** a single resolved edge
`app::run` → `text::get`. Not a `Conflict`.

### Anti-expectation (do not guess)

[`parser-dumps/edges.txt`](parser-dumps/edges.txt) is output from the abandoned
spike resolver. It contains:

```text
app.rs::run (L71)  ->  shapes.rs::get (L12)   [Likely, glob import]
```

That line guessed `shapes::get` for this ambiguous call and labelled it
“Likely, glob”. **The product tool must never do that.** When two globs supply
the same name and there is no explicit import / local definition to break the
tie, the only correct emission is a `Conflict` naming both candidates — matching
rustc’s refusal (`E0659`), not a coin-flip “likely” edge.

---

## `path-dependency/`

**Hazard cluster:** multi-crate path dependency + visibility + module discovery.
Also still contains a *resolved* glob `get` (explicit `use crate::text::get;`),
overlapping [`glob-resolved/`](glob-resolved/) but embedded in a richer crate.
The failing `E0659` case is **not** here — use [`glob-ambiguity/`](glob-ambiguity/).

**Origin:** `horizon-freecrate`. This is a real two-crate tree:

- Root package `freecrate` (`Cargo.toml`) depends on
  `text-engine = { path = "engine" }`.
- Sibling folder `engine/` has package name **`text-engine`** (dash) and lib
  name `text_engine` (underscore in `use` paths). Folder name ≠ package name.

**Compile status:** intentional **success**. Recorded in
`expected-compile-ok.txt`.

### Path dependency & visibility (`uses_engine.rs`, `engine/`)

| Path | Reachable from `freecrate`? |
|---|---|
| `text_engine::format::upper` | yes (`pub mod` + `pub fn`) |
| `text_engine::format::deep::buried` | yes |
| `text_engine::upper` (re-export) | yes |
| `text_engine::version` | yes (fn in dependency lib root) |
| `text_engine::secret::hidden` | **no** — `mod secret` is private (`E0603`); `pub fn` inside does not help |
| `text_engine::format::trim_inner` | **no** — `pub(crate)` only |
| `crate::text_engine::...` | **no** — dependencies are not under `crate::` |
| `engine::...` | **no** — folder name is not the crate name |
| `text-engine::...` | **no** — dash is not a valid path segment |

**Map expectation (Phase 4):** discover both `freecrate` and `text-engine`;
resolve only the reachable free functions across the path edge
(`uses_engine::demo` → `text_engine::format::upper`, re-exports, `version`);
also resolve qualified calls through an *imported module* binding
(`use text_engine::format;` then `format::upper`, renamed `nested::buried`,
renamed crate-root `eng::version` in `via_imported_module`);
never draw edges into private / `pub(crate)` items of another crate (including
`secret::hidden`); never treat folder names or dashed package names as Rust
paths.

---

## `workspace-dep-gate/`

**Hazard:** dependency gate — two workspace members define `pub fn shared`, but
`consumer` declares only `dep_a`. Naming `dep_b::shared` must not resolve.

**Contents:** virtual workspace with `consumer`, `dep_a`, `dep_b` (empty
`[workspace]` not needed on members — they are listed in the root). Nested
fixture convention: analyse this directory as the repo root.

**Compile status:** intentional **mid-edit** — `wrong_sibling` names an
undeclared crate (`E0433`). Recorded in `expected-compile-ok.txt`.

**What the function map must produce:**

| Call site | Expected |
|---|---|
| `shared()` in `consumer::run` | Resolved → `dep_a::shared` |
| `dep_b::shared()` in `consumer::wrong_sibling` | `Unresolved` (not `dep_b::shared`) |

### Module discovery (must walk `mod`, not glob the directory)

| File | Role |
|---|---|
| `src/orphan.rs` | Full of garbage; **never** `mod orphan;` — compiler ignores it. Map must ignore it too. |
| `src/renamed_on_disk.rs` | Declared as `#[path = "renamed_on_disk.rs"] pub mod tidy_name;` — module path is `crate::tidy_name`, not the filename. |
| `src/selfdecl.rs` | Declared `pub mod selfdecl;` in `lib.rs`, and the file itself contains `pub mod selfdecl { ... }`. With both present, the inner module **nests**: outer fn is `selfdecl::outer_fn`, inner is `selfdecl::selfdecl::inner_fn` (see `try_selfdecl` in `lib.rs`). |
| `src/app/helper.rs` | Declared from `app.rs` via `mod helper;`, not from the crate root — exercises `super::` / `super::super::`. |

### Glob name clash: `get` (resolved form only)

Both `shapes::get` and `text::get` exist. `app.rs` has:

```rust
use crate::shapes::*;
use crate::text::*;
use crate::text::get;   // explicit import — disambiguates
```

**What `rustc` does:** `get(3)` resolves to **`text::get`**. No `E0659`.

**What the function map should produce:**

| Configuration | Expected |
|---|---|
| As checked in (explicit `use crate::text::get`) | Single resolved edge `app::run` → `text::get` |
| `describe(&n)` | Resolved → local `app::describe`, not `shapes::describe` |

For the failing globs-only configuration, see [`glob-ambiguity/`](glob-ambiguity/).

Other free-function edges in this crate mirror `impl-free-globs/` (re-exports,
nested modules, recursion, etc.), plus cross-crate edges from `uses_engine`.

---

## `use-tree-syntax/`

**Hazard:** Import table construction from awkward `use` tree syntax.

**Contents:** `Cargo.toml` (package `use-tree-syntax`, empty `[workspace]`),
`lib.rs`, `alpha.rs`, `beta.rs`.

Exercises:

- Nested brace lists: `use crate::{ alpha::{one, two as second}, beta::three }`
- `self` in a brace list binds the **module** (`alpha`), not a name `self`
- `as _` imports nothing nameable (`Marker as _`)
- Deep path `beta::deep::buried`
- `std::collections::HashMap` must stay external

**Map expectation:** `drive()` resolves `one` / `second`→`two` / `three` /
`four` / `buried` / `alpha::one` to the defining free functions. No edge into
`HashMap` methods. `Marker as _` contributes no import binding.

---

## `parser-dumps/`

Saved outputs from the spike session. Not inputs to the map tool; they document
what `ra_ap_syntax` and the probe resolver produced at the time.

| Kind | Files | Notes |
|---|---|---|
| CST / tree | `*.cst.txt`, `*.tree.txt` | From `dump` on specimen files (`sample`, `broken`, `paths`, `cache`, `app`, …). `paths.tree.txt` is essentially empty (`FILE` only) — the projection skipped pure-import files. `lang.*` / `lib.*` are large dumps (≈1 MB CST each), likely from probing Horizon or grammar-sized inputs. |
| Edges | `edges.txt` | Spike `calls` output over an impl-free directory. **Anti-expectation:** it resolved ambiguous glob `get` to `shapes::get` with “Likely, glob import” — the product tool must emit `Conflict` instead (see [`glob-ambiguity/`](glob-ambiguity/)). |
| Methods | `methods.txt` | Spike run against Horizon itself (`bin/server.rs`, …) — mostly `UnknownName` / `ExternalType` skips. |
| Metadata | `metadata-*.json`, `ca-*.json` | `cargo metadata` experiments (`--no-deps` vs full; also Cellular Automata samples). |

---

## Root `Cargo.toml` and self-map discovery

The Horizon root manifest already excludes fixtures from the package/workspace
build:

```toml
[workspace]
resolver = "2"
members = [
    "crates/horizon-map",
    "crates/horizon-engine",
    "crates/horizon",
    "crates/horizon-server",
    "crates/horizon-correctness",
]
default-members = ["crates/horizon"]
exclude = ["tests/fixtures"]
```

**Important:** directory-level `workspace.exclude` alone is **not** enough when
someone runs `cargo check --manifest-path tests/fixtures/.../Cargo.toml` —
Cargo still sees the parent workspace and errors unless the nested package is
excluded *or* declares its own empty `[workspace]` table. Each fixture crate
above already has `[workspace]` for that reason. Keep both: root `exclude`
*and* per-fixture `[workspace]`.

When the function map is pointed at the Horizon repo itself, crate discovery
**skips `tests/fixtures/**`** (see `discover::is_nested_fixture_manifest`).
Do not remove that skip — mapping deliberately broken/ambiguous fixtures as
ordinary source would corrupt every self-test.

---

## Salvage / repair notes

1. **`get` / E0659** is now a dedicated pair: [`glob-ambiguity/`](glob-ambiguity/)
   (fails) and [`glob-resolved/`](glob-resolved/) (explicit import). 
   `path-dependency/` still has the resolved form in a richer multi-crate
   setting.
2. **`impl-free-globs/`** was repaired to compile: removed accidental `get(3)` /
   `int`, added `use crate::text;` and a `Cargo.toml`. Glob ambiguity is no
   longer its job.
3. **Anti-guess:** `parser-dumps/edges.txt` “Likely, glob import” → `shapes::get`
   is exactly what `Conflict` replaces.
4. **Extra fixture found at salvage:** `use-tree-syntax/` — preserved.
5. **Extra hazards in `path-dependency/`:** `selfdecl` nesting, `uses_engine`
   failure modes, `tidy_name` via `#[path]`.
