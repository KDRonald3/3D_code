import * as fs from "fs";
import * as vscode from "vscode";

/**
 * Thin HTML / URI helpers + canonical webview message types (W2).
 *
 * Keep unions aligned with `media/bridge.js` and README.md.
 * W3 imports these from here (via `protocol.ts` / `mapView.ts`).
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
      tokens?: [string, string][] | unknown;
      error?: string;
      errorKind?: string;
      message?: string;
    }
  | { type: "error"; message: string };

/** Loose inbound type used by host message switches. */
export type MapWebviewMessage = WebviewToHostMessage & Record<string, unknown>;

/**
 * Build webview HTML from media/index.html with CSP + asWebviewUri rewrites.
 * Supports `{{cspSource}}` / `{{media}}` placeholders and `/static/*` paths.
 */
export function getMapWebviewHtml(
  webview: vscode.Webview,
  extensionUri: vscode.Uri
): string {
  const mediaRoot = vscode.Uri.joinPath(extensionUri, "media");
  const mediaUri = webview.asWebviewUri(mediaRoot);
  const htmlPath = vscode.Uri.joinPath(mediaRoot, "index.html").fsPath;
  let html = fs.readFileSync(htmlPath, "utf8");

  const csp = [
    `default-src 'none'`,
    `img-src ${webview.cspSource} data:`,
    `style-src ${webview.cspSource} 'unsafe-inline' https://fonts.googleapis.com`,
    `font-src ${webview.cspSource} https://fonts.gstatic.com data:`,
    `script-src ${webview.cspSource}`,
  ].join("; ");

  if (!/Content-Security-Policy/i.test(html)) {
    html = html.replace(
      /<head([^>]*)>/i,
      `<head$1>\n  <meta http-equiv="Content-Security-Policy" content="${csp}" />`
    );
  } else {
    html = html.replace(/\{\{cspSource\}\}/g, webview.cspSource);
  }

  html = html.replace(/\{\{media\}\}/g, mediaUri.toString());
  html = html.replace(
    /(?:src|href)="\/static\/([^"]+)"/g,
    (match, file: string) => {
      const attr = match.startsWith("href") ? "href" : "src";
      return `${attr}="${mediaUri.toString()}/${file}"`;
    }
  );

  return html;
}
