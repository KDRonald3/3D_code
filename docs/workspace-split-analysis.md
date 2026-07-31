# Workspace split analysis

**Recommendation: medium granularity** — four production crates under `crates/`, plus an optional fifth for the correctness harness. Do **not** split pipeline stages into separate crates.

Suggested names and layout:

```text
Horizon/                          # virtual workspace root
  Cargo.toml                      # [workspace] only
  crates/
    horizon-map/                  # JSON contract types + serde helpers
    horizon-engine/               # discover → resolve + build_function_map
    horizon/                      # CLI binary package (keeps name `horizon`)
    horizon-server/               # axum + static UI (new)
    horizon-correctness/          # optional: measure_correctness binary + lib
  tests/fixtures/                 # stay at repo root (self-map skip path)
  docs/
```

Dependency edges:

```text
horizon-map          ← serde, serde_json, anyhow
horizon-engine       → horizon-map   + ra_ap_syntax, anyhow, serde_json
horizon              → horizon-engine, horizon-map   + clap, anyhow
horizon-server       → horizon-map  (always)
                     → horizon-engine  (optional feature `live-analyse`, or always if live is required)
horizon-correctness  → horizon-engine, horizon-map   + clap, anyhow, serde_json
```

Keep the installable CLI package named **`horizon`** so `cargo install horizon` still yields the `horizon` binary. The analysis library becomes **`horizon-engine`** (or `horizon-core`); the contract crate is **`horizon-map`**.

---

## Why this recommendation

1. **`map` + `json` are already a clean leaf.** [`map.rs`](../crates/horizon-map/src/map.rs) depends only on `serde` / `std` ([lines 29–30](../crates/horizon-map/src/map.rs)). [`json.rs`](../crates/horizon-map/src/json.rs) depends only on `map` + `anyhow` + `serde_json` ([lines 3–6](../crates/horizon-map/src/json.rs)). A UI that only loads saved JSON can depend on `horizon-map` and never compile `ra_ap_syntax`.
2. **Pipeline stages are not separable without inventing awkward crates.** Intermediate types (`FileFacts`, `PendingCall`, `Import`, `TypeDef`, `ItemVisibility`, `ExtractedCrate`, `ResolveIndex`) are shared across [`extract`](../crates/horizon-engine/src/extract.rs) → [`resolve`](../crates/horizon-engine/src/resolve.rs) → [`pipeline`](../crates/horizon-engine/src/pipeline.rs) → [`lib`](../crates/horizon-engine/src/lib.rs) / [`correctness`](../crates/horizon-correctness/src/lib.rs). A one-crate-per-stage split forces those types into yet another crate and turns every stage boundary into a versioned public API for no external consumer.
3. **`modules` is entangled with `extract`.** [`modules.rs`](../crates/horizon-engine/src/modules.rs) imports `ItemVisibility`, `is_item_macro_allowlisted`, and macro helpers from extract ([lines 32–34](../crates/horizon-engine/src/modules.rs)) while also using `parse` and `ra_ap_syntax` directly. That is a real cycle-of-concerns, not a clean stage boundary.
4. **Correctness needs engine internals, not just the public map.** [`collect_call_outcomes`](../crates/horizon-correctness/src/lib.rs) calls `pipeline::{extract_repository, resolve_index_for}` ([lines 10–11, 274–276](../crates/horizon-correctness/src/lib.rs)), which are currently `pub(crate)`. Either keep correctness inside the engine crate, or promote a small public harness API when splitting it out.
5. **Fine-grained crates buy compile isolation nobody asked for**, at the cost of many path rewrites and perpetual “which crate owns `PendingCall`?” churn. Medium already isolates the heavy dependency (`ra_ap_syntax`) from the UI’s JSON-only path.

---

## Current public API and module graph

### Public surface today ([`lib.rs`](../crates/horizon-engine/src/lib.rs))

| Item | Role |
|---|---|
| `build_function_map` ([L53](../crates/horizon-engine/src/lib.rs)) | Sole analysis entry point |
| Re-exports from `map` ([L37–40](../crates/horizon-engine/src/lib.rs)) | Contract types (`Repository`, `CallTarget`, …) |
| Re-exports from `json` ([L33–36](../crates/horizon-engine/src/lib.rs)) | Serialize / deserialize helpers |
| `pub mod` discover, extract, modules, parse, resolve, correctness, map, json | Modules are public; `pipeline` is `pub(crate)` ([L30](../crates/horizon-engine/src/lib.rs)) |

Binaries:

- [`src/bin/horizon.rs`](../crates/horizon/src/main.rs) — thin CLI over `build_function_map` + `write_map*` + clap
- [`src/bin/measure_correctness.rs`](../crates/horizon-correctness/src/bin/measure_correctness.rs) — harness CLI over `horizon::correctness::*`

`thiserror` is declared in [`Cargo.toml`](../Cargo.toml) but **unused** in any `.rs` file. `clap` is only used by the two binaries, yet is a library dependency today.

### Actual internal dependency graph

```text
                    ┌─────────┐
                    │  map    │  serde only — leaf
                    └────▲────┘
           ┌─────────────┼──────────────┬──────────────┐
           │             │              │              │
        json          discover       extract        resolve
           │             │           ▲  │  ▲           │
           │             │           │  │  │           │
           │             │      modules─┘  │           │
           │             │           │     │           │
           │             │         parse   │           │
           │             │           │     │           │
           │             └─────► pipeline ◄┘───────────┘
           │                       │
           │              lib::build_function_map
           │                       │
           └─────────────── correctness
                                   │
                    horizon / measure_correctness bins
```

Concrete cross-boundary types (cite = definition / primary use):

| Type | Defined | Consumed by |
|---|---|---|
| `Repository`, `Crate`, `Function`, `CallSite`, `CallTarget`, … | [`map.rs`](../crates/horizon-map/src/map.rs) | Entire crate; public re-exports |
| `FileFacts`, `PendingCall`, `Import`, `TypeDef`, `ItemVisibility` | [`extract.rs` L47–178](../crates/horizon-engine/src/extract.rs) | `resolve`, `pipeline`, `lib`, `correctness`, `modules` (visibility + macros) |
| `ModuleWalk`, `ModuleFile` | [`modules.rs` L54–69](../crates/horizon-engine/src/modules.rs) | `pipeline` only |
| `ExtractedCrate` | [`pipeline.rs` L21–31](../crates/horizon-engine/src/pipeline.rs) | `lib`, `correctness` (via `extract_repository` / `resolve_index_for`) |
| `ResolveIndex`, `ResolveResult`, `PathCrateIndex` | [`resolve.rs`](../crates/horizon-engine/src/resolve.rs) | `lib`, `pipeline`, `correctness` |
| `SourceFile` (`ra_ap_syntax`) | parse / extract / modules | Never leaves those modules into `map` / `json` / discover (discover has **no** `ra_ap_syntax`) |

`Crate` from `map` is dual-use: discover fills name/roots/deps with empty folders/files ([`discover.rs` L47–59](../crates/horizon-engine/src/discover.rs)); `lib::build_function_map` later attaches the folder tree ([`lib.rs` L85–88](../crates/horizon-engine/src/lib.rs)). That is slightly impure for a “pure contract” crate, but harmless — the JSON schema already includes those fields ([`docs/json-output.md`](json-output.md)).

---

## Natural seams

### Cheap

| Seam | Why cheap |
|---|---|
| **`map` (+ optionally `json`) → `horizon-map`** | No engine imports; no `ra_ap_syntax`. UI and engine both depend on it. |
| **CLI binary → `horizon` package** | ~170 lines; only needs `build_function_map` + JSON writers + clap. |
| **UI/server → new crate** | New code; depend on `horizon-map` for load-from-JSON, optionally `horizon-engine` for live runs. |
| **Keep stages inside one engine crate** | Matches how [`pipeline.rs`](../crates/horizon-engine/src/pipeline.rs) already shares extract→index between CLI and correctness ([module docs L1–5](../crates/horizon-engine/src/pipeline.rs)). |

### Painful

| Seam | Why painful |
|---|---|
| **One crate per stage** | Shared intermediates must be hoisted; `modules`↔`extract` coupling; `resolve` unit tests build extract types by hand ([`resolve.rs` ~L2125+](../crates/horizon-engine/src/resolve.rs)). |
| **Put `json` in the engine only** | UI then either duplicates serde helpers or depends on the engine (and likely `ra_ap_syntax`) just to call `map_from_slice`. Prefer `json` with `map`. |
| **Leave correctness in the public lib without a decision** | It is measurement code that reaches into `pub(crate)` pipeline APIs; it is not part of the product library a UI needs. |

### Where `json.rs` belongs

With the contract types in `horizon-map`. Serialization is part of the consumer contract documented in [`docs/json-output.md`](json-output.md) (“Rust types live in `src/map.rs`”). Helpers are tiny and have no analysis logic. The UI’s “load a saved map” path is exactly `map_from_slice` ([`json.rs` L44–46](../crates/horizon-map/src/json.rs)).

### Where `correctness.rs` belongs

Prefer a **separate package** `horizon-correctness` (lib + `measure_correctness` binary), **or** keep it as a module inside `horizon-engine` behind the same crate so `pub(crate)` pipeline access stays free.

Do **not** make it an `example/` — examples are awkward for clap subcommands and standing CI (`tests/correctness_measurement.rs` imports `horizon::correctness`). Do **not** put it in the UI crate.

If split out: expose from the engine either (a) `pub use` of `extract_repository` / `resolve_index_for`, or (b) a single `pub fn collect_call_outcomes`-style API moved to engine and re-exported. Option (b) is cleaner.

### Confining `ra_ap_syntax`

| Crate | Needs `ra_ap_syntax`? |
|---|---|
| `horizon-map` | **No** |
| `horizon-engine` | **Yes** — `parse`, `extract`, `modules` |
| `horizon` (CLI) | No direct dep (transitive via engine) |
| `horizon-server` (JSON-only mode) | **No**, if it only depends on `horizon-map` |
| `horizon-server` (live analyse) | Yes, transitive via engine |

`discover` and `resolve` do not import `ra_ap_syntax` today, but they cannot form their own crates without the extract intermediates, so the practical confinement boundary is **engine vs map**, not discover-vs-parse.

**Compile-time implication:** `ra_ap_syntax` 0.0.x is a heavy rust-analyzer extract. Keeping it out of the UI crate’s dependency tree (when serving saved JSON) saves that compile and keeps UI iteration fast. Live analysis in-process still pulls it in for that feature/binary. Out-of-process alternative: UI shells out to the `horizon` CLI and only depends on `horizon-map` — zero `ra_ap_syntax` in the server crate always.

---

## Breakage and migration hazards

### Workspace `exclude` for fixtures

Today both `[package]` and `[workspace]` set `exclude = ["tests/fixtures"]` ([`Cargo.toml` L7–12](../Cargo.toml)). Documented further in [`tests/fixtures/README.md`](../tests/fixtures/README.md) (L636–659).

Under a **virtual** root manifest:

```toml
[workspace]
resolver = "2"
members = ["crates/horizon-map", "crates/horizon-engine", "crates/horizon", "crates/horizon-server"]
exclude = ["tests/fixtures"]
```

- Root `exclude` still prevents Cargo from treating fixture manifests as workspace members when discovered from the root.
- Fixture crates already declare their own `[workspace]` tables — keep that; `workspace.exclude` alone is not enough for `cargo check --manifest-path tests/fixtures/...`.
- If fixtures move under `crates/horizon-engine/tests/fixtures/`, update **`discover::is_nested_fixture_manifest`** ([`discover.rs` L244–251](../crates/horizon-engine/src/discover.rs)), which hard-codes the path segments `tests` then `fixtures` under the analysed `repo_root`. Self-analysis of the Horizon repo depends on that skip ([docs L41–45](../crates/horizon-engine/src/discover.rs), requirements doc).

**Recommendation:** leave `tests/fixtures/` at the **repository root** so the self-map skip path and docs stay valid.

### `CARGO_MANIFEST_DIR` and relative paths

All integration tests resolve fixtures via `env!("CARGO_MANIFEST_DIR")`:

- [`tests/phase1_single_file.rs` L7–8](../tests/phase1_single_file.rs)
- [`tests/phase2_modules.rs`](../tests/phase2_modules.rs), [`phase3_imports.rs`](../tests/phase3_imports.rs), [`phase4_multicrate.rs`](../tests/phase4_multicrate.rs), [`doc_comments.rs`](../tests/doc_comments.rs), [`correctness_measurement.rs` L11](../tests/correctness_measurement.rs)

If tests live in `crates/horizon-engine/tests/`, then `CARGO_MANIFEST_DIR` becomes that crate’s directory — either move fixtures next to them **or** join up to the workspace root (`../..` / `CARGO_WORKSPACE_DIR` once stabilized, or an env set in `.cargo/config.toml`).

`measure_correctness` defaults `fixtures_dir` to the **cwd-relative** string `"tests/fixtures"` ([`measure_correctness.rs` L37, L73](../crates/horizon-correctness/src/bin/measure_correctness.rs)). That breaks if the binary is run from `crates/horizon/` rather than the repo root. Fix: default via a path derived from a compile-time workspace root, or document “run from repo root”, or pass an absolute default in the binary crate using `env!("CARGO_MANIFEST_DIR")` + walk-up.

No uses of `std::env::current_dir` in `src/` or `tests/*.rs`. `thiserror` unused (safe to drop or leave for future errors).

### Docs that go stale

| Doc | What to update after the move |
|---|---|
| [`README.md`](../README.md) | `cargo run --bin horizon`, library path `use horizon::…` → package/crate names |
| [`docs/json-output.md`](json-output.md) | Links to `src/map.rs` |
| [`docs/correctness-measurement.md`](correctness-measurement.md) | Paths to `src/correctness.rs`, `src/pipeline.rs`, bin |
| [`docs/requirements/rust-function-map.md`](requirements/rust-function-map.md) | Links into `src/`; self-map fixture skip narrative |
| [`tests/fixtures/README.md`](../tests/fixtures/README.md) | Root `Cargo.toml` exclude snippet |
| [`docs/deferred-scope.tex`](deferred-scope.tex) / report tex | Fixture path mentions (lower priority) |

### `cargo run` ergonomics

- No `default-run` today ([`Cargo.toml`](../Cargo.toml)); workspace will need `default-members = ["crates/horizon"]` (and maybe the server) so `cargo run` from the root still builds the CLI.
- Prefer `cargo run -p horizon -- …` and `cargo run -p horizon-correctness --bin measure_correctness -- …`.
- `cargo install --path crates/horizon` (or publish package `horizon`) preserves the binary name.

### Integration-test ownership

Phase/oracle tests belong with **`horizon-engine`** (they call `build_function_map`). Contract round-trip tests in [`json.rs` `#[cfg(test)]`](../crates/horizon-map/src/json.rs) move with `horizon-map`.

---

## Three layout options

### Coarse — types + engine + CLI + UI

```text
crates/
  horizon-map/          map.rs, json.rs
  horizon-engine/       discover, modules, parse, extract, resolve, pipeline, lib glue, correctness?
  horizon/              bin/horizon.rs
  horizon-server/       axum + static assets (new)
tests/fixtures/         repo root
```

| | |
|---|---|
| **Edges** | server → map; server → engine (live); CLI → engine; engine → map |
| **External deps** | map: serde*; engine: ra_ap_syntax, anyhow, serde_json; CLI: clap; server: axum, tower, … |
| **Migration cost** | ~11 lib modules move into 2 crates; 2 bins into their packages; rewrite `use crate::` → `horizon_map::` / internal mods; ~6 test files’ manifest-dir paths; root becomes virtual manifest. Roughly **one focused PR**. |
| **Buys** | Clear product boundaries; UI can avoid engine for JSON-only; matches owner intent. |
| **Downside** | Correctness still muddied if left inside engine’s public API. |

Honestly sufficient for the UI work. Slightly worse than medium only because correctness identity is ignored.

### Medium — coarse + correctness (+ contract already separate) ★ recommended

Same as coarse, plus:

```text
crates/horizon-correctness/   correctness.rs + bin/measure_correctness.rs
```

and `horizon-map` definitely owns both `map` and `json`.

| | |
|---|---|
| **Edges** | correctness → engine (+ map); engine does **not** depend on correctness |
| **External deps** | correctness: clap, serde_json, anyhow (no direct ra_ap_syntax) |
| **Migration cost** | Coarse cost + promote `pipeline` helpers needed by harness to `pub` (or move `collect_call_outcomes` into engine); point `tests/correctness_measurement.rs` at `horizon_correctness` or keep thin re-export tests on engine. **~1–2 PRs**. |
| **Buys** | Product lib (`horizon-engine`) stays free of LSIF/oracle code; harness can evolve (extra deps later) without touching the analysis crate’s public surface. |
| **Over-engineering?** | No — correctness is already a separate binary with its own docs and CI story. |

### Fine — one crate per pipeline stage

```text
crates/
  horizon-map/
  horizon-facts/        PendingCall, Import, TypeDef, FileFacts, ItemVisibility  (hoisted)
  horizon-discover/
  horizon-parse/        ra_ap_syntax wrapper
  horizon-extract/      → facts, parse, map
  horizon-modules/      → extract/facts, parse, discover, map
  horizon-resolve/      → facts, map
  horizon-engine/       pipeline + build_function_map glue
  horizon/
  horizon-server/
  horizon-correctness/
```

| | |
|---|---|
| **Edges** | Long DAG; almost every analysis change touches 2–4 Cargo.toml files |
| **External deps** | `ra_ap_syntax` in parse + extract + modules (three crates); discover/resolve free of it but still useless alone |
| **Migration cost** | Hoist ~8 intermediate types; fix `modules`↔`extract` sharing; rewrite hundreds of `use` paths; re-home ~2400-line `resolve` tests; publish versioning noise. **Multi-PR, high churn, low payoff.** |
| **Buys** | Theoretical parallel compiles of resolve vs extract — irrelevant at this repo size (~7.5k LOC lib). |
| **Verdict** | **Over-engineering. Would hurt.** Do not do this unless an external crate consumer needs a single stage in isolation (none exists). |

---

## Recommended migration order

Each step should leave `cargo test` green.

1. **Introduce virtual workspace** at repo root; move current package to `crates/horizon` **without** splitting modules yet (`members = ["crates/horizon"]`, keep `exclude = ["tests/fixtures"]`). Fix any path assumptions. Confirm fixtures still skipped on self-map.
2. **Extract `horizon-map`** (`map.rs` + `json.rs`). Engine (still the old package, renamed) depends on it; re-export map types from engine if you want a temporary compatibility façade.
3. **Rename analysis package to `horizon-engine`**; slim `crates/horizon` to the CLI binary only (`horizon = { path = "../horizon-engine" }`). Set workspace `default-members = ["crates/horizon"]`.
4. **Split `horizon-correctness`** (optional but recommended): move module + binary; expose minimal harness API from engine; retarget `tests/correctness_measurement.rs`.
5. **Add `horizon-server`** depending on `horizon-map` first (load JSON + static UI). Add optional `horizon-engine` dependency when wiring live analyse.
6. **Docs pass:** README, json-output, correctness-measurement, fixtures README, requirements links.
7. Drop unused `thiserror` (or start using it) and stop depending on `clap` from the engine.

Do **not** start with fine-grained stage crates. Do **not** move `tests/fixtures` under `crates/` unless you simultaneously update `is_nested_fixture_manifest`.

---

## Appendix: module → recommended crate

| Current file | Lines (approx.) | Recommended crate |
|---|---:|---|
| `map.rs` | 411 | `horizon-map` |
| `json.rs` | 235 | `horizon-map` |
| `discover.rs` | 405 | `horizon-engine` |
| `modules.rs` | 563 | `horizon-engine` |
| `parse.rs` | 20 | `horizon-engine` |
| `extract.rs` | 1422 | `horizon-engine` |
| `resolve.rs` | 2380 | `horizon-engine` |
| `pipeline.rs` | 216 | `horizon-engine` |
| `lib.rs` (orchestration) | 220 | `horizon-engine` |
| `correctness.rs` | 864 | `horizon-correctness` (or engine) |
| `bin/horizon.rs` | 153 | `horizon` |
| `bin/measure_correctness.rs` | 407 | `horizon-correctness` |
| `tests/*.rs` (non-fixture) | ~1.2k | `horizon-engine` (integration) / correctness tests with harness |
