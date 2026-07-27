# Correctness measurement

**Status:** harness landed; figures are snapshots  
**Date:** 26 July 2026 (refreshed)  
**Conclusions folded into:** [`docs/requirements/rust-function-map.md`](requirements/rust-function-map.md) (Closed questions)

## Why this exists

Coverage (“how many calls resolved”) is not correctness. A confidently wrong
`Resolved` edge is worse than an honest `Conflict`. This document records a
**re-runnable** measurement approach and the numbers from one snapshot.

## Methodology (what we chose, and why)

Three complementary oracles:

| Approach | Role | Chosen? |
|---|---|---|
| **Hand-annotated fixtures** (`expected-edges.json`) | Certain ground truth on hard cases, including non-compiling crates | **Yes — primary standing check** |
| **Adversarial fixture** (`adversarial-resolution/`) | Directly probes confident wrong edges | **Yes** |
| **rust-analyzer LSIF as breadth oracle** on Horizon itself | Compiler-grade name resolution over real code | **Yes — optional, needs `rust-analyzer`** |
| SCIP protobuf index | Same semantic index as LSIF | Available (`rust-analyzer scip`) but LSIF is JSON and needs no extra crate |

**Why not SCIP-only:** filtering SCIP/LSIF to Horizon’s free-function universe is
most of the work; LSIF monikers already expose `::impl::` for methods/associated
items, which makes that filter tractable. Fixture oracles remain necessary for
deliberately broken code (SCIP/LSIF need a toolchain and a compiling-ish project).

**Why not fixtures-only:** they cannot speak to false-positive rate on a real
codebase’s hundreds of edges.

The harness never changes the resolver’s answers; it only compares them. Extract
and resolve-index construction go through the shared [`src/pipeline.rs`](../src/pipeline.rs)
path used by `build_function_map`, so the harness measures the same pipeline the
CLI runs.

## How to re-run

```bash
# Standing check (CI-friendly): fixture + adversarial oracles
cargo test --test correctness_measurement
cargo run --bin measure_correctness -- fixtures

# Exclusion samples on a repo (default: .)
cargo run --bin measure_correctness -- exclusions .

# Self-map + LSIF precision (requires rust-analyzer component)
rustup component add rust-analyzer   # once
cargo run --bin measure_correctness -- self-map . --generate-lsif

# Or reuse an existing index (must be generated against the same tree as the map)
cargo run --bin measure_correctness -- self-map . --lsif target/correctness/horizon.lsif

# Everything → target/correctness/report.json
cargo run --bin measure_correctness -- all --generate-lsif
```

Implementation lives in:

- `src/correctness.rs` — oracle check, exclusion audit, LSIF compare  
- `src/pipeline.rs` — shared extract → resolve-index path  
- `src/bin/measure_correctness.rs` — CLI  
- `tests/correctness_measurement.rs` — always-on fixture tests  
- `tests/fixtures/*/expected-edges.json` — hand annotations  
- `tests/fixtures/adversarial-resolution/` — false-positive traps  

## Snapshot results (26 July 2026, refreshed)

> Counts shift when `src/**` or discovery rules change. Re-run the harness; do
> not treat the numbers below as frozen product KPIs.

### 1. Fixture oracle (certain)

| Fixture | Checked | Passed | False positives |
|---|---:|---:|---:|
| `phase1-single-file` | 6 | 6 | 0 |
| `glob-ambiguity` | 1 | 1 | 0 |
| `glob-resolved` | 1 | 1 | 0 |
| `impl-free-globs` | 8 | 8 | 0 |
| `use-tree-syntax` | 5 | 5 | 0 |
| `exclude-non-functions` | 6 | 6 | 0 |
| `adversarial-resolution` | 9 | 9 | 0 |
| `macro-hidden-calls` | 10 | 10 | 0 |
| `renamed-path-dep` | 1 | 1 | 0 |
| `proc-macro-crate` | 2 | 2 | 0 |
| `cross-crate-facade` | 10 | 10 | 0 |
| **Total** | **59** | **59** | **0** |

**False-positive count on fixtures: 0.**  
Every edge that must be `Conflict` stayed a conflict (glob clash, cfg twins,
cross-crate glob facades); no case resolved to a guessed winner. Local
definitions beat globs; rename chains landed on the defining function; same
names at several module depths stayed distinct; Cargo rename aliases,
proc-macro free functions, and cross-crate `pub use` facades (plain / renamed /
multi-crate chain / glob) resolved to the defining `FunctionId`.

### 2. LSIF precision on Horizon (breadth)

On an atomically generated map + LSIF pair (after normalizing Horizon’s
`{crate}[bin]::…` ids and nested-function path spelling against rust-analyzer
monikers):

| Metric | Value |
|---|---|
| Resolved edges compared | 411 |
| LSIF-agreed (same free-function moniker) | 411 |
| **False positives** (Horizon Resolved ≠ LSIF free-fn moniker) | **0** |
| Ordinary cohort (non-macro) | 374 compared / 374 matched / **0 FP** |
| Macro-recovered cohort (`from_macro`) | 37 compared / 37 matched / **0 FP** |
| Unmatched (no same-name LSIF moniker in span) | 0 |
| LSIF said `::impl::` only | 0 |
| Self-map conflicts / unresolved | 0 / 23 (closure calls inside `assert_eq!`, not FPs) |

**False-positive rate among LSIF-comparable edges: 0 / 411 = 0%.**  
**Macro-recovered FP rate: 0 / 37 = 0%.**

Earlier noisy “FP” reports during harness development were matcher artifacts
(overlapping ranges for `Some` / fields / macros on the same line, stale LSIF
vs a moving tree, or `[bin]` id spelling). The harness now requires a same-name
moniker before scoring an FP, and self-map+LSIF must be generated against the
same tree.

### 3. Recall / exclusion audit

| Lens | Figure | Notes |
|---|---|---|
| Fixture expected edges present | **59 / 59 (100%)** | Includes required `Absent` drops |
| Self-map resolved edges confirmed by LSIF | **411 / 411 (100%)** | Same-name moniker agreement after id normalization |
| Deliberate drops that should stay dropped | **6 / 6** on `exclude-non-functions` | `Ok`, `Target::Ready`, `LocalId::make`, `Vec::new` absent; `mystery` Unresolved; `helper` Resolved |

**Exclusion swallowing (sampled):** on Horizon itself the harness recorded
**100 external / 204 constructor / 133 associated** drops (this snapshot).
Stratified samples were inspected; heuristic “suspicious drop” count was **0**.

### 4. Confidence

**High** for “Horizon is not systematically guessing on free functions in the
cases we can check” — including the LSIF `from_macro` cohort. **Lower** for
“every drop is perfectly categorized” and for behaviour on large or unfamiliar
corpora (hand sample in scale-validation; no full external LSIF breadth).

## False positives with evidence

**None found** in this measurement pass.

(If a future run prints `FP` lines from `measure_correctness self-map`, each
line lists call site, Horizon target, and LSIF moniker — that is the evidence
block to paste here.)

## What remains unmeasured

1. **Cross-crate precision** beyond the multi-crate integration tests
   (LSIF self-map is same-repo; foreign FunctionIds need a multi-crate LSIF
   filter). Facade following is covered by the `cross-crate-facade` oracle;
   newly resolved facade edges on large corpora still need hand spot-checks.
2. **Visibility / privacy** edges that rustc would reject (`E0603`) but Horizon
   still draws (by design for direct edges) — not a false positive under the
   product rule, but not compared to rustc accept/reject.
3. **Nested-function recursion** by unqualified name — observed while writing
   the harness (`walk` inside `walk` became `Unresolved`). Not fixed here;
   product code currently avoids that pattern. Worth a dedicated fixture.
4. **UpperCamelCase module mis-classified as associated** (documented risk in
   requirements) — no specimen in the adversarial set yet. Segments that fail
   the heuristic now become `Unresolved` instead of silent drops.
5. **Large external corpora** (e.g. ruff) — LSIF/SCIP breadth at that scale is
   still open.
6. **Macro-recovered calls to local closures / bindings** surface as
   `Unresolved` (e.g. `assert_eq!(by_local(…))` in unit tests). Not false
   positives; indirect-call analysis remains a non-goal. The LSIF harness
   breaks out `from_macro` vs `ordinary` cohorts so recovery precision is
   scored separately.

## Scope note for maintainers

Measurement code must not quietly change resolver answers. Prefer additive
changes under `src/correctness.rs` / the measure binary / fixtures. Edit
`resolve` / `extract` / `discover` / `modules` only for a proven bug with a
surgical fix, and re-run this harness afterwards. Keep the harness on the
shared `pipeline` path — do not reintroduce a twin walk.
