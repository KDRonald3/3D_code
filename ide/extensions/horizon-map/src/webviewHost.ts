import * as fs from "fs";
import * as vscode from "vscode";

/**
 * Thin HTML / URI helpers for the Map webview.
 * Keep protocol types here so W3 can import without digging into media/*.
 *
 * Aligned with `media/bridge.js` and README.md (W2).
 */

/** Messages the webview posts to the extension host. */
export type WebviewToHostMessage =
  | { type: "ready" }
  | { type: "analyse"; path?: string }
  | {
      type: "selectFunction";
      functionId: string | null;
      fileId?: string | null;
      filePath?: string | null;
      line?: number | null;
      byteStart?: number | null;
      byteEnd?: number | null;
      contentHash?: string | null;
    }
  | {
      type: "selectFile";
      fileId: string | null;
      filePath?: string | null;
    }
  | { type: "openMapJson" }
  | {
      type: "sourceRequest";
      requestId: string;
      path: string;
      byteStart: number;
      byteEnd: number;
      expectedHash: string;
    };

/** Messages the extension host posts into the webview. */
export type HostToWebviewMessage =
  | { type: "mapData"; map: unknown; label?: string; error?: string }
  | {
      type: "analyseResult";
      status: "running" | "done" | "failed" | "error" | "idle";
      path?: string;
      label?: string;
      error?: string;
      elapsed_ms?: number;
      /** When status is `done`, host may embed the Repository map here. */
      map?: unknown;
    }
  | { type: "selectFunction"; functionId: string }
  | { type: "selectFile"; fileId: string }
  | { type: "workspaceInfo"; root?: string; name?: string }
  | { type: "theme"; theme: "light" | "dark" }
  | {
      type: "sourceResult";
      requestId: string;
      tokens?: [string, string][];
      error?: string;
      message?: string;
    };

/** Loose inbound type used by message switches. */
export type MapWebviewMessage = WebviewToHostMessage & Record<string, unknown>;

/**
 * Build webview HTML from media/index.html with CSP + asWebviewUri rewrites.
 * Expects `{{cspSource}}` and `{{media}}` placeholders (W2 media contract).
 */
export function getMapWebviewHtml(
  webview: vscode.Webview,
  extensionUri: vscode.Uri
): string {
  const mediaRoot = vscode.Uri.joinPath(extensionUri, "media");
  const mediaUri = webview.asWebviewUri(mediaRoot);
  const indexPath = vscode.Uri.joinPath(mediaRoot, "index.html");

  const htmlPath = indexPath.fsPath;
  let html = fs.readFileSync(htmlPath, "utf8");

  html = html
    .replace(/\{\{cspSource\}\}/g, webview.cspSource)
    .replace(/\{\{media\}\}/g, mediaUri.toString());

  return html;
}
