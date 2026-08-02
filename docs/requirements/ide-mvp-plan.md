# Horizon IDE MVP Plan

## Decisions (locked)

| Topic | Decision |
|---|---|
| Host | Branded **Code-OSS fork** that builds/runs from this repo |
| **Not an extension** | Map is a **first-class workbench contribution** under `src/vs/workbench/contrib/horizon/` (built-in `EditorPane`), **not** a VS Code / Open VSX extension |
| Classic IDE | Keep full VS Code capabilities from Code-OSS (edit, terminal, SCM, search, rust-analyzer) |
| Map scope | **Rust free functions only** (methods/`impl` stay out of the map) |
| Map language | Rust only |
| View toggle | On-demand switch: **Map view** ↔ **Classic editor** |
| Inspection canvas | **Read-only** function source surface |
| Language intelligence | Full rust-analyzer on that surface: **hover docs, go-to-def, diagnostics, completions** (informational; canvas non-editable) |
| Analysis backend | Existing `horizon-engine` / `horizon-server` as local sidecar |
| Security review | Claude Opus 5 high after implementation |

## Correction note

An earlier iteration scaffolded `ide/extensions/horizon-map` as a development shortcut. That **violates** the product decision (“I don't want to build an extension”). The extension package is **not** the product surface. UI/media and sidecar lessons may be reused, but shipping path is **workbench `contrib/horizon`** inside the Code-OSS checkout.

## End state (MVP)

1. `./ide/scripts/bootstrap.sh` fetches Code-OSS and applies Horizon product + **contrib** overlays.
2. `./ide/scripts/build.sh` / `./ide/scripts/run.sh` produce and launch **Horizon IDE** (forked Code-OSS), not an Extension Development Host for a marketplace-style extension.
3. Opening a Rust workspace gives classic Code-OSS editing + rust-analyzer.
4. Map opens as a built-in **EditorPane** (workbench contrib); toggle returns to classic editors.
5. Selecting a free function opens **Inspection**: read-only editor on the real workspace file/range so rust-analyzer works.
6. Analyse uses the in-repo sidecar (`horizon-server` / engine), free-function contract unchanged.
7. **UI testing is done in the IDE**, not only in the standalone web viewer / preview server.

## Architecture

```text
Horizon IDE (Code-OSS fork)
├── Classic workbench                 ← stock Code-OSS
├── src/vs/workbench/contrib/horizon  ← Map EditorPane + toggle + inspection bridge
├── rust-analyzer                     ← normal language support in the product
└── Sidecar: horizon-server           ← analyse + map JSON + source slices
```

### Source of truth in this repo

| Path | Role |
|---|---|
| `ide/contrib/horizon/` | Horizon workbench contribution sources (synced into Code-OSS on bootstrap) |
| `ide/code-oss/` | Gitignored Code-OSS checkout (build tree) |
| `ide/product/product.json` | Product branding |
| `ide/scripts/*` | bootstrap / build / run |
| `ide/extensions/horizon-map/` | **Deprecated as product** — may hold reusable media temporarily until moved into contrib |

### Why inspection uses a real text editor

A webview alone cannot host rust-analyzer. Inspection is a **read-only workbench text editor** on the real `file:` URI/range. The Map EditorPane remains the visual surface (webview/canvas inside the pane).

## Workstreams

| ID | Focus | Deliverables |
|---|---|---|
| W1 | Product shell | bootstrap/build/run for forked Code-OSS (done baseline; retarget away from EDH-as-product) |
| W2′ | **Workbench contrib Map** | `ide/contrib/horizon` → EditorPane, register in workbench, port map UI |
| W3′ | Inspection + RA | Read-only reveal of function range inside the IDE product |
| W4 | Sidecar | Spawn/attach `horizon-server` from the workbench contrib |
| W5 | Integrate + **IDE UI tests** | Smoke in Horizon IDE window (not web preview alone) |
| W6 | Security (Opus 5) | Survey sidecar, webview messaging, path handling; fix findings |

## Non-goals for this MVP

- Shipping Horizon Map as a VS Code / Open VSX **extension**
- Methods / types on the map
- Non-Rust map analysis
- Editable inspection canvas
- Marketplace publishing
- Incremental / watch-mode analyse (full rebuild still OK)

## Acceptance checks

- [ ] Horizon IDE launches from repo scripts as a **forked product**
- [ ] Map is available **without** installing an extension
- [ ] Classic: open/edit Rust file, terminal, SCM, search usable
- [ ] rust-analyzer hover/diags work in classic editors
- [ ] Toggle to Map; analyse workspace; free-function nodes visible
- [ ] Select function → read-only inspection shows that function
- [ ] In inspection: hover docs, go-to-definition, diagnostics, completions (no edits)
- [ ] Toggle back to classic without losing workspace
- [ ] UI QA exercised **inside the IDE**
- [ ] Opus 5 security findings addressed
