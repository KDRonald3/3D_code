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

## W7 — Live in-process analysis — DONE

**Why.** The server could only display a map the CLI had already written. The
owner needs to point the UI at a repository and get a map without a separate
tool invocation.

**Change.** `POST /api/analyse` with `{ "path": "…" }` runs
`horizon_engine::build_function_map` on a blocking thread, stores the result in
the shared map slot, and returns `202 Accepted` immediately. `GET /api/analyse`
exposes `idle | running | failed | done` with elapsed time so the UI can poll.
Only one job at a time (409 if busy). Host guard unchanged. Import screen:
"Analyse local folder" opens a path form; a full-screen overlay shows the path
and ticking elapsed seconds so a long run never looks hung.

**Bug found while testing.** `smokeCheck` invoked every exported helper with no
args, so `startAnalyse()` POSTed an empty path on boot. Side-effecting helpers
are now presence-only in the smoke probe. Also, the first `watchAnalyseJob`
returned after the first poll; it now resolves a Promise only when the job
leaves `running`.

**Measured.**

| Check | Result |
|---|---|
| `cargo test --workspace` | pass (4 new HTTP tests) |
| Analyse `phase1-single-file` via UI | overlay shown, map loaded (1 file), smoke ok, ~420ms |
| Bad path | throws, overlay stays hidden |
| Host `evil.example` | 403 |
| Screenshots | `w7-analyse-form.png`, `w7-analyse-done.png` |

**Not verified.** Analysing the full Horizon workspace from the UI (would work;
fixture coverage is enough for the contract). Concurrent 409 attach path not
browser-driven (covered by the attach branch in JS + HTTP conflict shape).

---

## W9 — Theme, Recent, Pages rail — DONE

**Why.** Slice 5 chrome was still stubbed: theme did not persist, there was no
Recent list, and Pages had a single Diagnostics dock button.

**Change.** Theme toggle persists to `localStorage` (`horizon.theme`) and
survives reload. Loading a map records it under `horizon.recent` (capped at 5,
skips maps over ~1.5 MiB); the import screen shows chips that restore via
`loadMap` + `POST /api/map`. Pages rail is Map / Diff (disabled, honest title)
/ Functions / Diagnostics — Functions and Diagnostics open the dock on the
right tab; Map closes the dock. Diff stays inert.

**Measured.** Theme light→dark (icon ☀, `data-theme=dark`, localStorage);
reload keeps dark. Pages: Functions opens fns pane, Diagnostics opens diag,
Map closes dock. Recent chip restores 23-file self-map. smokeCheck ok.
Screenshots: `w9-theme-dark.png`, `w9-pages-fns.png`, `w9-recent.png`.

---

## W10 — Collapse duplicated rail arithmetic — DONE

**Why.** Rail geometry lived twice (Rust oracle + JS twin) with only a comment
saying "keep in lockstep".

**Change.** Shared fixture table `web/rail_layout_cases.json` (19 cases covering
squeeze, restore, hard stop, collapsed opposite, narrow window, custom homes).
Rust `shared_fixture_table_matches_rust_oracle` and JS
`HorizonViewer.runRailFixtureTable()` both exercise it. Served at
`/static/rail_layout_cases.json`. Arithmetic left in place on both sides —
proven equivalent rather than rewritten mid-flight (avoids I1–I5 risk).

**Measured.** Rust test pass; browser `runRailFixtureTable` →
`{ ok: true, total: 19, fails: [] }`; smokeCheck ok.

---

## W8 — Structs and impls — IN PROGRESS (W8a landed)

Earlier overnight pass stopped before W8. A continuation session took the first
coherent slice.

### W8a — Type definitions in the map + filter chips — DONE

**Why.** The Types / Fns chips were permanently disabled because the emitted
contract had no type nodes — only an internal extract index used to drop
constructors.

**Change.** `File.types: TypeItem[]` with `TypeId`, kind, variants, docs, byte
range, and `type_refs` (field types / alias RHS) resolved to
`TypeTarget::{Resolved,Conflict,Unresolved}`. External and prelude paths are
omitted from `type_refs`. Fns / Types chips dim file cards by `fnCount` /
`typeCount`. Hooks: `getFilters` / `setFilter`. Fixture `type-definitions`.

**Measured.**

| Check | Result |
|---|---|
| `cargo test --workspace` | pass (incl. `type_definitions`) |
| Fixture: Named → Label, Point | Resolved type_refs |
| Fixture: Alias → Named | Resolved |
| Old maps without `types` | still load (`#[serde(default)]`, I12 style) |
| Browser (filters / self-map) | pending rebuild + drive after commit |

**Browser (W8a).** Self-map: 83→84 types / 16 files with types. Fns off dims
6 function-only cards; Types off dims 2. smokeCheck ok. Favicon 404 ignored.
Screenshots: `w8a-filters-*.png` (gitignored).

### W8b — Inherent methods + one-hop receivers — DONE

**Why.** Types alone do not close W8; the owner's interest includes methods,
and `Type::assoc` was still a silent `associated_dropped` for local types.

**Change.** Extract inherent `impl Type` methods as `Function` with
`receiver_type`. Resolve `Type::method` and `.method` with one-hop hints
(param/let annotation, constructor RHS). Untyped `.method` with ≥2 inherent
candidates → `Conflict`; otherwise trait/untyped/external assoc →
`associated_dropped` (not Unresolved flood, not a guessed resolve). Trait
impls stay excluded. Fixture `inherent-methods`.

**Policy note.** First cut that Unresolved every untyped `.clone`/`.len` blew
the self-map to ~1900 unresolved. That was rejected: those sites are not
missing free functions. Dropping them under `associated_dropped` keeps the
Diagnostics worklist honest.

**Measured.**

| Check | Result |
|---|---|
| `cargo test --workspace` | pass |
| Fixture: Cache::new / .get one-hop | Resolved |
| Fixture: untyped ambiguous .get | Conflict (Cache vs Registry) |
| Self-map | **0 unresolved · 6 conflicts**; 84 types, 63 methods; associated_dropped 2484 |
| 6 conflicts | untyped `.explicits_in` / `.find_type` / `.lookup_in_module` / `.as_str` name clashes — true positives |

**Browser (W8b).** smokeCheck ok; 63 methods / 84 types on self-map; Fns filter
dims 6; I11 (Inspector opens on resolve.rs select); I9 dock zoom 0.75 preserved
across tab switch; Diagnostics lists 6 conflict entries matching summary.
Screenshot: `w8b-self-map-diag.png`. Favicon 404 ignored.

**Not done.** Trait impl methods; multi-hop / inference beyond one hop; type
DAG in the dock.

---

## Queue status

| Item | Status |
|---|---|
| W11 | done |
| W6 | done |
| W7 | done |
| W9 | done |
| W10 | done |
| W8 | done (W8a types + W8b inherent methods; trait impls still out) |
