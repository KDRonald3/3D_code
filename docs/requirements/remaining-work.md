# Remaining Work — Horizon Analyser and Review UI

**Status:** work queue and acceptance criteria for the outstanding items
**Date:** 30 July 2026
**Purpose of the UI:** audit the analyser. Every screen exists so a human can
judge whether a resolution is right, and every display must be honest about
uncertainty rather than guessing.
**Contract truth:** [`map.rs`](../../crates/horizon-map/src/map.rs)
**Target design:** [`desktop-horizon-spec.md`](../ui/desktop-horizon-spec.md)
**Build plan this continues:** [`desktop-ui-rebuild.md`](desktop-ui-rebuild.md)

---

## How to run and verify

```powershell
# Regenerate the self-map (the map used for all manual review below)
cargo run --bin horizon -- C:\Users\kouat\code\Horizon -o crates\horizon-server\testdata\horizon-self-map.json

# Serve it without stealing a browser tab
cargo run -p horizon-server -- --map crates\horizon-server\testdata\horizon-self-map.json --no-open
```

The server prints `http://127.0.0.1:<ephemeral>/`. It binds loopback only and
rejects non-loopback `Host` headers.

**Web assets are embedded with `include_str!`** in
[`static_files.rs`](../../crates/horizon-server/src/static_files.rs). Editing
anything under `crates/horizon-server/web/` has **no effect until the binary is
rebuilt**. Several rounds of work were wasted testing stale binaries; if a
change appears to do nothing, confirm the served asset contains it
(`GET /static/viewer.js`) before debugging further.

### Test surface

`window.HorizonViewer` exposes an honest test surface — no accessor may read an
undefined value (a `ReferenceError` from a dead accessor has already cost one
round of debugging). Current members:

`selectFile`, `selectFunction`, `goBack`, `getSelection`, `loadMap`,
`getUiState`, `getRailWidths`, `getTransform`, `screenXOfWorld`,
`computeRightAggressorLayout`, `computeLeftAggressorLayout`, `setRightWidth`,
`setLeftWidth`, `setBottomOpen`, `setBottomHeight`, `setBottomTab`, `getBottom`,
`getBottomRailHit`, `getBottomTransform`, `setBottomZoom`, `screenXOfFnsWorld`,
`screenYOfFnsWorld`, `getFnsNodeScreenRect`, `getFnsPaneMetrics`,
`getFunctionDag`, `renderFunctionDag`, `fitFunctionDag`, `openDiagnosticEntry`,
`renderDiagnostics`, `inspectorOpenPolicy`, `getLastCardGesture`, `smokeCheck`,
`applyRightWidth`, `applyLeftWidth`, `commitLeftHome`, `commitRightHome`,
`runLayoutAcceptance`, `fitView`.

`smokeCheck()` runs at boot and must return `ok: true`. Any new hook gets a
probe there, so a broken surface fails loudly on load instead of silently in a
panel nobody opened.

---

## Invariants that must not regress

These were each reported as bugs by the owner, diagnosed, fixed, and then
verified by measurement in a real browser. Numbers are the measured values on a
1920×1080 viewport with the self-map loaded. Treat any deviation as a
regression.

| # | Invariant | How it was measured |
|---|---|---|
| I1 | Rails resize independently of canvas **scale**: dragging a rail changes the viewport, never the zoom | `getTransform().zoom` and `getBottomTransform().zoom` constant across rail drags |
| I2 | An aggressor rail squeezes the opposite rail to its minimum, and the squeezed rail returns to its **remembered** width on the way back | `getRailWidths()` `leftHome`/`rightHome` restore |
| I3 | Rails have no maximum but the window edge, and remain **separable** after they meet, whichever one closed the gap | left and right both drivable to their max, then apart again |
| I4 | The **map** does not drift when the left rail expands: content stays under the same screen pixel | `panX -= Δ` compensation |
| I5 | The **dock graph** does not drift on any rail drag: a node holds its exact screen position | node rect constant at (331.3, 1005.5) across left ±280, right ±240, dock height ±160 |
| I6 | The dock's status line never wraps and the dock canvas never collapses | banner 31px and canvas 176px constant while dock width went 424 → 124; `getFnsPaneMetrics().viewportAlive` true, 48px floor |
| I7 | The dock resizes by dragging its top edge, clamped | handle is a full-width 11px strip on the dock's top edge; height clamps at min 120 and viewport-minus-chrome (1036) |
| I8 | Dock zoom is independent of map zoom, in both directions | wheel over dock: 0.307 → 0.332 with map at 0.25; wheel over map: 0.25 → 0.27 with dock unchanged |
| I9 | Dock zoom survives a tab switch, a close/reopen, and rail resizing | set 0.75, unchanged through all three |
| I10 | The zoom HUD appears on the Functions canvas only, never on the Diagnostics list | scaling a list is meaningless |
| I11 | **Sticky collapse:** a rail the user closed stays closed during pan, zoom, card drag and rail resize; but clicking a file, function or diagnostic entry does open the Inspector | `inspectorOpenPolicy()` → `selectionOpens: true`, `viewManipulationOpens: false` |
| I12 | Maps saved before `Function.byte_start`/`byte_end` and `File.content_hash` existed still load | `#[serde(default)]` on those fields, pinned by a test in `json.rs` |
| I13 | A DAG with cycles lays out and renders; it must never hang or throw | mutual recursion previously overflowed the layout queue (`RangeError`); back-edges are stripped by DFS |
| I14 | `/api/source` serves only paths present in the loaded map, and reports staleness by content hash rather than serving wrong bytes | path equality against `File.path`, SHA-256 compare |

---

## Work items

Each item states the problem, the evidence, what done means, and how to prove
it.

**W1–W4 are done and verified** — they are kept below with their evidence
because they define behaviour that must not regress. The open work is
**W5–W11**.

### W1 — DAG node clicks are swallowed by drag-to-pan — DONE

**Problem.** Clicking a function node in the Functions tab frequently does
nothing.

**Evidence.** Dispatching realistic pointer sequences at a node: 0px movement
selects correctly, 3px still works, **8px is discarded** and the selection never
changes. The dock's pan gesture claims any movement past the ~5px threshold even
when the gesture began on a node, so ordinary mouse and trackpad drift kills the
click.

**Done when.** A gesture beginning on a node selects that node; panning
originates from empty canvas background. Movement of 20px or more that is
released **inside** the node still selects. Raising the threshold alone is not a
fix.

**Prove it.** Replay 0/3/8/20px jitter clicks and assert `getSelection().fnId`
changes each time; assert background drags still pan.

**Resolved as:** a press beginning on `.fns-node` never pans; pan starts only
from empty background. Verified at 0, 8 and 24px drift, all selecting
correctly, with background drag still panning (panX 61 → 1) and zoom untouched.

### W2 — Node shading misleads, and there is no legend — DONE

**Problem.** The owner asked why nodes are "shaded dark even when they have been
resolved".

**Evidence.** Three styles exist: `fns-node function` cream `rgb(250,249,247)`;
`fns-node function external` **darker grey `rgb(236,234,228)` at opacity 0.92**
— a callee defined in another file, fully resolved; `fns-node unresolved` faint
red `rgba(197,48,48,0.08)`. So a healthy cross-file edge is drawn darker *and*
faded, which reads as disabled. `querySelector('.fns-legend, .dag-legend')`
returns null — there is no legend at all.

**Done when.** "Defined in another file" no longer looks degraded (drop the
opacity reduction; lean on the existing file badge plus a border or accent). A
compact legend names each state in the project's honest vocabulary: defined in
this file / defined in another file / **the analyser could not resolve this
call**. Clicking an unresolved stub surfaces the analyser's reason instead of
selecting an unrelated function. Must not violate I6.

**Resolved as:** cross-file nodes keep the full-opacity cream fill with a dashed
border and a blue left accent; in-file nodes carry a green accent, unresolved a
red wash. An absolute-positioned `#fns-legend` names all three states, so it
steals no canvas height (I6 measured intact afterwards).

### W3 — Closure calls reported as unresolved functions — DONE

**Problem.** 19 of 28 unresolved sites on the self-map are closures, not missing
functions.

**Evidence.** ``no free function `by_name` in module `crate::extract::tests` ``
(10 sites, `extract.rs` L1312–L1529) and the same for `by_local` (9 sites,
L1429–L1439). These are `let` bindings holding closures, several invoked inside
`assert!` macros.

**Done when.** A callee name that resolves to a local binding — `let`, closure,
or function parameter — is **dropped**, not reported, consistent with how other
out-of-scope call forms are handled. Consider a dropped-category counter beside
the existing `external` / `constructor` / `associated` counts so the map stays
auditable. A local `fn` item declared inside a function body **is** a real free
function and must still resolve.

**Resolved as:** the extractor records `let` bindings and parameters per
function; unqualified calls to those names are dropped as
`ExclusionKind::LocalBinding` under a new `local_dropped` summary counter, so
the drop stays auditable rather than silent. Nested `fn` items resolve first.
Fixtures: `tests/fixtures/local-bindings/`.

### W4 — Imports into inline `mod tests` are not followed — DONE

**Problem.** 6 of 28 unresolved sites are real, resolvable calls.

**Evidence.** ``no free function `is_item_macro_allowlisted` in module
`crate::modules::tests` `` at `modules.rs` L614–L619. The function exists in the
parent module and the inline test module imports it.

**Done when.** Imports declared inside an inline module are honoured for calls
within it, including `use super::*` glob semantics, respecting the established
visibility rules (a private parent item **is** visible to a child module — see
the existing `super_glob_sees_parent_private` and
`private_fn_not_in_glob_from_sibling` tests). These sites must **resolve**, not
be dropped or guessed.

**Resolved as:** glob and import following now includes private `use` bindings
visible to descendants, so `use super::*` sees parent private helpers and parent
private imports. Sibling globs still exclude private names. Fixtures:
`tests/fixtures/inline-mod-imports/`.

### W5 — Every remaining unresolved site is accounted for — DONE

**Outcome (pre-W11).** The self-map went from **35 unresolved to 3**: 26 closure
and local-binding false positives dropped, 6 `is_item_macro_allowlisted` sites
now resolved. Counts after: *24 files across 8 crates · 0 conflicts ·
3 unresolved; dropped 145 external, 226 constructor, 201 associated, 26 local.*

All three survivors were the same bug, tracked as **W11** — none was a true
positive.

**Outcome (post-W11).** Those three sites now resolve into `horizon_engine`.
Self-map regenerate: *0 conflicts · 0 unresolved; dropped 143 external,
232 constructor, 219 associated, 26 local.* Every previously unresolved site on
this repository is either dropped (local binding) or resolved. Re-run this
accounting after any further resolver change; the goal is not zero unresolved
but that every remaining one is **honest**.

### W6 — The DAG is unreadable for busy files — DONE

**Problem.** `resolve.rs` renders 58 nodes and 199 edges as a hairball. Correct,
but useless for review.

**Done when.** A busy graph is navigable: depth limiting from the seed, callee
collapsing, or focus-plus-context, with the control discoverable and the honest
edge semantics preserved. Zoom alone does not close this.

**Resolved as:** focus-plus-context with a discoverable `1 hop` / `2 hops` /
`All` control in the Functions chrome. Busy files (`>24` nodes or `>40` edges)
default to 1 hop around the selected function (else first seed by line); the
banner counts hidden nodes/edges. Measured on `resolve.rs` (now 66 / 227 after
W11): depth 1 shows 2 nodes, depth 2 shows 7, All restores 66 / 227.

### W7 — Live in-process analysis — DONE

**Problem.** The server can only display a map produced earlier by the CLI.

**Done when.** `POST /api/analyse` runs the pipeline **in process** for a
repository path, stores the result in the shared map slot, and the UI can
trigger it and show progress and failures without the CLI. Keep the loopback
bind and Host guard. Long analyses must not appear as a hung page.

**Resolved as:** async job (`202` + poll `GET /api/analyse`) with a path form
and elapsed-time overlay on the import screen. Host guard still rejects
non-loopback. Fixture `phase1-single-file` analysed end-to-end in the browser.

### W8 — Extend the analyser to structs and impls — IN PROGRESS

**Problem.** The Fns and Types filter chips are permanently disabled because the
contract carries only free functions. Methods and impls were deliberately
deferred.

**Done when.** Data structure definitions and their use in other definitions are
represented in the map, and the Types and Fns filters do real work. This was the
owner's original interest alongside functions, so treat it as a first-class
extension of the contract rather than a UI afterthought. Method receiver typing
is the hard part: one-hop inference at most, and anything less than certain must
be `Conflict` or `Unresolved` — never a guess.

**Progress (W8a).** `File.types` emits `TypeItem` nodes (struct / enum / trait /
type alias) with `TypeId`, byte ranges, docs, enum variants, and `type_refs` for
field / alias paths (external and prelude paths omitted). Fixture
`type-definitions`. Fns / Types filter chips are enabled: they dim file cards by
content (`fnCount` / `typeCount`). **Not yet:** inherent methods, associated
functions, or method-call receiver typing — those remain dropped /
`associated_dropped` as before.

### W9 — Slice 5: remaining shell chrome — DONE

**Done when.** Theme toggle (`☾`), the Recent list, and the Pages rail behave as
in the Desktop spec. `Diff · PR #142` is a design placeholder and stays disabled
unless a real diff source exists — do not fake it.

**Resolved as:** theme persists in `localStorage`; Recent chips restore stored
`Repository` JSON (size-capped); Pages are Map / Diff (disabled) / Functions /
Diagnostics. Diff title states there is no diff source — not faked.

### W10 — Collapse the duplicated rail arithmetic — DONE

**Problem.** Rail geometry exists twice: a Rust oracle in
[`rail_layout.rs`](../../crates/horizon-server/src/rail_layout.rs) and its
JavaScript twin in `viewer.js`. Two implementations of one rule will drift.

**Done when.** One is the single source of truth, or they are provably
equivalent by a shared fixture table exercised from both sides.

**Resolved as:** shared fixture table
[`rail_layout_cases.json`](../../crates/horizon-server/web/rail_layout_cases.json)
(19 cases) exercised by the Rust oracle test and
`HorizonViewer.runRailFixtureTable()` in the browser. Both sides report exact
agreement; arithmetic left in place to avoid rail-invariant regressions.

### W11 — Qualified calls through an imported path-dependency module — DONE

**Problem.** The only 3 unresolved sites left on the self-map, all the same
cause.

**Evidence.** In `horizon-correctness`: `use horizon_engine::discover;` then
`discover::normalize_path(...)` at `collect_call_outcomes` L278, `compare_lsif`
L376 and `uri_to_path` L906. `normalize_path` is a real `pub fn` in the
path-dependency crate `horizon_engine`. The resolver reports ``path
`discover::normalize_path` uses `discover` which is not a module in this crate``
— it never follows the *imported module binding* across the crate boundary.

**Done when.** A qualified call whose leading segment is a module imported from a
declared path dependency resolves into that crate, honouring the existing
dependency gate and cross-crate visibility rules (`pub` plus an unbroken module
chain). Renamed imports (`use x::y as z;`) must work too. Add a fixture pairing
two path-dependency crates, and re-run the W5 accounting afterwards.

**Resolved as:** cross-crate import targets that are an all-`pub` module chain
(or the bare crate root) become `ForeignModule`; qualified calls through that
binding continue inside the dependency. Fixture coverage in
`path-dependency` (`via_imported_module`: plain `format::upper`, renamed
`nested::buried`, renamed crate-root `eng::version`). Self-map: **0 unresolved**.

---

## Manual test matrix

The UI is a review instrument, so it earns trust only by being driven. Exercise
every control, and after each one confirm `smokeCheck().ok` and an empty console.

**Top bar:** `☰` layers toggle · breadcrumb · counts chip (now `24 frames ·
3 unresolved` after W3–W5; it read 28 unresolved before) · `Switch project` ·
`☾` theme · `☰` inspector toggle.

**Pages rail:** `Map` · `Diff · PR #142` (disabled) · `Diagnostics` with its
count badge.

**Layers rail:** `Find a file…` search, including a query matching nothing ·
crate group collapse arrows · `(crate root)` folder arrows · file rows including
`[bin]` crates and the `!` warning badge · a `★` entry-point row.

**Filter chips:** `Entry` · `Files` · `Fns` (disabled until W8) · `Types`
(disabled until W8).

**Canvas:** pan by background drag · wheel zoom · HUD `−` / `%` / `+` · card
click selects · card drag moves without selecting or opening the Inspector
(5px threshold) · click a call edge · the architecture note card.

**Rails:** drag each of the three, in both directions, to both extremes; then
the squeeze-and-restore and separability cases in I2–I5.

**Bottom dock:** `Functions` / `Diagnostics` tabs · `Fit` · `✕` close and
reopen from Pages · dock HUD `−` / `%` / `+` · drag the top edge · `By reason` /
`By file` grouping · click entries in each group · a group with one member and
the largest group.

**Inspector:** `← Back` through several hops until disabled · function and file
selections · a function with no doc comments · the source pane, including a
stale-hash and a missing-file case · every call-site row, following its
`→ target` link · a `RESOLVED` badge, a `Conflict` (use
`testdata/glob-ambiguity-map.json`, which reproduces rustc E0659) and an
`Unresolved` reason.

**Sequences that have broken before:** collapse the Inspector, then click a
node — it must open (I11). Resize a rail with the Inspector collapsed — it must
stay closed. Zoom the dock, switch tabs, return — zoom preserved (I9). Load a
second map while a function is selected. Select a file with an empty graph.
Select `resolve.rs` (58 nodes, 199 edges) and `extract.rs` (44 seed functions;
its 19 unresolved edges became dropped local bindings in W3, so it is now a good
check that the legend's third state is reachable elsewhere) — neither may hang or
throw (I13). Regenerate the map first; the numbers above shift as the resolver
improves, so treat them as landmarks rather than assertions.

**Data to test against:** `testdata/horizon-self-map.json` (this repo),
`testdata/cellular-automata-map.json` (a foreign codebase),
`testdata/glob-ambiguity-map.json` (conflicts), and
`testdata/glob-ambiguity-map-pre-byte-range.json` (old-format load, I12).

---

## Non-goals

- Do not make the analyser guess. `Conflict` and `Unresolved` with a truthful
  reason are correct outputs, not failures to paper over.
- Do not fake UI affordances for data that does not exist.
- Methods and impls stay out of the map until W8 is deliberately taken on.
- Calls into external crates remain dropped by design.

## Environment notes

- Shell commands on this machine need full permissions; the sandbox backend is
  unavailable and commands otherwise fail to spawn.
- PowerShell has no heredoc: pass commit messages with `git commit -F <file>`.
- `crates/horizon-server/testdata/` and `/target-agent` are gitignored.
