# Horizon IDE on Windows

The product is a **Code-OSS fork** (not a VS Code extension). On Windows you have two workable paths:

1. **Native Windows** — PowerShell scripts under `ide\scripts\windows\`
2. **WSL2 (recommended if native compile fails)** — use the Linux bash scripts inside Ubuntu

Compiling Code-OSS from source is heavy on every OS. Windows additionally requires the Visual C++ toolchain.

**One path:** `Bootstrap.ps1` → `Build.ps1` → `Run.ps1` = full Horizon IDE with Map buttons, auto-analyse, and folder picker built in.

## Path A — Native Windows (PowerShell)

### Prerequisites

| Tool | Notes |
|---|---|
| **Node.js 22.x** | Use the version in `ide\code-oss\.nvmrc` (currently 22.19.0); minimum is 22.15.1. Node 24+ breaks native module builds — see Common native failures. Close/reopen PowerShell after install. |
| **Git for Windows** | https://git-scm.com/download/win |
| **Python 3** | On PATH as `python`. Enable “Add python.exe to PATH” in the installer. |
| **Visual Studio 2026** (preferred) or **2022** | Install Build Tools / Community with workload **Desktop development with C++**. |
| **Rust / Cargo** (sidecar) | https://rustup.rs — needed for `horizon-server` analyse |
| **RAM / disk** | ~8–15 GB RAM free during compile; several GB under `ide\code-oss\` |

`Bootstrap.ps1` / `Build.ps1` always patch Code-OSS `preinstall.js` so the pinned vscode **1.105.x** tree accepts **VS 2026** (upstream that pin only listed 2019/2022). They also set `vs2026_install` / `vs2022_install` from `vswhere` when needed for older node-gyp — note VS 2026 installs under `...\18\...`, not `...\2026\...`.

Upstream reference: [VS Code How to Contribute — Prerequisites](https://github.com/microsoft/vscode/wiki/How-to-Contribute#prerequisites).

### Commands (from repo root, PowerShell)

```powershell
# Allow local scripts for this session if needed:
Set-ExecutionPolicy -Scope Process Bypass

.\ide\scripts\windows\Bootstrap.ps1
.\ide\scripts\windows\Build.ps1          # sync contrib + npm ci + gulp compile-client
.\ide\scripts\windows\Run.ps1 .          # overlay + freshness + sidecar + launch

# Fast iteration after editing ide\contrib\horizon (prior Build required):
.\ide\scripts\windows\Dev.ps1 .

# Optional: full client+extensions compile instead of compile-client
$env:HORIZON_COMPILE_MODE = "full"
.\ide\scripts\windows\Build.ps1

# Sidecar (also auto-started by Run.ps1 when horizon-server.exe exists)
cargo build -p horizon-server --release
.\ide\scripts\windows\Run-Sidecar.ps1    # writes ide\.cache\horizon-sidecar.url
```

Inside the IDE: activity-bar **Horizon**, status-bar **Horizon**, editor-title map/analyse — or Command Palette → **Horizon: Open Horizon Map**.

`Build.ps1` **always** syncs `ide\contrib\horizon` and compiles it into `out\` (default `npx gulp compile-client`). `Run.ps1` re-applies the product overlay, recompiles if contrib sources are newer than `out\`, and starts/attaches the sidecar via `ide\.cache\horizon-sidecar.url`.

The first `Build.ps1` takes tens of minutes.

### Common native failures

| Symptom | Fix |
|---|---|
| `Invalid C/C++ Compiler Toolchain` | Install **VS 2026** (or 2022) Build Tools + **Desktop development with C++**, re-run `Bootstrap.ps1` then `Build.ps1`. If only 2026 is installed and detect still fails, set `$env:vs2026_install` (and `$env:vs2022_install`) to the install path from `vswhere`. |
| `node-gyp` / unknown VS 2026 version | Run from **Developer PowerShell for VS 2026**, or `npm config set msvs_version 2026` (try `2022` if your node-gyp is older). Re-run `Build.ps1`. |
| `gyp ERR! Could not find any Visual Studio installation to use` with VS 2026 installed | node-gyp **11.x and older only detect up to VS 2022**. Upgrade the npm that ships it: `npm install -g npm@11` (bundles node-gyp 12+). Package install scripts spawn npm's bundled node-gyp, so `npm_config_node_gyp` alone is not enough. |
| `v8config.h ... error C1189: "C++20 or later required."` (usually building `tree-sitter`) | Node **24+** headers require C++20 while pinned native deps still compile as C++17. Use the version in `ide\code-oss\.nvmrc` (22.x): `nvm install 22.19.0; nvm use 22.19.0`, delete `ide\code-oss\node_modules` and `ide\code-oss\build\node_modules`, re-run `Build.ps1` |
| `The property 'Statement' cannot be found on this object` | npm 10.x `npm.ps1` is incompatible with `Set-StrictMode -Version Latest`. The scripts call `npm.cmd` / `npx.cmd`; if you invoke npm yourself from a strict-mode script, do the same. |
| `MSB8040: Spectre-mitigated libraries are required` | Install the Spectre-mitigated MSVC libs for your toolset from the VS Installer (Individual components). |
| Playwright download hangs during `npm ci` | `Build.ps1` sets `PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1`; export it yourself if you run `npm ci` directly. |
| `Please use Node.js v22.15.1 or later` | Upgrade Node; `node -v` must be ≥ 22.15.1 |
| `yarn is not supported` | Use **npm** only |
| `npm ci` / native module build fails | Delete `ide\code-oss\node_modules`, disable antivirus for that folder, re-run Build |
| Bash scripts fail in Git Bash | Use the **PowerShell** scripts in `ide\scripts\windows\` — do not rely on `*.sh` on Windows |
| Path length / EPERM under `node_modules` | Enable long paths (`git config --system core.longpaths true`), run PowerShell as Admin once if needed |
| Out of memory during compile | Close other apps; `NODE_OPTIONS=--max-old-space-size=8192` (set by Build.ps1) |

## Path B — WSL2 (recommended when native build keeps failing)

1. Install WSL2 + Ubuntu (`wsl --install`).
2. Clone the repo **inside the Linux filesystem** (e.g. `~/src/Horizon`), not under `/mnt/c/...` (slow + permission issues).
3. Install Linux deps (Node 22 via nvm, build-essential, python3, pkg-config, libkrb5-dev, libx11-dev, libxkbfile-dev, libsecret-1-dev).
4. Run the Linux product path:

```bash
./ide/scripts/bootstrap.sh
./ide/scripts/build.sh
./ide/scripts/run.sh .
```

WSL can launch the Linux Electron GUI with WSLg on recent Windows 11.

## What will not work

- Treating `ide\extensions\horizon-map` as the product (deprecated; Map is `ide\contrib\horizon`).
- Expecting `.\ide\scripts\build.sh` to be a supported Windows native entrypoint (use `windows\Build.ps1` or WSL).
- Skipping the C++ toolchain on native Windows — Code-OSS `npm` install compiles native modules.

## Getting help

Paste the **last ~40 lines** of the failing command (`Bootstrap.ps1` / `Build.ps1` / `npm ci` / `gulp compile-client`) and:

```powershell
node -v
npm -v
git --version
python --version
& "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
```
