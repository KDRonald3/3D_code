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
it. Items W1–W4 were in flight when this document was written; confirm their
state before starting.

### W1 — DAG node clicks are swallowed by drag-to-pan

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

### W2 — Node shading misleads, and there is no legend

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

### W3 — Closure calls reported as unresolved functions

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

### W4 — Imports into inline `mod tests` are not followed

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

### W5 — Every remaining unresolved site must be accounted for

**Done when.** After W3 and W4, re-run the CLI on this repository and, for each
site still reported unresolved, state whether it is a true positive (the code
really has no such free function) or another class of analyser bug worth its own
item. Baseline before W3/W4: *23 files across 8 crates · 0 conflicts, 28
unresolved; dropped 138 external, 220 constructor, 166 associated.* The goal is
not zero unresolved — it is that every remaining one is **honest**.

### W6 — The DAG is unreadable for busy files

**Problem.** `resolve.rs` renders 58 nodes and 199 edges as a hairball. Correct,
but useless for review.

**Done when.** A busy graph is navigable: depth limiting from the seed, callee
collapsing, or focus-plus-context, with the control discoverable and the honest
edge semantics preserved. Zoom alone does not close this.

### W7 — Live in-process analysis

**Problem.** The server can only display a map produced earlier by the CLI.

**Done when.** `POST /api/analyse` runs the pipeline **in process** for a
repository path, stores the result in the shared map slot, and the UI can
trigger it and show progress and failures without the CLI. Keep the loopback
bind and Host guard. Long analyses must not appear as a hung page.

### W8 — Extend the analyser to structs and impls

**Problem.** The Fns and Types filter chips are permanently disabled because the
contract carries only free functions. Methods and impls were deliberately
deferred.

**Done when.** Data structure definitions and their use in other definitions are
represented in the map, and the Types and Fns filters do real work. This was the
owner's original interest alongside functions, so treat it as a first-class
extension of the contract rather than a UI afterthought. Method receiver typing
is the hard part: one-hop inference at most, and anything less than certain must
be `Conflict` or `Unresolved` — never a guess.

### W9 — Slice 5: remaining shell chrome

**Done when.** Theme toggle (`☾`), the Recent list, and the Pages rail behave as
in the Desktop spec. `Diff · PR #142` is a design placeholder and stays disabled
unless a real diff source exists — do not fake it.

### W10 — Collapse the duplicated rail arithmetic

**Problem.** Rail geometry exists twice: a Rust oracle in
[`rail_layout.rs`](../../crates/horizon-server/src/rail_layout.rs) and its
JavaScript twin in `viewer.js`. Two implementations of one rule will drift.

**Done when.** One is the single source of truth, or they are provably
equivalent by a shared fixture table exercised from both sides.

---

## Manual test matrix

The UI is a review instrument, so it earns trust only by being driven. Exercise
every control, and after each one confirm `smokeCheck().ok` and an empty console.

**Top bar:** `☰` layers toggle · breadcrumb · counts chip (`23 frames · 23 links
· 28 unresolved`) · `Switch project` · `☾` theme · `☰` inspector toggle.

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
Select `resolve.rs` (58 nodes, 199 edges) and `extract.rs` (44 seeds, 86
resolved, 19 unresolved) — neither may hang or throw (I13).

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
