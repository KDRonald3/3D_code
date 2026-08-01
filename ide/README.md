# Horizon IDE

Branded **Code-OSS** product shell for Horizon: classic VS Code capabilities plus the
Rust free-function map and a read-only inspection canvas backed by rust-analyzer.

See [`docs/requirements/ide-mvp-plan.md`](../docs/requirements/ide-mvp-plan.md).

## Prerequisites

| Requirement | Notes |
|---|---|
| **Linux x64** (primary) | Scripts detect OS/arch; macOS/Windows may work with upstream vscode tooling |
| **Node.js ≥ 22.15.1** | Upstream Code-OSS rejects older 22.x; use [nvm](https://github.com/nvm-sh/nvm). Scripts prepend `nvm which 22` onto `PATH` so shims like `/exec-daemon/node` do not win |
| **npm** | Bundled with Node; yarn is not supported by modern vscode |
| **Git** | Shallow clone of `microsoft/vscode` |
| **Build tools** (full compile) | `build-essential`, `python3`, `pkg-config`, `libx11-dev`, `libxkbfile-dev`, `libsecret-1-dev`, `libkrb5-dev` (Debian/Ubuntu) |
| **RAM / disk** | Full compile wants ~8–15 GB RAM and several GB under `ide/code-oss/` |

Optional for Electron GUI: `libnss3`, `libgbm1`, `libgtk-3-0`, `libasound2t64`, and a display (`DISPLAY` or `xvfb-run`).

Pinned upstream ref: [`product/vscode-ref.txt`](product/vscode-ref.txt) (override with `HORIZON_VSCODE_REF`).

## Quick start

### Fast path (extension UI — no full Code-OSS compile)

```bash
./ide/scripts/dev-extension.sh                  # Extension Development Host + horizon-map
./ide/scripts/dev-extension.sh /path/to/workspace
```

Uses a system `code` / `codium` / `code-oss` CLI when available; otherwise downloads a
portable VSCodium Linux binary into `ide/.cache/prebuilt/` (gitignored).

### Full product shell

```bash
./ide/scripts/bootstrap.sh   # shallow-clone → ide/code-oss/, brand, sync extension
./ide/scripts/build.sh       # npm ci + npm run compile  (long-running / heavy)
./ide/scripts/run.sh         # launch built Horizon IDE
./ide/scripts/run.sh /path/to/workspace
```

If the full build is not ready, `run.sh` falls back to the same Extension Development Host
path as `dev-extension.sh`.

Re-sync the map extension into Code-OSS after editing it:

```bash
./ide/scripts/sync-extension.sh        # copy into ide/code-oss/extensions/
./ide/scripts/sync-extension.sh link   # symlink for live edits inside a built IDE
```

## Layout

```text
ide/
  product/
    product.json           # Horizon branding + Open VSX gallery overlay
    vscode-ref.txt         # pinned microsoft/vscode tag
    branding/              # icon placeholders (see branding/README.md)
  scripts/
    bootstrap.sh           # clone + apply product overlay + sync extension
    build.sh               # npm install + compile
    run.sh                 # launch built app (or editor fallback)
    dev-extension.sh       # fast EDH against horizon-map (no full compile)
    sync-extension.sh      # copy/link horizon-map into code-oss
    lib.sh                 # shared helpers
  extensions/horizon-map/  # built-in map + inspection (other workstreams)
  code-oss/                # gitignored Code-OSS checkout (created by bootstrap)
  patches/                 # optional patches (none required for MVP shell)
```

## Product branding

`bootstrap.sh` / `build.sh` merge [`product/product.json`](product/product.json) on top of stock Code-OSS `product.json`:

| Field | Value |
|---|---|
| Display name | **Horizon IDE** |
| `applicationName` | `horizon-ide` |
| `dataFolderName` | `.horizon-ide` |
| Marketplace | Open VSX (Microsoft Marketplace disabled) |

Built-in `horizon-map` is synced into `code-oss/extensions/horizon-map` (not a marketplace install).

## Environment knobs

| Variable | Effect |
|---|---|
| `HORIZON_VSCODE_REF` | Tag/commit to clone (default from `product/vscode-ref.txt`) |
| `HORIZON_VSCODE_REPO` | Git remote (default `https://github.com/microsoft/vscode.git`) |
| `HORIZON_EXTENSION_SYNC_MODE` | `copy` (default) or `link` |
| `HORIZON_FORCE_NPM_CI=1` | Force `npm ci` even if `node_modules` exists |
| `HORIZON_FORCE_EXT_COMPILE=1` | Force `tsc` for horizon-map in `dev-extension.sh` |
| `HORIZON_CODE_CLI` | Path/name of editor CLI for EDH fallback |
| `HORIZON_VSCODIUM_VERSION` | VSCodium release tag for prebuilt download |
| `VSCODE_SKIP_NODE_VERSION_CHECK=1` | Bypass upstream Node version gate (not recommended) |
| `NODE_OPTIONS` | Defaults to `--max-old-space-size=8192` during compile |

## Build notes

- First `build.sh` downloads Electron and compiles the workbench; expect **tens of minutes**.
- Prefer `./ide/scripts/dev-extension.sh` for map/webview UI work on constrained VMs.
- Headless agents: use `xvfb-run ./ide/scripts/run.sh` or `xvfb-run ./ide/scripts/dev-extension.sh` for a smoke launch.
- Map toggle / inspection / sidecar wiring are owned by other workstreams under `ide/extensions/**`.
