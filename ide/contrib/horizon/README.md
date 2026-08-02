# Horizon workbench contribution

**This is the product surface** for the Horizon Map inside Horizon IDE (a branded
Code-OSS fork). It is **not** a VS Code / Open VSX extension.

## Role

| Piece | Path |
|---|---|
| Source of truth (this repo) | `ide/contrib/horizon/` |
| Synced into Code-OSS | `ide/code-oss/src/vs/workbench/contrib/horizon/` |
| Compiled into product | `ide/code-oss/out/vs/workbench/contrib/horizon/` (via `gulp compile-client`) |
| Registration | imported from `workbench.common.main.ts` via bootstrap/sync |

The Map opens as a built-in **EditorPane** (`HorizonMapEditorPane`) with a
singleton `HorizonMapInput` (`horizon-map:` scheme). Commands:

| Command ID | Action |
|---|---|
| `horizon.map.open` / `horizon.map.show` | Open / focus the Map EditorPane |
| `horizon.map.toggle` | Map ↔ classic editors |
| `horizon.map.hide` | Close map, focus classic |
| `horizon.map.analyse` | Open map + request workspace analyse |

## One path: bootstrap → build → run

```bash
# Linux / WSL
./ide/scripts/bootstrap.sh          # clone Code-OSS, brand, sync this folder
./ide/scripts/build.sh              # always sync + gulp compile-client → out/
./ide/scripts/run.sh [workspace]    # full product (buttons, auto-analyse, folder picker)

# After editing this folder (prior build required):
./ide/scripts/dev.sh [workspace]    # sync + compile-client + run
```

```powershell
# Windows
.\ide\scripts\windows\Bootstrap.ps1
.\ide\scripts\windows\Build.ps1
.\ide\scripts\windows\Run.ps1 .
.\ide\scripts\windows\Dev.ps1 .     # fast iteration
```

`build` / `run` always keep this tree synced. `build` compiles contrib TypeScript into
`out/` (default `npx gulp compile-client`). `run` recompiles if sources are newer than
`out/`, applies the product overlay, and starts the analyse sidecar.

`ide/extensions/horizon-map/` is **deprecated as product** — see its `DEPRECATED.md`.

## Layout

```text
ide/contrib/horizon/
  README.md                 ← you are here
  common/horizon.ts         ← IDs, protocol types
  browser/
    horizon.contribution.ts ← register EditorPane, commands, menus, chrome
    horizonEditorPane.ts    ← EditorPane + webview host
    horizonInput.ts         ← EditorInput
    horizonHtml.ts          ← media HTML / CSP / asWebviewUri
    horizonAnalysis.ts      ← folder + analyse service (sidecar attach)
    horizonInspection.ts    ← read-only inspection + rust-analyzer
    horizonSidebar.ts       ← activity-bar Horizon view
    media/                  ← map viewer assets
```

## Visible entry points (no Command Palette required)

| Control | Where |
|---|---|
| **Horizon** activity-bar icon | Left activity bar → Open Map / Analyse / folder |
| Status-bar **Horizon** / **Horizon Map** | Bottom status bar (click toggles Map ↔ Classic) |
| Editor title **map** icon | Top-right of editor when Map is closed |
| Editor title **hide / analyse** | Top-right when Map is open |
| View → Horizon Map | Application menu |

## Workstream status

| Area | Status |
|---|---|
| EditorPane + commands + media webview | This folder (W2′) |
| Activity bar / status bar / editor title | This folder (W2′) |
| Read-only inspection + rust-analyzer | W3′ |
| Sidecar spawn/attach from contrib | W4 |
| Auto-analyse + folder picker | W4b |
| IDE UI smoke tests | W5 |

See [`docs/requirements/ide-mvp-plan.md`](../../../docs/requirements/ide-mvp-plan.md).
