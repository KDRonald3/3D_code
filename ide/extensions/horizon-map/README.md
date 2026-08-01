# Horizon Map

Built-in VS Code / Code-OSS extension that hosts the Horizon **Rust free-function map** webview, adapted from `crates/horizon-server/web/*`.

## Commands

| Command | Id | Default keybinding |
|---|---|---|
| Open Map | `horizon.map.open` | `Ctrl/Cmd+Shift+H` |
| Toggle Map ↔ Classic | `horizon.map.toggle` | `Ctrl/Cmd+Shift+M` |
| Analyse Workspace | `horizon.map.analyse` | — |

Aliases (same handlers): `horizon.map.show`, `horizon.map.hide`, `horizon.map.analyseWorkspace`.

## View

Activity-bar container **Horizon** → webview view `horizon.map.view`.

## Build

```bash
cd ide/extensions/horizon-map
npm install
npm run compile
```

Output: `out/extension.js` (committed / synced into the Code-OSS vendor tree).

## Static preview (no IDE)

Serve the self-contained `media/` assets and optionally proxy the sidecar:

```bash
# terminal 1
cargo run -p horizon-server -- --no-open
# note the printed http://127.0.0.1:PORT/

# terminal 2
cd ide/extensions/horizon-map
HORIZON_SIDECAR_URL=http://127.0.0.1:PORT npm run preview
# open http://127.0.0.1:5179/
```

In preview mode the page talks HTTP to the sidecar (desktop path). Inside the
real webview it uses `postMessage` only — never hardcodes a sidecar URL.

## Sidecar (IDE)

The extension host spawns or attaches to `horizon-server` on loopback only.

```bash
# Manual (attach mode)
./ide/scripts/run-sidecar.sh
export HORIZON_SIDECAR_URL=http://127.0.0.1:PORT
# or setting: horizon.map.sidecarUrl
```

When `HORIZON_SIDECAR_URL` / `horizon.map.sidecarUrl` is set, the host attaches for
`/api/analyse`, `/api/map`, `/api/source` and does not spawn or kill the process.
Otherwise it prefers `horizon.map.serverPath` → `target/release/horizon-server` →
PATH → `cargo run -p horizon-server -- --no-open`. Logs go to the **Horizon**
output channel.

---

## Webview ↔ extension message protocol

All traffic uses `acquireVsCodeApi().postMessage` / `webview.onDidReceiveMessage`. The webview **must not** call sidecar HTTP (`http://127.0.0.1:…/api/…`) directly when `HorizonBridge.isVsCode` is true.

### Webview → host

| `type` | Payload | Purpose |
|---|---|---|
| `ready` | _(none)_ | Webview DOM + scripts booted; host may push `workspaceInfo` / cached `mapData`. |
| `analyse` | `path?: string` | Request analysis. Omit `path` to use the workspace folder root. |
| `selectFunction` | `functionId`, `fileId?`, `filePath?`, `functionName?`, `line?`, `byteStart?`, `byteEnd?`, `contentHash?` | User selected a free function. Host opens the read-only Inspection canvas. |
| `selectFile` | `fileId`, `filePath?` | User selected a file card / layer row. |
| `openMapJson` | _(none)_ | Ask host to pick a Horizon Repository `.json` and send `mapData`. |
| `sourceRequest` | `requestId`, `path`, `byteStart`, `byteEnd`, `expectedHash` | Optional Inspector token preview. |

### Host → webview

| `type` | Payload | Purpose |
|---|---|---|
| `mapData` | `map` (Repository JSON), `label?`, or `error?` | Load / clear the map canvas. |
| `analyseResult` | `status`: `running` \| `done` \| `failed` \| `error` \| `idle`; `path?`, `elapsed_ms?`, `error?`, `map?` | Progress + terminal analyse status. |
| `selectFunction` | `functionId` | Host-driven selection on the map. |
| `selectFile` | `fileId` | Host-driven file selection. |
| `workspaceInfo` | `root?`, `name?` | Workspace label / default analyse root. |
| `theme` | `theme`: `light` \| `dark` | Optional theme sync. |
| `sourceResult` | `requestId`, `tokens?` \| `error?`, `message?` | Answer to `sourceRequest`. |

## Media layout

```text
media/
  index.html          # webview shell (`/static/*` rewritten by host)
  bridge.js           # acquireVsCodeApi + protocol helpers
  viewer.js           # adapted desktop viewer (postMessage in IDE; HTTP in preview)
  viewer.css
  diagnostics.js
  function_dag.js
  rail_layout_cases.json
  horizon-activity.svg
```

## Free-function-only UX

The **Types** filter chip stays disabled (type nodes are not in the map contract).
Map / Layers / Inspector / Functions DAG / Diagnostics match the desktop viewer.
