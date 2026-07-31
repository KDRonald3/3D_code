# Horizon Function-Map Review UI

**Status:** draft requirements + vertical build plan  
**Date:** 30 July 2026  
**Branch:** `feat/ast-system`  
**Companion specs:** [`rust-function-map.md`](rust-function-map.md),
[`json-output.md`](../json-output.md),
[`old-viewer-spec.md`](../ui/old-viewer-spec.md),
[`branch-ui-survey.md`](../ui/branch-ui-survey.md),
[`workspace-split-analysis.md`](../workspace-split-analysis.md),
[`unresolved-analysis.md`](../unresolved-analysis.md)

Filename and section shape follow [`rust-function-map.md`](rust-function-map.md)
(Summary → Goals → Non-goals → Users → Behaviour → Decisions → Open → Deferred,
then a phase plan).

## Summary

Horizon already emits a JSON function map. This document specifies a **local
web UI, living in this repository**, that loads that map (live or from a saved
file) and lets the owner **audit whether the analyser got the calls, conflicts,
unresolved sites, and drop counters right** — with the function’s real source
code beside the extracted call sites.

The front end is the Desktop throwaway viewer
([`old-viewer-spec.md`](../ui/old-viewer-spec.md)) adapted into hand-written
HTML/CSS/JS served by an `axum` binary in `horizon-server`. It is **not** a
WASM rewrite and not a native GUI.

**Framing shift (record, do not paper over):** earlier owner decisions treated
the UI as a first-class production surface. The owner has since clarified,
verbatim, that this *might be temporary UI* whose job is to help review the
current AST / function-map pipeline. The technical stack below still stands
(Rust server, in-repo assets, five-crate layout); the **purpose, lifespan, and
phase priority** are reframed around review value. Decisions made under the
production framing that are worth revisiting under the temporary-review framing
are listed under [Decisions made under the production framing](#decisions-made-under-the-production-framing).

## Goals

- Put the function map on screen quickly so the owner can walk real call sites
  against real source.
- Show every incomplete edge honestly: `conflict` (all candidates + reason),
  `unresolved` (reason), `from_macro` provenance, and `MapSummary` counters
  including the three `*_dropped` tallies.
- Support two load paths: **live in-process analysis** of a repository path,
  and **open a previously saved map JSON**.
- Show the **source text of a free function** next to that function’s call
  sites (core review loop — not a polish extra).
- Stay a **local development tool**: loopback bind, OS-assigned ephemeral port,
  open the browser; no auth.

## Non-goals

| Item | Status |
|---|---|
| WASM front end | Non-goal |
| Native desktop GUI | Non-goal |
| Hosted multi-user service | Non-goal |
| Authentication | Non-goal (loopback-only; see security constraint) |
| Methods / `impl` items in the map | Permanently out of map scope ([`rust-function-map.md`](rust-function-map.md)) |
| Click-to-open-in-editor | Deferred |
| Reverse lookup (all callers of a function) | Deferred |
| Light theme / theme toggle | Out (match old viewer: dark only) |
| Accessibility hardening (`aria-*`, tree roles, live regions) | Deferred — not needed for solo review |
| Virtualization of the tree | Deferred **unless** reviewing maps the size of rust-analyzer / tokio (see Deferred) |
| Faithful port of the abandoned Design-Component visualizer on `my-local-name` | Non-goal — wrong JSON contract ([`branch-ui-survey.md`](../ui/branch-ui-survey.md)) |

## Users and use cases

| User | Job |
|---|---|
| **Primary: the owner** | After changing extract/resolve, re-analyse Horizon itself (or a fixture / scale corpus) and check whether call sites, conflicts, unresolved reasons, macro recovery, and drop counters match the source. |
| Secondary (hypothetical) | Anyone with a saved `.json` map who wants a local tree view. Not a design driver. |

Use cases, in priority order:

1. Load a map → expand a function → read its **source** and the ordered
   `call_sites` Horizon attached; judge extraction/resolution by eye.
2. Filter to functions with conflicts or unresolved sites; jump candidates;
   read reasons.
3. Read `MapSummary` (including `external_dropped`, `constructor_dropped`,
   `associated_dropped`) and compare against expectations from
   [`unresolved-analysis.md`](../unresolved-analysis.md).
4. Spot `from_macro: true` sites (purple badge) and treat them as
   lower-certainty recovery.
5. Re-run **live** analysis after editing the analyser or the target repo,
   without a separate CLI → `data.js` dance.
6. *(Recommended, not yet approved)* Work a repository-wide list of every
   conflict and unresolved site systematically.

## Behaviour

### Inputs

| Input | How |
|---|---|
| Saved map JSON | Browser file picker (client-side parse), **or** server endpoint that reads a path / uploaded bytes and returns `Repository` JSON |
| Live repository path | Browser submits an absolute path; server calls `horizon_engine::build_function_map` in-process and returns the map |
| Source file bytes | Server reads `File.path` from disk and slices `[Function.byte_start, Function.byte_end)` (fields being added — see Decisions) |

### Outputs (what the UI shows)

Match the old viewer unless noted. Layout remains a single-column dark page:
sticky header → summary bar → toolbar → tree (see
[`old-viewer-spec.md`](../ui/old-viewer-spec.md) §1–5).

| Surface | Required for review |
|---|---|
| Tree: crate → folder → file → function | yes |
| Call-site cards: `resolved` / `conflict` / `unresolved` | yes |
| Conflict candidates + reason; unresolved reason | yes |
| `from_macro` → purple `macro` badge | yes (already in old viewer) |
| Summary pills: conflicts, unresolved, **and** the three `*_dropped` counters | yes — old viewer already consumed all five `MapSummary` fields; keep them visible |
| Function **source panel** (new) | yes — core |
| Docs blocks (`doc_comments[].text`) | yes (port as-is; `kind` still unused) |
| `Crate.dependencies` / `Crate.roots` | ignore for v1 (same as old viewer) |

### Colour palette

Port tokens from the old viewer unchanged:

| Token | Value | Role |
|---|---|---|
| `--bg` | `#0f1218` | Page background |
| `--bg-elevated` | `#171b24` | Pills, search, call-site cards |
| `--bg-hover` | `#1e2430` | Row hover |
| `--border` | `#2a3140` | Borders |
| `--text` | `#e6e9ef` | Primary |
| `--text-muted` | `#9aa3b5` | Secondary |
| `--text-dim` | `#6b7385` | Tertiary |
| `--accent` | `#7eb8ff` | Links / focus |
| `--resolved` | `#5dce8a` | Resolved + `lib` tag |
| `--conflict` | `#f0b35a` | Conflict |
| `--unresolved` | `#f07178` | Unresolved |
| `--macro` | `#c3a6ff` | Macro-recovered badge |

Fonts: `--font-ui` Segoe UI / Helvetica Neue; `--font-mono` Cascadia Code /
Consolas / Menlo. Source panel uses the mono stack.

### Endpoints (`horizon-server`)

Bind: `127.0.0.1:0` (ephemeral). Print `http://{addr}` and open the default
browser. Do **not** expose a bind-address flag in v1.

| Method | Path | Body / query | Response |
|---|---|---|---|
| `GET` | `/` | — | `index.html` |
| `GET` | `/static/*` | — | `viewer.css`, `viewer.js`, … |
| `GET` | `/api/health` | — | `{"ok":true}` |
| `POST` | `/api/analyse` | JSON `{"path":"<abs repo>"}` | `Repository` JSON, or `4xx` `{error}` |
| `POST` | `/api/map` | JSON map body, **or** multipart / path load — pick one in implementation; deserialize via [`map_from_slice`](../../crates/horizon-map/src/json.rs) | Validated `Repository` or `4xx` |
| `GET` | `/api/source` | `path`, `byte_start`, `byte_end` (and optionally `expected_hash` once hashing lands) | `{ text, … }` or structured error (`missing` / `stale` / `range`) |

Live analysis must call the engine **in-process** (`horizon-server` already
depends on `horizon-engine` per [`Cargo.toml`](../../crates/horizon-server/Cargo.toml)).
No `std::process` to the CLI.

**Security constraint:** bind address must not become configurable without first
locking down the repo-path / file-path parameters. A browser-chosen path against
a non-loopback bind would let a remote caller read arbitrary directories. Keep
loopback-only until that lock-down exists. The abandoned branch allowed
`HOST`/`PORT` env overrides ([`branch-ui-survey.md`](../ui/branch-ui-survey.md));
that must **not** be copied.

### Interactions

Port from [`old-viewer-spec.md`](../ui/old-viewer-spec.md) §6:

| Trigger | Effect |
|---|---|
| Expand tree node | Lazy-build children; twisty ▶/▼ |
| Click resolved / candidate id | Jump to function (see rough edges — fix brute-force) |
| Search + “Has conflicts” / “Has unresolved” chips | Filter (chip OR semantics retained) |
| Open JSON… | Load saved map |
| Analyse path (new chrome) | `POST /api/analyse` → `loadMap` |
| Expand function (new) | Fetch `/api/source` and show source above or beside call sites |

### JSON fields the UI must honour

Field names from [`map.rs`](../../crates/horizon-map/src/map.rs) /
[`json-output.md`](../json-output.md):

| Path | Fields |
|---|---|
| `Repository` | `root`, `crates`, `summary` |
| `MapSummary` | `conflicts`, `unresolved`, `external_dropped`, `constructor_dropped`, `associated_dropped` |
| `Crate` | `name`, `rustc_name`, `is_library`, `edition`, `folders`, `files` (`roots`, `dependencies` ignored) |
| `Folder` | `path`, `folders`, `files` |
| `File` | `path`, `module_path`, `functions`, `call_sites`, `doc_comments` (+ planned `content_hash` — Open questions) |
| `Function` | `id`, `name`, `module_path`, `line`, `call_sites`, `doc_comments`, **`byte_start`, `byte_end` (to add)** |
| `DocComment` | `text` (`kind` ignored) |
| `CallSite` | `call_path`, `line`, `byte_start`, `byte_end`, `target`, `from_macro` |
| `CallTarget` | adjacent tag `kind` + `data` (`resolved` \| `conflict` \| `unresolved`) |

`CallTarget` wire shapes (unchanged):

```json
{ "kind": "resolved", "data": "crate_key::path" }
{ "kind": "conflict", "data": { "candidates": ["…"], "reason": "…" } }
{ "kind": "unresolved", "data": { "reason": "…" } }
```

## Affected code

| Area | Paths |
|---|---|
| Contract | [`crates/horizon-map/src/map.rs`](../../crates/horizon-map/src/map.rs), [`crates/horizon-map/src/json.rs`](../../crates/horizon-map/src/json.rs), [`docs/json-output.md`](../json-output.md) |
| Extractor (populate function spans) | [`crates/horizon-engine/src/extract.rs`](../../crates/horizon-engine/src/extract.rs) — already has `func.syntax().text_range()` at extract time ([~L261–L274](../../crates/horizon-engine/src/extract.rs)) |
| Engine entry | `horizon_engine::build_function_map` |
| Server | [`crates/horizon-server/src/main.rs`](../../crates/horizon-server/src/main.rs) (skeleton today), plus new modules for routes / static / source |
| Static UI | new under `crates/horizon-server/web/` (or `static/`) — adapted from Desktop viewer |
| Docs | this file; later a one-line pointer from README |

Groundwork **already done** (not a phase): five-crate workspace
([`Cargo.toml`](../../Cargo.toml)), `horizon-server` binary that binds
`127.0.0.1:0` and serves a placeholder ([`main.rs`](../../crates/horizon-server/src/main.rs)).

## Edge cases and constraints

- Paths in the map are absolute host paths. Source fetch must reject paths
  outside the analysed `Repository.root` (prefix check after canonicalize).
- UTF-8 byte offsets (`u32`) match `ra_ap_syntax` ranges; do not reinterpret as
  chars or UTF-16.
- Mid-edit / broken Rust still yields a map ([`rust-function-map.md`](rust-function-map.md));
  the UI must render partial trees and incomplete edges without pretending they
  are errors of the UI.
- Parse failure of a user-selected JSON must clear **summary and toolbar**, not
  leave stale chrome (old-viewer rough edge #7 — fix).
- Large corpora (rust-analyzer ~15k kept sites, 1.5k unresolved per
  [`unresolved-analysis.md`](../unresolved-analysis.md)) stress the non-virtualized
  tree; see Deferred.
- Never guess a call target in the UI; display every candidate.

## Decisions, with reasoning

### Purpose: audit the analyser (reframed)

**Decision:** primary job is reviewing Horizon’s own function-map output for
correctness, not shipping a polished end-user product. Lifespan is uncertain
(“might be temporary”).

*Why:* owner clarification. Source-beside-call-sites and incomplete-edge
surfaces beat polish. Production stack choices already made are kept where they
still serve review (in-repo Rust server, loopback, live analyse) but phase order
and deferred scope follow review value.

### Adapt the Desktop viewer, not the branch visualizer

**Decision:** port [`old-viewer-spec.md`](../ui/old-viewer-spec.md) HTML/CSS/JS
into `horizon-server`. Do not revive `my-local-name`’s Design-Component UI.

*Why:* Desktop viewer already speaks today’s `Repository` / `CallTarget`
contract. The branch UI expects an incompatible heuristic `Model`
([`branch-ui-survey.md`](../ui/branch-ui-survey.md) §JSON contract drift).
Owner instruction: “don’t change much. take the previous UI, adapt it.”

### `horizon-server` + in-process engine

**Decision:** `axum` server in `horizon-server`; live analysis calls
`horizon-engine` in-process; always depends on the engine (not an optional
feature).

*Why:* settled owner decision. Matches abandoned branch’s in-process scan
pattern, with `axum` instead of `tiny_http`. Avoids CLI subprocess and keeps one
analysis code path with the CLI.

### Loopback + ephemeral port + open browser

**Decision:** bind `127.0.0.1:0` only; print URL; open browser; no auth.

*Why:* local tool. Security: configurable bind without locking repo-path would
be directory disclosure. Explicitly **do not** copy the old `HOST`/`PORT` env
escape hatch until path inputs are locked down.

### Both live analyse and saved JSON

**Decision:** both required.

*Why:* live for the edit→re-analyse→inspect loop; saved JSON for maps already
produced (scale results under `%TEMP%\horizon-scale-results\`, fixture sidecars,
CLI output).

### `Function.byte_start` / `Function.byte_end`

**Decision:** add two `u32` fields to [`Function`](../../crates/horizon-map/src/map.rs)
(same meaning as on [`CallSite`](../../crates/horizon-map/src/map.rs) L397–L413:
UTF-8 start and one-past-end of the function item). Populate in extract from
`ast::Fn`’s `text_range()` (full item, including signature and body — what the
owner needs to audit).

*Why:* `CallSite` already carries ranges for editor highlighting; `Function`
today has only 1-based `line` of the `fn` keyword ([L435–L436](../../crates/horizon-map/src/map.rs)).
Rejected alternatives: re-parse with `ra_ap_syntax` on the server (duplicated
logic); slice from this function’s line to the next function’s line (wrong when
non-`fn` items sit between). Extract already touches the syntax node
([`extract.rs` ~L249–L274](../../crates/horizon-engine/src/extract.rs)).

**Contradiction with today’s code:** these fields are **not present yet**.
Until Phase B lands, source display cannot use the contract.

### Source display: disk slice by byte range (not embedded tokens)

**Decision:** server reads the file and returns the UTF-8 slice; UI renders it
in a `<pre><code>` panel under the expanded function (plain text for v1; no
syntax-highlight token array required).

*Why:* settled with the byte-range contract. Differs from `my-local-name`, which
embedded highlighted `[text, class]` arrays at scan time and never re-read disk
for the Inspector ([`branch-ui-survey.md`](../ui/branch-ui-survey.md) §Lead
answers).

### Staleness when the file changed or disappeared

**What the other branch did:** embedded source in the scan JSON at analyse time.
**No** content hash, mtime, or re-read for display. After disk changed or a file
was deleted, the UI still showed the **snapshot**. The wrong-lines-from-disk
hazard **did not apply**; the opposite hazard applied (stale-but-stable until
re-scan). Schema `version` checked scanner-output shape, not source freshness.

**Precedent for offset-based disk reads:** **none.** The branch never served
fresh file bytes for the source panel. Following “embed forever” literally would
contradict the settled `byte_start`/`byte_end` + disk-slice decision.

**Recommendation (review-weighted — owner has not formally closed this):**

| Mode | Behaviour |
|---|---|
| **Live `POST /api/analyse`** | Map and source are co-produced from the same disk state in one session. Slice with `Function.byte_*`. No extra staleness ceremony inside that response. |
| **Saved JSON** | At extract time, store a per-file content digest on `File` (e.g. `content_hash`: hex sha256 of file bytes — small contract add). `GET /api/source` recomputes the hash; on mismatch return `stale` and show a banner (“source changed since map was built — re-analyse”) **without** showing a sliced body; on missing file return `missing` and hide the panel with that reason; on range out of bounds return `range`. |
| **Reject** | Silent best-effort slice of a possibly moved file (shows wrong “evidence” during audit). Embed full function bodies in JSON (duplicates the rejected approach; blows up rust-analyzer-scale maps). Live-only source (blocks auditing existing `%TEMP%` scale maps). |

*Why this over “do what the branch did”:* under temporary-review use the owner
edits the analyser and re-runs constantly; **detecting drift is more valuable**,
not less. The branch’s embed strategy achieved “show what was analysed” by
snapshotting text. With disk slices, a **hash gate** is the honest equivalent:
either show bytes that still match the analyse-time file, or refuse and push
re-analyse. Recorded as a recommendation until the owner confirms; see Open
questions.

### Rough edges from the old viewer

| Rough edge | Disposition |
|---|---|
| Jump-to-id opens every file/crate/folder | **Fix** in an early UI phase — use `idIndex`’s known file |
| Parse errors leave stale summary/toolbar | **Fix** — clear chrome on error |
| Singular “conflict” pill label | **Accept** (cosmetic) |
| `DocComment.kind` ignored | **Accept** for review UI |
| `dependencies` / `roots` ignored | **Accept** |
| Filter chip OR easy to misread as AND | **Accept**; document in UI copy if cheap |
| No virtualization | **Defer** unless reviewing RA/tokio-scale maps in-browser |
| Weak a11y | **Defer** |
| String-built HTML | Prefer escaping as today; typed widgets not required for a maybe-temporary tool |
| CallSite `byte_*` unused in old viewer | Still unused for display chrome; used server-side for optional call-range highlight later if cheap |

### Repository-wide diagnostics view (recommendation)

**Previously:** deferred (with reverse lookup and editor deep-links).

**Recommendation under review framing:** **pull into scope** as a dedicated
phase after source + live analyse work. A flat list of every `conflict` and
every `unresolved` across the repository (call path, reason, enclosing
`FunctionId`, file, line, `from_macro`, jump into tree + source) is exactly the
workflow behind [`unresolved-analysis.md`](../unresolved-analysis.md), which
today required out-of-repo helpers and manual source checks on 1,533 RA sites.

*Why valuable:* classifications there (correct / miscategorised / real gap)
were done by grouping unresolved reasons and reading source. The UI should make
that pass interactive. *Not approved yet* — flagged under Open questions.
Suggested minimum columns: target kind, `call_path`, reason, `from_macro`,
function id, file path, line; click → expand that function with source.

### Decisions made under the production framing

These still stand unless the owner revisits them; listed so the tension is
explicit:

| Decision | Production rationale | Under temporary-review framing |
|---|---|---|
| First-class versioned in-repo UI | Ship with the product | Still fine as the review harness; do not invest in packaging/release polish |
| Five-crate layout + `horizon-server` | Clean product boundaries | Already landing; keep — splitting cost is sunk |
| Faithful visual port of Desktop viewer | Brand/consistency | Keep as cheapest path to a usable tree; skip pixel-perfect chase |
| Production-quality bar | End users | **Demote** — correctness of displayed analysis evidence matters; chrome polish does not |
| Virtualization / a11y as future needs | Large users, a11y | Defer; virtualization only if owner reviews RA/tokio **in this UI** |
| Optional `live-analyse` feature in workspace analysis | Compile isolation | **Overridden:** live is required; engine dep is always on |

## Open questions

1. **Staleness policy for saved maps** — recommendation above (per-file
   `content_hash` + refuse stale slices). Owner has not closed this.
2. **Repository-wide diagnostics view** — recommended in-scope; needs explicit
   yes/no before that phase starts.
3. **Per-file `content_hash` field** — needed if staleness recommendation is
   accepted; exact algorithm (sha256 of bytes) and serde name.
4. **Function range extent** — full `ast::Fn` syntax node (recommended) vs
   body-only vs `fn` keyword through body. Affects what the audit panel shows
   for attributes / where-clauses.
5. **Syntax highlighting in the source panel** — plain `<pre>` is enough for
   v1; branch had a tiny custom highlighter. Defer unless reading plain text
   slows review.
6. **Whether to keep the UI after the audit pass** — product vs delete; do not
   let that uncertainty block the review phases.

## Deferred scope

| Item | Notes |
|---|---|
| Reverse caller lookup | Still deferred |
| Click-to-open-in-editor | Still deferred |
| Virtualization | Needed **if** the owner reviews rust-analyzer / tokio maps in-browser and the tree becomes unusable; not needed for Horizon-self and fixture maps |
| Accessibility beyond button/focus-visible | Solo local tool |
| Dependency graph / `Crate.roots` display | Not needed for call-site audit |
| Light theme | No |
| Embedding highlighted source in JSON | Rejected for this architecture (see Decisions) |
| Configurable bind address | Blocked on path lockdown |

---

## Vertical build plan

Build **vertically**: each phase is a thin end-to-end slice that leaves the tree
compiling and something checkable in a browser. Groundwork (workspace + server
skeleton) is done.

Phases are ordered by **review value**, not by “finish the faithful port
before any new feature.”

### Parallelism map

| Can overlap | Collision risk |
|---|---|
| **A** (static viewer + JSON load) ‖ **B** (`Function` byte ranges in map + extract) | **Low** — A touches `horizon-server/web/*` + server static routes; B touches `horizon-map` + `horizon-engine/extract` + json-output docs/tests. Do not both edit `horizon-server` `Cargo.toml` carelessly. |
| **C** (source panel) needs **A** + **B** | Sequential after both |
| **D** (live analyse) can start after **A** (needs routes); independent of **B**/**C** at first, but the useful review loop is A+B+C+D together | May edit same `main.rs` / router as C — **serialize C and D** or split modules first (`routes.rs` vs `static.rs`) |
| **E** (staleness / hash) needs **B** + **C** | After C |
| **F** (diagnostics view) needs **A**; much better after **C** | After C; approval gate |
| **G** (jump + error-chrome fixes) can ride with **A** or a tiny follow-up | Same JS files as A — fold into A if cheap, else immediately after A |

### Phase A — Map on screen from saved JSON

**Goal:** Serve the adapted Desktop viewer and load a real `Repository` JSON so
conflicts, unresolved reasons, `from_macro`, and all five summary counters are
visible in a browser.

**Touches:** `crates/horizon-server/web/*` (new),
[`crates/horizon-server/src/main.rs`](../../crates/horizon-server/src/main.rs)
(+ static file routing), optionally `POST /api/map` for server-side validate.

**Demo:** `cargo run -p horizon-server` → browser opens → Open JSON… on
`tests/fixtures` output or a CLI-produced map → expand a conflict site (e.g.
glob-ambiguity) and an unresolved site; confirm purple macro badges and dropped
counters in the summary bar.

**Verify:** manual browser check; fixture map from
`cargo run -p horizon -- <fixture>` written to a temp JSON; optional
`/api/health` smoke test.

**Depends on:** groundwork only. **∥ Phase B.**

**Rough edges folded in:** clear summary/toolbar on JSON parse failure; prefer
fixing jump-to-id brute force here while `viewer.js` is being adapted (**G**
can be absorbed).

### Phase B — `Function` byte ranges in the contract

**Goal:** Every `Function` in emitted JSON carries `byte_start` / `byte_end`
populated by extract.

**Touches:** [`map.rs`](../../crates/horizon-map/src/map.rs) `Function`,
[`json.rs`](../../crates/horizon-map/src/json.rs) sample/tests,
[`extract.rs`](../../crates/horizon-engine/src/extract.rs),
[`json-output.md`](../json-output.md), any oracle/fixtures that round-trip full
function objects.

**Demo:** CLI map of a fixture; jq/assert a known function’s range matches a
manual byte count on that file.

**Verify:** unit test in extract or map round-trip; `cargo test -p horizon-map
-p horizon-engine`; spot-check fixture `glob_ambiguity` or `doc_comments`.

**Depends on:** groundwork. **∥ Phase A.**

### Phase C — Source panel (core review loop)

**Goal:** Expanding a function shows its real source beside its call sites.

**Touches:** `GET /api/source` in `horizon-server`; `viewer.js` / CSS for a
source block under the function node; path sandbox against `map.root`.

**Demo:** load Horizon’s own map (saved or from A); expand
`extract_facts` (or similar); confirm the displayed text is the function body
from disk and that listed `call_sites` appear in that text.

**Verify:** server unit/integration test with a tiny temp file + known range;
browser check on a fixture; intentional out-of-range / missing-file returns
structured errors.

**Depends on:** A + B.

### Phase D — Live in-process analyse

**Goal:** Submit a repo path; server runs `build_function_map` and the UI loads
the result without a pre-saved file.

**Touches:** `POST /api/analyse` in `horizon-server`; small header/toolbar
control for path + Analyse; engine already a dependency.

**Demo:** point at `tests/fixtures/glob-ambiguity` (or repo root); map appears;
re-run after a no-op to confirm latency acceptable for fixtures.

**Verify:** request against fixture path returns JSON with `crates.len() >= 1`;
browser round-trip; ensure no subprocess to CLI (code review / grep).

**Depends on:** A. Serialize with C if both edit the same router file — split
route modules first to allow parallel completion.

### Phase E — Staleness gate for saved-map source

**Goal:** Source panel refuses silently-wrong slices when the file changed since
the map was built.

**Touches:** `File.content_hash` (or chosen field) in `horizon-map` + extract;
`/api/source` hash check; UI banner for `stale` / `missing`.

**Demo:** build map of a fixture; edit the `.rs` file without re-analysing; open
source → banner, no misleading body; re-analyse → source works again.

**Verify:** automated test with temp dir; hash mismatch → 409/400 + error kind.

**Depends on:** B + C. **Blocked on Open question 1** — if owner picks
live-only source, shrink this phase to “source disabled for saved JSON” UI copy
instead.

### Phase F — Repository-wide diagnostics (recommended)

**Goal:** One list of every conflict and unresolved site in the loaded map, with
reasons, for systematic audit (pairs with
[`unresolved-analysis.md`](../unresolved-analysis.md)).

**Touches:** mostly `viewer.js` / CSS (client-side walk of the in-memory map);
optional trivial server noop.

**Demo:** load `%TEMP%\horizon-scale-results\ripgrep-after.json` (or RA map);
open Diagnostics; filter unresolved; click a row → function opens with source
(if C done) and the site highlighted by line.

**Verify:** count of listed unresolved equals `summary.unresolved`; same for
conflicts; spot-check a known miscategorised example from the unresolved-analysis
doc.

**Depends on:** A; **should** follow C. **Blocked on Open question 2 (owner
yes/no).**

### Phase G — Jump + error chrome (if not absorbed in A)

**Goal:** Jump uses `idIndex`’s file; JSON errors reset summary/toolbar.

**Touches:** `viewer.js` only.

**Depends on:** A.

### Explicitly not phased for v1

Virtualization, a11y pass, dependency graph, editor deep links, reverse
callers, syntax-highlight token port from `my-local-name`, configurable bind.

---

## Phase order (compact)

```
A  Saved JSON viewer on screen          ┐
B  Function byte_start/byte_end         ┘  parallel
C  Source panel                         ← after A+B   ★ core review
D  Live analyse                         ← after A; serialize w/ C on server router
E  Staleness / content_hash             ← after C (+ owner OK on policy)
F  Repo-wide diagnostics (recommended)  ← after C (+ owner yes)
G  Jump/error fixes                     ← fold into A if possible
```
