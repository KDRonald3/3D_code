# Horizon IDE

Branded **Code-OSS** product shell for Horizon: classic VS Code capabilities plus the
Rust free-function map as a **built-in workbench EditorPane** (not a VS Code extension).

See [`docs/requirements/ide-mvp-plan.md`](../docs/requirements/ide-mvp-plan.md).

## One path: bootstrap → build → run

That sequence produces a **full Horizon IDE** with Map chrome (activity bar / status bar /
editor title buttons), auto-analyse wiring, folder picker, and contrib compiled into `out/`.

### Linux / WSL

```bash
./ide/scripts/bootstrap.sh          # shallow-clone → ide/code-oss/, brand, sync contrib
./ide/scripts/build.sh              # sync contrib + npm ci + gulp compile-client
./ide/scripts/run.sh                # overlay + freshness check + sidecar + launch
./ide/scripts/run.sh /path/to/workspace

# Fast iteration after editing ide/contrib/horizon (requires a prior build):
./ide/scripts/dev.sh /path/to/workspace
# or: make ide-dev
```

### Windows (PowerShell)

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\ide\scripts\windows\Bootstrap.ps1
.\ide\scripts\windows\Build.ps1
.\ide\scripts\windows\Run.ps1 .

# Fast iteration:
.\ide\scripts\windows\Dev.ps1 .
```

Full Windows prerequisites and troubleshooting: **[`WINDOWS.md`](WINDOWS.md)**.

Inside the IDE you get:

- **Horizon** activity-bar + status-bar + editor-title buttons (Open Map / Analyse / folder)
- Auto-analyse of the active Horizon folder on workspace open
- Folder picker (workspace folders or Browse…) — not JSON upload as the primary path

## Prerequisites

| Requirement | Notes |
|---|---|
| **Linux x64** or **Windows** | Linux: bash scripts. Windows: PowerShell under [`scripts/windows/`](scripts/windows/) |
| **Node.js ≥ 22.15.1** | Upstream Code-OSS rejects older 22.x; [nvm](https://github.com/nvm-sh/nvm) on Linux / official installer on Windows |
| **npm** | Bundled with Node; yarn is not supported by modern vscode |
| **Git** | Shallow clone of `microsoft/vscode` |
| **Build tools** | Linux: `build-essential`, `python3`, `pkg-config`, `libx11-dev`, `libxkbfile-dev`, `libsecret-1-dev`, `libkrb5-dev`. Windows: **Visual Studio 2026** (preferred) or 2022 + **Desktop development with C++** |
| **Rust / Cargo** (analyse) | Needed for `horizon-server` sidecar |
| **RAM / disk** | Full compile wants ~8–15 GB RAM and several GB under `ide/code-oss/` |

Pinned upstream ref: [`product/vscode-ref.txt`](product/vscode-ref.txt) (override with `HORIZON_VSCODE_REF`).

## What `build` does

Every `build.sh` / `Build.ps1` run:

1. Re-applies [`product/product.json`](product/product.json) branding overlay
2. Patches Code-OSS `preinstall.js` to accept **VS 2026** (Windows toolchain)
3. **Always** syncs `ide/contrib/horizon` → `ide/code-oss/src/vs/workbench/contrib/horizon`
4. Runs `npm ci` when `node_modules` is missing/incomplete
5. Compiles the client so Horizon TypeScript lands in `out/`

**Compile mode** (`HORIZON_COMPILE_MODE`):

| Value | Command | When |
|---|---|---|
| `client` (**default**) | `npx gulp compile-client` | Product path — compiles workbench `src` → `out/`, including contrib |
| `full` | `npm run compile` | Client + extensions; heavier and more failure-prone |

```bash
HORIZON_COMPILE_MODE=full ./ide/scripts/build.sh   # optional full compile
```

## What `run` does

1. Re-applies the product overlay
2. If `ide/contrib/horizon` is newer than `out/.../horizon.contribution.js`, auto **sync + compile-client**
3. Starts or attaches `horizon-server` (URL written to `ide/.cache/sidecar.url`, exported as `HORIZON_SIDECAR_URL`)
4. Launches `scripts/code.sh` / `code.bat`

## Layout

```text
ide/
  contrib/horizon/         # PRODUCT: Map EditorPane + media (synced into Code-OSS)
  product/
    product.json           # Horizon branding + Open VSX gallery overlay
    vscode-ref.txt         # pinned microsoft/vscode tag
    branding/              # icon placeholders
  scripts/
    bootstrap.sh / build.sh / run.sh / dev.sh / sync-contrib.sh / run-sidecar.sh
    windows/               # Bootstrap / Build / Run / Dev / Sync-Contrib / Run-Sidecar
    lib.sh                 # shared helpers (bash)
  WINDOWS.md
  extensions/horizon-map/  # DEPRECATED as product
  code-oss/                # gitignored Code-OSS checkout (created by bootstrap)
  .cache/                  # gitignored (sidecar.url, prebuilt editor, …)
```

## Environment knobs

| Variable | Effect |
|---|---|
| `HORIZON_VSCODE_REF` | Tag/commit to clone (default from `product/vscode-ref.txt`) |
| `HORIZON_VSCODE_REPO` | Git remote (default `https://github.com/microsoft/vscode.git`) |
| `HORIZON_CONTRIB_SYNC_MODE` | `copy` (default) or `link` for contrib sync |
| `HORIZON_COMPILE_MODE` | `client` (default, gulp compile-client) or `full` (npm run compile) |
| `HORIZON_FORCE_NPM_CI=1` | Force `npm ci` even if `node_modules` exists |
| `HORIZON_SERVER_PATH` | Absolute path to `horizon-server` |
| `HORIZON_SIDECAR_URL` | Attach to a running server (`http://127.0.0.1:PORT`) |
| `HORIZON_SIDECAR_URL_FILE` | Default `ide/.cache/sidecar.url` — written by run / run-sidecar |
| `VSCODE_SKIP_NODE_VERSION_CHECK=1` | Bypass upstream Node version gate (not recommended) |
| `NODE_OPTIONS` | Defaults to `--max-old-space-size=8192` during compile |

## Sidecar (map analyse)

```bash
cargo build -p horizon-server --release
./ide/scripts/run-sidecar.sh          # writes ide/.cache/sidecar.url
# run.sh also auto-starts the sidecar when a binary is available
```

## Makefile shortcuts (Linux / WSL)

```bash
make ide-bootstrap
make ide-build
make ide-run
make ide-dev
```

## Build notes

- First `build.sh` downloads Electron and compiles the workbench; expect **tens of minutes**.
- Headless agents: use `xvfb-run ./ide/scripts/run.sh` for a smoke launch.
- Map sources: `ide/contrib/horizon/` (see its README).
- `./ide/scripts/dev-extension.sh` is **legacy** — not the product path.
