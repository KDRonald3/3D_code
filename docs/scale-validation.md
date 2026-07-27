# Scale validation on unfamiliar Rust code

**Status:** refreshed measurement pass  
**Date:** 26 July 2026  
**Resolution behaviour:** unchanged by this pass — ran the tool and measured  
**Prior pass:** earlier the same day (pre macro-mod recovery, pre proc-macro
discovery, pre capitalisation tightening). Figures below supersede that table.

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

- Binary: `cargo build --release --bin horizon`.
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
| ripgrep | 110 | 1.87 | 86 | 1.65 | 1.8–1.9 | 26 | **0.87** | 12 | 737 | 2103 | 20 | 142 | 355 | 474 | 801 |
| bat | 67 | 0.61 | 40 | 0.40 | 0.41 | 14 | **0.97** | 2 | 267 | 266 | 1 | 27 | 183 | 332 | 208 |
| tokio | 790 | 5.52 | 492 | 4.11 | 2.2 | 27 | **1.85**† | 12 | 762 | 529 | 2 | 749 | 458 | 336 | 629 |
| rust-analyzer | 1480 | 16.61 | 920 | 16.32 | ~21 | 67 | **0.78** | 49 | 11939 | 14960 | 28 | 1596 | 1228 | 3932 | 5771 |
| Horizon (self) | 81 | 0.38 | 12 | 0.25 | 0.23 | 10 | **1.06** | 3 | 186 | 393 | 0 | 23 | 96 | 195 | 126 |

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

### 3. `pub use` re-exports across path crates not followed

ripgrep’s `rg` binary still leaves facade paths like `grep::cli::hostname`
**Unresolved** when the definition lives in `grep-cli` behind a `pub use`.
Cross-crate resolution works for direct definitions in the dependency, not
through re-export barrels.

### 4. Associated calls on path-dep types → `Unresolved` instead of drop

`Dep::Type::assoc` forms where the type is only reached as a re-export can
surface as `Unresolved` (`no public module …`) rather than
`associated_dropped`. Inflates unresolved; not a false `Resolved`.

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

### Hand verification (prior pass; still the best unfamiliar-code evidence)

Method: random sample of `Resolved` edges; require the callee’s final path
segment to appear on the recorded source line, and the target `FunctionId` to
exist as a node in the same map.

| Corpus | Sample | OK | FP suspect | Target node missing |
|---|---:|---:|---:|---:|
| ripgrep | 30 | 30 | 0 | 0 |
| Cellular Automata | 20 | 20 | 0 | 0 |
| **Total** | **50** | **50** | **0** | **0** |

**Estimated false-positive rate on this sample: 0 / 50 = 0%.**  
Not a substitute for LSIF breadth, but evidence on *unfamiliar* code.

### LSIF compare (Horizon self-map, this refresh)

Standing harness (`measure_correctness all --generate-lsif`):

| Metric | Value |
|---|---:|
| Fixture oracles | **49 / 49**, **0** false positives |
| LSIF compared (Horizon) | 393 |
| LSIF matched | 393 |
| **False positives** | **0** |
| Ordinary / `from_macro` cohorts | 356 / 37, both **0** FP |

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
| Facade crates + path re-exports | Patchy (unresolved through barrels) |
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
2. Facade re-exports break cross-crate free-function resolution.
3. Dev-dependencies and some associated forms inflate Unresolved.
4. Method-centric crates omit most of the program by design.

Performance work (parallel file parse, skip fixture manifests, streaming JSON)
would help the 50k goal; **correctness/usefulness gaps above matter more**.

---

## Appendix: raw summary lines

```
cellular-automata: 2 crates, 87 functions; 175 resolved, 0 conflicts, 2 unresolved;
  dropped: 63 external, 38 constructor, 30 associated
serde: 5 crates, 156 functions; 248 resolved, 1 conflict, 14 unresolved;
  dropped: 56 external, 152 constructor, 31 associated
ripgrep: 12 crates, 737 functions; 2103 resolved, 20 conflicts, 142 unresolved;
  dropped: 355 external, 474 constructor, 801 associated
bat: 2 crates, 267 functions; 266 resolved, 1 conflict, 27 unresolved;
  dropped: 183 external, 332 constructor, 208 associated
tokio: 12 crates, 762 functions; 529 resolved, 2 conflicts, 749 unresolved;
  dropped: 458 external, 336 constructor, 629 associated
rust-analyzer: 49 crates, 11939 functions; 14960 resolved, 28 conflicts, 1596 unresolved;
  dropped: 1228 external, 3932 constructor, 5771 associated
horizon: 3 crates, 186 functions; 393 resolved, 0 conflicts, 23 unresolved;
  dropped: 96 external, 195 constructor, 126 associated
```

Artefacts (local, not in git): `%TEMP%\horizon-scale-results\`,
`%TEMP%\horizon-scale-corpus\`.
