# Horizon IDE

Branded **Code-OSS** product shell for Horizon: classic VS Code capabilities plus the
Rust free-function map as a **built-in workbench EditorPane** (not a VS Code extension).

See [`docs/requirements/ide-mvp-plan.md`](../docs/requirements/ide-mvp-plan.md).

## Prerequisites

| Requirement | Notes |
|---|---|
| **Linux x64** or **Windows** | Linux: bash scripts. Windows: PowerShell scripts in [`scripts/windows/`](scripts/windows/) — see **[`WINDOWS.md`](WINDOWS.md)** |
| **Node.js ≥ 22.15.1** | Upstream Code-OSS rejects older 22.x; use [nvm](https://github.com/nvm-sh/nvm) on Linux / official installer on Windows |
| **npm** | Bundled with Node; yarn is not supported by modern vscode |
| **Git** | Shallow clone of `microsoft/vscode` |
| **Build tools** (full compile) | Linux: `build-essential`, `python3`, `pkg-config`, `libx11-dev`, `libxkbfile-dev`, `libsecret-1-dev`, `libkrb5-dev`. Windows: **Visual Studio 2026** (preferred) or 2022 Build Tools + **Desktop development with C++** |
| **RAM / disk** | Full compile wants ~8–15 GB RAM and several GB under `ide/code-oss/` |

Optional for Electron GUI on Linux: `libnss3`, `libgbm1`, `libgtk-3-0`, `libasound2t64`, and a display (`DISPLAY` or `xvfb-run`).

Pinned upstream ref: [`product/vscode-ref.txt`](product/vscode-ref.txt) (override with `HORIZON_VSCODE_REF`).

## Quick start (Linux / WSL)

```bash
./ide/scripts/bootstrap.sh   # shallow-clone → ide/code-oss/, brand, sync contrib/horizon
./ide/scripts/build.sh       # npm ci + npm run compile  (long-running / heavy)
./ide/scripts/run.sh         # launch built Horizon IDE
./ide/scripts/run.sh /path/to/workspace
```

## Quick start (Windows)

Use PowerShell — do **not** rely on the `.sh` scripts on native Windows:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\ide\scripts\windows\Bootstrap.ps1
.\ide\scripts\windows\Build.ps1
.\ide\scripts\windows\Run.ps1 .
```

Full Windows prerequisites, failure checklist, and WSL2 fallback: **[`WINDOWS.md`](WINDOWS.md)**.

Map commands inside the IDE (Command Palette):

- **Horizon: Open Horizon Map** (`horizon.map.open`)
- **Horizon: Toggle Horizon Map** (`horizon.map.toggle`)
- **Horizon: Analyse Workspace** (`horizon.map.analyse`)

Re-sync the workbench contrib after editing it:

```bash
./ide/scripts/sync-contrib.sh        # copy into ide/code-oss/src/vs/workbench/contrib/horizon/
./ide/scripts/sync-contrib.sh link   # symlink for live edits inside a built IDE
```

## Layout

```text
ide/
  contrib/horizon/         # PRODUCT: Map EditorPane + media (synced into Code-OSS)
  product/
    product.json           # Horizon branding + Open VSX gallery overlay
    vscode-ref.txt         # pinned microsoft/vscode tag
    branding/              # icon placeholders (see branding/README.md)
  scripts/
    bootstrap.sh / build.sh / run.sh / sync-contrib.sh   # Linux / WSL product path
    windows/             # Windows PowerShell: Bootstrap / Build / Run / Sync-Contrib / Run-Sidecar
    sync-extension.sh    # LEGACY — deprecated extension package
    dev-extension.sh     # LEGACY — Extension Development Host (not product)
    lib.sh               # shared helpers (bash)
  WINDOWS.md             # Windows prerequisites + troubleshooting
  extensions/horizon-map/  # DEPRECATED as product — see DEPRECATED.md
  code-oss/                # gitignored Code-OSS checkout (created by bootstrap)
  patches/                 # optional patches
```

## Product branding

`bootstrap.sh` / `build.sh` merge [`product/product.json`](product/product.json) on top of stock Code-OSS `product.json`:

| Field | Value |
|---|---|
| Display name | **Horizon IDE** |
| `applicationName` | `horizon-ide` |
| `dataFolderName` | `.horizon-ide` |
| Marketplace | Open VSX (Microsoft Marketplace disabled) |

The Map is **built into** the workbench (`contrib/horizon`), not installed as an extension.

## Environment knobs

| Variable | Effect |
|---|---|
| `HORIZON_VSCODE_REF` | Tag/commit to clone (default from `product/vscode-ref.txt`) |
| `HORIZON_VSCODE_REPO` | Git remote (default `https://github.com/microsoft/vscode.git`) |
| `HORIZON_CONTRIB_SYNC_MODE` | `copy` (default) or `link` for contrib sync |
| `HORIZON_FORCE_NPM_CI=1` | Force `npm ci` even if `node_modules` exists |
| `HORIZON_SERVER_PATH` | Absolute path to `horizon-server` for the IDE sidecar / `run-sidecar.sh` |
| `HORIZON_SIDECAR_URL` | Attach IDE/preview to a running server (`http://127.0.0.1:PORT`) |
| `VSCODE_SKIP_NODE_VERSION_CHECK=1` | Bypass upstream Node version gate (not recommended) |
| `NODE_OPTIONS` | Defaults to `--max-old-space-size=8192` during compile |

## Sidecar (map analyse)

```bash
./ide/scripts/run-sidecar.sh          # prefer target/release/horizon-server
curl -s http://127.0.0.1:PORT/api/health
export HORIZON_SIDECAR_URL=http://127.0.0.1:PORT   # optional attach mode for the IDE
```

Workbench contrib analyse wiring is W4; until then the Map EditorPane loads media and
accepts bridge messages, with analyse stubbed to a clear notification.

## Build notes

- First `build.sh` downloads Electron and compiles the workbench; expect **tens of minutes**.
- Headless agents: use `xvfb-run ./ide/scripts/run.sh` for a smoke launch.
- Map EditorPane sources: `ide/contrib/horizon/` (see its README).
- `./ide/scripts/dev-extension.sh` is **legacy** and must not be presented as the product path.
