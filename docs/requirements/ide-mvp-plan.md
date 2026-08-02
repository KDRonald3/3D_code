# Horizon IDE MVP Plan

## Decisions (locked)

| Topic | Decision |
|---|---|
| Host | Branded **Code-OSS fork** that builds/runs from this repo |
| **Not an extension** | Map is a **first-class workbench contribution** under `src/vs/workbench/contrib/horizon/` |
| Classic IDE | Full Code-OSS capabilities (edit, terminal, SCM, search, rust-analyzer) |
| Map scope | Rust free functions only |
| Map language | Rust only |
| View toggle | Map ↔ Classic on demand |
| Inspection | **Read-only** real editor + rust-analyzer (hover, go-to-def, diagnostics, completions) |
| Analysis backend | `horizon-server` as localhost sidecar |
| **Entry UX** | Visible buttons (activity bar, status bar, editor title) — not Command Palette only |
| **Startup** | Opening the IDE / a workspace **auto-analyses** the active Horizon folder |
| **Folder choice** | In-IDE folder picker / workspace-folder QuickPick — **not** JSON upload as primary path |
| **Build = product** | `bootstrap` + `build` sync contrib, compile workbench, produce a runnable IDE with Horizon built in |
| Security review | Claude Opus 5 high after implementation |

## End state

1. `./ide/scripts/bootstrap.sh` (+ Windows `Bootstrap.ps1`) fetch Code-OSS, brand, sync `ide/contrib/horizon`.
2. `./ide/scripts/build.sh` (+ `Build.ps1`) compile the fork with Horizon contrib included.
3. `./ide/scripts/run.sh` (+ `Run.ps1`) launch Horizon IDE with Map, sidecar, auto-analyse, inspection ready.
4. Activity bar **Horizon** + status bar + editor title buttons open Map / analyse / pick folder.
5. On workspace open: auto-start sidecar + analyse selected (or default) folder; Map can open with results.
6. User can change Horizon folder via QuickPick (workspace folders) or folder dialog (under workspace).
7. Selecting a free function opens read-only inspection with rust-analyzer.
8. Opus 5 security survey + fixes.

## Workstreams (parallel)

| ID | Focus | Status target |
|---|---|---|
| W1 | Product shell + build includes contrib | Harden scripts so build always syncs + compiles horizon |
| W2′ | Map EditorPane + chrome buttons | Activity bar, status bar, editor title, menus |
| W3′ | Inspection + rust-analyzer | Read-only reveal of function range |
| W4 | Sidecar in contrib | Spawn/attach, analyse, map, source |
| W4b | Auto-analyse + folder UX | Start-up analyse; choose folder (no JSON upload primary) |
| W5 | Integrate + IDE UI tests | Smoke in Horizon IDE window |
| W6 | Security (Opus 5) | Survey + fix |

## Acceptance checks

- [ ] `build` produces runnable Horizon IDE with Map built in (no separate extension install)
- [ ] Launch opens usable classic IDE + Horizon chrome buttons visible
- [ ] Auto-analyse runs for default/selected folder on start
- [ ] User can choose Horizon folder without JSON upload
- [ ] Map shows free-function graph after analyse
- [ ] Function select → read-only inspection with RA hover/diags/defs/completions
- [ ] Map ↔ Classic toggle
- [ ] Opus 5 security findings addressed

## Non-goals

- Marketplace extension packaging
- Methods/types on the map
- Non-Rust map
- Editable inspection canvas
