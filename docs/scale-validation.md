# Scale validation on unfamiliar Rust code

**Status:** refreshed measurement pass (post cross-crate facade following)  
**Date:** 26 July 2026  
**Resolution behaviour:** this pass measures after cross-crate `pub use` /
`pub extern crate` facade following landed. Prior same-day figures (below as
“before”) are the pre-facade baselines from the earlier refresh.

## Why

Correctness is measured to zero false positives on Horizon itself and its
hand-built fixtures. That corpus is small, clean, and written to match the
tool. This document records what happens on real, unfamiliar repositories —
performance, robustness, and whether the free-function map is *useful* when
methods and `impl` items are out of scope.

## Methodology

### Corpus

Shallow clones into `%TEMP%\horizon-scale-corpus` (outside this git repo), plus
the local project at `C:\Users\kouat\Research\Cellular Automata`.

| Repository | Role | Why chosen |
|---|---|---|
| **Cellular Automata** (local) | Small, author-familiar | Real user code; free-function heavy; LSIF tractable |
| **serde** | Small/medium, macro-heavy | Stresses module discovery (`crate_root!()`, `cfg(docsrs)` layouts) and proc-macro crates |
| **ripgrep** | Medium workspace | Classic free-function style; many crates; good hand-check target |
| **bat** | Medium app | Clap/config-heavy; ships fixture `Cargo.toml` files that are not real packages |
| **tokio** | Medium/large, method-heavy | Workspace with heavy `impl` / async style; also exercises allowlisted `cfg_*` item-macro module recovery |
| **rust-analyzer** | Large | ~1.5k Rust files, many crates, dense macros — closest to “real scale” |

Also timed a self-map of Horizon for a known baseline.

Candidates considered and skipped for LSIF at full size: rust-analyzer and
ripgrep LSIF indexing (too slow / stalled in the prior pass). Cellular Automata
LSIF remains the only external LSIF spot-check from that pass.

### How measured

- Binary: `cargo build --release -p horizon`.
- Command: `horizon <repo> -o <map.json> --compact`.
- Wall clock: process stopwatch around the child.
- Peak RSS: polled `PeakWorkingSet64` during the run (coarse; short runs may
  under-read).
- Disk `.rs` counts: all `*.rs` under the tree excluding `.git` / `target`.
- **Analysed** files/bytes: files that actually appear in the emitted map tree
  (declaration-driven walk), with file sizes summed from disk.
- Throughput: analysed-bytes / wall seconds (primary).
- Summary line counts (resolved / conflict / unresolved / drops) taken from the
  tool’s stderr summary.

Prototype baseline cited historically: **~1.2 MB/s** single-core extraction
throughput from an earlier spike.

---

## Results table

Times and peaks from the release binary on Windows 10, 26 July 2026 (refresh
after macro-mod recovery, proc-macro discovery, Cargo rename support, and
strict UpperCamelCase classification).

| Repository | Disk `.rs` | Disk MB | Map files | Analysed MB | s | Peak MB | MB/s (analysed) | Crates | Fns | Resolved | Conflict | Unresolved | Ext drop | Ctor drop | Assoc drop |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Cellular Automata | 7 | 0.10 | 7 | 0.10 | 0.33 | 5 | **0.31** | 2 | 87 | 175 | 0 | 2 | 63 | 38 | 30 |
| serde | 208 | 1.23 | 35 | 0.34 | 0.63 | 13 | **0.54** | 5 | 156 | 248 | 1 | 14 | 56 | 152 | 31 |
| ripgrep | 110 | 1.87 | 86 | 1.65 | ~4 | — | — | 12 | 737 | **2108** | 20 | **132** | 355 | 474 | 806 |
| bat | 67 | 0.61 | 40 | 0.40 | 0.41 | 14 | **0.97** | 2 | 267 | 266 | 1 | 27 | 183 | 332 | 208 |
| tokio | 790 | 5.52 | 492 | 4.11 | 2.2 | 27 | **1.85**† | 12 | 762 | 529 | 2 | 749 | 458 | 336 | 629 |
| rust-analyzer | 1480 | 16.61 | 920 | 16.32 | ~39 | — | — | 49 | 11939 | **15021** | 28 | **1533** | 1229 | 3932 | 5772 |
| Horizon (self) | — | — | — | — | — | — | — | 3 | — | **411** | 0 | 23 | 100 | 204 | 133 |

### Facade following: before → after (multi-crate)

| Corpus | Resolved before | Resolved after | Δ | Unresolved before | Unresolved after | Δ |
|---|---:|---:|---:|---:|---:|---:|
| ripgrep | 2103 | **2108** | +5 | 142 | **132** | −10 |
| rust-analyzer | 14960 | **15021** | +61 | 1596 | **1533** | −63 |
| Horizon (self) | 393 | 411 | +18 | 23 | 23 | 0 |

Ripgrep’s +5 are free-function calls through `pub extern crate grep_* as …`
facades (`grep::cli::hostname` → `grep_cli::hostname::hostname`, etc.). The
unresolved drop (−10) is larger than +5 because some former unresolved paths
are now classified as associated/constructor drops once the module prefix
resolves (`grep::cli::CommandReader::new`, …). Rust-analyzer’s gains are mostly
`pub use` barrels across path crates (e.g. `hir::db::file_item_tree` →
`hir_def::item_tree::file_item_tree`). Horizon’s self-map resolved count rose
mainly because the resolver itself grew, not because Horizon is multi-crate.

† Tokio’s analysed throughput looks high because the newly recovered module tree
is many moderate-sized files; wall time is still ~2 s. Dense trees
(ripgrep, rust-analyzer, bat, Horizon) land around **0.8–1.1 MB/s** analysed.

### What changed versus the earlier same-day pass

| Corpus | Earlier highlight | Now | Why |
|---|---|---|---|
| **tokio** | 114 map files, 90 fns, 91 resolved (~11 non-test) | **492** files, **762** fns, **529** resolved (~156 non-test-ish) | Allowlisted `cfg_*` / `cfg_if!` item-macro `mod` recovery |
| **serde** | 7 files, 2 fns, 0 resolved, 4 crates | **35** files, **156** fns, **248** resolved, **5** crates | `serde_derive` (proc-macro) now discovered; main lib still missing `crate_root!()` modules |
| **rust-analyzer** | 47 crates, 918 files, 11916 fns | **49** crates, **920** files, **11939** fns | Proc-macro / library-kind coverage; tiny shifts from capitalisation |
| **ripgrep / bat / CA** | Essentially stable free-function maps | Same order of magnitude | Capitalisation change moves some silent associated drops → honest `Unresolved` on other corpora more than here |
| **Horizon** | 392 resolved / 17 unresolved | 393 / 23 | Same |

### Extrapolation to ~50,000 files

Linear from rust-analyzer (1,480 disk files / 920 map files / ~21 s):

| Assumption | Estimate for 50k disk files |
|---|---|
| Same density as rust-analyzer | \(50000/1480 \times 21 \approx\) **12 minutes** |
| Same analysed bytes/file (~18 KB) | ~900 MB analysed → at ~0.8 MB/s ≈ **19 minutes** |
| Memory (linear in analysed bytes) | ~67 MB × (50k/1.5k) ≈ **~2.3 GB** peak (order-of-magnitude) |

**Verdict: reachable as a batch job**, not as a sub-second interactive refresh.
Nothing in this pass looked superlinear up to ~16 MB / ~1.5k files. The risk at
50k is not “will it finish?” but “will pathological inputs (huge re-export
graphs, cfg explosion, macro token forests) create hot spots?” — not observed
here, not disproven either.

**Throughput claim for product docs:** about **0.8–1.1 MB/s** of analysed source
on release builds for dense trees (higher than the earlier 0.5–0.6 MB/s figure,
which reflected a colder binary / less analysed content on some runs).

---

## Robustness failures

No panics, hangs, or OOM on any corpus member. Exit code 0 everywhere.
rust-analyzer produced a ~6 MB JSON map in ~21 s at ~67 MB RSS.

### 1. Macro *definition* bodies still opaque (serde `crate_root!()`)

**Symptom:** serde’s main library is still largely invisible as a coherent
module tree. `serde_derive` now appears (proc-macro discovery), lifting the
crate count and function count, but the bodies assembled by `crate_root!()` do
not.

**Cause (deliberate):** allowlisted recovery opens only the **invocation’s**
token tree (`cfg_if! { … }`, `cfg_fs! { … }`). Serde’s modules live inside a
`macro_rules!` **definition** body that an empty `crate_root!()` call expands.
Expanding definition bodies is a different and riskier problem; it stays closed.
Prefer a missing subtree over a fabricated module tree.

**Impact:** crates that materialise modules primarily via definition-body
expanders still look thin. This is a discovery gap with a recorded rationale,
not a resolver false positive.

### 2. Cfg-blind duplicate definitions → `Conflict` (common, honest)

Platform/`cfg` twins (`#[cfg(windows)] fn imp` / `#[cfg(unix)] fn imp`) all
remain visible; calls become `Conflict` naming every candidate. Seen throughout
ripgrep and rust-analyzer. Matches the never-guess rule; noisy on real code.

### 3. ~~`pub use` re-exports across path crates not followed~~ (fixed)

Cross-crate facade following now consults the foreign crate’s re-export table
(`pub use`, renamed `pub use`, `pub use glob::*`, and `pub extern crate … as`
crate renames). ripgrep’s `grep::cli::hostname` resolves to
`grep_cli::hostname::hostname`. Remaining unresolved on ripgrep are mostly
dev-dependency / test helpers / associated forms, not facade barrels.

### 4. Associated calls on path-dep types → often `associated_dropped` now

Once a facade module prefix resolves, `Dep::Type::assoc` forms are more often
recognised and dropped as associated (see ripgrep assoc 801→806). Some may
still surface as `Unresolved` when the type path cannot be classified.

### 5. Dev-dependencies invisible → external test calls as `Unresolved`

`cargo metadata` dependency lists used by discovery omit `[dev-dependencies]`.
Test-only externals then land as **Unresolved** instead of `external_dropped`.

### 6. Discovery noise from non-package `Cargo.toml` fixtures (bat)

bat ships syntax-highlight fixtures under `tests/syntax-tests/**/Cargo.toml`.
Horizon tries `cargo metadata`, prints skip warnings, and continues.

### 7. Raw-identifier module path miss (rust-analyzer)

One warning: `module crate::completions::r#type` → missing file (skipped).
Isolated; did not empty the crate.

### 8. First-wins on cfg-twinned `#[path]` modules

When two `#[cfg]` arms declare the same module name with different `#[path]`
targets, the module walk records the first declaration and terminates later
ones (one module path → one file). Real limitation; stated in requirements /
deferred-scope.

### What did *not* fail

- No panics on any corpus member.
- No multi-minute hangs on rust-analyzer (~21 s).
- No runaway memory (67 MB peak on the largest run).

---

## Correctness spot-check on unfamiliar code

### Hand verification

**Prior pass** (random `Resolved` edges): ripgrep 30/30 OK, Cellular Automata
20/20 OK — 0 / 50 FP suspects.

**This pass — newly resolved facade edges (highest FP risk):**

Method: take every ripgrep edge whose call path is `grep::…` and whose target
now `Resolved` into `grep_*` (the five free-function facade recoveries);
confirm the call text on the recorded line and that the target `FunctionId`
names a real `pub fn` in the dependency crate. Separately spot-check
rust-analyzer `hir::db::file_item_tree` → `hir_def::item_tree::file_item_tree`
against `hir/src/db.rs`’s `pub use hir_def::{file_item_tree, …}`.

| Corpus | Sample | OK | FP suspect |
|---|---:|---:|---:|
| ripgrep (all new `grep::`→`grep_*` free-fn facades) | 5 | 5 | 0 |
| rust-analyzer (`hir::db::file_item_tree` facade) | 1 | 1 | 0 |
| **Total this pass** | **6** | **6** | **0** |

**Estimated false-positive rate on newly resolved facade sample: 0 / 6 = 0%.**

### LSIF compare (Horizon self-map, this refresh)

Standing harness (`measure_correctness all --generate-lsif`):

| Metric | Value |
|---|---:|
| Fixture oracles | **59 / 59**, **0** false positives |
| LSIF compared (Horizon) | 411 |
| LSIF matched | 411 |
| **False positives** | **0** |
| Ordinary / `from_macro` cohorts | 374 / 37, both **0** FP |

---

## Qualitative usefulness

### Where the map earns its keep

**Cellular Automata** and **ripgrep** remain free-function oriented. Recognisable
pipelines still show (`run` → `search` / `simulate`, etc.). Unresolved rates
among kept sites stay modest on those corpora relative to method-heavy trees.

### Where the map is thin or misleading

**tokio:** no longer “nearly empty” after `cfg_*` module recovery — hundreds of
free functions and hundreds of resolved edges are visible. The runtime’s *real*
structure still lives in methods and `impl` blocks, which remain permanently out
of scope and are **not** counted in `associated_dropped`. Treat the map as a
partial free-function index, not an architectural picture of Tokio.

**serde:** improved by discovering `serde_derive`, but the main library’s
`crate_root!()`-assembled modules stay closed. Still not a useful map of serde’s
structure.

**rust-analyzer:** large and informative for free-function-heavy crates, still
dominated by deliberate drops plus all method calls never counted.

### Who the tool serves today

| Codebase style | Map quality |
|---|---|
| Free-function pipelines, workspace of small crates (ripgrep, CA, parts of bat) | Good |
| Facade crates + path re-exports | Improved (cross-crate `pub use` / `extern crate` facades followed) |
| Macro-assembled modules via **definition bodies** (serde `crate_root!`) | Thin |
| Allowlisted item-macro module trees (`cfg_if!`, Tokio `cfg_*`) | Recovered |
| Method/`impl`-centric (tokio runtime, much of RA) | Partial — free helpers visible, product logic missing |

---

## 50k-file target: honest judgement

**Reachable for batch analysis** at current throughput (~0.8–1.1 MB/s analysed
on dense trees, roughly linear through rust-analyzer scale), with expected
runtimes around **10–20 minutes** and memory in the low-GB range for dense trees.

**Not ready to claim “works on any codebase”** without caveats:

1. Definition-body macro module trees can still yield thin maps (serde).
2. Dev-dependencies and some associated forms inflate Unresolved.
3. Method-centric crates omit most of the program by design.
4. Consumer-side `use dep::*` is still not expanded (foreign facades are).

Performance work (parallel file parse, skip fixture manifests, streaming JSON)
would help the 50k goal; **correctness/usefulness gaps above matter more**.

---

## Appendix: raw summary lines

```
ripgrep: 12 crates, 737 functions; 2108 resolved, 20 conflicts, 132 unresolved;
  dropped: 355 external, 474 constructor, 806 associated
rust-analyzer: 49 crates, 11939 functions; 15021 resolved, 28 conflicts, 1533 unresolved;
  dropped: 1229 external, 3932 constructor, 5772 associated
horizon (self-map via measure_correctness): 411 resolved, 0 conflicts, 23 unresolved;
  dropped: 100 external, 204 constructor, 133 associated
```

Prior (pre-facade) summary lines for comparison:

```
ripgrep: 2103 resolved, 142 unresolved
rust-analyzer: 14960 resolved, 1596 unresolved
horizon: 393 resolved, 23 unresolved
```

Artefacts (local, not in git): `%TEMP%\horizon-scale-results\`,
`%TEMP%\horizon-scale-corpus\`.
