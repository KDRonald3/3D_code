# Horizon IDE MVP Plan

## Decisions (locked)

| Topic | Decision |
|---|---|
| Host | Branded **Code-OSS fork** that builds/runs from this repo (not an extension-only product) |
| Classic IDE | Keep full VS Code capabilities we can get from Code-OSS (edit, terminal, SCM, search, extensions, rust-analyzer) |
| Map scope | **Rust free functions only** (methods/`impl` stay out of the map) |
| Map language | Rust only |
| View toggle | On-demand switch: **Map view** ↔ **Classic editor** |
| Inspection canvas | **Read-only** function source surface |
| Language intelligence | Full rust-analyzer on that surface: **hover docs, go-to-def, diagnostics/errors/warnings, completions** (completions are informational; canvas stays non-editable) |
| Analysis backend | Existing `horizon-engine` / `horizon-server` as local sidecar |
| Security review | Claude Opus 5 high after implementation |

## End state (MVP)

1. `./ide/scripts/bootstrap.sh` fetches Code-OSS and applies Horizon product overlays.
2. `./ide/scripts/build.sh` produces a runnable Horizon IDE binary/app.
3. Opening a Rust workspace gives classic Code-OSS editing + rust-analyzer.
4. Command/UI toggle opens the **Horizon Map** (spatial file/function canvas from the existing desktop viewer).
5. Selecting a free function opens the **Inspection** surface: read-only editor on the real workspace file (so rust-analyzer works), focused on that function’s range.
6. Map analyse uses the in-repo sidecar (`horizon-server` / engine), free-function contract unchanged.

## Architecture

```text
Horizon IDE (Code-OSS product)
├── Classic workbench          ← stock Code-OSS
├── Built-in: horizon-map      ← Map webview + toggle + inspection bridge
├── Built-in/recommended: rust-analyzer
└── Sidecar: horizon-server    ← analyse + map JSON + source slices
```

### Why inspection uses a real text editor

Webview Monaco cannot talk to rust-analyzer cleanly. The inspection canvas is a **read-only VS Code text editor** bound to the real file URI/range (hash-checked against the map). That gives hover, definitions, diagnostics, and completion lists without inventing a second LSP bridge.

The Map remains a webview (port of `crates/horizon-server/web/*`).

## Workstreams (parallel)

| ID | Owner focus | Deliverables |
|---|---|---|
| W1 | Product shell | `ide/product/product.json`, branding, bootstrap/build/run scripts, `.gitignore` for Code-OSS checkout |
| W2 | Map extension | `ide/extensions/horizon-map` — webview Map, analyse workspace, Layers/Inspector/DAG/diagnostics reused or adapted |
| W3 | Inspection + RA | Read-only reveal/open of function range; ensure rust-analyzer features work; toggle Map ↔ Classic |
| W4 | Sidecar bridge | Extension spawns/manages `horizon-server`; workspace root → `/api/analyse`; secure localhost-only |
| W5 | Integrate + smoke | Build/run path, fixture workspace demo, README/IDE docs |
| W6 | Security (Opus 5) | Survey sidecar, webview messaging, path handling; fix findings |

## Non-goals for this MVP

- Methods / types on the map
- Non-Rust map analysis
- Editable inspection canvas
- Marketplace publishing
- Incremental / watch-mode analyse (full rebuild still OK)
- Agent-facing query API

## Acceptance checks

- [ ] Horizon IDE launches from repo scripts
- [ ] Classic: open/edit Rust file, terminal, SCM, search usable
- [ ] rust-analyzer hover/diags work in classic editors
- [ ] Toggle to Map; analyse current workspace; free-function nodes visible
- [ ] Select function → read-only inspection shows that function
- [ ] In inspection: hover docs, go-to-definition, Problem diagnostics, trigger completions (no edits)
- [ ] Toggle back to classic without losing workspace
- [ ] Opus 5 security findings addressed
