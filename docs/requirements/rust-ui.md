# Horizon Function-Map Review UI

> **Supersession (30 July 2026):** the tree-viewer IA and Phases A–G build plan
> below are superseded by
> [`desktop-ui-rebuild.md`](desktop-ui-rebuild.md) (Desktop spatial Map +
> Inspector + bottom DAG). Server/security, `/api/source`, map field names,
> non-guessing rule, diagnostics *requirement*, and `POST /api/analyse` still
> stand; only the front-end shape and phase sequence moved.

**Status:** draft requirements + vertical build plan (technical decisions
closed 30 July 2026; one lifespan question remains)  
**Date:** 30 July 2026  
**Branch:** `feat/ast-system`  
**Companion specs:** [`rust-function-map.md`](rust-function-map.md),
[`json-output.md`](../json-output.md),
[`old-viewer-spec.md`](../ui/old-viewer-spec.md),
[`branch-ui-survey.md`](../ui/branch-ui-survey.md),
[`workspace-split-analysis.md`](../workspace-split-analysis.md),
[`unresolved-analysis.md`](../unresolved-analysis.md),
[`desktop-ui-rebuild.md`](desktop-ui-rebuild.md)

Filename and section shape follow [`rust-function-map.md`](rust-function-map.md)
(Summary → Goals → Non-goals → Users → Behaviour → Decisions → Open → Deferred,
then a phase plan).

## Summary

Horizon already emits a JSON function map. This document specifies a **local
web UI, living in this repository**, that loads that map (live or from a saved
file) and lets the owner **audit whether the analyser got the calls, conflicts,
unresolved sites, and drop counters right** — with the function’s real source
code beside the extracted call sites, and a repository-wide worklist of every
incomplete edge.

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
- Provide a **repository-wide diagnostics worklist**: every `Conflict` and
  every `Unresolved` site, with reasons (and conflict candidates), linking into
  the tree and the source panel.
- Support two load paths: **live in-process analysis** of a repository path,
  and **open a previously saved map JSON**.
- Show **server-highlighted source** of a free function next to that function’s
  call sites (core review loop — not a polish extra).
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
| Inline mark-up of call-site spans inside the source panel | Deferred (see Deferred) |
| Light theme / theme toggle | Out (match old viewer: dark only) |
| Accessibility hardening (`aria-*`, tree roles, live regions) | Deferred — not needed for solo review |
| Virtualization of the tree | Deferred **unless** reviewing maps the size of rust-analyzer / tokio (see Deferred) |
| Faithful port of the abandoned Design-Component visualizer on `my-local-name` | Non-goal — wrong JSON contract ([`branch-ui-survey.md`](../ui/branch-ui-survey.md)) |
| Incremental analysis | Non-goal now — but `File.content_hash` is shaped so a future mode can reuse it |

## Users and use cases

| User | Job |
|---|---|
| **Primary: the owner** | After changing extract/resolve, re-analyse Horizon itself (or a fixture / scale corpus) and check whether call sites, conflicts, unresolved reasons, macro recovery, and drop counters match the source. |
| Secondary (hypothetical) | Anyone with a saved `.json` map who wants a local tree view. Not a design driver. |

Use cases, in priority order:

1. Load a map → open the **diagnostics worklist** → walk every conflict and
   unresolved site with its reason (and candidates); jump into the enclosing
   function and read source.
2. Expand a function in the tree → read its **highlighted source** and the
   ordered `call_sites` Horizon attached; judge extraction/resolution by eye.
3. Filter the tree to functions with conflicts or unresolved sites; jump
   candidates; read reasons.
4. Read `MapSummary` (including `external_dropped`, `constructor_dropped`,
   `associated_dropped`) and compare against expectations from
   [`unresolved-analysis.md`](../unresolved-analysis.md).
5. Spot `from_macro: true` sites (purple badge) and treat them as
   lower-certainty recovery.
6. Re-run **live** analysis after editing the analyser or the target repo,
   without a separate CLI → `data.js` dance.

## Behaviour

### Inputs

| Input | How |
|---|---|
| Saved map JSON | Browser file picker (client-side parse), **or** server endpoint that reads a path / uploaded bytes and returns `Repository` JSON |
| Live repository path | Browser submits an absolute path; server calls [`horizon_engine::build_function_map`](../../crates/horizon-engine/src/lib.rs) in-process and returns the map |
| Source file bytes | Server reads `File.path` from disk, verifies `File.content_hash`, slices `[Function.byte_start, Function.byte_end)`, lexes the slice with `ra_ap_syntax`, returns tokens |

### Outputs (what the UI shows)

Match the old viewer unless noted. Layout remains a single-column dark page:
sticky header → summary bar → toolbar → tree (see
[`old-viewer-spec.md`](../ui/old-viewer-spec.md) §1–5), plus a diagnostics
surface (tab, toggle, or sibling list — implementation choice) and a source
panel under an expanded function.

| Surface | Required for review |
|---|---|
| Tree: crate → folder → file → function | yes |
| **Diagnostics worklist** (every conflict + unresolved) | yes — primary audit list |
| Call-site cards: `resolved` / `conflict` / `unresolved` | yes |
| Conflict candidates + reason; unresolved reason | yes |
| `from_macro` → purple `macro` badge | yes (already in old viewer) |
| Summary pills: conflicts, unresolved, **and** the three `*_dropped` counters | yes — old viewer already consumed all five `MapSummary` fields; keep them visible |
| Function **source panel** (server-highlighted tokens) | yes — core |
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

Source-token colours (CSS classes on spans inside the source `<pre>`; dark only):

| Class | Role | Suggested colour |
|---|---|---|
| `.tok-kw` | keywords (`fn`, `let`, `use`, …) | `#c3a6ff` (near `--macro`) |
| `.tok-fn` | function / call names | `#7eb8ff` (`--accent`) |
| `.tok-ty` | type names | `#5dce8a` (`--resolved`) |
| `.tok-c` | comments | `#6b7385` (`--text-dim`) |
| `.tok-str` | string literals | `#e6c48a` |
| `.tok-num` | numeric literals | `#e6e9ef` |
| *(no class / `.tok`)* | punctuation, whitespace, other | `#e6e9ef` (`--text`) |

### Endpoints (`horizon-server`)

Bind: `127.0.0.1:0` (ephemeral). Print `http://{addr}` and open the default
browser (skeleton today only prints the URL — browser open is still to land).
Do **not** expose a bind-address flag in v1.

| Method | Path | Body / query | Response |
|---|---|---|---|
| `GET` | `/` | — | `index.html` |
| `GET` | `/static/*` | — | `viewer.css`, `viewer.js`, … |
| `GET` | `/api/health` | — | `{"ok":true}` |
| `POST` | `/api/analyse` | JSON `{"path":"<abs repo>"}` | `Repository` JSON, or `4xx` `{error}` |
| `POST` | `/api/map` | JSON map body, **or** multipart / path load — pick one in implementation; deserialize via [`map_from_slice`](../../crates/horizon-map/src/json.rs) | Validated `Repository` or `4xx` |
| `GET` | `/api/source` | `path`, `byte_start`, `byte_end`, `expected_hash` | highlighted tokens (below), or structured error (`missing` / `stale` / `range`) |

**`/api/source` success body** (token wire format, same idea as
`my-local-name`’s `[[text, class], …]` but with the class vocabulary above):

```json
{
  "tokens": [
    ["fn", "kw"],
    [" ", ""],
    ["extract_facts", "fn"],
    ["(", ""],
    ["tree", ""],
    [": ", ""],
    ["&", ""],
    ["SourceFile", "ty"],
    [") ", ""],
    ["{", ""],
    ["\n    ", ""],
    ["// …", "c"]
  ]
}
```

Each element is `[text, class]` where `class` is one of
`kw` | `fn` | `ty` | `c` | `str` | `num` | `""`. The UI maps `class` →
`.tok-{class}` (empty class → unstyled / `.tok`). Lex and classify on the
server with `ra_ap_syntax` over the sliced snippet — no extra highlighting
crate. `horizon-server` already depends on `horizon-engine`, which pulls in
`ra_ap_syntax`.

Live analysis must call the engine **in-process** (`horizon-server` depends on
`horizon-engine` unconditionally — see
[`Cargo.toml`](../../crates/horizon-server/Cargo.toml)). No `std::process` to
the CLI.

**Security constraint:** bind address must not become configurable without first
locking down the repo-path / file-path parameters. A browser-chosen path against
a non-loopback bind would let a remote caller read arbitrary directories. Keep
loopback-only until that lock-down exists. The abandoned branch allowed
`HOST`/`PORT` env overrides ([`branch-ui-survey.md`](../ui/branch-ui-survey.md));
that must **not** be copied.

### Interactions

Port from [`old-viewer-spec.md`](../ui/old-viewer-spec.md) §6, plus:

| Trigger | Effect |
|---|---|
| Expand tree node | Lazy-build children; twisty ▶/▼ |
| Click resolved / candidate id | Jump to function (see rough edges — fix brute-force) |
| Search + “Has conflicts” / “Has unresolved” chips | Filter (chip OR semantics retained) |
| Open JSON… | Load saved map |
| Analyse path (new chrome) | `POST /api/analyse` → `loadMap` |
| Open diagnostics worklist | Flat list of every conflict + unresolved in the loaded map |
| Click a diagnostics row | Jump to enclosing function in the tree; fetch `/api/source` and show the source panel |
| Expand function | Fetch `/api/source` and show highlighted source above or beside call sites |

### JSON fields the UI must honour

Field names from [`map.rs`](../../crates/horizon-map/src/map.rs) /
[`json-output.md`](../json-output.md):

| Path | Fields |
|---|---|
| `Repository` | `root`, `crates`, `summary` |
| `MapSummary` | `conflicts`, `unresolved`, `external_dropped`, `constructor_dropped`, `associated_dropped` |
| `Crate` | `name`, `rustc_name`, `is_library`, `edition`, `folders`, `files` (`roots`, `dependencies` ignored) |
| `Folder` | `path`, `folders`, `files` |
| `File` | `path`, `module_path`, `functions`, `call_sites`, `doc_comments`, **`content_hash` (to add)** |
| `Function` | `id`, `name`, `module_path`, `line`, `call_sites`, `doc_comments`, **`byte_start`, `byte_end` (to add; full `ast::Fn` `syntax().text_range()`)** |
| `DocComment` | `text` (`kind` ignored) |
| `CallSite` | `call_path`, `line`, `byte_start`, `byte_end`, `target`, `from_macro` |
| `CallTarget` | adjacent tag `kind` + `data` (`resolved` \| `conflict` \| `unresolved`) |

`CallTarget` wire shapes (unchanged):

```json
{ "kind": "resolved", "data": "crate_key::path" }
{ "kind": "conflict", "data": { "candidates": ["…"], "reason": "…" } }
{ "kind": "unresolved", "data": { "reason": "…" } }
```

`File.content_hash`: hex-encoded SHA-256 of the raw file bytes at extract time.
Serde name `content_hash`. Omitted from UI chrome; consumed by `/api/source`.

## Affected code

| Area | Paths |
|---|---|
| Contract | [`crates/horizon-map/src/map.rs`](../../crates/horizon-map/src/map.rs), [`crates/horizon-map/src/json.rs`](../../crates/horizon-map/src/json.rs), [`docs/json-output.md`](../json-output.md) |
| Extractor (function spans + file hash) | [`crates/horizon-engine/src/extract.rs`](../../crates/horizon-engine/src/extract.rs) — already has `func.syntax().text_range()` at extract time ([L261–L274](../../crates/horizon-engine/src/extract.rs)) |
| Engine entry | [`build_function_map`](../../crates/horizon-engine/src/lib.rs) |
| Pipeline (shared with correctness) | [`pipeline`](../../crates/horizon-engine/src/pipeline.rs) — `ExtractedCrate`, `extract_repository`, `extract_crate`, `resolve_index_for` are `pub` |
| Server | [`crates/horizon-server/src/main.rs`](../../crates/horizon-server/src/main.rs) (skeleton today), plus new modules for routes / static / source highlight |
| Static UI | new under `crates/horizon-server/web/` (or `static/`) — adapted from Desktop viewer |
| Docs | this file; later a one-line pointer from README |

### Groundwork landed (not a phase)

Committed as `ec73c02` (this requirements doc as `6462c46`). Verified:

| Fact | Detail |
|---|---|
| Layout | `crates/horizon-map`, `crates/horizon-engine`, `crates/horizon` (CLI), `crates/horizon-server`, `crates/horizon-correctness`; [`tests/fixtures/`](../../tests/fixtures/) stays at repo root |
| Workspace | [`Cargo.toml`](../../Cargo.toml) virtual manifest; `cargo build --workspace` / `cargo test --workspace` green |
| Tests | 62 total: 29 engine unit, 18 engine integration, 12 correctness, 3 map |
| `horizon-map` leaf | depends only on `serde`, `serde_json`, `anyhow` — no `ra_ap_syntax` |
| Integration tests | live inside the crates they exercise (virtual root has no package for root tests); fixture paths walk up to the workspace root |
| `horizon-server` | compiling `axum` skeleton: binds `127.0.0.1:0`, prints URL, placeholder `/` — **does not yet** auto-open a browser |
| Correctness API | `pipeline` module, `ExtractedCrate`, `extract_repository`, `extract_crate`, `resolve_index_for` widened to `pub` |
| Engine dep | `horizon-server` → `horizon-engine` unconditional (no feature flag) |

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
  tree; see Deferred. The diagnostics list will be long on those maps — that is
  the point of the worklist; virtualization of the *tree* is a separate concern.
- Never guess a call target in the UI; display every candidate.

## Decisions, with reasoning

### Purpose: audit the analyser (reframed)

**Decision:** primary job is reviewing Horizon’s own function-map output for
correctness, not shipping a polished end-user product. Lifespan is uncertain
(“might be temporary”).

*Why:* owner clarification. Source-beside-call-sites, the diagnostics worklist,
and incomplete-edge surfaces beat polish. Production stack choices already made
are kept where they still serve review (in-repo Rust server, loopback, live
analyse) but phase order and deferred scope follow review value.

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
feature). Landed: unconditional dep in
[`crates/horizon-server/Cargo.toml`](../../crates/horizon-server/Cargo.toml).

*Why:* settled owner decision. Matches abandoned branch’s in-process scan
pattern, with `axum` instead of `tiny_http`. Avoids CLI subprocess and keeps one
analysis code path with the CLI.

### Loopback + ephemeral port + open browser

**Decision:** bind `127.0.0.1:0` only; print URL; open browser; no auth.
Skeleton already binds and prints; browser open remains to implement.

*Why:* local tool. Security: configurable bind without locking repo-path would
be directory disclosure. Explicitly **do not** copy the old `HOST`/`PORT` env
escape hatch until path inputs are locked down.

### Both live analyse and saved JSON

**Decision:** both required.

*Why:* live for the edit→re-analyse→inspect loop; saved JSON for maps already
produced (scale results under `%TEMP%\horizon-scale-results\`, fixture sidecars,
CLI output).

### `Function.byte_start` / `Function.byte_end` — full `ast::Fn` node

**Decision:** add two `u32` fields to [`Function`](../../crates/horizon-map/src/map.rs)
(same meaning as on [`CallSite`](../../crates/horizon-map/src/map.rs)
[L410–L413](../../crates/horizon-map/src/map.rs): UTF-8 start and one-past-end).
The extent is the **full `ast::Fn` syntax-node range**, including outer
attributes (`#[cfg]`, `#[inline]`, …), outer doc comments (`///` / `/**`),
signature, and body.

**How to obtain the range in extract:** for each free `ast::Fn` already visited
in [`extract_facts`](../../crates/horizon-engine/src/extract.rs)
([L249–L274](../../crates/horizon-engine/src/extract.rs)), record:

```rust
let range = func.syntax().text_range(); // SyntaxNode of the Fn item
let byte_start = u32::from(range.start());
let byte_end   = u32::from(range.end());   // one past the end
```

Do **not** start at `fn_token()` (that drops leading attributes) and do **not**
use only the body block’s range. `func.syntax().text_range()` is the single
source of truth.

*Why this extent:* an auditor judging whether calls were attributed to the
right one of two `#[cfg]`-duplicated definitions needs to see the attributes
sitting above each signature. Body-only and `fn`-keyword-through-end slices
hide those attributes and make that judgment harder.

*Accepted consequence:* doc comment text appears in the source panel **and** is
still carried separately in `Function.doc_comments`, so the same text shows up
twice (docs block + top of the highlighted slice). That duplication is
**accepted** rather than dropping attributes from the range. Trimming leading
outer docs from the *displayed* slice (while keeping attrs) is optional polish,
not a requirement — see Deferred.

*Why byte fields at all:* `CallSite` already carries ranges for positioning;
`Function` today has only 1-based `line` of the `fn` keyword
([L435–L436](../../crates/horizon-map/src/map.rs)). Rejected alternatives:
re-parse on the server solely to rediscover bounds; slice from this function’s
line to the next function’s line (wrong when non-`fn` items sit between).

**Contradiction with today’s code:** these fields are **not present yet**.
Until Phase B lands, source display cannot use the contract.

### Source display: disk slice + server-side highlight

**Decision:** server reads the file on demand, slices by `Function.byte_*`,
lexes the snippet with `ra_ap_syntax`, and returns `[text, class]` tokens.
Maps stay small; the UI renders spans with the `.tok-*` classes above. No
extra highlighting dependency — the branch UI’s hand-rolled highlighter was
approximating what the real lexer gives for free.

*Why:* owner decision. Deliberately diverges from `my-local-name`, which
**embedded** highlighted token arrays in the scan JSON at analyse time and never
re-read disk ([`branch-ui-survey.md`](../ui/branch-ui-survey.md) §Lead answers).
Disk + hash keeps saved maps lean and matches the ordinary analyse-then-review
flow where the file on disk is the file that was analysed.

### Staleness when the file changed or disappeared

**Settled** (with `content_hash` on `File`).

**Branch precedent (honest):** `my-local-name` embedded highlighted source at
scan time and had **no** drift detection. After disk changed or a file was
deleted, the UI still showed the snapshot. We deliberately diverge: the owner
will be editing the analyser and re-running constantly, so drift is the normal
case, not an edge case.

| Mode | Behaviour |
|---|---|
| **Live `POST /api/analyse`** | Map and source are co-produced from the same disk state. Slice with `Function.byte_*`, highlight, return tokens. |
| **Saved JSON** | Extract stores `File.content_hash` (SHA-256 hex of file bytes) beside `path` / `module_path`. `GET /api/source` recomputes the hash; on mismatch return `stale` and show a banner (“source changed since map was built — re-analyse”) **without** serving a body; on missing file return `missing`; on range out of bounds return `range`. |
| **Rejected** | Silent best-effort slice of a drifted file. Embed function bodies in JSON. Live-only source. |

**Second payoff (out of scope now, shape the field for it):** `content_hash` is
exactly what a future incremental-analysis mode needs to know which files
changed. Not building incremental now; do not invent other hash semantics that
would block that.

### Repository-wide diagnostics view

**Settled — in scope.** Flat, working-through-able list of every `Conflict` and
every `Unresolved` call site in the loaded map. Each row carries at least:
target kind, `call_path`, reason string, enclosing `FunctionId`, file path,
line, `from_macro`; conflict rows also list every candidate `FunctionId`.
Click → jump to that function in the tree and open the source panel.

*Why:* the tool’s job is auditing the analyser; this list **is** the audit
worklist. It is the interactive form of the pass documented in
[`unresolved-analysis.md`](../unresolved-analysis.md) (correct / miscategorised /
real gap), which previously needed out-of-repo helpers over 1,533 RA sites.

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
| CallSite `byte_*` unused in old viewer | Still unused for inline source mark-up (deferred); available if that idea is revived |

### Decisions made under the production framing

These still stand unless the owner revisits them; listed so the tension is
explicit:

| Decision | Production rationale | Under temporary-review framing |
|---|---|---|
| First-class versioned in-repo UI | Ship with the product | Still fine as the review harness; do not invest in packaging/release polish |
| Five-crate layout + `horizon-server` | Clean product boundaries | **Landed** (`ec73c02`); keep |
| Faithful visual port of Desktop viewer | Brand/consistency | Keep as cheapest path to a usable tree; skip pixel-perfect chase |
| Production-quality bar | End users | **Demote** — correctness of displayed analysis evidence matters; chrome polish does not |
| Virtualization / a11y as future needs | Large users, a11y | Defer; virtualization only if owner reviews RA/tokio **in this UI** |
| Optional `live-analyse` feature (workspace analysis draft) | Compile isolation | **Resolved:** engine dep is always on; no feature flag |

## Open questions

1. **Whether to keep the UI after the audit pass** — keep as a standing local
   tool, or delete once the analyser review is done. Do not let that uncertainty
   block the review phases. This is the only remaining open question.

## Deferred scope

| Item | Notes |
|---|---|
| Reverse caller lookup | Still deferred |
| Click-to-open-in-editor | Still deferred |
| **Inline call-site spans in the source panel** | Not chosen for v1. Would mark each extracted `CallSite`’s `[byte_start, byte_end)` inside the highlighted source so the auditor sees *exactly which spans* Horizon treated as calls — high audit value, separate from “show the function text”. Revisit after the basic source panel works. |
| Trim leading outer docs from the displayed source slice | Optional polish only. The recorded `byte_*` range stays the full `ast::Fn` node; a display-time skip of outer doc attributes would remove the accepted docs duplication without hiding `#[cfg]`. Not required for v1. |
| Virtualization | Needed **if** the owner reviews rust-analyzer / tokio maps in-browser and the tree becomes unusable; not needed for Horizon-self and fixture maps |
| Accessibility beyond button/focus-visible | Solo local tool |
| Dependency graph / `Crate.roots` display | Not needed for call-site audit |
| Light theme | No |
| Embedding highlighted source in JSON | Rejected — disk slice + hash instead (see Decisions) |
| Configurable bind address | Blocked on path lockdown |
| Incremental analysis | Out of scope; `content_hash` shaped for it |

---

## Vertical build plan

Build **vertically**: each phase is a thin end-to-end slice that leaves the tree
compiling and something checkable in a browser. Groundwork (workspace + server
skeleton, `ec73c02`) is done.

Phases are ordered by **review value**, not by “finish the faithful port
before any new feature.” Diagnostics is a **primary deliverable**, so it is
scheduled as soon as a map is on screen — not after source and live analyse.

### Parallelism map

| Can overlap | Collision risk |
|---|---|
| **A** (static viewer + JSON load) ‖ **B** (`Function` byte ranges + `File.content_hash` in map + extract) | **Low** — A touches `horizon-server/web/*` + server static routes; B touches `horizon-map` + `horizon-engine/extract` + json-output docs/tests. |
| **F** (diagnostics list) after **A**; ‖ **B** | **Low** with B — F is almost entirely `viewer.js` / CSS. Same JS files as A: either fold F into the end of A or take A first then F immediately. |
| **C** (source panel) needs **A** + **B**; then wires F → source | After A+B; edits server router + JS — **serialize with D** or split modules (`routes.rs` / `static.rs` / `source.rs`) |
| **D** (live analyse) after **A** | Same router files as C — serialize or split modules first |
| **E** (staleness gate) needs **B** + **C** | After C; hash field itself is populated in B |
| **G** (jump + error-chrome) | Fold into A if cheap |

### Phase A — Map on screen from saved JSON

**Goal:** Serve the adapted Desktop viewer and load a real `Repository` JSON so
conflicts, unresolved reasons, `from_macro`, and all five summary counters are
visible in a browser.

**Touches:** `crates/horizon-server/web/*` (new),
[`crates/horizon-server/src/main.rs`](../../crates/horizon-server/src/main.rs)
(+ static file routing, browser open), optionally `POST /api/map` for
server-side validate.

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

### Phase B — `Function` byte ranges + `File.content_hash`

**Goal:** Every `Function` in emitted JSON carries `byte_start` / `byte_end`
spanning the **full `ast::Fn` syntax node**; every `File` carries
`content_hash` (SHA-256 hex of file bytes at extract time).

**Range to record:** in [`extract.rs`](../../crates/horizon-engine/src/extract.rs),
for each free-function `ast::Fn` (`func`), set
`byte_start = u32::from(func.syntax().text_range().start())` and
`byte_end = u32::from(func.syntax().text_range().end())`. That `SyntaxNode`
range includes outer attributes, outer doc comments, signature, and body — the
settled extent (see Decisions). Do not use `fn_token().text_range()` or the
body block alone.

**Touches:** [`map.rs`](../../crates/horizon-map/src/map.rs) `Function` + `File`,
[`json.rs`](../../crates/horizon-map/src/json.rs) sample/tests,
[`extract.rs`](../../crates/horizon-engine/src/extract.rs),
[`json-output.md`](../json-output.md), any oracle/fixtures that round-trip full
function/file objects.

**Demo:** CLI map of a fixture that has `#[cfg]` (or docs) above a free `fn`;
jq/assert the function’s `[byte_start, byte_end)` slice starts at the first
attribute or doc line (not at `fn`) and a known file’s hash matches SHA-256 of
its bytes.

**Verify:** unit test in extract or map round-trip; `cargo test -p horizon-map
-p horizon-engine`; spot-check fixture `glob_ambiguity` or `doc_comments`.

**Depends on:** groundwork. **∥ Phase A** (and **∥ Phase F** once A exists).

### Phase F — Repository-wide diagnostics (primary worklist)

**Goal:** Flat list of every conflict and unresolved site in the loaded map,
with reasons and (for conflicts) all candidate ids, so the owner can work the
audit systematically.

**Why this early:** the worklist needs only an in-memory `Repository` — not byte
ranges, not disk source. Moving it before the source panel gets the owner
triaging incomplete edges as soon as a map is loadable. Source click-through is
wired when Phase C lands (progressive enhancement: F ships with tree-jump
first).

**Touches:** mostly `viewer.js` / CSS (client-side walk of the loaded map).

**Demo:** load a conflict-bearing fixture or
`%TEMP%\horizon-scale-results\ripgrep-after.json`; open Diagnostics; confirm
row count equals `summary.conflicts + summary.unresolved`; click a row → tree
jumps to the enclosing function.

**Verify:** listed unresolved count == `summary.unresolved`; same for
conflicts; spot-check a known example from
[`unresolved-analysis.md`](../unresolved-analysis.md).

**Depends on:** A. **∥ B.** Source panel link depends on C (add in C or a
tiny C follow-up).

### Phase C — Highlighted source panel (core review loop)

**Goal:** Expanding a function (or clicking a diagnostics row) shows
server-highlighted source beside its call sites (full `ast::Fn` slice from
Phase B, including attributes and docs).

**Touches:** `GET /api/source` in `horizon-server` — read file, slice
`[byte_start, byte_end)`, lex with `ra_ap_syntax`, return `tokens`; path
sandbox against `map.root`; `viewer.js` / CSS for `.tok-*` and the source
block; wire diagnostics row → source. Hash verification against
`expected_hash` / `File.content_hash` may land here or in Phase E; if deferred
to E, C still returns tokens when the file and range are valid.

**Demo:** load Horizon’s own map; expand `extract_facts` (or similar); confirm
highlighted text matches the full item on disk (attrs/docs/signature/body) and
listed `call_sites` appear in that text; from Diagnostics, click an unresolved
row and land on the same panel.

**Verify:** server test with a tiny temp file + known range → expected token
classes; browser check on a fixture; missing-file / bad-range structured errors.

**Depends on:** A + B. Completes F’s source link.

### Phase D — Live in-process analyse

**Goal:** Submit a repo path; server runs `build_function_map` and the UI loads
the result without a pre-saved file.

**Touches:** `POST /api/analyse` in `horizon-server`; small header/toolbar
control for path + Analyse; engine already a dependency.

**Demo:** point at `tests/fixtures/glob-ambiguity` (or repo root); map appears;
diagnostics list refreshes; re-run after a no-op to confirm latency acceptable
for fixtures.

**Verify:** request against fixture path returns JSON with `crates.len() >= 1`;
browser round-trip; ensure no subprocess to CLI (code review / grep).

**Depends on:** A. Serialize with C if both edit the same router file — split
route modules first to allow parallel completion.

### Phase E — Staleness gate for saved-map source

**Goal:** Source panel refuses silently-wrong slices when the file changed since
the map was built (`content_hash` already emitted by B).

**Touches:** `/api/source` hash check (if not fully done in C); UI banner for
`stale` / `missing`.

**Demo:** build map of a fixture; edit the `.rs` file without re-analysing; open
source → banner, no misleading body; re-analyse → source works again.

**Verify:** automated test with temp dir; hash mismatch → 4xx + `stale`.

**Depends on:** B + C. Policy is settled — no approval gate.

### Phase G — Jump + error chrome (if not absorbed in A)

**Goal:** Jump uses `idIndex`’s file; JSON errors reset summary/toolbar.

**Touches:** `viewer.js` only.

**Depends on:** A.

### Explicitly not phased for v1

Virtualization, a11y pass, dependency graph, editor deep links, reverse
callers, inline call-site span mark-up in the source panel, embedding source in
JSON, configurable bind, incremental analysis.

---

## Phase order (compact)

```
A  Saved JSON viewer on screen (+ browser open, static, optional POST /api/map)
B  Function byte_* = full ast::Fn text_range(); File.content_hash   ∥ A
F  Repo-wide diagnostics (tree jump)       ← after A; ∥ B   ★ primary worklist
C  GET /api/source → highlighted tokens (+ F→source)   ← after A+B
D  POST /api/analyse (live)                ← after A; serialize w/ C on server router
E  Staleness gate on /api/source (hash)    ← after B+C
G  Jump/error fixes                        ← fold into A if possible
```
