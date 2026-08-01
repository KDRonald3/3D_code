/**
 * Horizon Map webview view provider.
 *
 * Uses W2 `getMapWebviewHtml` (`{{cspSource}}` / `{{media}}` placeholders) when
 * media is present; falls back to a minimal analyse stub otherwise.
 */

import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";
import { openFileReadonly, openInspection } from "./inspection";
import { resolveUnderRoot, workspaceRootFsPath } from "./paths";
import type { HorizonSidecar } from "./sidecar";
import type { MapToggle } from "./toggle";
import {
  getMapWebviewHtml,
  type HostToWebviewMessage,
  type WebviewToHostMessage,
} from "./webviewHost";

export class HorizonMapViewProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "horizon.map.view";

  private view: vscode.WebviewView | undefined;
  private analysing = false;

  constructor(
    private readonly extensionUri: vscode.Uri,
    private readonly sidecar: HorizonSidecar,
    private readonly toggle: MapToggle
  ) {}

  resolveWebviewView(
    webviewView: vscode.WebviewView,
    _context: vscode.WebviewViewResolveContext,
    _token: vscode.CancellationToken
  ): void {
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

    webviewView.webview.onDidReceiveMessage((msg: unknown) => {
      void this.onMessage(msg);
    });

    webviewView.onDidChangeVisibility(() => {
      if (webviewView.visible) {
        this.toggle.markMapShown();
      }
    });
  }

  /** Post a typed message to the webview if it exists. */
  post(message: HostToWebviewMessage): void {
    void this.view?.webview.postMessage(message);
  }

  /** Re-run analyse for the active workspace (command palette / view title). */
  async analyseWorkspace(pathOverride?: string): Promise<void> {
    await this.runAnalyse(pathOverride);
  }

  private async onMessage(raw: unknown): Promise<void> {
    if (!raw || typeof raw !== "object") {
      return;
    }
    const type = (raw as { type?: unknown }).type;
    if (typeof type !== "string") {
      return;
    }
    const msg = raw as WebviewToHostMessage;
    switch (msg.type) {
      case "ready":
        await this.onReady();
        break;
      case "analyse":
        await this.runAnalyse(msg.path);
        break;
      case "selectFunction":
        await openInspection({
          type: "selectFunction",
          functionId: msg.functionId ?? null,
          fileId: msg.fileId ?? null,
          filePath: msg.filePath ?? null,
          line: msg.line ?? null,
          byteStart: msg.byteStart ?? null,
          byteEnd: msg.byteEnd ?? null,
          contentHash: msg.contentHash ?? null,
        });
        break;
      case "selectFile":
        if (msg.filePath) {
          await openFileReadonly(msg.filePath);
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

  private async onReady(): Promise<void> {
    const root = workspaceRootFsPath();
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
    } catch (err) {
      console.debug("[Horizon] no cached map on ready", err);
    }
  }

  private postTheme(): void {
    const kind = vscode.window.activeColorTheme.kind;
    const theme =
      kind === vscode.ColorThemeKind.Dark ||
      kind === vscode.ColorThemeKind.HighContrast
        ? "dark"
        : "light";
    this.post({ type: "theme", theme });
  }

  private async runAnalyse(pathOverride?: string): Promise<void> {
    if (this.analysing) {
      this.post({
        type: "analyseResult",
        status: "running",
        error: "an analysis is already running",
      });
      return;
    }

    const root = workspaceRootFsPath();
    let target = (pathOverride || "").trim() || root;
    if (!target) {
      this.post({
        type: "analyseResult",
        status: "error",
        error: "No workspace folder open.",
      });
      void vscode.window.showErrorMessage(
        "Horizon: open a Rust workspace folder to analyse."
      );
      return;
    }

    if (root) {
      const rootAbs = path.resolve(root);
      const resolved = path.resolve(target);
      if (resolved !== rootAbs && !resolved.startsWith(rootAbs + path.sep)) {
        // Relative paths: try under root.
        const safe = resolveUnderRoot(root, target);
        if (!safe) {
          this.post({
            type: "analyseResult",
            status: "error",
            error: "Analyse path must stay under the workspace folder.",
          });
          return;
        }
        target = safe;
      } else {
        target = resolved;
      }
    } else {
      target = path.resolve(target);
    }

    this.analysing = true;
    try {
      const { map, path: analysed } = await this.sidecar.analyse(
        target,
        (p) => {
          this.post({
            type: "analyseResult",
            status: p.status,
            path: p.path,
            elapsed_ms: p.elapsed_ms,
            error: p.error,
          });
        }
      );
      this.post({ type: "mapData", map, label: analysed });
      this.post({
        type: "analyseResult",
        status: "done",
        path: analysed,
        map,
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      this.post({
        type: "analyseResult",
        status: "failed",
        path: target,
        error: message,
      });
      void vscode.window.showErrorMessage(`Horizon analyse failed: ${message}`);
    } finally {
      this.analysing = false;
    }
  }

  private async openMapJson(): Promise<void> {
    const picked = await vscode.window.showOpenDialog({
      canSelectMany: false,
      filters: { JSON: ["json"] },
      title: "Open Horizon map JSON",
    });
    if (!picked?.[0]) {
      return;
    }
    if (picked[0].scheme !== "file") {
      void vscode.window.showErrorMessage(
        "Horizon: only local map JSON files are supported."
      );
      return;
    }
    const fsPath = picked[0].fsPath;
    try {
      const raw = await fs.promises.readFile(fsPath);
      const map = await this.sidecar.postMap(raw);
      this.post({ type: "mapData", map, label: fsPath });
      this.post({ type: "analyseResult", status: "done", path: fsPath, map });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      this.post({ type: "mapData", map: null, error: message });
      void vscode.window.showErrorMessage(`Horizon: ${message}`);
    }
  }

  private async fulfillSource(
    msg: Extract<WebviewToHostMessage, { type: "sourceRequest" }>
  ): Promise<void> {
    const root = workspaceRootFsPath();
    if (!root) {
      this.post({
        type: "sourceResult",
        requestId: msg.requestId,
        error: "no workspace",
        message: "No workspace folder open.",
      });
      return;
    }
    const safe = resolveUnderRoot(root, msg.path);
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
          tokens: result.tokens as [string, string][],
        });
      } else {
        this.post({
          type: "sourceResult",
          requestId: msg.requestId,
          error: result.errorKind,
          message: result.error,
        });
      }
    } catch (err) {
      this.post({
        type: "sourceResult",
        requestId: msg.requestId,
        error: "error",
        message: err instanceof Error ? err.message : String(err),
      });
    }
  }

  private renderHtml(webview: vscode.Webview): string {
    const indexPath = path.join(this.extensionUri.fsPath, "media", "index.html");
    if (fs.existsSync(indexPath)) {
      try {
        return getMapWebviewHtml(webview, this.extensionUri);
      } catch (err) {
        console.warn("[Horizon] getMapWebviewHtml failed; using stub", err);
      }
    }
    return this.renderFallbackHtml(webview);
  }

  /** Minimal stub when W2 media is not yet present. */
  private renderFallbackHtml(webview: vscode.Webview): string {
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

function getNonce(): string {
  const chars =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  let nonce = "";
  for (let i = 0; i < 32; i++) {
    nonce += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  return nonce;
}
