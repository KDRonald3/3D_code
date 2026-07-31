# Desktop UI Rebuild — Vertical Build Plan

**Status:** plan for rebuilding the review UI against the Desktop design  
**Date:** 30 July 2026  
**Supersedes:** the tree-viewer phase plan and visual IA in
[`rust-ui.md`](rust-ui.md) (Phases A–G framing around containment tree +
diagnostics tab).  
**Still stands from [`rust-ui.md`](rust-ui.md):** server/security constraints,
source contract (`GET /api/source`), map field names, non-guessing rule,
live-analyse requirement, and the purpose (audit the analyser).  
**Target design:** [`desktop-horizon-spec.md`](../ui/desktop-horizon-spec.md)
(+ screenshots in [`docs/ui/`](../ui/)).  
**Contract truth:** [`map.rs`](../../crates/horizon-map/src/map.rs).

---

## Diagnostics recommendation (read first)

**Recommend: a Diagnostics tab in the bottom panel**, beside Functions, as the
primary worklist. Keep conflict/unresolved **badges on file cards and Layers
rows** as secondary filters that dim the canvas to the hot files.

| Placement | Fit for “group by reason, largest first, click → site + source” | Verdict |
|---|---|---|
| Layers panel section | Narrow (236px); competes with navigation; reason strings and candidate lists do not fit | Reject as primary |
| **Bottom tab beside Functions** | Expandable list surface (~248px → full height); Pages already open this dock; click can select file on canvas + function + fill Inspector | **Primary** |
| Card badges only | Good spatial heat map; cannot group by reason or walk a class systematically | Supporting chrome only |
| Inspector section (selected file) | Useful for “this file’s incomplete edges”; fails repo-wide largest-class-first | Secondary, optional later |
| Dedicated overlay replacing Map | Maximum list space (as in `ebfeac4`); abandons the Desktop composition the owner chose | Reject unless scale forces it |

**Why this wins:** the owner’s audit loop is a **worklist**, not a spatial
query — group by `reason`, sort largest class first, open a site, read source
beside candidates. The bottom dock is already the Desktop place for dense
symbol work; a Diagnostics tab reuses that chrome without inventing a fourth
pane. Card/Layers badges stay so the map still answers “which files are hot?”
Reuse the client-side walk from commit `ebfeac4`
(`collectDiagnostics`, `groupByReason`, `groupByFile`, MapSummary reconcile) —
port the logic, not the Tree|Diagnostics view switch.

---

## What already exists (do not rebuild)

| Piece | Where | Carry forward |
|---|---|---|
| Loopback ephemeral bind, `--map`, open browser | [`main.rs`](../../crates/horizon-server/src/main.rs) | Unchanged |
| Router, 64 MiB body limit, Host guard | [`app.rs`](../../crates/horizon-server/src/app.rs), [`host_guard.rs`](../../crates/horizon-server/src/host_guard.rs) | Unchanged |
| `GET`/`POST /api/map`, `GET /api/health` | [`routes.rs`](../../crates/horizon-server/src/routes.rs) | Unchanged |
| Shared map slot | [`state.rs`](../../crates/horizon-server/src/state.rs) | Unchanged; analyse will write here too |
| `GET /api/source` + token classes + error kinds | [`source.rs`](../../crates/horizon-server/src/source.rs) | Unchanged wire contract |
| Embedded assets | [`static_files.rs`](../../crates/horizon-server/src/static_files.rs) → `web/{index.html,viewer.css,viewer.js}` | Shell replaced by Slice 1; route paths stay |
| Function `byte_*`, `File.content_hash` | [`map.rs`](../../crates/horizon-map/src/map.rs) | Already in contract |
| Diagnostics grouping (to reuse) | `ebfeac4` in committed `viewer.js` history | Logic only; UI home changes per recommendation above |

The committed tree viewer + Diagnostics view-switch are **being replaced** by
the Desktop shell. Their contract handling (id index, jump, `/api/source`
error banners, `collectDiagnostics`) is the **data layer** to keep.

---

## Slice 1 — foundation (in progress; plan around it)

**Goal:** Replace the front-end shell with the Desktop Map view: dot-grid
canvas, pan/zoom, one card per `File`, edges from resolved cross-file call
sites, left Layers (crates → folders → files), Desktop palette/chrome;
positions computed client-side.

**Touches:** `crates/horizon-server/web/{index.html,viewer.css,viewer.js}`
(and only those unless static route names change).

**Demo:** `--map` a multi-file fixture → browser shows a spatial file map with
edges, Layers list, conflict/unresolved badges on cards; pan/zoom works.

**Verify:** manual browser check on Horizon-self or a fixture; `cargo build -p
horizon-server`; no server API changes required.

**Depends on:** existing server. **Do not re-litigate** rendering foundation,
layout approach, or file set — later slices build on whatever Slice 1 lands.

---

## Vertical slices (2 onward)

Audit value first: source beside call sites, then the diagnostics worklist,
then the function DAG and remaining chrome, then live analyse.

### Parallelism (file-level, honest)

The front end is **three large files**
(`index.html`, `viewer.css`, `viewer.js`). Almost every UI slice edits all
three. **Do not schedule two UI slices in parallel** unless one agent owns
`viewer.js` and another owns only server Rust — and even then CSS/HTML
collisions are likely.

| Pair | Parallel? | Why |
|---|---|---|
| Slice 1 ‖ Slice 6 (analyse server) | **Yes**, after Slice 1 HTML shell exists enough to add a path control later | Server: new `analyse.rs` + `app.rs`/`routes.rs`; UI chrome for Analyse can wait until Slice 6b |
| Slice 2 ‖ Slice 6a (server-only analyse) | **Yes** | Different crates/files: `web/*` vs `horizon-server/src/analyse.rs` (+ thin `app.rs` wire-up) |
| Slice 2 ‖ Slice 3 | **No** | Both dominate `viewer.js` (selection model, Inspector, bottom dock) |
| Slice 3 ‖ Slice 4 | **No** | Same bottom-panel chrome and selection |
| Slice 4 ‖ Slice 5 | **No** | Shared header/Pages/bottom chrome |
| Slice 5 ‖ Slice 6b (Analyse button) | **Tight** — serialize or fold Analyse control into Slice 5/6 as one owner |

**Rule of thumb:** one agent on `web/*` at a time; server-only work can
overlap any UI slice that does not need the new endpoint yet.

---

### Slice 2 — Inspector: docs → source → call sites

**Goal:** Selecting a file or function fills the right Inspector with identity,
joined `doc_comments`, server-highlighted source via `GET /api/source`, and
that function’s ordered `call_sites` (resolved / conflict / unresolved /
`from_macro`), so the owner can audit one function end to end.

**Touches:** `web/index.html` (right aside structure), `web/viewer.css`
(Desktop inspector + `.tok-*` including `str`/`num`), `web/viewer.js`
(selection model, fetch source, call-site cards, error banners for
`stale` / `missing` / `unverifiable` / `no_source` / `not_in_map`).

**Demo:** click a card → Inspector shows file identity + fn count; open a
function (Layers, card detail, or temporary fn list) → Source well matches disk;
conflict sites show every candidate + reason; macro badge present; stale map
shows banner, no wrong body.

**Verify:** browser on a hashed map; deliberately edit a `.rs` file → `stale`;
fixture with known conflict (e.g. glob-ambiguity); optional existing server
source tests unchanged.

**Depends on:** Slice 1 (shell + selection hooks). Server source already done.

**Slots later:** when the analyser emits types/`impl`s, Inspector gains a kind
chip and sections without a layout redesign — leave a labelled empty “Symbols
(types · impls)” stub **only if** Pages already expose Data structures;
otherwise omit until data exists (see Dropped).

---

### Slice 3 — Diagnostics worklist (bottom tab)

**Goal:** Repository-wide Conflict + Unresolved list in the bottom panel,
grouped by reason (largest first) with a by-file alternate, click-through to
the enclosing function’s Inspector source and the call site.

**Touches:** `web/index.html` (bottom tabs: Functions | Diagnostics; controls
ported from `ebfeac4`), `web/viewer.css` (diag-group / entry styles, adapted to
Desktop tokens), `web/viewer.js` (port `collectDiagnostics`, `groupByReason`,
`groupByFile`, reconcile banner; wire click → select file/function +
`fetchSource`; card/Layers badge click → filter canvas to files with that kind).

**Demo:** load a conflict-bearing or scale map → open Diagnostics → groups
sorted by size; row count matches `summary.conflicts + summary.unresolved`
(or mismatch banner); click → canvas selects file, Inspector shows source and
the site; dropped counters explained (not listed as sites).

**Verify:** reconcile against `MapSummary`; spot-check a known reason from
[`unresolved-analysis.md`](../unresolved-analysis.md); keyboard not required.

**Depends on:** Slice 2 (Inspector + source fetch). Reuses `ebfeac4` logic.

---

### Slice 4 — Bottom function DAG

**Goal:** For the selected file(s), show a function call DAG: nodes =
`Function`, edge A→B when a `CallSite` of A has `Resolved(B)`; conflict sites
get multi-target or stub chrome (never a guessed single edge); unresolved as
sink/list chrome; click node → Inspector full function body.

**Touches:** `web/index.html` (Functions tab chrome, scope controls if kept),
`web/viewer.css` (sub-node / edge styles from Desktop spec), `web/viewer.js`
(build graph from map; layout for sub-nodes; sync `selectedSym` with Slice 2).

**Demo:** select a file with cross-function calls → bottom Functions shows
green nodes and edges; click callee → Inspector source swaps; a conflict site
does not collapse to one edge.

**Verify:** small fixture with known A→B resolved edge; conflict fixture shows
non-guessing chrome; empty file shows honest empty state.

**Depends on:** Slice 2 (selection + Inspector). Can land after Slice 3 or
immediately after 2 if diagnostics is deferred one slice — **prefer after 3**
so audit worklist arrives before DAG polish.

**Slots later:** Data-structures / field DAG when types exist — same bottom
tabs, new graph builder; keep a disabled or hidden “Data structures” Page
entry with copy that types/`impl`s are out of analyser scope (see Dropped).

---

### Slice 5 — Desktop chrome completion

**Goal:** Finish loaded-state chrome that Slice 1 stubbed: resizable left/right
rails, theme toggle (light/dark Desktop tokens), header `MapSummary` stats
(all five counters), Architecture sticky fed by factual summary template (not
AI), search dimming Layers + canvas, Pages (Map active; Diff stub removed or
inert; Functions/Diagnostics open bottom), import/empty + Recent
(`localStorage` storing `Repository` JSON), selection-isolation polish from
`my-local-name` tip if cheap.

**Touches:** `web/*` primarily; no new API.

**Demo:** toggle theme; resize panels; sticky shows conflict/unresolved/dropped
counts; Switch project → import; Recent restores a map; search dims non-matches.

**Verify:** manual; Recent round-trip; light/dark screenshot parity vs
[`desktop-horizon-spec.md`](../ui/desktop-horizon-spec.md) §2 tokens.

**Depends on:** Slice 1; benefits from 2–4 already using the panels.

---

### Slice 6 — Live in-process analyse

**Goal:** `POST /api/analyse` with `{"path":"<abs repo>"}` runs
`horizon_engine::build_function_map` in-process, stores the map in
`AppState`, returns `Repository` JSON; UI path field + Analyse replaces the
CLI → Open JSON dance.

**Touches:** new `crates/horizon-server/src/analyse.rs` (handler),
[`app.rs`](../../crates/horizon-server/src/app.rs) / router wire-up,
[`Cargo.toml`](../../crates/horizon-server/Cargo.toml) if needed (engine dep
already present), `web/*` for path control + progress/error; tests beside
host_guard/source style.

**Demo:** paste repo or fixture path → Analyse → map loads on canvas;
Diagnostics and Inspector work without a pre-saved JSON; re-run after analyser
edit.

**Verify:** automated request against `tests/fixtures/…`; confirm no
`std::process` to CLI; Host guard still applies; path not served for
non-loopback (existing middleware).

**Depends on:** Slice 1 shell for a place to put the control; **server half can
parallel Slice 2–5**. UI half after Slice 5 or folded into Slice 5.

**Security:** keep loopback-only; do **not** add `HOST`/`PORT` env binds
([`rust-ui.md`](rust-ui.md) security constraint).

---

### Compact order

```
1  Canvas Map + Layers + edges + badges          ← concurrent / foundation
2  Inspector (docs → /api/source → call sites)   ★ first audit payoff
3  Diagnostics bottom tab (reuse ebfeac4 logic)  ★ worklist
4  Function DAG (bottom Functions tab)
5  Chrome: theme, rails, sticky, Recent, Pages
6  POST /api/analyse (+ UI control)              ‖ 2–5 on server half
```

---

## Dropped from the Desktop design

| Desktop feature | Why dropped |
|---|---|
| **Data structures page / struct DAG / `fieldx`** | Analyser omits types, `impl`s, methods; no honest feed. Leave a future Pages slot or empty-state copy only — do not fabricate nodes. |
| **Diff · PR #142** | Decorative stub; map has no `diff`. Remove or leave inert “coming never” — prefer remove. |
| **Plain English · AI summary** | Heuristic/template, not AI; do not invent LLM text. Replace with joined `doc_comments` + factual one-liner (module path, fn count, summary counters). |
| **Tests as docs** | No tests index in map. |
| **Review flags / risks / LARGE / ORPHAN / unsafe badges** | Old analyzer essays; not in contract. Optional honest “orphan” = no inbound resolved calls — only if defined explicitly in UI copy. |
| **Multi-language Open folder / non-Rust kinds** | Rust-only map. |
| **GitHub URL import** | Unwired in Desktop. |
| **Heuristic file dependency / import edges** | Product reason the old UI was deleted (`dda97d2`). Edges = aggregated **resolved cross-file `CallSite`s** only. |
| **Embedded scan-time source tokens** | Replaced by `/api/source` + `content_hash`. |
| **Persisted hand-placed `x,y`** | Not in map; client layout only (Slice 1). |
| **Filter chips for `fn` / `struct` kinds as card kinds** | Cards are files; prefer resolved/conflict/unresolved/macro filters (Slice 3 badges). |
| **DC / `support.js` runtime** | Rebuild in plain HTML/CSS/JS like the current server viewer — same IA, not the DC compiler. |

**Keep (honest substitutes):** Desktop three-pane + bottom dock composition;
light/dark tokens; file cards + derived edges; Inspector section order
(identity → docs/summary → Source → references); MapSummary in header/sticky;
`from_macro` badges; conflict/unresolved chrome from the old viewer’s
vocabulary.

---

## Remaining work from the superseded [`rust-ui.md`](rust-ui.md) plan

| Item | Status in this rebuild |
|---|---|
| Phases A–C, E (viewer, bytes, source, staleness) | **Done** on the server + old tree UI; Slice 1+ re-homes the UI |
| Phase F diagnostics | **Relocate** into Slice 3 (bottom tab), logic from `ebfeac4` |
| Phase D `POST /api/analyse` | **Still required** — Slice 6 |
| Phase G jump/error chrome | Absorb into Slices 2–3 (id index + clear chrome on bad load) |
| Dark-only palette | **Superseded** — Desktop light/dark |
| Tree as primary IA | **Superseded** — spatial Map |
| Virtualization / a11y / reverse callers / inline call-site spans / editor deep links | Still deferred ([`rust-ui.md`](rust-ui.md) Deferred) |

---

## Future analyser extensions (do not fake now)

When free functions are joined by types / `impl` methods / associated items:

| Slot | What lands |
|---|---|
| File card / Layers | Optional child counts; no new card kind until useful |
| Bottom “Data structures” tab | Struct/type nodes + field or inherent-method edges from new map fields |
| Inspector | Kind chip; source via same `/api/source` ranges once extents exist |
| MapSummary | Today’s `associated_dropped` may shrink as items move into the tree |
| Canvas edges | Still call-derived unless a new edge kind is added to the contract |

Until then: omit or show a one-line empty state — never placeholder structs.

---

## Effort estimate

| Assumption | Value |
|---|---|
| One strong engineer familiar with the repo | yes |
| Slice 1 concurrent / largely done as foundation | yes |
| Server source + map load + security already shipped | yes |
| No Design-Component runtime; plain JS | yes |
| Layout “good enough” for Horizon-self and fixtures first | yes |
| Scale corpora (RA/tokio) may need layout iteration | risk buffer |

| Slice | Calendar (order of magnitude) |
|---|---|
| 1 (foundation) | concurrent / ~1–1.5 weeks if started cold |
| 2 Inspector + source | 3–5 days |
| 3 Diagnostics tab | 2–4 days (logic exists) |
| 4 Function DAG | 4–7 days |
| 5 Chrome / theme / Recent | 3–5 days |
| 6 Live analyse | 2–4 days (server simpler than UI) |
| Integration + layout pain on real maps | 3–7 days |

**Total: about 4–6 weeks** after Slice 1’s foundation is mergeable — **agree
with the spec’s 3–6 weeks**, leaning **4–5** if Slice 1 lands clean and layout
is accepted as iterative. Uncertainty is almost entirely **canvas layout
quality** and **DAG interaction polish**, not the server contract.

Disagree with a 3-week floor only if Slice 1 is incomplete or layout for
100+ file maps becomes a research problem.

---

## Risks (most likely first)

1. **Layout quality dominates** — Desktop assumed hand-placed cards; the map
   has no coordinates. Hierarchical-by-folder layout may look like a messy
   tree; force-directed layouts thrash on dense cross-file graphs. Hundreds of
   files (scale corpora) will overlap or spider-web. Mitigation: ship Slice 1
   with a simple deterministic layout (e.g. by crate/folder columns); treat
   layout tuning as an explicit follow-up, not a blocker for Slices 2–3.
2. **`viewer.js` merge contention** — three-file front end; parallel UI agents
   will conflict. Mitigation: one UI owner at a time; split modules only if
   pain is proven (`map-view.js`, `inspector.js`, `diagnostics.js`).
3. **Selection model complexity** — file vs function vs diagnostics row vs DAG
   node must share one Inspector pipeline; easy to fork state. Mitigation:
   Slice 2 defines the selection API; 3–4 only call into it.
4. **Scale diagnostics lists** — RA-sized unresolved counts; bottom panel
   without virtualization may jank. Mitigation: lazy group bodies (as
   `ebfeac4`); virtualize only if owner reviews those maps in-browser.
5. **Live analyse latency / UX** — large repos block the request. Mitigation:
   progress or “running…”; keep saved-JSON path.

---

## Owner decisions before Slice 2

1. **Diagnostics home** — **Settled:** bottom Diagnostics tab beside Functions
   (this doc’s recommendation), with card/Layers badges as secondary filters.
2. **Data structures Page** — **Settled:** omit entirely until the analyser
   emits structs. No disabled placeholder.
3. **Slice 1 layout acceptance** — is “readable on Horizon-self / fixtures”
   enough to start Inspector, with scale layout as follow-up?
4. **Hard-cut the tree** — Slice 1 removes Tree|Diagnostics; no dual Tree/Map
   mode unless requested (this plan assumes hard-cut).
