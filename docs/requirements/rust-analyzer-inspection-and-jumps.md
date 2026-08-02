# rust-analyzer in the inspection tab, and completing Map navigation

## Summary

Two related gaps in the Horizon IDE. **rust-analyzer features do not work in
Horizon** — no hover information, no go-to-definition, and no semantic
highlighting — and they are wanted in the **inspection tab** (the editor Horizon
opens on a real `.rs` file). Separately, navigation out of the Map is
incomplete: call-site rows in the function view give nothing on hover, and call
targets that are not present in the map are inert text.

The hover source-peek shipped earlier answered a different question ("show me
this function's text"). It stays, but it is not what "hover to get information
about the code" meant.

## Goals

1. rust-analyzer **hover** works in the inspection tab — hovering any symbol
   returns what it would return in a normal editor tab.
2. rust-analyzer **go-to-definition** works in the inspection tab.
3. rust-analyzer **semantic highlighting** applies in the inspection tab, so code
   is coloured by resolved meaning rather than plain TextMate rules.
4. Hovering a **call-site row** shows the same source peek a Functions-list row
   shows.
5. Clicking a call target **not present in the map** jumps to its definition
   line, the way rust-analyzer would.

> **Correction (post-discussion).** "Inspection canvas" means the **source
> panes inside the Horizon Map** (the Inspector rail's source view and the
> hover peek), not the read-only editor tab. rust-analyzer already works in
> editor tabs; the gap is that Horizon's own source panes have no rust-analyzer
> hover and no rust-analyzer styling. The webview cannot reach the language
> server directly, so the host bridges: token position → hover / semantic
> tokens providers → rendered result back into the webview.

## Non-goals
- **Changing file-card behaviour.** Clicking a file card (canvas `.card-frame`
  or a LAYERS row) selects the file and fills the Inspector; it opens no editor.
  That is current behaviour and stays.
- **Changing in-map function jumps.** Call targets that exist in the map are
  already click-to-jump and must not be touched.
- **Removing the existing hover source peek** on the Functions list.

## Behaviour

| Surface | Hover | Click |
|---|---|---|
| Functions list (file view) | source peek — *exists* | jump within Horizon — *exists* |
| Call-site row (function view) | source peek — **new** | target in map: jump within Horizon — *exists*<br>target not in map: go to definition — **new** |
| File card / LAYERS row | — | selects only, opens nothing — *unchanged* |
| Inspection tab | rust-analyzer hover — **broken** | rust-analyzer go-to-definition — **broken** |
| Inspection tab rendering | rust-analyzer semantic highlighting — **broken** | |

## Affected code

- [horizonInspection.ts](../../ide/contrib/horizon/browser/horizonInspection.ts) —
  owns the inspection tab. `openInspection` opens the real file URI, then calls
  `setReadonly(uri)`, `lockModel(uri)` and `attachModelGuard(uri)`. This is the
  prime suspect for all three broken rust-analyzer features (see Open questions).
- [viewer.js `renderTargetEl`](../../ide/contrib/horizon/browser/media/viewer.js#L2134) —
  decides what a call target renders as. Today:
  - `resolved` **and** `fnIndex.has(id)` → `jumpLink(id)` → `selectFunction(...)`
  - `resolved` **not** in map → inert `.raw-id` span, title "No matching function in this map"
  - `conflict` candidates → same split per candidate
  - `unresolved` → text plus reason, nothing clickable
  - external / constructor / associated → raw JSON dump, inert
- [viewer.js `renderCallSites`](../../ide/contrib/horizon/browser/media/viewer.js#L2205) —
  builds the rows; each carries `site.line`, `site.call_path`, `site.byte_start`.
- [viewer.js `attachFnPreview`](../../ide/contrib/horizon/browser/media/viewer.js) —
  the existing hover peek, currently attached only to `.fn-list-item`.
- [bridge.js](../../ide/contrib/horizon/browser/media/bridge.js) and
  [horizonEditorPane.ts](../../ide/contrib/horizon/browser/horizonEditorPane.ts) —
  the webview↔host message channel any new jump would travel over.

## Decisions

- **rust-analyzer stays in the inspection tab only.** The Map is a webview with
  no language-server access; the inspection tab is a real editor on the real file
  URI, so the features come for free once the model is wired correctly.
- **Jump by position, not by name.** For a target outside the map, ask
  rust-analyzer for the definition **at the call site's own position**
  (`site.line` / `site.byte_start` in the enclosing file) rather than resolving
  the callee's name ourselves. This works for external crates, methods,
  constructors and associated functions — exactly the categories the map drops —
  without Horizon reimplementing name resolution.
- **Call-site hover reuses the Functions-list peek.** Call-site rows reference
  Horizon functions, so the same source peek applies; no second hover concept.
- **Keep the existing source peek.** It answers "what does this function
  contain" without leaving the list, which clicking cannot do (clicking replaces
  the Functions list with the function view).

## Open questions

1. **Why rust-analyzer is dead in Horizon.** Two candidates, with different fixes:
   - *Not installed.* A fresh Horizon IDE ships no extensions; the product
     overlay points the gallery at Open VSX. `rust-lang.rust-analyzer` was only
     installed into a throwaway test profile during earlier testing.
   - *Suppressed by our model handling.* `setReadonly` / `lockModel` /
     `attachModelGuard` may detach or synthesise the model so rust-analyzer never
     associates it with the workspace file — which would kill hover,
     definitions and semantic tokens together, matching the reported symptom.

   Settle empirically first: open a `.rs` file from the Explorer in a normal tab
   and hover a symbol, then do the same in the inspection tab. Works in the
   normal tab only ⇒ our bug. Fails in both ⇒ rust-analyzer is absent, and
   shipping/requiring it is a prerequisite for everything in this document.
2. **Does the inspection tab have to stay read-only?** If the read-only lock is
   what breaks the providers, the requirement to keep it needs revisiting.

## Out of scope / deferred

- rust-analyzer hover inside the Map's Inspector rail or the hover peek popover.
- Any change to file-card or in-map function-jump behaviour.
- Emitting per-token positions from `/api/source`
  ([source.rs](../../crates/horizon-server/src/source.rs#L92) returns bare
  `[text, class]` pairs). Only needed if hover ever has to map a token inside
  rendered source back to a file position — not required by the decisions above,
  since jumps use the call site's own recorded position.
