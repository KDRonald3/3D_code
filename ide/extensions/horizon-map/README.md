# Horizon Map

Built-in VS Code / Code-OSS extension that hosts the Horizon **Rust free-function map** webview, adapted from `crates/horizon-server/web/*`.

Inspection canvas + rust-analyzer and sidecar HTTP are owned by **W3 / W4**. This package owns the webview UI and the message protocol surface.

## Commands

| Command | Id |
|---|---|
| Toggle Map ↔ Classic | `horizon.map.toggle` |
| Analyse Workspace | `horizon.map.analyseWorkspace` |

## View

Activity-bar container **Horizon** → webview view `horizon.map.view`.

## Build

```bash
cd ide/extensions/horizon-map
npm install
npm run compile
```

Output: `out/extension.js`.

---

## Webview ↔ extension message protocol

All traffic uses `acquireVsCodeApi().postMessage` / `webview.onDidReceiveMessage`. The webview **must not** call sidecar HTTP (`http://127.0.0.1:…/api/…`) directly.

### Webview → host

| `type` | Payload | Purpose |
|---|---|---|
| `ready` | _(none)_ | Webview DOM + scripts booted; host may push `workspaceInfo` / cached `mapData`. |
| `analyse` | `path?: string` | Request analysis. Omit `path` to use the workspace folder root. |
| `selectFunction` | `functionId`, `fileId?`, `filePath?`, `line?`, `byteStart?`, `byteEnd?`, `contentHash?` | User selected a free function. **W3** opens the read-only Inspection canvas on that range. |
| `selectFile` | `fileId`, `filePath?` | User selected a file card / layer row. **W3** may reveal in classic editor. |
| `openMapJson` | _(none)_ | Ask host to pick a Horizon Repository `.json` and send `mapData`. |
| `sourceRequest` | `requestId`, `path`, `byteStart`, `byteEnd`, `expectedHash` | Optional Inspector token preview. Prefer Inspection canvas; reply with `sourceResult` or ignore (webview times out with a hint). |

### Host → webview

| `type` | Payload | Purpose |
|---|---|---|
| `mapData` | `map` (Repository JSON), `label?`, or `error?` | Load / clear the map canvas. |
| `analyseResult` | `status`: `running` \| `done` \| `failed` \| `error` \| `idle`; `path?`, `elapsed_ms?`, `error?`, `map?` | Progress + terminal analyse status. On `done`, either embed `map` or follow with `mapData`. |
| `selectFunction` | `functionId` | Host-driven selection on the map (no echo back). |
| `selectFile` | `fileId` | Host-driven file selection. |
| `workspaceInfo` | `root?`, `name?` | Workspace label / default analyse root. |
| `theme` | `theme`: `light` \| `dark` | Optional theme sync. |
| `sourceResult` | `requestId`, `tokens?` \| `error?`, `message?` | Answer to `sourceRequest`. |

### Typical flows

**Boot**

1. Webview posts `ready`
2. Host posts `workspaceInfo`
3. Host posts `mapData` if a map is cached; otherwise webview shows **Analyse workspace**

**Analyse**

1. User runs `horizon.map.analyseWorkspace` **or** clicks Analyse workspace → webview posts `analyse`
2. Host runs sidecar (W3/W4) and posts `analyseResult` `{ status: "running", … }`
3. Host posts `analyseResult` `{ status: "done", map }` **or** `{ status: "done" }` then `mapData`

**Inspect function**

1. User selects a function on the map / DAG / diagnostics
2. Webview posts `selectFunction` with path + byte range + hash
3. **W3** opens read-only Inspection editor bound to that range (rust-analyzer)

## W3 host (already in tree)

Host TypeScript under `src/` (sidecar, inspection, toggle, `mapView`) is owned by **W3**. This webview posts the protocol above; W3's `mapView.renderW2Html` rewrites `/static/*` media URLs and injects CSP.

## Media layout

```text
media/
  index.html          # webview shell (`/static/*` rewritten by host)
  bridge.js           # acquireVsCodeApi + protocol helpers
  viewer.js           # adapted desktop viewer (no direct /api fetch in IDE mode)
  viewer.css
  diagnostics.js
  function_dag.js
  rail_layout_cases.json
  horizon-activity.svg
```
