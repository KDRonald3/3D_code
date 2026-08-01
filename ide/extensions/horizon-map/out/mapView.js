"use strict";
/**
 * Horizon Map webview view provider.
 *
 * Uses W2 `getMapWebviewHtml` (`{{cspSource}}` / `{{media}}` placeholders) when
 * media is present; falls back to a minimal analyse stub otherwise.
 */
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.HorizonMapViewProvider = void 0;
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const vscode = __importStar(require("vscode"));
const paths_1 = require("./paths");
const webviewHost_1 = require("./webviewHost");
class HorizonMapViewProvider {
    extensionUri;
    sidecar;
    toggle;
    inspection;
    static viewType = "horizon.map.view";
    view;
    analysing = false;
    constructor(extensionUri, sidecar, toggle, inspection) {
        this.extensionUri = extensionUri;
        this.sidecar = sidecar;
        this.toggle = toggle;
        this.inspection = inspection;
    }
    resolveWebviewView(webviewView, _context, _token) {
        this.view = webviewView;
        this.toggle.markMapShown();
        webviewView.webview.options = {
            enableScripts: true,
            localResourceRoots: [
                vscode.Uri.joinPath(this.extensionUri, "media"),
                vscode.Uri.joinPath(this.extensionUri, "out"),
            ],
        };
        webviewView.webview.html = this.renderHtml(webviewView.webview);
        webviewView.webview.onDidReceiveMessage((msg) => {
            void this.onMessage(msg);
        });
        webviewView.onDidChangeVisibility(() => {
            if (webviewView.visible) {
                this.toggle.markMapShown();
            }
        });
    }
    /** Post a typed message to the webview if it exists. */
    post(message) {
        void this.view?.webview.postMessage(message);
    }
    /** Re-run analyse for the active workspace (command palette / view title). */
    async analyseWorkspace(pathOverride) {
        await this.runAnalyse(pathOverride);
    }
    async onMessage(raw) {
        if (!raw || typeof raw !== "object") {
            return;
        }
        const type = raw.type;
        if (typeof type !== "string") {
            return;
        }
        const msg = raw;
        switch (msg.type) {
            case "ready":
                await this.onReady();
                break;
            case "analyse":
                await this.runAnalyse(msg.path);
                break;
            case "selectFunction":
                await this.inspection.openInspection({
                    type: "selectFunction",
                    functionId: msg.functionId ?? null,
                    fileId: msg.fileId ?? null,
                    filePath: msg.filePath ?? null,
                    functionName: msg.functionName ?? null,
                    line: msg.line ?? null,
                    byteStart: msg.byteStart ?? null,
                    byteEnd: msg.byteEnd ?? null,
                    contentHash: msg.contentHash ?? null,
                });
                break;
            case "selectFile":
                if (msg.filePath) {
                    await this.inspection.openFileReadonly(msg.filePath);
                }
                break;
            case "openMapJson":
                await this.openMapJson();
                break;
            case "sourceRequest":
                await this.fulfillSource(msg);
                break;
        }
    }
    async onReady() {
        const root = (0, paths_1.workspaceRootFsPath)();
        if (root) {
            this.post({
                type: "workspaceInfo",
                root,
                name: path.basename(root),
            });
        }
        this.postTheme();
        try {
            const map = await this.sidecar.getMap();
            if (map) {
                this.post({ type: "mapData", map });
            }
        }
        catch (err) {
            console.debug("[Horizon] no cached map on ready", err);
        }
    }
    postTheme() {
        const kind = vscode.window.activeColorTheme.kind;
        const theme = kind === vscode.ColorThemeKind.Dark ||
            kind === vscode.ColorThemeKind.HighContrast
            ? "dark"
            : "light";
        this.post({ type: "theme", theme });
    }
    async runAnalyse(pathOverride) {
        if (this.analysing) {
            this.post({
                type: "analyseResult",
                status: "running",
                error: "an analysis is already running",
            });
            return;
        }
        const root = (0, paths_1.workspaceRootFsPath)();
        let target = (pathOverride || "").trim() || root;
        if (!target) {
            this.post({
                type: "analyseResult",
                status: "error",
                error: "No workspace folder open.",
            });
            void vscode.window.showErrorMessage("Horizon: open a Rust workspace folder to analyse.");
            return;
        }
        if (root) {
            const rootAbs = path.resolve(root);
            const resolved = path.resolve(target);
            if (resolved !== rootAbs && !resolved.startsWith(rootAbs + path.sep)) {
                // Relative paths: try under root.
                const safe = (0, paths_1.resolveUnderRoot)(root, target);
                if (!safe) {
                    this.post({
                        type: "analyseResult",
                        status: "error",
                        error: "Analyse path must stay under the workspace folder.",
                    });
                    return;
                }
                target = safe;
            }
            else {
                target = resolved;
            }
        }
        else {
            target = path.resolve(target);
        }
        this.analysing = true;
        try {
            const { map, path: analysed } = await this.sidecar.analyse(target, (p) => {
                this.post({
                    type: "analyseResult",
                    status: p.status,
                    path: p.path,
                    elapsed_ms: p.elapsed_ms,
                    error: p.error,
                });
            }, root);
            this.post({ type: "mapData", map, label: analysed });
            this.post({
                type: "analyseResult",
                status: "done",
                path: analysed,
                map,
            });
        }
        catch (err) {
            const message = err instanceof Error ? err.message : String(err);
            this.post({
                type: "analyseResult",
                status: "failed",
                path: target,
                error: message,
            });
            void vscode.window.showErrorMessage(`Horizon analyse failed: ${message}`);
        }
        finally {
            this.analysing = false;
        }
    }
    async openMapJson() {
        const picked = await vscode.window.showOpenDialog({
            canSelectMany: false,
            filters: { JSON: ["json"] },
            title: "Open Horizon map JSON",
        });
        if (!picked?.[0]) {
            return;
        }
        if (picked[0].scheme !== "file") {
            void vscode.window.showErrorMessage("Horizon: only local map JSON files are supported.");
            return;
        }
        const fsPath = picked[0].fsPath;
        try {
            const raw = await fs.promises.readFile(fsPath);
            const map = await this.sidecar.postMap(raw);
            this.post({ type: "mapData", map, label: fsPath });
            this.post({ type: "analyseResult", status: "done", path: fsPath, map });
        }
        catch (err) {
            const message = err instanceof Error ? err.message : String(err);
            this.post({ type: "mapData", map: null, error: message });
            void vscode.window.showErrorMessage(`Horizon: ${message}`);
        }
    }
    async fulfillSource(msg) {
        const root = (0, paths_1.workspaceRootFsPath)();
        if (!root) {
            this.post({
                type: "sourceResult",
                requestId: msg.requestId,
                error: "no workspace",
                message: "No workspace folder open.",
            });
            return;
        }
        const safe = (0, paths_1.resolveUnderRoot)(root, msg.path);
        if (!safe) {
            this.post({
                type: "sourceResult",
                requestId: msg.requestId,
                error: "not_in_map",
                message: "path outside workspace",
            });
            return;
        }
        try {
            const result = await this.sidecar.getSource({
                path: safe,
                byteStart: msg.byteStart,
                byteEnd: msg.byteEnd,
                expectedHash: msg.expectedHash,
            });
            if (result.ok) {
                this.post({
                    type: "sourceResult",
                    requestId: msg.requestId,
                    tokens: result.tokens,
                });
            }
            else {
                this.post({
                    type: "sourceResult",
                    requestId: msg.requestId,
                    error: result.errorKind,
                    message: result.error,
                });
            }
        }
        catch (err) {
            this.post({
                type: "sourceResult",
                requestId: msg.requestId,
                error: "error",
                message: err instanceof Error ? err.message : String(err),
            });
        }
    }
    renderHtml(webview) {
        const indexPath = path.join(this.extensionUri.fsPath, "media", "index.html");
        if (fs.existsSync(indexPath)) {
            try {
                return (0, webviewHost_1.getMapWebviewHtml)(webview, this.extensionUri);
            }
            catch (err) {
                console.warn("[Horizon] getMapWebviewHtml failed; using stub", err);
            }
        }
        return this.renderFallbackHtml(webview);
    }
    /** Minimal stub when W2 media is not yet present. */
    renderFallbackHtml(webview) {
        const nonce = getNonce();
        const csp = [
            `default-src 'none'`,
            `style-src ${webview.cspSource} 'unsafe-inline'`,
            `script-src 'nonce-${nonce}'`,
        ].join("; ");
        return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta http-equiv="Content-Security-Policy" content="${csp}" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Horizon Map</title>
  <style>
    :root { color-scheme: light dark; }
    body {
      margin: 0; padding: 24px;
      font-family: "IBM Plex Sans", "Segoe UI", sans-serif;
      background: radial-gradient(120% 80% at 10% 0%, #1a3038 0%, #0e1619 55%, #0a1012 100%);
      color: #e7eef0; min-height: 100vh; box-sizing: border-box;
    }
    h1 { font-size: 1.4rem; font-weight: 600; margin: 0 0 8px; letter-spacing: 0.02em; }
    p { margin: 0 0 16px; opacity: 0.8; line-height: 1.45; max-width: 36rem; }
    button {
      background: #3d8f7a; color: #04110e; border: 0; border-radius: 4px;
      padding: 8px 14px; font-weight: 600; cursor: pointer;
    }
    #status { margin-top: 16px; font-family: "IBM Plex Mono", monospace; font-size: 12px; opacity: 0.7; }
  </style>
</head>
<body>
  <h1>Horizon Map</h1>
  <p>
    Host stub — W2 map media not found under <code>media/</code>.
    Analyse still runs via the sidecar; results will appear here once the webview lands.
  </p>
  <button id="analyse" type="button">Analyse workspace</button>
  <div id="status">Waiting…</div>
  <script nonce="${nonce}">
    const vscode = acquireVsCodeApi();
    const status = document.getElementById('status');
    document.getElementById('analyse').addEventListener('click', () => {
      status.textContent = 'Analysing…';
      vscode.postMessage({ type: 'analyse' });
    });
    window.addEventListener('message', (event) => {
      const msg = event.data;
      if (!msg || !msg.type) return;
      if (msg.type === 'analyseResult') {
        status.textContent = msg.status + (msg.error ? (': ' + msg.error) : '');
      } else if (msg.type === 'mapData') {
        const crates = msg.map && msg.map.crates ? msg.map.crates.length : '?';
        status.textContent = 'Map ready (' + crates + ' crates). Full UI pending W2 media.';
      } else if (msg.type === 'workspaceInfo') {
        status.textContent = 'Workspace: ' + (msg.root || '');
      }
    });
    vscode.postMessage({ type: 'ready' });
  </script>
</body>
</html>`;
    }
}
exports.HorizonMapViewProvider = HorizonMapViewProvider;
function getNonce() {
    const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let nonce = "";
    for (let i = 0; i < 32; i++) {
        nonce += chars.charAt(Math.floor(Math.random() * chars.length));
    }
    return nonce;
}
//# sourceMappingURL=mapView.js.map