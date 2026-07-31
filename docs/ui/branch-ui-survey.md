# Branch UI survey — Codebase Visualizer (`my-local-name`)

Read-only archaeology of UI-carrying branches. Working tree was not modified
except for this file. Reproduce any quoted path with:

```bash
git show my-local-name:<path>
# tip: 32cb6b18c14638afc4730b9a0993c35df08ce7dd
```

---

## Lead answers (blockers)

### Does it show function source code?

**Yes.** The right-hand Inspector has a **Source** section (`data-cv-section="source"`) that renders the selected file’s representative snippet, or — when a function / data-structure is selected in the bottom panel — the **full symbol body**.

There is **no** HTTP endpoint that serves source. Bytes are located **at scan time** inside the analyzer, sliced by a **brace-/indent-heuristic from the function’s start line**, syntax-highlighted into a token array, and **embedded in the scan JSON**. The browser only re-renders that array.

| Step | What happens |
|---|---|
| 1. Ingest | Browser uploads `{path, text}` via `POST /api/scan`, or server reads disk via `GET /api/scan-path?path=…` |
| 2. Extract | Heuristic scanner finds `fn`/`def`/… start **line** (1-based); stores body text for call extraction |
| 3. Slice | `Lang::snippet_at_lines(lines, f.line, 4000)` walks lines from that start, matching `{`/`}` (string/comment-aware) or Python indent, capped at 4000 lines for symbols (18 for file previews) |
| 4. Highlight | `highlight(src, lang)` → `Vec<(String /*text*/, &'static str /*class*/)>` with classes `kw` / `fn` / `ty` / `c` / `""` |
| 5. Serialize | Stored on `detail[fileId].code` (file) and `sub[fileId].fns[].code` / `structs[].code` (symbols) |
| 6. Display | Inspector `<pre><code>{{ sel.codeEl }}</code></pre>`; JS `codeEl(rows, dark)` maps classes to colours |

Reproduce:

```bash
git show my-local-name:src/lang.rs          # snippet_at_lines, highlight
git show my-local-name:src/lib.rs           # FnOut.code population
git show my-local-name:web/index.dc.html    # Inspector + codeEl
```

### What about changed or missing source files?

**Nothing live. No re-read. No staleness check.**

| Concern | Behaviour |
|---|---|
| File hash / content digest | **Absent** |
| mtime / length / etag | **Absent** |
| Cache invalidation | **Absent** (aside from “scan again”) |
| Schema `version` | Present (`"1"`); UI warns if missing or ≠ `"1"` — this is **scanner-output schema**, not source freshness |
| Disk changed after scan | Display still shows the **embedded snapshot** from scan/JSON/localStorage. It does **not** silently re-slice the new file |
| File deleted after scan | Same — snapshot already in memory/JSON; deletion is invisible |
| Wrong-lines-from-disk hazard | **Does not apply** for display: the server never serves fresh file bytes for the Inspector. Hazard is the opposite: **stale-but-stable** source until the user re-scans or loads a new JSON |

`/api/scan-path` *does* re-read disk when invoked, and a fresh `POST /api/scan` uses whatever the browser uploads at that moment. Between those events, the UI is frozen on the last model.

---

## Most recent UI branch

| Field | Value |
|---|---|
| Branch | `my-local-name` |
| Tip | `32cb6b18c14638afc4730b9a0993c35df08ce7dd` |
| Date | 2026-07-17 10:43:26 −0700 |
| Subject | Ignore comments/strings in call edges and isolate panel text selection. |
| UI files | `src/bin/server.rs`, `web/index.dc.html`, `web/support.js` |
| Package | `codebase_visualizer` 0.2.0 (not Horizon function-map) |

### Relation to current branch (`feat/ast-system` @ `1404ac7`)

```
* 1404ac7  feat/ast-system — Follow pub use facade re-exports…
* dda97d2  Replace heuristic scanner with a Rust function-map analyser   ← deletes web/ + server.rs
| * 32cb6b1  my-local-name — (UI polish)
|/
* 3a72a2c  origin/cursor/single-file-support-5079   ← merge-base
```

- Merge-base with `my-local-name`: `3a72a2c`
- `feat/ast-system` is **2 ahead / 1 behind** `my-local-name` (`git rev-list --left-right --count HEAD...my-local-name` → `2 1`)
- Neither tip is an ancestor of the other
- Rewrite commit **`dda97d2`** (2026-07-26) explicitly removed the UI and replaced the heuristic multi-language scanner with the `ra_ap_syntax` function-map analyser. That is why the UI is gone from the current branch — not an accidental delete.

---

## All branches (tip hash, date, subject)

| Branch | Tip | Date | Subject | UI? |
|---|---|---|---|---|
| `feat/ast-system` (HEAD) | `1404ac7` | 2026-07-26 −0700 | Follow pub use facade re-exports across crate boundaries | **No** (dropped in `dda97d2`) |
| `my-local-name` | `32cb6b1` | 2026-07-17 −0700 | Ignore comments/strings in call edges and isolate panel text selection. | **Yes** — complete DC visualizer + server |
| `origin/cursor/single-file-support-5079` | `3a72a2c` | 2026-07-02 UTC | Add single-file support, harden server security, optimize analyzer 2.2x | **Yes** — parent of tip UI |
| `origin/cursor/setup-dev-environment-e237` | `7cde2da` | 2026-07-02 UTC | Add Cursor Cloud dev environment notes (AGENTS.md) | Old 3D only (`codebase-map.html`) |
| `cursor/codebase-visualizer-revamp` (= `origin/…`) | `54a017a` | 2026-06-21 UTC | docs: re-audit comments… | **Yes** — older revamp |
| `main` / `origin/main` | `e07ac11` | 2026-05-02 −0700 | Merge pull request #7 … graph-ux-overhaul | Old 3D (`codebase-map.html` + `vendor/3d-force-graph.min.js`) |

UI path search across all refs (`web/`, `server.rs`, `*.html`/`*.css`/`*.js`, `ui/`, `static/`, `assets/`, `templates/`): only the visualizer branches above plus the legacy 3D HTML on `main`. **No CSS files** on the DC branches — styling is inline / CSS variables set from JS. **`support.js`** is the Design-Component runtime (~1726 lines), not app logic (app logic lives in the `<script data-dc-script>` inside `index.dc.html`).

---

## Server shape

**Dependencies** (`git show my-local-name:Cargo.toml`):

| Crate | Version | Role |
|---|---|---|
| `tiny_http` | **0.12.0** (lockfile) | HTTP server |
| `serde` | 1 (+ derive) | Request/model (de)serialize |
| `serde_json` | 1 | JSON bodies |

No axum, warp, hyper, or tower. No browser-open crate.

**How the map is obtained:** **in-process**. `handle_scan` / `handle_scan_path` call `analyze(...)` / `scan_path(...)` from the library and `serde_json::to_string` the `Model`. No subprocess CLI. The separate `codebase_visualizer` binary is a CLI that prints the same JSON to stdout/file.

**Bind:**

| Setting | Default | Override |
|---|---|---|
| Host | `127.0.0.1` | env `HOST` (must parse as IP; e.g. `0.0.0.0`) |
| Port | `8787` | env `PORT` |
| Auto-open browser | **No** | README says open `http://localhost:8787` yourself |
| Body cap | 64 MiB on `POST /api/scan` | hard-coded `MAX_BODY_BYTES` |
| Host check | When bound to loopback, API requires `Host` ∈ `{localhost,127.0.0.1,::1}` (DNS-rebinding guard) | skipped if non-loopback bind |

**Routes:**

| Method | Path | Params / body | Response |
|---|---|---|---|
| `GET` | `/` or `/index.html` | — | `text/html` — embedded `web/index.dc.html` |
| `GET` | `/support.js` | — | `text/javascript` — embedded `web/support.js` |
| `GET` | `/api/health` | — | `application/json` `{"ok":true}` |
| `GET` | `/api/scan-path` | query `?path=` (percent-decoded); scans server-side dir/file, max 500 000 bytes/file | `application/json` `Model`, or `400` `{error}` |
| `POST` | `/api/scan` | JSON `{ name?: string, files: [{path, text}] }` | `application/json` `Model`, or `400`/`413` `{error}` |
| (any) | `/api/*` with non-local Host on loopback bind | — | `403` `{error}` |
| other | — | — | `404` plain `"Not found"` |

The UI’s import flow uses **`POST /api/scan` only**. `/api/scan-path` is a local/testing convenience; the HTML never calls it.

Reproduce: `git show my-local-name:src/bin/server.rs`

---

## Source display — end-to-end detail

### Where it appears

Right sidebar Inspector, section heading **Source**, with `{{ sel.loc }} lines`. Rendered as:

```html
<pre style="…background:var(--code);…font-family:'JetBrains Mono',monospace;font-size:11px…">
  <code>{{ sel.codeEl }}</code>
</pre>
```

(`git show my-local-name:web/index.dc.html` ≈ lines 343–348)

Selection precedence: bottom-panel symbol (`selectedSym` → `symDetail`) over file card (`detail(selectedId)`).

### How bytes are located (server-side, at analyze time)

Not offsets in the JSON. For each function:

```rust
// git show my-local-name:src/lib.rs  (≈644–650)
let code = info.lang.snippet_at_lines(&file_lines, f.line, 4000);
FnOut { /* … */ code, loc: f.body.lines().count().max(1), … }
```

`snippet_at_lines` (`src/lang.rs`):

1. Index into `lines[start_line - 1]`.
2. Brace languages: accumulate lines until brace depth returns to 0 after first `{`, skipping braces inside strings / `/* */` / `//` (Rust char-vs-lifetime heuristic). Cap `max_lines`.
3. Python: accumulate while indent > declaration indent.
4. Join lines → `highlight(...)`.

File-level `detail.code` uses a **short** preview: start at `main` else first public fn else first fn/type else line 1, **`max_lines = 18`**.

Token wire shape: JSON arrays of two-element arrays `[text, class]` (Rust `Vec<(String, &'static str)>`). UI:

```javascript
// git show my-local-name:web/index.dc.html  (codeEl)
codeEl(rows,dark){
  const C = dark ? {kw:'#c586c0',fn:'#dcdcaa',ty:'#4fc1ff',c:'#6a9955','':'#d4d4d4'}
                 : {kw:'#a626a4',fn:'#4078f2',ty:'#0184bc',c:'#a0a1a7','':'#383a42'};
  return React.createElement('span',null,rows.map((r,i)=>
    React.createElement('span',{key:i,style:{color:C[r[1]]||C['']}},r[0])));
}
```

No Prism/Highlight.js. No modal. No inline expansion on the canvas — Inspector only (plus the same `code` fields available if something else consumed the JSON).

---

## UI features inventory

### Layout regions

1. **Top bar** — brand `{}`, project name, Map / Diff / Functions / Data-structures page controls, zoom when in full sub-view, frame stats, Switch, theme toggle, panel toggles  
2. **Import screen** — Open local folder / source file(s) / scanner `.json`; drag-drop; Recent (localStorage)  
3. **Scanning screen** — progress bar  
4. **Left sidebar (Layers)** — Pages, search (“Find a symbol…”), folder groups expand/collapse (`▸`/`▾`), filter chips (entry/file/fn/struct)  
5. **Center canvas** — Figma-style file cards + SVG dependency edges; sticky Architecture note; zoom/pan; card drag-to-move  
6. **Bottom panel** — per-file function / data-structure DAG (union / intersect / path / single-file modes); expand to full center view  
7. **Right Inspector** — identity, AI-ish summary, **Source**, tests-as-docs, review flags, callers/callees  
8. **Resize rails** — left/right sidebar width drag  

### Interaction

| Feature | Present? | Notes |
|---|---|---|
| Tree / indentation | Yes | Left layers; folders collapsible |
| Expand/collapse | Yes | Folders; sidebars; bottom panel dock/full |
| Resolved / conflict / unresolved call sites | **No** | Different product: heuristic name-matching edges, not Horizon `CallTarget` |
| Click call site → definition | Partial | Inspector **callers/callees** buttons and bottom-panel nodes select symbols; no Horizon-style call-site list |
| Search / filter | Yes | Symbol search + kind filter chips |
| Keyboard shortcuts | **None found** | No `keydown` handlers for navigation |
| Load different map | Yes | Switch → import; Open `.json`; Recent projects |
| Theme | Yes | Light/dark; OS preference seed; user override |
| Diff view | Stub | Button labelled **“Diff · PR #142”** toggles `view:'diff'`; uses optional `node.diff` ∈ `{added,changed,removed}` — analyzer always emits `diff: null` |

### Colour palette (CSS variables from `tokens(dark)`)

**Fonts:** Inter (UI) + JetBrains Mono (code/labels), loaded from Google Fonts.

**Kind accents (hard-coded, not CSS vars):**  
`entry #e8922a` · `file #3b82f6` · `fn #16a35c` · `struct #7c5be0`  
Also: Functions nav `#14ae5c`, Data structures `#9747ff`, entry star / bridge `#f5a623`, selection indigo `#6366f1`.

**Diff legend:** added `#14ae5c` · changed `#f5a623` · removed `#e5484d`.

| Token | Dark | Light |
|---|---|---|
| `--bg` | `#1e1e1e` | `#f2f0eb` |
| `--grid` | `#272727` | `#e3e0d9` |
| `--panel` | `#252525` | `#faf9f7` |
| `--panel2` | `#313131` | `#eceae4` |
| `--border` | `#3a3a3a` | `#dedad2` |
| `--text` | `#e2e2e2` | `#1a1917` |
| `--text2` | `#949494` | `#6b6860` |
| `--text3` | `#5e5e5e` | `#a8a49d` |
| `--sel` | `#6366f1` | `#6366f1` |
| `--selbg` | `rgba(99,102,241,.18)` | `rgba(99,102,241,.1)` |
| `--frame` | `#2e2e2e` | `#fdfcfb` |
| `--frameborder` | `#484848` | `#ccc9c1` |
| `--code` | `#161616` | `#f7f5f0` |
| `--conn` | `#424242` | `#bdbab2` |
| `--sticky` / `--stickytxt` | `#c9b84a` / `#2a2208` | `#f5d84a` / `#3d2e00` |
| `--warn` / `--warnbg` | `#f87171` / `rgba(248,113,113,.12)` | `#c53030` / `rgba(197,48,48,.08)` |
| `--okbg` | `rgba(22,163,92,.12)` | `rgba(22,163,92,.09)` |
| `--shadow` | `rgba(0,0,0,.5)` | `rgba(0,0,0,.07)` |
| `--scroll` | `#444444` | `#cac7bf` |
| `--skel` | `#404040` | `#e8e5de` |
| `--toolbar` | `#1a1a1a` | `#28261f` |

---

## Why it was abandoned

Stated explicitly in rewrite commit `dda97d2` (2026-07-26), which deleted `web/*` and `src/bin/server.rs`:

> The previous implementation scanned several languages with regex heuristics behind a web frontend, and could not distinguish a real call from something that merely looked like one. This replaces it with a structural analyser built on `ra_ap_syntax`…

So the UI was dropped **because the product pivoted** from a multi-language heuristic visualizer to a Rust-only non-guessing function map — not because of a documented UI bug.

### Obviously unfinished / hardcoded

- **Diff · PR #142** — decorative stub; no PR integration  
- **`githubUrl` state** — reserved, unused for import  
- **Architecture sticky** — generated English paragraph, not editable  
- **“Plain English · AI summary”** — template/heuristic text from docs or canned sentences, not an LLM  
- **`node.diff`** — reserved, always null from analyzer  
- Heuristic brace matching still imperfect for call extraction (`body_of` docs note braces in strings ignored only in `snippet_at`, not in `body_of`)

---

## JSON contract drift vs current `crates/horizon-map/src/map.rs` / `json.rs`

These are **different products**. The old UI cannot load a Horizon function map without a full adapter (or rewrite). Drift is not “a few renames” — the root shape is incompatible.

### Old visualizer model (what the UI reads)

Top-level: `version`, `repoName`, `language`, `arch`, `nodes[]`, `edges[][]`, `detail{}`, `folders[]`, `sub{}`, `fnx[][]`, `fieldx[][]`.

Per-file node: `id`, `label`, `kind` (`entry`|`file`|`fn`|`struct`), `x`, `y`, `loc`, `diff?`, `orphan`.  
Detail: `path`, `summary`, `code[[text,class]]`, `loc`, `tests[]`, `callers[]`, `callees[]`, `risks?`, `badges?`.  
Sub: `{ fns: FnOut[], structs: StructOut[] }` with per-symbol `code`, `calls`/`fields`, `sig`, `doc`, `loc`.

### Current Horizon model (what `feat/ast-system` emits)

Containment tree: `Repository { root, crates[], summary }` → `Crate` → `Folder` → `File` → `Function` + file-level `call_sites` / `doc_comments`.  
Call edges: `CallSite { call_path, line, byte_start, byte_end, target, from_macro? }` with adjacent-tagged `CallTarget` `{ kind, data }`.

### Breakage table (old UI ← today’s map)

| Today’s field / shape | Old UI | Effect if fed raw |
|---|---|---|
| Root `root` / `crates` / `summary` | Expects `nodes`/`edges`/`detail`/… | `validateModel` fails: “Scanner output has no nodes.” |
| `Function.id` / `module_path` / `line` | Expects canvas `nodes` with `x,y,kind` | No cards |
| `CallSite` + `CallTarget` (`kind`+`data`) | No concept of call sites / conflict / unresolved | Entire resolution story invisible |
| `from_macro` | Unknown | Ignored (N/A — never read) |
| `MapSummary.external_dropped` / `constructor_dropped` / `associated_dropped` | Unknown | Ignored |
| `Dependency.rename` | No dependency objects in UI model | N/A |
| `Crate.is_library` | Unknown | N/A |
| `DocComment` on File/Function | Old has string `doc` on symbols only | Shape mismatch |
| `byte_start` / `byte_end` | Not in old model; source was pre-sliced `code` | Old UI cannot highlight call ranges |
| File `call_sites` (module-level) | Unknown | Dropped |
| Adjacent tagging of targets | Old edges are bare id pairs / `fnx` quads | Incompatible |

### Reverse: fields the old UI requires that Horizon does not emit

`version`, `repoName`, `language`, `arch`, `nodes`, `edges`, `detail.code` token arrays, `folders`, `sub`, `fnx`, `fieldx`, canvas coordinates, `orphan`, `tests`, `risks`, `badges`, per-symbol highlighted `code`.

**Port implication:** adapting the DC visualizer to Horizon is a **new frontend data layer + layout model**, not a field-mapping exercise. The Desktop throwaway viewer already speaks today’s JSON.

---

## Cheap note vs Desktop viewer

(`C:\Users\kouat\OneDrive\Desktop\Horizon Map Viewer\` — detailed by another agent in `docs/ui/old-viewer-spec.md`.)

| | Branch UI (`my-local-name`) | Desktop Horizon Map Viewer |
|---|---|---|
| Data contract | Heuristic visualizer `Model` | Current `Repository` function map |
| Source preview | **Yes** (embedded highlighted bodies) | **No** (`byte_*` unused; spec lists source preview as a gap) |
| Conflict / unresolved / `from_macro` | No | Yes (pills, filters, expand) |
| Server | `tiny_http` in-process scan | Static HTML; load JSON via file picker |
| Chrome richness | High (canvas, inspector, themes, sidebars) | Low (tree + toolbar) |

---

## Assessment: better starting point for the Horizon port?

**Prefer the Desktop viewer as the starting point for the function-map port**, not this branch UI.

Reasons, briefly:

1. **Contract fit** — Desktop already consumes `CallTarget` adjacent tagging, `from_macro`, summary counters, containment tree. The branch UI would need its entire model layer replaced before any pixel is honest.
2. **Product intent** — Branch UI optimises for multi-language file-graph exploration; Horizon’s job is non-guessing call resolution. Porting chrome without the data model buys the wrong UX.
3. **What to steal from the branch UI** — Inspector source panel pattern (especially once Horizon can serve or embed snippets via `byte_start`/`byte_end` or re-slice from disk), light/dark tokens, resizable inspector — as **features**, not as the codebase base.

If the goal were “revive the Figma-style visualizer for the old scanner,” `my-local-name` @ `32cb6b1` is the tip to check out (via worktree). For Horizon, treat it as a **reference for source-in-inspector UX**, not the port trunk.
