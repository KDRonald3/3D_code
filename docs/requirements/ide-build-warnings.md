# Horizon IDE build warnings (local Windows)

**Status:** local product build **SUCCEEDS** on Windows via `ide\scripts\windows\Build.ps1`  
(first attempt failed at `npm ci`; see Build failure and Resolved blockers below)  
**Date:** 1 August 2026  
**Host:** Windows 10.0.26200 (Git Bash `mingw64` first, then PowerShell)  
**Branch:** `main` @ `b43261d` (includes `ide/` from PR #15)  
**Commands:**

```powershell
.\ide\scripts\windows\Bootstrap.ps1        # succeeded
.\ide\scripts\windows\Build.ps1            # succeeded (after fixes below)
cargo build -p horizon-server --release    # succeeded (sidecar)
.\ide\scripts\windows\Run-Sidecar.ps1      # succeeded, serves 127.0.0.1
.\ide\scripts\windows\Run.ps1 .            # succeeded, Horizon Map opens
```

**Build log source:** Cursor terminal running the scripts above  
**Pinned Code-OSS:** `1.105.1` → `ide/code-oss/`  
**Node / npm at first (failing) attempt:** Node `v26.3.0`, npm `11.16.0`  
**Node / npm for the successful build:** Node `v22.19.0` (per `ide/code-oss/.nvmrc`), npm `11.19.0`

> Agents: append new warnings from later build/compile/run phases below. Prefer unique lines; do not delete prior entries unless confirmed obsolete. Keep this file under `docs/requirements/` so it stays in-repo and discoverable.

---

## Environment / platform notes (not npm warnings, but important)

| Note | Detail |
|---|---|
| Primary docs assume Linux | `ide/README.md` marks Linux x64 as primary; Windows is “may work” |
| Platform string from scripts | `mingw64_nt-10.0-26200-x64` |
| Bootstrap | Shallow-cloned `microsoft/vscode` @ `1.105.1`, product overlay applied, `contrib/horizon` synced |
| Sidecar | `target/release/horizon-server.exe` built successfully |
| Visual Studio | VS **2026 Insiders** found at `C:\Program Files\Microsoft Visual Studio\18\Insiders` — missing Spectre-mitigated libs (see Build failure) |
| Python used by node-gyp | 3.14.4 at `C:\Users\kouat\AppData\Local\Python\pythoncore-3.14-64\python.exe` |

---

## npm config warnings (from `npm ci` / install)

These come from Cursor/agent env + Code-OSS `.npmrc` knobs that current npm does not recognise:

```text
npm warn Unknown env config "devdir". This will stop working in the next major version of npm. See `npm help npmrc` for supported config options.
npm warn Unknown project config "disturl". This will stop working in the next major version of npm. See `npm help npmrc` for supported config options.
npm warn Unknown project config "target". This will stop working in the next major version of npm. See `npm help npmrc` for supported config options.
npm warn Unknown project config "ms_build_id". This will stop working in the next major version of npm. See `npm help npmrc` for supported config options.
npm warn Unknown project config "runtime". This will stop working in the next major version of npm. See `npm help npmrc` for supported config options.
npm warn Unknown project config "build_from_source". This will stop working in the next major version of npm. See `npm help npmrc` for supported config options.
npm warn Unknown project config "timeout". This will stop working in the next major version of npm. See `npm help npmrc` for supported config options.
npm warn Unknown project config "npm_config_node_gyp". This will stop working in the next major version of npm. See `npm help npmrc` for supported config options.
```

**Interpretation for agents:**

- `devdir` is almost certainly injected by the agent/Cursor environment, not by Horizon product code.
- The `disturl` / `target` / `runtime` / `ms_build_id` / `build_from_source` / `timeout` / `npm_config_node_gyp` set are typical Electron/`node-gyp` project configs from upstream `microsoft/vscode`. Upstream still ships them; npm 11 only warns that they will stop working in a future major.

Also seen:

```text
npm warn skipping integrity check for git dependency ssh://git@github.com/parcel-bundler/watcher.git
```

---

## Deprecated dependency warnings (from `npm ci`)

Upstream Code-OSS transitive deps. Unique lines observed:

```text
npm warn deprecated urix@0.1.0: Please see https://github.com/lydell/urix#deprecated
npm warn deprecated stable@0.1.8: Modern JS already guarantees Array#sort() is a stable sort, so this library is deprecated. See the compatibility table on MDN: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Array/sort#browser_compatibility
npm warn deprecated source-map-url@0.4.0: See https://github.com/lydell/source-map-url#deprecated
npm warn deprecated source-map-resolve@0.6.0: See https://github.com/lydell/source-map-resolve#deprecated
npm warn deprecated rimraf@2.6.3: Rimraf versions prior to v4 are no longer supported
npm warn deprecated resolve-url@0.2.1: https://github.com/lydell/resolve-url#deprecated
npm warn deprecated osenv@0.1.5: This package is no longer supported.
npm warn deprecated sinon@12.0.1: 16.1.1
npm warn deprecated is-data-descriptor@1.0.0: Please upgrade to v1.0.1
npm warn deprecated is-accessor-descriptor@1.0.0: Please upgrade to v1.0.1
npm warn deprecated inflight@1.0.6: This module is not supported, and leaks memory. Do not use it. Check out lru-cache if you want a good and tested way to coalesce async requests by a key value, which is much more comprehensive and powerful.
npm warn deprecated gulp-vinyl-zip@2.1.2: Package no longer supported. Contact Support at https://www.npmjs.com/support for more info.
npm warn deprecated glob@5.0.15: Glob versions prior to v9 are no longer supported
npm warn deprecated fstream@1.0.12: This package is no longer supported.
npm warn deprecated asar@3.0.3: Please use @electron/asar moving forward.  There is no API change, just a package name change
npm warn deprecated glob@7.1.6: Glob versions prior to v9 are no longer supported
npm warn deprecated glob@7.2.3: Glob versions prior to v9 are no longer supported
npm warn deprecated is-data-descriptor@0.1.4: Please upgrade to v0.1.5
npm warn deprecated is-accessor-descriptor@0.1.6: Please upgrade to v0.1.7
npm warn deprecated source-map-resolve@0.5.3: See https://github.com/lydell/source-map-resolve#deprecated
npm warn deprecated glob@7.1.7: Glob versions prior to v9 are no longer supported
npm warn deprecated @azure/core-http@2.3.2: This package is no longer supported. Please migrate to use @azure/core-rest-pipeline
npm warn deprecated glob@8.1.0: Glob versions prior to v9 are no longer supported
npm warn deprecated tar@2.2.2: This version of tar is no longer supported, and will not receive security updates. Please upgrade asap.
npm warn deprecated chokidar@2.1.8: Chokidar 2 does not receive security updates since 2019. Upgrade to chokidar 3 with 15x fewer dependencies
```

**Interpretation for agents:** these are upstream vscode dependency-tree noise unless install/compile fails. Do not “fix” them inside Horizon by editing `ide/code-oss/package-lock.json` casually — that tree is a shallow clone of upstream.

---

## Bootstrap warnings

None printed. Bootstrap reported:

- platform `mingw64_nt-10.0-26200-x64`
- clone of `1.105.1` succeeded
- product overlay applied
- `contrib/horizon` copied / wired into `workbench.common.main.ts`

---

## Build failure (blocking) — `npm ci` / `node-gyp`

`./ide/scripts/build.sh` exited **1** after ~3 minutes. Root cause:

```text
npm error path ...\ide\code-oss\node_modules\@vscode\windows-registry
npm error command failed
npm error command ... node-gyp rebuild
error MSB8040: Spectre-mitigated libraries are required for this project.
Install them from the Visual Studio installer (Individual components tab)
for any toolsets and architectures being used.
Learn more: https://aka.ms/Ofhn4c
[...\@vscode\windows-registry\build\winregistry.vcxproj]
```

Also logged:

```text
npm warn cleanup [Error: EPERM: operation not permitted, rmdir '...\node_modules\webpack']
gyp info using node@26.3.0 | win32 | x64
gyp info find VS using VS2026 (18.6.11723.189) at
  C:\Program Files\Microsoft Visual Studio\18\Insiders
npm notice New major version of npm available! 11.16.0 -> 12.0.2
error: npm ci failed — see errors above
```

**Fix for the owner / next agent:**

1. Open **Visual Studio Installer** → VS 2026 Insiders → Modify.
2. Individual components → install Spectre-mitigated libs for the MSVC toolset in use (v180 / VS 2026), x64 (and x86 if requested), e.g.  
   **MSVC v143/v145 Spectre-mitigated libs (x64/x86)** / equivalent for VS 18.
3. Clean partial install and rebuild:

```bash
rm -rf ide/code-oss/node_modules
./ide/scripts/build.sh
```

Or via PowerShell: `Remove-Item -Recurse -Force ide\code-oss\node_modules` then re-run build through Git Bash.

---

## Resolved blockers (Windows, in the order they appeared)

| Blocker | Resolution |
|---|---|
| `MSB8040: Spectre-mitigated libraries are required` | Installed **VS 2026 Community** with Spectre-mitigated MSVC libs. The Insiders channel had expired and could not be modified. |
| Playwright Chromium download hangs/times out during `npm ci` | Set `PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1` (now defaulted by `Build.ps1`). |
| `MSB3374: ... file is being used by another process` | Killed leftover `msbuild`/`cl`/`link`/`node` processes; serialised install scripts with `npm_config_foreground_scripts=true` and `npm_config_maxsockets=1`. |
| Code-OSS `preinstall.js`: `Invalid C/C++ Compiler Toolchain` | `preinstall.js` only knew VS 2019/2022. Patched to accept VS 2026, and `vs2026_install` / `vs2022_install` are exported to the VS 18 install path. |
| `v8config.h ... C1189: "C++20 or later required."` building `tree-sitter` | Node 24+ V8 headers need C++20 while pinned native deps still compile as C++17. Switched to Node 22.19.0 from `.nvmrc`. |
| `The property 'Statement' cannot be found on this object` | npm 10.x `npm.ps1` is incompatible with `Set-StrictMode -Version Latest`. Scripts now invoke `npm.cmd` directly. |
| `gyp ERR! Could not find any Visual Studio installation to use` | node-gyp 11.x detects only up to VS 2022. Upgraded npm (`npm install -g npm@11`) so the bundled node-gyp is 12.x, which knows VS 2026. |

---

## Compile / gulp / TypeScript warnings

`npm run compile` completed and produced `out/vs/workbench/contrib/horizon/**`. No warnings retained in the agent capture.

```text
(none retained)
```

---

## Run / launch warnings

From `%APPDATA%\code-oss-dev\logs\<stamp>\window1\renderer.log` on a clean launch:

```text
[error] CodeExpectedError: No default agent contributed
    at ChatService.activateDefaultAgent (.../contrib/chat/common/chatServiceImpl.js)
```

Upstream Code-OSS noise: the Chat contrib has no default agent in an OSS build. Not a Horizon regression.

Horizon Map itself loads: all five `Horizon:` commands register, the Map EditorPane opens, and the webview renders.

Measured against the build at `b43261d`, in-IDE analyse reported a W4 stub and a map had to be produced out-of-band (`POST /api/analyse`, then `GET /api/map`) and loaded via **Open map JSON**. Sidecar spawn/attach landed on `main` afterwards, so re-check this section against a current build before trusting it.

---

## Cargo / sidecar warnings

`cargo build -p horizon-server --release` finished successfully with no warning lines retained in the agent capture. If future sidecar builds emit `warning:` lines, append them here.

---

## Action checklist for later agents

1. When `./ide/scripts/build.sh` finishes, re-scan the build terminal / log and append **compile** warnings/errors to the Compile section.
2. If native modules fail on Windows, record the exact `node-gyp` / Visual Studio message here — that is the likely real blocker, not the deprecation spam above.
3. After `./ide/scripts/run.sh`, capture Electron / workbench / Horizon Map console warnings.
4. Do not treat unknown npm project-config warnings as Horizon regressions unless npm starts *failing* on them.
