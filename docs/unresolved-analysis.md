# Unresolved-call analysis (rust-analyzer & ripgrep)

**Status:** diagnostic snapshot  
**Date:** 26 July 2026  
**Commit measured:** `1404ac7`  
**Maps:** `%TEMP%\horizon-scale-results\rust-analyzer-after.json`,
`ripgrep-after.json` (post facade-following; summary counters match
[`scale-validation.md`](scale-validation.md))  
**Resolution behaviour:** unchanged — this document only classifies what the
tool already emits.

## Why

Correctness is zero false positives on fixtures and on Horizon’s own LSIF
cohort. On unfamiliar corpora the unresolved bucket is still ~9%
(rust-analyzer) / ~6% (ripgrep). An earlier self-map pass showed that most
“unresolved” items were constructors and associated functions mislabelled as
missing modules. This pass asks the same question of the large corpora:
**what is actually in those buckets?**

Dropped calls (`external_dropped` / `constructor_dropped` /
`associated_dropped`) never appear in the tree. A wrongly dropped free
function would be invisible in every resolved/unresolved statistic, so drop
buckets were sampled as well.

## Method

1. Group every `CallTarget::Unresolved` by reason-string pattern, then by a
   finer semantic tag derived from call path shape, enclosing `FunctionId`,
   and the **source line** (and surrounding definitions) in the corpus.
2. Verify each major tag by reading real call sites in
   `%TEMP%\horizon-scale-corpus\{rust-analyzer,ripgrep}`.
3. Dump all resolve outcomes (including drops) via a throwaway out-of-repo
   helper that calls `horizon::correctness::collect_call_outcomes` — same
   extract → resolve-index path as the CLI.
4. Do **not** change resolver code; do **not** treat reason strings as ground
   truth.

---

## 1. Distributions

### 1.1 rust-analyzer — unresolved by reason string (1,533)

| Count | Reason pattern |
|---:|---|
| **1,137** | `no free function \`…\` in module \`…\`` |
| **273** | `path \`…\` uses \`…\` which is not a module in this crate (from \`…\`)` |
| **70** | `import \`…\` in \`…\` did not resolve to a free function` |
| **49** | `no public module \`…\` in \`…\` for \`…\`` |
| **4** | `path \`…\` names a crate, not a function` |

Summary counters: resolved 15,021 · conflict 28 · unresolved 1,533 ·
external 1,229 · constructor 3,932 · associated 5,772.  
Of the 1,533 unresolved, **62** have `from_macro: true`.

### 1.2 rust-analyzer — unresolved by verified semantics

Reason strings collapse several different situations. Counts below are from
source-checked tagging of the same 1,533 sites (tags are mutually exclusive;
± a few from heuristic edge cases).

| Count | Semantic bucket | Verdict |
|---:|---|---|
| **~780** | Closure / local binding / parameter called as a function (`add_keyword`, `handle_trait`, `cb`, `ctor`, `default_hook`, …) | **Correct** — indirect; not a free-function edge |
| **~144** | Sibling / ancestor module used without `self::` / `super::` (parser `types::type_`, `expressions::block_expr`, …) | **Real gap** — Horizon does not walk enclosing modules for the first path segment |
| **~101** | Import of a **path-crate module** used as a prefix (`make::path_from_text`, `algo::least_common_ancestor`, `ast::attrs_including_inner`, …) | **Real gap** — cross-crate import targets that are modules are not returned as `Module` |
| **~109** | Nested `fn` (self-recursion ~70 + other nested calls ~39) | **Real gap** — nested functions are not indexed |
| **~65** | Test / fixture helpers with no indexed free `fn` | Mix of honest misses, macros, and cfg-gated helpers |
| **~70** | Explicit import that is not a free function (`unescape_char`, `extract_offset`, …) | Mix — often external/`std` items or non-fn bindings; reason is blunt but usually not a free-fn miss |
| **~43** | Cross-crate `Type::assoc` / variant reported as `no public module \`Type\`` (`syntax::SourceFile::parse`, `span::FileId::from_raw`, …) | **Miscategorised** — should be `associated_dropped` (or constructor), not unresolved |
| **~20–23** | Bare UpperCamelCase constructor / newtype (`SourceRootId(1)`, `IndentLevel(1)`, …) | **Miscategorised** — should be `constructor_dropped` |
| **~29** | Macro-recovered call to a test-local helper | **Correct** — no free function in the index |
| **~5** | Dev-dep / tooling crate as “not a module” (`test_utils::…`, `stdx::…`) | **Product / small gap** — not in ordinary dependency list → not `external_dropped` |
| **~4** | Path that is only a crate name (`span`, `tt`) | **Correct** — not a function call |
| residual | Leftover after the above (mostly more bindings the heuristics missed) | Treat as further indirect / cfg noise unless proven otherwise |

### 1.3 ripgrep — unresolved by reason string (132)

| Count | Reason pattern |
|---:|---|
| **124** | `no free function \`…\` in module \`…\`` |
| **8** | `path \`…\` uses \`…\` which is not a module in this crate …` |

Summary: resolved 2,108 · conflict 20 · unresolved 132 · external 355 ·
constructor 474 · associated 806.  
**101 / 132** unresolved sites are `from_macro: true`.

### 1.4 ripgrep — unresolved by verified semantics

| Count | Semantic bucket | Verdict |
|---:|---|---|
| **~93** | Macro-recovered test locals (`get`, `b64`, `mkctx`, `err`, …) | **Correct** |
| **~20** | Closure / param (`append` / `name_to_index` in `interpolate`, `invalid`, …) | **Correct** |
| **~8** | External / optional / dev dep (`serde_json::…` in tests, `pcre2::…`) | **Product** — same “not in dep list → unresolved” pattern |
| **~7** | Bare enum-variant constructor (`InvalidVariable(...)`) | **Miscategorised** — should be `constructor_dropped` |
| **~2** | `index::read` / `index::write` behind `pub(crate) use self::imp::*` | **Real gap** — within-crate **glob** re-export not followed on qualified paths |
| **~2** | Other (`drop`, small residuals) | Correct / negligible |

Ripgrep’s shape is different from rust-analyzer’s: almost no sibling-module or
cross-crate-module-prefix gaps; the mass is test-macro locals.

### 1.5 Dropped categories (sampled)

| Corpus | external | constructor | associated |
|---|---:|---:|---:|
| rust-analyzer | 1,229 | 3,932 | 5,772 |
| ripgrep | 355 | 474 | 806 |

**Top constructors (both corpora):** `Some` / `Ok` / `Err` dominate (RA:
2,526 + 451 + 161; ripgrep: 270 + 76 + 26). Remaining are enum/tuple forms
(`DiagnosticCode::…`, `Mode::Search`, `PathResolution::Def`, …). Spot checks
match real constructors.

**Top associated:** `Vec::new`, `String::new`, `Default::default`,
`TextRange::new`, builder `::new` patterns, `SourceFile::parse`, etc. Spot
checks match associated functions / inherent methods’ cousins.

**Top external:** `std::…`, `Arc::new`, `Either::Left`/`Right`,
`serde_json::…`, `NoColor::new`, … Match declared externals / language crates.

**Bucket noise (not silent free-function drops):** some **enum variants** land
in `associated_dropped` when the type is not indexed as an enum, because the
UpperCamelCase heuristic classifies `Type::Variant` as associated. Examples
verified in source:

- ripgrep `BinaryDetection::Convert` / `::Quit` — variants of
  `enum BinaryDetection` in `searcher/src/line_buffer.rs`, counted as associated
- rust-analyzer `NodeOrToken::Token`, `Definition::Local`, `Snippet::Tabstop` —
  same pattern

These are still correctly **absent** from the tree (not resolved to a fake
function). The error is which drop counter increments, not a false edge.

No sampled drop looked like a real free-function call that should have been
kept. The dangerous failure mode (swallowing a free function into a drop) was
**not** observed in this pass; the open risk remains the UpperCamelCase
module-name case documented in correctness-measurement (no specimen found
here).

---

## 2. Worked examples (source-verified)

### Closures reported as “no free function” (correct)

```352:357:%TEMP%/horizon-scale-corpus/rust-analyzer/crates/ide-completion/src/completions/expr.rs
                    let mut add_keyword = |kw, snippet| {
                        acc.add_keyword_snippet_expr(ctx, incomplete_let, kw, snippet)
                    };
                    // ...
                        add_keyword("unsafe", "unsafe {\n    $0\n}");
```

Reason: `no free function \`add_keyword\` in module \`crate::completions::expr\``.  
This alone accounts for dozens of the top reason-string rows (`add_keyword`
×75 across completion modules). Same story for `postfix_snippet` (bound from
`build_postfix_snippet_builder`), `handle_trait` / `parent_trait` (closures in
`hir/src/attrs.rs`), and ripgrep’s `append` / `name_to_index` parameters in
`grep_matcher::interpolate`.

### Nested function (real gap)

```623:641:%TEMP%/horizon-scale-corpus/rust-analyzer/crates/base-db/src/input.rs
        fn go(
            graph: &CrateGraphBuilder,
            // ...
        ) -> Crate {
            // ...
                    crate_id: go(
```

`FunctionId` is `base_db::input::set_in_db::go`; the call is bare `go`. Nested
`fn` items are not in the resolve index, so self-calls and sibling nested
calls become unresolved. Correctness-measurement already noted this pattern on
Horizon itself.

### Sibling module without parent walk (real gap)

In `parser/src/grammar.rs`, child modules are declared next to nested
`entry::prefix`:

```31:39:%TEMP%/horizon-scale-corpus/rust-analyzer/crates/parser/src/grammar.rs
mod attributes;
mod expressions;
// ...
mod types;
```

From `crate::grammar::entry::prefix`, rustc resolves `types::type_(p)` by
searching enclosing modules and finding `crate::grammar::types`. Horizon starts
navigation at the call-site module and only looks for a **child**
`…::prefix::types`, then falls through to
“`types` is not a module in this crate”. **~144** parser (and similar) sites.

### Path-crate module import used as prefix (real gap)

```1:3:%TEMP%/horizon-scale-corpus/rust-analyzer/crates/hir-def/src/expr_store/lower/path/tests.rs
use syntax::ast::{self, make};
// ...
    let path = make::path_from_text(path);
```

`path_from_text` is a real `pub fn` in `syntax/src/ast/make.rs`. The import
target `syntax::ast::make` is a **module** in a path dependency. When resolving
that import target, cross-crate lookup treats the final segment as a function
(or returns “names a module, not a function” as `Unresolved`) and never yields
`TargetResolve::Module`. The call then gets the misleading
“`make` is not a module in this crate” reason. Same mechanism for
`algo::least_common_ancestor` (`use syntax::{algo::{self, …}}`) and
`ast::attrs_including_inner`.

### Cross-crate associated form as unresolved (miscategorised)

`syntax::SourceFile::parse` / `span::FileId::from_raw` produce
`no public module \`SourceFile\` / \`FileId\``. Those segments are types, not
modules. They should be classified and **dropped** as associated (as
`Vec::new` already is when the heuristic fires), not counted as unresolved
free-function failures. **~43** sites on rust-analyzer.

### Bare constructor as unresolved (miscategorised)

ripgrep hyperlink tests:

```1061:1064:%TEMP%/horizon-scale-corpus/ripgrep/crates/printer/src/hyperlink/mod.rs
        assert_eq!(
            HyperlinkFormat::from_str("foo://{bar}").unwrap_err(),
            err(InvalidVariable("bar".to_string())),
        );
```

`InvalidVariable` is an enum variant brought into scope via
`use super::HyperlinkFormatErrorKind::*`. Reported as
`no free function \`InvalidVariable\``. Same class as the old `Ok(x)` bug;
bare UpperCamelCase / glob-imported variants are not consistently dropped.
**~7** on ripgrep; **~20** on rust-analyzer (`SourceRootId`, …).

### Dev / optional dependency as unresolved (product)

```87:88:%TEMP%/horizon-scale-corpus/ripgrep/crates/globset/src/serde_impl.rs
        let map: HashMap<String, Glob> =
            serde_json::from_str(&string).unwrap();
```

`serde_json` is a test-only dependency; discovery’s dependency list omits
`[dev-dependencies]`, so the call is unresolved instead of
`external_dropped`. Scale-validation already recorded this. Same for
`pcre2::version` behind an optional feature.

### Within-crate glob re-export (real gap, small)

```1:8:%TEMP%/horizon-scale-corpus/ripgrep/crates/core/index/mod.rs
pub(crate) use self::imp::*;
// cfg-twinned imp in disabled.rs / enabled.rs both define read/write
```

Call `index::read` resolves the module `crate::index` but not the name:
`no free function \`read\` in module \`crate::index\``. Cross-crate code
follows `pub use glob::*` facades; within-crate qualified lookup follows
**explicit** re-exports only. **2** sites on ripgrep; rare on rust-analyzer in
this sample.

---

## 3. Classification summary

| Class | What it means here |
|---|---|
| **Correct behaviour** | Closures/params/locals; macro test locals; crate-name-as-call; most ordinary drops |
| **Miscategorised** | Bare / glob-imported constructors still unresolved; cross-crate `Type::assoc` as “no public module”; some variants counted as `associated_dropped` instead of `constructor_dropped` |
| **Real gap** | Enclosing-module walk for path prefixes; cross-crate **module** import targets; nested `fn`; within-crate glob re-exports on qualified paths |
| **Misleading reason** | “not a module in this crate” when the name *is* a module via import or parent; “no free function” for closures (technically true, operationally noisy) |

---

## 4. Prioritised recommendations

Ordered by **estimated honest free-function edges recovered (or noise removed)
per unit of false-positive risk**. Counts are rust-analyzer-first; ripgrep
deltas noted where they differ.

| Priority | Change | Est. calls | Risk to zero-FP rule | Notes |
|---:|---|---:|---|---|
| **1** | Reclassify bare UpperCamelCase + failed cross-crate type prefixes as constructor/associated **drops** (same family as the Ok/Vec fix) | ~60–70 RA + ~7 RG removed from unresolved; **0** new resolved edges | **Very low** — only moves Unresolved → drop | Pure categorisation; improves the signal immediately |
| **2** | Make `resolve_target_path` return `Module` for cross-crate module import targets (so `make::f`, `algo::f` navigate) | **~80–100** new resolved on RA (free fns like `path_from_text`, `least_common_ancestor`, `attrs_including_inner`) | **Medium** — must not treat function-final paths as modules; add fixtures | Highest real recall win seen in this pass |
| **3** | Walk enclosing modules for the first segment of a relative qualified path (rustc-style) | **~140** RA, concentrated in `parser` | **Medium** — shadowing / multi-parent rules need tests | Large, localised payoff |
| **4** | Index nested `fn` items and resolve unqualified calls inside the parent body | **~100** RA | **Medium–high** — scoping, shadowing, recursive ids | Already on the correctness “unmeasured” list; do not guess across bodies |
| **5** | Follow within-crate `pub use glob::*` when looking up a name in a module via a qualified path | **~2** RG; small on RA | **Low–medium** — mirror existing cross-crate glob discipline (conflicts, visibility) | Tiny but clean; unlocks cfg-twinned `imp` modules |
| **6** | Treat undeclared externals (dev-deps / optional) as `external_dropped` when the first segment is clearly a crate name | ~5–10 per corpus, categorisation only | **Low** for drops; **do not** invent resolved edges into unindexed crates | Product choice; scale-validation already describes the symptom |
| **—** | “Resolve” closures / locals / macro test helpers | ~800 RA / ~110 RG | **Unacceptable** for this tool | Out of scope (indirect). Optional: a separate `indirect_dropped` counter if the unresolved % is politically painful — still not resolution |

**Do not prioritise** chasing the raw “no free function in module” string. On
rust-analyzer most of that row is closures.

---

## 5. Irreducible floor

### rust-analyzer

After priorities 1–4 (categorisation + the three real gaps), a plausible
remaining unresolved mass is:

| Residue | Approx. |
|---|---:|
| Indirect (closures / bindings / params) | **~750–850** |
| Macro / test locals | **~50–80** |
| Dev-deps / optional / cfg-invisible modules / true unknowns | **~100–200** |
| **Floor (honest unresolved free-fn attempts)** | **~150–250** if indirect stays in the unresolved counter; **~900–1,100** if indirect remains labelled unresolved |

Today: **1,533** unresolved out of **16,582** kept sites (resolved + conflict +
unresolved) ≈ **9.2%**.  
After 1–4: unresolved ≈ **1,100** if indirect stays (~6.6%), of which only
**~150–250** are “we should have found a free function.”  
Distance to that free-function floor: **large in absolute count if you count
closures; small if you only count true free-fn misses** — roughly **half to
two-thirds of the interesting gap is already identified** (items 2–4), and
another **~4% of kept sites** is categorisation noise (item 1).

A certain irreducible remainder is expected: heavy `cfg`, feature-gated
modules, and test-only machines. This pass did not try to quantify cfg-dead
code separately from indirect calls.

### ripgrep

Already near its free-function floor. After item 1 (7 constructors) + item 5
(2 glob re-exports) + item 6 (~8 externals as drops), unresolved falls from
**132 → ~115**, almost all macro/closure honesty. Further resolver work will
not move the needle.

### Comparison

| | rust-analyzer | ripgrep |
|---|---|---|
| Dominant unresolved story | Closures + two module-resolution gaps | Macro test locals |
| Miscategorised constructors / assoc | Present (~60–70) | Present but small (~7) |
| Real free-fn recall gaps | ~350 combined (items 2–4) | ~2 (glob re-export) |
| Drop buckets | Healthy; some variant↔associated noise | Same |

---

## 6. How to reproduce

```powershell
# Maps used for this write-up (already present from scale validation):
#   $env:TEMP\horizon-scale-results\rust-analyzer-after.json
#   $env:TEMP\horizon-scale-results\ripgrep-after.json

cargo build --release -p horizon
$corpus = "$env:TEMP\horizon-scale-corpus"
# Re-clone if missing:
#   git clone --depth 1 https://github.com/rust-lang/rust-analyzer $corpus\rust-analyzer
#   git clone --depth 1 https://github.com/BurntSushi/ripgrep $corpus\ripgrep

.\target\release\horizon.exe "$corpus\rust-analyzer" -o "$env:TEMP\ra-map.json" --compact
.\target\release\horizon.exe "$corpus\ripgrep" -o "$env:TEMP\rg-map.json" --compact
```

Group `target.kind == "unresolved"` by `target.data.reason`. For drops, use
`cargo run -p horizon-correctness --bin measure_correctness -- exclusions <repo>` or
`horizon::correctness::collect_call_outcomes` (drops are not in the JSON tree).

---

## 7. Bottom line

The 1,533 unresolved sites on rust-analyzer are **not** mostly missing import
support. About **half** are honest indirect calls. About **~60** are the
Ok/Vec-class miscategorisation repeating (constructors / associated forms).
About **~350** are real, bounded free-function gaps: cross-crate module
imports, enclosing-module path prefixes, and nested functions. Ripgrep’s 132
are already mostly irreducible test/macro noise plus a handful of
categorisation and glob-re-export issues.

The next resolution work that is worth the false-positive risk is **(2) then
(3) then (4)** above; the next *measurement* win with essentially no FP risk is
**(1)**.
