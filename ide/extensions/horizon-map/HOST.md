# Horizon Map — extension host (W3)

Host-side TypeScript for the built-in `horizon-map` extension: sidecar
lifecycle, Map ↔ Classic toggle, and the **read-only inspection canvas**.

W2 owns `media/*`, `README.md` (protocol tables), and `src/webviewHost.ts`
(`getMapWebviewHtml` + shared message unions). This document covers host
behaviour and merge notes for inspection / sidecar / toggle.

## How inspection gets rust-analyzer features

Inspection does **not** use webview Monaco. On `selectFunction` the host:

1. Resolves `filePath` under the workspace folder (rejects escapes).
2. Opens the real file URI with `vscode.workspace.openTextDocument` /
   `showTextDocument` in `ViewColumn.Beside`.
3. Sets the session read-only via
   `workbench.action.files.setActiveEditorReadonlyInSession`.
4. Reveals the function range (`byte_start`/`byte_end` UTF-8 offsets →
   `Position`, or 1-based `line` fallback).
5. Ensures `languageId === "rust"` for `*.rs` so **rust-analyzer** attaches.

Because the surface is a normal VS Code text editor on the workspace file,
RA hover, go-to-definition, diagnostics, and completions work unchanged.
Completions are informational only — the canvas stays non-editable.

## Sidecar lifecycle

`HorizonSidecar` (`src/sidecar.ts`):

| Phase | Behaviour |
|---|---|
| Attach | If `HORIZON_SIDECAR_URL` / `horizon.map.sidecarUrl` is set, health-check and use it (do not spawn/kill) |
| Start | Else prefer `horizon.map.serverPath` / `HORIZON_SERVER_PATH`, else a built `target/{release,debug}/horizon-server`, else `cargo run -p horizon-server -- --no-open` |
| Discover | Parse `http://127.0.0.1:PORT/` from stdout; refuse non-loopback URLs |
| Health | `GET /api/health` → `{ ok: true }` |
| Analyse | `POST /api/analyse` `{ path }`, poll `GET /api/analyse`, then `GET /api/map` |
| Stop | `SIGTERM` on deactivate / dispose (skipped when attached externally) |

Localhost + server `host_guard` already restrict the HTTP surface.

## Commands

| Command | Action |
|---|---|
| `horizon.map.open` | Show/focus Map view |
| `horizon.map.toggle` | Show/focus Map view ↔ focus classic text editor |
| `horizon.map.analyse` | Analyse active workspace folder via sidecar |
| `horizon.map.show` / `horizon.map.hide` | Aliases for open / classic |
| `horizon.map.analyseWorkspace` | Alias for `horizon.map.analyse` |

Keybindings: `Ctrl/Cmd+Shift+M` → toggle; `Ctrl/Cmd+Shift+H` → open.

View id: `horizon.map.view` (activity-bar container `horizon`).

## Sidecar attach

Prefer an already-running server:

```bash
export HORIZON_SIDECAR_URL=http://127.0.0.1:PORT
# or setting horizon.map.sidecarUrl
```

When set, `HorizonSidecar` attaches (health-checks `/api/health`) and does **not**
spawn or kill the process. Otherwise it spawns via `serverPath` / built binary /
`cargo run -p horizon-server`.

## rust-analyzer recommendation

`package.json` lists `extensionDependencies: ["rust-lang.rust-analyzer"]`.

- **VS Code / marketplace builds**: marketplace id works.
- **Code-OSS / Horizon fork**: install from [Open VSX](https://open-vsx.org/extension/rust-lang/rust-analyzer)
  or ship RA as a built-in/recommended extension in product overlays (W1).

Without RA, the inspection editor still opens read-only; hover/defs/diags/completions will be missing.

## Webview protocol (merge with W2 `media/bridge.js`)

### Webview → host

| `type` | Fields | Host action |
|---|---|---|
| `ready` | — | Push `workspaceInfo`; push cached `mapData` if any |
| `analyse` | `path?` | Sidecar analyse (path must stay under workspace); post progress + `mapData` |
| `selectFunction` | `functionId`, `filePath`, `line`, `byteStart`, `byteEnd`, `contentHash`, … | Open read-only inspection |
| `selectFile` | `filePath` | Open file read-only beside |
| `openMapJson` | — | Local file picker → `POST /api/map` → `mapData` |
| `sourceRequest` | `requestId`, `path`, `byteStart`, `byteEnd`, `expectedHash` | `GET /api/source` → `sourceResult` (Inspector preview; RA inspection preferred) |

### Host → webview

| `type` | Fields |
|---|---|
| `workspaceInfo` | `root`, `name` |
| `mapData` | `map` (Repository JSON) |
| `analyseResult` | `status`: `idle` \| `running` \| `done` \| `failed` \| `error`; optional `path`, `elapsed_ms`, `error`, `map` |
| `sourceResult` | `requestId`, `tokens?`, `error?`, `errorKind?` |
| `error` | `message` |

Host rewrites W2 `index.html` `/static/*` paths to `webview.asWebviewUri` and
injects `bridge.js` when the HTML does not already include it. If `media/index.html`
is missing, a minimal analyse stub HTML is served instead.

## Configuration

| Setting | Purpose |
|---|---|
| `horizon.map.sidecarUrl` | Attach to running server (`http://127.0.0.1:PORT`); env `HORIZON_SIDECAR_URL` |
| `horizon.map.serverPath` | Absolute path to `horizon-server` binary |
| `horizon.map.cargoWorkspace` | Horizon repo root for `cargo run -p horizon-server` |
| `horizon.map.autoStartSidecar` | Warm-start / attach sidecar on activate (default `true`) |

## Security notes

- Sidecar base URL must be loopback (`127.0.0.1` / `localhost` / `::1`).
- Analyse / inspection / source paths are resolved under the workspace folder.
- No remote URL fetches for map JSON — only `file:` URIs from the open dialog.
