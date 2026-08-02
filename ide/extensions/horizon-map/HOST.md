# Horizon Map — extension host (W3)

Host-side TypeScript for the built-in `horizon-map` extension: sidecar
lifecycle, Map ↔ Classic toggle, and the **read-only inspection canvas**.

W2 owns `media/*`, `README.md` (protocol tables), and `src/webviewHost.ts`
(`getMapWebviewHtml` + shared message unions). This document covers host
behaviour and merge notes for inspection / sidecar / toggle.

## How inspection is opened

On webview `selectFunction` (free functions only):

1. Resolve `filePath` under the workspace folder (reject escapes).
2. Optionally hash-check `contentHash` against the on-disk file (warn if stale).
3. `vscode.workspace.openTextDocument` + `showTextDocument` on the **real
   `file:` URI** in `ViewColumn.Beside` (reuse the inspection column on
   subsequent selects).
4. Reveal the function range: UTF-8 `byteStart`/`byteEnd` → `Position`, or
   1-based `line` fallback. Cursor placed at the range start.
5. Mark the session read-only with
   `workbench.action.files.setActiveEditorReadonlyInSession`.
6. Ensure `languageId === "rust"` for `*.rs` so **rust-analyzer** attaches.
7. Status bar shows `Inspect: <fn>` (`functionName`, else FunctionId tail).
   Tab chrome still shows the real filename — RA requires the real URI.

`InspectionController` also:
- Re-asserts readonly when the inspection editor regains focus.
- Reverts buffer changes if readonly somehow fails (undo / revert).
- Exposes `horizon.inspection.triggerSuggest` → `editor.action.triggerSuggest`
  (suggest widget is allowed; accepting cannot write a readonly buffer).

## rust-analyzer features — verified assumptions

| Feature | How it works on Inspection |
|---|---|
| Hover docs | Normal editor hover on the real file URI; RA must be installed/running |
| Go-to-definition | Normal F12 / ctrl-click; may open another classic tab (writable) |
| Diagnostics | Problems panel + gutter from RA on the workspace file |
| Completions | Suggest widget via Ctrl+Space or `Horizon: Trigger Inspection Completions`; informational only — canvas stays non-editable |

Assumptions:
- `rust-lang.rust-analyzer` is available (repo `.vscode/extensions.json`
  recommends it; `package.json` lists `extensionDependencies`).
- The open folder is a Rust workspace RA can load (Cargo.toml present).
- Map offsets match the current file bytes (re-analyse after edits).

Without RA, the inspection editor still opens read-only; hover/defs/diags/completions will be missing.

## Map ↔ Classic toggle

`MapToggle` snapshots the active `file:` editor before showing the map and
restores that document/column/selection on classic. Workspace folders and open
tabs are not closed. Keybinding: `Ctrl/Cmd+Shift+M`.

## Sidecar lifecycle

`HorizonSidecar` (`src/sidecar.ts`, W4):

| Phase | Behaviour |
|---|---|
| Attach | If `HORIZON_SIDECAR_URL` / `horizon.map.sidecarUrl` is set, health-check and use it (do not spawn/kill) |
| Start | Else prefer `horizon.map.serverPath` / `HORIZON_SERVER_PATH`, else `target/release/horizon-server` (then debug), else `horizon-server` on PATH, else `cargo run -p horizon-server -- --no-open` |
| Discover | Parse `http://127.0.0.1:PORT/` from stdout **or** stderr; refuse non-loopback URLs |
| Health | `GET /api/health` → `{ ok: true }` |
| Analyse | `POST /api/analyse` `{ path }` (path must stay under workspace when known), poll `GET /api/analyse`, then `GET /api/map` |
| Stop | Process-group `SIGTERM`/`SIGKILL` on deactivate / dispose (skipped when attached externally) |
| Logs | VS Code output channel **Horizon** |

Localhost + server `host_guard` already restrict the HTTP surface.

Manual test without the IDE:

```bash
./ide/scripts/run-sidecar.sh
# note http://127.0.0.1:PORT/
curl -s http://127.0.0.1:PORT/api/health
# {"ok":true}
export HORIZON_SIDECAR_URL=http://127.0.0.1:PORT
```

## Commands

| Command | Action |
|---|---|
| `horizon.map.open` | Show/focus Map view |
| `horizon.map.toggle` | Show/focus Map view ↔ restore classic text editor |
| `horizon.map.analyse` | Analyse active workspace folder via sidecar |
| `horizon.map.show` / `horizon.map.hide` | Aliases for open / classic |
| `horizon.map.analyseWorkspace` | Alias for `horizon.map.analyse` |
| `horizon.inspection.triggerSuggest` | Trigger completions in the active Inspection editor |

Keybindings: `Ctrl/Cmd+Shift+M` → toggle; `Ctrl/Cmd+Shift+H` → open.

View id: `horizon.map.view` (activity-bar container `horizon`).

## Manual test checklist

1. Install / enable **rust-analyzer**; open a Rust Cargo workspace in Horizon IDE.
2. Classic: confirm hover + Problems work on a `.rs` file.
3. `Horizon: Open Map` → Analyse workspace → free-function nodes appear.
4. Select a free function → editor opens **beside** the map on the real file,
   scrolled to the fn; status bar reads `Inspect: <name>`; typing does nothing
   (readonly).
5. In Inspection: hover a symbol (RA docs), F12 go-to-def, confirm diagnostics
   in Problems, run **Horizon: Trigger Inspection Completions** (or Ctrl+Space)
   — widget may show; accepting must not edit the buffer.
6. `Ctrl/Cmd+Shift+M` → classic restores the previous editor focus; workspace
   folder unchanged. Toggle back to Map — map webview retained.

## Sidecar attach

Prefer an already-running server:

```bash
./ide/scripts/run-sidecar.sh
# then:
export HORIZON_SIDECAR_URL=http://127.0.0.1:PORT
# or setting horizon.map.sidecarUrl
```

When set, `HorizonSidecar` attaches (health-checks `/api/health`) and does **not**
spawn or kill the process. Otherwise it spawns via `serverPath` / release binary /
PATH / `cargo run -p horizon-server`.

## rust-analyzer recommendation

- Repo root: `.vscode/extensions.json` → `rust-lang.rust-analyzer`.
- Extension: `package.json` → `extensionDependencies: ["rust-lang.rust-analyzer"]`.
- **Code-OSS / Horizon fork**: install from [Open VSX](https://open-vsx.org/extension/rust-lang/rust-analyzer)
  or ship RA as a built-in in product overlays (W1).

## Webview protocol (merge with W2 `media/bridge.js`)

### Webview → host

| `type` | Fields | Host action |
|---|---|---|
| `ready` | — | Push `workspaceInfo`; push cached `mapData` if any |
| `analyse` | `path?` | Sidecar analyse (path must stay under workspace); post progress + `mapData` |
| `selectFunction` | `functionId`, `filePath`, `functionName?`, `line`, `byteStart`, `byteEnd`, `contentHash`, … | Open read-only inspection |
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
| `horizon.map.serverPath` | Absolute path to `horizon-server` binary; env `HORIZON_SERVER_PATH` |
| `horizon.map.cargoWorkspace` | Horizon repo root for `target/release` lookup / `cargo run -p horizon-server` |
| `horizon.map.autoStartSidecar` | Warm-start / attach sidecar on activate (default `true`); else start on first Analyse |

## Security notes

- Sidecar base URL must be loopback (`127.0.0.1` / `localhost` / `::1`).
- Analyse / inspection / source paths are resolved under the workspace folder.
- No remote URL fetches for map JSON — only `file:` URIs from the open dialog.
- Inspection never writes the buffer; edit guard discards accidental mutations.
