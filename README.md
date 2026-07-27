# Horizon

Horizon builds a **function map** of a Rust repository: a navigable tree

```text
Repository → Crate → Folder → File → Function
```

where each free function carries its outgoing call sites, and each call site
resolves to the function it targets — or records why it could not.

It parses with [`ra_ap_syntax`](https://crates.io/crates/ra_ap_syntax) and
**never compiles** the target code, so in-progress and non-compiling source
still yields a map. Cargo is used only for crate discovery
(`cargo metadata --no-deps --offline`).

**Defining rule: the tool never guesses.** A call resolves to exactly one
definition, or records a `Conflict` naming every candidate, or records
`Unresolved`. Recognised-but-out-of-scope calls (external crates, methods,
`impl` items, constructors, associated functions) are dropped from the tree
entirely rather than mislabelled.

## Who this tool is for

Horizon maps **free functions only**. Methods, `impl` / `trait` items, and
associated functions (`Type::name`) are **permanently out of scope** — not a
temporary gap and not on a roadmap for this tool. If most of your program lives
in methods, the map will be thin by design.

Scale validation on real repositories
([`docs/scale-validation.md`](docs/scale-validation.md)):

| Codebase style | What you get |
|---|---|
| Free-function pipelines (e.g. ripgrep) | Useful: see [`docs/scale-validation.md`](docs/scale-validation.md) for current resolved/unresolved counts |
| Method / `impl`-centric (e.g. tokio) | Partial: free helpers visible after `cfg_*` module recovery (762 fns / 529 resolved), but the runtime still lives in methods |
| Macro-assembled via definition bodies (e.g. serde `crate_root!`) | Thin: proc-macro crates appear; main library modules inside `macro_rules!` bodies stay closed |

**Throughput:** about **0.8–1.1 MB/s** of analysed source on release builds for
dense trees. A ~50,000-file repository is a batch job of roughly **10–20
minutes**, not an interactive refresh. Details and methodology are in the
scale-validation doc.

## Why use it

You want a static picture of free-function call structure across a real Cargo
tree — workspaces, path dependencies (including Cargo renames), mid-edit code —
without waiting on a full type-aware index, and without silent “likely” edges
when the language itself refuses to pick a winner.

## Build and run

```bash
cargo build --bin horizon
```

```bash
# JSON map on stdout (pretty-printed)
cargo run --bin horizon -- path/to/repo

# write to a file; brief summary still goes to stderr
cargo run --bin horizon -- path/to/repo -o map.json

# single-line JSON
cargo run --bin horizon -- path/to/repo --compact
```

```rust
use horizon::build_function_map;

let map = build_function_map("path/to/repo")?;
```

## What it handles today

Through Phase 5a (including allowlisted macro recovery):

- Crate discovery across workspaces and path dependencies via `cargo metadata`
- Library-like targets: `lib` / `rlib` / `dylib` / `cdylib` / `staticlib` /
  **`proc-macro`** (ordinary free functions in proc-macro crates appear).
  Binaries included; examples, tests, benches, and `build.rs` deliberately
  excluded (see requirements)
- Dependency rename aliases (`alias = { package = "real-name", … }`) for
  cross-crate resolution and the dependency gate
- Declaration-driven `mod` walks (`#[path]` honoured; undeclared files omitted)
- `mod` declarations inside **allowlisted** item macros (`cfg_if!`, Tokio-style
  `cfg_*!`) recovered by re-parsing the invocation token tree (depth-bounded);
  all `cfg_if!` branches taken as a union (no build-cfg choice)
- Unqualified, module-qualified, `crate::`, `self::`, and `super::` paths
- Import table with rustc-accurate precedence (local definition beats explicit
  `use`, explicit `use` beats glob; two globs for the same name → `Conflict`
  matching rustc `E0659`)
- Cross-crate resolution behind a declared-dependency gate, with cross-crate
  visibility enforcement (`pub` item behind an all-`pub` module chain) and
  facade following (`pub use` / renamed `pub use` / `pub use glob::*` /
  `pub extern crate dep as name`, including chains through several path
  crates, hop-bounded at 32)
- Doc comments (`///`, `//!`, block forms, `#[doc = "…"]`)
- File-level call sites (e.g. `const` / `static` initialisers calling a
  `const fn`)
- Binary targets distinguished in `FunctionId` as `{name}[bin]`
- Calls hidden inside **allowlisted** expression macros (`format!`, `println!`,
  `assert_eq!`, `vec!`, …), recovered by re-parsing the token-tree interior and
  marked `from_macro: true` in JSON

A standing correctness harness reports zero false positives on hand-built
fixture oracles and on an LSIF comparison against Horizon itself — see
[`docs/correctness-measurement.md`](docs/correctness-measurement.md) for scope
and limitations of that claim.

## What it deliberately does not

Equal prominence: these are not “not yet” unless stated.

| Exclusion | Status |
|---|---|
| Methods and `impl` / `trait` items (declarations and call sites) | **Permanently out of scope** |
| Associated functions on types (`Vec::new`, `Type::assoc`) | Permanently out of scope; counted in `associated_dropped` |
| Enum variants and tuple-struct constructors | Permanently out of scope; counted in `constructor_dropped` |
| Calls into `std` / registry / git dependencies | Deliberately dropped; counted in `external_dropped` |
| Indirect calls / functions passed as values | Out of scope |
| Calls inside non-allowlisted macros (user macros, `matches!`, `stringify!`, `macro_rules!` bodies, …) | Stay absent — prefer a miss over a fabricated edge |
| Modules assembled only inside `macro_rules!` **definition** bodies (e.g. serde `crate_root!()`) | Stay absent — expanding definition bodies is a different, riskier problem |
| Examples, integration tests, benches, `build.rs` | Deliberately not discovered as map crates |
| Consumer-side cross-crate glob imports (`use dep::*` in the *calling* crate) | Not expanded; use explicit paths / imports. (Foreign crates' own `pub use dep::*` facades *are* followed.) |
| `use` inside a function body | Treated as module-wide (`scope_widened`), not body-scoped |
| Visibility filtering on direct within-crate edges | Deliberately **not** enforced (map what was written); globs and cross-crate edges **do** filter |
| Visual frontend | Deferred; consume the JSON instead |
| Live incremental updating / caching | Non-goal; each run is a single-shot batch rebuild |

## Documentation

| Document | Contents |
|---|---|
| [`docs/json-output.md`](docs/json-output.md) | JSON contract for consumers (node hierarchy, `CallTarget`, ids, summary counters, worked example) |
| [`docs/requirements/rust-function-map.md`](docs/requirements/rust-function-map.md) | Specification of record — goals, non-goals, pipeline, decisions |
| [`docs/correctness-measurement.md`](docs/correctness-measurement.md) | Fixture oracles + LSIF harness; how to re-run; what the zero-FP claim covers |
| [`docs/scale-validation.md`](docs/scale-validation.md) | Real-repo throughput, free-function vs method-heavy usefulness, 50k-file estimates |
| [`docs/deferred-scope.tex`](docs/deferred-scope.tex) | Design-phase exclusions and accepted limitations (LaTeX) |
| [`tests/fixtures/README.md`](tests/fixtures/README.md) | Specimen crates for resolution hazards |

## License

MIT
