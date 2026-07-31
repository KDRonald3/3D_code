# Overnight work log — 31 July 2026

Unattended pass over W11 → W6 → W7 → W9 → W10 → W8 on `feat/ast-system`
(branch `cursor/overnight-queue-5755`). Spec:
[`remaining-work.md`](remaining-work.md).

---

## W11 — Qualified calls through an imported path-dependency module — DONE

**Why.** The self-map's last three unresolved sites were all the same false
positive: `use horizon_engine::discover;` then `discover::normalize_path(...)`.
The import target is a real `pub mod` in a declared path dependency, but
`resolve_target_path` forced every cross-crate target through the "final
segment is a function" path, so the binding never became a module the
qualified-call navigator could enter.

**Change.** When a cross-crate import target is an all-`pub` module chain
(including the bare crate root), resolve it as `ForeignModule` instead of
"names a module, not a function". Qualified calls whose leading segment is
that binding (`mod::fn`, renamed `alias::fn`, renamed crate-root `eng::fn`)
continue inside the dependency under the existing visibility / dependency-gate
rules. Unit tests cover the three shapes; the `path-dependency` fixture gains
`via_imported_module` and the Phase 4 integration test asserts the edges.

**Measured.**

| Check | Result |
|---|---|
| `cargo test --workspace` | pass |
| Unit: `format::upper` / `fmt::upper` / `eng::version` | Resolved to the foreign ids |
| Integration: `path-dependency` `via_imported_module` | all three sites Resolved; fixture summary **0 unresolved** |
| Self-map regenerate | **0 conflicts · 0 unresolved**; dropped 143 external, 232 constructor, 219 associated, 26 local |

**W5 re-accounting.** Before: 3 unresolved, all W11 false positives. After: **0
unresolved**. Every previously unresolved self-map site is now either dropped
(local binding) or resolved. The map reports no genuinely missing free function
on this repository.

**Not verified yet.** Browser / UI matrix — deferred until a server is up for
W6+; analyser-only change, no web assets touched.

---

## W6 — Busy function graphs are navigable — DONE

**Why.** `resolve.rs` is 66 nodes / 227 edges after W11 — correct, but a
hairball. Zoom does not make it auditable.

**Change.** Focus-plus-context in the Functions dock: busy files
(`>24` nodes or `>40` edges) open at a **1-hop** undirected neighborhood
around the focused function (selected fn, else first seed by line). A
segmented control beside Fit offers `1 hop` / `2 hops` / `All`. Small files
still open fully. The banner names the focus and counts hidden nodes/edges so
truncation is honest; visible edges keep their Resolved / Conflict /
Unresolved kinds. Hooks: `getFnsDepth` / `setFnsDepth` (probed by
`smokeCheck`).

**Bug found while testing.** Introducing a neighborhood `meta` binding
collided with an inner `const meta = document.createElement(...)` in the same
loop body (TDZ `ReferenceError` on every Functions render). Renamed the DOM
node to `metaEl`. Caught only by driving the UI — the code "looked right".

**Measured (Chrome 1920×1080, self-map, rebuilt binary).**

| Check | Result |
|---|---|
| `cargo test --workspace` | pass (pre-commit; analyser unchanged) |
| `resolve.rs` default depth | **1**; showing 2 of 66 nodes / 1 of 227 edges |
| Depth 2 | widens to 7 nodes |
| All | restores 66 / 227 |
| Depth button click | sets depth to 1 |
| Select another fn | refocuses neighborhood |
| smokeCheck | `ok: true` after each step |
| I1, I6, I8, I9, I11, I12, I13 | pass |
| `glob-ambiguity-map.json` | conflict stub still renders (1 conflict node) |
| Screenshots | `testdata/screenshots/w6-resolve-depth{1,2}.png`, `w6-resolve-all.png`, `w6-resolve-refocus.png` (gitignored) |

**Not verified.** Full manual matrix of every top-bar / Layers control (deferred
to a later pass). I4/I5 rail-drift node-rect constancy not re-measured this
round (I1 zoom constancy was). Favicon 404 is pre-existing noise, ignored.

---

## Queue status

| Item | Status |
|---|---|
| W11 | done |
| W6 | done |
| W7 | next |
| W9 | pending |
| W10 | pending |
| W8 | pending / may stop cleanly |
