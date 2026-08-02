# Horizon web viewer — functionality catalogue (pre-IDE reference)

The standalone viewer `horizon-server` serves at `/` (assets from
`crates/horizon-server/web/`). It predates the IDE and is the **reference for
intended behaviour**: the IDE's Map EditorPane embeds a *forked copy* of the same
UI (`ide/contrib/horizon/browser/media/`), so anything working here should work
in the IDE unless it is explicitly IDE-only.

Captured live against the running sidecar with the Horizon repo analysed
(31 frames · 37 links, 9 crates, 30 files). **91 buttons, 72 visible, 3 inputs.**

> The two copies have **diverged** (~707 diff lines). Treat differences as
> findings, not noise.

## Layout regions

```text
┌─ topbar ──────────────────────────────────────────────────────────┐
│ ☰  {}   Horizon / Codebase Map   31 frames · 37 links   ⧉  ☾  ☰  │
├──────────────┬─────────────────────────────┬──────────────────────┤
│ PAGES        │                             │  INSPECTOR           │
│ LAYERS       │        CANVAS               │  (identity, docs,    │
│  (search)    │        (+ zoom HUD)         │   Functions list,    │
│ FILTER       │                             │   call sites)        │
├──────────────┴─────────────────────────────┴──────────────────────┤
│ BOTTOM PANEL — tabs: Functions | Diagnostics                      │
└───────────────────────────────────────────────────────────────────┘
```

## Controls

### Topbar

| Control | id | Purpose |
|---|---|---|
| ☰ left | `toggle-left` | Toggle the layers panel |
| Switch project | `switch-project` | Re-open the analyse/project picker |
| ☀/☾ | `toggle-theme` | Switch light ⇄ dark theme |
| ☰ right | `toggle-right` | Toggle the inspector panel |

Topbar centre is a live label: `Horizon / Codebase Map · 31 frames · 37 links`.

### Pages (left rail)

| Page | id | `data-page` | Purpose |
|---|---|---|---|
| Codebase map | `page-map` | `map` | The frame/link canvas |
| Diff · PR #142 | `page-diff` | `diff` | **Placeholder** — "no diff source in the map contract" |
| Functions | `page-fns` | `fns` | Function call DAG for the selected file |
| Diagnostics | `page-dock` | `diag` | Diagnostics worklist |

### Layers

- `#layer-search` — text input, placeholder **"Find a file…"**; filters the tree.
- `#layer-rows` — **49 rows**. Crate rows (`▾horizon-engine`) expand/collapse;
  file rows carry `data-id` = absolute path. Markers: `★` entry point, `#` plain file.

### Filter chips

| Chip | `data-kind` | Notes |
|---|---|---|
| Entry | `entry` | Entry-point frames |
| Files | `file` | File frames |
| Fns | `fn` | "Dim files that have free functions when off" |
| Types | `struct` | **Placeholder** — "type definitions live in horizon-types, not this map" |

### Canvas

- Zoom HUD: `#zoom-out`, `#zoom-reset` (Fit to content), `#zoom-in`.
- 31 frames and **39 rendered SVG edge paths** for 37 links.
- Frames are pannable/draggable; selection drives the inspector.

### Inspector (right)

- Identity block: file/function name, path, `ENTRY` badge, `crate` + `N functions` chips.
- `.insp-back` — back to previous selection (disabled: "No previous selection").
- **Documentation** — module `//!` docs, or "No module documentation (//!)".
- **Functions** — `.fn-list-item` per free function, showing name + `L<line>`.
  For `crates/horizon/src/main.rs`: `main L52`, `write_summary_line L99`,
  `count_map L129`, `count_crate L141`, `count_folder L151`, `count_file L161`,
  `plural L178`.
- **Call sites · references** — or "No call sites in this function."
- Selecting a function fetches its body from `GET /api/source` and renders
  highlighted tokens in a `.source-well` (collapsed at 320px with an expand toggle).

### Bottom panel

| Control | id / data | Purpose |
|---|---|---|
| Functions tab | `tab-functions` / `bottomTab=fns` | Call-DAG view |
| Diagnostics tab | `tab-diagnostics` / `bottomTab=diag` | Diagnostics list |
| By reason / By file | `data-group=reason\|file` | Diagnostics grouping |
| 1 hop / 2 hops / All hops | `data-depth=1\|2\|all` | DAG traversal depth |
| Fit DAG in view | `fns-fit` | Fit the DAG |
| DAG zoom | `fns-zoom-out`, `fns-zoom-reset`, `fns-zoom-in` | DAG zoom HUD |
| Close | `bottom-close` | Close the bottom panel |

### Project / data entry (hidden until needed)

| Control | id | Purpose |
|---|---|---|
| Analyse folder | `open-folder` | "Analyse a local Cargo repository via POST /api/analyse" |
| Path field | `analyse-path` | `/path/to/cargo/repo` |
| Analyse | `analyse-run` | Runs the analysis |
| Hide path form | `analyse-cancel-form` | Dismiss the form |
| Open map JSON | `open-json` | Drop/choose a Horizon Repository `.json` |
| JSON file input | `json-input` | The file picker itself |
| Clear error | `clear-error` | Dismiss the error banner |
| Single-file import | — | **Placeholder** — "not in this rebuild" |

## Observed behaviour — what each control actually does

Every row below was exercised against the live viewer and the resulting state
change recorded. This is the expected-result table for IDE comparison.

### Chrome

| Interaction | Observed result |
|---|---|
| `#toggle-left` | Layers panel width **236px ⇄ 0px**; `.left-aside` gains/loses `collapsed` |
| `#toggle-right` | Inspector width **316px ⇄ 0px**; `.right-aside` gains/loses `collapsed` |
| `#toggle-theme` | `.app[data-theme]` flips **light ⇄ dark** |

### Pages

| Interaction | Observed result |
|---|---|
| **Functions** (`#page-fns`) | Opens bottom panel **0 → 248px**, canvas shrinks **1032 → 784**, bottom tab → `tab-functions` |
| **Diagnostics** (`#page-dock`) | Bottom tab → `tab-diagnostics` (panel already open) |
| **Diff** (`#page-diff`) | **Nothing at all** — does not even take the active class (placeholder) |
| **Map** (`#page-map`) | Closes bottom panel **248 → 0**, canvas restores **784 → 1032** |

### Filters and search — "dim, don't hide"

Nothing is ever removed from the DOM; non-matching items get a `dim` class.

| Interaction | Observed result |
|---|---|
| **Entry** chip | Dims **8** canvas items; toggles back off |
| **Fns** chip | Dims **52** items |
| **Files** chip | Same dimming behaviour for file frames |
| **Types** chip | **No change** (placeholder) |
| Type `extract` in `#layer-search` | Dims **29 / 49** layer rows and **30 / 31** canvas cards; clearing restores all |

Matching is case-insensitive across name, module path, file path and crate name
(`cardVisible()`), and drives **both** the tree and the canvas.

### Zoom

| Interaction | Observed result |
|---|---|
| `#zoom-in` | 25% → 37% → 49% (~×1.33 per step) |
| `#zoom-out` | 49% → 37% |
| `#zoom-reset` | Fit to content (37% → 29% at this map size) |

### Inspector — the core navigation loop

| Interaction | Observed result |
|---|---|
| Click a **file row** in layers | Inspector switches to FILE view: title, `FILE`/`ENTRY` badge, path, module, function count. Functions list repopulates (e.g. **7 → 49** for `extract.rs`) |
| Click a **function** in the Functions list | Inspector switches to **FN view**: title becomes `crate::extract::is_item_macro_allowlisted · L230`; **the functions list disappears (49 → 0)**; a `.source-well` appears with the real highlighted source (doc comments + body); `.source-expand` toggle appears; **Back becomes enabled** |
| Click `.source-expand` | Toggles `.source-frame.expanded` (releases the 320px clamp) |
| Click **Back** | Returns FN → FILE view: functions list restored (0 → 49), source well removed |

> **Design consequence.** Previewing a function's source *costs you the list* —
> clicking navigates away from it. That is precisely the gap the IDE's hover
> preview fills: read the body without losing your place.

### Bottom panel

| Interaction | Observed result |
|---|---|
| `#tab-functions` / `#tab-diagnostics` | Switches the active bottom tab |
| `#bottom-close` | Closes the panel (canvas reclaims the height) |
| Depth `1 / 2 / all`, group `by reason / by file` | Only meaningful once a DAG/diagnostic selection exists |

## Known placeholders (expected non-functional in both versions)

1. **Diff · PR #142** page — no diff source in the map contract.
2. **Types** filter chip — types are not in this map.
3. **Single-file import** — not in this rebuild.

Do not report these as IDE regressions.

## HTTP surface the UI depends on

| Endpoint | Used by |
|---|---|
| `GET /api/health` | sidecar liveness |
| `GET /api/map` · `POST /api/map` | load / push a map |
| `GET /api/analyse` · `POST /api/analyse` | run + poll analysis |
| `GET /api/source` | inspector source preview (byte range + content hash) |

## IDE-only additions (not in this reference)

- Activity-bar, status-bar and editor-title entries; the Horizon sidebar pane.
- Command palette commands (Open/Toggle/Hide/Show Map, Analyse Workspace,
  Choose Horizon Folder, Trigger Inspection Completions).
- Host **Inspection canvas** (rust-analyzer path) — the webview defers to it.
- Workspace-folder awareness and auto-analyse on open.
- **Hover preview** on the Functions list (added during this testing pass;
  present only in the IDE copy of `viewer.js`).

## IDE comparison run

Every interaction above was replayed inside the IDE's Map EditorPane with the
same probes. **No regressions.**

| Interaction | Reference | IDE | Verdict |
|---|---|---|---|
| Toggle layers | 236 ⇄ 0, `collapsed` | 230 ⇄ 0, `collapsed` | match |
| Toggle inspector | 316 ⇄ 0 | 316 ⇄ 0 | match |
| Theme toggle | light ⇄ dark | dark ⇄ light | match |
| Page Functions | bottom 0→248, canvas −248 | bottom 0→248, canvas 764→516 | match |
| Page Diagnostics | tab → diagnostics | tab → diagnostics | match |
| Page Diff | no change | no change | match (placeholder) |
| Page Map | bottom → 0, canvas restored | bottom → 0, canvas 516→764 | match |
| Chip Entry | 8 dimmed (4 rows + 4 cards) | rows 4, cards 4 | match |
| Chip Fns | 52 dimmed (26 + 26) | rows 26, cards 26 | match |
| Chip Types | no change | no change | match (placeholder) |
| Search `extract` | 29 rows, 30 cards dimmed | 29 rows, 29 cards dimmed | match |
| Zoom in / out | 25→37→49 / 49→37 | 25→37→49 / 49→37 | match |
| Zoom reset | → 29% | → 25% | match (fit depends on canvas height) |
| Click file row | fn list 7 → 49, FILE view | fn list 7 → 49, FILE view | match |
| Click function | list → 0, source well, Back enabled | list → 0, source well, Back enabled | match |
| Source expand | toggles `.expanded` | toggles `.expanded` | match |
| Back | list restored, well removed | list restored, well removed | match |
| Bottom tabs / close | switches / closes | switches / closes | match |

Workbench renderer errors during the run: **none**.

### IDE-only behaviour observed

- Selecting a file in the Map also opens it **read-only in an editor tab**
  ("Inspect: main.rs"), alongside the Map — the reference has no editor.
- **rust-analyzer** runs in the IDE (status bar), giving real diagnostics and
  `N implementations` / `Run | Debug` code lenses in that editor.
- **Hover preview** on the Functions list (new): hovering shows
  `main · L52–101` with highlighted source while the list stays intact
  (`fnItems` remains 49 during hover) — the reference cannot do this, because
  clicking is the only way to see source and it destroys the list.

### Defects found in the new hover preview, and fixed

Screenshots of the interaction exposed three problems not visible from state probes:

1. **Covered the layers tree** — the popover was placed left of the anchor with
   only a viewport clamp, landing at x=193 over the LAYERS panel
   (`overlapsLayers: true`). Now clamped to the right edge of `#left-aside`, so
   it floats over the canvas only.
2. **Clipped code** — `max-width: min(560px, 46vw)` resolved to 431px in the
   webview, cutting lines mid-identifier with a scrollbar the popover cannot
   receive (`pointer-events: none`). Widened to `min(720px, 62vw)` with a
   `min-width` floor.
3. **Showed with no visible anchor** — hovering a row scrolled out of the list
   parked the popover in a corner pointing at nothing. Now suppressed unless the
   anchor is on screen.
4. **Covered the Functions list itself** — after (1) and (2), the wider popover
   (580px) reached back across the hovered row, hiding the names of the
   neighbouring functions (only their `L99` / `L129` badges showed). Hiding the
   list you are navigating is the same fault as hiding the tree. `max-width` is
   now capped to the free gutter between the tree and the hovered row, and the
   peek flips to the other side when that gutter is under 300px.

5. **Transparent, uncoloured popover** — the element was appended to
   `document.body`, but every theme token (`--panel`, `--border`, `--tok-*`) is
   declared on `.app[data-theme]`. Outside `.app` those `var()`s resolved to
   nothing: `background: rgba(0,0,0,0)`, so the map cards showed straight
   through the source, and the syntax highlighting was silently colourless.
   Now appended to `.app` (which sets no transform, so `position: fixed` still
   resolves against the viewport).

Each was found by **screenshotting after the interaction** — all five were
invisible to state probes, which reported the feature working correctly.
Defect 5 in particular was mis-diagnosed from the screenshot alone as a
z-index problem; `getComputedStyle` showed z-index 60 was already the highest
and the real cause was variable scoping.

## Map selection no longer takes over the editor

Clicking in the Map used to open editors as a side effect of *selection*:

| Gesture | Before | After |
|---|---|---|
| Canvas file card | opened a read-only editor + toast | nothing |
| Layer row | opened a read-only editor + toast | nothing |
| Function in Inspector | opened a read-only editor, split the group, toast | nothing (selection is recorded) |
| `Horizon: Open Selected Function in Editor` | — | opens the inspection editor deliberately |

The rust-analyzer inspection editor is kept, but it is now opt-in: it is what
backs `Horizon: Trigger Inspection Completions`, so deleting it would remove
completions/diagnostics for a selected function entirely.

## How to use this when testing the IDE

For each row above: perform the interaction in the IDE, screenshot the result,
and compare against this reference. Differences fall into three buckets —
*IDE-only addition*, *known placeholder*, or *regression to fix*.
